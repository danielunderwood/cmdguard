# claude-permissions

A PreToolUse hook for Claude Code that provides policy-driven permission control using Rego.

## Features

- **Wrapper extraction**: Recognizes commands through `nix develop`, `docker run`, `sudo`, etc.
- **Policy-based decisions**: Allow, deny, or ask based on Rego rules
- **Flag normalization**: `-rf` treated same as `-r -f`
- **Path awareness**: Detect and resolve paths in commands
- **Fail-safe**: Defaults to `ask` on any error

## Installation

```bash
./install.sh
```

Then add the hook to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/claude-permissions"
          }
        ]
      }
    ]
  }
}
```

## Directory Structure

```
policies/                    # For unit tests (minimal, stable)
  stdlib.rego                # Standard helpers (required by all)
  test_policy.rego           # Minimal policy for unit tests

examples/                    # Reference implementations
  basic/
    policy.rego              # Simple single-file example
  split/
    git.rego                 # Git rules example
    cargo.rego               # Cargo rules example
    npm.rego                 # NPM/yarn rules example
    safety.rego              # Dangerous command blocking
  policy_tests.yaml          # Example test file

config/                      # User's working directory (symlink target)
  stdlib.rego                # Copy of standard helpers
```

## Configuration

Policies live in `~/.config/claude-permissions/`:

- `stdlib.rego` - Standard helpers (git_subcommand, path checks, etc.)
- `policy.rego` - Your custom rules

### Development Workflow (Symlink)

For active development, symlink your config directory to this repo:

```bash
# Remove installed config (if exists)
rm -rf ~/.config/claude-permissions

# Symlink to repo's config directory
ln -s /path/to/claude-hooks/config ~/.config/claude-permissions
```

This lets you edit policies in the repo and test immediately. The install script detects symlinks and won't overwrite them.

### Using Split Examples

The `examples/split/` directory shows how to organize rules by domain. Copy the files you need:

```bash
# Start with stdlib
cp examples/split/../../../config/stdlib.rego ~/.config/claude-permissions/

# Add only the rules you want
cp examples/split/git.rego ~/.config/claude-permissions/
cp examples/split/cargo.rego ~/.config/claude-permissions/
```

All `.rego` files in the directory are loaded and merged automatically.

## Writing Policies

```rego
package claude.permissions

import data.claude.permissions.stdlib

default decision = "ask"

# Allow git status
decision = "allow" {
    input.command[0] == "git"
    stdlib.git_subcommand == "status"
}

# Deny force push
decision = "deny" {
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
    "--force" in input.command
}

reason = "Force push blocked" {
    decision == "deny"
}
```

## Policy Input

Your policies receive this input:

```json
{
  "tool": "Bash",
  "raw_command": "nix develop --command git status",
  "command": ["git", "status"],
  "wrapper_chain": ["nix develop"],
  "flags_expanded": [],
  "paths": [],
  "cwd": "/home/user/project",
  "project_root": "/home/user/project",
  "session_id": "abc123"
}
```

## Testing Policies

Run policy tests:

```bash
# Run all tests from policy_tests.yaml
claude-permissions test

# Run with verbose output
claude-permissions test --verbose

# Run specific test file
claude-permissions test my_tests.yaml
```

Test file format (`policy_tests.yaml`):

```yaml
tests:
  - name: "allow git status"
    command: "git status"
    expect: allow
    reason_contains: "Safe git"

  - name: "deny force push"
    command: "git push --force origin main"
    expect: deny
```

## Debugging

Evaluate a single command:

```bash
claude-permissions eval "git status"
claude-permissions eval "nix develop --command cargo build"
```

Enable logging:

```bash
export RUST_LOG=debug
```

Logs written to `~/.local/state/claude-permissions/debug.log`

## License

MIT
