#!/usr/bin/env bash
# loop-test.sh — E2E smoke test: Claude CLI → fabric → LM Studio bridge → reply
#
# Prerequisites:
#   • twilight-daemon running  (twilight-cli daemon status)
#   • LM Studio running with its REST API on :1234
#
# Usage:
#   ./scripts/loop-test.sh
#   LMS_MODEL=my-model ./scripts/loop-test.sh
#   TWILIGHT_DAEMON_SOCKET=/run/user/1000/twilight-daemon.sock ./scripts/loop-test.sh

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE_PY="$REPO/scripts/lmstudio-bridge.py"
BINARY="$REPO/target/debug/twilight-cli"
SOCK="${TWILIGHT_DAEMON_SOCKET:-/run/user/$(id -u)/twilight-daemon.sock}"
MAX_WAIT=90   # seconds to wait for a reply
BRIDGE_PID=""

cleanup() {
    [[ -n "$BRIDGE_PID" ]] && kill "$BRIDGE_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ── 1. Sanity checks ──────────────────────────────────────────────────────────

echo "[loop-test] Checking prerequisites…"

if ! "$BINARY" daemon status 2>&1 | grep -q "reachable ✓"; then
    echo "[loop-test] FAIL — daemon not running. Start with:"
    echo "  twilight-cli daemon start --config ~/.config/twilight/daemon.toml"
    exit 1
fi

if ! curl -sf "http://localhost:1234/v1/models" >/dev/null 2>&1; then
    echo "[loop-test] FAIL — LM Studio REST API not reachable on :1234"
    exit 1
fi

echo "[loop-test] Prerequisites OK"

# ── 2. Build if needed ────────────────────────────────────────────────────────

if [[ ! -f "$BINARY" ]]; then
    echo "[loop-test] Building twilight-cli…"
    cargo build -p twilight-cli 2>&1
fi

# ── 3. Start bridge ───────────────────────────────────────────────────────────

# Kill any leftover bridge
pkill -f lmstudio-bridge.py 2>/dev/null || true
sleep 0.5

echo "[loop-test] Starting LM Studio bridge…"
TWILIGHT_DAEMON_SOCKET="$SOCK" python3 "$BRIDGE_PY" &
BRIDGE_PID=$!

# Wait until 'lmstudio' appears in the daemon registry (up to 10 s)
echo -n "[loop-test] Waiting for bridge registration"
for i in $(seq 1 20); do
    sleep 0.5
    if "$BINARY" daemon status 2>/dev/null | grep -q "reachable"; then
        # Quick registry probe
        AGENTS=$(python3 - <<'PYEOF'
import socket, json, os, sys

SOCK = os.environ.get("TWILIGHT_DAEMON_SOCKET",
    f"/run/user/{os.getuid()}/twilight-daemon.sock")
try:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    s.settimeout(2)
    def send(m): s.sendall((json.dumps(m)+"\n").encode())
    def recv():
        buf=b""
        while True:
            try:
                c=s.recv(4096); buf+=c
                if b"\n" in buf:
                    line,_=buf.split(b"\n",1); return json.loads(line)
            except: return {}
    send({"cmd":"register","name":"loop-probe","role":"op"}); recv()
    send({"cmd":"get_registry"}); r=recv()
    names=[a["agent_name"] for a in r.get("agents",[])]
    print(" ".join(names))
    s.close()
except Exception as e:
    print("", file=sys.stderr)
PYEOF
)
        if echo "$AGENTS" | grep -q "lmstudio"; then
            echo " ✓"
            break
        fi
    fi
    echo -n "."
    if [[ $i -eq 20 ]]; then
        echo ""
        echo "[loop-test] FAIL — bridge never registered (lmstudio not in registry after 10s)"
        exit 1
    fi
done

# ── 4. Send task and wait for reply ───────────────────────────────────────────

echo "[loop-test] Sending loop_ping to lmstudio…"

OUTPUT=$("$BINARY" agent send \
    --name claude-test \
    --operation loop_ping \
    --input '{"msg":"say the word PONG and nothing else /no_think"}' \
    --target lmstudio 2>&1)

echo "$OUTPUT"

if echo "$OUTPUT" | grep -q "REPLY RECEIVED.*success=true"; then
    echo ""
    echo "[loop-test] PASS ✓"
    exit 0
else
    echo ""
    echo "[loop-test] FAIL ✗ — no successful reply received"
    exit 1
fi
