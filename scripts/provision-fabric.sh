#!/usr/bin/env bash
# Twilight Bark — Fabric Provisioning Script
# Runs on the hub node (pop-os). Creates Ziti service, policies, and node identities.
# Re-run with --add-node to enroll additional nodes without touching existing ones.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENROLLMENTS_DIR="$REPO/config/enrollments"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
info() { echo -e "${CYAN}[--]${NC} $*"; }
warn() { echo -e "${YELLOW}[??]${NC} $*"; }
die()  { echo -e "${RED}[!!]${NC} $*" >&2; exit 1; }

# ── Args ──────────────────────────────────────────────────────
ADD_NODE=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --add-node) ADD_NODE="$2"; shift 2 ;;
        *) die "Unknown arg: $1. Usage: $0 [--add-node <node-id>]" ;;
    esac
done

# ── Read controller config ────────────────────────────────────
NET_CONFIG="$REPO/config/twilight-net.yaml"
if [[ ! -f "$NET_CONFIG" ]]; then
    die "No $NET_CONFIG found. Run scripts/setup-ziti-infra.sh first."
fi

CONTROLLER_URL=$(grep 'controller_url' "$NET_CONFIG" | awk '{print $2}' | tr -d '"')
ADMIN_USER=$(grep 'admin_user' "$NET_CONFIG" | awk '{print $2}' | tr -d '"')

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║   Twilight Bark — Fabric Provisioning               ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""
info "Controller: $CONTROLLER_URL"
info "Admin user: $ADMIN_USER"
echo ""

read -s -p "Admin password: " ADMIN_PASS; echo ""

# ── Login ─────────────────────────────────────────────────────
info "Logging in to Ziti controller..."
ziti edge login "$CONTROLLER_URL" -u "$ADMIN_USER" -p "$ADMIN_PASS" -y
ok "Logged in"

mkdir -p "$ENROLLMENTS_DIR"

# ── Add a single node (called by --add-node) ─────────────────
add_node_identity() {
    local NODE_ID="$1"
    local ROLE_ATTRS="$2"
    local JWT_PATH="$ENROLLMENTS_DIR/${NODE_ID}.jwt"

    if ziti edge list identities "name=\"$NODE_ID\"" | grep -q "$NODE_ID"; then
        warn "Identity '$NODE_ID' already exists — skipping creation"
    else
        info "Creating identity: $NODE_ID (roles: $ROLE_ATTRS)"
        ziti edge create identity "$NODE_ID" \
            --role-attributes "$ROLE_ATTRS" \
            -o "$JWT_PATH"
        ok "Created identity → $JWT_PATH"
    fi
}

if [[ -n "$ADD_NODE" ]]; then
    add_node_identity "$ADD_NODE" "fabric-nodes"
    echo ""
    echo "Enrollment JWT ready: $ENROLLMENTS_DIR/${ADD_NODE}.jwt"
    echo "Send this file to the node, then on that node run:"
    echo "  twilight-cli daemon enroll --jwt ${ADD_NODE}.jwt"
    exit 0
fi

# ── Full fabric bootstrap (first run) ────────────────────────

# 1. Service configs
info "Creating Zenoh router service configs..."
if ! ziti edge list configs "name=\"twilight-zenoh-host\"" | grep -q "twilight-zenoh-host"; then
    ziti edge create config twilight-zenoh-host host.v1 \
        '{"protocol":"tcp","address":"127.0.0.1","port":7447}'
    ok "Created host config"
fi
if ! ziti edge list configs "name=\"twilight-zenoh-intercept\"" | grep -q "twilight-zenoh-intercept"; then
    ziti edge create config twilight-zenoh-intercept intercept.v1 \
        '{"protocols":["tcp"],"addresses":["twilight-zenoh-router"],"portRanges":[{"low":7447,"high":7447}]}'
    ok "Created intercept config"
fi

# 2. Service
info "Creating twilight-zenoh-router service..."
if ! ziti edge list services "name=\"twilight-zenoh-router\"" | grep -q "twilight-zenoh-router"; then
    ziti edge create service twilight-zenoh-router \
        --configs twilight-zenoh-host,twilight-zenoh-intercept
    ok "Created service"
fi

# 3. Service policies
info "Creating service policies..."
if ! ziti edge list service-policies "name=\"twilight-zenoh-bind\"" | grep -q "twilight-zenoh-bind"; then
    ziti edge create service-policy twilight-zenoh-bind Bind \
        --service-roles "@twilight-zenoh-router" \
        --identity-roles "#hub-nodes"
    ok "Created bind policy (#hub-nodes)"
fi
if ! ziti edge list service-policies "name=\"twilight-zenoh-dial\"" | grep -q "twilight-zenoh-dial"; then
    ziti edge create service-policy twilight-zenoh-dial Dial \
        --service-roles "@twilight-zenoh-router" \
        --identity-roles "#fabric-nodes"
    ok "Created dial policy (#fabric-nodes)"
fi

# 4. Hub node identity
HUB_NODE_ID="hub-$(hostname)-$(whoami)"
info "Creating hub identity: $HUB_NODE_ID"
add_node_identity "$HUB_NODE_ID" "hub-nodes,fabric-nodes"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Fabric provisioned successfully."
echo ""
echo " Hub enrollment JWT:  $ENROLLMENTS_DIR/${HUB_NODE_ID}.jwt"
echo ""
echo " Next steps on this hub node:"
echo "   twilight-cli daemon enroll --jwt $ENROLLMENTS_DIR/${HUB_NODE_ID}.jwt"
echo "   cp $REPO/config/daemon-hub.toml ~/.config/twilight/daemon.toml"
echo "   # edit node.name to match: $HUB_NODE_ID"
echo "   scripts/install-service.sh"
echo "   systemctl --user enable --now twilight-daemon"
echo ""
echo " To add more nodes later:"
echo "   $0 --add-node <hostname>-<username>"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
