use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
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
    /// Run a standalone agent connected to the daemon — for scripted E2E testing
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
        /// Daemon socket path (defaults to $XDG_RUNTIME_DIR or /tmp)
        #[arg(long, env = "TWILIGHT_DAEMON_SOCKET", global = true)]
        socket: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Listen for incoming tasks. Prints each one; optionally auto-replies.
    Listen {
        /// Agent name shown in the registry
        #[arg(long, default_value = "test-listener")]
        name: String,
        #[arg(long, default_value = "worker")]
        role: String,
        /// Immediately send a canned success reply to every received task
        #[arg(long)]
        auto_reply: bool,
    },
    /// Register as a sender, send one task, wait for the reply, then exit.
    Send {
        /// Agent name shown in the registry
        #[arg(long, default_value = "test-sender")]
        name: String,
        /// MCP operation name (e.g. "analyze", "bark_echo")
        #[arg(long)]
        operation: String,
        /// JSON input payload
        #[arg(long, default_value = "{}")]
        input: String,
        /// Target by agent name (registry lookup). Omit for broadcast.
        #[arg(long)]
        target: Option<String>,
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

        Commands::Agent { command, socket } => {
            let socket_path = socket.clone().unwrap_or_else(default_socket_path);
            match command {
                AgentCommands::Listen { name, role, auto_reply } => {
                    run_agent_listen(name, role, *auto_reply, &socket_path).await?;
                }
                AgentCommands::Send { name, operation, input, target } => {
                    run_agent_send(name, operation, input, target.as_deref(), &socket_path).await?;
                }
            }
        }
    }

    Ok(())
}

// ── Agent listen/send — IPC-based E2E test helpers ────────────────────────────

async fn ipc_write(w: &mut tokio::net::unix::OwnedWriteHalf, v: serde_json::Value) -> Result<()> {
    let mut s = serde_json::to_string(&v)?;
    s.push('\n');
    w.write_all(s.as_bytes()).await?;
    Ok(())
}

async fn ipc_read(lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>) -> Result<serde_json::Value> {
    match lines.next_line().await? {
        Some(line) => Ok(serde_json::from_str(&line)?),
        None => anyhow::bail!("daemon closed connection"),
    }
}

async fn ipc_connect_and_register(socket: &std::path::Path, name: &str, role: &str)
    -> Result<(tokio::net::unix::OwnedWriteHalf, tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>, String)>
{
    let stream = UnixStream::connect(socket).await
        .map_err(|e| anyhow::anyhow!("Cannot connect to daemon at {:?}: {e}\n  → Start it with: twilight-cli daemon start --config ~/.config/twilight/daemon.toml", socket))?;
    let (r, mut w) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(r).lines();
    ipc_write(&mut w, serde_json::json!({"cmd":"register","name":name,"role":role})).await?;
    let reg = ipc_read(&mut lines).await?;
    let uuid = reg["agent_uuid"].as_str().unwrap_or("?").to_string();
    Ok((w, lines, uuid))
}

async fn run_agent_listen(name: &str, role: &str, auto_reply: bool, socket: &std::path::Path) -> Result<()> {
    let (mut w, mut lines, uuid) = ipc_connect_and_register(socket, name, role).await?;

    println!("[{name}] Registered  uuid={}", &uuid[..8.min(uuid.len())]);
    println!("[{name}] Subscribing to tasks...");

    ipc_write(&mut w, serde_json::json!({"cmd":"subscribe_tasks"})).await?;
    let ack = ipc_read(&mut lines).await?;
    if !ack["ok"].as_bool().unwrap_or(false) {
        anyhow::bail!("subscribe_tasks failed: {ack}");
    }

    println!("[{name}] Listening — press Ctrl-C to stop\n");

    while let Ok(Some(line)) = lines.next_line().await {
        let event: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        let ts = chrono::Utc::now().format("%H:%M:%S%.3f");

        match event["event"].as_str() {
            Some("task_request") => {
                let task_id  = event["task_id"].as_str().unwrap_or("?");
                let op       = event["operation"].as_str().unwrap_or("?");
                let input    = event["input_json"].as_str().unwrap_or("{}");
                let src      = event["source_uuid"].as_str().unwrap_or("?");

                println!("[{name}] [{ts}] RECEIVED task");
                println!("  task_id  : {}", task_id);
                println!("  operation: {op}");
                println!("  input    : {input}");
                println!("  from     : {}", &src[..8.min(src.len())]);

                if auto_reply {
                    let output = serde_json::json!({"status":"ok","handled_by":name,"operation":op});
                    ipc_write(&mut w, serde_json::json!({
                        "cmd": "reply_task",
                        "task_id": task_id,
                        "output_json": output.to_string(),
                        "success": true,
                    })).await?;
                    let rep = ipc_read(&mut lines).await?;
                    println!("  replied  : {rep}");
                }
                println!();
            }
            Some("task_result") => {
                let task_id = event["task_id"].as_str().unwrap_or("?");
                let output  = event["output_json"].as_str().unwrap_or("{}");
                let ok      = event["success"].as_bool().unwrap_or(false);
                println!("[{name}] [{ts}] RESULT  task_id={task_id}  success={ok}");
                println!("  output: {output}\n");
            }
            _ => {}
        }
    }

    println!("[{name}] Connection closed.");
    Ok(())
}

