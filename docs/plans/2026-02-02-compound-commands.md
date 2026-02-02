# Compound Command Parsing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Parse compound bash commands (`&&`, `||`, `;`, `|`) and evaluate each command in the chain, short-circuiting on first non-allow decision.

**Architecture:** Add tree-sitter-bash for parsing shell syntax. Create a new `parser` module that extracts individual commands from compound statements. The main hook loop iterates over commands left-to-right, evaluating each against policies and returning immediately on deny/ask.

**Tech Stack:** tree-sitter (0.26), tree-sitter-bash (0.25), Rust

---

## Task 1: Add tree-sitter dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dependencies to Cargo.toml**

Add after the existing dependencies:

```toml
tree-sitter = "0.26"
tree-sitter-bash = "0.25"
```

**Step 2: Verify dependencies resolve**

Run: `cargo check`
Expected: Compiles successfully (may take a moment to download)

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add tree-sitter and tree-sitter-bash dependencies"
```

---

## Task 2: Create parser module with basic structure

**Files:**
- Create: `src/parser.rs`
- Modify: `src/main.rs` (add mod declaration)

**Step 1: Write the failing test**

Create `src/parser.rs`:

```rust
//! Parse shell commands using tree-sitter-bash
//!
//! Extracts individual commands from compound statements like:
//! - `cmd1 && cmd2` (AND list)
//! - `cmd1 || cmd2` (OR list)
//! - `cmd1 ; cmd2` (sequential)
//! - `cmd1 | cmd2` (pipeline)

use tree_sitter::{Parser, Tree};

/// A single command extracted from a compound statement
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// The command text
    pub text: String,
    /// Position in the chain (0-indexed)
    pub position: usize,
    /// Total number of commands in the chain
    pub chain_length: usize,
    /// Operator connecting to next command (if any)
    pub next_operator: Option<String>,
}

/// Result of parsing a command string
#[derive(Debug)]
pub struct ParseResult {
    /// Individual commands extracted
    pub commands: Vec<ParsedCommand>,
    /// Whether parsing encountered errors (unparseable constructs)
    pub has_errors: bool,
}

/// Parse a shell command string and extract individual commands
pub fn parse_command(input: &str) -> ParseResult {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let result = parse_command("git status");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].text, "git status");
        assert_eq!(result.commands[0].position, 0);
        assert_eq!(result.commands[0].chain_length, 1);
        assert!(result.commands[0].next_operator.is_none());
    }
}
```

**Step 2: Add module declaration to main.rs**

Add after `mod tokenizer;`:

```rust
mod parser;
```

**Step 3: Run test to verify it fails**

Run: `cargo test test_simple_command -- --nocapture`
Expected: FAIL with "not yet implemented"

**Step 4: Commit**

```bash
git add src/parser.rs src/main.rs
git commit -m "feat: add parser module skeleton with ParsedCommand struct"
```

---

## Task 3: Implement basic tree-sitter parsing

**Files:**
- Modify: `src/parser.rs`

**Step 1: Implement parse_command for simple commands**

Replace the `parse_command` function:

```rust
/// Parse a shell command string and extract individual commands
pub fn parse_command(input: &str) -> ParseResult {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .expect("Failed to load bash grammar");

    let tree = match parser.parse(input, None) {
        Some(t) => t,
        None => {
            return ParseResult {
                commands: vec![ParsedCommand {
                    text: input.to_string(),
                    position: 0,
                    chain_length: 1,
                    next_operator: None,
                }],
                has_errors: true,
            };
        }
    };

    let root = tree.root_node();
    let has_errors = root.has_error();

    // Extract commands from the parse tree
    let commands = extract_commands(&root, input);

    // If no commands found, treat whole input as single command
    if commands.is_empty() {
        return ParseResult {
            commands: vec![ParsedCommand {
                text: input.to_string(),
                position: 0,
                chain_length: 1,
                next_operator: None,
            }],
            has_errors,
        };
    }

    ParseResult {
        commands,
        has_errors,
    }
}

