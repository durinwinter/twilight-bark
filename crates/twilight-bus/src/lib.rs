use anyhow::Result;
use twilight_proto::twilight::{TwilightEnvelope, AgentPresence, Heartbeat, AgentStatus};
use zenoh::Session;
use zenoh::config::Config;
use prost::Message;
use std::sync::Arc;
use futures::Stream;
use std::pin::Pin;
use chrono::Utc;
use tokio::task::JoinHandle;

pub type BoxedStream<T> = Pin<Box<dyn Stream<Item = T> + Send>>;

pub struct TwilightBus {
    pub session: Arc<Session>,
    pub tenant: String,
    pub node_id: String,
}

impl TwilightBus {
    /// Open a Zenoh session in default peer mode (used for local dev/CLI scenarios).
    pub async fn new(tenant: &str, node_id: &str) -> Result<Self> {
        let config = Config::default();
        let session = zenoh::open(config).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(Self {
            session: Arc::new(session),
            tenant: tenant.to_string(),
            node_id: node_id.to_string(),
        })
    }

    /// Open a Zenoh session from a JSON5 config object (used by the daemon for
    /// client/router mode with specific peers or listen endpoints).
    pub async fn new_with_config(zenoh_json: serde_json::Value, tenant: &str, node_id: &str) -> Result<Self> {
        let json_str = serde_json::to_string(&zenoh_json)?;
        let config = Config::from_json5(&json_str)
            .map_err(|e| anyhow::anyhow!("Zenoh config parse error: {:?}", e))?;
        let session = zenoh::open(config).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(Self {
            session: Arc::new(session),
            tenant: tenant.to_string(),
            node_id: node_id.to_string(),
        })
    }

    pub async fn publish_envelope(&self, envelope: &TwilightEnvelope) -> Result<()> {
        let mut buf = Vec::new();
        envelope.encode(&mut buf)?;
        let key = format!("twilight/{}/{}/traffic/{}", self.tenant, self.node_id, envelope.message_uuid);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        self.publish_signal_json(envelope, "traffic").await?;
        Ok(())
    }

    async fn publish_signal_json<T: serde::Serialize>(&self, data: &T, kind: &str) -> Result<()> {
        let json = serde_json::to_string(data)?;
        let key = format!("twilight/{}/{}/signal/{}", self.tenant, self.node_id, kind);
        self.session.put(&key, json).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(())
    }

    pub async fn publish_presence(&self, presence: &AgentPresence) -> Result<()> {
        let mut buf = Vec::new();
        presence.encode(&mut buf)?;
        let node_id = presence.identity.as_ref()
            .map(|id| id.node_uuid.as_str())
            .unwrap_or("unknown");
        let key = format!("twilight/{}/{}/presence/{}", self.tenant, self.node_id, node_id);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        self.publish_signal_json(presence, "presence").await?;
        Ok(())
    }

    pub async fn publish_heartbeat(&self, heartbeat: &Heartbeat) -> Result<()> {
        let mut buf = Vec::new();
        heartbeat.encode(&mut buf)?;
        let key = format!("twilight/{}/{}/heartbeat/{}", self.tenant, self.node_id, heartbeat.node_id);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        self.publish_signal_json(heartbeat, "heartbeat").await?;
        Ok(())
    }

    pub fn start_heartbeat_loop(self: Arc<Self>, node_id: String, interval_secs: u64) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let hb = Heartbeat {
                    node_id: node_id.clone(),
                    status: AgentStatus::Online as i32,
                    timestamp_unix_ms: Utc::now().timestamp_millis(),
                    active_tasks: 0,
                    queued_tasks: 0,
                };
                if let Err(e) = self.publish_heartbeat(&hb).await {
                    log::warn!("Heartbeat failed: {:?}", e);
                }
            }
        })
    }

    // ── Node-local subscriptions (own node only) ─────────────────────────────

    pub async fn subscribe_presence(&self) -> Result<BoxedStream<AgentPresence>> {
        let key = format!("twilight/{}/{}/presence/*", self.tenant, self.node_id);
        self.make_presence_stream(&key).await
    }

    pub async fn subscribe_traffic(&self) -> Result<BoxedStream<TwilightEnvelope>> {
        let key = format!("twilight/{}/{}/traffic/*", self.tenant, self.node_id);
        let subscriber = self.session.declare_subscriber(&key).await
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let stream = futures::stream::unfold(subscriber, |sub| async move {
            match sub.recv_async().await {
                Ok(sample) => {
                    let data: Vec<u8> = sample.payload().to_bytes().to_vec();
                    let envelope = TwilightEnvelope::decode(data.as_slice()).unwrap_or_default();
                    Some((envelope, sub))
                }
                Err(_) => None,
            }
        });
        Ok(Box::pin(stream))
    }

    pub async fn subscribe_heartbeat(&self) -> Result<BoxedStream<Heartbeat>> {
        let key = format!("twilight/{}/{}/heartbeat/*", self.tenant, self.node_id);
        self.make_heartbeat_stream(&key).await
    }

    // ── Cross-node subscriptions (all nodes in tenant) ────────────────────────

    /// Subscribe to presence messages from ALL nodes in the tenant.
    /// Used by the daemon to build a global agent registry.
    pub async fn subscribe_all_presence(&self) -> Result<BoxedStream<AgentPresence>> {
        let key = format!("twilight/{}/*/presence/*", self.tenant);
        self.make_presence_stream(&key).await
    }

    /// Subscribe to heartbeats from ALL nodes in the tenant.
    pub async fn subscribe_all_heartbeats(&self) -> Result<BoxedStream<Heartbeat>> {
        let key = format!("twilight/{}/*/heartbeat/*", self.tenant);
        self.make_heartbeat_stream(&key).await
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    async fn make_presence_stream(&self, key: &str) -> Result<BoxedStream<AgentPresence>> {
        let subscriber = self.session.declare_subscriber(key).await
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let stream = futures::stream::unfold(subscriber, |sub| async move {
            match sub.recv_async().await {
                Ok(sample) => {
                    let data: Vec<u8> = sample.payload().to_bytes().to_vec();
                    let presence = AgentPresence::decode(data.as_slice()).unwrap_or_default();
                    Some((presence, sub))
                }
                Err(_) => None,
            }
        });
        Ok(Box::pin(stream))
    }

    async fn make_heartbeat_stream(&self, key: &str) -> Result<BoxedStream<Heartbeat>> {
        let subscriber = self.session.declare_subscriber(key).await
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let stream = futures::stream::unfold(subscriber, |sub| async move {
            match sub.recv_async().await {
                Ok(sample) => {
                    let data: Vec<u8> = sample.payload().to_bytes().to_vec();
                    let heartbeat = Heartbeat::decode(data.as_slice()).unwrap_or_default();
                    Some((heartbeat, sub))
                }
                Err(_) => None,
            }
        });
        Ok(Box::pin(stream))
    }
}
