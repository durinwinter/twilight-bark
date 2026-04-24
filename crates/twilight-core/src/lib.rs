use twilight_proto::twilight::{AgentIdentity, NodeKind, AgentStatus, AgentPresence};
use uuid::Uuid;
use chrono::Utc;

pub fn create_default_identity(name: &str, role: &str) -> AgentIdentity {
    AgentIdentity {
        agent_uuid: Uuid::new_v4().to_string(),
        instance_uuid: Uuid::new_v4().to_string(),
        node_uuid: Uuid::new_v4().to_string(),
        agent_name: name.to_string(),
        display_name: name.to_string(),
        role: role.to_string(),
        role_description: String::new(),
        node_kind: NodeKind::Agent as i32,
        tenant: "default".to_string(),
        site: "default".to_string(),
        region: "local".to_string(),
        llm_provider: String::new(),
        llm_service: String::new(),
        model_name: String::new(),
        model_uuid: String::new(),
        roles: vec![role.to_string()],
        scopes: Vec::new(),
        capabilities: Vec::new(),
        allowed_tools: Vec::new(),
        requires_approval_for: Vec::new(),
        public_key: String::new(),
        auth_provider: String::new(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime: "rust".to_string(),
        host: "local".to_string(),
    }
}

pub fn create_presence(identity: AgentIdentity, status: AgentStatus) -> AgentPresence {
    let now = Utc::now().timestamp_millis();
    AgentPresence {
        identity: Some(identity),
        status: status as i32,
        subscribes_to: Vec::new(),
        publishes_to: Vec::new(),
        started_unix_ms: now,
        last_seen_unix_ms: now,
        ttl_ms: 30000, // 30 seconds default TTL
    }
}
