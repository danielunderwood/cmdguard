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

## Configuration

Policies live in `~/.config/claude-permissions/`:

- `stdlib.rego` - Standard helpers (git_subcommand, path checks, etc.)
- `policy.rego` - Your custom rules

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
