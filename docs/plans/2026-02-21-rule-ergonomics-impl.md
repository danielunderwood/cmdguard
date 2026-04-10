# Rule Ergonomics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add stdlib decision helpers (`allow`/`deny`/`ask`) and a declarative `allowed_subcommands` table to reduce Rego rule boilerplate.

**Architecture:** Extend `config/stdlib.rego` with helper functions and a rule-generation pattern. Migrate all existing policy files to use the new helpers. No Rust code changes needed — this is purely Rego.

**Tech Stack:** Rego (OPA policy language), evaluated by regorus engine

---

### Task 1: Add decision helpers to stdlib.rego

**Files:**
- Modify: `config/stdlib.rego`

**Step 1: Add helper functions to stdlib.rego**

Add these functions after the existing helper functions (after line 31, before the priority-based aggregation section):

```rego
# Decision helpers - reduces per-rule boilerplate
allow(reason) := {"decision": "allow", "reason": reason, "priority": 25}
deny(reason) := {"decision": "deny", "reason": reason, "priority": 100}
ask(reason) := {"decision": "ask", "reason": reason, "priority": 50}

# Custom-priority variants
allow_at(reason, priority) := {"decision": "allow", "reason": reason, "priority": priority}
deny_at(reason, priority) := {"decision": "deny", "reason": reason, "priority": priority}
ask_at(reason, priority) := {"decision": "ask", "reason": reason, "priority": priority}
```

**Step 2: Run existing tests to verify nothing breaks**

Run: `cargo run -- test`
Expected: All existing tests pass (helpers are additive, no existing behavior changes)

**Step 3: Commit**

```bash
git add config/stdlib.rego
git commit -m "feat: add allow/deny/ask decision helpers to stdlib.rego"
```

---

### Task 2: Add declarative subcommand table to stdlib.rego

**Files:**
- Modify: `config/stdlib.rego`

**Step 1: Add subcommand table rule generator**

Add after the decision helpers, before the priority aggregation:

```rego
# Declarative subcommand allowlist
# Policy files contribute entries via partial object rules:
#   allowed_subcommands["git"] := {"status", "log", "diff"}
#   allowed_subcommands["cargo"] := {"build", "test", "check"}
default allowed_subcommands := {}

rules[rule_name] := allow(reason) if {
	some binary, subcmds in allowed_subcommands
	input.binary_name == binary
	input.subcommand in subcmds
	rule_name := sprintf("allowed_%s_%s", [binary, input.subcommand])
	reason := sprintf("Allowed %s subcommand", [binary])
}
```

**Step 2: Run existing tests**

Run: `cargo run -- test`
Expected: All existing tests still pass (no policy files define `allowed_subcommands` yet, so the default `{}` kicks in and generates no rules)

**Step 3: Commit**

```bash
git add config/stdlib.rego
git commit -m "feat: add declarative allowed_subcommands table to stdlib.rego"
```

---

### Task 3: Migrate rust.rego

**Files:**
- Modify: `config/rust.rego`

**Step 1: Rewrite rust.rego**

Replace the entire file with:

```rego
package cmdguard

import rego.v1

allowed_subcommands["cargo"] := {
	"bench",
	"build",
	"check",
	"clean",
	"clippy",
	"config",
	"doc",
	"fix",
	"fmt",
	"generate-lockfile",
	"help",
	"info",
	"list",
	"locate-project",
	"metadata",
	"pkgid",
	"run",
	"report",
	"rustc",
	"rustdoc",
	"search",
	"test",
	"version",
}

rules["allowed_rustfmt"] := allow("Allowed rust tool") if {
	input.binary_name == "rustfmt"
}

rules["allowed_rustc"] := allow("Allowed rustc command") if {
	input.binary_name == "rustc"
}
```

This removes the local `allowed()` helper (now in stdlib) and moves the cargo subcommand list into `allowed_subcommands`.

**Step 2: Run tests**

Run: `cargo run -- test`
Expected: All existing cargo/rust tests still pass. The "allow cargo build", "allow cargo test", "allow cargo fmt" tests and compound chain tests all produce the same `allow` decision.

