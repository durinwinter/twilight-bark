use anyhow::Result;
use futures::StreamExt;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use twilight_bus::TwilightBus;
use twilight_core::{auto_node_id, default_socket_path};
use twilight_proto::twilight::{AgentPresence, Heartbeat, TwilightEnvelope};
use twilight_traffic_controller::{AgentSnapshot, AnalyticsSnapshot, TrafficController};

pub struct AppState {
    pub bus: Mutex<Option<Arc<TwilightBus>>>,
    pub controller: Arc<TrafficController>,
}

#[tauri::command]
async fn connect_bus(
    tenant: String,
    node_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let bus = TwilightBus::new(&tenant, &node_id)
        .await
        .map_err(|e| format!("Bus connection failed: {:?}", e))?;

    let bus_arc = Arc::new(bus);
    *state.bus.lock().await = Some(Arc::clone(&bus_arc));

    let controller = Arc::clone(&state.controller);

    // Traffic — node-local (console is an observer on the same node)
    let traffic_bus = Arc::clone(&bus_arc);
    let app_traffic = app.clone();
    let traffic_ctrl = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = traffic_bus.subscribe_traffic().await {
            while let Some(env) = stream.next().await {
                let e: TwilightEnvelope = env;
                traffic_ctrl.record_traffic(&e);
                let _ = app_traffic.emit("bus-traffic", e);
            }
        }
    });

    // Presence — cross-node wildcard so the console shows ALL agents in the fabric
    let presence_bus = Arc::clone(&bus_arc);
    let app_presence = app.clone();
    let presence_ctrl = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = presence_bus.subscribe_all_presence().await {
            while let Some(pres) = stream.next().await {
                let p: AgentPresence = pres;
                presence_ctrl.update_presence(p.clone());
                let _ = app_presence.emit("bus-presence", p);
            }
        }
    });

    // Heartbeats — cross-node wildcard
    let hb_bus = Arc::clone(&bus_arc);
    let app_hb = app.clone();
    let hb_ctrl = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = hb_bus.subscribe_all_heartbeats().await {
            while let Some(hb) = stream.next().await {
                let h: Heartbeat = hb;
                hb_ctrl.update_heartbeat(h.clone());
                let _ = app_hb.emit("bus-heartbeat", h);
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn get_analytics(state: State<'_, AppState>) -> Result<AnalyticsSnapshot, String> {
    Ok(state.controller.get_analytics_snapshot())
}

#[tauri::command]
async fn get_admin_data(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let bus_guard = state.bus.lock().await;
    let bus = bus_guard.as_ref().ok_or("Bus not connected")?;

    let mut keys = Vec::new();
    if let Ok(replies) = bus.session.get("zenoh/admin/**").await {
        while let Ok(reply) = replies.recv_async().await {
            if let Ok(sample) = reply.result() {
                keys.push(sample.key_expr().to_string());
            }
        }
    }
    Ok(keys)
}

/// Returns the local node_id ("{hostname}-{username}") for use in connect_bus.
#[tauri::command]
fn get_node_id() -> String {
    auto_node_id()
}

/// Start the twilight-daemon using the default config path.
/// The daemon writes its own PID file — no need to track it here.
#[tauri::command]
async fn start_daemon(role: String, _state: State<'_, AppState>) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config = format!("{}/.config/twilight/daemon.toml", home);

    if !std::path::Path::new(&config).exists() {
        return Err(format!(
            "No daemon config at {config}. Run 'twilight-cli daemon enroll' and create the config first."
        ));
    }

    let binary = find_twilight_binary("twilight-daemon");
    std::process::Command::new(&binary)
        .arg("--config")
        .arg(&config)
        .spawn()
        .map_err(|e| format!("Failed to spawn {binary}: {e}"))?;

    Ok(format!("Daemon starting ({role} mode) with config {config}"))
}

/// Stop the daemon by sending SIGTERM to the PID recorded in its PID file.
#[tauri::command]
async fn stop_daemon(_state: State<'_, AppState>) -> Result<String, String> {
    let pid_path = default_socket_path().with_extension("pid");
    let pid = std::fs::read_to_string(&pid_path)
        .map_err(|_| format!("No PID file at {:?}. Is the daemon running?", pid_path))?;
    let pid = pid.trim().to_string();

    std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .map_err(|e| format!("kill failed: {e}"))?;

    let _ = std::fs::remove_file(&pid_path);
    Ok(format!("Sent SIGTERM to daemon (pid={pid})"))
}

/// Enroll a Ziti identity from a JWT file.
/// The path argument is the JWT file path. Output defaults to ~/.config/twilight/identity.json.
#[tauri::command]
async fn enroll_identity(path: String, _state: State<'_, AppState>) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let out = std::path::PathBuf::from(format!("{}/.config/twilight/identity.json", home));
    twilight_ziti::enroll("ziti", std::path::Path::new(&path), &out)
        .await
        .map(|_| format!("Identity enrolled → {:?}", out))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn provision_network(
    name: String,
    controller_url: String,
    _state: State<'_, AppState>,
) -> Result<String, String> {
    // Shells out to the provision-fabric.sh script
    let script = find_provision_script();
    if script.is_empty() {
        return Err("Cannot find scripts/provision-fabric.sh".to_string());
    }
    let output = std::process::Command::new("bash")
        .args(["-c", &format!("ZITI_CTRL_URL={controller_url} {script} 2>&1")])
        .output()
        .map_err(|e| format!("Script failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(format!("Provisioned network '{name}': {}", &stdout[..stdout.len().min(200)]))
}

#[tauri::command]
async fn generate_identities(
    count: u32,
    _state: State<'_, AppState>,
) -> Result<Vec<(String, String)>, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let enrollments_dir = format!("{home}/.config/twilight/enrollments");
    std::fs::create_dir_all(&enrollments_dir).map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for i in 0..count {
        let node_id = format!("node-{:03}", i + 1);
        let jwt_path = format!("{enrollments_dir}/{node_id}.jwt");
        // Slot pre-created — admin runs provision-fabric.sh --add-node <node_id> to populate JWT.
        let status = if std::path::Path::new(&jwt_path).exists() {
            "jwt ready".to_string()
        } else {
            "awaiting jwt".to_string()
        };
        results.push((node_id, status));
    }
    Ok(results)
}

/// Returns daemon process status: running, pid, socket path.
#[tauri::command]
async fn get_daemon_status() -> Result<serde_json::Value, String> {
    let pid_path = default_socket_path().with_extension("pid");
    let socket_path = default_socket_path();

    let pid: Option<u32> = std::fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    let process_alive = pid.map(|p| {
        std::process::Command::new("kill")
            .args(["-0", &p.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }).unwrap_or(false);

    let socket_ok = tokio::net::UnixStream::connect(&socket_path).await.is_ok();

    Ok(serde_json::json!({
        "running": process_alive && socket_ok,
        "pid": pid,
        "socket": socket_path.to_string_lossy().as_ref(),
    }))
}

/// Returns a snapshot of all agents in the local registry (populated from bus presence events).
#[tauri::command]
async fn get_fabric_agents(state: State<'_, AppState>) -> Result<Vec<AgentSnapshot>, String> {
    Ok(state.controller.get_registry_snapshot())
}

fn find_twilight_binary(name: &str) -> String {
    // 1. ~/.cargo/bin (installed by install-service.sh)
    if let Ok(home) = std::env::var("HOME") {
        let p = format!("{home}/.cargo/bin/{name}");
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    // 2. Next to the current binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            if p.exists() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    // 3. PATH
    name.to_string()
}

fn find_provision_script() -> String {
    if let Ok(exe) = std::env::current_exe() {
        // Walk up to find the repo root (looks for scripts/provision-fabric.sh)
        let mut dir = exe.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        while let Some(d) = dir {
            let script = d.join("scripts/provision-fabric.sh");
            if script.exists() {
                return script.to_string_lossy().to_string();
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    String::new()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            bus: Mutex::new(None),
            controller: Arc::new(TrafficController::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_node_id,
            connect_bus,
            get_analytics,
            get_admin_data,
            get_daemon_status,
            get_fabric_agents,
            start_daemon,
            stop_daemon,
            enroll_identity,
            provision_network,
            generate_identities,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
