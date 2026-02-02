# Rego Policy Refactoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor Rego policies to use a unified `rules` collection pattern that enables traceability (which rule matched) and supports the upcoming priority-based merge system.

**Architecture:** Change from separate `decision`/`reason` rules to a `rules["name"]` map pattern. Add aggregation logic in stdlib.rego that collects all matching rules and picks by priority. Update the Rust evaluator to parse the new `result` object format.

**Tech Stack:** Rust, Rego (regorus crate), existing test infrastructure

---

## Task 1: Update PolicyResult Struct

**Files:**
- Modify: `src/policy.rs:21-24`
- Modify: `src/output.rs` (no changes needed, Decision enum is fine)

**Step 1: Write the failing test**

Add to `src/policy.rs` in the `#[cfg(test)]` module:

```rust
#[test]
fn test_policy_result_has_rule_name() {
    let mut engine = PolicyEngine::new();
    let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
    engine.load_policies_from_dir(&policy_dir).unwrap();

    let input = make_input(vec!["git", "status"]);
    let result = engine.evaluate(&input);

    assert_eq!(result.decision, Decision::Allow);
    assert!(result.rule.is_some());
    assert_eq!(result.rule.unwrap(), "safe_git_read");
    assert!(result.explicit);
}

#[test]
fn test_policy_result_default_not_explicit() {
    let mut engine = PolicyEngine::new();
    let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
    engine.load_policies_from_dir(&policy_dir).unwrap();

    let input = make_input(vec!["curl", "https://example.com"]);
    let result = engine.evaluate(&input);

    assert_eq!(result.decision, Decision::Ask);
    assert!(!result.explicit);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_policy_result_has_rule_name -- --nocapture`
Expected: FAIL with "no field `rule` on type `PolicyResult`"

**Step 3: Update PolicyResult struct**

In `src/policy.rs`, change:

```rust
pub struct PolicyResult {
    pub decision: Decision,
    pub reason: Option<String>,
    pub rule: Option<String>,
    pub explicit: bool,
}
```

**Step 4: Update evaluate method signature**

The evaluate method now needs to return the new fields. Update the default return in error cases:

```rust
pub fn evaluate(&mut self, input: &PolicyInput) -> PolicyResult {
    // Set input data
    let input_json = match serde_json::to_value(input) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to serialize policy input: {}", e);
            return PolicyResult {
                decision: Decision::Ask,
                reason: Some("Internal error serializing input".to_string()),
                rule: None,
                explicit: false,
            };
        }
    };

    // Convert serde_json::Value to regorus::Value
    let input_value: regorus::Value = input_json.into();
    self.engine.set_input(input_value);

    // Evaluate the new result object
    self.eval_result()
}
```

**Step 5: Add eval_result method**

Replace `eval_decision` and `eval_reason` with a single `eval_result` method:

```rust
fn eval_result(&mut self) -> PolicyResult {
    match self.engine.eval_rule("data.claude.permissions.result".to_string()) {
        Ok(value) => {
            // Try to parse as object with decision, reason, rule, explicit
            let decision = value
                .as_object()
                .and_then(|obj| obj.get(&"decision".into()))
                .and_then(|v| v.as_string().ok())
                .map(|s| match s.as_ref() {
                    "allow" => Decision::Allow,
                    "deny" => Decision::Deny,
                    _ => Decision::Ask,
                })
                .unwrap_or(Decision::Ask);

            let reason = value
                .as_object()
                .and_then(|obj| obj.get(&"reason".into()))
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string());

            let rule = value
                .as_object()
                .and_then(|obj| obj.get(&"rule".into()))
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string());

            let explicit = value
                .as_object()
                .and_then(|obj| obj.get(&"explicit".into()))
                .and_then(|v| v.as_bool().ok())
                .unwrap_or(false);

            PolicyResult {
                decision,
                reason,
                rule,
                explicit,
            }
        }
        Err(e) => {
            warn!("Failed to evaluate result: {}", e);
            PolicyResult {
                decision: Decision::Ask,
                reason: Some(format!("Policy evaluation error: {}", e)),
                rule: None,
                explicit: false,
            }
        }
    }
}
```

