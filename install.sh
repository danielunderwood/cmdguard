#!/bin/bash
set -euo pipefail

# cmdguard installation script
# This script builds and installs the cmdguard PreToolUse hook

HOOK_COMMAND="$HOME/.local/bin/cmdguard"
SETTINGS_FILE="$HOME/.claude/settings.json"

echo "=== cmdguard Installation ==="
echo ""

# Step 1: Build release binary
echo "[1/4] Building release binary..."
cargo build --release
echo "Build complete."
echo ""

# Step 2: Create ~/.local/bin and copy binary
echo "[2/4] Installing binary to ~/.local/bin..."
mkdir -p "$HOME/.local/bin"
cp target/release/cmdguard "$HOME/.local/bin/"
echo "Binary installed: $HOOK_COMMAND"
echo ""

# Step 3: Set up config directory
echo "[3/4] Setting up configuration..."
CONFIG_DIR="$HOME/.config/cmdguard"

if [ -e "$CONFIG_DIR" ] || [ -L "$CONFIG_DIR" ]; then
    echo "Config directory exists (may be symlink) - skipping policy installation"
    echo "Manage your policies at: $CONFIG_DIR"
else
    mkdir -p "$CONFIG_DIR"
    cp policies/stdlib.rego "$CONFIG_DIR/"
    cp examples/basic/policy.rego "$CONFIG_DIR/"
    cp examples/policy_tests.yaml "$CONFIG_DIR/"
    echo "Installed example policies to $CONFIG_DIR"
fi
echo ""

# Step 4: Register hook in ~/.claude/settings.json
echo "[4/4] Registering hook in $SETTINGS_FILE..."
"$HOOK_COMMAND" hook install
echo ""

# Installation complete
echo "=== Installation Complete ==="
echo ""
echo "Make sure ~/.local/bin is in your PATH. Add this to your shell profile if needed:"
echo '  export PATH="$HOME/.local/bin:$PATH"'
echo ""
echo "Edit your policy at: $HOME/.config/cmdguard/policy.rego"