fn extract_commands(node: &tree_sitter::Node, source: &str) -> Vec<ParsedCommand> {
    let mut commands = Vec::new();
    extract_commands_recursive(node, source, &mut commands, &mut None);

    // Update chain_length for all commands
    let len = commands.len();
    for cmd in &mut commands {
        cmd.chain_length = len;
    }

    commands
}

fn extract_commands_recursive(
    node: &tree_sitter::Node,
    source: &str,
    commands: &mut Vec<ParsedCommand>,
    pending_operator: &mut Option<String>,
) {
    match node.kind() {
        // Simple command
        "command" | "simple_command" => {
            let text = node_text(node, source);
            if !text.trim().is_empty() {
                commands.push(ParsedCommand {
                    text: text.trim().to_string(),
                    position: commands.len(),
                    chain_length: 0, // Will be updated later
                    next_operator: None,
                });
            }
        }
        // AND list: cmd1 && cmd2
        "list" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "&&" || child.kind() == "||" || child.kind() == ";" {
                        // Set operator on previous command
                        if let Some(last) = commands.last_mut() {
                            last.next_operator = Some(child.kind().to_string());
                        }
                    } else {
                        extract_commands_recursive(&child, source, commands, pending_operator);
                    }
                }
            }
        }
        // Pipeline: cmd1 | cmd2
        "pipeline" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "|" || child.kind() == "|&" {
                        if let Some(last) = commands.last_mut() {
                            last.next_operator = Some(child.kind().to_string());
                        }
                    } else {
                        extract_commands_recursive(&child, source, commands, pending_operator);
                    }
                }
            }
        }
        // Recurse into other node types
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_commands_recursive(&child, source, commands, pending_operator);
                }
            }
        }
    }
}

fn node_text(node: &tree_sitter::Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test test_simple_command -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add src/parser.rs
git commit -m "feat: implement basic tree-sitter parsing for simple commands"
```

---

## Task 4: Add tests for compound commands

**Files:**
- Modify: `src/parser.rs` (tests module)

**Step 1: Add AND list test**

Add to tests module:

```rust
#[test]
fn test_and_list() {
    let result = parse_command("echo foo && git status");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 2);

    assert_eq!(result.commands[0].text, "echo foo");
    assert_eq!(result.commands[0].position, 0);
    assert_eq!(result.commands[0].chain_length, 2);
    assert_eq!(result.commands[0].next_operator, Some("&&".to_string()));

    assert_eq!(result.commands[1].text, "git status");
    assert_eq!(result.commands[1].position, 1);
    assert_eq!(result.commands[1].chain_length, 2);
    assert!(result.commands[1].next_operator.is_none());
}

#[test]
fn test_or_list() {
    let result = parse_command("false || echo fallback");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 2);
    assert_eq!(result.commands[0].next_operator, Some("||".to_string()));
}

#[test]
fn test_semicolon_list() {
    let result = parse_command("echo a ; echo b ; echo c");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 3);
    assert_eq!(result.commands[0].next_operator, Some(";".to_string()));
    assert_eq!(result.commands[1].next_operator, Some(";".to_string()));
    assert!(result.commands[2].next_operator.is_none());
}

#[test]
fn test_pipeline() {
    let result = parse_command("cat file.txt | grep pattern | head -10");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 3);
    assert_eq!(result.commands[0].next_operator, Some("|".to_string()));
    assert_eq!(result.commands[1].next_operator, Some("|".to_string()));
}

#[test]
fn test_mixed_operators() {
    let result = parse_command("echo start && cat file | grep foo || echo failed");
    assert!(!result.has_errors);
    assert!(result.commands.len() >= 3);
}

