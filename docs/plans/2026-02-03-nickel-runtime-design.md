# Nickel Runtime Integration Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable user-defined functions in Nickel config for wrapper extraction and command parsing customization.

**Architecture:** NickelContext module holds the Nickel VM, loads config once at startup, and provides methods to call user-defined functions. Falls back to built-in Rust logic when Nickel functions aren't defined.

**Tech Stack:** nickel-lang 2.0 for Nickel runtime, serde_json for data interchange

---

## Config File Location

`~/.config/claude-permissions/commands.ncl`

Falls back to built-in Rust definitions if file doesn't exist.

## Schema Structure

Note: Nickel 2.0 uses `std.array.slice start end array` instead of `drop`. The function `std.array.at idx array` gets an element at an index.

```nickel
{
  wrappers = {
    # Custom wrapper example: my_tool run <command>
    my_tool = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len > 2 && std.array.at 1 tokens == "run" then
          { remaining = std.array.slice 2 len tokens, wrapper_name = "my_tool run" }
        else
          null
    },

    # Example: flatpak run <app-id> <command>
    flatpak = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len >= 4 && std.array.at 1 tokens == "run" then
          { remaining = std.array.slice 3 len tokens, wrapper_name = "flatpak run" }
        else
          null
    },
  },

  commands = {
    # Command definitions (existing schema)
    rm = {
      flags = { ... },
      positional = [ ... ],
    },
  },

  defaults = {
    combine_short_flags = true,
    double_dash_ends_flags = true,
  },
}
```

## Function Signatures

### Wrapper Extract Function

**Input:** Array of string tokens
**Output:** Record with `remaining` (array) and `wrapper_name` (string), or `null` if not a wrapper

```nickel
# Type signature (conceptual, Nickel doesn't require types)
extract : Array String -> { remaining : Array String, wrapper_name : String } | Null
```

## Implementation

### New Module: `src/nickel_config.rs`

```rust
use nickel_lang::{Context, Expr};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::collections::HashMap;

/// Result of calling a wrapper extract function
#[derive(Debug, Clone, Deserialize)]
pub struct WrapperExtractResult {
    pub remaining: Vec<String>,
    pub wrapper_name: String,
}

/// Nickel configuration context
pub struct NickelConfig {
    context: Option<Context>,
    config_expr: Option<Expr>,
}

impl NickelConfig {
    /// Create new config, loading from file if it exists
    pub fn load(config_path: &Path) -> Self {
        let ncl_path = config_path.join("commands.ncl");
        if !ncl_path.exists() {
            return Self { context: None, config_expr: None };
        }

        let mut context = Context::new();
        let content = std::fs::read_to_string(&ncl_path).ok()?;

        match context.eval_deep(&content) {
            Ok(expr) => Self {
                context: Some(context),
                config_expr: Some(expr),
            },
            Err(e) => {
                tracing::warn!("Failed to load Nickel config: {}", e);
                Self { context: None, config_expr: None }
            }
        }
    }

    /// Check if a custom wrapper extractor is defined
    pub fn has_wrapper(&self, name: &str) -> bool {
        self.get_wrapper_extract(name).is_some()
    }

    /// Call a wrapper's extract function
    pub fn extract_wrapper(
        &mut self,
        name: &str,
        tokens: &[String],
    ) -> Option<WrapperExtractResult> {
        let context = self.context.as_mut()?;
        let config = self.config_expr.as_ref()?;

        // Navigate to wrappers.<name>.extract
        let wrappers = config.as_record()?.get("wrappers")?;
        let wrapper = wrappers.as_record()?.get(name)?;
        let extract_fn = wrapper.as_record()?.get("extract")?;

        // Build function call: extract_fn tokens
        let tokens_json = serde_json::to_string(tokens).ok()?;
        let call_expr = format!(
            "({}) ({})",
            context.expr_to_json(extract_fn).ok()?,
            tokens_json
        );

        // Evaluate the call
        let result = context.eval_deep(&call_expr).ok()?;

        // Check for null
        if result.is_null() {
            return None;
        }

        // Deserialize result
        result.to_serde::<WrapperExtractResult>().ok()
    }

    /// Get command definitions (static data, not functions)
    pub fn get_commands(&self) -> Option<HashMap<String, CommandDef>> {
        let config = self.config_expr.as_ref()?;
        let commands = config.as_record()?.get("commands")?;
        commands.to_serde().ok()
    }

    fn get_wrapper_extract(&self, name: &str) -> Option<&Expr> {
        let config = self.config_expr.as_ref()?;
        let wrappers = config.as_record()?.get("wrappers")?;
        let wrapper = wrappers.as_record()?.get(name)?;
        wrapper.as_record()?.get("extract")
    }
}
```

