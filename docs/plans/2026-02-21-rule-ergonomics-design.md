# Design: Rule Ergonomics — Stdlib Helpers and Declarative Subcommand Table

## Problem

Writing Rego policy rules requires repeating `{ "decision": "allow", "reason": "...", "priority": 25 }` for every rule. For the common case of "allow binary X with subcommands Y", this results in ~6 lines of boilerplate per rule.

## Solution

Two complementary changes to `stdlib.rego`:

### 1. Decision helper functions

```rego
# Default-priority helpers
allow(reason) := {"decision": "allow", "reason": reason, "priority": 25}
deny(reason) := {"decision": "deny", "reason": reason, "priority": 100}
ask(reason) := {"decision": "ask", "reason": reason, "priority": 50}

# Custom-priority variants
allow_at(reason, priority) := {"decision": "allow", "reason": reason, "priority": priority}
deny_at(reason, priority) := {"decision": "deny", "reason": reason, "priority": priority}
ask_at(reason, priority) := {"decision": "ask", "reason": reason, "priority": priority}
```

Default priorities (25/50/100) match existing conventions across all policy files.

### 2. Declarative subcommand table

A rule-generation pattern in stdlib that consumes a user-defined `allowed_subcommands` partial object:

```rego
# In stdlib.rego:
default allowed_subcommands := {}

rules[rule_name] := allow(reason) if {
    some binary, subcmds in allowed_subcommands
    input.binary_name == binary
    input.subcommand in subcmds
    rule_name := sprintf("allowed_%s_%s", [binary, input.subcommand])
    reason := sprintf("Allowed %s subcommand", [binary])
}
```

User policy files contribute entries using Rego partial object rules, which merge automatically across files in the same package:

```rego
# git.rego
allowed_subcommands["git"] := {"status", "log", "diff", "show", "branch"}

# rust.rego
allowed_subcommands["cargo"] := {"build", "check", "test", "run", "clippy", "fmt"}

# node.rego
allowed_subcommands["npm"] := {"test", "run", "list"}
```

Complex rules (flag checks, path constraints, regex matching) continue to use the decision helpers directly:

```rego
rules["no_force_push"] := deny("Force push blocked") if {
    input.binary_name == "git"
    input.subcommand == "push"
    input.parsed_flags.force == true
}
```

## Migration

Existing policy files are updated to use the new helpers:

- **Remove local helper functions** (e.g., `allowed()` in rust.rego) in favor of stdlib `allow()`
- **Replace inline decision maps** with `allow()`/`deny()`/`ask()` calls
- **Move simple subcommand lists** into `allowed_subcommands` partial objects
- **Keep complex conditional rules** as regular rules using the helpers

No output format changes — decision/reason/priority structure stays identical.

## Testing

- Existing test suite validates migrated rules produce identical results
- New test cases for helpers (`allow`, `deny`, `ask`, `_at` variants)
- New test cases for subcommand table (allowed subcommand, unlisted subcommand, multi-file merge)

## Files to modify

- `config/stdlib.rego` — Add helpers and subcommand table rule generator
- `config/rust.rego` — Migrate to helpers + subcommand table
- `config/git.rego` — Migrate to helpers, move safe subcommands to table
- `config/safe.rego` — Migrate to helpers
- `config/gh.rego` — Migrate to helpers + subcommand table
- `config/python.rego` — Migrate to helpers
- `examples/` — Update examples to demonstrate both patterns
- `config/policy_tests.yaml` — Add test cases for new features