async fn run_agent_send(name: &str, operation: &str, input: &str, target_name: Option<&str>, socket: &std::path::Path) -> Result<()> {
    let (mut w, mut lines, uuid) = ipc_connect_and_register(socket, name, "sender").await?;
    println!("[{name}] Registered  uuid={}", &uuid[..8.min(uuid.len())]);

    // Resolve target by name if given
    let target_uuid: Option<String> = if let Some(tgt) = target_name {
        ipc_write(&mut w, serde_json::json!({"cmd":"get_registry"})).await?;
        let resp = ipc_read(&mut lines).await?;
        let agents = resp["agents"].as_array().cloned().unwrap_or_default();
        let found = agents.iter()
            .find(|a| a["agent_name"].as_str() == Some(tgt))
            .and_then(|a| a["node_uuid"].as_str().map(|s| s.to_string()));
        if found.is_none() {
            let names: Vec<&str> = agents.iter()
                .filter_map(|a| a["agent_name"].as_str())
                .collect();
            anyhow::bail!("Agent '{tgt}' not found in registry. Online: {:?}", names);
        }
        found
    } else {
        None
    };

    let ts = chrono::Utc::now().format("%H:%M:%S%.3f");
    let cmd = if let Some(ref tu) = target_uuid {
        println!("[{name}] [{ts}] SENDING → {}", target_name.unwrap_or("?"));
        serde_json::json!({"cmd":"ask_agent","agent_uuid":tu,"operation":operation,"input_json":input})
    } else {
        println!("[{name}] [{ts}] BROADCASTING");
        serde_json::json!({"cmd":"publish_task","operation":operation,"input_json":input})
    };
    println!("  operation: {operation}");
    println!("  input    : {input}\n");

    ipc_write(&mut w, cmd).await?;
    let sent = ipc_read(&mut lines).await?;
    let task_id = sent["task_id"].as_str().unwrap_or("?").to_string();
    let ts2 = chrono::Utc::now().format("%H:%M:%S%.3f");
    println!("[{name}] [{ts2}] Dispatched  task_id={task_id}");

    // Subscribe to receive the reply
    ipc_write(&mut w, serde_json::json!({"cmd":"subscribe_tasks"})).await?;
    let _ = ipc_read(&mut lines).await?;
    println!("[{name}] Waiting for reply (15s timeout)...\n");

    let result = tokio::time::timeout(Duration::from_secs(15), async {
        while let Ok(Some(line)) = lines.next_line().await {
            let event: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            if event["event"].as_str() == Some("task_result") {
                return Some(event);
            }
        }
        None
    }).await;

    match result {
        Ok(Some(event)) => {
            let ts3 = chrono::Utc::now().format("%H:%M:%S%.3f");
            let output = event["output_json"].as_str().unwrap_or("{}");
            let ok = event["success"].as_bool().unwrap_or(false);
            println!("[{name}] [{ts3}] REPLY RECEIVED  success={ok}");
            println!("  output: {output}");
        }
        Ok(None) => println!("[{name}] Connection closed — no reply received."),
        Err(_) => println!("[{name}] Timeout — no reply after 15s."),
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
