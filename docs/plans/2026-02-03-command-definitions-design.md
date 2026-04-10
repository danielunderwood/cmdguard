# Command Definitions & Flag Parsing Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add structured flag parsing using Nickel config files, enabling policies to inspect command flags and positional arguments in a structured way instead of string matching.

**Architecture:** Nickel config defines command schemas (flags, positional args). A new parser module uses these definitions to extract structured data from commands. Results are added to PolicyInput.

**Tech Stack:** nickel-lang for config, Rust for parsing logic

---

## Config File Location

`~/.config/cmdguard/commands.ncl`

Falls back to built-in defaults if file doesn't exist.

## Schema Structure

```nickel
{
  commands = {
    rm = {
      flags = {
        recursive = { short = ["-r", "-R"], long = "--recursive", type = "boolean" },
        force = { short = "-f", long = "--force", type = "boolean" },
        no_preserve_root = { long = "--no-preserve-root", type = "boolean" },
      },
      positional = {
        targets = { type = "path", variadic = true },
      },
      parsing = {
        combine_short_flags = true,
      },
    },

    chmod = {
      flags = {
        recursive = { short = "-R", long = "--recursive", type = "boolean" },
      },
      positional = {
        mode = { type = "string", position = 0 },
        targets = { type = "path", variadic = true },
      },
    },

    cp = {
      flags = {
        recursive = { short = ["-r", "-R"], long = "--recursive", type = "boolean" },
        force = { short = "-f", long = "--force", type = "boolean" },
        no_clobber = { short = "-n", long = "--no-clobber", type = "boolean" },
      },
      positional = {
        sources = { type = "path", variadic = true },
        destination = { type = "path", last = true },
      },
    },

    mv = {
      flags = {
        force = { short = "-f", long = "--force", type = "boolean" },
        no_clobber = { short = "-n", long = "--no-clobber", type = "boolean" },
      },
      positional = {
        sources = { type = "path", variadic = true },
        destination = { type = "path", last = true },
      },
    },

    chown = {
      flags = {
        recursive = { short = "-R", long = "--recursive", type = "boolean" },
      },
      positional = {
        owner = { type = "string", position = 0 },
        targets = { type = "path", variadic = true },
      },
    },

    sudo = {
      flags = {
        user = { short = "-u", long = "--user", type = "with_arg" },
        group = { short = "-g", long = "--group", type = "with_arg" },
        stdin = { short = "-S", long = "--stdin", type = "boolean" },
        login = { short = "-i", long = "--login", type = "boolean" },
        preserve_env = { short = "-E", long = "--preserve-env", type = "boolean" },
      },
      is_wrapper = true,
    },

    git = {
      subcommands = {
        push = {
          flags = {
            force = { short = "-f", long = "--force", type = "boolean" },
            force_with_lease = { long = "--force-with-lease", type = "boolean" },
            delete = { short = "-d", long = "--delete", type = "boolean" },
            set_upstream = { short = "-u", long = "--set-upstream", type = "boolean" },
          },
        },
        reset = {
          flags = {
            hard = { long = "--hard", type = "boolean" },
            soft = { long = "--soft", type = "boolean" },
            mixed = { long = "--mixed", type = "boolean" },
          },
        },
        clean = {
          flags = {
            force = { short = "-f", long = "--force", type = "boolean" },
            directories = { short = "-d", type = "boolean" },
            ignored = { short = "-x", type = "boolean" },
          },
        },
      },
    },
  },

  defaults = {
    combine_short_flags = true,
    double_dash_ends_flags = true,
  },
}
```

## Flag Types

| Type | Description | Example |
|------|-------------|---------|
| `boolean` | Flag with no argument | `-v`, `--verbose` |
| `with_arg` | Flag requires argument | `-u root`, `--user=root` |
| `with_optional_arg` | Argument is optional | `--color`, `--color=always` |

## Positional Argument Types

| Type | Description | Resolution |
|------|-------------|------------|
| `string` | Plain string value | No processing |
| `path` | File/directory path | Resolved with trust zone |
| `number` | Numeric value | Parsed as number |

## Positional Argument Modifiers

| Modifier | Description |
|----------|-------------|
| `position = N` | Fixed position (0-indexed) |
| `variadic = true` | Consumes multiple arguments |
| `last = true` | Must be last argument (like cp destination) |
| `optional = true` | May not be present |

## Parsing Rules

### Combined Short Flags

When `combine_short_flags = true` (default):
- `-rf` expands to `-r -f`
- Only for single-dash flags
- Only alphanumeric characters
- Not for numeric values (`-1` stays as `-1`)

### Double Dash

When `double_dash_ends_flags = true` (default):
- `--` ends flag parsing
- Everything after is positional
- Example: `rm -- -rf` removes file named `-rf`

