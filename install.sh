#!/bin/bash
set -euo pipefail

# claude-permissions installation script
# This script builds and installs the claude-permissions PreToolUse hook

echo "=== claude-permissions Installation ==="
echo ""

# Step 1: Build release binary
echo "[1/3] Building release binary..."
cargo build --release
echo "Build complete."
echo ""

# Step 2: Create ~/.local/bin and copy binary
echo "[2/3] Installing binary to ~/.local/bin..."
mkdir -p "$HOME/.local/bin"
cp target/release/claude-permissions "$HOME/.local/bin/"
echo "Binary installed: $HOME/.local/bin/claude-permissions"
echo ""

# Step 3: Set up config directory
echo "[3/3] Setting up configuration..."
CONFIG_DIR="$HOME/.config/claude-permissions"

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

# Installation complete
echo "=== Installation Complete ==="
echo ""
echo "To enable the hook, add the following to your ~/.claude/settings.json:"
echo ""
echo '  "hooks": {'
echo '    "preToolUse": {'
echo '      "command": "'"$HOME/.local/bin/claude-permissions"'",'
echo '      "args": []'
echo '    }'
echo '  }'
echo ""
echo "Make sure ~/.local/bin is in your PATH. Add this to your shell profile if needed:"
echo '  export PATH="$HOME/.local/bin:$PATH"'
echo ""
echo "Edit your policy at: $HOME/.config/claude-permissions/policy.rego"
