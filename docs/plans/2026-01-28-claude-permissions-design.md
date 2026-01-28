# Claude Permissions: PreToolUse Hook for Policy-Driven Permission Control

## Overview

A Rust binary (`claude-permissions`) that acts as a PreToolUse hook for Claude Code, providing flexible, policy-driven permission control using embedded Rego (via regorus).

**Goals:**
- More granular trust than Claude's built-in `Bash(git:*)` syntax
- Recognize commands through wrappers (`nix develop`, `docker run`, etc.)
- Configurable rules via Rego policies with hot-reload
- Fail-safe defaults

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Claude Code                                  │
│                         │                                        │
│                    PreToolUse hook                               │
│                         ▼                                        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 claude-permissions                        │   │
│  │                                                           │   │
│  │  1. Parse JSON input (tool_name, tool_input)             │   │
│  │  2. If Bash: extract real command from wrappers          │   │
│  │  3. Normalize flags (-rf → -r, -f)                       │   │
│  │  4. Detect and resolve paths                             │   │
│  │  5. Build policy input object                            │   │
│  │  6. Evaluate against .rego policy (via regorus)          │   │
│  │  7. Return JSON: {decision: allow|deny|ask, reason: ...} │   │
│  └──────────────────────────────────────────────────────────┘   │
│                         ▲                                        │
│                         │ (loaded fresh each invocation)         │
│                 ~/.config/claude-permissions/                    │
│                     ├── stdlib.rego                              │
│                     └── policy.rego                              │
└─────────────────────────────────────────────────────────────────┘
```

## Wrapper Extraction

Commands run through wrappers should be evaluated as their underlying command.

**Supported wrappers:**

| Wrapper | Pattern | Extract |
|---------|---------|---------|
| nix develop | `nix develop --command <cmd>` | `<cmd>` |
| nix-shell | `nix-shell --run "<cmd>"` | `<cmd>` |
| docker run | `docker run [opts] <image> <cmd>` | `<cmd>` |
| docker exec | `docker exec [opts] <container> <cmd>` | `<cmd>` |
| env | `env [VAR=val]... <cmd>` | `<cmd>` |
| sudo | `sudo [opts] <cmd>` | `<cmd>` |
| sh -c | `sh -c "<cmd>"` | `<cmd>` |
| bash -c | `bash -c "<cmd>"` | `<cmd>` |

**Extraction approach:**
1. Parse command string into tokens (respecting quotes)
2. Match against known wrapper patterns
3. Recursively extract until we hit a "real" command
4. Provide both wrapper chain and extracted command to policy

## Policy Input Structure

Rust provides this input to Rego:

```json
{
  "tool": "Bash",
  "raw_command": "nix develop --command rm -rf build/",
  "command": ["rm", "-rf", "build/"],
  "wrapper_chain": ["nix develop"],
  "flags_expanded": ["-r", "-f"],
  "paths": [
    {
      "raw": "build/",
      "resolved": "/home/user/project/build/",
      "exists": true,
      "is_dir": true
    }
  ],
  "cwd": "/home/user/project",
  "project_root": "/home/user/project",
  "session_id": "abc123"
}
```

**Division of responsibility:**
- **Rust:** Mechanical processing (wrapper extraction, flag expansion, path resolution)
- **Rego:** Semantic interpretation (what is a subcommand, what does `--output` mean for a specific command)

## Rego Policy Structure

**stdlib.rego** - Ships with common helpers:

```rego
package claude.permissions.stdlib

# Get value following a flag (e.g., --output foo)
flag_value(flag) := input.command[i+1] {
    input.command[i] == flag
    i + 1 < count(input.command)
    not startswith(input.command[i+1], "-")
}

# Git helpers
git_subcommand := input.command[1] {
    input.command[0] == "git"
    count(input.command) > 1
    not startswith(input.command[1], "-")
}

# Check if any path is outside project root
path_outside_project {
    some path in input.paths
    not startswith(path.resolved, input.project_root)
}

