#!/usr/bin/env bash
# Twilight Bark — Node install script
# Installs: ziti binary, twilight-daemon binary, and systemd user unit.
# Run once per node after `cargo build --release`.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_BIN="$REPO/target/release"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
info() { echo -e "${CYAN}[--]${NC} $*"; }
die()  { echo -e "${RED}[!!]${NC} $*" >&2; exit 1; }

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║   Twilight Bark — Node Installation                  ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""

# ── 1. Build release binaries ────────────────────────────────
info "Building release binaries..."
cargo build --release -p twilight-daemon -p twilight-mcp-server -p twilight-cli \
    --manifest-path "$REPO/Cargo.toml"
ok "Build complete"

# ── 2. Install binaries to ~/.cargo/bin ─────────────────────
CARGO_BIN="${HOME}/.cargo/bin"
mkdir -p "$CARGO_BIN"
for bin in twilight-daemon twilight-mcp-server twilight-cli; do
    cp "$RELEASE_BIN/$bin" "$CARGO_BIN/$bin"
    ok "Installed $bin → $CARGO_BIN/$bin"
done

# ── 3. Install ziti CLI binary ───────────────────────────────
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

# ── 4. Create default config directory ──────────────────────
CONFIG_DIR="${HOME}/.config/twilight"
mkdir -p "$CONFIG_DIR"
ok "Config directory: $CONFIG_DIR"

# ── 5. Install systemd user unit ────────────────────────────
SYSTEMD_DIR="${HOME}/.config/systemd/user"
mkdir -p "$SYSTEMD_DIR"
cp "$REPO/scripts/templates/twilight-daemon.service" "$SYSTEMD_DIR/twilight-daemon.service"
ok "Installed systemd user unit → $SYSTEMD_DIR/twilight-daemon.service"

systemctl --user daemon-reload
ok "Systemd user daemon reloaded"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Next steps:"
echo ""
echo "  1. Get an enrollment JWT from the hub admin:"
echo "     (hub admin runs: scripts/provision-fabric.sh --add-node \$(hostname)-\$(whoami))"
echo ""
echo "  2. Enroll this node:"
echo "     twilight-cli daemon enroll --jwt ~/my-node.jwt"
echo ""
echo "  3. Edit config (set node.role, ziti.controller_url, etc.):"
echo "     \$EDITOR ~/.config/twilight/daemon.toml"
echo ""
echo "  4. Start the daemon:"
echo "     systemctl --user enable --now twilight-daemon"
echo "     # or for quick dev: twilight-cli daemon start --config ~/.config/twilight/daemon.toml"
echo ""
echo "  5. Verify:"
echo "     twilight-cli daemon status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
