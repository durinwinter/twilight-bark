use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

/// IPC client that connects to a running twilight-daemon over a Unix socket.
/// Registers on connect then immediately subscribes to task events (Phase 2).
/// A background task routes incoming lines: push events go to `task_queue`,
/// command responses go to `cmd_rx` — keeping the two streams separate.
pub struct DaemonClient {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    pub agent_uuid: String,
    task_queue: Arc<Mutex<Vec<serde_json::Value>>>,
    /// Locking this serializes in-flight commands (write + read response atomically).
    cmd_rx: Arc<Mutex<mpsc::Receiver<serde_json::Value>>>,
}

impl DaemonClient {
    pub async fn connect(socket: &Path, name: &str, role: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket).await.map_err(|e| {
            anyhow::anyhow!(
                "Cannot connect to twilight-daemon at {:?}: {e}\n\
                 Make sure the daemon is running: twilight-cli daemon start",
                socket
            )
        })?;
        let (r, w) = stream.into_split();
        let mut reader = BufReader::new(r);
        let writer = Arc::new(Mutex::new(w));

        // Phase 1: register
        {
            let mut line = serde_json::to_string(&json!({"cmd":"register","name":name,"role":role}))?;
            line.push('\n');
            writer.lock().await.write_all(line.as_bytes()).await?;
        }
        let mut buf = String::new();
        reader.read_line(&mut buf).await?;
        let resp: serde_json::Value = serde_json::from_str(buf.trim())?;
        let agent_uuid = resp["agent_uuid"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("daemon rejected registration: {:?}", resp))?
            .to_string();
        log::info!("Registered as '{name}' (uuid={agent_uuid})");

        // Phase 1→2: subscribe_tasks
        {
            let mut line = serde_json::to_string(&json!({"cmd":"subscribe_tasks"}))?;
            line.push('\n');
            writer.lock().await.write_all(line.as_bytes()).await?;
        }
        buf.clear();
        reader.read_line(&mut buf).await?;
        let ack: serde_json::Value = serde_json::from_str(buf.trim())?;
        if ack["ok"].as_bool() != Some(true) {
            anyhow::bail!("subscribe_tasks rejected: {:?}", ack);
        }
        log::info!("Subscribed to task events");

        // Spawn background reader: events → task_queue, responses → resp_tx
        let task_queue = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let (resp_tx, resp_rx) = mpsc::channel::<serde_json::Value>(16);
        {
            let tq = Arc::clone(&task_queue);
            tokio::spawn(async move {
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let val: serde_json::Value =
                                match serde_json::from_str(line.trim()) {
                                    Ok(v) => v,
                                    Err(_) => continue,
                                };
                            if val.get("event").is_some() {
                                tq.lock().await.push(val);
                            } else {
                                let _ = resp_tx.send(val).await;
                            }
                        }
                    }
                }
            });
        }

        Ok(Self {
            writer,
            agent_uuid,
            task_queue,
            cmd_rx: Arc::new(Mutex::new(resp_rx)),
        })
    }

    pub async fn get_registry(&self) -> Result<serde_json::Value> {
        let resp = self.call(json!({"cmd":"get_registry"})).await?;
        Ok(resp["agents"].clone())
    }

    pub async fn publish_task(&self, operation: &str, input_json: &str) -> Result<String> {
        let resp = self.call(json!({
            "cmd": "publish_task",
            "operation": operation,
            "input_json": input_json,
        })).await?;
        extract_task_id(&resp)
    }

    pub async fn ask_agent(&self, agent_uuid: &str, operation: &str, input_json: &str) -> Result<String> {
        let resp = self.call(json!({
            "cmd": "ask_agent",
            "agent_uuid": agent_uuid,
            "operation": operation,
            "input_json": input_json,
        })).await?;
        extract_task_id(&resp)
    }

    pub async fn reply_task(&self, task_id: &str, output_json: &str, success: bool) -> Result<()> {
        let resp = self.call(json!({
            "cmd": "reply_task",
            "task_id": task_id,
            "output_json": output_json,
            "success": success,
        })).await?;
        if resp["ok"].as_bool() != Some(true) {
            anyhow::bail!("reply_task failed: {:?}", resp);
        }
        Ok(())
    }

    /// Drain and return all queued incoming task events (non-blocking).
    pub async fn get_pending_tasks(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.task_queue.lock().await)
    }

    /// Send a command and wait for its response, serialised so no two commands race.
    async fn call(&self, cmd: serde_json::Value) -> Result<serde_json::Value> {
        let mut rx = self.cmd_rx.lock().await;
        {
            let mut w = self.writer.lock().await;
            let mut line = serde_json::to_string(&cmd)?;
            line.push('\n');
            w.write_all(line.as_bytes()).await?;
        }
        rx.recv().await
            .ok_or_else(|| anyhow::anyhow!("daemon connection closed"))
    }
}

fn extract_task_id(resp: &serde_json::Value) -> Result<String> {
    if let Some(err) = resp["error"].as_str() {
        anyhow::bail!("daemon error: {err}");
    }
    resp["task_id"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing task_id in response: {:?}", resp))
}
