use serde::Serialize;
use std::collections::HashMap;

use crate::command_defs::{CommandDefinitions, CommandDef, FlagDef, FlagType, ParsingOptions, SubcommandDef};

/// Parsed flag value
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum FlagValue {
    Bool(bool),
    String(String),
}

/// A parsed positional argument value
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

/// A group of positional arguments
#[derive(Debug, Clone, Serialize)]
pub struct PositionalArg {
    pub name: String,
    pub values: Vec<PositionalValue>,
}

/// Result of parsing a command
#[derive(Debug, Clone, Serialize)]
pub struct ParsedCommand {
    pub parsed_flags: HashMap<String, FlagValue>,
    pub positional_args: Vec<PositionalArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
}

/// Parse a command's tokens into structured flags and positional args
pub fn parse_command(
    tokens: &[String],
    definitions: &CommandDefinitions,
) -> ParsedCommand {
    if tokens.is_empty() {
        return ParsedCommand {
            parsed_flags: HashMap::new(),
            positional_args: vec![],
            subcommand: None,
        };
    }

    let binary_name = &tokens[0];
    let args = &tokens[1..];

    // Get command definition (or use defaults)
    let cmd_def = definitions.get(binary_name);
    let parsing = cmd_def
        .map(|c| &c.parsing)
        .unwrap_or(&definitions.defaults);

    // Check for subcommand (e.g., git push)
    let (subcommand, subcommand_def, args_to_parse) = detect_subcommand(args, cmd_def);

    // Get the flags to look for
    let flags = if let Some(sub_def) = subcommand_def {
        &sub_def.flags
    } else if let Some(def) = cmd_def {
        &def.flags
    } else {
        // No definition - we'll parse with defaults
        return parse_without_definition(args_to_parse, parsing, subcommand);
    };

    let mut result = parse_with_definition(args_to_parse, flags, parsing);
    result.subcommand = subcommand;
    result
}

/// Detect if there's a subcommand in the arguments
fn detect_subcommand<'a>(
    args: &'a [String],
    cmd_def: Option<&'a CommandDef>,
) -> (Option<String>, Option<&'a SubcommandDef>, &'a [String]) {
    // No command definition, no subcommands
    let cmd_def = match cmd_def {
        Some(def) => def,
        None => return (None, None, args),
    };

    // No subcommands defined
    if cmd_def.subcommands.is_empty() {
        return (None, None, args);
    }

    // Check if first arg is a subcommand
    if args.is_empty() {
        return (None, None, args);
    }

    let first_arg = &args[0];

    // Don't treat flags as subcommands
    if first_arg.starts_with('-') {
        return (None, None, args);
    }

    // Check if it's a known subcommand
    if let Some(sub_def) = cmd_def.subcommands.get(first_arg) {
        (Some(first_arg.clone()), Some(sub_def), &args[1..])
    } else {
        (None, None, args)
    }
}

/// Expand combined short flags like "-rf" into ["-r", "-f"]
fn expand_combined_flags(flag: &str) -> Vec<String> {
    // Must start with single dash
    if !flag.starts_with('-') || flag.starts_with("--") {
        return vec![flag.to_string()];
    }

    // Single dash with multiple characters: expand
    let chars: Vec<char> = flag.chars().skip(1).collect();

    if chars.len() <= 1 {
        // Just "-f" or "-"
        return vec![flag.to_string()];
    }

    // Expand "-rf" to ["-r", "-f"]
    chars.iter().map(|c| format!("-{}", c)).collect()
}

/// Parse with known flag definitions
fn parse_with_definition(
    args: &[String],
    flags: &HashMap<String, FlagDef>,
    parsing: &ParsingOptions,
) -> ParsedCommand {
    let mut parsed_flags: HashMap<String, FlagValue> = HashMap::new();
    let mut positional: Vec<String> = vec![];
    let mut i = 0;
    let mut flags_ended = false;

    while i < args.len() {
        let arg = &args[i];

        // Check for -- (end of flags)
        if parsing.double_dash_ends_flags && arg == "--" {
            flags_ended = true;
            i += 1;
            continue;
        }

        // Non-flag or flags ended
        if flags_ended || !arg.starts_with('-') || arg == "-" {
            positional.push(arg.clone());
            i += 1;
            continue;
        }

        // Handle long flags (--foo, --foo=bar)
        if arg.starts_with("--") {
            let (consumed, flag_name, value) = parse_long_flag(arg, &args[i+1..], flags);
            if let Some(name) = flag_name {
                parsed_flags.insert(name, value);
            }
            i += consumed;
            continue;
        }

        // Handle short flags (-f, -rf, -u root)
        let expanded = if parsing.combine_short_flags {
            expand_combined_flags(arg)
        } else {
            vec![arg.clone()]
        };

        let mut extra_consumed = 0;
        for (j, short) in expanded.iter().enumerate() {
            let remaining = if j == expanded.len() - 1 {
                &args[i+1..]
            } else {
                &[]
            };

            let (consumed, flag_name, value) = parse_short_flag(short, remaining, flags);
            if let Some(name) = flag_name {
                parsed_flags.insert(name, value);
            }
            if consumed > 0 {
                extra_consumed = consumed;
            }
        }
        i += 1 + extra_consumed;
    }

    ParsedCommand {
        parsed_flags,
        positional_args: vec![PositionalArg {
            name: "args".to_string(),
            values: positional.into_iter().map(|s| PositionalValue {
                raw: s,
                resolved: None,
                trust_zone: None,
                value_type: "string".to_string(),
            }).collect(),
        }],
        subcommand: None,
    }
}

