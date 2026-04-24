# Twilight Bark Roadmap

This roadmap outlines the evolution of Twilight Bark from its MVP state to a production-ready, highly secure agent coordination fabric.

## Current State: Phase 1 (MVP) - 🟩 COMPLETED
*Focus: Core infrastructure and basic connectivity.*

- [x] Zenoh 1.9 (Longwang) Integration.
- [x] Protobuf-based message contracts.
- [x] Agent Identity & Presence registry.
- [x] Basic Traffic Controller with Unicast/Multicast routing.
- [x] MCP Server with core toolset (`publish_task`, `get_registry`).
- [x] Filesystem & Obsidian adapters.
- [x] JSONL Event Logging.

## Phase 2: Enhanced Observability & Management (Upcoming)
*Focus: Making the system easier to monitor and manage at scale.*

- [ ] **Nuze Integration**: Live dashboard for traffic and presence visualization.
- [ ] **Heartbeat Hardening**: Configurable TTLs and stale agent cleanup in Traffic Controller.
- [ ] **CLI Expansion**: Tools for manual message injection and registry inspection.
- [ ] **Unified Config**: Centralized configuration management for multi-site deployments.

## Phase 3: Advanced Routing & Security
*Focus: Hardening the fabric for untrusted environments.*

- [ ] **OpenZiti Integration**: Seamless Zero-Trust overlay for cross-cloud communication.
- [ ] **End-to-End Encryption**: Optional payload encryption for sensitive task data.
- [ ] **Capabilities Negotiation**: Fine-grained tool discovery based on agent permissions.
- [ ] **Priority Queuing**: Quality of Service (QoS) levels for mission-critical tasks.

## Phase 4: Scaling & Ecosystem
*Focus: Broadening reach and integration points.*

- [ ] **Python/JS Bridges**: Client libraries for non-Rust agent implementations.
- [ ] **Cloud Connectors**: Built-in adapters for AWS S3, Google Drive, and Notion.
- [ ] **Task Orchestrator**: High-level state machine for complex, multi-agent workflows.
- [ ] **Global Fabric**: Highly available, multi-tenant Zenoh bridging for global deployments.
