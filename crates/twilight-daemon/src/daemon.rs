use crate::config::{DaemonConfig, NodeRole};
use crate::ipc::IpcServer;
use crate::process::ManagedProcess;
use anyhow::Result;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use twilight_bus::TwilightBus;
use twilight_core::{create_node_identity, create_presence};
use twilight_proto::twilight::AgentStatus;
use twilight_traffic_controller::TrafficController;
use twilight_ziti::ZitiTunnel;
use log::info;

pub struct TwilightDaemon {
    pid_path: PathBuf,
    ziti_tunnel: Option<ManagedProcess>,
    _bus: Arc<TwilightBus>,
}

impl TwilightDaemon {
    pub async fn start(config: DaemonConfig) -> Result<Self> {
        let node_id = config.node_id().to_string();
        let tenant = config.zenoh.tenant.clone();
        let socket_path = config.daemon.resolved_socket();
        let pid_path = socket_path.with_extension("pid");

        // 1. Ziti tunnel sidecar — client nodes dial the zenoh-router service
        let ziti_tunnel = if config.ziti.enabled && config.node.role == NodeRole::Client {
            let zt = ZitiTunnel {
                binary: config.ziti.binary.clone(),
                identity_file: config.identity.file.clone(),
                service: config.ziti.service.clone(),
                local_port: config.ziti.local_port,
            };
            let (prog, args) = zt.build_args();
            let mut proc = ManagedProcess::new("ziti-tunnel", prog, args);
            proc.start().await?;
            proc.wait_port_ready(config.ziti.local_port, Duration::from_secs(15)).await?;
            info!("Ziti tunnel ready on port {}", config.ziti.local_port);
            Some(proc)
        } else {
            None
        };

        // 2. Zenoh session
        let zenoh_json = build_zenoh_config(&config);
        let bus = Arc::new(TwilightBus::new_with_config(zenoh_json, &tenant, &node_id).await?);

        // 3. Traffic controller + cross-node subscriptions
        let controller = Arc::new(TrafficController::new());
        {
            let bus2 = Arc::clone(&bus);
            let ctrl = Arc::clone(&controller);
            tokio::spawn(async move {
                match bus2.subscribe_all_presence().await {
                    Ok(mut s) => while let Some(p) = s.next().await { ctrl.update_presence(p); },
                    Err(e) => log::error!("Presence subscription failed: {e}"),
                }
            });
        }
        {
            let bus2 = Arc::clone(&bus);
            let ctrl = Arc::clone(&controller);
            tokio::spawn(async move {
                match bus2.subscribe_all_heartbeats().await {
                    Ok(mut s) => while let Some(hb) = s.next().await { ctrl.update_heartbeat(hb); },
                    Err(e) => log::error!("Heartbeat subscription failed: {e}"),
                }
            });
        }
        tokio::spawn(Arc::clone(&controller).run_cleanup_loop(5_000, 30));

        // 4. IPC server
        let ipc = Arc::new(IpcServer::new(
            socket_path.clone(),
            Arc::clone(&bus),
            Arc::clone(&controller),
            node_id.clone(),
            tenant.clone(),
        ));
        tokio::spawn(Arc::clone(&ipc).run());

        // 5. Own presence + heartbeat
        let identity = create_node_identity("twilight-daemon", "daemon", &node_id, &tenant);
        let presence = create_presence(identity.clone(), AgentStatus::Online);
        bus.publish_presence(&presence).await?;
        Arc::clone(&bus).start_heartbeat_loop(identity.node_uuid, 10);

        // 6. PID file — lets CLI daemon stop/status find us
        std::fs::write(&pid_path, std::process::id().to_string())
            .map_err(|e| anyhow::anyhow!("Cannot write PID file {:?}: {e}", pid_path))?;

        info!(
            "Daemon ready  node_id={node_id}  tenant={tenant}  socket={:?}  pid={}",
            socket_path,
            std::process::id()
        );

        Ok(Self { pid_path, ziti_tunnel, _bus: bus })
    }

    pub async fn run_until_signal(&mut self) -> Result<()> {
        wait_for_shutdown_signal().await;
        info!("Shutdown signal received, cleaning up...");

        if let Some(t) = &mut self.ziti_tunnel {
            t.stop().await;
        }
        let _ = std::fs::remove_file(&self.pid_path);
        Ok(())
    }
}

fn build_zenoh_config(config: &DaemonConfig) -> serde_json::Value {
    match config.node.role {
        NodeRole::Hub => {
            let listen = if config.zenoh.listen.is_empty() {
                vec!["tcp/0.0.0.0:7447".to_string()]
            } else {
                config.zenoh.listen.clone()
            };
            serde_json::json!({ "mode": "router", "listen": { "endpoints": listen } })
        }
        NodeRole::Client => {
            serde_json::json!({
                "mode": "client",
                "connect": { "endpoints": config.zenoh.peers }
            })
        }
    }
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }
}
