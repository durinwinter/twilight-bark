use twilight_proto::twilight::{AgentPresence, AgentIdentity, MessageTarget, TargetKind, Heartbeat, TwilightEnvelope};
use dashmap::DashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use log::{debug, info};
use serde::Serialize;

pub struct RegistryEntry {
    pub identity: AgentIdentity,
    pub last_seen: DateTime<Utc>,
    pub status: i32,
}

#[derive(Serialize, Clone)]
pub struct AnalyticsEdge {
    pub source: String,
    pub target: String,
    pub weight: u64,
}

#[derive(Serialize, Clone)]
pub struct AnalyticsSnapshot {
    pub nodes: Vec<String>,
    pub edges: Vec<AnalyticsEdge>,
}

pub struct TrafficController {
    registry: Arc<DashMap<String, RegistryEntry>>,
    // (Source Node ID, Target Node ID) -> Message Count
    traffic_matrix: Arc<DashMap<(String, String), u64>>,
}

impl TrafficController {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(DashMap::new()),
            traffic_matrix: Arc::new(DashMap::new()),
        }
    }

    pub fn update_presence(&self, presence: AgentPresence) {
        if let Some(identity) = presence.identity {
            let node_id = identity.node_uuid.clone();
            debug!("Updating presence for node: {}", node_id);
            self.registry.insert(node_id, RegistryEntry {
                identity,
                last_seen: Utc::now(),
                status: presence.status,
            });
        }
    }

    pub fn update_heartbeat(&self, heartbeat: Heartbeat) {
        if let Some(mut entry) = self.registry.get_mut(&heartbeat.node_id) {
            debug!("Heartbeat received for node: {}", heartbeat.node_id);
            entry.last_seen = Utc::now();
        }
    }

    pub fn record_traffic(&self, envelope: &TwilightEnvelope) {
        let source_id = envelope.source.as_ref().map(|id| id.node_uuid.clone()).unwrap_or_else(|| "unknown".to_string());
        
        let targets = if let Some(ref target) = envelope.target {
            self.get_targets(target)
        } else {
            // Default to broadcast if no target specified? Or skip?
            self.registry.iter().map(|e| e.key().clone()).collect()
        };

        for target_id in targets {
            let mut counter = self.traffic_matrix.entry((source_id.clone(), target_id)).or_insert(0);
            *counter += 1;
        }
    }

    pub fn get_analytics_snapshot(&self) -> AnalyticsSnapshot {
        let mut nodes = std::collections::HashSet::new();
        let mut edges = Vec::new();

        for entry in self.traffic_matrix.iter() {
            let (source, target) = entry.key();
            nodes.insert(source.clone());
            nodes.insert(target.clone());
            edges.push(AnalyticsEdge {
                source: source.clone(),
                target: target.clone(),
                weight: *entry.value(),
            });
        }

        AnalyticsSnapshot {
            nodes: nodes.into_iter().collect(),
            edges,
        }
    }

    pub fn cleanup_stale_agents(&self, timeout_seconds: i64) -> Vec<String> {
        let now = Utc::now();
        let timeout = Duration::seconds(timeout_seconds);
        let mut stale_ids = Vec::new();

        for entry in self.registry.iter() {
            if now.signed_duration_since(entry.value().last_seen) > timeout {
                stale_ids.push(entry.key().clone());
            }
        }

        for id in &stale_ids {
            info!("Removing stale agent: {}", id);
            self.registry.remove(id);
        }

        stale_ids
    }

    pub fn get_targets(&self, target: &MessageTarget) -> Vec<String> {
        let mut recipients = Vec::new();
        
        match target.target_kind() {
            TargetKind::Unicast => {
                recipients.extend(target.target_agent_uuids.clone());
            },
            TargetKind::Role => {
                for entry in self.registry.iter() {
                    if target.target_roles.iter().any(|r| entry.value().identity.roles.contains(r)) {
                        recipients.push(entry.key().clone());
                    }
                }
            },
            TargetKind::Capability => {
                for entry in self.registry.iter() {
                    if target.target_capabilities.iter().any(|c| entry.value().identity.capabilities.contains(c)) {
                        recipients.push(entry.key().clone());
                    }
                }
            },
            _ => {
                // Broadcast or unspecified
                for entry in self.registry.iter() {
                    recipients.push(entry.key().clone());
                }
            }
        }
        
        recipients
    }

    pub fn get_all_identities(&self) -> Vec<AgentIdentity> {
        self.registry.iter().map(|entry| entry.value().identity.clone()).collect()
    }

    pub async fn run_cleanup_loop(self: Arc<Self>, interval_ms: u64, timeout_seconds: i64) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));
        loop {
            interval.tick().await;
            self.cleanup_stale_agents(timeout_seconds);
        }
    }
}