**Step 6: Remove old eval_decision and eval_reason methods**

Delete the `eval_decision` and `eval_reason` methods (lines 97-122).

**Step 7: Run test to verify it still fails**

Run: `cargo test test_policy_result_has_rule_name -- --nocapture`
Expected: FAIL - test compiles but policy doesn't return new format yet

**Step 8: Commit**

```bash
git add src/policy.rs
git commit -m "refactor: update PolicyResult to include rule name and explicit flag"
```

---

## Task 2: Update Test Policy with New Format

**Files:**
- Modify: `policies/test_policy.rego`

**Step 1: Update test_policy.rego**

Replace the contents of `policies/test_policy.rego`:

```rego
package claude.permissions

import future.keywords.in
import future.keywords.if

# Rules collection - each rule adds to this map
rules["safe_git_read"] := {
    "decision": "allow",
    "reason": "Safe git read operation",
    "priority": 25,
} if {
    input.command[0] == "git"
    input.command[1] in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

rules["force_push_blocked"] := {
    "decision": "deny",
    "reason": "Force push blocked by policy",
    "priority": 100,
} if {
    input.command[0] == "git"
    input.command[1] == "push"
    "--force" in input.command
}

# Aggregation: collect all matching rules, pick highest priority
default result := {
    "decision": "ask",
    "reason": "No rule matched",
    "rule": "default",
    "explicit": false,
}

# Helper to find the highest priority rule
highest_priority_rule := rule if {
    some name
    rule := rules[name]
    not _higher_priority_exists(rule.priority)
}

_higher_priority_exists(p) if {
    some name
    rules[name].priority > p
}

# Result from highest priority matching rule
result := {
    "decision": highest_priority_rule.decision,
    "reason": highest_priority_rule.reason,
    "rule": _winning_rule_name,
    "explicit": true,
} if {
    count(rules) > 0
}

# Get the name of the winning rule
_winning_rule_name := name if {
    some name
    rules[name] == highest_priority_rule
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test test_policy_result_has_rule_name -- --nocapture`
Expected: PASS

**Step 3: Run all policy tests**

Run: `cargo test -- --nocapture`
Expected: All tests pass

**Step 4: Commit**

```bash
git add policies/test_policy.rego
git commit -m "refactor: update test_policy.rego to use rules collection pattern"
```

---

## Task 3: Update stdlib.rego with Aggregation Helpers

**Files:**
- Modify: `config/stdlib.rego`

**Step 1: Read current stdlib.rego**

Current helpers: `flag_value`, `git_subcommand`, `path_outside_project`, `all_paths_in_project`, `no_paths`

**Step 2: Add aggregation logic to stdlib.rego**

Append to `config/stdlib.rego`:

```rego
# Priority-based rule aggregation
# Each policy file adds rules to the `rules` map
# This picks the highest priority matching rule

default result := {
    "decision": "ask",
    "reason": "No rule matched",
    "rule": "default",
    "explicit": false,
}

# Find the rule with highest priority
highest_priority_rule := rule if {
    some name
    rule := rules[name]
    not _higher_priority_exists(rule.priority)
}

_higher_priority_exists(p) if {
    some name
    rules[name].priority > p
}

# Get the name of the winning rule
_winning_rule_name := name if {
    some name
    rules[name] == highest_priority_rule
}

# Result from highest priority matching rule
result := {
    "decision": highest_priority_rule.decision,
    "reason": highest_priority_rule.reason,
    "rule": _winning_rule_name,
    "explicit": true,
} if {
    count(rules) > 0
}
```

**Step 3: Commit**

```bash
git add config/stdlib.rego
git commit -m "feat: add priority-based rule aggregation to stdlib.rego"
```

---

## Task 4: Refactor git.rego

