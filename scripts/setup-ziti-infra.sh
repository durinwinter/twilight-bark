#!/bin/bash
set -e

# Twilight Bark: OpenZiti Infrastructure Setup
# This script installs the Ziti Controller and a local Edge Router.
# Ideal for the 'Routing Node' of your fabric.

echo "--- [Twilight Bark: OpenZiti Infra Setup] ---"

# 1. Environment Check
if [[ "$OSTYPE" != "linux-gnu"* ]]; then
    echo "Error: This script is only supported on Linux."
    exit 1
fi

# 2. Interactive Configuration
echo "--- [Configuration] ---"
read -p "Network Name [twilight-fabric]: " ZITI_NETWORK_NAME
ZITI_NETWORK_NAME=${ZITI_NETWORK_NAME:-twilight-fabric}

read -p "Admin Username [admin]: " ZITI_USER
ZITI_USER=${ZITI_USER:-admin}

read -s -p "Admin Password [bark-password-123]: " ZITI_PWD
ZITI_PWD=${ZITI_PWD:-bark-password-123}
echo ""

read -p "Network Config Filename [twilight-net.yaml]: " NET_CONFIG_NAME
NET_CONFIG_NAME=${NET_CONFIG_NAME:-twilight-net.yaml}

# 3. Setup OpenZiti Infrastructure
echo -e "\n[1/3] Fetching OpenZiti helper functions..."
# We source the official helper functions which handle binary downloads and setup
# Using a more robust connection method to handle redirection and errors
source /dev/stdin <<< "$(curl -sSfL https://get.openziti.io/ziti-cli-functions.sh)"

echo "[2/3] Executing Express Installation..."
# Prepare environment for expressInstall
export ZITI_PKI_NAME="$ZITI_NETWORK_NAME-pki"
export ZITI_CTRL_NAME="$ZITI_NETWORK_NAME-controller"

# Run the express install (Handles Controller and local Edge Router)
expressInstall

# 4. Generate Twilight Network ID
echo "[3/3] Generating Network ID (config/$NET_CONFIG_NAME)..."
mkdir -p ../config

cat <<EOF > "../config/$NET_CONFIG_NAME"
network_name: "$ZITI_NETWORK_NAME"
controller_url: "https://${ZITI_CTRL_ADVERTISED_ADDRESS}:${ZITI_CTRL_ADVERTISED_PORT}"
tenant_id: "default-tenant"
admin_user: "$ZITI_USER"
provisioned_at: "$(date)"
EOF

# 5. Success Summary
echo "------------------------------------------------"
echo "INFRASTRUCTURE PROVISIONED SUCCESSFULLY"
echo "------------------------------------------------"
echo "Controller: https://${ZITI_CTRL_ADVERTISED_ADDRESS}:${ZITI_CTRL_ADVERTISED_PORT}"
echo "Router:     ${ZITI_ROUTER_ADVERTISED_ADDRESS}"
echo "------------------------------------------------"
echo "Network ID created at: config/twilight-net.yaml"
echo "Distribute this file to your 15 team members."
echo "------------------------------------------------"
echo "WARNING: Change your admin password using 'ziti edge update identity'!"