#[test]
fn test_quoted_strings_preserved() {
    let result = parse_command(r#"echo "hello && world""#);
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 1);
    // The && inside quotes should NOT split the command
}
```

**Step 2: Run tests**

Run: `cargo test parser::tests -- --nocapture`
Expected: Some tests may fail - that's OK, we'll fix them

**Step 3: Commit (even if some tests fail)**

```bash
git add src/parser.rs
git commit -m "test: add compound command parsing tests"
```

---

## Task 5: Fix parsing issues and edge cases

**Files:**
- Modify: `src/parser.rs`

**Step 1: Debug and fix any failing tests**

Run tests and examine tree-sitter output to understand the AST structure:

```rust
// Add this debug helper temporarily
#[cfg(test)]
fn print_tree(node: &tree_sitter::Node, source: &str, indent: usize) {
    println!(
        "{}{} [{}-{}] {:?}",
        "  ".repeat(indent),
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        &source[node.start_byte()..node.end_byte()]
    );
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            print_tree(&child, source, indent + 1);
        }
    }
}
```

Use this to understand how tree-sitter-bash structures different command types, then adjust `extract_commands_recursive` accordingly.

**Step 2: Run all tests**

Run: `cargo test parser -- --nocapture`
Expected: All tests pass

**Step 3: Commit**

```bash
git add src/parser.rs
git commit -m "fix: correct compound command parsing for all operator types"
```

---

## Task 6: Add error detection test

**Files:**
- Modify: `src/parser.rs` (tests module)

**Step 1: Add test for parse errors**

```rust
#[test]
fn test_parse_error_detected() {
    // Unclosed quote should be detected as error
    let result = parse_command(r#"echo "unclosed"#);
    assert!(result.has_errors);
    // Should still return the input as a single command
    assert_eq!(result.commands.len(), 1);
}

#[test]
fn test_subshell_treated_as_single() {
    // Subshells should be extracted but marked if we can't fully parse them
    let result = parse_command("(cd /tmp && ls)");
    // For now, we treat this as needing review
    assert!(result.commands.len() >= 1);
}
```

**Step 2: Run tests**

Run: `cargo test parser -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add src/parser.rs
git commit -m "test: add error detection and subshell tests"
```

---

## Task 7: Update PolicyInput with chain info

**Files:**
- Modify: `src/policy.rs`

**Step 1: Add chain fields to PolicyInput**

Find the `PolicyInput` struct and add:

```rust
#[derive(Debug, Serialize)]
pub struct PolicyInput {
    pub tool: String,
    pub raw_command: String,
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
    pub flags_expanded: Vec<String>,
    pub paths: Vec<PathInfo>,
    pub cwd: String,
    pub project_root: String,
    pub session_id: String,
    // New fields for compound commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_position: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_operator: Option<String>,
}
```

**Step 2: Update all PolicyInput construction sites**

Search for `PolicyInput {` and add the new fields with `None` values:

```rust
chain_position: None,
chain_length: None,
chain_operator: None,
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/policy.rs src/test_runner.rs
git commit -m "feat: add chain_position, chain_length, chain_operator to PolicyInput"
```

---

## Task 8: Create evaluate_compound function

**Files:**
- Modify: `src/main.rs`

**Step 1: Add function to evaluate compound commands**

Add this function before `run_hook_inner`:

```rust
use parser::{parse_command, ParsedCommand};

/// Evaluate a compound command, short-circuiting on first non-allow
fn evaluate_compound(
    parsed: &[ParsedCommand],
    has_parse_errors: bool,
    cwd: &str,
    cwd_path: &PathBuf,
    session_id: &str,
    project_root: &str,
    engine: &mut PolicyEngine,
) -> HookOutput {
    // If parsing had errors, be conservative and ask
    if has_parse_errors {
        return HookOutput::ask_with_reason("Command contains unparseable constructs");
    }

    for cmd in parsed {
        // Tokenize this individual command
        let tokens = match tokenizer::tokenize(&cmd.text) {
            Ok(t) if !t.is_empty() => t,
            _ => continue, // Skip empty/invalid
        };

        // Extract from wrappers
        let extracted = extract_command(&tokens);
        if extracted.command.is_empty() {
            continue;
        }

        // Expand flags
        let flags_expanded = expand_flags(&extracted.command);

        // Detect paths
        let paths = detect_paths(&extracted.command, cwd_path);

        // Build policy input with chain info
        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: cmd.text.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: cwd.to_string(),
            project_root: project_root.to_string(),
            session_id: session_id.to_string(),
            chain_position: Some(cmd.position),
            chain_length: Some(cmd.chain_length),
            chain_operator: cmd.next_operator.clone(),
        };

        // Evaluate
        let result = engine.evaluate(&policy_input);

        // Short-circuit on non-allow
        match result.decision {
            output::Decision::Allow => continue,
            output::Decision::Deny => {
                let reason = result.reason.unwrap_or_else(|| {
                    format!("Denied at command {} of {}", cmd.position + 1, cmd.chain_length)
                });
                return HookOutput::deny(&reason);
            }
            output::Decision::Ask => {
                let reason = result.reason.unwrap_or_else(|| {
                    format!("Review needed for command {} of {}", cmd.position + 1, cmd.chain_length)
                });
                return HookOutput::ask_with_reason(&reason);
            }
        }
    }

    // All commands allowed
    HookOutput::new(output::Decision::Allow, None)
}
```

**Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: add evaluate_compound function for chain evaluation"
```

---

## Task 9: Integrate parser into hook flow

**Files:**
- Modify: `src/main.rs`

**Step 1: Update run_hook_inner to use parser**

Replace the command processing section in `run_hook_inner` (after getting raw_command, before building PolicyInput):

```rust
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

    // Determine project root (for now, use cwd)
    let project_root = cwd.clone();

    // Parse command for compound operators
    let parse_result = parser::parse_command(raw_command);
    debug!(
        commands = ?parse_result.commands,
        has_errors = parse_result.has_errors,
        "Parsed command"
    );

    // Load policy engine
    let policy_dir = get_policy_dir(None);
    let mut engine = PolicyEngine::new();
    engine
        .load_policies_from_dir(&policy_dir)
        .map_err(|e| format!("Failed to load policies: {}", e))?;

    // Evaluate compound command
    Ok(evaluate_compound(
        &parse_result.commands,
        parse_result.has_errors,
        &cwd,
        &cwd_path,
        &session_id,
        &project_root,
        &mut engine,
    ))
}
```

**Step 2: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: integrate compound command parser into hook flow"
```

---

## Task 10: Update run_eval for compound commands

**Files:**
- Modify: `src/main.rs`

**Step 1: Update run_eval to show compound parsing**

Update `run_eval` to parse and display compound command info:

```rust
fn run_eval(command: &str, cwd: &str, policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);
    let cwd_path = PathBuf::from(cwd);

    // Parse for compound operators
    let parse_result = parser::parse_command(command);

    println!("=== Compound Command Analysis ===");
    println!("Commands:   {} in chain", parse_result.commands.len());
    println!("Has errors: {}", parse_result.has_errors);

    if parse_result.commands.len() > 1 {
        println!("\nChain breakdown:");
        for (i, cmd) in parse_result.commands.iter().enumerate() {
            let op = cmd.next_operator.as_deref().unwrap_or("(end)");
            println!("  [{}] {} {}", i + 1, cmd.text, op);
        }
    }
    println!();

    // Load engine
    let mut engine = match PolicyEngine::new().tap(|e| e.load_policies_from_dir(&policy_dir)) {
        e if e.load_policies_from_dir(&policy_dir).is_ok() => {
            let mut eng = PolicyEngine::new();
            if let Err(e) = eng.load_policies_from_dir(&policy_dir) {
                eprintln!("Error loading policies: {}", e);
                return;
            }
            eng
        }
        _ => return,
    };

    // Actually, let's simplify - just create and load
    let mut engine = PolicyEngine::new();
    if let Err(e) = engine.load_policies_from_dir(&policy_dir) {
        eprintln!("Error loading policies: {}", e);
        return;
    }

    // Evaluate each command in chain
    println!("=== Per-Command Evaluation ===");
    for cmd in &parse_result.commands {
        println!("\n--- Command {}/{}: {} ---", cmd.position + 1, cmd.chain_length, cmd.text);

        let tokens = match tokenizer::tokenize(&cmd.text) {
            Ok(t) => t,
            Err(e) => {
                println!("Tokenize error: {}", e);
                continue;
            }
        };

        if tokens.is_empty() {
            println!("(empty command)");
            continue;
        }

        let extracted = extract_command(&tokens);
        let flags_expanded = expand_flags(&extracted.command);
        let paths = detect_paths(&extracted.command, &cwd_path);

        println!("Command:    {:?}", extracted.command);
        if !extracted.wrapper_chain.is_empty() {
            println!("Wrappers:   {:?}", extracted.wrapper_chain);
        }
        if !flags_expanded.is_empty() {
            println!("Flags:      {:?}", flags_expanded);
        }
        if !paths.is_empty() {
            println!("Paths:      {:?}", paths.iter().map(|p| &p.raw).collect::<Vec<_>>());
        }

        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: cmd.text.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: cwd.to_string(),
            project_root: cwd.to_string(),
            session_id: "eval".to_string(),
            chain_position: Some(cmd.position),
            chain_length: Some(cmd.chain_length),
            chain_operator: cmd.next_operator.clone(),
        };

        let result = engine.evaluate(&policy_input);
        println!("Decision:   {:?}", result.decision);
        if let Some(reason) = result.reason {
            println!("Reason:     {}", reason);
        }
        if let Some(rule) = result.rule {
            println!("Rule:       {}", rule);
        }
        println!("Explicit:   {}", result.explicit);
    }

    // Show final result
    println!("\n=== Final Result ===");
    let final_output = evaluate_compound(
        &parse_result.commands,
        parse_result.has_errors,
        cwd,
        &cwd_path,
        "eval",
        cwd,
        &mut engine,
    );
    println!("{}", serde_json::to_string_pretty(&final_output).unwrap_or_default());
}
```

**Step 2: Test eval with compound command**

Run: `cargo run -- eval "echo foo && git status" --policy-dir ./config`
Expected: Shows both commands analyzed separately

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: update eval command to show compound command analysis"
```