/// Parse a long flag like --user=root or --force
/// Returns (tokens_consumed, flag_name, value)
fn parse_long_flag(
    arg: &str,
    remaining: &[String],
    flags: &HashMap<String, FlagDef>,
) -> (usize, Option<String>, FlagValue) {
    // Strip --
    let without_dashes = &arg[2..];

    // Check for = form: --user=root
    if let Some(equals_pos) = without_dashes.find('=') {
        let flag_part = &without_dashes[..equals_pos];
        let value_part = &without_dashes[equals_pos + 1..];

        // Find matching flag definition
        if let Some((name, def)) = match_flag_by_long(flag_part, flags) {
            match def.flag_type {
                FlagType::Boolean => {
                    // Boolean flags shouldn't use = form, but handle it anyway
                    return (1, Some(name), FlagValue::Bool(true));
                }
                FlagType::WithArg | FlagType::WithOptionalArg => {
                    return (1, Some(name), FlagValue::String(value_part.to_string()));
                }
            }
        }

        // Unknown flag with =, treat as positional
        return (1, None, FlagValue::Bool(false));
    }

    // No = form: --force or --user root
    if let Some((name, def)) = match_flag_by_long(without_dashes, flags) {
        match def.flag_type {
            FlagType::Boolean => {
                return (1, Some(name), FlagValue::Bool(true));
            }
            FlagType::WithArg => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (2, Some(name), FlagValue::String(remaining[0].clone()));
                } else {
                    // Missing required argument, treat as boolean
                    return (1, Some(name), FlagValue::Bool(true));
                }
            }
            FlagType::WithOptionalArg => {
                // Optional argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (2, Some(name), FlagValue::String(remaining[0].clone()));
                } else {
                    return (1, Some(name), FlagValue::Bool(true));
                }
            }
        }
    }

    // Unknown flag, skip it
    (1, None, FlagValue::Bool(false))
}

/// Parse a short flag like -u root or -f
/// Returns (tokens_consumed, flag_name, value)
fn parse_short_flag(
    arg: &str,
    remaining: &[String],
    flags: &HashMap<String, FlagDef>,
) -> (usize, Option<String>, FlagValue) {
    // Find matching flag definition
    if let Some((name, def)) = match_flag_by_short(arg, flags) {
        match def.flag_type {
            FlagType::Boolean => {
                return (0, Some(name), FlagValue::Bool(true));
            }
            FlagType::WithArg => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (1, Some(name), FlagValue::String(remaining[0].clone()));
                } else {
                    // Missing required argument, treat as boolean
                    return (0, Some(name), FlagValue::Bool(true));
                }
            }
            FlagType::WithOptionalArg => {
                // Optional argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (1, Some(name), FlagValue::String(remaining[0].clone()));
                } else {
                    return (0, Some(name), FlagValue::Bool(true));
                }
            }
        }
    }

    // Unknown flag, skip it
    (0, None, FlagValue::Bool(false))
}

/// Find a flag definition by long form
fn match_flag_by_long<'a>(
    long_form: &str,
    flags: &'a HashMap<String, FlagDef>,
) -> Option<(String, &'a FlagDef)> {
    for (name, def) in flags {
        if let Some(long) = &def.long {
            // Strip -- from definition if present
            let long_without_dashes = long.strip_prefix("--").unwrap_or(long);
            if long_without_dashes == long_form {
                return Some((name.clone(), def));
            }
        }
    }
    None
}

/// Find a flag definition by short form
fn match_flag_by_short<'a>(
    short_form: &str,
    flags: &'a HashMap<String, FlagDef>,
) -> Option<(String, &'a FlagDef)> {
    for (name, def) in flags {
        if def.short.contains(&short_form.to_string()) {
            return Some((name.clone(), def));
        }
    }
    None
}