**Files:**
- Modify: `config/git.rego`

**Step 1: Update git.rego to new pattern**

Replace contents of `config/git.rego`:

```rego
package claude.permissions

import rego.v1

rules["safe_git"] := {
    "decision": "allow",
    "reason": "Safe git command",
    "priority": 25,
} if {
    input.command[0] == "git"
    input.command[1] in {"status", "log", "ls-tree", "show", "version", "diff"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "git status" --policy-dir ./config`
Expected: Decision: Allow, Rule: safe_git

**Step 3: Commit**

```bash
git add config/git.rego
git commit -m "refactor: update git.rego to rules collection pattern"
```

---

## Task 5: Refactor safe.rego

**Files:**
- Modify: `config/safe.rego`

**Step 1: Update safe.rego to new pattern**

Replace contents of `config/safe.rego`:

```rego
package claude.permissions

import rego.v1

rules["safe_command"] := {
    "decision": "allow",
    "reason": "Safe command",
    "priority": 25,
} if {
    input.command[0] in {"ls", "echo", "cat", "grep", "head", "tail", "rg", "mkdir", "touch", "file"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "ls -la" --policy-dir ./config`
Expected: Decision: Allow, Rule: safe_command

**Step 3: Commit**

```bash
git add config/safe.rego
git commit -m "refactor: update safe.rego to rules collection pattern"
```

---

## Task 6: Refactor rust.rego

**Files:**
- Modify: `config/rust.rego`

**Step 1: Update rust.rego to new pattern**

Replace contents of `config/rust.rego`:

```rego
package claude.permissions

import rego.v1

rules["allowed_cargo"] := {
    "decision": "allow",
    "reason": "Allowed cargo command",
    "priority": 25,
} if {
    input.command[0] == "cargo"
    input.command[1] in {"run", "test", "build", "doc", "version", "fmt", "search"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "cargo test" --policy-dir ./config`
Expected: Decision: Allow, Rule: allowed_cargo

**Step 3: Commit**

```bash
git add config/rust.rego
git commit -m "refactor: update rust.rego to rules collection pattern"
```

---

## Task 7: Refactor python.rego

**Files:**
- Modify: `config/python.rego`

**Step 1: Update python.rego to new pattern**

Replace contents of `config/python.rego`:

```rego
package claude.permissions

import rego.v1

is_python if input.command[0] == "python"

python_module := module if {
    is_python
    module := flag_value("-m")
}

is_python_module(name) if python_module == name

is_pytest if input.command[0] == "pytest"

is_pytest if {
    is_python_module("pytest")
}

is_tests_main if {
    is_python
    input.command[1] == "tests/main.py"
}

is_json_tool if is_python_module("json.tool")

rules["pytest"] := {
    "decision": "allow",
    "reason": "Pytest allowed",
    "priority": 25,
} if is_pytest

rules["tests_main"] := {
    "decision": "allow",
    "reason": "tests/main.py allowed",
    "priority": 25,
} if is_tests_main

rules["json_tool"] := {
    "decision": "allow",
    "reason": "json.tool allowed",
    "priority": 25,
} if is_json_tool

rules["safe_python_tools"] := {
    "decision": "allow",
    "reason": "Safe Python tool allowed",
    "priority": 25,
} if {
    input.command[0] in {"mypy", "pylint", "black", "isort", "pytest"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "pytest tests/" --policy-dir ./config`
Expected: Decision: Allow

**Step 3: Commit**

```bash
git add config/python.rego
git commit -m "refactor: update python.rego to rules collection pattern"
```

---

## Task 8: Refactor nix.rego

**Files:**
- Modify: `config/nix.rego`

**Step 1: Update nix.rego to new pattern**

Replace contents of `config/nix.rego`:

```rego
package claude.permissions

import rego.v1

is_nix if input.command[0] == "nix"

is_nix_flake if {
    is_nix
    input.command[1] == "flake"
}

rules["allowed_nix"] := {
    "decision": "allow",
    "reason": "Allowed nix command",
    "priority": 25,
} if {
    is_nix
    input.command[1] in {"build", "version"}
}

rules["allowed_flake"] := {
    "decision": "allow",
    "reason": "Allowed flake command",
    "priority": 25,
} if {
    is_nix_flake
    input.command[2] in {"check", "info", "show", "update"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "nix build" --policy-dir ./config`
Expected: Decision: Allow, Rule: allowed_nix

**Step 3: Commit**

```bash
git add config/nix.rego
git commit -m "refactor: update nix.rego to rules collection pattern"
```

---

## Task 9: Refactor opa.rego

**Files:**
- Modify: `config/opa.rego`

**Step 1: Update opa.rego to new pattern**

Replace contents of `config/opa.rego`:

```rego
package claude.permissions

import rego.v1

rules["safe_opa"] := {
    "decision": "allow",
    "reason": "Allowed opa command",
    "priority": 25,
} if {
    input.command[0] == "opa"
    input.command[1] in {"help", "version", "exec", "fmt", "parse", "eval", "test"}
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "opa eval" --policy-dir ./config`
Expected: Decision: Allow, Rule: safe_opa

**Step 3: Commit**

```bash
git add config/opa.rego
git commit -m "refactor: update opa.rego to rules collection pattern"
```

---

## Task 10: Refactor rego.rego

**Files:**
- Modify: `config/rego.rego`

**Step 1: Update rego.rego to new pattern**

Replace contents of `config/rego.rego`:

```rego
package claude.permissions

import rego.v1

rules["regal"] := {
    "decision": "allow",
    "reason": "Regal commands allowed",
    "priority": 25,
} if {
    input.command[0] == "regal"
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "regal lint ." --policy-dir ./config`
Expected: Decision: Allow, Rule: regal

**Step 3: Commit**

```bash
git add config/rego.rego
git commit -m "refactor: update rego.rego to rules collection pattern"
```

---

## Task 11: Refactor find.rego

**Files:**
- Modify: `config/find.rego`

**Step 1: Update find.rego to new pattern**

Replace contents of `config/find.rego`:

```rego
package claude.permissions

import rego.v1

is_find if input.command[0] == "find"

find_with_exec if {
    is_find
    "-exec" in input.command
}

rules["safe_find"] := {
    "decision": "allow",
    "reason": "Allowed find command",
    "priority": 25,
} if {
    is_find
    not find_with_exec
}

rules["find_with_exec"] := {
    "decision": "ask",
    "reason": "Find command with -exec requires approval",
    "priority": 50,
} if find_with_exec
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "find . -name '*.rs'" --policy-dir ./config`
Expected: Decision: Allow, Rule: safe_find

Run: `cargo run -- eval "find . -exec rm {} \\;" --policy-dir ./config`
Expected: Decision: Ask, Rule: find_with_exec

**Step 3: Commit**

```bash
git add config/find.rego
git commit -m "refactor: update find.rego to rules collection pattern"
```

---

## Task 12: Refactor tools.rego

**Files:**
- Modify: `config/tools.rego`

**Step 1: Update tools.rego to new pattern**

Replace contents of `config/tools.rego`:

```rego
package claude.permissions

import rego.v1

in_bin_path(name) if {
    some path in ["", "./target/release/", "./target/debug/"]
    input.command[0] == sprintf("%s%s", [path, name])
}

rules["local_claude_permissions"] := {
    "decision": "allow",
    "reason": "Allowed local tool",
    "priority": 25,
} if in_bin_path("claude-permissions")
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "./target/debug/claude-permissions eval 'ls'" --policy-dir ./config`
Expected: Decision: Allow, Rule: local_claude_permissions

**Step 3: Commit**

```bash
git add config/tools.rego
git commit -m "refactor: update tools.rego to rules collection pattern"
```

---

## Task 13: Refactor javascript.rego

**Files:**
- Modify: `config/javascript.rego`

