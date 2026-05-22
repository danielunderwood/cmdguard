#!/bin/bash
set -euo pipefail

# cmdguard installation script

HOOK_COMMAND="$HOME/.local/bin/cmdguard"

echo "=== cmdguard Installation ==="
echo ""

# Step 1: Build release binary
echo "[1/3] Building release binary..."
cargo build --release
echo "Build complete."
echo ""

# Step 2: Create ~/.local/bin and copy binary
echo "[2/3] Installing binary to ~/.local/bin..."
mkdir -p "$HOME/.local/bin"
cp target/release/cmdguard "$HOME/.local/bin/"
echo "Binary installed: $HOOK_COMMAND"
echo ""

# Step 3: Sync base policies and register hook
echo "[3/3] Setting up configuration..."
"$HOOK_COMMAND" base sync
echo ""
"$HOOK_COMMAND" hook install
echo ""

# Installation complete
echo "=== Installation Complete ==="
echo ""
echo "Make sure ~/.local/bin is in your PATH. Add this to your shell profile if needed:"
echo '  export PATH="$HOME/.local/bin:$PATH"'
echo ""
echo "Base policies: $HOME/.config/cmdguard/base/"
echo "Your overrides: $HOME/.config/cmdguard/policies/custom.rego"
