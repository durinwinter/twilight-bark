use twilight_proto::twilight::{AgentIdentity, NodeKind, AgentStatus, AgentPresence};
use uuid::Uuid;
use chrono::Utc;

/// Returns "{hostname}-{username}", used as the fabric-wide node identifier.
pub fn auto_node_id() -> String {
    let hostname = std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    format!("{}-{}", hostname, username)
}

/// Returns the default daemon Unix socket path, respecting XDG_RUNTIME_DIR on Linux.
pub fn default_socket_path() -> std::path::PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(runtime_dir).join("twilight-daemon.sock");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    std::path::PathBuf::from(format!("/tmp/twilight-{}-daemon.sock", user))
}

/// Creates an identity with auto-detected node_id and "default" tenant.
/// Use for local CLI scenarios and adapters that don't have daemon config.
pub fn create_default_identity(name: &str, role: &str) -> AgentIdentity {
    let node_id = auto_node_id();
    create_node_identity(name, role, &node_id, "default")
}

/// Creates an identity with an explicit node_id and tenant — used by the daemon
/// when it knows the configured node_id from daemon.toml.
pub fn create_node_identity(name: &str, role: &str, node_id: &str, tenant: &str) -> AgentIdentity {
    AgentIdentity {
        agent_uuid: Uuid::new_v4().to_string(),
        instance_uuid: Uuid::new_v4().to_string(),
        node_uuid: Uuid::new_v4().to_string(),
        agent_name: name.to_string(),
        display_name: name.to_string(),
        role: role.to_string(),
        role_description: String::new(),
        node_kind: NodeKind::Agent as i32,
        tenant: tenant.to_string(),
        site: node_id.to_string(),
        region: String::new(),
        node_id: node_id.to_string(),
        host: node_id.to_string(),
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
        ttl_ms: 30000,
    }
}