---

## Task 11: Add integration tests

**Files:**
- Modify: `src/parser.rs` or create integration test

**Step 1: Add integration-style tests**

Add to parser tests:

```rust
#[test]
fn test_dangerous_compound_detected() {
    // This is the motivating example from the design doc
    let result = parse_command("echo foo && rm -rf /");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 2);
    assert_eq!(result.commands[0].text, "echo foo");
    assert_eq!(result.commands[1].text, "rm -rf /");
}

#[test]
fn test_redirect_preserved() {
    let result = parse_command("echo '.gitignore' >> .gitignore && git add .");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 2);
    // Redirect should be part of first command
    assert!(result.commands[0].text.contains(">>"));
}

#[test]
fn test_complex_real_world() {
    let result = parse_command("cd /tmp && git clone repo && cd repo && make");
    assert!(!result.has_errors);
    assert_eq!(result.commands.len(), 4);
}
```

**Step 2: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git add src/parser.rs
git commit -m "test: add integration tests for compound command parsing"
```

---

## Task 12: Update test_runner for compound commands

**Files:**
- Modify: `src/test_runner.rs`

**Step 1: Update TestRunner to use compound parsing**

The test runner should also evaluate compound commands properly. Update `run_single_test`:

```rust
fn run_single_test(&mut self, test: &TestCase) -> TestResult {
    use crate::parser::parse_command;

    // Parse for compound commands
    let parse_result = parse_command(&test.command);

    // If compound, evaluate each and short-circuit
    if parse_result.commands.len() > 1 || parse_result.has_errors {
        // For compound commands in tests, evaluate each
        for cmd in &parse_result.commands {
            let tokens = match tokenize(&cmd.text) {
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
                continue;
            }

            let extracted = extract_command(&tokens);
            let flags_expanded = expand_flags(&extracted.command);
            let cwd_path = PathBuf::from(&test.cwd);
            let paths = detect_paths(&extracted.command, &cwd_path);

            let policy_input = PolicyInput {
                tool: "Bash".to_string(),
                raw_command: cmd.text.clone(),
                command: extracted.command,
                wrapper_chain: extracted.wrapper_chain,
                flags_expanded,
                paths,
                cwd: test.cwd.clone(),
                project_root: test.cwd.clone(),
                session_id: "test".to_string(),
                chain_position: Some(cmd.position),
                chain_length: Some(cmd.chain_length),
                chain_operator: cmd.next_operator.clone(),
            };

            let result = self.engine.evaluate(&policy_input);

            // Short-circuit on non-allow
            if result.decision != Decision::Allow {
                let decision_matches = test.expect.matches(result.decision);
                let reason_matches = test.reason_contains.as_ref().map_or(true, |expected| {
                    result.reason.as_ref().map_or(false, |r| r.contains(expected))
                });

                return TestResult {
                    name: test.name.clone(),
                    passed: decision_matches && reason_matches,
                    expected: test.expect,
                    actual: result.decision,
                    reason: result.reason,
                    error: None,
                };
            }
        }

        // All allowed
        let decision_matches = test.expect.matches(Decision::Allow);
        return TestResult {
            name: test.name.clone(),
            passed: decision_matches,
            expected: test.expect,
            actual: Decision::Allow,
            reason: None,
            error: None,
        };
    }

    // Original single-command logic...
    // (keep existing code for simple commands)
}
```

**Step 2: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git add src/test_runner.rs
git commit -m "feat: update test_runner to handle compound commands"
```

