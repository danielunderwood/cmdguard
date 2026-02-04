//! Nickel configuration runtime for user-defined wrapper extractors and command definitions.

use crate::command_defs::{CommandDef, FlagDef, FlagType, PositionalDef, ArgType, ParsingOptions, SubcommandDef};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};

/// Result of calling a wrapper extract function
#[derive(Debug, Clone, Deserialize)]
pub struct WrapperExtractResult {
    pub remaining: Vec<String>,
    pub wrapper_name: String,
}

/// Result of validating a Nickel configuration
#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub wrappers: Vec<String>,
    pub commands: Vec<String>,
}

/// Nickel configuration context
pub struct NickelConfig {
    context: Option<nickel_lang::Context>,
    /// The evaluated config expression, kept for querying
    config_source: Option<String>,
    /// Cached wrapper names that have extract functions
    wrapper_names: Vec<String>,
}

impl NickelConfig {
    /// Create new config, loading from file if it exists
    pub fn load(config_dir: &Path) -> Self {
        let ncl_path = config_dir.join("commands.ncl");

        if !ncl_path.exists() {
            debug!(?ncl_path, "No Nickel config file found");
            return Self::empty();
        }

        let content = match std::fs::read_to_string(&ncl_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(?ncl_path, error = ?e, "Failed to read Nickel config");
                return Self::empty();
            }
        };

        let mut context = nickel_lang::Context::new();