**Step 3: Commit**

```bash
git add config/rust.rego
git commit -m "refactor: migrate rust.rego to stdlib helpers and allowed_subcommands"
```

---

### Task 4: Migrate git.rego

**Files:**
- Modify: `config/git.rego`

**Step 1: Rewrite git.rego**

Replace the entire file with:

```rego
package cmdguard

import rego.v1

allowed_subcommands["git"] := {
	"check-ignore",
	"diff",
	"fetch",
	"log",
	"ls-tree",
	"rev-parse",
	"show",
	"status",
	"version",
}

rules["allow_git_help"] := allow("Allowed git town") if {
	input.binary_name == "git"
	input.positional.args[0].raw == "town"
	input.positional.args[1].raw == "help"
}

# Deny git push --force using parsed_flags
rules["force_push_structured"] := deny("Force push blocked (detected via parsed_flags)") if {
	input.subcommand == "push"
	input.parsed_flags.force == true
}

# Deny git reset --hard using parsed_flags
rules["hard_reset"] := deny("Hard reset blocked - use --soft or --mixed instead") if {
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}

# Deny git clean -x (removes ignored files)
rules["clean_ignored"] := ask("git clean -x removes ignored files - please confirm") if {
	input.subcommand == "clean"
	input.parsed_flags.ignored == true
}

rules["git_worktree"] := allow("git worktree") if {
	input.binary_name == "git"
	input.subcommand == "worktree"
	input.positional.subcommand[0].raw == "list"
}

is_git_branch if {
	input.binary_name == "git"
	input.subcommand == "branch"
}

git_branch_allowed_flag if "--show-current" in input.flags_expanded
git_branch_allowed_flag if "-a" in input.flags_expanded

# Just "git branch" or "git branch --list"
git_branch_allowed_flag if {
	is_git_branch
	input.flags_expanded in {[], ["--list"]}
	input.positional.args == []
}

rules["git_branch_readonly"] := allow("git branch readonly") if {
	is_git_branch
	git_branch_allowed_flag
}
```

Key changes:
- `safe_git_commands` set + `safe_git_parsed_args` rule replaced by `allowed_subcommands["git"]`
- Inline `{ "decision": ... }` maps replaced with `allow()`/`deny()`/`ask()` helpers
- Removed old commented-out rule block

**Step 2: Run tests**

Run: `cargo run -- test`
Expected: All git-related tests pass with same decisions.

**Step 3: Commit**

```bash
git add config/git.rego
git commit -m "refactor: migrate git.rego to stdlib helpers and allowed_subcommands"
```

---

### Task 5: Migrate remaining config policy files

**Files:**
- Modify: `config/safe.rego`
- Modify: `config/python.rego`
- Modify: `config/gh.rego`
- Modify: `config/javascript.rego`
- Modify: `config/nix.rego`
- Modify: `config/mise.rego`
- Modify: `config/opa.rego`
- Modify: `config/find.rego`
- Modify: `config/inproject.rego`
- Modify: `config/nickel.rego`
- Modify: `config/kubectl.rego`
- Modify: `config/rego.rego`
- Modify: `config/psql.rego`

**Step 1: Migrate each file**

For each file, replace inline `{ "decision": ..., "reason": ..., "priority": ... }` maps with the corresponding `allow()`/`deny()`/`ask()` helper call.

`config/safe.rego`:
```rego
package cmdguard

import rego.v1

rules["safe_command"] := allow("Safe command") if {
	input.command[0] in {
		"./scripts/run_postgres_tests.sh",
		"base64",
		"basename",
		"cat",
		"cd",
		"cut",
		"date",
		"df",
		"dig",
		"du",
		"echo",
		"env",
		"file",
		"getent",
		"grep",
		"head",
		"host",
		"jq",
		"ls",
		"mkdir",
		"nslookup",
		"paste",
		"ping",
		"printenv",
		"pwd",
		"read",
		"rg",
		"sed",
		"sleep",
		"sort",
		"sqq",
		"tail",
		"touch",
		"tr",
		"uniq",
		"wc",
		"which",
		"xxd",
	}
}

rules["unknown_binary_caution"] := ask("Binary from unknown location - please verify") if {
	input.resolved_trust_zone == "unknown"
	input.binary_name in {"rm", "chmod", "chown", "mv", "cp"}
}
```

