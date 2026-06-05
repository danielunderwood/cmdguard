# cmdguard

Policy-driven permission control for AI coding agents. cmdguard evaluates shell commands against [Rego](https://www.openpolicyagent.org/docs/latest/policy-language/) policies as a PreToolUse hook, silently allowing safe commands and blocking dangerous ones.

**See also:** [Security Model](docs/security-model.md) | [Common Rules Recipes](docs/common-rules.md)

## Features

- **Automatic decisions**: Safe commands pass silently; dangerous ones are blocked or prompt the user
- **Compound command parsing**: Handles `&&`, `||`, `;`, `|` chains -- every segment is evaluated
- **Wrapper extraction**: Sees through `nix develop`, `docker run`, `sudo`, inline env vars
- **Trust zones**: Classifies binaries as system, user, project, or unknown by path
- **Parsed flags**: Access flags by name (`input.parsed_flags.force`) instead of string matching
- **Declarative tables**: Simple allow-lists for subcommands and first-argument patterns
- **Exclusion tables**: Surgically block specific subcommands without rewriting allow rules
- **Base + user separation**: Shipped policies update cleanly; your customizations are never overwritten
- **Project-local rules**: Per-project policies in `.cmdguard/`
- **Fail-safe**: Defaults to `ask` on any error

## Quick Start

### Option A: Prebuilt binary (no Rust toolchain needed)

Download the latest release for your platform from the [releases page][releases],
extract it, and place the binary on `PATH`:

```bash
# Linux x86_64
curl -L -o /tmp/cmdguard.tar.gz \
  https://github.com/danielunderwood/cmdguard/releases/latest/download/cmdguard-x86_64-unknown-linux-gnu.tar.gz

# macOS Apple Silicon
curl -L -o /tmp/cmdguard.tar.gz \
  https://github.com/danielunderwood/cmdguard/releases/latest/download/cmdguard-aarch64-apple-darwin.tar.gz

mkdir -p ~/.local/bin
tar xzf /tmp/cmdguard.tar.gz -C ~/.local/bin
cmdguard base sync       # populate ~/.config/cmdguard/{base,policies}/
cmdguard hook install    # register in ~/.claude/settings.json
```

For other platforms (e.g. Linux aarch64), build from source.

### Option B: Build from source

Requires a Rust toolchain (1.75+ recommended).

```bash
git clone https://github.com/danielunderwood/cmdguard
cd cmdguard
./install.sh
```

`install.sh` runs three steps:

1. `cargo build --release` and copies to `~/.local/bin/cmdguard`
2. `cmdguard base sync` writes base policies to `~/.config/cmdguard/base/`
3. `cmdguard hook install` registers the binary in `~/.claude/settings.json`

Or via cargo directly:

```bash
cargo install --git https://github.com/danielunderwood/cmdguard
cmdguard base sync
cmdguard hook install
```

[releases]: https://github.com/danielunderwood/cmdguard/releases

After installation, cmdguard is active. Test it:

```bash
cmdguard eval "git status"               # -> allow
cmdguard eval "rm --no-preserve-root /"  # -> deny
cmdguard eval "curl example.com"         # -> ask
```

## Directory Structure

```
~/.config/cmdguard/
  base/                          # Shipped policies (managed by cmdguard base sync)
    stdlib.rego                  # Decision helpers, table dispatch, priority resolution
    safe.rego                    # Always-safe read-only commands (cat, ls, grep, ...)
    git.rego                     # Git subcommand allow-list + force-push deny
    rust.rego                    # Cargo subcommand allow-list
    go.rego                      # Go tool allow-list
    python.rego                  # Python/pytest rules + inline code analysis
    javascript.rego              # npm/yarn/npx rules
    gh.rego                      # GitHub CLI rules
    kubectl.rego                 # kubectl/helm/flux rules
    docker.rego                  # Docker subcommand rules
    file-ops.rego                # rm/chmod/chown/mv/cp path restrictions
    find.rego                    # find command rules
    network.rego                 # curl/wget/rsync rules
    sed.rego                     # sed rules
    inproject.rego               # Project-scoped binary rules
    tools.rego                   # Developer tools (jq, xargs, ...)
    builtins.ncl                 # Command flag/positional schemas

  policies/                      # Your customizations (never overwritten)
    custom.rego                  # Starter template created on first `base sync`;
                                 # add your own rules here. Drop in additional
                                 # *.rego files alongside it for organization.

  commands.ncl                   # Optional: custom wrapper extractors and command schemas

.cmdguard/                       # Project-local policies (in any project repo)
  *.rego                         # Merged with global rules at lower priority
```

## How It Works

cmdguard receives the command string from the agent hook, then:

1. **Parses** compound commands into segments
2. **Extracts** the real command from wrappers (`sudo`, `nix develop --command`, etc.)
3. **Resolves** the binary path and classifies its trust zone
4. **Parses** flags and positional arguments using command schemas
5. **Evaluates** all Rego policies; each matching rule returns a decision with priority
6. **Returns** the highest-priority decision: deny (100) > ask (50) > allow (25)

### Decisions

| Decision | Priority | Behavior |
|----------|----------|----------|
| `deny`   | 100      | Block the command, agent sees the reason |
| `ask`    | 50       | Prompt the user for confirmation |
| `allow`  | 25       | Silent pass, no prompt |

When multiple rules match, the highest priority wins. Override with explicit `"priority": N`.

## Writing Policies

Custom rules live in `~/.config/cmdguard/policies/`. The `cmdguard base
sync` command creates a starter `custom.rego` there on first run; edit
it directly, or add additional `*.rego` files alongside it (every
`.rego` file in `policies/` is loaded). Files in `base/` are managed by
the binary and shouldn't be edited — your changes there are blown away
on the next `base sync`.

After editing, no reload is needed. The hook reads policies fresh on
each invocation, so the next command will use the updated rules. To
sanity-check, run `cmdguard eval "<some command>"` (or
`cmdguard test path/to/tests.yaml`) before relying on the rule.

### Declarative Tables (Simplest)

For commands with subcommands (git, cargo, docker, etc.), add entries to the allow-list table:

```rego
package cmdguard

import rego.v1

allowed_subcommands["cargo"] := {"build", "test", "check", "clippy"}
```

For commands where the first argument is the action (go, make, npx, etc.):

```rego
package cmdguard

import rego.v1

allowed_with_args["make"] := {"build", "test", "clean", "lint"}
```

These are automatically dispatched by stdlib. No rule body needed.

### Exclusion Tables

Block specific subcommands without editing the base allow-list:

```rego
package cmdguard

import rego.v1

# Prevent cargo publish (allowed in base rust.rego)
denied_subcommands["cargo"] := {"publish"}
```

Similarly for first-argument patterns:

```rego
denied_with_args["npx"] := {"some-dangerous-tool"}
```

### Custom Rules

For conditional logic, write named rules:

```rego
package cmdguard

import rego.v1

# Block rm outside the project directory
rules["deny_rm_outside_project"] := deny("Cannot rm files outside project") if {
    input.binary_name == "rm"
    some target in input.positional.targets
    target.trust_zone != "project"
}

# Ask before force push
rules["ask_force_push"] := ask("Force push requires confirmation") if {
    input.binary_name == "git"
    input.subcommand == "push"
    input.parsed_flags.force
}
```

The `allow()`, `deny()`, and `ask()` helpers from stdlib set the default priorities. Use `allow_at(reason, priority)`, `deny_at()`, and `ask_at()` to set custom priorities.

### Claude Code Auto Mode

cmdguard installs as a Claude Code Bash `PreToolUse` hook. It can allow,
deny, or ask before a Bash command runs. Claude Code auto mode is a
separate classifier layer with its own rules for actions such as direct
pushes to default branches, irreversible local writes, production
changes, and data exfiltration.

The base policy is intentionally conservative where cmdguard cannot infer
Claude's session context. For example, `git push` prompts by default
because the command string alone does not prove whether the target is a
non-default working branch. `PermissionDenied` hooks are useful for
observing auto-mode classifier denials, but they cannot reverse those
denials.

### Priority System

| Source  | Decision | Default Priority |
|---------|----------|------------------|
| Global  | deny     | 100              |
| Project | deny     | 75               |
| Global  | ask      | 50               |
| Project | ask      | 40               |
| Global  | allow    | 25               |
| Project | allow    | 20               |

## Policy Input

Your policies receive structured input for each command:

```json
{
  "tool": "Bash",
  "raw_command": "sudo -u postgres rm -rf ./temp",
  "command": ["rm", "-rf", "./temp"],
  "wrapper_chain": ["sudo"],
  "binary_name": "rm",
  "resolved_path": "/bin/rm",
  "resolved_trust_zone": "system",
  "subcommand": null,
  "parsed_flags": {
    "recursive": true,
    "force": true
  },
  "positional": {
    "targets": [{"raw": "./temp", "resolved": "/project/temp", "trust_zone": "project"}]
  },
  "paths": [{"raw": "./temp", "resolved": "/project/temp", "exists": true, "is_dir": true}],
  "cwd": "/home/user/project",
  "project_root": "/home/user/project",
  "chain_position": 1,
  "chain_length": 1,
  "chain_operator": null
}
```

### Trust Zones

Binaries are classified by location:

| Zone | Paths |
|------|-------|
| `system` | `/usr/bin`, `/bin`, `/usr/local/bin`, Nix store, Homebrew |
| `user` | `~/.local/bin`, `~/.cargo/bin`, `~/bin` |
| `project` | Under `$PROJECT_ROOT` |
| `unknown` | Resolution failed or not in any known zone |

### Parsed Flags

Instead of fragile string matching, use structured flag access:

```rego
# Fragile -- breaks with -rf, --recursive, etc.
dangerous if "-rf" in input.command

# Robust -- works regardless of flag format
dangerous if {
    input.parsed_flags.recursive
    input.parsed_flags.force
}
```

Flag definitions come from built-in schemas (`builtins.ncl`) and can be extended via `commands.ncl`.

## CLI Reference

```bash
# Evaluate a command (for debugging/testing)
cmdguard eval "git push --force"
cmdguard eval "rm -rf ./temp" --show-input    # Show JSON input sent to Rego

# Run policy tests
cmdguard test                                  # Uses policy_tests.yaml in policy dir
cmdguard test my_tests.yaml --verbose

# Manage base policies
cmdguard base sync                             # Write/update base policies

# Show loaded policies and tables
cmdguard status

# Manage hook registration
cmdguard hook install                          # Register in ~/.claude/settings.json
cmdguard hook uninstall
cmdguard hook status

# Validate Nickel configuration
cmdguard validate
```

## Testing Policies

Write test cases in YAML:

```yaml
tests:
  - name: "allow git status"
    command: "git status"
    expect: allow
    reason_contains: "git"

  - name: "ask for force push"
    command: "git push --force origin main"
    expect: ask

  - name: "ask for curl"
    command: "curl https://example.com"
    expect: ask
```

Run with:

```bash
cmdguard test
cmdguard test --verbose
```

## Project-Local Rules

Add `.cmdguard/*.rego` files to a project for project-specific policies:

```
my-project/
  .cmdguard/
    make.rego          # Allow make targets in this project
    custom.rego        # Project-specific rules
```

Project rules merge with global rules. They have lower default priority (allow=20, ask=40, deny=75) so global deny rules win by default. To override a global deny:

```rego
rules["allow_npm_scripts"] := {
    "decision": "allow",
    "reason": "NPM scripts allowed in this project",
    "priority": 101,
} if {
    input.binary_name == "npm"
    input.subcommand == "run"
}
```

## Nickel Configuration

Custom wrappers and command definitions go in `~/.config/cmdguard/commands.ncl`:

```nickel
{
  wrappers = {
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

## Debugging

```bash
# Show the full JSON input for a command
cmdguard eval "rm -rf ./temp" --show-input

# Enable debug logging
export RUST_LOG=debug
# Logs go to ~/.local/state/cmdguard/debug.log

# Check what policies are loaded
cmdguard status
```