**Step 1: Update javascript.rego to new pattern**

Replace contents of `config/javascript.rego`:

```rego
package claude.permissions

import rego.v1

is_npm if input.command[0] == "npm"
is_yarn if input.command[0] == "yarn"
safe_npm_commands := {"build", "test"}

rules["safe_npm"] := {
    "decision": "allow",
    "reason": "Safe npm command",
    "priority": 25,
} if {
    is_npm
    input.command[1] in safe_npm_commands
}

rules["safe_yarn"] := {
    "decision": "allow",
    "reason": "Safe yarn command",
    "priority": 25,
} if {
    is_yarn
    input.command[1] in safe_npm_commands
}
```

**Step 2: Test with eval command**

Run: `cargo run -- eval "npm test" --policy-dir ./config`
Expected: Decision: Allow, Rule: safe_npm

**Step 3: Commit**

```bash
git add config/javascript.rego
git commit -m "refactor: update javascript.rego to rules collection pattern"
```

---

## Task 14: Update Existing Tests

**Files:**
- Modify: `src/policy.rs` (test module)

**Step 1: Update test assertions**

Update the existing tests in `src/policy.rs` to check for the new fields:

```rust
#[test]
fn test_load_and_evaluate_policy() {
    let mut engine = PolicyEngine::new();
    let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
    engine.load_policies_from_dir(&policy_dir).unwrap();

    let input = make_input(vec!["git", "status"]);
    let result = engine.evaluate(&input);
    assert_eq!(result.decision, Decision::Allow);
    assert_eq!(result.reason, Some("Safe git read operation".to_string()));
    assert_eq!(result.rule, Some("safe_git_read".to_string()));
    assert!(result.explicit);
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
    assert_eq!(result.rule, Some("force_push_blocked".to_string()));
    assert!(result.explicit);
}

#[test]
fn test_ask_for_unknown() {
    let mut engine = PolicyEngine::new();
    let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
    engine.load_policies_from_dir(&policy_dir).unwrap();

    let input = make_input(vec!["curl", "https://example.com"]);
    let result = engine.evaluate(&input);
    assert_eq!(result.decision, Decision::Ask);
    assert_eq!(result.rule, Some("default".to_string()));
    assert!(!result.explicit);
}
```

**Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git add src/policy.rs
git commit -m "test: update policy tests for new result format"
```

---

## Task 15: Update Eval Output to Show Rule Name

**Files:**
- Modify: `src/main.rs:141-156`

**Step 1: Update run_eval to print rule name**

In the `run_eval` function, update the output section:

```rust
println!("Decision:   {:?}", result.decision);
if let Some(reason) = result.reason {
    println!("Reason:     {}", reason);
}
if let Some(rule) = result.rule {
    println!("Rule:       {}", rule);
}
println!("Explicit:   {}", result.explicit);
```

**Step 2: Test eval command**

Run: `cargo run -- eval "git status" --policy-dir ./config`
Expected: Output includes "Rule: safe_git" and "Explicit: true"

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: show rule name and explicit flag in eval output"
```

---

## Task 16: Final Integration Test

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 2: Test with config policies**

Run: `cargo run -- eval "git status" --policy-dir ./config`
Run: `cargo run -- eval "cargo test" --policy-dir ./config`
Run: `cargo run -- eval "unknown-command" --policy-dir ./config`

**Step 3: Create final commit if any cleanup needed**

```bash
git status
# If any uncommitted changes, commit them
```

---

## Summary

After completing all tasks:

1. `PolicyResult` includes `rule: Option<String>` and `explicit: bool`
2. All policy files use `rules["name"] := { ... }` pattern
3. `stdlib.rego` contains priority-based aggregation logic
4. `eval` command shows which rule matched
5. All tests pass with new assertions

This foundation enables:
- Traceability: know exactly which rule produced a decision
- Priority system: ready for project-local rules with priority merge
- Dual engine comparison: can compare explicit vs default decisions
