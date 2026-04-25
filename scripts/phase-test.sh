#!/usr/bin/env bash
# Twilight Bark — End-to-End Phase Test
#
# Automated test: starts a daemon, two CLI agents, drives a full
# task request → reply round-trip through the daemon IPC, and verifies.
#
# Usage:
#   ./scripts/phase-test.sh             # full daemon-based E2E test
#   ./scripts/phase-test.sh --smoke     # Zenoh peer-mode round-trip only
#   ./scripts/phase-test.sh --build     # build only

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLI="$REPO/target/debug/twilight-cli"
DAEMON="$REPO/target/debug/twilight-daemon"
MCP="$REPO/target/debug/twilight-mcp-server"

TEST_CONFIG="$REPO/config/daemon-e2e-test.toml"
DAEMON_SOCKET="${XDG_RUNTIME_DIR:-/tmp}/twilight-$(whoami)-daemon.sock"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; EXIT_CODE=1; }
info() { echo -e "${CYAN}[INFO]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }

EXIT_CODE=0
PIDS=()
LISTENER_LOG="$(mktemp /tmp/twilight-e2e-listener.XXXXXX)"
SENDER_LOG="$(mktemp /tmp/twilight-e2e-sender.XXXXXX)"

cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    # Stop any daemon started by this script
    if [[ -n "${DAEMON_PID:-}" ]]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        sleep 0.3
        rm -f "$DAEMON_SOCKET" "${DAEMON_SOCKET%.sock}.pid" 2>/dev/null || true
    fi
    rm -f "$LISTENER_LOG" "$SENDER_LOG" 2>/dev/null || true
}
trap cleanup EXIT

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Twilight Bark — End-to-End Phase Test                ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ── 1. BUILD ──────────────────────────────────────────────────────────────────
info "Building workspace (daemon + cli + mcp-server)..."
cargo build -p twilight-daemon -p twilight-cli -p twilight-mcp-server \
    --manifest-path "$REPO/Cargo.toml" 2>&1 | tail -3

[[ -x "$CLI"    ]] || { fail "twilight-cli not found at $CLI";    exit 1; }
[[ -x "$DAEMON" ]] || { fail "twilight-daemon not found at $DAEMON"; exit 1; }
[[ -x "$MCP"    ]] || { fail "twilight-mcp-server not found at $MCP"; exit 1; }
pass "Build succeeded"

[[ "${1:-}" == "--build" ]] && { info "Build-only mode. Done."; exit 0; }

# ── 2. ZENOH SMOKE TEST (peer mode, no daemon) ────────────────────────────────
info "Running Zenoh smoke test (2-bus peer-mode round-trip)..."
if RUST_LOG=warn "$CLI" smoke-test 2>&1; then
    pass "Zenoh smoke test"
else
    fail "Zenoh smoke test — Zenoh peer mode is not working"
    exit 1
fi

[[ "${1:-}" == "--smoke" ]] && { info "Smoke-only mode. Done."; exit 0; }

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Daemon-Based E2E Test (IPC path)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── 3. START DAEMON ───────────────────────────────────────────────────────────
if "$CLI" daemon status --socket "$DAEMON_SOCKET" 2>/dev/null | grep -q "reachable"; then
    info "Using already-running daemon at $DAEMON_SOCKET"
    DAEMON_PID=""
else
    info "Starting test daemon (config: $TEST_CONFIG)..."
    RUST_LOG=info "$DAEMON" --config "$TEST_CONFIG" &
    DAEMON_PID=$!

    # Wait up to 5s for the socket to appear
    for i in $(seq 1 10); do
        sleep 0.5
        if [[ -S "$DAEMON_SOCKET" ]]; then
            pass "Daemon socket ready ($DAEMON_SOCKET)"
            break
        fi
        if [[ $i -eq 10 ]]; then
            fail "Daemon did not create socket after 5s"
            exit 1
        fi
    done
fi

# Verify daemon responds to ping
if "$CLI" daemon status --socket "$DAEMON_SOCKET" 2>/dev/null | grep -q "reachable"; then
    pass "Daemon IPC reachable"
