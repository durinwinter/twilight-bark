# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build --workspace
cargo build -p <crate-name>    # e.g. twilight-cli, twilight-daemon

# Test
cargo test --workspace
cargo test -p <crate-name> <test_name>

# Check / Lint / Format
cargo check --workspace
cargo clippy --workspace
cargo fmt --all

# Run CLI
cargo run -p twilight-cli -- run --name "my-node"
cargo run -p twilight-cli -- list
cargo run -p twilight-cli -- daemon status
```

Set `RUST_LOG=debug` (or `info`, `trace`) to enable logging output.

## Phase Test

```bash
# Full end-to-end test
./scripts/phase-test.sh

# Fabric-only smoke test (no daemon or LLM clients needed)
./scripts/phase-test.sh --smoke

# Build only
./scripts/phase-test.sh --build
```

## Node Onboarding (one-time per machine)

```bash
# 1. On the hub node — provision fabric and create a JWT for this node
scripts/provision-fabric.sh --add-node $(hostname)-$(whoami)
# → outputs config/enrollments/<node-id>.jwt

# 2. On the new node — install binaries + systemd unit
scripts/install-service.sh

# 3. Enroll Ziti identity
twilight-cli daemon enroll --jwt ~/my-node.jwt

# 4. Edit config
cp config/daemon-client.toml ~/.config/twilight/daemon.toml
$EDITOR ~/.config/twilight/daemon.toml   # set node.name, ziti.controller_url

# 5. Start daemon
systemctl --user enable --now twilight-daemon
twilight-cli daemon status
```

For the hub node, use `config/daemon-hub.toml` and run `scripts/provision-fabric.sh` (no `--add-node`) first.

## MCP Client Configuration (one-time setup)

The `twilight-mcp-server` binary **requires a running daemon** — it connects via Unix socket.
Start the daemon first, then configure LLM clients as before.

| Client | Config file | Agent name | Socket env var |
|---|---|---|---|
| **Claude Code** | `.claude/settings.json` | `claude` | `TWILIGHT_DAEMON_SOCKET` |
| **LM Studio** | `~/.lmstudio/mcp.json` | `lmstudio` | `TWILIGHT_DAEMON_SOCKET` |
| **Antigravity** | `~/.gemini/antigravity/mcp_config.json` | `antigravity` | `TWILIGHT_DAEMON_SOCKET` |

Socket path defaults to `$XDG_RUNTIME_DIR/twilight-daemon.sock` (Linux with systemd) or
`/tmp/twilight-{username}-daemon.sock`. Override with `--socket` or `TWILIGHT_DAEMON_SOCKET`.

## Architecture

Twilight Bark is a distributed agent communication fabric. Agents discover each other, exchange
tasks, and coordinate via **Zenoh** pub/sub transport secured by **OpenZiti** zero-trust overlay.
Messages are **Protobuf 3** encoded.

### Node Architecture

Each physical node runs one long-lived **twilight-daemon** process. LLM clients connect
ephemeral MCP shims to the daemon over a Unix socket. The daemon owns the Zenoh session,
Ziti tunnel sidecar, agent registry, and heartbeat loop.

```
LLM Client (Claude / LM Studio / Antigravity)
  ↓ stdio or HTTP MCP
twilight-mcp-server  (thin shim — no Zenoh, no Ziti)
  ↓ Unix socket IPC
twilight-daemon  (persistent — owns Zenoh session + registry)
  ↓ Zenoh router link via ziti-tunnel proxy