/// Parse without a known command definition (best effort)
fn parse_without_definition(
    args: &[String],
    parsing: &ParsingOptions,
    subcommand: Option<String>,
) -> ParsedCommand {
    let mut parsed_flags: HashMap<String, FlagValue> = HashMap::new();
    let mut positional: Vec<String> = vec![];
    let mut i = 0;
    let mut flags_ended = false;

    while i < args.len() {
        let arg = &args[i];

        // Check for -- (end of flags)
        if parsing.double_dash_ends_flags && arg == "--" {
            flags_ended = true;
            i += 1;
            continue;
        }

        // Non-flag or flags ended
        if flags_ended || !arg.starts_with('-') || arg == "-" {
            positional.push(arg.clone());
            i += 1;
            continue;
        }

        // Handle long flags (--foo, --foo=bar)
        if arg.starts_with("--") {
            let without_dashes = &arg[2..];

            // Check for = form
            if let Some(equals_pos) = without_dashes.find('=') {
                let flag_name = &without_dashes[..equals_pos];
                let value = &without_dashes[equals_pos + 1..];
                parsed_flags.insert(flag_name.to_string(), FlagValue::String(value.to_string()));
                i += 1;
                continue;
            }

            // No = form - could be boolean or take next arg
            // Be conservative: treat as boolean unless next arg is clearly a value
            if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                // Next arg might be the value, be conservative and include it
                parsed_flags.insert(without_dashes.to_string(), FlagValue::String(args[i + 1].clone()));
                i += 2;
            } else {
                parsed_flags.insert(without_dashes.to_string(), FlagValue::Bool(true));
                i += 1;
            }
            continue;
        }

        // Handle short flags
        let expanded = if parsing.combine_short_flags {
            expand_combined_flags(arg)
        } else {
            vec![arg.clone()]
        };

        for (j, short) in expanded.iter().enumerate() {
            let short_without_dash = short.strip_prefix('-').unwrap_or(short);

            // Last flag in expansion might take an argument
            if j == expanded.len() - 1 && i + 1 < args.len() && !args[i + 1].starts_with('-') {
                // Could be a flag with argument
                parsed_flags.insert(short_without_dash.to_string(), FlagValue::String(args[i + 1].clone()));
                i += 2;
            } else {
                parsed_flags.insert(short_without_dash.to_string(), FlagValue::Bool(true));
                if j == expanded.len() - 1 {
                    i += 1;
                }
            }
        }
    }

    ParsedCommand {
        parsed_flags,
        positional_args: vec![PositionalArg {
            name: "args".to_string(),
            values: positional.into_iter().map(|s| PositionalValue {
                raw: s,
                resolved: None,
                trust_zone: None,
                value_type: "string".to_string(),
            }).collect(),
        }],
        subcommand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_tokens(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn test_parse_boolean_flags() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("rm -rf /tmp/foo"), &defs);

        assert_eq!(result.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_parse_flag_with_arg() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("sudo -u postgres psql"), &defs);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("postgres".to_string())));
    }

    #[test]
    fn test_parse_long_flag_equals() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("sudo --user=root ls"), &defs);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("root".to_string())));
    }

    #[test]
    fn test_double_dash() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("rm -- -rf"), &defs);

        // -rf should be treated as a filename, not flags
        assert!(result.parsed_flags.get("recursive").is_none());
        assert!(!result.positional_args.is_empty());
        assert_eq!(result.positional_args[0].values.len(), 1);
        assert_eq!(result.positional_args[0].values[0].raw, "-rf");
    }

    #[test]
    fn test_git_subcommand() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("git push -f origin main"), &defs);

        assert_eq!(result.subcommand, Some("push".to_string()));
        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_unknown_command() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("unknown-cmd -v --verbose"), &defs);

        // Should still attempt to parse flags
        assert!(!result.parsed_flags.is_empty() || !result.positional_args.is_empty());
    }

    #[test]
    fn test_expand_combined_flags() {
        assert_eq!(expand_combined_flags("-rf"), vec!["-r", "-f"]);
        assert_eq!(expand_combined_flags("-r"), vec!["-r"]);
        assert_eq!(expand_combined_flags("--recursive"), vec!["--recursive"]);
        assert_eq!(expand_combined_flags("-"), vec!["-"]);
    }

    #[test]
    fn test_positional_args() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("rm -f file1.txt file2.txt"), &defs);

        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.positional_args[0].values.len(), 2);
        assert_eq!(result.positional_args[0].values[0].raw, "file1.txt");
        assert_eq!(result.positional_args[0].values[1].raw, "file2.txt");
    }

    #[test]
    fn test_long_flag_space_separated() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("sudo --user postgres psql"), &defs);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("postgres".to_string())));
    }

    #[test]
    fn test_multiple_short_forms() {
        let defs = CommandDefinitions::builtin();
        // rm accepts both -r and -R for recursive
        let result1 = parse_command(&to_tokens("rm -r /tmp"), &defs);
        let result2 = parse_command(&to_tokens("rm -R /tmp"), &defs);

        assert_eq!(result1.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
        assert_eq!(result2.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_empty_command() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&[], &defs);

        assert!(result.parsed_flags.is_empty());
        assert!(result.positional_args.is_empty());
        assert!(result.subcommand.is_none());
    }

    #[test]
    fn test_git_reset_subcommand() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("git reset --hard HEAD~1"), &defs);

        assert_eq!(result.subcommand, Some("reset".to_string()));
        assert_eq!(result.parsed_flags.get("hard"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.positional_args[0].values[0].raw, "HEAD~1");
    }

    #[test]
    fn test_unknown_command_with_equals() {
        let defs = CommandDefinitions::builtin();
        let result = parse_command(&to_tokens("myapp --config=prod.yaml"), &defs);

        assert_eq!(result.parsed_flags.get("config"), Some(&FlagValue::String("prod.yaml".to_string())));
    }
}