`config/python.rego`:
```rego
package cmdguard

import rego.v1

is_python if input.binary_name == "python"

python_module := module if {
	is_python
	module := input.parsed_flags.module
}

is_python_module(name) if python_module == name

is_pytest if input.binary_name == "pytest"

is_pytest if {
	is_python_module("pytest")
}

is_tests_main if {
	is_python
	input.positional.file[0].raw == "tests/main.py"
}

is_json_tool if is_python_module("json.tool")

rules["pytest"] := allow("Pytest allowed") if {
	is_pytest
}

rules["tests_main"] := allow("tests/main.py allowed") if {
	is_tests_main
}

rules["json_tool"] := allow("json.tool allowed") if {
	is_json_tool
}

rules["safe_python_tools"] := allow("Safe Python tool allowed") if {
	input.command[0] in {"alembic", "mypy", "pylint", "black", "isort", "ruff"}
}

# ============================================================================
# Python inline code analysis (python -c)
# ============================================================================

# Helper to check if any pattern matches a capture name
has_pattern(capture_name) if {
	input.python_analysis.patterns[_].capture == capture_name
}

# Allow safe inspection code (no dangerous patterns)
rules["python_safe_inspection"] := allow_at("Python code is safe for inspection", 30) if {
	is_python
	input.python_analysis.is_inspection_safe
}

# Deny dynamic execution (eval, exec, compile)
rules["python_deny_dynamic_exec"] := deny_at("Python code contains dynamic execution (eval/exec)", 40) if {
	is_python
	has_pattern("dynamic_exec")
}

# Deny subprocess operations
rules["python_deny_subprocess"] := deny_at("Python code contains subprocess operations", 40) if {
	is_python
	has_pattern("subprocess_op")
}
```

`config/gh.rego`:
```rego
package cmdguard

import rego.v1

rules["gh_cli"] := allow("Allowed gh command") if {
	input.command[0] == "gh"
	input.command[1] == "pr"
	input.command[2] in {"checks", "diff", "list", "view"}
}

rules["gh_cli_run"] := allow("Allowed gh command") if {
	input.command[0] == "gh"
	input.command[1] == "run"
	input.command[2] in {"list", "view"}
}

is_gh_cli if input.binary_name == "gh"

is_gh_api if {
	is_gh_cli
	input.positional_args[0].values[0].raw == "api"
}

is_gh_api_pr_files if {
	is_gh_api
	regex.match(`pulls/\d+/files$`, input.positional_args[0].values[1].raw)
}

is_gh_api_action_run if {
	is_gh_api
	regex.match(`actions/jobs/\d+/logs$`, input.positional_args[0].values[1].raw)
}

rules["gh_cli_new"] := allow("Allowed gh command") if is_gh_api_pr_files
rules["gh_cli_actions_runs"] := allow("Allowed gh command") if is_gh_api_action_run
```

`config/javascript.rego`:
```rego
package cmdguard

import rego.v1

is_npm if input.command[0] == "npm"
is_yarn if input.command[0] == "yarn"
safe_npm_commands := {"build", "test"}

rules["safe_npm"] := allow("Safe npm command") if {
	is_npm
	input.command[1] in safe_npm_commands
}

rules["safe_yarn"] := allow("Safe yarn command") if {
	is_yarn
	input.command[1] in safe_npm_commands
}
```

`config/nix.rego`:
```rego
package cmdguard

import rego.v1

is_nix if input.command[0] == "nix"

is_nix_flake if {
	is_nix
	input.command[1] == "flake"
}

rules["allowed_nix"] := allow("Allowed nix command") if {
	is_nix
	input.command[1] in {"build", "version"}
}

rules["allowed_flake"] := allow("Allowed flake command") if {
	is_nix_flake
	input.command[2] in {"check", "info", "show", "update"}
}

rules["allowed_nh"] := allow("Allowed nh command") if {
	input.command[0] == "nh"
	input.command[1] == "search"
}
```

