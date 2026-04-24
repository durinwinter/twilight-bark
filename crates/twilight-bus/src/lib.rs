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
    session: Arc<Session>,
    tenant: String,
    site: String,
}

impl TwilightBus {
    pub async fn new(tenant: &str, site: &str) -> Result<Self> {
        let config = Config::default();
        let session = zenoh::open(config).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        
        Ok(Self {
            session: Arc::new(session),
            tenant: tenant.to_string(),
            site: site.to_string(),
        })
    }

    pub async fn publish_envelope(&self, envelope: &TwilightEnvelope) -> Result<()> {
        let mut buf = Vec::new();
        envelope.encode(&mut buf)?;
        
        let key = format!("twilight/{}/{}/traffic/{}", self.tenant, self.site, envelope.message_uuid);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        
        // Also publish to signal mirror for NuZe/visualization
        self.publish_signal_json(envelope, "traffic").await?;
        
        Ok(())
    }

    async fn publish_signal_json<T: serde::Serialize>(&self, data: &T, kind: &str) -> Result<()> {
        let json = serde_json::to_string(data)?;
        let key = format!("twilight/{}/{}/signal/{}", self.tenant, self.site, kind);
        self.session.put(&key, json).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(())
    }

    pub async fn publish_presence(&self, presence: &AgentPresence) -> Result<()> {
        let mut buf = Vec::new();
        presence.encode(&mut buf)?;
        
        let node_id = presence.identity.as_ref().map(|id| id.node_uuid.as_str()).unwrap_or("unknown");
        let key = format!("twilight/{}/{}/presence/{}", self.tenant, self.site, node_id);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        
        self.publish_signal_json(presence, "presence").await?;
        
        Ok(())
    }

    pub async fn publish_heartbeat(&self, heartbeat: &Heartbeat) -> Result<()> {
        let mut buf = Vec::new();
        heartbeat.encode(&mut buf)?;
        
        let key = format!("twilight/{}/{}/heartbeat/{}", self.tenant, self.site, heartbeat.node_id);
        self.session.put(&key, buf).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
        
        self.publish_signal_json(heartbeat, "heartbeat").await?;
        
        Ok(())
    }

    /// Spawns a background task that periodically publishes heartbeats for this node.
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
                    log::warn!("Automated heartbeat failed: {:?}", e);
                }
            }
        })
    }

    pub async fn subscribe_presence(&self) -> Result<BoxedStream<AgentPresence>> {
        let key = format!("twilight/{}/{}/presence/*", self.tenant, self.site);
        let subscriber = self.session.declare_subscriber(&key).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let stream = futures::stream::unfold(subscriber, |sub| async move {
            match sub.recv_async().await {
                Ok(sample) => {
                    let payload = sample.payload();
                    let data: Vec<u8> = payload.to_bytes().to_vec();
                    let presence = AgentPresence::decode(data.as_slice()).unwrap_or_default();
                    Some((presence, sub))
                }
                Err(_) => None,
            }
        });
        Ok(Box::pin(stream))
    }

    pub async fn subscribe_traffic(&self) -> Result<BoxedStream<TwilightEnvelope>> {
        let key = format!("twilight/{}/{}/traffic/*", self.tenant, self.site);
        let subscriber = self.session.declare_subscriber(&key).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;

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
        let key = format!("twilight/{}/{}/heartbeat/*", self.tenant, self.site);
        let subscriber = self.session.declare_subscriber(&key).await.map_err(|e| anyhow::anyhow!("{:?}", e))?;

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
