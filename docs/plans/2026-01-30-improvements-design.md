# Claude Permissions Improvements Design

Date: 2026-01-30

## Overview

Six improvements to the claude-permissions hook system:

1. Compound command parsing
2. Dual policy engine (Rego + Nickel)
3. Rego policy refactoring for traceability
4. Execution environment info
5. Project-local rules
6. Configurable wrappers and command definitions

## 1. Compound Command Parsing

### Problem

Commands like `echo foo && rm -rf /` are parsed as just `echo`, allowing dangerous subsequent commands through.

### Solution

Replace `shlex` tokenization with tree-sitter bash grammar parsing.

### Behavior

- Parse compound operators: `&&`, `||`, `;`, `|`
- Respect quotes, escapes, and parentheses grouping
- Evaluate each command left-to-right
- Short-circuit on first non-allow (deny or ask returns immediately)
- Unparseable constructs (tree-sitter error nodes) default to `ask`

### Scope

Minimal - top-level compounds only. Embedded code analysis (`python -c "..."`) deferred to future work.

### Dependencies

- `tree-sitter`
- `tree-sitter-bash`

### Policy Input Changes

```json
{
  "chain_position": 2,
  "chain_length": 3,
  "chain_operator": "&&"
}
```

## 2. Dual Policy Engine (Rego + Nickel)

### Problem

Rego has sharp edges:
- Conflicting rules hard to debug
- No static type checking
- Unclear which rule produced a decision

### Solution

Add Nickel as a second policy engine. Run both in parallel for UX comparison.

### Architecture

```rust
trait PolicyEngine {
    fn load_policies(&mut self, path: &Path) -> Result<()>;
    fn evaluate(&self, input: &PolicyInput) -> Result<PolicyResult>;
}
```

### PolicyResult Structure

Both engines return:

```json
{
  "decision": "allow",
  "reason": "Safe git command",
  "rule": "safe_git",
  "explicit": true
}
```

The `explicit` field distinguishes rules that matched vs. default fallback.

### Decision Source Modes

Configurable via settings:

- `rego` - use Rego's decision
- `nickel` - use Nickel's decision
- `strict` - use stricter decision, comparing only explicit rules

### Strict Mode Logic

```
(rego.explicit, nickel.explicit) →
  (true, true)   → stricter of the two (deny > ask > allow)
  (true, false)  → rego's decision
  (false, true)  → nickel's decision
  (false, false) → ask
```

### Logging

All discrepancies between engines logged for analysis.

### Dependencies

- `nickel-lang-core`

## 3. Rego Policy Refactoring

### Problem

Current policies use separate `decision` and `reason` rules with no traceability.

### Current Pattern

```rego
decision := "allow" if is_safe_git
reason := "Safe git command" if is_safe_git
```

### New Pattern

```rego
rules["safe_git"] := {
    "decision": "allow",
    "reason": "Safe git command",
} if is_safe_git
```

### Aggregation (stdlib.rego)

```rego
matched_rules := [{"name": name, "rule": r} | some name; r := rules[name]]

result := pick_highest_priority(matched_rules) if count(matched_rules) > 0

default result := {
    "decision": "ask",
    "reason": "No rule matched",
    "rule": "default",
    "explicit": false
}
```

### Files to Refactor

All 11 policy files in `config/`:
- git.rego
- safe.rego
- rust.rego
- python.rego
- nix.rego
- opa.rego
- rego.rego
- find.rego
- tools.rego
- javascript.rego
- stdlib.rego (add aggregation logic)

### Effort

~2-3 hours mechanical refactoring plus evaluator update in `policy.rs`.

## 4. Execution Environment Info

### Problem

Policies can't distinguish `/usr/bin/git` from a user wrapper at `~/.local/bin/git`.

### Solution

Add PATH resolution and trust zone classification to policy input.

### New Policy Input Fields

```json
{
  "command_as_typed": "./target/debug/claude-permissions",
  "binary_name": "claude-permissions",
  "resolved_path": "/Users/user/project/target/debug/claude-permissions",
  "resolved_trust_zone": "project",
  "is_symlink": false,
  "symlink_target": null
}
```

When resolution fails: `resolved_path: null`, `resolved_trust_zone: "unknown"`.

### Trust Zones

Hybrid approach - sensible defaults with config override:

- `system` - `/usr/bin`, `/bin`, `/usr/local/bin`, etc.
- `user` - `~/.local/bin`, `~/bin`, etc.
- `project` - under `$PROJECT_ROOT`
- `unknown` - resolution failed or not in any known zone

### Configuration Override

```nickel
trust_zones = {
  system = ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"],
  user = ["~/.local/bin", "~/bin", "~/.cargo/bin"],
  project = ["./node_modules/.bin", "./target/release"],
}
```

### Policy Usage Example

```rego
deny_untrusted_binary if {
    input.resolved_trust_zone == "unknown"
    input.binary_name in {"rm", "chmod", "chown"}
}

allowed_local_tool if {
    input.binary_name == "claude-permissions"
    input.resolved_trust_zone == "project"
}
```

## 5. Project-Local Rules

### Problem

All rules are global. Projects may need custom policies.

### Solution

Support project-local rules in `.claude/permissions/` with priority-based merging.

### Location

`.claude/permissions/*.rego` and/or `.claude/permissions/*.ncl`

### Priority-Based Merge

Higher number wins:

| Source  | Decision | Default Priority |
|---------|----------|------------------|
| Global  | deny     | 100              |
| Project | deny     | 75               |
| Global  | ask      | 50               |
| Project | ask      | 40               |
| Global  | allow    | 25               |
| Project | allow    | 20               |

### Merge Behavior

1. Collect all matching rules from global and project
2. Sort by priority (highest first)
3. Return first match

### Override Capability

To override a global deny from a project, explicitly set higher priority:

```rego
rules["allow_rm_in_temp"] := {
    "decision": "allow",
    "reason": "Temp cleanup allowed in this project",
    "priority": 101,
} if is_rm_in_temp_dir
```

### Loading Order

1. Load global rules from `~/.config/claude-permissions/`
2. Load project rules from `$PROJECT_ROOT/.claude/permissions/`
3. Merge by priority at evaluation time

## 6. Configurable Wrappers & Command Definitions

### Problem

Wrapper extraction is hardcoded in Rust. Adding new wrappers requires recompilation.

### Solution

Define wrappers and command schemas in Nickel config.

### Flag Types

```nickel
let Flag = {
  Boolean,         # -v, --verbose
  WithArg,         # -u root, --user=root
  WithOptionalArg, # --color, --color=always
  Repeatable,      # -v -v -v
}
```

### Argument Types

```nickel
let Arg = {
  String,          # arbitrary string value
  Path,            # file/directory path (will be resolved and classified)
  Number,          # numeric value
}
```

### Command Definition with Flag Aliases

```nickel
commands.sudo = {
  flags = {
    "user" = {
      type = Flag.WithArg,
      short = "-u",
      long = "--user",
    },
    "group" = {
      type = Flag.WithArg,
      short = "-g",
      long = "--group",
    },
    "preserve_env" = {
      type = Flag.Boolean,
      short = "-E",
      long = "--preserve-env",
    },
  },
}

commands.rm = {
  flags = {
    "recursive" = {
      type = Flag.Boolean,
      short = "-r",
      long = "--recursive",
    },
    "force" = {
      type = Flag.Boolean,
      short = "-f",
      long = "--force",
    },
  },
  positional = [
    { name = "targets", type = Arg.Path, variadic = true },
  ],
}

commands.chmod = {
  flags = {
    "recursive" = { type = Flag.Boolean, short = "-R" },
  },
  positional = [
    { name = "mode", type = Arg.String },
    { name = "targets", type = Arg.Path, variadic = true },
  ],
}

commands.cp = {
  flags = {
    "recursive" = { type = Flag.Boolean, short = "-r", long = "--recursive" },
  },
  positional = [
    { name = "sources", type = Arg.Path, variadic = true },
    { name = "dest", type = Arg.Path, last = true },
  ],
}
```

### Wrapper Definition

```nickel
wrappers.sudo = {
  flags = commands.sudo.flags,
  extract = fun parsed => parsed.remaining_args,
}
```

### Enhanced Policy Input

For `sudo -u postgres psql`:

```json
{
  "command": ["sudo", "-u", "postgres", "psql"],
  "parsed_flags": {
    "user": "postgres"
  },
  "positional_args": {
    "_raw": ["psql"]
  }
}
```

For `chmod 755 ./src ../other/file`:

```json
{
  "command": ["chmod", "755", "./src", "../other/file"],
  "parsed_flags": {
    "recursive": false
  },
  "positional_args": {
    "mode": "755",
    "targets": [
      { "raw": "./src", "resolved": "/project/src", "trust_zone": "project" },
      { "raw": "../other/file", "resolved": "/other/file", "trust_zone": "unknown" }
    ]
  }
}
```

Path-typed positional args are automatically resolved and classified with trust zones.

Flags normalized to canonical name regardless of whether `-u` or `--user` was typed.

### Policy Usage

```rego
# Before: fragile string matching
dangerous_rm if "-rf" in input.command

# After: structured flag access
dangerous_rm if {
    input.binary_name == "rm"
    input.parsed_flags.recursive
    input.parsed_flags.force
}

# Flag with argument
deny_sudo_root if {
    input.binary_name == "sudo"
    input.parsed_flags.user == "root"
}

allow_sudo_service if {
    input.binary_name == "sudo"
    input.parsed_flags.user in {"postgres", "redis", "nginx"}
}

# Positional args with path checking
chmod_outside_project if {
    input.binary_name == "chmod"
    some target in input.positional_args.targets
    target.trust_zone != "project"
}

rm_outside_project if {
    input.binary_name == "rm"
    some target in input.positional_args.targets
    target.trust_zone != "project"
}

# Deny cp to system directories
cp_to_system if {
    input.binary_name == "cp"
    input.positional_args.dest.trust_zone == "system"
}
```

## Implementation Order

Suggested sequence based on dependencies:

| Order | Feature | Effort | Dependencies |
|-------|---------|--------|--------------|
| 1 | Rego refactoring | ~3 hours | None |
| 2 | Compound commands | ~1 week | tree-sitter |
| 3 | Execution environment | ~2-3 days | None |
| 4 | Project-local rules | ~2-3 days | #1 (priority system) |
| 5 | Dual engine | ~1 week | nickel-lang-core |
| 6 | Configurable wrappers | ~1 week | #5 (Nickel) |

## New Dependencies

- `tree-sitter` - incremental parsing library
- `tree-sitter-bash` - bash grammar
- `nickel-lang-core` - Nickel language runtime