`config/mise.rego`:
```rego
package cmdguard

import rego.v1

allowed_mise_commands := {
	"build",
	"check",
	"t",
	"tasks",
	"test",
	"version",
}

rules["allowed_mise"] := allow("Allowed mise command") if {
	input.command[0] == "mise"
	input.command[1] in allowed_mise_commands
}

# TODO: This will be better with improved flag handling
rules["allowed_mise_env"] := allow("Allowed mise command with env") if {
	regex.match(`[A-Za-z][A-Za-z_]*=.*`, input.command[0])
	input.command[1] == "mise"
	input.command[2] in allowed_mise_commands
}
```

`config/opa.rego`:
```rego
package cmdguard

import rego.v1

rules["safe_opa"] := allow("Allowed opa command") if {
	input.command[0] == "opa"
	input.command[1] in {
		"eval",
		"exec",
		"fmt",
		"help",
		"parse",
		"test",
		"version",
	}
}
```

`config/find.rego`:
```rego
package cmdguard

import rego.v1

is_find if input.command[0] == "find"

find_with_exec if {
	is_find
	"-exec" in input.command
}

rules["safe_find"] := allow("Allowed find command") if {
	is_find
	not find_with_exec
}

rules["find_with_exec"] := ask("Find command with -exec requires approval") if find_with_exec
```

`config/inproject.rego`:
```rego
package cmdguard

import rego.v1

rules["project_associated_binary"] := allow("Binary in project") if {
	regex.match(`target/(debug|release)/[^/]+$`, input.resolved_path)
	input.resolved_trust_zone == "project"
}
```

`config/nickel.rego`:
```rego
package cmdguard

import rego.v1

rules["nickel"] := allow("Allowed nickel command") if {
	input.binary_name == "nickel"
	input.positional.args[0].raw == "eval"
}
```

`config/kubectl.rego`:
```rego
package cmdguard

import rego.v1

rules["kubectl_readonly"] := allow("kubectl readonly") if {
	input.binary_name == "kubectl"
	input.positional.args[0].raw in {"get", "describe", "logs", "rollout"}
	not startswith(input.positional.args[1].raw, "secret")
}

rules["helm_readonly"] := allow("helm readonly") if {
	input.binary_name == "helm"
	input.positional.args[0].raw in {"get", "history", "list", "show", "view"}
}

rules["flux"] := allow("flux") if {
	input.binary_name == "flux"
	input.positional.args[0].raw in {"get", "reconcile"}
}
```

`config/rego.rego`:
```rego
package cmdguard

import rego.v1

rules["regal"] := allow("Regal commands allowed") if {
	input.command[0] == "regal"
}
```

`config/psql.rego`:
```rego
package cmdguard

import rego.v1

rules["allowed_psql"] := allow("Allowed psql command") if {
	input.command[0] == "psql"
	input.command[1] == "-h"
	input.command[2] == "localhost"
}
```

**Step 2: Run tests**

Run: `cargo run -- test`
Expected: All existing tests pass with identical decisions.

**Step 3: Commit**

```bash
git add config/safe.rego config/python.rego config/gh.rego config/javascript.rego config/nix.rego config/mise.rego config/opa.rego config/find.rego config/inproject.rego config/nickel.rego config/kubectl.rego config/rego.rego config/psql.rego
git commit -m "refactor: migrate all config policy files to stdlib helpers"
```

---

### Task 6: Update examples

**Files:**
- Modify: `examples/basic/policy.rego`
- Modify: `examples/split/git.rego`
- Modify: `examples/split/cargo.rego`
- Modify: `examples/split/npm.rego`
- Modify: `examples/split/safety.rego`

**Step 1: Update examples**

Note: The examples use the older flat `decision`/`reason` pattern (not the `rules[]` map pattern). Update them to use the modern `rules[]` pattern with the new helpers, demonstrating both helpers and the declarative table.

