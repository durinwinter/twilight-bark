#!/usr/bin/env bash
# Twilight Bark — Node install script
# Installs binaries, systemd units, and MCP wrapper scripts.
# Handles full boot-order chain: zenohd → twilight-daemon → lmstudio-bridge.
# Run once per node after `cargo build --release`.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_BIN="$REPO/target/release"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
info() { echo -e "${CYAN}[--]${NC} $*"; }
warn() { echo -e "${YELLOW}[!!]${NC} $*"; }
die()  { echo -e "${RED}[!!]${NC} $*" >&2; exit 1; }

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║   Twilight Bark — Node Installation                  ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""

# ── 1. Build release binaries ────────────────────────────────────────────────

info "Building release binaries..."
cargo build --release -p twilight-daemon -p twilight-mcp-server -p twilight-cli \
    --manifest-path "$REPO/Cargo.toml"
ok "Build complete"

# ── 2. Install binaries to ~/.cargo/bin ──────────────────────────────────────

CARGO_BIN="${HOME}/.cargo/bin"
mkdir -p "$CARGO_BIN"
for bin in twilight-daemon twilight-mcp-server twilight-cli; do
    cp "$RELEASE_BIN/$bin" "$CARGO_BIN/$bin"
    ok "Installed $bin → $CARGO_BIN/$bin"
done

# ── 3. Install bridge script to ~/.local/lib/twilight-bark/ ──────────────────

BRIDGE_LIB="${HOME}/.local/lib/twilight-bark"
mkdir -p "$BRIDGE_LIB"
cp "$REPO/scripts/lmstudio-bridge.py" "$BRIDGE_LIB/lmstudio-bridge.py"
chmod +x "$BRIDGE_LIB/lmstudio-bridge.py"
ok "Installed lmstudio-bridge.py → $BRIDGE_LIB/"

# ── 4. Install MCP wrapper scripts (point to installed binary) ───────────────

LOCAL_BIN="${HOME}/.local/bin"
mkdir -p "$LOCAL_BIN"

for agent in claude lmstudio antigravity; do
    wrapper="$LOCAL_BIN/twilight-mcp-$agent"
    cat > "$wrapper" <<EOF
#!/usr/bin/env bash
exec "$CARGO_BIN/twilight-mcp-server" "\$@"
EOF
    chmod +x "$wrapper"
    ok "Installed MCP wrapper → $wrapper"
done

# ── 5. Install ziti CLI binary ────────────────────────────────────────────────

if command -v ziti &>/dev/null; then
    ok "ziti already installed at $(command -v ziti)"
else
    info "Downloading ziti CLI binary..."
    ARCH=$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
    ZITI_VERSION=$(curl -fsSL https://api.github.com/repos/openziti/ziti/releases/latest \
        | grep '"tag_name"' | head -1 | cut -d'"' -f4)
    ZITI_URL="https://github.com/openziti/ziti/releases/download/${ZITI_VERSION}/ziti-linux-${ARCH}-${ZITI_VERSION}.tar.gz"
    info "Downloading ${ZITI_URL}"
    curl -fsSL "$ZITI_URL" | tar -xz -C /usr/local/bin ziti
    chmod +x /usr/local/bin/ziti
    ok "Installed ziti ${ZITI_VERSION} → /usr/local/bin/ziti"
fi

# ── 6. Create config directories ─────────────────────────────────────────────

CONFIG_DIR="${HOME}/.config/twilight"
mkdir -p "$CONFIG_DIR"
ok "Config directory: $CONFIG_DIR"

# Create a default bridge env file if one doesn't exist
BRIDGE_ENV="$CONFIG_DIR/bridge.env"
if [[ ! -f "$BRIDGE_ENV" ]]; then
    cat > "$BRIDGE_ENV" <<'EOF'
# LM Studio bridge configuration
# Uncomment and edit as needed.
# LMS_URL=http://localhost:1234
# LMS_MODEL=my-model-id
# LMS_TIMEOUT=90
# TWILIGHT_AGENT_NAME=lmstudio
EOF
    ok "Created bridge env template → $BRIDGE_ENV"
fi

# ── 7. Enable zenohd system service (requires sudo) ──────────────────────────

info "Checking zenohd system service..."
if systemctl is-enabled zenohd &>/dev/null; then
    ok "zenohd.service already enabled"
elif systemctl list-unit-files zenohd.service &>/dev/null 2>&1; then
    info "Enabling zenohd.service (requires sudo)..."
    if sudo systemctl enable --now zenohd; then
        ok "zenohd.service enabled and started"
    else
        warn "Could not enable zenohd — enable manually: sudo systemctl enable --now zenohd"
    fi
else
    warn "zenohd.service not found — install zenohd package first"
fi

# ── 8. Install and enable systemd user units ─────────────────────────────────

SYSTEMD_DIR="${HOME}/.config/systemd/user"
mkdir -p "$SYSTEMD_DIR"

cp "$REPO/scripts/templates/twilight-daemon.service" \
   "$SYSTEMD_DIR/twilight-daemon.service"
ok "Installed twilight-daemon.service"

cp "$REPO/scripts/templates/twilight-lmstudio-bridge.service" \
   "$SYSTEMD_DIR/twilight-lmstudio-bridge.service"
ok "Installed twilight-lmstudio-bridge.service"

# Patch bridge service to use system python3
PYBIN=$(command -v python3 || echo "/usr/bin/python3")
sed -i "s|%h/.local/bin/python3|$PYBIN|g" \
    "$SYSTEMD_DIR/twilight-lmstudio-bridge.service"

systemctl --user daemon-reload
ok "Systemd user daemon reloaded"

# ── 9. Enable user services ───────────────────────────────────────────────────

systemctl --user enable twilight-daemon.service
ok "twilight-daemon.service enabled (starts on login)"

systemctl --user enable twilight-lmstudio-bridge.service
ok "twilight-lmstudio-bridge.service enabled (starts after daemon)"

# Ensure lingering is on so user services survive logout
if loginctl show-user "$USER" 2>/dev/null | grep -q "Linger=yes"; then
    ok "Lingering already enabled for $USER"
else
    info "Enabling lingering so services run at boot (requires sudo)..."
    if sudo loginctl enable-linger "$USER"; then
        ok "Lingering enabled for $USER"
    else
        warn "Could not enable lingering — run: sudo loginctl enable-linger $USER"
    fi
fi

# ── Done ─────────────────────────────────────────────────────────────────────

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Boot order after install:"
echo ""
echo "   [boot] → zenohd.service        (system, router on :7447)"
echo "         → twilight-daemon.service (user, connects to zenohd)"
echo "         → twilight-lmstudio-bridge.service (user, LMS bridge)"
echo ""
echo " Next steps (if this is a fresh node):"
echo ""
echo "  1. Enroll this node with the Ziti fabric:"
echo "     twilight-cli daemon enroll --jwt ~/my-node.jwt"
echo ""
echo "  2. Edit daemon config:"
echo "     \$EDITOR ~/.config/twilight/daemon.toml"
echo ""
echo "  3. Start everything now:"
echo "     systemctl --user start twilight-daemon"
echo "     systemctl --user start twilight-lmstudio-bridge"
echo ""
echo "  4. Verify:"
echo "     twilight-cli daemon status"
echo "     journalctl --user -u twilight-daemon -f"
echo "     journalctl --user -u twilight-lmstudio-bridge -f"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
