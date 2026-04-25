use anyhow::Result;
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use twilight_bus::TwilightBus;
use twilight_core::{create_node_identity, create_presence};
use twilight_proto::twilight::{
    twilight_envelope::Payload, AgentStatus, MessageKind, MessageTarget, TargetKind, TaskRequest,
    TaskResult, TwilightEnvelope,
};
use twilight_traffic_controller::TrafficController;
use uuid::Uuid;
use log::{error, info, warn};

// ── IPC request types ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum IpcRequest {
    Register { name: String, role: String },
    GetRegistry,
    PublishTask { operation: String, input_json: String },
    AskAgent { agent_uuid: String, operation: String, input_json: String },
    /// Subscribe to task events. After the {"ok":true} ack the daemon will push
    /// {"event":"task_request",...} and {"event":"task_result",...} JSON lines.
    SubscribeTasks,
    /// Send a TaskResult back to the original requester of a task.
    ReplyTask { task_id: String, output_json: String, success: bool },
    Ping,
}

#[derive(Serialize, Default)]
struct IpcResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agents: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl IpcResponse {
    fn ok() -> Self { Self { ok: true, ..Default::default() } }
    fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, error: Some(msg.into()), ..Default::default() }
    }
}

// ── IpcServer ─────────────────────────────────────────────────────────────────

pub struct IpcServer {
    pub socket_path: PathBuf,
    bus: Arc<TwilightBus>,
    controller: Arc<TrafficController>,
    node_id: String,
    tenant: String,
    /// agent_uuid → channel for pushing task events to subscribed connections.
    pub task_senders: Arc<DashMap<String, mpsc::Sender<serde_json::Value>>>,
    /// task_id → source_agent_uuid — used to route TaskResult replies back.
    pub pending_tasks: Arc<DashMap<String, String>>,
}

impl IpcServer {
    pub fn new(
        socket_path: PathBuf,
        bus: Arc<TwilightBus>,
        controller: Arc<TrafficController>,
        node_id: String,
        tenant: String,
    ) -> Self {
        Self {
            socket_path,
            bus,
            controller,
            node_id,
            tenant,
            task_senders: Arc::new(DashMap::new()),
            pending_tasks: Arc::new(DashMap::new()),
        }
    }

