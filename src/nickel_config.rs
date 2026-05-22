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
    /// Builtins source (builtins.ncl)
    builtins_source: Option<String>,
    /// User config source (commands.ncl)
    user_source: Option<String>,
    /// Cached wrapper names that have extract functions
    wrapper_names: Vec<String>,
}

impl NickelConfig {
    /// Create new config, loading builtins and user config separately
    pub fn load(config_dir: &Path) -> Self {
        // Prefer base/builtins.ncl when present, else fall back to legacy
        // flat layout. Keying off the file (not just the directory) means a
        // partial/empty base/ doesn't shadow a valid legacy config.
        let base_builtins = config_dir.join("base").join("builtins.ncl");
        let builtins_path = if base_builtins.exists() {
            base_builtins
        } else {
            config_dir.join("builtins.ncl")
        };

        let user_path = config_dir.join("commands.ncl");

        // Load and validate builtins
        let builtins_source = Self::load_and_validate(&builtins_path, "builtins.ncl");

        // Load and validate user config
        let user_source = Self::load_and_validate(&user_path, "commands.ncl");

        // If neither file exists, return empty config
        if builtins_source.is_none() && user_source.is_none() {
            return Self::empty();
        }

        // Extract wrapper names from combined config
        let wrapper_names = Self::extract_wrapper_names_from_sources(
            builtins_source.as_deref(),
            user_source.as_deref(),
        );

        Self {
            builtins_source,
            user_source,
            wrapper_names,
        }
    }

    /// Load and validate a single Nickel file
    fn load_and_validate(path: &Path, name: &str) -> Option<String> {
        if !path.exists() {
            debug!(?path, "No {} found", name);
            return None;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(?path, error = ?e, "Failed to read {}", name);
                return None;
            }
        };

