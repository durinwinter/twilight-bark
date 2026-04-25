# Twilight Bark Roadmap

This roadmap outlines the evolution of Twilight Bark from its MVP state to a production-ready, highly secure agent coordination fabric.

## Phase 1 (MVP) - 🟩 COMPLETED
*Focus: Core infrastructure and basic connectivity.*

- [x] Zenoh 1.9 (Longwang) Integration.
- [x] Protobuf-based message contracts.
- [x] Agent Identity & Presence registry.
- [x] Basic Traffic Controller with Unicast/Multicast routing.
- [x] MCP Server with core toolset (`publish_task`, `get_registry`).
- [x] Filesystem & Obsidian adapters.
- [x] JSONL Event Logging.

## Phase 2 (Management & Hardening) - 🟩 COMPLETED
*Focus: Making the system easier to monitor and manage at scale.*

- [x] Nuze Integration (Signal Mirror).
- [x] Heartbeat Hardening & Automation.
- [x] CLI Expansion (Inspect & Inject).
- [x] Unified configuration example.

## Phase 3 (Daemon Architecture & Zero-Trust) - 🟩 COMPLETED
*Focus: Production-grade node model and network security infrastructure.*

- [x] **Persistent Daemon**: `twilight-daemon` owns the Zenoh session, Ziti tunnel sidecar, agent registry, and heartbeat loop. LLM clients connect via Unix socket IPC.
- [x] **Thin MCP Shim**: `twilight-mcp-server` rewritten as a stateless DaemonClient — no Zenoh, no Ziti. Supports stdio (Claude/LM Studio) and HTTP gateway (Antigravity) modes.
- [x] **OpenZiti Structure**: Ziti identity enrollment (`ziti edge enroll`), tunnel sidecar integration, hub and client node config templates.
- [x] **Node Identity**: `node_id = {hostname}-{username}` — fabric-wide unique identifier embedded in all presence/heartbeat/traffic messages.
- [x] **Cross-Node Visibility**: Wildcard subscriptions (`twilight/{tenant}/*/presence/*`) so the daemon and console see ALL agents across all nodes.
- [x] **Systemd Integration**: `install-service.sh` installs binaries and `twilight-daemon.service` user unit. `twilight-cli daemon {enroll,start,stop,status}` manages the lifecycle.
- [x] **Fabric Provisioning**: `provision-fabric.sh` creates Ziti identities and enrollment JWTs. `setup-ziti-infra.sh` bootstraps the controller and edge router on the hub.

## Phase 4 (The Twilight Console) - 🟩 COMPLETED
*Focus: Professional management suite for distributed agent fabrics.*

- [x] **Tauri-Powered GUI**: Dark-mode desktop app with sidebar navigation.
- [x] **Security Tab**: JWT enrollment UI, daemon start/stop toggle, Network ID config.
- [x] **Live Bus Tab**: Real-time scrolling feed of all traffic, presence, and heartbeat events.
- [x] **Traffic Analytics**: SVG Sankey diagram of agent-to-agent message flow, auto-refreshing every 3 s.
- [x] **Agents Tab**: Live card grid of all agents in the fabric with status, node, role, UUID, and last-seen.
- [x] **Bark Bus Admin**: Zenoh admin-space tree viewer (queries `zenoh/admin/**`).
- [x] **Management Suite**: Participant Factory (pre-generates node enrollment slots), Network Provisioner.
- [x] **Daemon Status Bar**: Sidebar indicator polls daemon health every 8 s via PID + socket liveness check.

## Phase 5 (Scaling & Ecosystem) - Upcoming
*Focus: Broadening reach and integration points.*

- [ ] **Python/JS Bridges**: Client libraries for non-Rust agent implementations.
- [ ] **Cloud Connectors**: Built-in adapters for AWS S3, Google Drive, and Notion.
- [ ] **Task Orchestrator**: High-level state machine for complex, multi-agent workflows.
- [ ] **AgentShield & Hook Architecture**: Decentralized tool security, `PreToolUse` and `PostToolUse` interception on the fabric, and red/blue vulnerability scanning of MCP definitions.
- [ ] **Standardized Agent Topologies**: Cross-LLM role standardization using explicitly shaped personas (e.g., planner, loop-operator) for predictable inter-agent coordination.
- [ ] **End-to-End Encryption**: Optional payload encryption for sensitive task data.
- [ ] **Capabilities Negotiation**: Fine-grained tool discovery based on agent permissions.
- [ ] **Priority Queuing**: Quality of Service (QoS) levels for mission-critical tasks.
- [ ] **Global Fabric**: Highly available, multi-tenant Zenoh bridging for global deployments.
