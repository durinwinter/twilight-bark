use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Hub,
    Client,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NodeConfig {
    /// Fabric-wide node identifier. Auto-populated as "{hostname}-{username}" if empty.
    pub name: String,
    pub role: NodeRole,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IdentityConfig {
    pub file: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ZitiConfig {
    pub enabled: bool,
    pub controller_url: String,
    pub service: String,
    pub local_port: u16,
    #[serde(default = "default_ziti_binary")]
    pub binary: String,
}

fn default_ziti_binary() -> String {
    "ziti".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ZenohConfig {
    pub tenant: String,
    #[serde(default)]
    pub peers: Vec<String>,
    #[serde(default)]
    pub listen: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DaemonSection {
    /// Unix socket path. Empty string → auto-detect via XDG_RUNTIME_DIR.
    pub socket: PathBuf,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl DaemonSection {
    pub fn resolved_socket(&self) -> PathBuf {
        if self.socket == PathBuf::from("") {
            twilight_core::default_socket_path()
        } else {
            self.socket.clone()
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DaemonConfig {
    pub node: NodeConfig,
    pub identity: IdentityConfig,
    pub ziti: ZitiConfig,
    pub zenoh: ZenohConfig,
    pub daemon: DaemonSection,
}

impl DaemonConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read config {:?}: {}", path, e))?;
        let mut config: DaemonConfig = toml::from_str(&content)?;

        // Auto-fill node_id if empty
        if config.node.name.is_empty() {
            config.node.name = twilight_core::auto_node_id();
        }

        // Expand ~ in paths
        config.identity.file = expand_path(&config.identity.file);
        let resolved = config.daemon.resolved_socket();
        config.daemon.socket = expand_path(&resolved);
        config.ziti.binary = config.ziti.binary.trim().to_string();
        if config.ziti.binary.is_empty() {
            config.ziti.binary = "ziti".to_string();
        }

        Ok(config)
    }

    pub fn node_id(&self) -> &str {
        &self.node.name
    }
}

/// Expands a leading `~/` to the home directory.
pub fn expand_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(&s[2..]);
        }
    }
    path.to_path_buf()
}
