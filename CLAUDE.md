# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build --workspace
cargo build -p <crate-name>    # e.g. twilight-cli, twilight-bus

# Test
cargo test --workspace
cargo test -p <crate-name> <test_name>   # run a single test

# Check / Lint / Format
cargo check --workspace
cargo clippy --workspace
cargo fmt --all

# Run CLI
cargo run -p twilight-cli -- run --name "my-node"
cargo run -p twilight-cli -- list
```

Set `RUST_LOG=debug` (or `info`, `trace`) to enable logging output.

## Architecture

Twilight Bark is a distributed agent communication fabric. Agents discover each other, exchange tasks, share observations, and coordinate via **Zenoh** pub/sub transport. Messages are **Protobuf 3** encoded.

### Topic Namespace

All traffic flows through Zenoh topics structured as:
```
twilight/{tenant}/{site}/{message_type}/{id}
```

### Crate Layers (bottom → top)

| Crate | Role |
|---|---|
| `twilight-proto` | Message contracts — Protobuf schema + `prost`-generated Rust types. `build.rs` compiles `.proto` → Rust at build time. Key types: `AgentIdentity`, `AgentPresence`, `TwilightEnvelope`, `TaskRequest/Result`, `Observation`, `MessageTarget`. |
| `twilight-core` | Pure data utilities: `create_default_identity()` (UUID-assigned `AgentIdentity`) and `create_presence()` (30-second TTL). No I/O. |
| `twilight-bus` | Transport abstraction over a Zenoh session. `publish_envelope()`, `publish_presence()`, `publish_heartbeat()`, `subscribe_presence()`. |
| `twilight-traffic-controller` | In-memory agent registry (`DashMap`). `update_presence()` registers agents; `get_targets()` resolves `MessageTarget` routing (unicast by UUID, role-based, capability-based, broadcast). |
| `twilight-mcp-server` | MCP gateway for LLM agents. `publish_task()` / `ask_agent()` send `TaskRequest` envelopes; `get_registry()` lists known agents. |
| `twilight-eventlog` | Appends `TwilightEnvelope` as JSONL to a file for audit/observability. |
| `twilight-cli` | Binary entrypoint (`clap`). `run` starts a named node; `list` is a registry query placeholder. |
| `twilight-management` | Stub — reserved for future admin operations. |

### Adapters

| Adapter | Role |
|---|---|
| `filesystem-adapter` | Scans a directory tree every 10 minutes; publishes file listings as `Observation` messages. |
| `obsidian-adapter` | Scans an Obsidian markdown vault every 5 minutes; extracts YAML frontmatter via `gray_matter` and publishes note metadata as `Observation` messages. |

### Message Flow

```
LLM / MCP client
  → twilight-mcp-server.publish_task()
  → twilight-bus.publish_envelope()    (serializes to Protobuf, publishes on Zenoh)
  → Zenoh transport
  → twilight-traffic-controller        (routing resolution)
  → receiving agents
  → twilight-eventlog                  (optional JSONL audit trail)
```

### Workspace Layout

```
Cargo.toml               # workspace root, shared [workspace.dependencies]
crates/
  twilight-proto/        # proto/ subdir holds .proto files
  twilight-core/
  twilight-bus/
  twilight-traffic-controller/
  twilight-mcp-server/
  twilight-eventlog/
  twilight-cli/
  twilight-management/
adapters/
  filesystem-adapter/
  obsidian-adapter/
```

All async I/O uses Tokio. Error handling uses `anyhow::Result` throughout.
