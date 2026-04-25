use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use twilight_bus::TwilightBus;
use twilight_core::{create_default_identity, create_presence, default_socket_path};
use twilight_proto::twilight::{
    twilight_envelope::Payload, AgentStatus, MessageKind, MessageTarget, TargetKind,
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
    /// Run a live A2A coordination scenario (Observer & Worker)
    ScenarioA2a,
    /// Run the 4-dog multi-provider demonstration (Claude & LM Studio nodes)
    ScenarioDogs,
    /// Run the Twilight-to-MCP bridge server
    McpServer {
        #[arg(short, long, default_value_t = 7447)]
        port: u16,
    },
    /// Inject a manual task request into the fabric
    Inject {
        #[arg(short, long)]
        operation: String,
        #[arg(short, long)]
        input: String,
        #[arg(short, long)]
        target_uuid: Option<String>,
    },
    /// Manage the Twilight Bark daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Enroll this node with the Ziti controller using an enrollment JWT
    Enroll {
        /// Path to the enrollment JWT file (provided by the hub admin)
        #[arg(long)]
        jwt: PathBuf,
        /// Output path for the generated identity.json
        #[arg(long)]
        out: Option<PathBuf>,
        /// Path or name of the ziti CLI binary
        #[arg(long, default_value = "ziti")]
        binary: String,
    },
    /// Start the daemon in the background
    Start {
        /// Path to daemon.toml config file
        #[arg(long, env = "TWILIGHT_CONFIG")]
        config: PathBuf,
    },
    /// Stop the running daemon
    Stop {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Show daemon process and socket status
    Status {
        #[arg(long)]
        socket: Option<PathBuf>,
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
            
            let hb_bus = Arc::clone(&bus);
            let hb_controller = Arc::clone(&controller);
            tokio::spawn(async move {
                let mut hb_stream = hb_bus.subscribe_heartbeat().await.unwrap();
                while let Some(hb) = hb_stream.next().await {
                   hb_controller.update_heartbeat(hb);
                }
            });

            tokio::spawn(Arc::clone(&controller).run_cleanup_loop(5000, 30));

            let hb_task = Arc::clone(&bus).start_heartbeat_loop(identity.node_uuid.clone(), 10);

            tokio::signal::ctrl_c().await?;
            println!("Shutting down.");
            hb_task.abort();
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

        Commands::ScenarioA2a => {
            run_scenario_a2a().await?;
        }

        Commands::ScenarioDogs => {
            run_scenario_dogs().await?;
        }

        Commands::McpServer { port } => {
            run_mcp_server(*port).await?;
        }

        Commands::Daemon { command } => {
            run_daemon_command(command).await?;
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

async fn run_scenario_a2a() -> Result<()> {
    println!("--- [SCENARIO: A2A Coordination] ---");
    
    let observer_bus = Arc::new(TwilightBus::new("default", "local").await?);
    let worker_bus = Arc::new(TwilightBus::new("default", "local").await?);

    let observer_id = create_default_identity("observer-agent", "scout");
    let worker_id = create_default_identity("worker-agent", "processor");

    println!("[OBSERVER] {} started.", observer_id.node_uuid);
    println!("[WORKER]   {} started.", worker_id.node_uuid);

    observer_bus.publish_presence(&create_presence(observer_id.clone(), AgentStatus::Online)).await?;
    worker_bus.publish_presence(&create_presence(worker_id.clone(), AgentStatus::Online)).await?;

    let _hb1 = Arc::clone(&observer_bus).start_heartbeat_loop(observer_id.node_uuid.clone(), 5);
    let _hb2 = Arc::clone(&worker_bus).start_heartbeat_loop(worker_id.node_uuid.clone(), 5);

    let mut worker_stream = worker_bus.subscribe_traffic().await?;
    let mut observer_stream = observer_bus.subscribe_traffic().await?;

    // Worker Logic: Listen for tasks and reply
    let wb = Arc::clone(&worker_bus);
    let w_id = worker_id.clone();
    tokio::spawn(async move {
        while let Some(envelope) = worker_stream.next().await {
            if let Some(Payload::TaskRequest(req)) = envelope.payload {
                println!("[WORKER] Received task: {} (operation: {})", req.task_id, req.operation);
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                let result = TwilightEnvelope {
                    message_uuid: Uuid::new_v4().to_string(),
                    correlation_uuid: req.task_id.clone(),
                    causation_uuid: envelope.message_uuid.clone(),
                    source: Some(w_id.clone()),
                    target: envelope.source.as_ref().map(|s| {
                        let mut t = MessageTarget::default();
                        t.set_target_kind(TargetKind::Unicast);
                        t.target_agent_uuids.push(s.node_uuid.clone());
                        t
                    }),
                    kind: MessageKind::TaskResult as i32,
                    priority: 2,
                    created_unix_ms: Utc::now().timestamp_millis(),
                    expires_unix_ms: Utc::now().timestamp_millis() + 30000,
                    tags: vec!["scenario-result".to_string()],
                    payload: Some(Payload::TaskResult(TaskResult {
                        task_id: req.task_id.clone(),
                        output_json: r#"{"status": "success", "files_scanned": 42}"#.to_string(),
                        success: true,
                        error_message: String::new(),
                    })),
                };
                let _ = wb.publish_envelope(&result).await;
                println!("[WORKER] Task result sent.");
            }
        }
    });

    // Observer Logic: Wait for worker, then task it
    println!("[OBSERVER] Waiting for Worker discovery...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let task_id = Uuid::new_v4().to_string();
    let mut target = MessageTarget::default();
    target.set_target_kind(TargetKind::Unicast);
    target.target_agent_uuids.push(worker_id.node_uuid.clone());

    let request = TwilightEnvelope {
        message_uuid: Uuid::new_v4().to_string(),
        correlation_uuid: task_id.clone(),
        causation_uuid: String::new(),
        source: Some(observer_id.clone()),
        target: Some(target),
        kind: MessageKind::TaskRequest as i32,
        priority: 2,
        created_unix_ms: Utc::now().timestamp_millis(),
        expires_unix_ms: Utc::now().timestamp_millis() + 30000,
        tags: vec!["scenario-task".to_string()],
        payload: Some(Payload::TaskRequest(TaskRequest {
            task_id: task_id.clone(),
            operation: "analyze_vault".to_string(),
            input_json: r#"{"vault": "main"}"#.to_string(),
            timeout_ms: 10000,
        })),
    };

    println!("[OBSERVER] Sending 'analyze_vault' task to Worker...");
    observer_bus.publish_envelope(&request).await?;

    // Wait for result
    println!("[OBSERVER] Waiting for result...");
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(envelope) = observer_stream.next().await {
            if let Some(Payload::TaskResult(res)) = envelope.payload {
                if res.task_id == task_id {
                    return Some(res.output_json);
                }
            }
        }
        None
    }).await;

    match outcome {
        Ok(Some(json)) => println!("[OBSERVER] Task SUCCESS! Result: {}", json),
        Ok(None) => println!("[OBSERVER] FAIL: Stream ended without result."),
        Err(_) => println!("[OBSERVER] FAIL: Timed out waiting for result."),
    }

    println!("\nScenario complete. Press Ctrl+C to exit (keeping heartbeats active for Console monitoring).");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn run_scenario_dogs() -> Result<()> {
    println!("--- [SCENARIO: Multi-Dog / Multi-Provider] ---");
    
    // Virtual Providers
    let providers = ["claude-node", "lms-node"];
    let dogs = [
        ("beagle", "scout", 0),
        ("terrier", "processor", 0),
        ("husky", "scout", 1),
        ("boxer", "processor", 1),
    ];

    let mut buses = Vec::new();
    let mut tasks = Vec::new();

    for (name, role, p_idx) in dogs {
        let bus = Arc::new(TwilightBus::new("default", providers[p_idx]).await?);
        let identity = create_default_identity(&format!("{}-{}", providers[p_idx], name), role);
        
        println!("[{}] Agent {}-{} joining the fabric.", providers[p_idx].to_uppercase(), name, role);
        
        bus.publish_presence(&create_presence(identity.clone(), AgentStatus::Online)).await?;
        
        // Start independent heartbeats
        let hb_task = Arc::clone(&bus).start_heartbeat_loop(identity.node_uuid.clone(), 5);
        tasks.push(hb_task);
        
        // Listener for traffic
        let mut traffic_stream = bus.subscribe_traffic().await?;
        let b = Arc::clone(&bus);
        let id_clone = identity.clone();
        
        tokio::spawn(async move {
            while let Some(envelope) = traffic_stream.next().await {
                if let Some(Payload::TaskRequest(req)) = envelope.payload {
                    println!("[{}] Received task: {}", id_clone.agent_name, req.operation);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    
                    let reply = TwilightEnvelope {
                        message_uuid: Uuid::new_v4().to_string(),
                        correlation_uuid: req.task_id.clone(),
                        causation_uuid: envelope.message_uuid.clone(),
                        source: Some(id_clone.clone()),
                        target: envelope.source.as_ref().map(|s| {
                            let mut t = MessageTarget::default();
                            t.set_target_kind(TargetKind::Unicast);
                            t.target_agent_uuids.push(s.node_uuid.clone());
                            t
                        }),
                        kind: MessageKind::TaskResult as i32,
                        priority: 2,
                        created_unix_ms: Utc::now().timestamp_millis(),
                        expires_unix_ms: Utc::now().timestamp_millis() + 30000,
                        tags: vec!["dog-scenario".to_string()],
                        payload: Some(Payload::TaskResult(TaskResult {
                            task_id: req.task_id.clone(),
                            output_json: r#"{"status": "barked"}"#.to_string(),
                            success: true,
                            error_message: String::new(),
                        })),
                    };
                    let _ = b.publish_envelope(&reply).await;
                }
            }
        });

        buses.push((bus, identity));
    }

    println!("\n[FABRIC] 4 Dogs registered and heartbeating.");
    println!("[FABRIC] Simulating cross-provider bark-exchange...");

    // Beagle (Claude) tasks Boxer (LMS)
    let beagle = &buses[0];
    let boxer = &buses[3];
    
    let task_id = Uuid::new_v4().to_string();
    let mut target = MessageTarget::default();
    target.set_target_kind(TargetKind::Unicast);
    target.target_agent_uuids.push(boxer.1.node_uuid.clone());

    let req = TwilightEnvelope {
        message_uuid: Uuid::new_v4().to_string(),
        correlation_uuid: task_id.clone(),
        causation_uuid: String::new(),
        source: Some(beagle.1.clone()),
        target: Some(target),
        kind: MessageKind::TaskRequest as i32,
        priority: 2,
        created_unix_ms: Utc::now().timestamp_millis(),
        expires_unix_ms: Utc::now().timestamp_millis() + 30000,
        tags: vec!["cross-provider"].iter().map(|s| s.to_string()).collect(),
        payload: Some(Payload::TaskRequest(TaskRequest {
            task_id,
            operation: "bark_echo".to_string(),
            input_json: r#"{"volume": "high"}"#.to_string(),
            timeout_ms: 10000,
        })),
    };

    println!("[BEAGLE -> BOXER] Sending cross-provider bark...");
    beagle.0.publish_envelope(&req).await?;

    println!("\nAll agents active. Press Ctrl+C to terminate the scenario.");
    tokio::signal::ctrl_c().await?;
    
    for t in tasks {
        t.abort();
    }
    Ok(())
}

async fn run_mcp_server(_port: u16) -> Result<()> {
    eprintln!("The mcp-server subcommand is deprecated.");
    eprintln!("Use the standalone binary instead:");
    eprintln!("  1. Start daemon:  twilight-cli daemon start --config ~/.config/twilight/daemon.toml");
    eprintln!("  2. Start server:  twilight-mcp-server --name my-agent");
    Ok(())
}

async fn run_smoke_test() -> Result<()> {
    // ... (Existing smoke test remains unchanged)
    let sender_bus = Arc::new(TwilightBus::new("default", "local").await?);
    let receiver_bus = Arc::new(TwilightBus::new("default", "local").await?);

    let mut receiver_stream = receiver_bus.subscribe_traffic().await?;
    let mut result_stream = sender_bus.subscribe_traffic().await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let smoke_identity = create_default_identity("smoke-test-sender", "cli");
    let task_id = Uuid::new_v4().to_string();
    let mut target = MessageTarget::default();
    target.set_target_kind(TargetKind::Broadcast);
    sender_bus.publish_envelope(&TwilightEnvelope {
        message_uuid: Uuid::new_v4().to_string(),
        correlation_uuid: task_id.clone(),
        causation_uuid: String::new(),
        source: Some(smoke_identity),
        target: Some(target),
        kind: MessageKind::TaskRequest as i32,
        priority: 2,
        created_unix_ms: Utc::now().timestamp_millis(),
        expires_unix_ms: Utc::now().timestamp_millis() + 30_000,
        tags: Vec::new(),
        payload: Some(Payload::TaskRequest(TaskRequest {
            task_id: task_id.clone(),
            operation: "smoke-test".to_string(),
            input_json: r#"{"ping":true}"#.to_string(),
            timeout_ms: 30_000,
        })),
    }).await?;

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

async fn run_daemon_command(command: &DaemonCommands) -> Result<()> {
    match command {
        DaemonCommands::Enroll { jwt, out, binary } => {
            let out_path = out.clone().unwrap_or_else(|| {
                dirs_home().join(".config/twilight/identity.json")
            });
            println!("Enrolling identity from {:?} → {:?}", jwt, out_path);
            twilight_ziti::enroll(binary, jwt, &out_path).await?;
            println!("Enrollment complete. Run `twilight-cli daemon start` next.");
        }

        DaemonCommands::Start { config } => {
            let binary = find_daemon_binary();

            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let mut cmd = std::process::Command::new(&binary);
                cmd.arg("--config").arg(config);
                cmd.process_group(0); // detach from parent session
                let child = cmd.spawn()
                    .map_err(|e| anyhow::anyhow!("Failed to spawn {:?}: {e}", binary))?;
                println!("Daemon spawned (pid={}, binary={:?})", child.id(), binary);
                println!("PID file will be at: {:?}", default_socket_path().with_extension("pid"));
            }
            #[cfg(not(unix))]
            anyhow::bail!("daemon start is only supported on Unix");
        }

        DaemonCommands::Stop { socket } => {
            let pid_path = socket.as_ref()
                .map(|s| s.with_extension("pid"))
                .unwrap_or_else(|| default_socket_path().with_extension("pid"));
            let pid_str = std::fs::read_to_string(&pid_path)
                .map_err(|_| anyhow::anyhow!("No PID file at {:?}. Is the daemon running?", pid_path))?;
            let pid = pid_str.trim().to_string();
            std::process::Command::new("kill")
                .args(["-TERM", &pid])
                .status()?;
            let _ = std::fs::remove_file(&pid_path);
            println!("Sent SIGTERM to daemon (pid={pid})");
        }

        DaemonCommands::Status { socket } => {
            let socket_path = socket.clone().unwrap_or_else(default_socket_path);
            let pid_path = socket_path.with_extension("pid");

            let process_status = if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
                let pid = pid_str.trim().to_string();
                let alive = std::process::Command::new("kill")
                    .args(["-0", &pid])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if alive { format!("running (pid={pid})") } else { "dead (stale PID file)".to_string() }
            } else {
                "not running (no PID file)".to_string()
            };

            let socket_status = tokio::net::UnixStream::connect(&socket_path).await
                .map(|_| "reachable ✓")
                .unwrap_or("not reachable ✗");

            println!("Process:  {process_status}");
            println!("Socket:   {:?}  {socket_status}", socket_path);
        }
    }
    Ok(())
}

fn find_daemon_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("twilight-daemon");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("twilight-daemon")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