        // Validate the Nickel file parses correctly
        let mut context = nickel_lang::Context::new();
        match context.eval_deep(&content) {
            Ok(_) => {
                debug!(?path, "Loaded {}", name);
                Some(content)
            }
            Err(e) => {
                warn!(?path, error = ?e, "Failed to parse {}", name);
                None
            }
        }
    }

    /// Extract wrapper names from user config (wrappers are user-defined only)
    fn extract_wrapper_names_from_sources(
        _builtins_src: Option<&str>,
        user_src: Option<&str>,
    ) -> Vec<String> {
        // Wrappers are only defined in user config, not builtins
        // So we only need to look at user_src
        let source = match user_src {
            Some(s) => s,
            None => return vec![],
        };

        let mut context = nickel_lang::Context::new();
        match context.eval_deep(source) {
            Ok(expr) => {
                let names = Self::extract_wrapper_names(&expr);
                debug!(?names, "Found wrapper names");
                names
            }
            Err(e) => {
                warn!(error = ?e, "Failed to extract wrapper names");
                vec![]
            }
        }
    }

    /// Create an empty config (no Nickel functions)
    pub fn empty() -> Self {
        Self {
            builtins_source: None,
            user_source: None,
            wrapper_names: vec![],
        }
    }

    /// Check if Nickel config is loaded
    #[cfg(test)]
    pub fn is_loaded(&self) -> bool {
        self.builtins_source.is_some() || self.user_source.is_some()
    }

    /// Get command definitions from builtins only
    pub fn get_builtin_command_definitions(&self) -> HashMap<String, CommandDef> {
        self.builtins_source
            .as_ref()
            .map(|s| Self::parse_command_definitions_from_source(s))
            .unwrap_or_default()
    }

    /// Get command definitions from user config only
    pub fn get_user_command_definitions(&self) -> HashMap<String, CommandDef> {
        self.user_source
            .as_ref()
            .map(|s| Self::parse_command_definitions_from_source(s))
            .unwrap_or_default()
    }

    /// Parse command definitions from a Nickel source string
    fn parse_command_definitions_from_source(source: &str) -> HashMap<String, CommandDef> {
        let mut context = nickel_lang::Context::new();
        match context.eval_deep(source) {
            Ok(expr) => Self::parse_commands_from_expr(&expr),
            Err(_) => HashMap::new(),
        }
    }

    /// Parse commands from an evaluated Nickel expression
    fn parse_commands_from_expr(expr: &nickel_lang::Expr) -> HashMap<String, CommandDef> {
        let mut commands = HashMap::new();

        let config_record = match expr.as_record() {
            Some(r) => r,
            None => return commands,
        };

        let commands_expr = match config_record.value_by_name("commands") {
            Some(e) => e,
            None => return commands,
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

        // Get the combined source for wrapper evaluation
        let combined_source = self.get_combined_source()?;

        // Build expression to call: (config.wrappers.<name>.extract) tokens
        let tokens_json = serde_json::to_string(tokens).ok()?;

        let call_expr = format!(
            "let config = {} in (config.wrappers.{}.extract) {}",
            combined_source, name, tokens_json
        );

        debug!(%name, "Calling Nickel wrapper extract");

        let mut context = nickel_lang::Context::new();
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

    /// Get user source for wrapper evaluation (wrappers are user-defined only)
    fn get_combined_source(&self) -> Option<String> {
        // Wrappers are only defined in user config, not builtins
        self.user_source.clone()
    }

    /// Get command definitions from Nickel config
    ///
    /// Returns a HashMap of command name -> CommandDef for commands defined
    /// in the `commands` section of both builtins and user config, with
    /// user definitions deep-merged on top of builtins.
    pub fn get_command_definitions(&self) -> HashMap<String, CommandDef> {
        let mut commands = self.get_builtin_command_definitions();
        let user_commands = self.get_user_command_definitions();

        // Deep merge user commands on top of builtins
        for (cmd_name, user_def) in user_commands {
            if let Some(existing) = commands.get_mut(&cmd_name) {
                // Deep merge: add user flags to existing
                for (flag_name, flag_def) in user_def.flags {
                    existing.flags.insert(flag_name, flag_def);
                }

                // Deep merge subcommands
                for (subcmd_name, subcmd_def) in user_def.subcommands {
                    if let Some(existing_subcmd) = existing.subcommands.get_mut(&subcmd_name) {
                        // Merge subcommand flags
                        for (flag_name, flag_def) in subcmd_def.flags {
                            existing_subcmd.flags.insert(flag_name, flag_def);
                        }
                        // Merge positional if user provides any
                        if !subcmd_def.positional.is_empty() {
                            existing_subcmd.positional = subcmd_def.positional;
                        }
                    } else {
                        // New subcommand from user
                        existing.subcommands.insert(subcmd_name, subcmd_def);
                    }
                }

                // Override positional if user provides any
                if !user_def.positional.is_empty() {
                    existing.positional = user_def.positional;
                }

                // Override is_wrapper if user sets it
                if user_def.is_wrapper {
                    existing.is_wrapper = true;
                }
            } else {
                // New command from user
                debug!(%cmd_name, "Adding user command definition");
                commands.insert(cmd_name, user_def);
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
            "repeatable" => FlagType::Repeatable,
            _ => FlagType::Boolean,
        };

        // Parse claim_pattern for capturing unknown flags (e.g., -NUM syntax)
        let claim_pattern = record
            .value_by_name("claim_pattern")
            .and_then(|e| e.to_serde::<String>().ok());

        Some(FlagDef {
            short,
            long,
            flag_type,
            claim_pattern,
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

    fn create_temp_config_with_builtins(
        builtins: &str,
        user: &str,
    ) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().to_path_buf();
        fs::write(config_path.join("builtins.ncl"), builtins).unwrap();
        fs::write(config_path.join("commands.ncl"), user).unwrap();
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
        let config = NickelConfig::load(&path);

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
        let config = NickelConfig::load(&path);

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
        let config = NickelConfig::load(&path);

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

    // ==========================================================================
    // Tests for separate loading and Rust merge behavior
    // ==========================================================================

    #[test]
    fn test_load_builtins_only() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().to_path_buf();

        // Only create builtins.ncl, no commands.ncl
        let builtins = r#"
{
  commands = {
    git = {
      flags = {
        directory = { short = ["-C"], type = "with_arg" },
      },
      subcommands = {
        push = {
          flags = {
            force = { short = ["-f"], long = "--force", type = "boolean" },
          },
        },
      },
    },
  },
}
"#;
        fs::write(config_path.join("builtins.ncl"), builtins).unwrap();

        let config = NickelConfig::load(&config_path);
        assert!(config.is_loaded());

        let commands = config.get_command_definitions();
        assert!(commands.contains_key("git"));

        let git = commands.get("git").unwrap();
        assert!(git.flags.contains_key("directory"));
        assert!(git.subcommands.contains_key("push"));
        assert!(git.subcommands.get("push").unwrap().flags.contains_key("force"));

        drop(dir);
    }

    #[test]
    fn test_deep_merge_user_adds_subcommand_preserves_builtin_flags() {
        // Builtins define git with push.force flag
        let builtins = r#"
{
  commands = {
    git = {
      flags = {
        directory = { short = ["-C"], type = "with_arg" },
      },
      subcommands = {
        push = {
          flags = {
            force = { short = ["-f"], long = "--force", type = "boolean" },
            delete = { short = ["-d"], long = "--delete", type = "boolean" },
          },
        },
      },
    },
  },
}
"#;
        // User adds new subcommand without touching push
        let user = r#"
{
  commands = {
    git = {
      subcommands = {
        town = {
          positional = [{ name = "subcmd", type = "string" }],
        },
      },
    },
  },
}
"#;
        let (dir, path) = create_temp_config_with_builtins(builtins, user);
        let config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        let git = commands.get("git").unwrap();

        // Builtin flags should still exist
        assert!(git.flags.contains_key("directory"), "git -C flag should exist");

        // Builtin subcommand push with flags should still exist
        assert!(git.subcommands.contains_key("push"), "push subcommand should exist");
        let push = git.subcommands.get("push").unwrap();
        assert!(push.flags.contains_key("force"), "push --force flag should exist");
        assert!(push.flags.contains_key("delete"), "push --delete flag should exist");

        // User subcommand should also exist
        assert!(git.subcommands.contains_key("town"), "town subcommand should exist");
        let town = git.subcommands.get("town").unwrap();
        assert_eq!(town.positional.len(), 1);
        assert_eq!(town.positional[0].name, "subcmd");

        drop(dir);
    }

    #[test]
    fn test_deep_merge_user_adds_flags_to_existing_subcommand() {
        // Builtins define git push with force flag
        let builtins = r#"
{
  commands = {
    git = {
      subcommands = {
        push = {
          flags = {
            force = { short = ["-f"], long = "--force", type = "boolean" },
          },
        },
      },
    },
  },
}
"#;
        // User adds more flags to push subcommand
        let user = r#"
{
  commands = {
    git = {
      subcommands = {
        push = {
          flags = {
            set_upstream = { short = ["-u"], long = "--set-upstream", type = "boolean" },
          },
        },
      },
    },
  },
}
"#;
        let (dir, path) = create_temp_config_with_builtins(builtins, user);
        let config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        let git = commands.get("git").unwrap();
        let push = git.subcommands.get("push").unwrap();

        // Both builtin and user flags should exist
        assert!(push.flags.contains_key("force"), "builtin force flag should exist");
        assert!(push.flags.contains_key("set_upstream"), "user set_upstream flag should exist");

        drop(dir);
    }

    #[test]
    fn test_deep_merge_user_overrides_builtin_flag() {
        // Builtins define rm with force flag using -f
        let builtins = r#"
{
  commands = {
    rm = {
      flags = {
        force = { short = ["-f"], long = "--force", type = "boolean" },
        recursive = { short = ["-r"], long = "--recursive", type = "boolean" },
      },
    },
  },
}
"#;
        // User overrides force flag (maybe to change the short form)
        let user = r#"
{
  commands = {
    rm = {
      flags = {
        force = { short = ["-f", "-F"], long = "--force", type = "boolean" },
      },
    },
  },
}
"#;
        let (dir, path) = create_temp_config_with_builtins(builtins, user);
        let config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();
        let rm = commands.get("rm").unwrap();

        // User's force flag should override builtin
        let force = rm.flags.get("force").unwrap();
        assert_eq!(force.short.len(), 2, "should have both -f and -F");
        assert!(force.short.contains(&"-f".to_string()));
        assert!(force.short.contains(&"-F".to_string()));

        // Builtin recursive flag should still exist
        assert!(rm.flags.contains_key("recursive"), "recursive flag should exist");

        drop(dir);
    }

    #[test]
    fn test_deep_merge_user_adds_new_command() {
        // Builtins define git
        let builtins = r#"
{
  commands = {
    git = {
      subcommands = {
        status = {},
      },
    },
  },
}
"#;
        // User adds completely new command
        let user = r#"
{
  commands = {
    my_tool = {
      flags = {
        verbose = { short = ["-v"], long = "--verbose", type = "boolean" },
      },
    },
  },
}
"#;
        let (dir, path) = create_temp_config_with_builtins(builtins, user);
        let config = NickelConfig::load(&path);

        let commands = config.get_command_definitions();

        // Both commands should exist
        assert!(commands.contains_key("git"), "builtin git should exist");
        assert!(commands.contains_key("my_tool"), "user my_tool should exist");

        let my_tool = commands.get("my_tool").unwrap();
        assert!(my_tool.flags.contains_key("verbose"));

        drop(dir);
    }

    #[test]
    fn test_get_builtin_and_user_definitions_separately() {
        let builtins = r#"
{
  commands = {
    git = {
      subcommands = { push = {} },
    },
  },
}
"#;
        let user = r#"
{
  commands = {
    my_tool = {
      flags = { verbose = { type = "boolean" } },
    },
  },
}
"#;
        let (dir, path) = create_temp_config_with_builtins(builtins, user);
        let config = NickelConfig::load(&path);

        // Get definitions separately
        let builtin_defs = config.get_builtin_command_definitions();
        let user_defs = config.get_user_command_definitions();

        // Builtins should only have git
        assert!(builtin_defs.contains_key("git"));
        assert!(!builtin_defs.contains_key("my_tool"));

        // User should only have my_tool
        assert!(user_defs.contains_key("my_tool"));
        assert!(!user_defs.contains_key("git"));

        // Combined should have both
        let combined = config.get_command_definitions();
        assert!(combined.contains_key("git"));
        assert!(combined.contains_key("my_tool"));

        drop(dir);
    }
}