---

## Task 13: Add YAML test cases for compound commands

**Files:**
- Modify: `config/policy_tests.yaml` (or wherever test file is)

**Step 1: Add compound command test cases**

Add to test file:

```yaml
# Compound command tests
- name: "and list both allowed"
  command: "git status && git diff"
  expect: allow

- name: "and list with dangerous second"
  command: "echo foo && rm -rf /"
  expect: ask  # or deny depending on your policies

- name: "or list fallback"
  command: "false || git status"
  expect: allow

- name: "pipeline allowed"
  command: "cat file.txt | grep pattern"
  expect: allow

- name: "semicolon chain"
  command: "echo a ; echo b"
  expect: allow
```

**Step 2: Run policy tests**

Run: `cargo run -- test --policy-dir ./config -v`
Expected: Tests pass according to policy rules

**Step 3: Commit**

```bash
git add config/policy_tests.yaml
git commit -m "test: add compound command test cases"
```

---

## Task 14: Final cleanup and version bump

**Files:**
- Modify: `Cargo.toml`

**Step 1: Bump version**

Change version to `0.3.0`:

```toml
version = "0.3.0"
```

**Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 3: Test manually**

Run: `cargo run -- eval "echo foo && rm -rf /" --policy-dir ./config`
Expected: Shows both commands evaluated, with rm -rf asking/denying

**Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore: bump version to 0.3.0 for compound command support"
```

---

## Summary

After completing all tasks:

1. **New parser module** using tree-sitter-bash for shell syntax parsing
2. **Compound command support** for `&&`, `||`, `;`, `|`
3. **Short-circuit evaluation** - stops on first deny/ask
4. **Parse error handling** - unparseable constructs default to ask
5. **Chain info in PolicyInput** - position, length, operator available to policies
6. **Updated eval command** - shows per-command analysis
7. **Updated test runner** - handles compound commands in tests

This enables policies to:
- See each command in a compound statement individually
- Know the command's position in the chain
- Know what operator connects to the next command
- Block dangerous commands even when hidden after safe ones
