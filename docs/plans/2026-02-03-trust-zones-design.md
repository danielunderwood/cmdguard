# Trust Zones Implementation Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add PATH resolution and trust zone classification to policy input, enabling policies to make decisions based on where binaries are located.

**Architecture:** New `resolver.rs` module handles PATH lookup, symlink resolution, and trust zone classification. Classification is based on where the binary was found in PATH (symlink source), not where it ultimately resolves to.

**Tech Stack:** Rust std::fs for path resolution, std::env for PATH parsing

---

## Data Model

New fields added to `PolicyInput`:

```json
{
  "command_as_typed": "git",
  "binary_name": "git",
  "resolved_path": "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
  "resolved_trust_zone": "system",
  "is_symlink": true,
  "symlink_source": "/usr/bin/git"
}
```

### Field Definitions

| Field | Type | Description |
|-------|------|-------------|
| `command_as_typed` | `String` | First token exactly as it appeared in the command |
| `binary_name` | `String` | Filename portion only (basename) |
| `resolved_path` | `Option<String>` | Absolute path to actual binary after symlink resolution. `null` if resolution fails |
| `resolved_trust_zone` | `String` | One of: `"system"`, `"user"`, `"project"`, `"unknown"` |
| `is_symlink` | `bool` | Whether the PATH entry was a symlink |
| `symlink_source` | `Option<String>` | Where found in PATH (before symlink resolution). Present only if `is_symlink` is true |

### Trust Zone Definitions

- **system** - Binary found in system-managed directories
- **user** - Binary found in user-specific directories
- **project** - Binary found under the project root
- **unknown** - Resolution failed, or path doesn't match any zone

## Design Decision: Classify by Symlink Source

Classification is based on where the binary was **found in PATH**, not where it **resolves to**.

**Rationale:**
1. PATH order reflects explicit user/system configuration choices
2. Package managers (Homebrew, Nix, etc.) use symlink farms - the symlink location is the trust anchor
3. Matches user mental model (`which git` shows PATH location)
4. Works correctly on NixOS where everything resolves to `/nix/store`

**Examples:**

| Platform | Command | Found in PATH | Resolves to | Trust Zone |
|----------|---------|---------------|-------------|------------|
| macOS | `git` | `/usr/bin/git` | `/Applications/Xcode.app/.../git` | system |
| NixOS | `git` | `/run/current-system/sw/bin/git` | `/nix/store/abc.../git` | system |
| NixOS | `my-tool` | `~/.nix-profile/bin/my-tool` | `/nix/store/xyz.../my-tool` | user |
| Any | `./build/tool` | `./build/tool` | `/project/build/tool` | project |

**TOCTOU Limitation:** We cannot guarantee the resolved path matches what the shell will execute. The information is advisory - policies can choose their paranoia level using `command_as_typed` vs `resolved_path`.

## Default Trust Zone Paths

### Tier 1 - Well Tested (macOS, NixOS)

**macOS System:**
- `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`
- `/usr/local/bin`, `/usr/local/sbin`
- `/opt/homebrew/bin`, `/opt/homebrew/sbin`

**NixOS System:**
- `/run/current-system/sw/bin`
- `/nix/var/nix/profiles/default/bin`

**NixOS User:**
- `~/.nix-profile/bin`

### Tier 2 - Common Cross-Platform (Less Tested)

**Common User:**
- `~/.local/bin`, `~/bin`
- `~/.cargo/bin`
- `~/.go/bin`, `~/go/bin`

**Linux System:**
- `/usr/local/bin`, `/usr/local/sbin`
- `/snap/bin`

**Version Manager Shims (User):**
- `~/.pyenv/shims`
- `~/.rbenv/shims`
- `~/.asdf/shims`
- `~/.nvm/versions/node/*/bin` (pattern match)

### Classification Order

1. Check if path is under project root → `project`
2. Check against user paths → `user`
3. Check against system paths → `system`
4. Otherwise → `unknown`

Project is checked first so `./node_modules/.bin/eslint` is correctly classified as `project`.

## Project Root Detection

1. Start at current working directory
2. Walk up parent directories looking for `.git` directory
3. If found, use that directory as project root
4. If filesystem root reached without finding `.git`, fall back to original cwd

### Project Root Validation

Reject obviously invalid project roots to prevent misclassification:
- `/`
- `/usr`
- `/home`
- `/var`
- `/etc`
- `/tmp`
- `/opt`

If cwd or detected git root is one of these, skip project zone classification entirely.

## Implementation

### New Module: `src/resolver.rs`

```rust
use std::path::{Path, PathBuf};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TrustZone {
    System,
    User,
    Project,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedCommand {
    pub command_as_typed: String,
    pub binary_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    pub resolved_trust_zone: TrustZone,
    pub is_symlink: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_source: Option<String>,
}

/// Resolve a command to its binary location and classify trust zone
pub fn resolve_command(
    command: &str,
    project_root: &Path,
) -> ResolvedCommand;

/// Detect project root by walking up to find .git directory
pub fn detect_project_root(cwd: &Path) -> PathBuf;

/// Get default trust zone paths for current platform
fn get_default_paths() -> TrustZonePaths;

/// Classify a path into a trust zone
fn classify_path(
    path: &Path,
    project_root: &Path,
    zone_paths: &TrustZonePaths,
) -> TrustZone;
```

### Resolution Logic