### Integration with Extractor

Update `src/extractor.rs` to use NickelConfig:

```rust
pub fn extract_command(
    tokens: &[String],
    nickel_config: Option<&mut NickelConfig>,
) -> ExtractedCommand {
    let mut wrapper_chain = Vec::new();
    let mut current = tokens.to_vec();

    loop {
        match try_extract_wrapper(&current, nickel_config) {
            Some((wrapper, inner)) => {
                wrapper_chain.push(wrapper);
                current = inner;
            }
            None => break,
        }
    }

    ExtractedCommand { command: current, wrapper_chain }
}

fn try_extract_wrapper(
    tokens: &[String],
    nickel_config: Option<&mut NickelConfig>,
) -> Option<(String, Vec<String>)> {
    if tokens.is_empty() {
        return None;
    }

    let cmd = &tokens[0];

    // Try Nickel-defined extractor first
    if let Some(config) = nickel_config {
        if let Some(result) = config.extract_wrapper(cmd, tokens) {
            return Some((result.wrapper_name, result.remaining));
        }
    }

    // Fall back to built-in extractors
    match cmd.as_str() {
        "sudo" => extract_sudo(tokens),
        "env" => extract_env(tokens),
        // ... other built-ins
        _ => None,
    }
}
```

### Startup Integration

Update `src/main.rs`:

```rust
fn run_hook_inner() -> Result<HookOutput, String> {
    // ... existing code ...

    // Load Nickel config (once)
    let config_dir = get_policy_dir(None);
    let mut nickel_config = NickelConfig::load(&config_dir);

    // ... later, in evaluate_compound ...
    let extracted = extract_command(&tokens, Some(&mut nickel_config));

    // ...
}
```

## Performance Considerations

1. **One-time load**: Nickel config is loaded once at startup, not per-command
2. **Lazy function calls**: Functions are only called when the wrapper name matches
3. **Fallback path**: If no Nickel config, use fast Rust built-ins
4. **Caching**: Nickel's built-in caching helps with repeated evaluations

Expected overhead:
- Config load: ~10-50ms (one-time)
- Function call: ~1-5ms per wrapper extraction
- Total per-command: negligible compared to network latency

## Tasks

### Task 1: Create nickel_config module structure
- Create `src/nickel_config.rs`
- Add `mod nickel_config;` to main.rs
- Define NickelConfig struct and WrapperExtractResult

### Task 2: Implement config loading
- Load from `~/.config/claude-permissions/commands.ncl`
- Handle missing file (return empty config)
- Handle parse errors (log warning, return empty config)

### Task 3: Implement wrapper function calling
- Navigate Nickel record structure to find extract function
- Build and evaluate function call expression
- Parse result or return None for null

### Task 4: Update extractor module
- Add nickel_config parameter to extract_command
- Try Nickel extractor before built-in
- Fall back to Rust implementation

### Task 5: Integrate into main.rs
- Load NickelConfig at startup
- Pass to evaluate_compound
- Pass to extract_command calls

### Task 6: Update eval command
- Show which extractor was used (nickel vs built-in)
- Show Nickel config load status

### Task 7: Create example commands.ncl
- Include example wrapper extractors (sudo, env, custom)
- Include command definitions
- Add comments explaining the schema

### Task 8: Add unit tests
- Test config loading (file exists, missing, invalid)
- Test wrapper extraction with Nickel functions
- Test fallback to built-in extractors
- Test null return handling

### Task 9: Add integration tests
- Full flow with custom Nickel wrapper
- Test that built-ins still work when Nickel is loaded
- Test error handling

### Task 10: Update documentation
- Document commands.ncl schema
- Document extract function signature
- Add examples for common wrappers
