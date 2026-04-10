# Claude Permissions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust binary that acts as a PreToolUse hook for Claude Code, evaluating Bash commands against Rego policies.

**Architecture:** Rust handles JSON I/O, command tokenization, wrapper extraction, flag normalization, and path resolution. Regorus (embedded Rego) evaluates policies. Fail-safe default is `ask`.

**Tech Stack:** Rust, regorus, serde_json, shlex, tracing

---

### Task 1: Project Setup

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

**Step 1: Initialize Cargo project**

Run: `cargo init --name cmdguard`

**Step 2: Configure Cargo.toml with dependencies**

Replace `Cargo.toml` with:

```toml
[package]
name = "cmdguard"
version = "0.1.0"
edition = "2021"
description = "PreToolUse hook for policy-driven permission control"

[dependencies]
regorus = "0.2"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
shlex = "1.3"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
dirs = "5.0"

[profile.release]
lto = true
strip = true
```

**Step 3: Write minimal main.rs that compiles**

Replace `src/main.rs` with:

```rust
fn main() {
    println!("cmdguard stub");
}
```

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: initialize cargo project with dependencies"
```

---

### Task 2: Input Parsing

**Files:**
- Create: `src/input.rs`
- Modify: `src/main.rs`

**Step 1: Write failing test for input parsing**

Create `src/input.rs`:

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    pub tool_input: ToolInput,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToolInput {
    pub command: String,
}

pub fn parse_input(json: &str) -> Result<HookInput, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_input() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let input = parse_input(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.tool_input.command, "git status");
    }

    #[test]
    fn test_parse_input_with_cwd() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},"cwd":"/home/user"}"#;
        let input = parse_input(json).unwrap();
        assert_eq!(input.cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "not json";
        assert!(parse_input(json).is_err());
    }
}
```

**Step 2: Update main.rs to declare module**

Replace `src/main.rs`:

```rust
mod input;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test input`
Expected: All 3 tests pass

**Step 4: Commit**

```bash
git add src/input.rs src/main.rs
git commit -m "feat: add input parsing with serde"
```

---

### Task 3: Command Tokenization

**Files:**
- Create: `src/tokenizer.rs`
- Modify: `src/main.rs`

**Step 1: Write tokenizer module with tests**

Create `src/tokenizer.rs`:

```rust
/// Tokenize a command string respecting quotes
pub fn tokenize(command: &str) -> Result<Vec<String>, String> {
    shlex::split(command).ok_or_else(|| "Failed to tokenize command".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let tokens = tokenize("git status").unwrap();
        assert_eq!(tokens, vec!["git", "status"]);
    }

    #[test]
    fn test_command_with_flags() {
        let tokens = tokenize("rm -rf build/").unwrap();
        assert_eq!(tokens, vec!["rm", "-rf", "build/"]);
    }

    #[test]
    fn test_command_with_quotes() {
        let tokens = tokenize(r#"echo "hello world""#).unwrap();
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn test_command_with_single_quotes() {
        let tokens = tokenize("bash -c 'git status'").unwrap();
        assert_eq!(tokens, vec!["bash", "-c", "git status"]);
    }

    #[test]
    fn test_nested_quotes() {
        let tokens = tokenize(r#"bash -c "echo 'hello'""#).unwrap();
        assert_eq!(tokens, vec!["bash", "-c", "echo 'hello'"]);
    }
}
```

**Step 2: Update main.rs**

Add to `src/main.rs`:

```rust
mod input;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test tokenizer`
Expected: All 5 tests pass

**Step 4: Commit**

```bash
git add src/tokenizer.rs src/main.rs
git commit -m "feat: add command tokenization using shlex"
```

---

### Task 4: Flag Expansion

**Files:**
- Create: `src/flags.rs`
- Modify: `src/main.rs`

**Step 1: Write flag expansion module**

Create `src/flags.rs`:

```rust
/// Expand combined short flags (-rf -> -r, -f)
pub fn expand_flags(tokens: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for token in tokens {
        if is_combined_short_flag(token) {
            // Skip the leading '-' and expand each char
            for c in token[1..].chars() {
                expanded.push(format!("-{}", c));
            }
        } else if token.starts_with('-') {
            expanded.push(token.clone());
        }
    }
    expanded
}

fn is_combined_short_flag(token: &str) -> bool {
    // Must start with single dash, have multiple chars after dash,
    // and not be a long flag (--) or contain =
    token.starts_with('-')
        && !token.starts_with("--")
        && token.len() > 2
        && !token.contains('=')
        && token[1..].chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_expand_combined_flags() {
        let tokens = to_vec(&["rm", "-rf", "build/"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-r", "-f"]);
    }

    #[test]
    fn test_preserve_separate_flags() {
        let tokens = to_vec(&["rm", "-r", "-f", "build/"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-r", "-f"]);
    }

    #[test]
    fn test_preserve_long_flags() {
        let tokens = to_vec(&["git", "push", "--force"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["--force"]);
    }

    #[test]
    fn test_mixed_flags() {
        let tokens = to_vec(&["cmd", "-abc", "--verbose", "-x"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-a", "-b", "-c", "--verbose", "-x"]);
    }

    #[test]
    fn test_flag_with_value() {
        // -o=file should not be expanded
        let tokens = to_vec(&["gcc", "-o=output", "-Wall"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-o=output", "-W", "-a", "-l", "-l"]);
    }

    #[test]
    fn test_no_flags() {
        let tokens = to_vec(&["echo", "hello"]);
        let flags = expand_flags(&tokens);
        assert!(flags.is_empty());
    }
}
```

**Step 2: Update main.rs**

```rust
mod flags;
mod input;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test flags`
Expected: All 6 tests pass

**Step 4: Commit**

```bash
git add src/flags.rs src/main.rs
git commit -m "feat: add flag expansion (-rf -> -r, -f)"
```

---

### Task 5: Wrapper Extraction

**Files:**
- Create: `src/extractor.rs`
- Modify: `src/main.rs`

**Step 1: Write wrapper extraction module**

Create `src/extractor.rs`:

```rust
use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedCommand {
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
}

/// Extract the real command from wrapper commands
pub fn extract_command(tokens: &[String]) -> ExtractedCommand {
    let mut wrapper_chain = Vec::new();
    let mut current = tokens.to_vec();

    loop {
        match try_extract_wrapper(&current) {
            Some((wrapper, inner)) => {
                wrapper_chain.push(wrapper);
                current = inner;
            }
            None => break,
        }
    }

    ExtractedCommand {
        command: current,
        wrapper_chain,
    }
}

fn try_extract_wrapper(tokens: &[String]) -> Option<(String, Vec<String>)> {
    if tokens.is_empty() {
        return None;
    }

    let cmd = &tokens[0];

    match cmd.as_str() {
        "sudo" => extract_sudo(tokens),
        "env" => extract_env(tokens),
        "nix" => extract_nix(tokens),
        "nix-shell" => extract_nix_shell(tokens),
        "docker" => extract_docker(tokens),
        "sh" | "bash" | "zsh" => extract_shell_c(tokens),
        _ => None,
    }
}

fn extract_sudo(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // sudo [options] command
    // Skip sudo and any flags starting with -
    let mut idx = 1;
    while idx < tokens.len() && tokens[idx].starts_with('-') {
        idx += 1;
    }
    if idx < tokens.len() {
        Some(("sudo".to_string(), tokens[idx..].to_vec()))
    } else {
        None
    }
}

fn extract_env(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // env [VAR=val]... command
    let mut idx = 1;
    while idx < tokens.len() && tokens[idx].contains('=') {
        idx += 1;
    }
    if idx < tokens.len() {
        Some(("env".to_string(), tokens[idx..].to_vec()))
    } else {
        None
    }
}

fn extract_nix(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // nix develop --command <cmd>
    // nix shell --command <cmd>
    if tokens.len() < 2 {
        return None;
    }

    let subcommand = &tokens[1];
    if subcommand != "develop" && subcommand != "shell" {
        return None;
    }

    // Find --command flag
    for (i, token) in tokens.iter().enumerate() {
        if token == "--command" || token == "-c" {
            if i + 1 < tokens.len() {
                let wrapper = format!("nix {}", subcommand);
                return Some((wrapper, tokens[i + 1..].to_vec()));
            }
        }
    }
    None
}

fn extract_nix_shell(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // nix-shell --run "command"
    for (i, token) in tokens.iter().enumerate() {
        if token == "--run" {
            if i + 1 < tokens.len() {
                // The next token is a quoted command string, need to re-tokenize
                if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                    return Some(("nix-shell".to_string(), inner_tokens));
                }
            }
        }
    }
    None
}

fn extract_docker(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // docker run [options] image [command]
    // docker exec [options] container command
    if tokens.len() < 2 {
        return None;
    }

    let subcommand = &tokens[1];
    if subcommand != "run" && subcommand != "exec" {
        return None;
    }

    // Find where options end and command begins
    // This is tricky - we look for patterns that indicate end of docker args
    let mut idx = 2;
    while idx < tokens.len() {
        let token = &tokens[idx];

        // Skip known docker flags that take values
        if token.starts_with('-') {
            idx += 1;
            // If it's a flag that takes a value (not --flag=value form), skip the value too
            if !token.contains('=') && idx < tokens.len() && !tokens[idx].starts_with('-') {
                // Heuristic: common docker flags that take values
                let takes_value = matches!(
                    token.as_str(),
                    "-e" | "--env" | "-v" | "--volume" | "-p" | "--publish" |
                    "-w" | "--workdir" | "--name" | "-u" | "--user" |
                    "--network" | "--entrypoint" | "-m" | "--memory"
                );
                if takes_value {
                    idx += 1;
                }
            }
            continue;
        }

        // First non-flag is the image (for run) or container (for exec)
        // The rest is the command
        if idx + 1 < tokens.len() {
            let wrapper = format!("docker {}", subcommand);
            return Some((wrapper, tokens[idx + 1..].to_vec()));
        }
        break;
    }
    None
}

fn extract_shell_c(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // bash -c "command"
    // sh -c "command"
    let shell = &tokens[0];

    for (i, token) in tokens.iter().enumerate() {
        if token == "-c" {
            if i + 1 < tokens.len() {
                // The next token is a quoted command string, need to re-tokenize
                if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                    return Some((shell.clone(), inner_tokens));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_no_wrapper() {
        let tokens = to_vec(&["git", "status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_sudo() {
        let tokens = to_vec(&["sudo", "rm", "-rf", "/"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["rm", "-rf", "/"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_sudo_with_flags() {
        let tokens = to_vec(&["sudo", "-u", "root", "ls"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["ls"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_env() {
        let tokens = to_vec(&["env", "FOO=bar", "BAZ=qux", "echo", "hello"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["echo", "hello"]));
        assert_eq!(result.wrapper_chain, vec!["env"]);
    }

    #[test]
    fn test_nix_develop() {
        let tokens = to_vec(&["nix", "develop", "--command", "git", "status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["nix develop"]);
    }

    #[test]
    fn test_bash_c() {
        let tokens = to_vec(&["bash", "-c", "git status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["bash"]);
    }

    #[test]
    fn test_nested_wrappers() {
        let tokens = to_vec(&["sudo", "bash", "-c", "git status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["sudo", "bash"]);
    }

    #[test]
    fn test_nix_shell_run() {
        let tokens = to_vec(&["nix-shell", "--run", "cargo build"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["cargo", "build"]));
        assert_eq!(result.wrapper_chain, vec!["nix-shell"]);
    }
}
```

**Step 2: Update main.rs**

```rust
mod extractor;
mod flags;
mod input;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test extractor`
Expected: All 8 tests pass

**Step 4: Commit**

```bash
git add src/extractor.rs src/main.rs
git commit -m "feat: add wrapper extraction for sudo, nix, docker, etc."
```

---

### Task 6: Path Detection and Resolution

**Files:**
- Create: `src/paths.rs`
- Modify: `src/main.rs`

**Step 1: Write path detection module**

Create `src/paths.rs`:

```rust
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DetectedPath {
    pub raw: String,
    pub resolved: String,
    pub exists: bool,
    pub is_dir: bool,
}

/// Detect and resolve paths in command arguments
pub fn detect_paths(tokens: &[String], cwd: &Path) -> Vec<DetectedPath> {
    tokens
        .iter()
        .filter(|t| looks_like_path(t))
        .map(|t| resolve_path(t, cwd))
        .collect()
}

fn looks_like_path(token: &str) -> bool {
    // Skip flags
    if token.starts_with('-') {
        return false;
    }

    // Contains path separator
    if token.contains('/') || token.contains('\\') {
        return true;
    }

    // Starts with . (relative path)
    if token.starts_with('.') {
        return true;
    }

    // Check if it exists on filesystem (but not just any word)
    // Only do this for tokens that have some path-like quality
    false
}

fn resolve_path(raw: &str, cwd: &Path) -> DetectedPath {
    let path = Path::new(raw);

    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    // Canonicalize if possible (resolves .., symlinks)
    let resolved = resolved.canonicalize().unwrap_or(resolved);

    let exists = resolved.exists();
    let is_dir = resolved.is_dir();

    DetectedPath {
        raw: raw.to_string(),
        resolved: resolved.to_string_lossy().to_string(),
        exists,
        is_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_detect_absolute_path() {
        let tokens = to_vec(&["rm", "-rf", "/tmp/foo"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "/tmp/foo");
        assert!(paths[0].resolved.starts_with("/tmp/foo"));
    }

    #[test]
    fn test_detect_relative_path() {
        let tokens = to_vec(&["rm", "-rf", "./build"]);
        let cwd = env::current_dir().unwrap();
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "./build");
    }

    #[test]
    fn test_detect_path_with_slash() {
        let tokens = to_vec(&["cat", "src/main.rs"]);
        let cwd = env::current_dir().unwrap();
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "src/main.rs");
    }

    #[test]
    fn test_skip_flags() {
        let tokens = to_vec(&["ls", "-la", "/tmp"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        // Should only detect /tmp, not -la
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "/tmp");
    }

    #[test]
    fn test_no_paths() {
        let tokens = to_vec(&["echo", "hello", "world"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        assert!(paths.is_empty());
    }
}
```

**Step 2: Update main.rs**

```rust
mod extractor;
mod flags;
mod input;
mod paths;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test paths`
Expected: All 5 tests pass

**Step 4: Commit**

```bash
git add src/paths.rs src/main.rs
git commit -m "feat: add path detection and resolution"
```

---

### Task 7: Output Formatting

**Files:**
- Create: `src/output.rs`
- Modify: `src/main.rs`

**Step 1: Write output module**

Create `src/output.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub hook_specific_output: HookSpecificOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
}

impl HookOutput {
    pub fn new(decision: Decision, reason: Option<String>) -> Self {
        HookOutput {
            hook_specific_output: HookSpecificOutput {
                permission_decision: decision.as_str().to_string(),
                updated_input: None,
            },
            system_message: reason,
        }
    }

    pub fn allow() -> Self {
        Self::new(Decision::Allow, None)
    }

    pub fn deny(reason: &str) -> Self {
        Self::new(Decision::Deny, Some(reason.to_string()))
    }

    pub fn ask() -> Self {
        Self::new(Decision::Ask, None)
    }

    pub fn ask_with_reason(reason: &str) -> Self {
        Self::new(Decision::Ask, Some(reason.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"hookSpecificOutput":{"permissionDecision":"ask"}}"#.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_output() {
        let output = HookOutput::allow();
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"allow""#));
        assert!(!json.contains("systemMessage"));
    }

    #[test]
    fn test_deny_output() {
        let output = HookOutput::deny("blocked by policy");
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"deny""#));
        assert!(json.contains(r#""systemMessage":"blocked by policy""#));
    }

    #[test]
    fn test_ask_output() {
        let output = HookOutput::ask();
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"ask""#));
    }
}
```

**Step 2: Update main.rs**

```rust
mod extractor;
mod flags;
mod input;
mod output;
mod paths;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test output`
Expected: All 3 tests pass

**Step 4: Commit**

```bash
git add src/output.rs src/main.rs
git commit -m "feat: add output formatting for hook responses"
```

---

### Task 8: Policy Evaluation with Regorus

**Files:**
- Create: `src/policy.rs`
- Create: `policies/stdlib.rego`
- Create: `policies/test_policy.rego`
- Modify: `src/main.rs`

**Step 1: Create stdlib.rego**

Create `policies/stdlib.rego`:

```rego
package cmdguard.stdlib

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
    count(input.paths) > 0
    every path in input.paths {
        startswith(path.resolved, input.project_root)
    }
}

# No paths provided
no_paths {
    count(input.paths) == 0
}
```

**Step 2: Create test policy**

Create `policies/test_policy.rego`:

```rego
package cmdguard

import data.cmdguard.stdlib

default decision = "ask"

# Allow safe git commands
decision = "allow" {
    input.command[0] == "git"
    stdlib.git_subcommand in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

# Deny force push
decision = "deny" {
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
    "--force" in input.command
}

# Reasons
reason = "Safe git read operation" {
    decision == "allow"
    input.command[0] == "git"
}

reason = "Force push blocked by policy" {
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
    "--force" in input.command
}
```

**Step 3: Write policy evaluation module**

Create `src/policy.rs`:

```rust
use crate::output::Decision;
use crate::paths::DetectedPath;
use regorus::Engine;
use serde::Serialize;
use std::path::Path;
use tracing::{debug, warn};

#[derive(Debug, Serialize)]
pub struct PolicyInput {
    pub tool: String,
    pub raw_command: String,
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
    pub flags_expanded: Vec<String>,
    pub paths: Vec<DetectedPath>,
    pub cwd: String,
    pub project_root: String,
    pub session_id: String,
}

pub struct PolicyResult {
    pub decision: Decision,
    pub reason: Option<String>,
}

pub struct PolicyEngine {
    engine: Engine,
}

impl PolicyEngine {
    pub fn new() -> Self {
        PolicyEngine {
            engine: Engine::new(),
        }
    }

    pub fn load_policy_file(&mut self, path: &Path) -> Result<(), String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read policy file {:?}: {}", path, e))?;

        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("policy.rego");

        self.engine
            .add_policy(filename.to_string(), contents)
            .map_err(|e| format!("Failed to parse policy {:?}: {}", path, e))
    }

    pub fn load_policies_from_dir(&mut self, dir: &Path) -> Result<(), String> {
        if !dir.exists() {
            return Err(format!("Policy directory {:?} does not exist", dir));
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("Failed to read policy directory {:?}: {}", dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("rego") {
                debug!("Loading policy file: {:?}", path);
                self.load_policy_file(&path)?;
            }
        }

        Ok(())
    }

    pub fn evaluate(&mut self, input: &PolicyInput) -> PolicyResult {
        // Set input data
        let input_json = match serde_json::to_value(input) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to serialize policy input: {}", e);
                return PolicyResult {
                    decision: Decision::Ask,
                    reason: Some("Internal error serializing input".to_string()),
                };
            }
        };

        if let Err(e) = self.engine.set_input(input_json) {
            warn!("Failed to set policy input: {}", e);
            return PolicyResult {
                decision: Decision::Ask,
                reason: Some("Internal error setting input".to_string()),
            };
        }

        // Evaluate decision
        let decision = self.eval_decision();
        let reason = self.eval_reason();

        PolicyResult { decision, reason }
    }

    fn eval_decision(&mut self) -> Decision {
        match self.engine.eval_rule("data.cmdguard.decision".to_string()) {
            Ok(results) => {
                if let Some(value) = results.result.as_str() {
                    match value {
                        "allow" => Decision::Allow,
                        "deny" => Decision::Deny,
                        _ => Decision::Ask,
                    }
                } else {
                    Decision::Ask
                }
            }
            Err(e) => {
                warn!("Failed to evaluate decision: {}", e);
                Decision::Ask
            }
        }
    }

    fn eval_reason(&mut self) -> Option<String> {
        match self.engine.eval_rule("data.cmdguard.reason".to_string()) {
            Ok(results) => results.result.as_str().map(|s| s.to_string()),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_input(command: Vec<&str>) -> PolicyInput {
        PolicyInput {
            tool: "Bash".to_string(),
            raw_command: command.join(" "),
            command: command.iter().map(|s| s.to_string()).collect(),
            wrapper_chain: vec![],
            flags_expanded: vec![],
            paths: vec![],
            cwd: "/home/user/project".to_string(),
            project_root: "/home/user/project".to_string(),
            session_id: "test".to_string(),
        }
    }

    #[test]
    fn test_load_and_evaluate_policy() {
        let mut engine = PolicyEngine::new();

        // Load from policies directory
        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        // Test allowed command
        let input = make_input(vec!["git", "status"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Allow);
        assert_eq!(result.reason, Some("Safe git read operation".to_string()));
    }

    #[test]
    fn test_deny_force_push() {
        let mut engine = PolicyEngine::new();

        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["git", "push", "--force", "origin", "main"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Deny);
        assert!(result.reason.unwrap().contains("Force push"));
    }

    #[test]
    fn test_ask_for_unknown() {
        let mut engine = PolicyEngine::new();

        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["curl", "https://example.com"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Ask);
    }
}
```

**Step 4: Update main.rs**

```rust
mod extractor;
mod flags;
mod input;
mod output;
mod paths;
mod policy;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 5: Create policies directory structure**

Run: `mkdir -p policies`

**Step 6: Run tests**

Run: `cargo test policy`
Expected: All 3 tests pass

**Step 7: Commit**

```bash
git add src/policy.rs src/main.rs policies/
git commit -m "feat: add policy evaluation with regorus"
```

---

### Task 9: Logging Setup

**Files:**
- Create: `src/logging.rs`
- Modify: `src/main.rs`

**Step 1: Write logging module**

Create `src/logging.rs`:

```rust
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init_logging() -> Option<WorkerGuard> {
    // Only enable logging if RUST_LOG is set
    let filter = match std::env::var("RUST_LOG") {
        Ok(f) => f,
        Err(_) => return None,
    };

    // Create log directory
    let log_dir = dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("cmdguard");

    if std::fs::create_dir_all(&log_dir).is_err() {
        return None;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "debug.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(false),
        )
        .init();

    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_disabled_by_default() {
        // Clear RUST_LOG if set
        std::env::remove_var("RUST_LOG");
        let guard = init_logging();
        assert!(guard.is_none());
    }
}
```

**Step 2: Update main.rs**

```rust
mod extractor;
mod flags;
mod input;
mod logging;
mod output;
mod paths;
mod policy;
mod tokenizer;

fn main() {
    println!("cmdguard stub");
}
```

**Step 3: Run tests**

Run: `cargo test logging`
Expected: Test passes

**Step 4: Commit**

```bash
git add src/logging.rs src/main.rs
git commit -m "feat: add file-based logging with RUST_LOG"
```

---

### Task 10: Main Entry Point Integration

**Files:**
- Modify: `src/main.rs`

**Step 1: Write complete main.rs**

Replace `src/main.rs`:

```rust
mod extractor;
mod flags;
mod input;
mod logging;
mod output;
mod paths;
mod policy;
mod tokenizer;

use extractor::extract_command;
use flags::expand_flags;
use input::parse_input;
use logging::init_logging;
use output::HookOutput;
use paths::detect_paths;
use policy::{PolicyEngine, PolicyInput};
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{debug, error, info};

fn main() {
    let _guard = init_logging();

    let start = Instant::now();

    let result = run();

    let elapsed = start.elapsed();
    debug!(total_ms = elapsed.as_secs_f64() * 1000.0, "Completed");

    match result {
        Ok(output) => {
            println!("{}", output.to_json());
        }
        Err(e) => {
            error!("Error: {}", e);
            // Fail safe: return ask on any error
            println!("{}", HookOutput::ask_with_reason(&e).to_json());
        }
    }
}