```
resolve_command(command, project_root):
    1. If command contains '/', treat as literal path
       - Resolve to absolute path
       - Classify directly

    2. Otherwise, search PATH entries in order:
       for each dir in PATH:
           candidate = dir / command
           if candidate exists and is executable:
               symlink_source = candidate
               break

    3. If no match found:
       return ResolvedCommand with resolved_path=None, zone=Unknown

    4. Resolve symlinks:
       resolved_path = canonicalize(symlink_source)
       is_symlink = (symlink_source != resolved_path)

    5. Classify by symlink_source (not resolved_path):
       trust_zone = classify_path(symlink_source, project_root)

    6. Return ResolvedCommand with all fields populated
```

### Platform Detection

```rust
fn get_default_paths() -> TrustZonePaths {
    let mut paths = TrustZonePaths::common();

    #[cfg(target_os = "macos")]
    paths.extend(TrustZonePaths::macos());

    #[cfg(target_os = "linux")]
    {
        paths.extend(TrustZonePaths::linux());

        // Detect NixOS
        if Path::new("/nix/store").exists() {
            paths.extend(TrustZonePaths::nixos());
        }
    }

    paths
}
```

## PolicyInput Changes

Add new fields to `PolicyInput` struct in `src/policy.rs`:

```rust
pub struct PolicyInput {
    // ... existing fields ...

    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_as_typed: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_trust_zone: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_symlink: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_source: Option<String>,
}
```

## Integration Points

### `run_hook_inner()` in `src/main.rs`

```rust
// After parsing command, before policy evaluation
let project_root = resolver::detect_project_root(&cwd_path);

// For each command in compound chain
let resolved = resolver::resolve_command(&extracted.command[0], &project_root);

let policy_input = PolicyInput {
    // ... existing fields ...
    command_as_typed: Some(resolved.command_as_typed),
    binary_name: Some(resolved.binary_name),
    resolved_path: resolved.resolved_path,
    resolved_trust_zone: Some(resolved.resolved_trust_zone.to_string()),
    is_symlink: Some(resolved.is_symlink),
    symlink_source: resolved.symlink_source,
};
```

### `run_eval()` in `src/main.rs`

Add trust zone info to eval output:

```
=== Command 1/1: git status ===
Command:    ["git", "status"]
Binary:     git
Resolved:   /Applications/Xcode.app/.../git
Trust Zone: system
Symlink:    /usr/bin/git -> /Applications/Xcode.app/.../git
Decision:   Allow
```

### Test Runner

Update `src/test_runner.rs` to include resolved command info in test evaluation.

## Example Policy Usage

```rego
package cmdguard

import rego.v1

# Deny dangerous commands from unknown locations
rules["unknown_dangerous"] := {
    "decision": "deny",
    "reason": "Dangerous command from unknown location",
    "priority": 100,
} if {
    input.binary_name in {"rm", "chmod", "chown", "dd"}
    input.resolved_trust_zone == "unknown"
}

# Allow project-local tools
rules["project_tools"] := {
    "decision": "allow",
    "reason": "Project-local development tool",
    "priority": 30,
} if {
    input.resolved_trust_zone == "project"
    input.binary_name in {"eslint", "prettier", "jest", "webpack"}
}

# Trust system binaries for safe operations
rules["system_git"] := {
    "decision": "allow",
    "reason": "System git",
    "priority": 25,
} if {
    input.binary_name == "git"
    input.resolved_trust_zone == "system"
}

# Be cautious with user-installed binaries
rules["user_zone_ask"] := {
    "decision": "ask",
    "reason": "User-installed binary - please verify",
    "priority": 40,
} if {
    input.resolved_trust_zone == "user"
}
```

## Testing Strategy

### Unit Tests (`src/resolver.rs`)

1. PATH resolution finds correct binary
2. Symlink detection and resolution
3. Trust zone classification for each zone type
4. Project root detection (with .git, without .git)
5. Invalid project root rejection
6. Relative path handling (`./script`, `../other/tool`)
7. Direct path handling (`/usr/bin/git`)
8. Command not found (returns unknown zone)

### Integration Tests

1. Full flow from command to policy input with trust zone
2. Compound commands each get resolved independently
3. Eval command shows trust zone info

### Platform-Specific Tests

1. macOS Homebrew paths
2. NixOS profile paths (if `/nix/store` exists)
3. Standard Linux paths

## Tasks

### Task 1: Create resolver module structure
- Create `src/resolver.rs` with types and function signatures
- Add `mod resolver;` to `src/main.rs`
- Implement `TrustZone` enum with serde

### Task 2: Implement project root detection
- `detect_project_root()` walks up looking for `.git`
- Falls back to cwd if not found
- Add validation for invalid roots

### Task 3: Implement default zone paths
- Platform detection (macos, linux, nixos)
- `TrustZonePaths` struct with system/user vectors
- Path expansion for `~` home directory

### Task 4: Implement PATH resolution
- Parse `PATH` environment variable
- Search for executable in each directory
- Return first match or None

### Task 5: Implement symlink resolution
- Use `std::fs::canonicalize` for full resolution
- Detect if original path was symlink
- Handle resolution failures gracefully

### Task 6: Implement trust zone classification
- Check project zone first
- Then user paths
- Then system paths
- Default to unknown

### Task 7: Update PolicyInput
- Add new fields to struct
- Update serde attributes
- Update all PolicyInput construction sites

### Task 8: Integrate into hook flow
- Call resolver from `run_hook_inner()`
- Call resolver from `evaluate_compound()`
- Call resolver from test runner

### Task 9: Update eval command output
- Show binary name, resolved path, trust zone
- Show symlink info if applicable

### Task 10: Add unit tests
- Test each resolver function
- Test platform-specific paths
- Test edge cases (not found, invalid paths, etc.)

### Task 11: Add integration tests
- Full hook flow with trust zones
- Test with real binaries on system

### Task 12: Update example policies
- Add trust zone examples to config/*.rego
- Document new input fields