        // Evaluate the config to verify it's valid
        match context.eval_deep(&content) {
            Ok(expr) => {
                // Extract wrapper names that have extract functions
                let wrapper_names = Self::extract_wrapper_names(&expr);
                debug!(?wrapper_names, "Loaded Nickel config");

                Self {
                    context: Some(context),
                    config_source: Some(content),
                    wrapper_names,
                }
            }
            Err(e) => {
                warn!(?ncl_path, error = ?e, "Failed to parse Nickel config");
                Self::empty()
            }
        }
    }

    /// Create an empty config (no Nickel functions)
    pub fn empty() -> Self {
        Self {
            context: None,
            config_source: None,
            wrapper_names: vec![],
        }
    }

    /// Check if Nickel config is loaded
    pub fn is_loaded(&self) -> bool {
        self.context.is_some()
    }

    /// Validate a Nickel configuration file and return detailed results
    pub fn validate(config_dir: &Path) -> ValidationResult {
        let ncl_path = config_dir.join("commands.ncl");
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut wrappers = Vec::new();
        let mut commands = Vec::new();

        if !ncl_path.exists() {
            return ValidationResult {
                valid: true,
                errors: vec![],
                warnings: vec!["No commands.ncl file found".to_string()],
                wrappers: vec![],
                commands: vec![],
            };
        }

        let content = match std::fs::read_to_string(&ncl_path) {
            Ok(c) => c,
            Err(e) => {
                return ValidationResult {
                    valid: false,
                    errors: vec![format!("Failed to read file: {}", e)],
                    warnings: vec![],
                    wrappers: vec![],
                    commands: vec![],
                };
            }
        };

        let mut context = nickel_lang::Context::new();

        // Try to evaluate the config
        let expr = match context.eval_deep(&content) {
            Ok(e) => e,
            Err(e) => {
                // Format the error nicely
                let error_msg = format!("{:?}", e);
                // Try to extract the key error info
                if error_msg.contains("UnboundIdentifier") {
                    if let Some(start) = error_msg.find("ident: `") {
                        if let Some(end) = error_msg[start..].find("`,") {
                            let ident = &error_msg[start + 8..start + end];
                            errors.push(format!("Undefined identifier: `{}`", ident));
                            errors.push("Hint: For recursive functions, use `let rec` instead of `let`".to_string());
                        }
                    }
                } else if error_msg.contains("TypecheckError") {
                    errors.push("Type check error in Nickel config".to_string());
                    errors.push(format!("Details: {}", error_msg));
                } else if error_msg.contains("ParseError") {
                    errors.push("Syntax error in Nickel config".to_string());
                    errors.push(format!("Details: {}", error_msg));
                } else {
                    errors.push(format!("Nickel evaluation error: {}", error_msg));
                }

                return ValidationResult {
                    valid: false,
                    errors,
                    warnings,
                    wrappers,
                    commands,
                };
            }
        };

        // Check structure
        let config_record = match expr.as_record() {
            Some(r) => r,
            None => {
                errors.push("Config must be a record (object)".to_string());
                return ValidationResult {
                    valid: false,
                    errors,
                    warnings,
                    wrappers,
                    commands,
                };
            }
        };

        // Validate wrappers section
        if let Some(wrappers_expr) = config_record.value_by_name("wrappers") {
            if let Some(wrappers_record) = wrappers_expr.as_record() {
                for (name, def_opt) in wrappers_record {
                    if let Some(def) = def_opt {
                        if let Some(def_record) = def.as_record() {
                            if def_record.value_by_name("extract").is_some() {
                                wrappers.push(name.to_string());
                            } else {
                                warnings.push(format!(
                                    "Wrapper '{}' has no 'extract' function",
                                    name
                                ));
                            }
                        } else {
                            errors.push(format!(
                                "Wrapper '{}' must be a record with 'extract' function",
                                name
                            ));
                        }
                    }
                }
            } else {
                errors.push("'wrappers' must be a record".to_string());
            }
        }

        // Validate commands section
        if let Some(commands_expr) = config_record.value_by_name("commands") {
            if let Some(commands_record) = commands_expr.as_record() {
                for (name, def_opt) in commands_record {
                    if def_opt.is_some() {
                        commands.push(name.to_string());
                    }
                }
            } else {
                errors.push("'commands' must be a record".to_string());
            }
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
            wrappers,
            commands,
        }
    }

    /// Check if a custom wrapper extractor is defined for the given command
    pub fn has_wrapper(&self, name: &str) -> bool {
        self.wrapper_names.contains(&name.to_string())
    }

    /// Call a wrapper's extract function
    ///
    /// Returns Some(result) if extraction succeeded, None if:
    /// - No Nickel config loaded
    /// - Wrapper not defined
    /// - Extract function returned null
    /// - Any evaluation error
    pub fn extract_wrapper(
        &mut self,
        name: &str,
        tokens: &[String],
    ) -> Option<WrapperExtractResult> {
        if !self.has_wrapper(name) {
            return None;
        }

        let context = self.context.as_mut()?;
        let config_source = self.config_source.as_ref()?;

        // Build expression to call: (config.wrappers.<name>.extract) tokens
        let tokens_json = serde_json::to_string(tokens).ok()?;

        let call_expr = format!(
            "let config = {} in (config.wrappers.{}.extract) {}",
            config_source, name, tokens_json
        );

        debug!(%name, "Calling Nickel wrapper extract");

        match context.eval_deep(&call_expr) {
            Ok(result) => {
                // Check if result is null
                if let Some(record) = result.as_record() {
                    // Try to extract the fields
                    let remaining = record.value_by_name("remaining")?;
                    let wrapper_name = record.value_by_name("wrapper_name")?;

                    let remaining_vec: Vec<String> = remaining.to_serde().ok()?;
                    let wrapper_name_str: String = wrapper_name.to_serde().ok()?;

                    Some(WrapperExtractResult {
                        remaining: remaining_vec,
                        wrapper_name: wrapper_name_str,
                    })
                } else {
                    // null or non-record result
                    None
                }
            }
            Err(e) => {
                warn!(%name, error = ?e, "Nickel extract function failed");
                None
            }
        }
    }

    /// Get command definitions from Nickel config
    ///
    /// Returns a HashMap of command name -> CommandDef for commands defined
    /// in the `commands` section of the Nickel config.
    pub fn get_command_definitions(&mut self) -> HashMap<String, CommandDef> {
        let mut commands = HashMap::new();

        let context = match self.context.as_mut() {
            Some(c) => c,
            None => return commands,
        };

        let config_source = match self.config_source.as_ref() {
            Some(s) => s,
            None => return commands,
        };

        // Evaluate to get the commands section
        let expr_str = format!("let config = {} in config.commands", config_source);
        let commands_expr = match context.eval_deep(&expr_str) {
            Ok(e) => e,
            Err(_) => return commands, // No commands section or error
        };

        let commands_record = match commands_expr.as_record() {
            Some(r) => r,
            None => return commands,
        };

        for (cmd_name, cmd_def_opt) in commands_record {
            if let Some(cmd_def_expr) = cmd_def_opt {
                if let Some(cmd_def) = Self::parse_command_def(&cmd_def_expr) {
                    debug!(%cmd_name, "Loaded command definition from Nickel");
                    commands.insert(cmd_name.to_string(), cmd_def);
                }
            }
        }

        commands
    }

    /// Parse a command definition from a Nickel expression
    fn parse_command_def(expr: &nickel_lang::Expr) -> Option<CommandDef> {
        let record = expr.as_record()?;

        let mut flags = HashMap::new();
        let mut positional = Vec::new();
        let mut subcommands = HashMap::new();

        // Parse flags
        if let Some(flags_expr) = record.value_by_name("flags") {
            if let Some(flags_record) = flags_expr.as_record() {
                for (flag_name, flag_def_opt) in flags_record {
                    if let Some(flag_def_expr) = flag_def_opt {
                        if let Some(flag_def) = Self::parse_flag_def(&flag_def_expr) {
                            flags.insert(flag_name.to_string(), flag_def);
                        }
                    }
                }
            }
        }

        // Parse positional arguments
        if let Some(positional_expr) = record.value_by_name("positional") {
            if let Some(positional_array) = positional_expr.as_array() {
                for (idx, pos_def_expr) in positional_array.into_iter().enumerate() {
                    if let Some(pos_def) = Self::parse_positional_def(&pos_def_expr, idx) {
                        positional.push(pos_def);
                    }
                }
            }
        }

        // Parse subcommands
        if let Some(subcommands_expr) = record.value_by_name("subcommands") {
            if let Some(subcommands_record) = subcommands_expr.as_record() {
                for (subcmd_name, subcmd_def_opt) in subcommands_record {
                    if let Some(subcmd_def_expr) = subcmd_def_opt {
                        if let Some(subcmd_def) = Self::parse_subcommand_def(&subcmd_def_expr) {
                            subcommands.insert(subcmd_name.to_string(), subcmd_def);
                        }
                    }
                }
            }
        }

        // Parse is_wrapper
        let is_wrapper = record
            .value_by_name("is_wrapper")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(false);

        // Parse parsing options
        let parsing = record
            .value_by_name("parsing")
            .and_then(|e| Self::parse_parsing_options(&e))
            .unwrap_or_default();

        Some(CommandDef {
            flags,
            positional,
            subcommands,
            is_wrapper,
            parsing,
        })
    }

    /// Parse a subcommand definition from a Nickel expression
    fn parse_subcommand_def(expr: &nickel_lang::Expr) -> Option<SubcommandDef> {
        let record = expr.as_record()?;

        let mut flags = HashMap::new();
        let mut positional = Vec::new();

        // Parse flags
        if let Some(flags_expr) = record.value_by_name("flags") {
            if let Some(flags_record) = flags_expr.as_record() {
                for (flag_name, flag_def_opt) in flags_record {
                    if let Some(flag_def_expr) = flag_def_opt {
                        if let Some(flag_def) = Self::parse_flag_def(&flag_def_expr) {
                            flags.insert(flag_name.to_string(), flag_def);
                        }
                    }
                }
            }
        }

        // Parse positional arguments
        if let Some(positional_expr) = record.value_by_name("positional") {
            if let Some(positional_array) = positional_expr.as_array() {
                for (idx, pos_def_expr) in positional_array.into_iter().enumerate() {
                    if let Some(pos_def) = Self::parse_positional_def(&pos_def_expr, idx) {
                        positional.push(pos_def);
                    }
                }
            }
        }

        Some(SubcommandDef {
            flags,
            positional,
        })
    }

    /// Parse a flag definition from a Nickel expression
    fn parse_flag_def(expr: &nickel_lang::Expr) -> Option<FlagDef> {
        let record = expr.as_record()?;

        // Parse short forms - can be string or array
        let short = if let Some(short_expr) = record.value_by_name("short") {
            if let Some(arr) = short_expr.as_array() {
                arr.into_iter()
                    .filter_map(|e| e.to_serde::<String>().ok())
                    .collect()
            } else if let Ok(s) = short_expr.to_serde::<String>() {
                vec![s]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Parse long form
        let long = record
            .value_by_name("long")
            .and_then(|e| e.to_serde::<String>().ok());

        // Parse type
        let flag_type_str = record
            .value_by_name("type")
            .and_then(|e| e.to_serde::<String>().ok())
            .unwrap_or_else(|| "boolean".to_string());

        let flag_type = match flag_type_str.as_str() {
            "boolean" => FlagType::Boolean,
            "with_arg" => FlagType::WithArg,
            "with_optional_arg" => FlagType::WithOptionalArg,
            _ => FlagType::Boolean,
        };

        Some(FlagDef {
            short,
            long,
            flag_type,
        })
    }

    /// Parse a positional argument definition from a Nickel expression
    fn parse_positional_def(expr: &nickel_lang::Expr, default_idx: usize) -> Option<PositionalDef> {
        let record = expr.as_record()?;

        let name = record
            .value_by_name("name")
            .and_then(|e| e.to_serde::<String>().ok())
            .unwrap_or_else(|| format!("arg{}", default_idx));

        let arg_type_str = record
            .value_by_name("type")
            .and_then(|e| e.to_serde::<String>().ok())
            .unwrap_or_else(|| "string".to_string());

        let arg_type = match arg_type_str.as_str() {
            "string" => ArgType::String,
            "path" => ArgType::Path,
            "number" => ArgType::Number,
            _ => ArgType::String,
        };

        let position = record
            .value_by_name("position")
            .and_then(|e| e.to_serde::<i32>().ok());

        let variadic = record
            .value_by_name("variadic")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(false);

        let last = record
            .value_by_name("last")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(false);

        let optional = record
            .value_by_name("optional")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(false);

        Some(PositionalDef {
            name,
            arg_type,
            position,
            variadic,
            last,
            optional,
        })
    }

    /// Parse parsing options from a Nickel expression
    fn parse_parsing_options(expr: &nickel_lang::Expr) -> Option<ParsingOptions> {
        let record = expr.as_record()?;

        let combine_short_flags = record
            .value_by_name("combine_short_flags")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(true);

        let double_dash_ends_flags = record
            .value_by_name("double_dash_ends_flags")
            .and_then(|e| e.to_serde::<bool>().ok())
            .unwrap_or(true);

        Some(ParsingOptions {
            combine_short_flags,
            double_dash_ends_flags,
        })
    }

    /// Extract wrapper names from config that have extract functions
    fn extract_wrapper_names(expr: &nickel_lang::Expr) -> Vec<String> {
        let mut names = vec![];

        if let Some(config) = expr.as_record() {
            if let Some(wrappers_expr) = config.value_by_name("wrappers") {
                if let Some(wrappers_record) = wrappers_expr.as_record() {
                    for (name, wrapper_def_opt) in wrappers_record {
                        if let Some(wrapper_def) = wrapper_def_opt {
                            if let Some(def_record) = wrapper_def.as_record() {
                                if def_record.value_by_name("extract").is_some() {
                                    names.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_config(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().to_path_buf();
        let ncl_path = config_path.join("commands.ncl");
        fs::write(&ncl_path, content).unwrap();
        (dir, config_path)
    }

    #[test]
    fn test_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let config = NickelConfig::load(dir.path());
        assert!(!config.is_loaded());
        assert!(config.wrapper_names.is_empty());
    }

    #[test]
    fn test_load_empty_config() {
        let (dir, path) = create_temp_config("{}");
        let config = NickelConfig::load(&path);
        assert!(config.is_loaded());
        assert!(config.wrapper_names.is_empty());
        drop(dir);
    }

    #[test]
    fn test_load_invalid_nickel() {
        let (dir, path) = create_temp_config("{ invalid syntax here");
        let config = NickelConfig::load(&path);
        assert!(!config.is_loaded());
        drop(dir);
    }

    #[test]
    fn test_load_with_wrappers() {
        let content = r#"
{
  wrappers = {
    my_wrapper = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len > 1 then
          { remaining = std.array.slice 1 len tokens, wrapper_name = "my_wrapper" }
        else
          null
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let config = NickelConfig::load(&path);
        assert!(config.is_loaded());
        assert!(config.has_wrapper("my_wrapper"));
        assert!(!config.has_wrapper("other"));
        drop(dir);
    }

    #[test]
    fn test_extract_wrapper_simple() {
        // Note: Nickel uses std.array.slice instead of drop
        let content = r#"
{
  wrappers = {
    test_wrapper = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len > 1 then
          { remaining = std.array.slice 1 len tokens, wrapper_name = "test_wrapper" }
        else
          null
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        let tokens = vec!["test_wrapper".to_string(), "inner".to_string(), "cmd".to_string()];
        let result = config.extract_wrapper("test_wrapper", &tokens);

        assert!(result.is_some(), "Expected Some result, got None");
        let result = result.unwrap();
        assert_eq!(result.wrapper_name, "test_wrapper");
        assert_eq!(result.remaining, vec!["inner", "cmd"]);
        drop(dir);
    }

    #[test]
    fn test_extract_wrapper_returns_null() {
        let content = r#"
{
  wrappers = {
    test_wrapper = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len > 5 then
          { remaining = std.array.slice 1 len tokens, wrapper_name = "test_wrapper" }
        else
          null
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        // Only 2 tokens, function returns null
        let tokens = vec!["test_wrapper".to_string(), "cmd".to_string()];
        let result = config.extract_wrapper("test_wrapper", &tokens);

        assert!(result.is_none());
        drop(dir);
    }

    #[test]
    fn test_extract_wrapper_not_defined() {
        let content = r#"
{
  wrappers = {
    other = {
      extract = fun tokens => null
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        let tokens = vec!["unknown".to_string(), "cmd".to_string()];
        let result = config.extract_wrapper("unknown", &tokens);

        assert!(result.is_none());
        drop(dir);
    }

    #[test]
    fn test_has_wrapper() {
        let content = r#"
{
  wrappers = {
    sudo = {
      extract = fun tokens => null
    },
    env = {
      extract = fun tokens => null
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let config = NickelConfig::load(&path);

        assert!(config.has_wrapper("sudo"));
        assert!(config.has_wrapper("env"));
        assert!(!config.has_wrapper("docker"));
        drop(dir);
    }

    #[test]
    fn test_get_command_definitions() {
        let content = r#"
{
  commands = {
    my_tool = {
      flags = {
        verbose = { short = ["-v"], long = "--verbose", type = "boolean" },
        output = { short = ["-o"], long = "--output", type = "with_arg" },
      },
      positional = [
        { name = "input", type = "path", variadic = false },
        { name = "targets", type = "path", variadic = true },
      ],
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        assert!(commands.contains_key("my_tool"));

        let my_tool = commands.get("my_tool").unwrap();
        assert!(my_tool.flags.contains_key("verbose"));
        assert!(my_tool.flags.contains_key("output"));
        assert_eq!(my_tool.positional.len(), 2);
        assert_eq!(my_tool.positional[0].name, "input");
        assert_eq!(my_tool.positional[1].name, "targets");
        assert!(my_tool.positional[1].variadic);
        drop(dir);
    }

    #[test]
    fn test_get_command_definitions_empty() {
        let content = r#"
{
  wrappers = {}
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        assert!(commands.is_empty());
        drop(dir);
    }

    #[test]
    fn test_get_command_definitions_with_subcommands() {
        let content = r#"
{
  commands = {
    git = {
      subcommands = {
        rev-parse = {
          flags = {
            verify = { long = "--verify", type = "boolean" },
          },
          positional = [
            { name = "revision", type = "string" },
          ],
        },
        stash = {
          flags = {
            push = { long = "--push", type = "boolean" },
          },
          positional = [],
        },
      },
    }
  }
}
"#;
        let (dir, path) = create_temp_config(content);
        let mut config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        assert!(commands.contains_key("git"));

        let git = commands.get("git").unwrap();
        assert!(git.subcommands.contains_key("rev-parse"));
        assert!(git.subcommands.contains_key("stash"));

        let rev_parse = git.subcommands.get("rev-parse").unwrap();
        assert!(rev_parse.flags.contains_key("verify"));
        assert_eq!(rev_parse.positional.len(), 1);
        assert_eq!(rev_parse.positional[0].name, "revision");

        let stash = git.subcommands.get("stash").unwrap();
        assert!(stash.flags.contains_key("push"));
        assert!(stash.positional.is_empty());
        drop(dir);
    }
}