else
    fail "Daemon not responding at $DAEMON_SOCKET"
    exit 1
fi

# ── 4. START LISTENER AGENT ──────────────────────────────────────────────────
info "Starting listener agent (auto-reply mode)..."
RUST_LOG=warn "$CLI" agent listen \
    --name "e2e-listener" \
    --role "worker" \
    --auto-reply \
    --socket "$DAEMON_SOCKET" \
    > "$LISTENER_LOG" 2>&1 &
LISTENER_PID=$!
PIDS+=("$LISTENER_PID")

sleep 1  # let it register and subscribe

# ── 5. SENDER: TARGETED A2A TASK ─────────────────────────────────────────────
info "Sender: looking up e2e-listener in registry..."
AGENT_LIST=$("$CLI" agent send \
    --name "e2e-sender" \
    --operation "analyze" \
    --input '{"target":"e2e-test","depth":2}' \
    --target "e2e-listener" \
    --socket "$DAEMON_SOCKET" \
    2>&1)

echo "$AGENT_LIST"
echo "$AGENT_LIST" > "$SENDER_LOG"

# ── 6. VERIFY RESULTS ────────────────────────────────────────────────────────
echo ""
info "Listener output:"
cat "$LISTENER_LOG"

echo ""
info "Verifying exchange..."

if grep -q "RECEIVED task" "$LISTENER_LOG"; then
    pass "Listener received the task"
else
    fail "Listener did NOT receive the task"
fi

if grep -q "replied" "$LISTENER_LOG"; then
    pass "Listener sent auto-reply"
else
    fail "Listener did NOT send reply"
fi

if grep -q "REPLY RECEIVED" "$SENDER_LOG"; then
    pass "Sender received the reply"
else
    fail "Sender did NOT receive reply"
fi

# ── 7. BROADCAST TEST ────────────────────────────────────────────────────────
echo ""
info "Broadcast test (no --target, all subscribers receive)..."

RUST_LOG=warn "$CLI" agent send \
    --name "e2e-broadcaster" \
    --operation "bark_echo" \
    --input '{"message":"howl"}' \
    --socket "$DAEMON_SOCKET" \
    > "$SENDER_LOG" 2>&1

sleep 1
cat "$SENDER_LOG"

if grep -q "Dispatched" "$SENDER_LOG"; then
    pass "Broadcast task dispatched"
else
    fail "Broadcast task dispatch failed"
fi

# ── 8. MCP CONFIG CHECKLIST ───────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " MCP Client Configuration Status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  MCP binary:   $MCP"
echo "  Daemon socket: $DAEMON_SOCKET"
echo ""
echo "  ┌─ CLAUDE CODE ────────────────────────────────────────"
echo "  │  Config: $REPO/.claude/settings.json"
echo "  │  Status: $([ -f "$REPO/.claude/settings.json" ] && grep -q twilight "$REPO/.claude/settings.json" && echo '✓ configured' || echo '✗ missing')"
echo "  │"
echo "  ├─ LM STUDIO ──────────────────────────────────────────"
echo "  │  Config: $HOME/.lmstudio/mcp.json"
echo "  │  Status: $(grep -q 'twilight' "$HOME/.lmstudio/mcp.json" 2>/dev/null && echo '✓ configured' || echo '✗ not configured')"
echo "  │"
echo "  └─ ANTIGRAVITY ────────────────────────────────────────"
echo "     Config: $HOME/.gemini/antigravity/mcp_config.json"
echo "     Status: $(grep -q 'twilight' "$HOME/.gemini/antigravity/mcp_config.json" 2>/dev/null && echo '✓ configured' || echo '✗ not configured')"
echo ""

# ── 9. SUMMARY ────────────────────────────────────────────────────────────────
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [[ $EXIT_CODE -eq 0 ]]; then
    echo -e "${GREEN}  ALL CHECKS PASSED — fabric E2E test complete${NC}"
else
    echo -e "${RED}  SOME CHECKS FAILED — see output above${NC}"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

exit $EXIT_CODE
