# Twilight Bark: Agent-to-Agent (A2A) Protocols

This document defines the standard patterns for registration, negotiation, and lifecycle management within the Twilight Bark distributed fabric.

## 1. Lifecycle Management

### 1.1 Registration (Onboarding)
Every agent MUST publish an `AgentPresence` message on its startup.
- **Topic**: `twilight/{tenant}/{site}/presence/{node_id}`
- **Payload**: `AgentPresence` containing full `AgentIdentity`.
- **Status**: Set to `AGENT_STATUS_ONLINE`.

### 1.2 Heartbeats (Vitality)
Agents MUST emit a `Heartbeat` every 10 seconds (default).
- **Topic**: `twilight/{tenant}/{site}/heartbeat/{node_id}`
- **Stale Threshold**: 30 seconds.

### 1.3 Deregistration (Offboarding)
Agents SHOULD gracefully deregister by publishing a final presence message with status `OFFLINE`.
- **Reason Tags**: `graceful-shutdown`, `error-exit`, etc.

## 2. A2A Negotiation Pattern

When Agent A needs to collaborate with Agent B on a sensitive task, it follows this handshake:

1. **Discovery**: Agent A queries the `TrafficController` (via MCP or direct subscription) to find agents with specific capabilities.
2. **Capability Check**: Agent A sends a `TaskRequest` with operation `ping_capabilities`.
3. **Formal Negotiation**:
   - Agent A sends a `ManagementCommand` with `negotiate_handshake`.
   - Agent B replies with a `ManagementResult` containing public keys or session parameters.
4. **Secure Tasking**: Subsequent `TaskRequest` envelopes may include encrypted metadata for the task.

## 3. Best Practices

- **Minimal Identity**: Only share required capabilities in the public `AgentIdentity`.
- **Self-Healing**: Use the `start_heartbeat_loop` utility to ensure presence is maintained automatically.
- **Topic Isolation**: Use granular topics for specific task types to reduce noise in high-volume fabrics.
