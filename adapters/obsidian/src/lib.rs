use twilight_proto::twilight::{AgentIdentity, AgentStatus, TwilightEnvelope, Observation, MessageKind};
use twilight_bus::TwilightBus;
use twilight_core::{create_default_identity, create_presence};
use std::sync::Arc;
use anyhow::Result;
use tokio::time::{sleep, Duration};
use walkdir::WalkDir;
use log::info;
use gray_matter::Matter;
use gray_matter::engine::YAML;

#[derive(Debug, serde::Serialize)]
pub struct Pod {
    pub id: String,
    pub data: String,
}

pub struct ObsidianAdapter {
    bus: Arc<TwilightBus>,
    identity: AgentIdentity,
    vault_path: String,
}

impl ObsidianAdapter {
    pub async fn new(bus: Arc<TwilightBus>, vault_path: &str) -> Result<Self> {
        let identity = create_default_identity("obsidian-adapter", "adapter");
        Ok(Self {
            bus,
            identity,
            vault_path: vault_path.to_string(),
        })
    }

    pub async fn run(&self) -> Result<()> {
        let presence = create_presence(self.identity.clone(), AgentStatus::Online);
        self.bus.publish_presence(&presence).await?;

        let matter = Matter::<YAML>::new();

        loop {
            info!("Scanning Obsidian vault at {}", self.vault_path);
            
            for entry in WalkDir::new(&self.vault_path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() && entry.path().extension().map_or(false, |ext| ext == "md") {
                    let content = std::fs::read_to_string(entry.path())?;
                    let result = matter.parse(&content);
                    
                    // gray_matter::Pod doesn't implement Serialize, so we use its Debug form for now
                    // or just an empty JSON if none.
                    let data_json = if let Some(pod) = result.data {
                        format!("{:?}", pod)
                    } else {
                        "{}".to_string()
                    };

                    let observation = Observation {
                        source_id: self.identity.node_uuid.clone(),
                        event_type: "obsidian_note".to_string(),
                        data_json,
                    };

                    let envelope = TwilightEnvelope {
                        message_uuid: uuid::Uuid::new_v4().to_string(),
                        correlation_uuid: String::new(),
                        causation_uuid: String::new(),
                        source: Some(self.identity.clone()),
                        target: None,
                        kind: MessageKind::Observation as i32,
                        priority: 2,
                        created_unix_ms: chrono::Utc::now().timestamp_millis(),
                        expires_unix_ms: chrono::Utc::now().timestamp_millis() + 60000,
                        tags: vec!["obsidian".to_string(), entry.file_name().to_string_lossy().into_owned()],
                        payload: Some(twilight_proto::twilight::twilight_envelope::Payload::Observation(observation)),
                    };

                    self.bus.publish_envelope(&envelope).await?;
                }
            }

            sleep(Duration::from_secs(300)).await; // Scan every 5 minutes
        }
    }
}
