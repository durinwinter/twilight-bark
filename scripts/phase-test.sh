#!/usr/bin/env bash
# Twilight Bark — End-to-End Phase Test
#
# Verifies that the full agent communication pipeline works across all three
# LLM clients (Claude Code, LM Studio, Antigravity) on a single machine.
#
# Usage:
#   ./scripts/phase-test.sh             # full test (requires Tauri console running)
#   ./scripts/phase-test.sh --smoke     # quick smoke test only (no LLM clients needed)
#   ./scripts/phase-test.sh --build     # build only

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/debug/twilight-mcp-server"
DAEMON_BIN="$REPO/target/debug/twilight-daemon"
CLI_BIN="$REPO/target/debug/twilight-cli"
DAEMON_CONFIG="${TWILIGHT_CONFIG:-$HOME/.config/twilight/daemon.toml}"
DAEMON_SOCKET="${XDG_RUNTIME_DIR:-/tmp}/twilight-$(whoami)-daemon.sock"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }
info() { echo -e "${CYAN}[INFO]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }

PIDS=()
cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║      Twilight Bark — End-to-End Phase Test           ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""

# ── 1. BUILD ────────────────────────────────────────────────
info "Building workspace..."
cargo build -p twilight-daemon -p twilight-mcp-server -p twilight-cli \
    --manifest-path "$REPO/Cargo.toml" 2>&1 | tail -3

if [[ ! -x "$BIN" ]]; then
    fail "twilight-mcp-server binary not found at $BIN"; exit 1
fi
if [[ ! -x "$DAEMON_BIN" ]]; then
    fail "twilight-daemon binary not found at $DAEMON_BIN"; exit 1
fi
pass "Build succeeded"

if [[ "${1:-}" == "--build" ]]; then
    info "Build-only mode. Done."
    exit 0
fi

# ── 2. DAEMON HEALTH CHECK ──────────────────────────────────
info "Checking daemon status..."
if "$CLI_BIN" daemon status --socket "$DAEMON_SOCKET" 2>/dev/null | grep -q "reachable"; then
    pass "Daemon is running and socket is reachable"
    DAEMON_RUNNING=1
else
    warn "Daemon not running — smoke test will use direct Zenoh (peer mode)"
    DAEMON_RUNNING=0
fi

# ── 3. FABRIC SMOKE TEST ────────────────────────────────────
info "Running fabric smoke test (2-bus round-trip via Zenoh)..."
if RUST_LOG=warn "$CLI_BIN" smoke-test; then
    pass "Fabric smoke test"
else
    fail "Fabric smoke test — Zenoh may not be reachable"
    exit 1
fi

if [[ "${1:-}" == "--smoke" ]]; then
    info "Smoke-only mode. Done."
    exit 0
fi

# ── 3. START BUS OBSERVER ────────────────────────────────────
info "Starting bus observer (scenario-a2a in background for 30s)..."
RUST_LOG=warn "$CLI_BIN" scenario-a2a &
OBSERVER_PID=$!
PIDS+=("$OBSERVER_PID")
sleep 1

# ── 4. MCP CLIENT VERIFICATION ──────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " MCP Client Setup Checklist"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
info "Binary path (used by all three clients):"
echo "    $BIN"
echo ""
echo "  ┌─ CLAUDE CODE ────────────────────────────────────"
echo "  │  Config: $REPO/.claude/settings.json"
echo "  │  Agent name: claude"
echo "  │  Transport: stdio (auto-launched per session)"
echo "  │  Status: $([ -f "$REPO/.claude/settings.json" ] && echo '✓ configured' || echo '✗ missing')"
echo "  │"
echo "  ├─ LM STUDIO ──────────────────────────────────────"
echo "  │  Config: $HOME/.lmstudio/mcp.json"
echo "  │  Agent name: lmstudio"
echo "  │  Transport: stdio"
echo "  │  Status: $(grep -q 'twilight-bark' "$HOME/.lmstudio/mcp.json" 2>/dev/null && echo '✓ configured' || echo '✗ missing — run scripts/phase-test.sh again after setup')"
echo "  │"
echo "  └─ ANTIGRAVITY ────────────────────────────────────"
echo "     Config: $HOME/.gemini/antigravity/mcp_config.json"
echo "     Agent name: antigravity"
echo "     Transport: stdio (bash wrapper)"
echo "     Status: $(grep -q 'twilight-mcp-server' "$HOME/.gemini/antigravity/mcp_config.json" 2>/dev/null && echo '✓ configured' || echo '✗ missing')"
echo ""

# ── 5. SCENARIO TEST ─────────────────────────────────────────
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Inter-Agent Communication Test"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
info "Starting the Dogs scenario (4-agent multi-provider demo) in background..."
RUST_LOG=warn "$CLI_BIN" scenario-dogs &
DOGS_PID=$!
PIDS+=("$DOGS_PID")

sleep 3
info "The Dogs scenario is running. Open the Twilight Console to see agents communicating."
echo ""
echo "  Tauri console: cd $REPO/twilight-console && npm run tauri dev"
echo ""

# ── 6. LLM PROMPT ────────────────────────────────────────────
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Manual LLM Verification Steps"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  1. In Antigravity, ask: 'Use the twilight-bark tool to list all agents'"
echo "     → Should return the 4 dog agents + any MCP gateway agents"
echo ""
echo "  2. In LM Studio, ask: 'Use publish_task to send operation=bark_echo'"
echo "     → Should return a task_id"
echo ""
echo "  3. In Claude Code, use: get_registry"
echo "     → Should show all agents including lmstudio and antigravity gateways"
echo ""
echo "  4. Check Twilight Console → Monitor tab for all three LLM agents appearing"
echo ""
echo "Press Ctrl+C when done."

wait "$DOGS_PID" 2>/dev/null || true
