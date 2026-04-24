use twilight_proto::twilight::{AgentIdentity, AgentStatus, TwilightEnvelope, Observation, MessageKind};
use twilight_bus::TwilightBus;
use twilight_core::{create_default_identity, create_presence};
use std::sync::Arc;
use anyhow::Result;
use tokio::time::{sleep, Duration};
use walkdir::WalkDir;
use log::info;

pub struct FilesystemAdapter {
    bus: Arc<TwilightBus>,
    identity: AgentIdentity,
    root_path: String,
}

impl FilesystemAdapter {
    pub async fn new(bus: Arc<TwilightBus>, root_path: &str) -> Result<Self> {
        let identity = create_default_identity("filesystem-adapter", "adapter");
        Ok(Self {
            bus,
            identity,
            root_path: root_path.to_string(),
        })
    }

    pub async fn run(&self) -> Result<()> {
        let presence = create_presence(self.identity.clone(), AgentStatus::Online);
        self.bus.publish_presence(&presence).await?;

        loop {
            // Placeholder: periodic scan or watch
            info!("Scanning filesystem at {}", self.root_path);
            let mut files = Vec::new();
            for entry in WalkDir::new(&self.root_path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    files.push(entry.path().to_string_lossy().to_string());
                }
            }

            let observation = Observation {
                source_id: self.identity.node_uuid.clone(),
                event_type: "fs_scan".to_string(),
                data_json: serde_json::to_string(&files)?,
            };

            let envelope = TwilightEnvelope {
                message_uuid: uuid::Uuid::new_v4().to_string(),
                correlation_uuid: String::new(),
                causation_uuid: String::new(),
                source: Some(self.identity.clone()),
                target: None,
                kind: MessageKind::Observation as i32,
                priority: 1, // Low
                created_unix_ms: chrono::Utc::now().timestamp_millis(),
                expires_unix_ms: chrono::Utc::now().timestamp_millis() + 60000,
                tags: vec!["fs_scan".to_string()],
                payload: Some(twilight_proto::twilight::twilight_envelope::Payload::Observation(observation)),
            };

            self.bus.publish_envelope(&envelope).await?;

            sleep(Duration::from_secs(600)).await; // Scan every 10 minutes
        }
    }
}
