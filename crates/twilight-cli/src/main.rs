use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use twilight_bus::TwilightBus;
use twilight_core::{create_default_identity, create_presence};
use twilight_mcp_server::TwilightMcpServer;
use twilight_proto::twilight::{
    twilight_envelope::Payload, AgentStatus, Heartbeat, MessageKind, MessageTarget, TargetKind,
    TaskRequest, TaskResult, TwilightEnvelope,
};
use twilight_traffic_controller::TrafficController;
use uuid::Uuid;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a node and run a heartbeat loop
    Run {
        #[arg(short, long, default_value = "twilight-cli")]
        name: String,
    },
    /// List all agents currently active on the fabric
    List,
    /// Run the two-agent round-trip smoke test and exit
    SmokeTest,
    /// Inject a manual task request into the fabric
    Inject {
        #[arg(short, long)]
        operation: String,
        #[arg(short, long)]
        input: String,
        #[arg(short, long)]
        target_uuid: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Run { name } => {
            let bus = Arc::new(TwilightBus::new("default", "local").await?);
            let identity = create_default_identity(name, "cli");
            let presence = create_presence(identity.clone(), AgentStatus::Online);

            println!("Starting node: {} ({})", name, identity.node_uuid);
            bus.publish_presence(&presence).await?;

            let controller = Arc::new(TrafficController::new());
            
            // Wire in heartbeat updates
            let hb_bus = Arc::clone(&bus);
            let hb_controller = Arc::clone(&controller);
            tokio::spawn(async move {
                let mut hb_stream = hb_bus.subscribe_heartbeat().await.unwrap();
                while let Some(hb) = hb_stream.next().await {
                   hb_controller.update_heartbeat(hb);
                }
            });

            // Start cleanup loop
            let cleanup_controller = Arc::clone(&controller);
            tokio::spawn(cleanup_controller.run_cleanup_loop(5000, 30));

            let hb_bus = Arc::clone(&bus);
            let node_id = identity.node_uuid.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    let hb = Heartbeat {
                        node_id: node_id.clone(),
                        status: AgentStatus::Online as i32,
                        timestamp_unix_ms: Utc::now().timestamp_millis(),
                        active_tasks: 0,
                        queued_tasks: 0,
                    };
                    if let Err(e) = hb_bus.publish_heartbeat(&hb).await {
                        log::warn!("Heartbeat failed: {e}");
                    }
                }
            });

            tokio::signal::ctrl_c().await?;
            println!("Shutting down.");
            heartbeat_task.abort();
        }

        Commands::List => {
            println!("Scanning fabric for agents (5s)...");
            let bus = Arc::new(TwilightBus::new("default", "local").await?);
            let mut presence_stream = bus.subscribe_presence().await?;
            
            let mut agents = std::collections::HashMap::new();
            
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(presence) = presence_stream.next().await {
                    if let Some(id) = presence.identity {
                        agents.insert(id.node_uuid.clone(), id);
                    }
                }
            }).await;

            if agents.is_empty() {
                println!("No agents discovered.");
            } else {
                println!("{:<36} | {:<20} | {:<15}", "UUID", "Name", "Roles");
                println!("{:-<36}-+-{:-<20}-+-{:-<15}", "", "", "");
                for (uuid, id) in agents {
                    println!("{:<36} | {:<20} | {:<15?}", uuid, id.agent_name, id.roles);
                }
            }
        }

        Commands::SmokeTest => {
            if let Err(e) = run_smoke_test().await {
                eprintln!("[SMOKE TEST] FAIL: {e}");
                std::process::exit(1);
            }
        }

        Commands::Inject { operation, input, target_uuid } => {
            let bus = Arc::new(TwilightBus::new("default", "local").await?);
            let mut target = MessageTarget::default();
            if let Some(uuid) = target_uuid {
                target.set_target_kind(TargetKind::Unicast);
                target.target_agent_uuids.push(uuid.to_string());
            } else {
                target.set_target_kind(TargetKind::Broadcast);
            }

            let task_id = Uuid::new_v4().to_string();
            let envelope = TwilightEnvelope {
                message_uuid: Uuid::new_v4().to_string(),
                correlation_uuid: task_id.clone(),
                causation_uuid: String::new(),
                source: Some(create_default_identity("cli-injector", "operator")),
                target: Some(target),
                kind: MessageKind::TaskRequest as i32,
                priority: 2,
                created_unix_ms: Utc::now().timestamp_millis(),
                expires_unix_ms: Utc::now().timestamp_millis() + 60000,
                tags: vec!["manual-injection".to_string()],
                payload: Some(Payload::TaskRequest(TaskRequest {
                    task_id: task_id.clone(),
                    operation: operation.clone(),
                    input_json: input.clone(),
                    timeout_ms: 30000,
                })),
            };

            println!("Injecting task {} (operation: {})", task_id, operation);
            bus.publish_envelope(&envelope).await?;
            println!("Task injected successfully.");
        }
    }

    Ok(())
}

async fn run_smoke_test() -> Result<()> {
    let sender_bus = Arc::new(TwilightBus::new("default", "local").await?);
    let receiver_bus = Arc::new(TwilightBus::new("default", "local").await?);

    let mut receiver_stream = receiver_bus.subscribe_traffic().await?;
    let mut result_stream = sender_bus.subscribe_traffic().await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let controller = Arc::new(TrafficController::new());
    let mcp = TwilightMcpServer::new(Arc::clone(&sender_bus), controller);

    let mut target = MessageTarget::default();
    target.set_target_kind(TargetKind::Broadcast);

    let task_id = mcp.publish_task("smoke-test", r#"{"ping":true}"#, target).await?;

    let echo_id = task_id.clone();
    let rb = Arc::clone(&receiver_bus);
    let receiver_task = tokio::spawn(async move {
        while let Some(envelope) = receiver_stream.next().await {
            if envelope.kind != MessageKind::TaskRequest as i32 {
                continue;
            }
            if let Some(Payload::TaskRequest(ref req)) = envelope.payload {
                if req.task_id == echo_id {
                    let reply = TwilightEnvelope {
                        message_uuid: Uuid::new_v4().to_string(),
                        correlation_uuid: echo_id.clone(),
                        causation_uuid: envelope.message_uuid.clone(),
                        source: None,
                        target: None,
                        kind: MessageKind::TaskResult as i32,
                        priority: 2,
                        created_unix_ms: Utc::now().timestamp_millis(),
                        expires_unix_ms: Utc::now().timestamp_millis() + 30_000,
                        tags: Vec::new(),
                        payload: Some(Payload::TaskResult(TaskResult {
                            task_id: req.task_id.clone(),
                            output_json: r#"{"pong":true}"#.to_string(),
                            success: true,
                            error_message: String::new(),
                        })),
                    };
                    let _ = rb.publish_envelope(&reply).await;
                    return;
                }
            }
        }
    });

    let expected = task_id.clone();
    let outcome = tokio::time::timeout(Duration::from_secs(5), async move {
        while let Some(envelope) = result_stream.next().await {
            if envelope.kind == MessageKind::TaskResult as i32
                && envelope.correlation_uuid == expected
            {
                return true;
            }
        }
        false
    })
    .await;

    receiver_task.abort();

    match outcome {
        Ok(true) => {
            println!("[SMOKE TEST] PASS");
            Ok(())
        }
        Ok(false) => Err(anyhow::anyhow!("stream ended before TaskResult arrived")),
        Err(_) => Err(anyhow::anyhow!("timed out after 5s waiting for TaskResult")),
    }
}
