use anyhow::Result;
use twilight_bus::TwilightBus;
use twilight_proto::twilight::{TwilightEnvelope, AgentPresence, Heartbeat};
use twilight_traffic_controller::{TrafficController, AnalyticsSnapshot};
use futures::StreamExt;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub struct AppState {
    pub bus: Mutex<Option<Arc<TwilightBus>>>,
    pub controller: Arc<TrafficController>,
}

#[tauri::command]
async fn connect_bus(
    tenant: String,
    site: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let bus = TwilightBus::new(&tenant, &site)
        .await
        .map_err(|e| e.to_string())?;
    
    let bus_arc = Arc::new(bus);
    *state.bus.lock().await = Some(Arc::clone(&bus_arc));

    let controller = Arc::clone(&state.controller);

    // Spawn listeners for various topics
    let traffic_bus = Arc::clone(&bus_arc);
    let app_traffic = app.clone();
    let traffic_controller = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = traffic_bus.subscribe_traffic().await {
            while let Some(envelope) = stream.next().await {
                // Record analytics
                traffic_controller.record_traffic(&envelope);
                // Emit for monitor tab
                let _ = app_traffic.emit("bus-traffic", envelope);
            }
        }
    });

    let presence_bus = Arc::clone(&bus_arc);
    let app_presence = app.clone();
    let presence_controller = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = presence_bus.subscribe_presence().await {
            while let Some(presence) = stream.next().await {
                presence_controller.update_presence(presence.clone());
                let _ = app_presence.emit("bus-presence", presence);
            }
        }
    });

    let hb_bus = Arc::clone(&bus_arc);
    let app_hb = app.clone();
    let hb_controller = Arc::clone(&controller);
    tokio::spawn(async move {
        if let Ok(mut stream) = hb_bus.subscribe_heartbeat().await {
            while let Some(hb) = stream.next().await {
                hb_controller.update_heartbeat(hb.clone());
                let _ = app_hb.emit("bus-heartbeat", hb);
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
    
    // Query zenoh admin space
    let mut keys = Vec::new();
    if let Ok(mut replies) = bus.session.get("zenoh/admin/**").await {
        while let Some(reply) = replies.next().await {
            if let Ok(sample) = reply.sample {
                keys.push(sample.key_expression.to_string());
            }
        }
    }
    
    Ok(keys)
}

#[tauri::command]
async fn start_mcp_bridge(port: u16, state: State<'_, AppState>) -> Result<String, String> {
    // In a real implementation, we'd spawn a background task with the MCP server
    // For now, we'll simulate the successful start
    println!("[TAURI] Starting MCP Bridge on port {}", port);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(format!("Bridge started on port {}", port))
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
            connect_bus, 
            get_analytics, 
            get_admin_data,
            start_mcp_bridge
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
