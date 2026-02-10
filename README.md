# claude-permissions

A PreToolUse hook for Claude Code that provides policy-driven permission control using Rego.

## Features

- **Wrapper extraction**: Recognizes commands through `nix develop`, `docker run`, `sudo`, inline env vars, etc.
- **Policy-based decisions**: Allow, deny, or ask based on Rego rules with priority
- **Compound command parsing**: Handles `&&`, `||`, `;`, `|` chains safely
- **Trust zones**: Classify binaries as system, user, project, or unknown
- **Parsed flags**: Access flags by name (e.g., `input.parsed_flags.force`) instead of string matching
- **Path-typed arguments**: Positional args with paths are resolved and classified
- **Project-local rules**: Per-project policies in `.claude/permissions/`
- **Configurable via Nickel**: Define custom wrappers and command schemas
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

Policies use named rules with priority-based resolution:

```rego
package claude.permissions

import data.claude.permissions.stdlib

# Safe git read operations
rules["safe_git_read"] := {
    "decision": "allow",
    "reason": "Safe git read operation",
} if {
    input.binary_name == "git"
    input.subcommand in {"status", "log", "diff", "branch", "show"}
}

# Block force push
rules["deny_force_push"] := {
    "decision": "deny",
    "reason": "Force push to protected branch blocked",
    "priority": 100,  # Higher priority overrides allows
} if {
    input.binary_name == "git"
    input.subcommand == "push"
    input.parsed_flags.force
}

# Block rm outside project
rules["deny_rm_outside_project"] := {
    "decision": "deny",
    "reason": "Cannot rm files outside project",
} if {
    input.binary_name == "rm"
    some target in input.positional.targets
    target.trust_zone != "project"
}
```

### Priority System

When multiple rules match, highest priority wins. Default priorities:

| Source  | Decision | Default Priority |
|---------|----------|------------------|
| Global  | deny     | 100              |
| Project | deny     | 75               |
| Global  | ask      | 50               |
| Project | ask      | 40               |
| Global  | allow    | 25               |
| Project | allow    | 20               |

Override with explicit `"priority": N` in a rule.

## Policy Input

Your policies receive this input:

```json
{
  "tool": "Bash",
  "raw_command": "sudo -u postgres rm -rf ./temp",
  "command": ["rm", "-rf", "./temp"],
  "wrapper_chain": ["sudo"],
  "flags_expanded": ["-r", "-f"],
  "paths": [{"raw": "./temp", "resolved": "/project/temp", "exists": true, "is_dir": true}],
  "cwd": "/home/user/project",
  "project_root": "/home/user/project",
  "session_id": "abc123",

  "binary_name": "rm",
  "resolved_path": "/bin/rm",
  "resolved_trust_zone": "system",
  "is_symlink": false,

  "parsed_flags": {
    "recursive": true,
    "force": true
  },
  "positional_args": [
    {"name": "targets", "values": [{"raw": "./temp", "resolved": "/project/temp", "trust_zone": "project"}]}
  ],
  "positional": {
    "targets": [{"raw": "./temp", "resolved": "/project/temp", "trust_zone": "project"}]
  },
  "subcommand": null,

  "chain_position": 1,
  "chain_length": 1,
  "chain_operator": null
}
```

### Trust Zones

Binaries are classified by location:

- `system` - `/usr/bin`, `/bin`, `/usr/local/bin`, Nix store, Homebrew
- `user` - `~/.local/bin`, `~/.cargo/bin`, `~/bin`
- `project` - Under `$PROJECT_ROOT`
- `unknown` - Resolution failed or not in any known zone

### Parsed Flags

Instead of string matching (`"-rf" in input.command`), use structured access:

```rego
# Old way (fragile)
dangerous if "-rf" in input.command

# New way (robust)
dangerous if {
    input.parsed_flags.recursive
    input.parsed_flags.force
}
```

Flag definitions come from built-in command schemas and can be extended via Nickel config.

## Project-Local Rules

Add project-specific policies in `.claude/permissions/`:

```
my-project/
  .claude/
    permissions/
      project.rego    # Project-specific rules
```

Project rules merge with global rules via priority. To allow something globally denied:

```rego
rules["allow_npm_scripts"] := {
    "decision": "allow",
    "reason": "NPM scripts allowed in this project",
    "priority": 101,  # Override global deny (100)
} if {
    input.binary_name == "npm"
    input.subcommand == "run"
}
```

## Nickel Configuration

Custom wrappers and command definitions go in `~/.config/claude-permissions/commands.ncl`:

```nickel
{
  wrappers = {
    # Define custom wrapper extraction
    my_runner = {
      extract = fun tokens =>
        if std.array.length tokens >= 3 && std.array.at 1 tokens == "run" then
          { remaining = std.array.slice 2 (std.array.length tokens) tokens,
            wrapper_name = "my_runner run" }
        else
          null
    },
  },

  commands = {
    # Define flags and positional args for parsing
    my_tool = {
      flags = {
        verbose = { short = ["-v"], long = "--verbose", type = "boolean" },
        output = { short = ["-o"], long = "--output", type = "with_arg" },
      },
      positional = [
        { name = "input", type = "path" },
        { name = "targets", type = "path", variadic = true },
      ],
    },
  },
}
```

See `config/commands.ncl.example` for more examples.

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
claude-permissions eval "RUST_LOG=debug cargo run"
```

Show full policy input (useful for writing Rego rules):

```bash
claude-permissions eval "rm -rf ./temp" --show-input
```

Validate Nickel configuration:

```bash
claude-permissions validate
```

Enable logging:

```bash
export RUST_LOG=debug
```

Logs written to `~/.local/state/claude-permissions/debug.log`

## License

MIT