    pub fn push_task_event(&self, agent_uuid: &str, event: serde_json::Value) {
        if let Some(tx) = self.task_senders.get(agent_uuid) {
            let _ = tx.try_send(event);
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);
        let listener = UnixListener::bind(&self.socket_path)?;
        info!("IPC server listening on {:?}", self.socket_path);
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let srv = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = srv.handle(stream).await {
                            warn!("IPC client error: {e}");
                        }
                    });
                }
                Err(e) => error!("IPC accept error: {e}"),
            }
        }
    }

    async fn handle(&self, stream: UnixStream) -> Result<()> {
        let (r, mut w) = stream.into_split();
        let mut lines = BufReader::new(r).lines();
        let mut registered: Option<String> = None;

        // ── Phase 1: normal request/response until subscribe_tasks ───────────
        let push_rx = loop {
            match lines.next_line().await {
                Ok(Some(ref line)) => {
                    match serde_json::from_str::<IpcRequest>(line) {
                        Ok(IpcRequest::SubscribeTasks) => {
                            let uuid = match registered.as_ref() {
                                Some(u) => u.clone(),
                                None => {
                                    write_resp(&mut w, IpcResponse::err("must register first")).await?;
                                    continue;
                                }
                            };
                            let (tx, rx) = mpsc::channel(64);
                            self.task_senders.insert(uuid, tx);
                            write_resp(&mut w, IpcResponse::ok()).await?;
                            break rx;
                        }
                        Ok(req) => {
                            let resp = self.dispatch(req, &mut registered).await;
                            write_resp(&mut w, resp).await?;
                        }
                        Err(e) => {
                            write_resp(&mut w, IpcResponse::err(format!("parse error: {e}"))).await?;
                        }
                    }
                }
                _ => {
                    self.cleanup(&registered).await;
                    return Ok(());
                }
            }
        };

        // ── Phase 2: concurrent incoming commands + outgoing task pushes ─────
        let mut push_rx = push_rx;
        loop {
            tokio::select! {
                // Push task events to client
                event = push_rx.recv() => {
                    match event {
                        Some(e) => {
                            let mut out = serde_json::to_string(&e).unwrap_or_default();
                            out.push('\n');
                            w.write_all(out.as_bytes()).await?;
                        }
                        None => break,
                    }
                }
                // Accept reply_task / ping commands from client
                line = lines.next_line() => {
                    match line {
                        Ok(Some(ref l)) => {
                            let resp = match serde_json::from_str::<IpcRequest>(l) {
                                Ok(req) => self.dispatch(req, &mut registered).await,
                                Err(e) => IpcResponse::err(format!("parse error: {e}")),
                            };
                            write_resp(&mut w, resp).await?;
                        }
                        _ => break,
                    }
                }
            }
        }

        self.cleanup(&registered).await;
        Ok(())
    }

    async fn cleanup(&self, registered: &Option<String>) {
        if let Some(uuid) = registered {
            info!("IPC client disconnected: {uuid}");
            self.task_senders.remove(uuid);
            if let Some(identity) = self.controller.get_identity(uuid) {
                let offline = create_presence(identity, AgentStatus::Offline);
                let _ = self.bus.publish_presence(&offline).await;
            }
            self.controller.remove_agent(uuid);
        }
    }

    async fn dispatch(&self, req: IpcRequest, registered: &mut Option<String>) -> IpcResponse {
        match req {
            IpcRequest::Ping => IpcResponse::ok(),

            IpcRequest::SubscribeTasks => {
                IpcResponse::err("subscribe_tasks must be sent before any other commands after registration — reconnect")
            }

            IpcRequest::Register { name, role } => {
                let identity = create_node_identity(&name, &role, &self.node_id, &self.tenant);
                let uuid = identity.node_uuid.clone();
                *registered = Some(uuid.clone());
                let presence = create_presence(identity.clone(), AgentStatus::Online);
                let _ = self.bus.publish_presence(&presence).await;
                self.controller.update_presence(presence);
                info!("Registered agent '{}' uuid={}", name, uuid);
                IpcResponse { ok: true, agent_uuid: Some(uuid), ..Default::default() }
            }

            IpcRequest::GetRegistry => {
                let agents = self.controller.get_all_identities();
                IpcResponse {
                    ok: true,
                    agents: Some(serde_json::to_value(&agents).unwrap_or(json!([]))),
                    ..Default::default()
                }
            }

            IpcRequest::PublishTask { operation, input_json } => {
                let src_uuid = match registered.as_ref() {
                    Some(uuid) => uuid.clone(),
                    None => return IpcResponse::err("must register before publishing tasks"),
                };
                let mut target = MessageTarget::default();
                target.target_kind = TargetKind::Broadcast as i32;
                match self.send_task_envelope(&src_uuid, &operation, &input_json, target).await {
                    Ok(id) => IpcResponse { ok: true, task_id: Some(id), ..Default::default() },
                    Err(e) => IpcResponse::err(e.to_string()),
                }
            }

            IpcRequest::AskAgent { agent_uuid, operation, input_json } => {
                let src_uuid = match registered.as_ref() {
                    Some(uuid) => uuid.clone(),
                    None => return IpcResponse::err("must register before sending tasks"),
                };
                let mut target = MessageTarget::default();
                target.target_kind = TargetKind::Unicast as i32;
                target.target_agent_uuids.push(agent_uuid);
                match self.send_task_envelope(&src_uuid, &operation, &input_json, target).await {
                    Ok(id) => IpcResponse { ok: true, task_id: Some(id), ..Default::default() },
                    Err(e) => IpcResponse::err(e.to_string()),
                }
            }

            IpcRequest::ReplyTask { task_id, output_json, success } => {
                let src_uuid = match registered.as_ref() {
                    Some(uuid) => uuid.clone(),
                    None => return IpcResponse::err("must register before replying"),
                };

                // Route result to the original requester's push channel
                if let Some((_, req_uuid)) = self.pending_tasks.remove(&task_id) {
                    let event = json!({
                        "event": "task_result",
                        "task_id": &task_id,
                        "output_json": &output_json,
                        "success": success,
                        "source_uuid": &src_uuid,
                    });
                    self.push_task_event(&req_uuid, event);
                }

                // Also publish as a TaskResult envelope to Zenoh for bus-wide visibility
                let src_identity = self.controller.get_identity(&src_uuid).unwrap_or_else(|| {
                    create_node_identity("unknown", "unknown", &self.node_id, &self.tenant)
                });
                let envelope = TwilightEnvelope {
                    message_uuid: Uuid::new_v4().to_string(),
                    correlation_uuid: task_id.clone(),
                    causation_uuid: String::new(),
                    source: Some(src_identity),
                    target: None,
                    kind: MessageKind::TaskResult as i32,
                    priority: 2,
                    created_unix_ms: Utc::now().timestamp_millis(),
                    expires_unix_ms: Utc::now().timestamp_millis() + 30_000,
                    tags: Vec::new(),
                    payload: Some(Payload::TaskResult(TaskResult {
                        task_id,
                        output_json,
                        success,
                        error_message: String::new(),
                    })),
                };
                let _ = self.bus.publish_envelope(&envelope).await;
                IpcResponse::ok()
            }
        }
    }

    async fn send_task_envelope(
        &self,
        src_uuid: &str,
        operation: &str,
        input_json: &str,
        target: MessageTarget,
    ) -> anyhow::Result<String> {
        let task_id = Uuid::new_v4().to_string();
        let source = self.controller.get_identity(src_uuid).unwrap_or_else(|| {
            create_node_identity("unknown", "unknown", &self.node_id, &self.tenant)
        });
        let envelope = TwilightEnvelope {
            message_uuid: Uuid::new_v4().to_string(),
            correlation_uuid: task_id.clone(),
            causation_uuid: String::new(),
            source: Some(source),
            target: Some(target),
            kind: MessageKind::TaskRequest as i32,
            priority: 2,
            created_unix_ms: Utc::now().timestamp_millis(),
            expires_unix_ms: Utc::now().timestamp_millis() + 30_000,
            tags: Vec::new(),
            payload: Some(Payload::TaskRequest(TaskRequest {
                task_id: task_id.clone(),
                operation: operation.to_string(),
                input_json: input_json.to_string(),
                timeout_ms: 30_000,
            })),
        };
        self.bus.publish_envelope(&envelope).await?;
        Ok(task_id)
    }
}

async fn write_resp(w: &mut tokio::net::unix::OwnedWriteHalf, resp: IpcResponse) -> Result<()> {
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    w.write_all(out.as_bytes()).await?;
    Ok(())
}
