use twilight_proto::twilight::{TwilightEnvelope, TaskRequest, MessageTarget, TargetKind, MessageKind};
use twilight_bus::TwilightBus;
use twilight_traffic_controller::TrafficController;
use std::sync::Arc;
use anyhow::Result;
use uuid::Uuid;
use chrono::Utc;

pub struct TwilightMcpServer {
    bus: Arc<TwilightBus>,
    controller: Arc<TrafficController>,
}

impl TwilightMcpServer {
    pub fn new(bus: Arc<TwilightBus>, controller: Arc<TrafficController>) -> Self {
        Self { bus, controller }
    }

    pub async fn publish_task(&self, operation: &str, input_json: &str, target: MessageTarget) -> Result<String> {
        let task_id = Uuid::new_v4().to_string();
        let envelope = TwilightEnvelope {
            message_uuid: Uuid::new_v4().to_string(),
            correlation_uuid: task_id.clone(),
            causation_uuid: String::new(),
            source: None, // Should be filled by the bus or caller
            target: Some(target),
            kind: MessageKind::TaskRequest as i32,
            priority: 2, // Normal
            created_unix_ms: Utc::now().timestamp_millis(),
            expires_unix_ms: Utc::now().timestamp_millis() + 30000,
            tags: Vec::new(),
            payload: Some(twilight_proto::twilight::twilight_envelope::Payload::TaskRequest(TaskRequest {
                task_id: task_id.clone(),
                operation: operation.to_string(),
                input_json: input_json.to_string(),
                timeout_ms: 30000,
            })),
        };

        self.bus.publish_envelope(&envelope).await?;
        Ok(task_id)
    }

    pub async fn ask_agent(&self, target_uuid: &str, operation: &str, input_json: &str) -> Result<String> {
        let mut target = MessageTarget::default();
        target.target_kind = TargetKind::Unicast as i32;
        target.target_agent_uuids.push(target_uuid.to_string());
        
        self.publish_task(operation, input_json, target).await
    }

    pub fn get_registry(&self) -> Vec<twilight_proto::twilight::AgentIdentity> {
        // This is a bit simplified, usually we'd want to return a wrap or filtered view
        self.controller.get_all_identities()
    }
}

