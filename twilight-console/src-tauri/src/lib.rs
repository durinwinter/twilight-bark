use anyhow::Result;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use twilight_bus::TwilightBus;
use twilight_core::{auto_node_id, default_socket_path};
use twilight_mcp_server::DaemonClient;
use twilight_proto::twilight::{AgentPresence, Heartbeat, TwilightEnvelope};
use twilight_traffic_controller::{AgentSnapshot, AnalyticsSnapshot, TrafficController};

// ── App state ─────────────────────────────────────────────────────────────────

pub struct AppState {
    /// Zenoh bus — passive observer of fabric-wide traffic/presence/heartbeats.
    pub bus: Mutex<Option<Arc<TwilightBus>>>,
    /// Local agent registry, populated from Zenoh presence events.
    pub controller: Arc<TrafficController>,
    /// IPC connection to the daemon — the console's own agent identity.
    /// None until connect_daemon is called.
    pub daemon: Mutex<Option<Arc<DaemonClient>>>,
}

// ── Daemon IPC commands ───────────────────────────────────────────────────────

/// Connect to the running daemon as a named human agent.
/// Registers on the fabric, subscribes to incoming tasks, and starts a
/// background loop that forwards task_request / task_result events to the
/// frontend as "fabric:incoming" events.
#[tauri::command]
async fn connect_daemon(
    name: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    let socket = default_socket_path();
    let client = DaemonClient::connect(&socket, &name, "human")
        .await
        .map_err(|e| e.to_string())?;

    let uuid = client.agent_uuid.clone();
    let client = Arc::new(client);
    *state.daemon.lock().await = Some(Arc::clone(&client));

    // Background loop: poll get_pending_tasks every 300 ms, emit to frontend
    let app2 = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let tasks = client.get_pending_tasks().await;
            for task in tasks {
                let _ = app2.emit("fabric:incoming", &task);
            }
        }
    });

    Ok(uuid)
}

/// Send a chat message into the fabric.
/// - target_uuid = None  → broadcast (publish_task)
/// - target_uuid = Some  → direct (ask_agent)
/// Returns the task_id so the frontend can correlate replies.
#[tauri::command]
async fn send_chat(
    target_uuid: Option<String>,
    message: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let guard = state.daemon.lock().await;
    let client = guard.as_ref().ok_or("Not connected to daemon")?;

    let input = serde_json::json!({ "msg": message, "source": "human" }).to_string();

    let task_id = match target_uuid {
        Some(uuid) => client.ask_agent(&uuid, "chat", &input).await,
        None => client.publish_task("chat", &input).await,
    }
    .map_err(|e| e.to_string())?;

    Ok(task_id)
}

/// Reply to an incoming task_request (e.g. an agent asked the human something).
#[tauri::command]
async fn reply_chat(
    task_id: String,
    message: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let guard = state.daemon.lock().await;
    let client = guard.as_ref().ok_or("Not connected to daemon")?;

    let output = serde_json::json!({ "msg": message, "source": "human" }).to_string();
    client
        .reply_task(&task_id, &output, true)
        .await
        .map_err(|e| e.to_string())
}

/// Return the live agent registry from the daemon (authoritative, deduped).
#[tauri::command]
async fn get_fabric_registry(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let guard = state.daemon.lock().await;
    let client = guard.as_ref().ok_or("Not connected to daemon")?;
    client.get_registry().await.map_err(|e| e.to_string())
}

/// Return this console's own agent UUID (empty string if not yet connected).
#[tauri::command]
async fn get_my_uuid(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state
        .daemon
        .lock()
        .await
        .as_ref()
        .map(|c| c.agent_uuid.clone())
        .unwrap_or_default())
}

// ── Existing Zenoh bus commands (unchanged) ───────────────────────────────────

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

#[tauri::command]
fn get_node_id() -> String {
    auto_node_id()
}

#[tauri::command]
async fn start_daemon(role: String, _state: State<'_, AppState>) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config = format!("{}/.config/twilight/daemon.toml", home);

    if !std::path::Path::new(&config).exists() {
        return Err(format!(
            "No daemon config at {config}. Run 'twilight-cli daemon enroll' first."
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
        let status = if std::path::Path::new(&jwt_path).exists() {
            "jwt ready".to_string()
        } else {
            "awaiting jwt".to_string()
        };
        results.push((node_id, status));
    }
    Ok(results)
}

#[tauri::command]
async fn get_daemon_status() -> Result<serde_json::Value, String> {
    let pid_path = default_socket_path().with_extension("pid");
    let socket_path = default_socket_path();

    let pid: Option<u32> = std::fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    let process_alive = pid
        .map(|p| {
            std::process::Command::new("kill")
                .args(["-0", &p.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let socket_ok = tokio::net::UnixStream::connect(&socket_path).await.is_ok();

    Ok(serde_json::json!({
        "running": process_alive && socket_ok,
        "pid": pid,
        "socket": socket_path.to_string_lossy().as_ref(),
    }))
}

#[tauri::command]
async fn get_fabric_agents(state: State<'_, AppState>) -> Result<Vec<AgentSnapshot>, String> {
    Ok(state.controller.get_registry_snapshot())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn find_twilight_binary(name: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let p = format!("{home}/.cargo/bin/{name}");
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            if p.exists() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    name.to_string()
}

fn find_provision_script() -> String {
    if let Ok(exe) = std::env::current_exe() {
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

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            bus: Mutex::new(None),
            controller: Arc::new(TrafficController::new()),
            daemon: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            // daemon IPC (human agent)
            connect_daemon,
            send_chat,
            reply_chat,
            get_fabric_registry,
            get_my_uuid,
            // daemon lifecycle
            get_daemon_status,
            start_daemon,
            stop_daemon,
            enroll_identity,
            // zenoh bus (observer / analytics)
            get_node_id,
            connect_bus,
            get_analytics,
            get_admin_data,
            get_fabric_agents,
            // provisioning
            provision_network,
            generate_identities,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