# Check if all paths are within project root
all_paths_in_project {
    every path in input.paths {
        startswith(path.resolved, input.project_root)
    }
}
```

**policy.rego** - User-defined rules:

```rego
package claude.permissions

import data.claude.permissions.stdlib

default decision = "ask"

# Allow safe git commands
decision = "allow" {
    input.command[0] == "git"
    stdlib.git_subcommand in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

# Allow rm -rf only within project root
decision = "allow" {
    input.command[0] == "rm"
    "-r" in input.flags_expanded
    stdlib.all_paths_in_project
}

# Deny force push
decision = "deny" {
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
    "--force" in input.command
}

# Deny rm -rf outside project
decision = "deny" {
    input.command[0] == "rm"
    "-r" in input.flags_expanded
    stdlib.path_outside_project
}

# Reasons
reason = "Safe git read operation" {
    decision == "allow"
    input.command[0] == "git"
}

reason = "rm -rf only allowed within project root" {
    input.command[0] == "rm"
    decision == "deny"
}

reason = "Force push blocked by policy" {
    input.command[0] == "git"
    decision == "deny"
}
```

## Output Format

Response to Claude:

```json
{
  "hookSpecificOutput": {
    "permissionDecision": "allow|deny|ask",
    "updatedInput": null
  },
  "systemMessage": "Optional reason for the decision"
}
```

## Error Handling

**Principle:** Fail safe. Default to `ask` on any error.

| Scenario | Behavior |
|----------|----------|
| Policy file missing | Log warning, return `ask` |
| Rego syntax error | Log error with details, return `ask` |
| Invalid JSON input | Log error, return `ask` |
| Wrapper extraction fails | Use raw command, continue |
| Path resolution fails | Include raw path only, continue |

**Exit codes:**
- `0` - Success (stdout has decision JSON)
- `2` - Hard deny (stderr has reason for Claude)

## Logging

- Controlled via `RUST_LOG` environment variable
- Log file: `~/.local/state/claude-permissions/debug.log`
- Includes compilation timing, evaluation timing, decisions

```
2026-01-28T10:30:00Z DEBUG compile_ms=3.2 eval_ms=0.8 decision="allow" command=["git", "status"]
```

## Project Structure

```
claude-permissions/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, stdin/stdout handling
│   ├── input.rs             # Parse Claude's JSON input
│   ├── extractor.rs         # Wrapper extraction, tokenization
│   ├── policy.rs            # Load .rego files, run regorus
│   └── output.rs            # Format response JSON
├── policies/
│   └── stdlib.rego          # Standard helpers
├── examples/
│   └── policy.rego          # Example user policy
├── install.sh               # Setup script
└── tests/
    ├── extractor_tests.rs
    └── policy_tests.rs
```

**Dependencies:**
- `regorus` - Rego evaluation
- `serde` / `serde_json` - JSON handling
- `shlex` or `shell-words` - Command tokenization
- `tracing` / `tracing-subscriber` - Structured logging

## Installation

```bash
# Build and install
cargo build --release
cp target/release/claude-permissions ~/.local/bin/

# Create config directory and copy policies
mkdir -p ~/.config/claude-permissions
cp policies/stdlib.rego ~/.config/claude-permissions/
cp examples/policy.rego ~/.config/claude-permissions/
```

**Hook registration** (add to `~/.claude/settings.json`):

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

## Testing

**Manual testing without Claude:**

```bash
echo '{"tool_name":"Bash","tool_input":{"command":"git status"}}' | \
  RUST_LOG=debug ~/.local/bin/claude-permissions
```

**Unit tests:**
- Wrapper extraction for all supported wrappers
- Flag normalization
- Path detection and resolution
- Policy evaluation with various inputs

## Future Enhancements

- **Learning mode:** Log decisions, CLI to review and generate rules
- **Config file layer:** TOML/JSON config that compiles to Rego
- **Project-local policies:** `.claude-permissions/policy.rego` overrides
- **Caching:** If compilation proves slow, add timestamp-based caching
