#!/bin/bash
set -e

# Twilight Console Build & Install Script (Linux .deb)
echo "--- [Twilight Console: Build & Install] ---"

# 1. Install System Dependencies (Ubuntu/Debian)
echo "[1/4] Checking system dependencies..."
sudo apt-get update
sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libxdo-dev \
    libssl-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev

# 2. Setup Node dependencies
echo "[2/4] Installing node dependencies..."
cd "$(dirname "$0")/../twilight-console"
npm install

# 3. Build the Tauri App (.deb bundle)
echo "[3/4] Building Tauri .deb bundle..."
# Note: We use 'npm run tauri build' which triggers the cargo build internally
npm run tauri build -- --bundles deb

# 4. Locate and notify about the .deb
DEB_PATH=$(find src-tauri/target/release/bundle/deb -name "*.deb" | head -n 1)

if [ -f "$DEB_PATH" ]; then
    echo "------------------------------------------------"
    echo "SUCCESS: Bundle created at:"
    echo "$DEB_PATH"
    echo "------------------------------------------------"
    echo "To install, run:"
    echo "sudo dpkg -i $DEB_PATH"
    echo "------------------------------------------------"
else
    echo "ERROR: .deb bundle not found. Check build logs above."
    exit 1
fi