`examples/basic/policy.rego`:
```rego
# METADATA
# entrypoint: true
package cmdguard

import rego.v1

# Declarative subcommand allowlists
allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
}

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}

# Safe git write operations
rules["safe_git_write"] := allow("Safe git write operation") if {
	input.command[0] == "git"
	input.subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

# Package manager commands
rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

# Read-only commands
rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Echo commands
rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

# Deny force push
rules["deny_force_push"] := deny("Force push is blocked - use regular push instead") if {
	input.command[0] == "git"
	input.subcommand == "push"
	some flag in input.command
	flag in {"--force", "-f", "--force-with-lease"}
}

# Deny dangerous commands
rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

# Deny recursive delete outside project
rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	path_outside_project
}
```

`examples/split/git.rego`:
```rego
package cmdguard

import rego.v1

allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
	"ls-tree",
}

rules["safe_git_write"] := allow("Safe git write operation") if {
	input.command[0] == "git"
	input.subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

rules["deny_force_push"] := deny("Force push is blocked - use regular push instead") if {
	input.command[0] == "git"
	input.subcommand == "push"
	some flag in input.command
	flag in {"--force", "-f", "--force-with-lease"}
}
```

`examples/split/cargo.rego`:
```rego
package cmdguard

import rego.v1

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}
```

`examples/split/npm.rego`:
```rego
package cmdguard

import rego.v1

rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}
```

`examples/split/safety.rego`:
```rego
package cmdguard

import rego.v1

rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	path_outside_project
}

rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}
```

**Step 2: Verify examples parse correctly**

Run: `cargo run -- eval --policy-dir examples/basic "cargo build"`
Expected: Decision is allow

Run: `cargo run -- eval --policy-dir examples/split "git status"`
Expected: Decision is allow

**Step 3: Commit**

```bash
git add examples/
git commit -m "refactor: update examples to use stdlib helpers and allowed_subcommands"
```

---

### Task 7: Add test cases for new features

**Files:**
- Modify: `config/policy_tests.yaml`

**Step 1: Add test cases**

Append these to the end of `config/policy_tests.yaml`:

```yaml
  # ==========================================================================
  # Stdlib Helpers (verified via migrated rules)
  # ==========================================================================

  - name: "allow() helper - cargo clippy via allowed_subcommands"
    command: "cargo clippy"
    expect: allow

  - name: "deny() helper - git reset --hard"
    command: "git reset --hard HEAD"
    expect: deny

  - name: "ask() helper - find with -exec"
    command: "find . -name '*.tmp' -exec rm {} \\;"
    expect: ask

  # ==========================================================================
  # Declarative Subcommand Table
  # ==========================================================================

  - name: "allowed_subcommands - git fetch"
    command: "git fetch"
    expect: allow

  - name: "allowed_subcommands - git rev-parse"
    command: "git rev-parse HEAD"
    expect: allow

  - name: "allowed_subcommands - cargo doc"
    command: "cargo doc"
    expect: allow

  - name: "allowed_subcommands - cargo clean"
    command: "cargo clean"
    expect: allow

  - name: "unlisted subcommand falls through to ask"
    command: "cargo install some-crate"
    expect: ask
```

**Step 2: Run all tests**

Run: `cargo run -- test`
Expected: All tests pass, including the new ones.

**Step 3: Commit**

```bash
git add config/policy_tests.yaml
git commit -m "test: add test cases for stdlib helpers and allowed_subcommands"
```

---

### Task 8: Final verification

**Step 1: Run full test suite**

Run: `cargo run -- test --verbose`
Expected: All tests pass with verbose output showing each test result.

**Step 2: Run cargo tests**

Run: `cargo test`
Expected: All Rust unit tests pass (including hook tests from earlier work).

**Step 3: Spot-check eval output**

Run: `cargo run -- eval "cargo build"`
Expected: Decision is allow, rule is `allowed_cargo_build`

Run: `cargo run -- eval "git push -f origin main"`
Expected: Decision is deny, rule is `force_push_structured`

Run: `cargo run -- eval "unknown-tool --flag"`
Expected: Decision is ask (no matching rule)
