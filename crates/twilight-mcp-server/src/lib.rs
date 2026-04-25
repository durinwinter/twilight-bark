use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// IPC client that connects to a running twilight-daemon over a Unix socket.
/// Each MCP shim instance holds one DaemonClient, which registers the agent on connect
/// and publishes offline presence automatically when the connection is dropped.
pub struct DaemonClient {
    reader: Arc<Mutex<BufReader<tokio::net::unix::OwnedReadHalf>>>,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    pub agent_uuid: String,
}

impl DaemonClient {
    /// Connect to the daemon socket and register as the given agent name/role.
    pub async fn connect(socket: &Path, name: &str, role: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket).await.map_err(|e| {
            anyhow::anyhow!(
                "Cannot connect to twilight-daemon at {:?}: {e}\n\
                 Make sure the daemon is running: twilight-cli daemon start",
                socket
            )
        })?;
        let (r, w) = stream.into_split();
        let reader = Arc::new(Mutex::new(BufReader::new(r)));
        let writer = Arc::new(Mutex::new(w));

        // Send registration command
        {
            let cmd = json!({"cmd": "register", "name": name, "role": role});
            let mut line = serde_json::to_string(&cmd)?;
            line.push('\n');
            writer.lock().await.write_all(line.as_bytes()).await?;
        }

        // Read registration response
        let mut resp_line = String::new();
        reader.lock().await.read_line(&mut resp_line).await?;
        let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;

        let agent_uuid = resp["agent_uuid"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("daemon rejected registration: {:?}", resp))?
            .to_string();

        log::info!("Registered as '{name}' (uuid={agent_uuid})");
        Ok(Self { reader, writer, agent_uuid })
    }

    pub async fn get_registry(&self) -> Result<serde_json::Value> {
        let resp = self.call(json!({"cmd": "get_registry"})).await?;
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

    async fn call(&self, cmd: serde_json::Value) -> Result<serde_json::Value> {
        {
            let mut w = self.writer.lock().await;
            let mut line = serde_json::to_string(&cmd)?;
            line.push('\n');
            w.write_all(line.as_bytes()).await?;
        }
        let mut resp_line = String::new();
        self.reader.lock().await.read_line(&mut resp_line).await?;
        Ok(serde_json::from_str(resp_line.trim())?)
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