Ziti Overlay  (zero-trust mTLS, controller at https://pop-os:6262)
  ↓
Hub node Zenoh router  (tcp/0.0.0.0:7447)
```

### Topic Namespace

```
twilight/{tenant}/{node_id}/{message_type}/{id}
```

- `tenant` — shared fabric name (e.g. `twilight-bark`)
- `node_id` — `{hostname}-{username}`, unique per user per machine
- Daemon subscribes to `twilight/{tenant}/*/presence/*` for cross-node registry

### IPC Protocol (daemon ↔ MCP shim)

JSON-lines over Unix socket. First message must be `register`:

```json
→ {"cmd":"register","name":"claude","role":"mcp-agent"}
← {"ok":true,"agent_uuid":"<uuid>"}
→ {"cmd":"get_registry"}
← {"ok":true,"agents":[...]}
→ {"cmd":"publish_task","operation":"bark_echo","input_json":"{}"}
← {"ok":true,"task_id":"<uuid>"}
→ {"cmd":"ask_agent","agent_uuid":"<uuid>","operation":"ping","input_json":"{}"}
← {"ok":true,"task_id":"<uuid>"}
```

### Crate Layers (bottom → top)

| Crate | Role |
|---|---|
| `twilight-proto` | Protobuf schema + prost-generated types. All types derive serde. Key types: `AgentIdentity` (includes `node_id` field), `AgentPresence`, `TwilightEnvelope`, `TaskRequest/Result`. |
| `twilight-core` | Pure utilities: `create_default_identity()`, `create_node_identity()`, `auto_node_id()`, `default_socket_path()`, `create_presence()`. No I/O. |
| `twilight-ziti` | Ziti integration: `ZitiTunnel::build_args()` for sidecar process args, `enroll()` shells out to `ziti edge enroll`. |
| `twilight-bus` | Zenoh transport. `new()` = peer mode. `new_with_config(json)` = client/router mode. `subscribe_all_presence/heartbeats()` use wildcards for cross-node visibility. |
| `twilight-traffic-controller` | Agent registry (`DashMap`). `update_presence/heartbeat`, `get_identity`, `remove_agent`, `get_all_identities`, `run_cleanup_loop`. |
| `twilight-daemon` | Persistent daemon binary. Starts Ziti tunnel sidecar, opens Zenoh session, runs IPC server, owns registry and heartbeat loop. Config: `~/.config/twilight/daemon.toml`. |
| `twilight-mcp-server` | Thin MCP shim. Connects to daemon socket via `DaemonClient`. Supports stdio (Claude Code, LM Studio) and HTTP Streamable (Antigravity). No Zenoh code. |
| `twilight-eventlog` | Appends `TwilightEnvelope` as JSONL for audit/observability. |
| `twilight-cli` | CLI binary. Subcommands: `run`, `list`, `smoke-test`, `scenario-a2a`, `scenario-dogs`, `inject`, `daemon {enroll,start,stop,status}`. |

### Adapters

| Adapter | Role |
|---|---|
| `filesystem-adapter` | Scans a directory tree every 10 minutes; publishes file listings as `Observation` messages. |
| `obsidian-adapter` | Scans an Obsidian markdown vault every 5 minutes; extracts YAML frontmatter and publishes note metadata as `Observation` messages. |

### Message Flow

```
LLM / MCP client
  → twilight-mcp-server  (DaemonClient IPC call)
  → twilight-daemon IPC server
  → twilight-bus.publish_envelope()   (Protobuf → Zenoh)
  → Zenoh transport  (cross-node via Ziti overlay)
  → receiving daemon(s)
  → twilight-traffic-controller  (registry update)
  → twilight-eventlog  (optional JSONL audit)
```

### Workspace Layout

```
Cargo.toml               # workspace root
config/
  twilight-net.yaml      # Ziti controller config (written by setup-ziti-infra.sh)
  daemon-hub.toml        # Hub node daemon config template
  daemon-client.toml     # Client node daemon config template
  enrollments/           # Per-node enrollment JWTs (gitignored)
crates/
  twilight-proto/        # proto/ subdir holds .proto files
  twilight-core/
  twilight-ziti/
  twilight-bus/
  twilight-traffic-controller/
  twilight-daemon/       # New — persistent daemon binary
  twilight-mcp-server/
  twilight-eventlog/
  twilight-cli/
adapters/
  filesystem/
  obsidian/
twilight-console/        # Tauri desktop app
scripts/
  phase-test.sh
  install-service.sh     # New — installs binaries + systemd unit
  provision-fabric.sh    # New/updated — Ziti service + identity provisioning
  setup-ziti-infra.sh    # Controller + edge router bootstrap (hub only)
  templates/
    twilight-daemon.service   # systemd user unit template
```

All async I/O uses Tokio. Error handling uses `anyhow::Result` throughout.
