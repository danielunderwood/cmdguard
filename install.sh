#!/bin/bash
set -euo pipefail

# claude-permissions installation script
# This script builds and installs the claude-permissions PreToolUse hook

echo "=== claude-permissions Installation ==="
echo ""

# Step 1: Build release binary
echo "[1/5] Building release binary..."
cargo build --release
echo "Build complete."
echo ""

# Step 2: Create ~/.local/bin and copy binary
echo "[2/5] Installing binary to ~/.local/bin..."
mkdir -p "$HOME/.local/bin"
cp target/release/claude-permissions "$HOME/.local/bin/"
echo "Binary installed: $HOME/.local/bin/claude-permissions"
echo ""

# Step 3: Create config directory
echo "[3/5] Creating configuration directory..."
mkdir -p "$HOME/.config/claude-permissions"
echo "Config directory created: $HOME/.config/claude-permissions"
echo ""

# Step 4: Copy stdlib.rego
echo "[4/5] Copying standard library policy..."
cp policies/stdlib.rego "$HOME/.config/claude-permissions/"
echo "Installed: $HOME/.config/claude-permissions/stdlib.rego"
echo ""

# Step 5: Copy example policy if none exists
echo "[5/5] Setting up policy.rego..."
if [ -f "$HOME/.config/claude-permissions/policy.rego" ]; then
    echo "Existing policy.rego found - preserving your configuration."
else
    cp examples/policy.rego "$HOME/.config/claude-permissions/"
    echo "Installed example policy: $HOME/.config/claude-permissions/policy.rego"
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
