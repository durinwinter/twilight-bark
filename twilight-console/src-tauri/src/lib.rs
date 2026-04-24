use anyhow::Result;
use twilight_bus::TwilightBus;
use twilight_proto::twilight::{TwilightEnvelope, AgentPresence, Heartbeat};
use futures::StreamExt;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub struct AppState {
    pub bus: Mutex<Option<Arc<TwilightBus>>>,
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

    // Spawn listeners for various topics
    let traffic_bus = Arc::clone(&bus_arc);
    let app_traffic = app.clone();
    tokio::spawn(async move {
        if let Ok(mut stream) = traffic_bus.subscribe_traffic().await {
            while let Some(envelope) = stream.next().await {
                let _ = app_traffic.emit("bus-traffic", envelope);
            }
        }
    });

    let presence_bus = Arc::clone(&bus_arc);
    let app_presence = app.clone();
    tokio::spawn(async move {
        if let Ok(mut stream) = presence_bus.subscribe_presence().await {
            while let Some(presence) = stream.next().await {
                let _ = app_presence.emit("bus-presence", presence);
            }
        }
    });

    let hb_bus = Arc::clone(&bus_arc);
    let app_hb = app.clone();
    tokio::spawn(async move {
        if let Ok(mut stream) = hb_bus.subscribe_heartbeat().await {
            while let Some(hb) = stream.next().await {
                let _ = app_hb.emit("bus-heartbeat", hb);
            }
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            bus: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![connect_bus])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