fn run() -> Result<HookOutput, String> {
    // Read input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    debug!(input = %input_str, "Received input");

    // Parse input JSON
    let hook_input = parse_input(&input_str)
        .map_err(|e| format!("Failed to parse input: {}", e))?;

    // Only handle Bash tool
    if hook_input.tool_name != "Bash" {
        return Ok(HookOutput::ask_with_reason("Not a Bash command"));
    }

    let raw_command = &hook_input.tool_input.command;
    let cwd = hook_input.cwd.clone().unwrap_or_else(|| ".".to_string());
    let cwd_path = PathBuf::from(&cwd);
    let session_id = hook_input.session_id.clone().unwrap_or_default();

    // Tokenize command
    let tokens = tokenizer::tokenize(raw_command)
        .map_err(|e| format!("Failed to tokenize: {}", e))?;

    if tokens.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty command"));
    }

    // Extract from wrappers
    let extracted = extract_command(&tokens);
    debug!(
        raw = ?tokens,
        extracted = ?extracted.command,
        wrappers = ?extracted.wrapper_chain,
        "Extracted command"
    );

    if extracted.command.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty extracted command"));
    }

    // Expand flags
    let flags_expanded = expand_flags(&extracted.command);
    debug!(flags = ?flags_expanded, "Expanded flags");

    // Detect paths
    let paths = detect_paths(&extracted.command, &cwd_path);
    debug!(paths = ?paths, "Detected paths");

    // Build policy input
    let policy_input = PolicyInput {
        tool: hook_input.tool_name,
        raw_command: raw_command.clone(),
        command: extracted.command,
        wrapper_chain: extracted.wrapper_chain,
        flags_expanded,
        paths,
        cwd: cwd.clone(),
        project_root: cwd, // For now, assume cwd is project root
        session_id,
    };

    // Load and evaluate policy
    let compile_start = Instant::now();
    let mut engine = PolicyEngine::new();

    // Load policies from config directory
    let config_dir = dirs::config_dir()
        .map(|d| d.join("cmdguard"))
        .unwrap_or_else(|| PathBuf::from("/etc/cmdguard"));

    if config_dir.exists() {
        engine.load_policies_from_dir(&config_dir)?;
    } else {
        info!("Config directory {:?} not found, using defaults", config_dir);
        return Ok(HookOutput::ask_with_reason("No policy configured"));
    }

    let compile_elapsed = compile_start.elapsed();
    debug!(compile_ms = compile_elapsed.as_secs_f64() * 1000.0, "Compiled policies");

    // Evaluate
    let eval_start = Instant::now();
    let result = engine.evaluate(&policy_input);
    let eval_elapsed = eval_start.elapsed();

    info!(
        decision = ?result.decision,
        reason = ?result.reason,
        compile_ms = compile_elapsed.as_secs_f64() * 1000.0,
        eval_ms = eval_elapsed.as_secs_f64() * 1000.0,
        command = ?policy_input.command,
        "Policy evaluation complete"
    );

    Ok(HookOutput::new(result.decision, result.reason))
}
```

**Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Build release**

Run: `cargo build --release`
Expected: Builds successfully

**Step 4: Test manually**

Run:
```bash
mkdir -p ~/.config/cmdguard
cp policies/*.rego ~/.config/cmdguard/
echo '{"tool_name":"Bash","tool_input":{"command":"git status"},"cwd":"/tmp"}' | ./target/release/cmdguard
```

Expected: Output contains `"permissionDecision":"allow"`

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: integrate all components in main entry point"
```

---

### Task 11: Example Policy

**Files:**
- Create: `examples/policy.rego`

**Step 1: Write example policy**

Create `examples/policy.rego`:

```rego
package cmdguard

import data.cmdguard.stdlib

default decision = "ask"

# =============================================================================
# ALLOW RULES
# =============================================================================

# Allow safe git read commands
decision = "allow" {
    input.command[0] == "git"
    stdlib.git_subcommand in {
        "status", "diff", "log", "branch", "show",
        "fetch", "stash", "remote", "tag", "describe"
    }
}

# Allow git add/commit (common safe operations)
decision = "allow" {
    input.command[0] == "git"
    stdlib.git_subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

# Allow cargo commands
decision = "allow" {
    input.command[0] == "cargo"
    input.command[1] in {"build", "test", "check", "fmt", "clippy", "run", "doc"}
}

# Allow npm/yarn/pnpm safe commands
decision = "allow" {
    input.command[0] in {"npm", "yarn", "pnpm"}
    input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

# Allow common read-only commands
decision = "allow" {
    input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Allow echo and printf
decision = "allow" {
    input.command[0] in {"echo", "printf"}
}

# =============================================================================
# DENY RULES
# =============================================================================

# Deny git push --force
decision = "deny" {
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
    has_force_flag
}

has_force_flag {
    some flag in input.command
    flag in {"--force", "-f", "--force-with-lease"}
}

# Deny rm -rf outside project root
decision = "deny" {
    input.command[0] == "rm"
    "-r" in input.flags_expanded
    stdlib.path_outside_project
}

# Deny dangerous commands entirely
decision = "deny" {
    input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

# =============================================================================
# REASONS
# =============================================================================

reason = "Safe git read operation" {
    decision == "allow"
    input.command[0] == "git"
}

reason = "Safe cargo operation" {
    decision == "allow"
    input.command[0] == "cargo"
}

reason = "Safe package manager operation" {
    decision == "allow"
    input.command[0] in {"npm", "yarn", "pnpm"}
}

reason = "Read-only command" {
    decision == "allow"
    input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which", "echo", "printf"}
}

reason = "Force push is blocked - use regular push instead" {
    decision == "deny"
    input.command[0] == "git"
    stdlib.git_subcommand == "push"
}

reason = "Recursive delete outside project root is blocked" {
    decision == "deny"
    input.command[0] == "rm"
    "-r" in input.flags_expanded
}

reason = "This command is blocked for safety" {
    decision == "deny"
    input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}
```

**Step 2: Commit**

```bash
git add examples/
git commit -m "feat: add comprehensive example policy"
```

---

### Task 12: Installation Script

**Files:**
- Create: `install.sh`

**Step 1: Write installation script**

Create `install.sh`:

```bash
#!/bin/bash
set -euo pipefail

echo "Building cmdguard..."
cargo build --release

echo "Installing binary..."
mkdir -p ~/.local/bin
cp target/release/cmdguard ~/.local/bin/
chmod +x ~/.local/bin/cmdguard

echo "Installing policies..."
mkdir -p ~/.config/cmdguard
cp policies/stdlib.rego ~/.config/cmdguard/

# Only copy example policy if no policy.rego exists
if [ ! -f ~/.config/cmdguard/policy.rego ]; then
    cp examples/policy.rego ~/.config/cmdguard/
    echo "Installed example policy.rego"
else
    echo "Existing policy.rego preserved"
fi

echo ""
echo "Installation complete!"
echo ""
echo "Add this to your ~/.claude/settings.json:"
echo ""
cat << 'EOF'
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/cmdguard"
          }
        ]
      }
    ]
  }
}
EOF
echo ""
echo "Policy files are in ~/.config/cmdguard/"
echo "Edit policy.rego to customize your rules."
```

**Step 2: Make executable**

Run: `chmod +x install.sh`

**Step 3: Commit**

```bash
git add install.sh
git commit -m "feat: add installation script"
```

---

### Task 13: README

**Files:**
- Create: `README.md`

**Step 1: Write README**

Create `README.md`:

```markdown
# cmdguard

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
            "command": "~/.local/bin/cmdguard"
          }
        ]
      }
    ]
  }
}
```

## Configuration

Policies live in `~/.config/cmdguard/`:

- `stdlib.rego` - Standard helpers (git_subcommand, path checks, etc.)
- `policy.rego` - Your custom rules

## Writing Policies

```rego
package cmdguard

import data.cmdguard.stdlib

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
cmdguard test

# Run with verbose output
cmdguard test --verbose

# Run specific test file
cmdguard test my_tests.yaml
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
cmdguard eval "git status"
cmdguard eval "nix develop --command cargo build"
```

Enable logging:

```bash
export RUST_LOG=debug
```

Logs written to `~/.local/state/cmdguard/debug.log`

## License

MIT
```

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add README with usage instructions"
```

---

### Task 14: CLI with Test Subcommand

**Files:**
- Modify: `Cargo.toml`
- Create: `src/cli.rs`
- Create: `src/test_runner.rs`
- Modify: `src/main.rs`

**Step 1: Add dependencies to Cargo.toml**

Add to `[dependencies]` in `Cargo.toml`:

```toml
clap = { version = "4.5", features = ["derive"] }
serde_yaml = "0.9"
```

**Step 2: Create CLI module**

Create `src/cli.rs`:

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cmdguard")]
#[command(about = "Policy-driven permission control for Claude Code")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run policy tests from a YAML file
    Test {
        /// Path to test file (default: looks for policy_tests.yaml in policy dir)
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,

        /// Show detailed output for each test
        #[arg(short, long)]
        verbose: bool,

        /// Policy directory (default: ~/.config/cmdguard)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,
    },

    /// Evaluate a single command (for debugging)
    Eval {
        /// The command to evaluate
        command: String,

        /// Working directory context
        #[arg(short, long, default_value = ".")]
        cwd: String,

        /// Policy directory
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,
    },
}
```

**Step 3: Create test runner module**

Create `src/test_runner.rs`:

```rust
use crate::extractor::extract_command;
use crate::flags::expand_flags;
use crate::output::Decision;
use crate::paths::detect_paths;
use crate::policy::{PolicyEngine, PolicyInput};
use crate::tokenizer::tokenize;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct TestFile {
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub command: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    pub expect: ExpectedDecision,
    #[serde(default)]
    pub reason_contains: Option<String>,
}

fn default_cwd() -> String {
    "/home/user/project".to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedDecision {
    Allow,
    Deny,
    Ask,
}

impl ExpectedDecision {
    fn matches(&self, decision: Decision) -> bool {
        match (self, decision) {
            (ExpectedDecision::Allow, Decision::Allow) => true,
            (ExpectedDecision::Deny, Decision::Deny) => true,
            (ExpectedDecision::Ask, Decision::Ask) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub expected: ExpectedDecision,
    pub actual: Decision,
    pub reason: Option<String>,
    pub error: Option<String>,
}

pub struct TestRunner {
    engine: PolicyEngine,
}

impl TestRunner {
    pub fn new(policy_dir: &Path) -> Result<Self, String> {
        let mut engine = PolicyEngine::new();
        engine.load_policies_from_dir(policy_dir)?;
        Ok(TestRunner { engine })
    }

    pub fn run_tests(&mut self, test_file: &TestFile) -> Vec<TestResult> {
        test_file
            .tests
            .iter()
            .map(|tc| self.run_single_test(tc))
            .collect()
    }

    fn run_single_test(&mut self, test: &TestCase) -> TestResult {
        // Tokenize
        let tokens = match tokenize(&test.command) {
            Ok(t) => t,
            Err(e) => {
                return TestResult {
                    name: test.name.clone(),
                    passed: false,
                    expected: test.expect,
                    actual: Decision::Ask,
                    reason: None,
                    error: Some(format!("Tokenize error: {}", e)),
                }
            }
        };

        if tokens.is_empty() {
            return TestResult {
                name: test.name.clone(),
                passed: false,
                expected: test.expect,
                actual: Decision::Ask,
                reason: None,
                error: Some("Empty command".to_string()),
            };
        }

        // Process command
        let extracted = extract_command(&tokens);
        let flags_expanded = expand_flags(&extracted.command);
        let cwd_path = PathBuf::from(&test.cwd);
        let paths = detect_paths(&extracted.command, &cwd_path);

        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: test.command.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: test.cwd.clone(),
            project_root: test.cwd.clone(),
            session_id: "test".to_string(),
        };

        // Evaluate
        let result = self.engine.evaluate(&policy_input);

        // Check result
        let decision_matches = test.expect.matches(result.decision);
        let reason_matches = test.reason_contains.as_ref().map_or(true, |expected| {
            result
                .reason
                .as_ref()
                .map_or(false, |r| r.contains(expected))
        });

        TestResult {
            name: test.name.clone(),
            passed: decision_matches && reason_matches,
            expected: test.expect,
            actual: result.decision,
            reason: result.reason,
            error: if !reason_matches {
                Some(format!(
                    "Reason '{}' does not contain '{}'",
                    result.reason.as_deref().unwrap_or("(none)"),
                    test.reason_contains.as_deref().unwrap_or("")
                ))
            } else {
                None
            },
        }
    }
}

pub fn load_test_file(path: &Path) -> Result<TestFile, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read test file {:?}: {}", path, e))?;

    serde_yaml::from_str(&contents)
        .map_err(|e| format!("Failed to parse test file {:?}: {}", path, e))
}

pub fn print_results(results: &[TestResult], verbose: bool) {
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();

    if verbose {
        for result in results {
            let status = if result.passed { "✓" } else { "✗" };
            let decision_str = format!("{:?}", result.actual).to_lowercase();

            println!("{} {} -> {} (expected {:?})",
                status,
                result.name,
                decision_str,
                result.expected
            );

            if !result.passed {
                if let Some(ref err) = result.error {
                    println!("    Error: {}", err);
                }
                if let Some(ref reason) = result.reason {
                    println!("    Reason: {}", reason);
                }
            }
        }
        println!();
    }

    if passed == total {
        println!("✓ {}/{} tests passed", passed, total);
    } else {
        println!("✗ {}/{} tests passed", passed, total);

        if !verbose {
            println!("\nFailed tests:");
            for result in results.iter().filter(|r| !r.passed) {
                println!("  - {} (expected {:?}, got {:?})",
                    result.name,
                    result.expected,
                    result.actual
                );
                if let Some(ref err) = result.error {
                    println!("    {}", err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
tests:
  - name: "test allow"
    command: "git status"
    expect: allow
  - name: "test deny"
    command: "rm -rf /"
    expect: deny
    reason_contains: "blocked"
"#;
        let test_file: TestFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(test_file.tests.len(), 2);
        assert_eq!(test_file.tests[0].name, "test allow");
        assert_eq!(test_file.tests[0].expect, ExpectedDecision::Allow);
        assert_eq!(test_file.tests[1].reason_contains, Some("blocked".to_string()));
    }
}
```

**Step 4: Update main.rs for CLI**

Replace `src/main.rs`:

```rust
mod cli;
mod extractor;
mod flags;
mod input;
mod logging;
mod output;
mod paths;
mod policy;
mod test_runner;
mod tokenizer;

use clap::Parser;
use cli::{Cli, Commands};
use extractor::extract_command;
use flags::expand_flags;
use input::parse_input;
use logging::init_logging;
use output::HookOutput;
use paths::detect_paths;
use policy::{PolicyEngine, PolicyInput};
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use test_runner::{load_test_file, print_results, TestRunner};
use tracing::{debug, error, info};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Test {
            file,
            verbose,
            policy_dir,
        }) => {
            run_tests(file, verbose, policy_dir);
        }
        Some(Commands::Eval {
            command,
            cwd,
            policy_dir,
        }) => {
            run_eval(&command, &cwd, policy_dir);
        }
        None => {
            // Default: run as hook (read from stdin)
            run_hook();
        }
    }
}

fn get_policy_dir(override_dir: Option<PathBuf>) -> PathBuf {
    override_dir.unwrap_or_else(|| {
        dirs::config_dir()
            .map(|d| d.join("cmdguard"))
            .unwrap_or_else(|| PathBuf::from("/etc/cmdguard"))
    })
}

fn run_tests(file: Option<PathBuf>, verbose: bool, policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);

    // Find test file
    let test_file_path = file.unwrap_or_else(|| policy_dir.join("policy_tests.yaml"));

    if !test_file_path.exists() {
        eprintln!("Test file not found: {:?}", test_file_path);
        eprintln!("Create a test file or specify one with: cmdguard test <file>");
        std::process::exit(1);
    }

    // Load tests
    let test_file = match load_test_file(&test_file_path) {
        Ok(tf) => tf,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Create runner
    let mut runner = match TestRunner::new(&policy_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading policies: {}", e);
            std::process::exit(1);
        }
    };

    // Run tests
    let results = runner.run_tests(&test_file);
    print_results(&results, verbose);

    // Exit with error if any failed
    if results.iter().any(|r| !r.passed) {
        std::process::exit(1);
    }
}

fn run_eval(command: &str, cwd: &str, policy_dir: Option<PathBuf>) {
    let _guard = init_logging();
    let policy_dir = get_policy_dir(policy_dir);

    // Process command
    let tokens = match tokenizer::tokenize(command) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tokenize error: {}", e);
            std::process::exit(1);
        }
    };

    let extracted = extract_command(&tokens);
    let flags_expanded = expand_flags(&extracted.command);
    let cwd_path = PathBuf::from(cwd);
    let paths = detect_paths(&extracted.command, &cwd_path);

    let policy_input = PolicyInput {
        tool: "Bash".to_string(),
        raw_command: command.to_string(),
        command: extracted.command.clone(),
        wrapper_chain: extracted.wrapper_chain.clone(),
        flags_expanded: flags_expanded.clone(),
        paths: paths.clone(),
        cwd: cwd.to_string(),
        project_root: cwd.to_string(),
        session_id: "eval".to_string(),
    };

    // Load and evaluate
    let mut engine = PolicyEngine::new();
    if let Err(e) = engine.load_policies_from_dir(&policy_dir) {
        eprintln!("Error loading policies: {}", e);
        std::process::exit(1);
    }

    let result = engine.evaluate(&policy_input);

    // Print results
    println!("Command:    {}", command);
    println!("Extracted:  {:?}", extracted.command);
    if !extracted.wrapper_chain.is_empty() {
        println!("Wrappers:   {:?}", extracted.wrapper_chain);
    }
    if !flags_expanded.is_empty() {
        println!("Flags:      {:?}", flags_expanded);
    }
    if !paths.is_empty() {
        println!("Paths:      {:?}", paths.iter().map(|p| &p.raw).collect::<Vec<_>>());
    }
    println!();
    println!("Decision:   {:?}", result.decision);
    if let Some(reason) = result.reason {
        println!("Reason:     {}", reason);
    }
}

fn run_hook() {
    let _guard = init_logging();
    let start = Instant::now();

    let result = run_hook_inner();

    let elapsed = start.elapsed();
    debug!(total_ms = elapsed.as_secs_f64() * 1000.0, "Completed");

    match result {
        Ok(output) => {
            println!("{}", output.to_json());
        }
        Err(e) => {
            error!("Error: {}", e);
            println!("{}", HookOutput::ask_with_reason(&e).to_json());
        }
    }
}

fn run_hook_inner() -> Result<HookOutput, String> {
    // Read input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    debug!(input = %input_str, "Received input");

    // Parse input JSON
    let hook_input =
        parse_input(&input_str).map_err(|e| format!("Failed to parse input: {}", e))?;

    // Only handle Bash tool
    if hook_input.tool_name != "Bash" {
        return Ok(HookOutput::ask_with_reason("Not a Bash command"));
    }

    let raw_command = &hook_input.tool_input.command;
    let cwd = hook_input.cwd.clone().unwrap_or_else(|| ".".to_string());
    let cwd_path = PathBuf::from(&cwd);
    let session_id = hook_input.session_id.clone().unwrap_or_default();

    // Tokenize command
    let tokens =
        tokenizer::tokenize(raw_command).map_err(|e| format!("Failed to tokenize: {}", e))?;

    if tokens.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty command"));
    }

    // Extract from wrappers
    let extracted = extract_command(&tokens);
    debug!(
        raw = ?tokens,
        extracted = ?extracted.command,
        wrappers = ?extracted.wrapper_chain,
        "Extracted command"
    );

    if extracted.command.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty extracted command"));
    }

    // Expand flags
    let flags_expanded = expand_flags(&extracted.command);
    debug!(flags = ?flags_expanded, "Expanded flags");

    // Detect paths
    let paths = detect_paths(&extracted.command, &cwd_path);
    debug!(paths = ?paths, "Detected paths");

    // Build policy input
    let policy_input = PolicyInput {
        tool: hook_input.tool_name,
        raw_command: raw_command.clone(),
        command: extracted.command,
        wrapper_chain: extracted.wrapper_chain,
        flags_expanded,
        paths,
        cwd: cwd.clone(),
        project_root: cwd,
        session_id,
    };

    // Load and evaluate policy
    let compile_start = Instant::now();
    let mut engine = PolicyEngine::new();

    let config_dir = get_policy_dir(None);

    if config_dir.exists() {
        engine.load_policies_from_dir(&config_dir)?;
    } else {
        info!("Config directory {:?} not found, using defaults", config_dir);
        return Ok(HookOutput::ask_with_reason("No policy configured"));
    }

    let compile_elapsed = compile_start.elapsed();
    debug!(
        compile_ms = compile_elapsed.as_secs_f64() * 1000.0,
        "Compiled policies"
    );

    // Evaluate
    let eval_start = Instant::now();
    let result = engine.evaluate(&policy_input);
    let eval_elapsed = eval_start.elapsed();

    info!(
        decision = ?result.decision,
        reason = ?result.reason,
        compile_ms = compile_elapsed.as_secs_f64() * 1000.0,
        eval_ms = eval_elapsed.as_secs_f64() * 1000.0,
        command = ?policy_input.command,
        "Policy evaluation complete"
    );

    Ok(HookOutput::new(result.decision, result.reason))
}
```

**Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 6: Build and test CLI**

Run: `cargo build --release`

Test help:
```bash
./target/release/cmdguard --help
./target/release/cmdguard test --help
./target/release/cmdguard eval --help
```

**Step 7: Commit**

```bash
git add Cargo.toml src/cli.rs src/test_runner.rs src/main.rs
git commit -m "feat: add CLI with test and eval subcommands"
```

---

### Task 15: Example Test File

**Files:**
- Create: `examples/policy_tests.yaml`

**Step 1: Create example test file**

Create `examples/policy_tests.yaml`:

```yaml
# Policy tests for cmdguard
# Run with: cmdguard test examples/policy_tests.yaml

tests:
  # ==========================================================================
  # Git Commands
  # ==========================================================================

  - name: "allow git status"
    command: "git status"
    expect: allow
    reason_contains: "Safe git"

  - name: "allow git diff"
    command: "git diff HEAD~1"
    expect: allow

  - name: "allow git log"
    command: "git log --oneline -10"
    expect: allow

  - name: "allow git add"
    command: "git add ."
    expect: allow

  - name: "allow git commit"
    command: "git commit -m 'test commit'"
    expect: allow

  - name: "deny git push --force"
    command: "git push --force origin main"
    expect: deny
    reason_contains: "Force push"

  - name: "deny git push -f"
    command: "git push -f origin main"
    expect: deny

  # ==========================================================================
  # Wrapper Extraction
  # ==========================================================================

  - name: "allow git through nix develop"
    command: "nix develop --command git status"
    expect: allow

  - name: "allow git through sudo"
    command: "sudo git status"
    expect: allow

  - name: "allow git through bash -c"
    command: "bash -c 'git status'"
    expect: allow

  - name: "deny force push through wrapper"
    command: "nix develop --command git push --force origin main"
    expect: deny

  # ==========================================================================
  # Cargo Commands
  # ==========================================================================

  - name: "allow cargo build"
    command: "cargo build"
    expect: allow

  - name: "allow cargo test"
    command: "cargo test"
    expect: allow

  - name: "allow cargo fmt"
    command: "cargo fmt"
    expect: allow

  # ==========================================================================
  # Package Manager Commands
  # ==========================================================================

  - name: "allow npm install"
    command: "npm install"
    expect: allow

  - name: "allow npm test"
    command: "npm test"
    expect: allow

  - name: "allow yarn build"
    command: "yarn build"
    expect: allow

  # ==========================================================================
  # Read-Only Commands
  # ==========================================================================

  - name: "allow ls"
    command: "ls -la"
    expect: allow

  - name: "allow cat"
    command: "cat README.md"
    expect: allow

  - name: "allow grep"
    command: "grep -r 'TODO' src/"
    expect: allow

  # ==========================================================================
  # Dangerous Commands
  # ==========================================================================

  - name: "deny rm -rf outside project"
    command: "rm -rf /etc"
    cwd: "/home/user/project"
    expect: deny
    reason_contains: "outside project"

  - name: "deny shutdown"
    command: "shutdown -h now"
    expect: deny
    reason_contains: "blocked"

  - name: "deny reboot"
    command: "reboot"
    expect: deny

  # ==========================================================================
  # Ask (Unknown Commands)
  # ==========================================================================

  - name: "ask for curl"
    command: "curl https://example.com"
    expect: ask

  - name: "ask for wget"
    command: "wget https://example.com/file.zip"
    expect: ask

  - name: "ask for unknown command"
    command: "some-random-command --with-args"
    expect: ask
```

**Step 2: Update install.sh to copy test file**

Add to `install.sh` before the final echo:

```bash
# Copy example test file
if [ ! -f ~/.config/cmdguard/policy_tests.yaml ]; then
    cp examples/policy_tests.yaml ~/.config/cmdguard/
    echo "Installed example policy_tests.yaml"
fi
```

**Step 3: Test the test runner**

Run:
```bash
./install.sh
cmdguard test --verbose
```

Expected: All tests pass with verbose output

**Step 4: Commit**

```bash
git add examples/policy_tests.yaml install.sh
git commit -m "feat: add example policy tests and update install script"
```

---

### Task 16: Final Integration Test

**Step 1: Build and install**

Run: `./install.sh`

**Step 2: Run policy tests**

```bash
cmdguard test --verbose
```

Expected: All tests pass

**Step 3: Test CLI eval command**

```bash
cmdguard eval "git status"
cmdguard eval "git push --force origin main"
cmdguard eval "nix develop --command git status"
```

**Step 4: Test as hook (stdin mode)**

```bash
# Test allowed command
echo '{"tool_name":"Bash","tool_input":{"command":"git status"},"cwd":"/tmp"}' | \
  ~/.local/bin/cmdguard

# Test denied command
echo '{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"},"cwd":"/tmp"}' | \
  ~/.local/bin/cmdguard
```

**Step 5: Verify outputs**

- `cmdguard test` → all pass
- `cmdguard eval "git status"` → Decision: Allow
- Hook mode git status → `"permissionDecision":"allow"`
- Hook mode force push → `"permissionDecision":"deny"`

**Step 6: Final commit**

```bash
git add -A
git commit -m "chore: final integration verification" --allow-empty
```