### Flag Argument Formats

All supported:
- `--user=root` (long with equals)
- `--user root` (long with space)
- `-u root` (short with space)

## PolicyInput Changes

New fields added:

```json
{
  "parsed_flags": {
    "recursive": true,
    "force": true,
    "user": "postgres"
  },
  "positional_args": [
    {
      "name": "targets",
      "values": [
        {
          "raw": "./src",
          "resolved": "/home/user/project/src",
          "trust_zone": "project",
          "type": "path"
        }
      ]
    }
  ],
  "subcommand": "push"
}
```

### Field Definitions

| Field | Type | Description |
|-------|------|-------------|
| `parsed_flags` | `Object` | Map of flag name to value (true for boolean, string for with_arg) |
| `positional_args` | `Array` | List of positional argument groups with resolved values |
| `subcommand` | `String` | For commands like git, the subcommand (push, reset, etc.) |

## Policy Usage Examples

```rego
# Before: fragile string matching
dangerous_rm if "-rf" in input.command
dangerous_rm if "-r" in input.command; "-f" in input.command

# After: structured flag access
dangerous_rm if {
    input.binary_name == "rm"
    input.parsed_flags.recursive
    input.parsed_flags.force
}

# Check flag values
deny_sudo_root if {
    input.binary_name == "sudo"
    input.parsed_flags.user == "root"
}

# Check positional args with trust zones
rm_outside_project if {
    input.binary_name == "rm"
    some arg in input.positional_args
    arg.name == "targets"
    some target in arg.values
    target.trust_zone != "project"
}

# Git subcommand handling
deny_force_push if {
    input.binary_name == "git"
    input.subcommand == "push"
    input.parsed_flags.force
}
```

## Implementation

### New Module: `src/command_parser.rs`

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct ParsedCommand {
    pub parsed_flags: HashMap<String, FlagValue>,
    pub positional_args: Vec<PositionalArg>,
    pub subcommand: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum FlagValue {
    Boolean(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionalArg {
    pub name: String,
    pub values: Vec<PositionalValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionalValue {
    pub raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_zone: Option<String>,
    #[serde(rename = "type")]
    pub value_type: String,
}

pub fn parse_command(
    tokens: &[String],
    binary_name: &str,
    definitions: &CommandDefinitions,
    project_root: Option<&Path>,
) -> ParsedCommand;
```

### New Module: `src/command_defs.rs`

```rust
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CommandDefinitions {
    pub commands: HashMap<String, CommandDef>,
    pub defaults: ParsingDefaults,
}

#[derive(Debug, Clone)]
pub struct CommandDef {
    pub flags: HashMap<String, FlagDef>,
    pub positional: Vec<PositionalDef>,
    pub subcommands: Option<HashMap<String, SubcommandDef>>,
    pub is_wrapper: bool,
    pub parsing: Option<ParsingOptions>,
}

impl CommandDefinitions {
    /// Load from Nickel file, falling back to built-in defaults
    pub fn load(config_dir: &Path) -> Result<Self, String>;

    /// Get built-in default definitions
    pub fn builtin() -> Self;
}
```

## Tasks

### Task 1: Add nickel-lang dependency
- Add `nickel-lang = "2.0"` to Cargo.toml
- Verify it compiles

### Task 2: Create command_defs module
- Define Rust structs for command definitions
- Implement built-in defaults (rm, chmod, cp, mv, chown, sudo, git)
- Add function to load from Nickel file

### Task 3: Create command_parser module
- Implement flag parsing logic
- Handle combined short flags expansion
- Handle `--` separator
- Handle flag arguments (equals and space forms)

### Task 4: Implement positional argument parsing
- Parse positional args based on definition
- Resolve path-type args with trust zones
- Handle variadic and last-position args

### Task 5: Handle subcommands (git)
- Detect subcommand from tokens
- Look up subcommand-specific flag definitions
- Parse with subcommand context

### Task 6: Update PolicyInput
- Add parsed_flags, positional_args, subcommand fields
- Update all PolicyInput construction sites

### Task 7: Integrate into hook flow
- Load command definitions on startup
- Call parser for each command
- Populate PolicyInput with parsed data

### Task 8: Update eval command
- Show parsed flags in output
- Show positional args with resolution

### Task 9: Write default commands.ncl
- Create example Nickel config file
- Include all minimal command definitions
- Add comments explaining the schema

### Task 10: Add unit tests
- Test flag parsing (boolean, with_arg)
- Test combined flag expansion
- Test double-dash handling
- Test positional arg parsing
- Test subcommand detection

### Task 11: Add integration tests
- Test full flow with parsed flags in PolicyInput
- Test policy rules using parsed_flags

### Task 12: Update example policies
- Add examples using parsed_flags
- Add examples using positional_args with trust zones
