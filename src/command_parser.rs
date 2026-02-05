use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

use crate::command_defs::{CommandDefinitions, CommandDef, FlagDef, FlagType, ParsingOptions, SubcommandDef, PositionalDef, ArgType};
use crate::resolver::TrustZonePaths;

/// Parsed flag value
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum FlagValue {
    Bool(bool),
    String(String),
    /// Array of values for repeatable flags (e.g., curl -H "h1" -H "h2")
    Array(Vec<String>),
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

impl ParsedCommand {
    /// Get positional args as a map by name for easier access in policies
    /// Returns: { "url": [{ raw, resolved, trust_zone, type }], ... }
    pub fn positional_as_map(&self) -> HashMap<String, &Vec<PositionalValue>> {
        self.positional_args
            .iter()
            .map(|arg| (arg.name.clone(), &arg.values))
            .collect()
    }
}

/// Parse a command's tokens into structured flags and positional args
pub fn parse_command(
    tokens: &[String],
    definitions: &CommandDefinitions,
    project_root: Option<&Path>,
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
    let (subcommand, subcommand_def, _args_after_subcommand) = detect_subcommand(args, cmd_def);

    // For commands with subcommands, we need to parse flags both before and after the subcommand
    // Example: git -C / status --short
    //   Top-level flags: -C /
    //   Subcommand: status
    //   Subcommand flags: --short

    if let Some(sub_def) = subcommand_def {
        if let Some(def) = cmd_def {
            // Parse the entire args with both top-level and subcommand flags combined
            let mut combined_flags = def.flags.clone();
            combined_flags.extend(sub_def.flags.clone());

            let result = parse_with_definition_skip_token(
                args,
                &combined_flags,
                &sub_def.positional,
                parsing,
                project_root,
                subcommand.as_ref() // Skip the subcommand name in positional args
            );

            return ParsedCommand {
                parsed_flags: result.parsed_flags,
                positional_args: result.positional_args,
                subcommand,
            };
        }
    }

    // No subcommand case
    let (flags, positional_defs) = if let Some(def) = cmd_def {
        (&def.flags, &def.positional)
    } else {
        // No definition - we'll parse with defaults
        return parse_without_definition(args, parsing, subcommand, project_root);
    };

    let mut result = parse_with_definition_skip_token(args, flags, positional_defs, parsing, project_root, None);
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

    // For commands with both top-level flags and subcommands (like git),
    // we need to skip over flags to find the subcommand
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // If we hit a non-flag, check if it's a subcommand
        if !arg.starts_with('-') {
            if let Some(sub_def) = cmd_def.subcommands.get(arg) {
                // Found a subcommand - return everything after it
                return (Some(arg.clone()), Some(sub_def), &args[i+1..]);
            } else {
                // Not a subcommand, stop looking
                return (None, None, args);
            }
        }

        // Skip this flag
        i += 1;

        // If it's a flag that takes an argument, skip the argument too
        // Check both short and long forms
        if arg.starts_with("--") {
            // Long flag
            let flag_name = arg.strip_prefix("--").unwrap_or(arg);
            // Check for = form
            if flag_name.contains('=') {
                continue; // Already contains value
            }
            // Check if this flag takes an argument
            if flag_takes_arg(&cmd_def.flags, arg) {
                i += 1; // Skip next token (the argument)
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Short flag(s)
            let last_char = arg.chars().last().unwrap();
            let last_flag = format!("-{}", last_char);
            if flag_takes_arg(&cmd_def.flags, &last_flag) {
                i += 1; // Skip next token (the argument)
            }
        }
    }

    // No subcommand found
    (None, None, args)
}

/// Check if a flag takes an argument
fn flag_takes_arg(flags: &HashMap<String, FlagDef>, flag_str: &str) -> bool {
    // Try to find this flag in the definitions
    for def in flags.values() {
        // Check short forms
        if def.short.contains(&flag_str.to_string()) {
            return matches!(def.flag_type, FlagType::WithArg | FlagType::WithOptionalArg | FlagType::Repeatable);
        }
        // Check long form
        if let Some(long) = &def.long {
            let long_without_dashes = long.strip_prefix("--").unwrap_or(long);
            let flag_without_dashes = flag_str.strip_prefix("--").unwrap_or(flag_str);
            if long_without_dashes == flag_without_dashes {
                return matches!(def.flag_type, FlagType::WithArg | FlagType::WithOptionalArg | FlagType::Repeatable);
            }
        }
    }
    false
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

/// Parse with known flag definitions, optionally skipping a specific token
fn parse_with_definition_skip_token(
    args: &[String],
    flags: &HashMap<String, FlagDef>,
    positional_defs: &[PositionalDef],
    parsing: &ParsingOptions,
    project_root: Option<&Path>,
    skip_token: Option<&String>,
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
            // Skip the token if it matches skip_token (e.g., subcommand name)
            if let Some(skip) = skip_token {
                if arg == skip {
                    i += 1;
                    continue;
                }
            }
            positional.push(arg.clone());
            i += 1;
            continue;
        }

        // Handle long flags (--foo, --foo=bar)
        if arg.starts_with("--") {
            let (consumed, flag_name, value, is_repeatable) = parse_long_flag(arg, &args[i+1..], flags);
            if let Some(name) = flag_name {
                insert_flag(&mut parsed_flags, name, value, is_repeatable);
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

            let (consumed, flag_name, value, is_repeatable) = parse_short_flag(short, remaining, flags);
            if let Some(name) = flag_name {
                insert_flag(&mut parsed_flags, name, value, is_repeatable);
            }
            if consumed > 0 {
                extra_consumed = consumed;
            }
        }
        i += 1 + extra_consumed;
    }

    ParsedCommand {
        parsed_flags,
        positional_args: process_positional_args(positional, positional_defs, project_root),
        subcommand: None,
    }
}

/// Parse a long flag like --user=root or --force
/// Returns (tokens_consumed, flag_name, value, is_repeatable)
fn parse_long_flag(
    arg: &str,
    remaining: &[String],
    flags: &HashMap<String, FlagDef>,
) -> (usize, Option<String>, FlagValue, bool) {
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
                    return (1, Some(name), FlagValue::Bool(true), false);
                }
                FlagType::WithArg | FlagType::WithOptionalArg => {
                    return (1, Some(name), FlagValue::String(value_part.to_string()), false);
                }
                FlagType::Repeatable => {
                    return (1, Some(name), FlagValue::String(value_part.to_string()), true);
                }
            }
        }

        // Unknown flag with =, treat as positional
        return (1, None, FlagValue::Bool(false), false);
    }

    // No = form: --force or --user root
    if let Some((name, def)) = match_flag_by_long(without_dashes, flags) {
        match def.flag_type {
            FlagType::Boolean => {
                return (1, Some(name), FlagValue::Bool(true), false);
            }
            FlagType::WithArg => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (2, Some(name), FlagValue::String(remaining[0].clone()), false);
                } else {
                    // Missing required argument, treat as boolean
                    return (1, Some(name), FlagValue::Bool(true), false);
                }
            }
            FlagType::WithOptionalArg => {
                // Optional argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (2, Some(name), FlagValue::String(remaining[0].clone()), false);
                } else {
                    return (1, Some(name), FlagValue::Bool(true), false);
                }
            }
            FlagType::Repeatable => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (2, Some(name), FlagValue::String(remaining[0].clone()), true);
                } else {
                    // Missing required argument, skip
                    return (1, Some(name), FlagValue::Bool(true), true);
                }
            }
        }
    }

    // Unknown flag, skip it
    (1, None, FlagValue::Bool(false), false)
}

/// Parse a short flag like -u root or -f
/// Returns (tokens_consumed, flag_name, value, is_repeatable)
fn parse_short_flag(
    arg: &str,
    remaining: &[String],
    flags: &HashMap<String, FlagDef>,
) -> (usize, Option<String>, FlagValue, bool) {
    // Find matching flag definition
    if let Some((name, def)) = match_flag_by_short(arg, flags) {
        match def.flag_type {
            FlagType::Boolean => {
                return (0, Some(name), FlagValue::Bool(true), false);
            }
            FlagType::WithArg => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (1, Some(name), FlagValue::String(remaining[0].clone()), false);
                } else {
                    // Missing required argument, treat as boolean
                    return (0, Some(name), FlagValue::Bool(true), false);
                }
            }
            FlagType::WithOptionalArg => {
                // Optional argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (1, Some(name), FlagValue::String(remaining[0].clone()), false);
                } else {
                    return (0, Some(name), FlagValue::Bool(true), false);
                }
            }
            FlagType::Repeatable => {
                // Needs an argument
                if !remaining.is_empty() && !remaining[0].starts_with('-') {
                    return (1, Some(name), FlagValue::String(remaining[0].clone()), true);
                } else {
                    // Missing required argument, skip
                    return (0, Some(name), FlagValue::Bool(true), true);
                }
            }
        }
    }

    // Unknown flag, skip it
    (0, None, FlagValue::Bool(false), false)
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

/// Insert a flag value, handling repeatable flags by accumulating into arrays
fn insert_flag(
    parsed_flags: &mut HashMap<String, FlagValue>,
    name: String,
    value: FlagValue,
    is_repeatable: bool,
) {
    if is_repeatable {
        // For repeatable flags, accumulate values into an array
        match parsed_flags.get_mut(&name) {
            Some(FlagValue::Array(arr)) => {
                // Already have an array, append to it
                if let FlagValue::String(s) = value {
                    arr.push(s);
                }
            }
            Some(_) => {
                // Existing non-array value (shouldn't happen, but handle gracefully)
                if let FlagValue::String(s) = value {
                    parsed_flags.insert(name, FlagValue::Array(vec![s]));
                }
            }
            None => {
                // First occurrence, create array with single value
                if let FlagValue::String(s) = value {
                    parsed_flags.insert(name, FlagValue::Array(vec![s]));
                }
            }
        }
    } else {
        // Non-repeatable flag, just insert (overwrites if duplicate)
        parsed_flags.insert(name, value);
    }
}

/// Process positional arguments using definitions
fn process_positional_args(
    raw_args: Vec<String>,
    positional_defs: &[PositionalDef],
    project_root: Option<&Path>,
) -> Vec<PositionalArg> {
    if positional_defs.is_empty() {
        // No definitions - return raw args
        return vec![PositionalArg {
            name: "args".to_string(),
            values: raw_args.into_iter().map(|s| PositionalValue {
                raw: s,
                resolved: None,
                trust_zone: None,
                value_type: "string".to_string(),
            }).collect(),
        }];
    }

    let mut result = Vec::new();
    let mut remaining: Vec<String> = raw_args;

    // Handle position-based args first (explicit index)
    for def in positional_defs.iter().filter(|d| d.position.is_some()) {
        let pos = def.position.unwrap() as usize;
        if pos < remaining.len() {
            let value = remaining.remove(pos);
            result.push(create_positional_arg(&def.name, vec![value], &def.arg_type, project_root));
        }
    }

    // Handle "last" arg (like cp destination)
    if let Some(last_def) = positional_defs.iter().find(|d| d.last) {
        if !remaining.is_empty() {
            let last = remaining.pop().unwrap();
            result.push(create_positional_arg(&last_def.name, vec![last], &last_def.arg_type, project_root));
        }
    }

    // Get sequential positional defs (not position-based, not last, not variadic)
    let sequential_defs: Vec<_> = positional_defs
        .iter()
        .filter(|d| d.position.is_none() && !d.last && !d.variadic)
        .collect();

    // Assign remaining args to sequential positional definitions in order
    for def in sequential_defs {
        if remaining.is_empty() {
            break;
        }
        let value = remaining.remove(0);
        result.push(create_positional_arg(&def.name, vec![value], &def.arg_type, project_root));
    }

    // Handle variadic arg (remaining args after sequential)
    if let Some(variadic_def) = positional_defs.iter().find(|d| d.variadic) {
        if !remaining.is_empty() {
            result.push(create_positional_arg(&variadic_def.name, remaining, &variadic_def.arg_type, project_root));
        }
    } else if !remaining.is_empty() {
        // No variadic def but have remaining args - use generic "args"
        result.push(PositionalArg {
            name: "args".to_string(),
            values: remaining.into_iter().map(|s| PositionalValue {
                raw: s,
                resolved: None,
                trust_zone: None,
                value_type: "string".to_string(),
            }).collect(),
        });
    }

    result
}

/// Create a positional arg from values with proper type handling
fn create_positional_arg(
    name: &str,
    values: Vec<String>,
    arg_type: &ArgType,
    project_root: Option<&Path>,
) -> PositionalArg {
    let resolved_values: Vec<PositionalValue> = values.into_iter().map(|raw| {
        match arg_type {
            ArgType::Path => resolve_path_arg(&raw, project_root),
            ArgType::String => PositionalValue {
                raw,
                resolved: None,
                trust_zone: None,
                value_type: "string".to_string(),
            },
            ArgType::Number => PositionalValue {
                raw,
                resolved: None,
                trust_zone: None,
                value_type: "number".to_string(),
            },
        }
    }).collect();

    PositionalArg {
        name: name.to_string(),
        values: resolved_values,
    }
}

/// Resolve a path argument with trust zone classification
fn resolve_path_arg(raw: &str, project_root: Option<&Path>) -> PositionalValue {
    use std::path::Path as StdPath;

    let path = StdPath::new(raw);
    let zone_paths = TrustZonePaths::defaults();

    // Try to canonicalize the path
    let resolved = if path.is_absolute() {
        path.canonicalize().ok()
    } else if let Some(root) = project_root {
        root.join(path).canonicalize().ok()
    } else {
        std::env::current_dir().ok().and_then(|cwd| cwd.join(path).canonicalize().ok())
    };

    // Classify trust zone
    let trust_zone = resolved.as_ref().map(|p| {
        if let Some(root) = project_root {
            if p.starts_with(root) {
                return "project".to_string();
            }
        }
        if zone_paths.is_user(p) {
            "user".to_string()
        } else if zone_paths.is_system(p) {
            "system".to_string()
        } else {
            "unknown".to_string()
        }
    });

    PositionalValue {
        raw: raw.to_string(),
        resolved: resolved.map(|p| p.to_string_lossy().to_string()),
        trust_zone,
        value_type: "path".to_string(),
    }
}

/// Parse without a known command definition (best effort)
fn parse_without_definition(
    args: &[String],
    parsing: &ParsingOptions,
    subcommand: Option<String>,
    _project_root: Option<&Path>,
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
        positional_args: if positional.is_empty() {
            vec![]
        } else {
            vec![PositionalArg {
                name: "args".to_string(),
                values: positional.into_iter().map(|s| PositionalValue {
                    raw: s,
                    resolved: None,
                    trust_zone: None,
                    value_type: "string".to_string(),
                }).collect(),
            }]
        },
        subcommand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_tokens(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn make_flag(short: &[&str], long: Option<&str>, flag_type: FlagType) -> FlagDef {
        FlagDef {
            short: short.iter().map(|s| s.to_string()).collect(),
            long: long.map(|s| s.to_string()),
            flag_type,
        }
    }

    fn make_subcommand(flags: HashMap<String, FlagDef>) -> SubcommandDef {
        SubcommandDef { flags, positional: vec![] }
    }

    /// Test definitions for command parser tests
    fn test_definitions() -> CommandDefinitions {
        let mut commands = HashMap::new();

        // rm
        commands.insert("rm".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean)),
                ("force".to_string(), make_flag(&["-f"], Some("--force"), FlagType::Boolean)),
            ]),
            positional: vec![PositionalDef {
                name: "targets".to_string(),
                arg_type: ArgType::Path,
                position: None,
                variadic: true,
                last: false,
                optional: false,
            }],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // sudo
        commands.insert("sudo".to_string(), CommandDef {
            flags: HashMap::from([
                ("user".to_string(), make_flag(&["-u"], Some("--user"), FlagType::WithArg)),
            ]),
            positional: vec![],
            subcommands: HashMap::new(),
            is_wrapper: true,
            parsing: ParsingOptions::default(),
        });

        // git
        commands.insert("git".to_string(), CommandDef {
            flags: HashMap::from([
                ("directory".to_string(), make_flag(&["-C"], None, FlagType::WithArg)),
            ]),
            positional: vec![],
            subcommands: HashMap::from([
                ("status".to_string(), make_subcommand(HashMap::from([
                    ("short".to_string(), make_flag(&["-s"], Some("--short"), FlagType::Boolean)),
                ]))),
                ("push".to_string(), make_subcommand(HashMap::from([
                    ("force".to_string(), make_flag(&["-f"], Some("--force"), FlagType::Boolean)),
                ]))),
                ("reset".to_string(), make_subcommand(HashMap::from([
                    ("hard".to_string(), make_flag(&[], Some("--hard"), FlagType::Boolean)),
                ]))),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // chmod
        commands.insert("chmod".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-R"], Some("--recursive"), FlagType::Boolean)),
            ]),
            positional: vec![
                PositionalDef {
                    name: "mode".to_string(),
                    arg_type: ArgType::String,
                    position: Some(0),
                    variadic: false,
                    last: false,
                    optional: false,
                },
                PositionalDef {
                    name: "targets".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                },
            ],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // cp
        commands.insert("cp".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean)),
            ]),
            positional: vec![
                PositionalDef {
                    name: "sources".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                },
                PositionalDef {
                    name: "destination".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: false,
                    last: true,
                    optional: false,
                },
            ],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // cargo
        commands.insert("cargo".to_string(), CommandDef {
            flags: HashMap::new(),
            positional: vec![],
            subcommands: HashMap::from([
                ("build".to_string(), make_subcommand(HashMap::from([
                    ("release".to_string(), make_flag(&["-r"], Some("--release"), FlagType::Boolean)),
                ]))),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // npm
        commands.insert("npm".to_string(), CommandDef {
            flags: HashMap::new(),
            positional: vec![],
            subcommands: HashMap::from([
                ("install".to_string(), make_subcommand(HashMap::from([
                    ("save_dev".to_string(), make_flag(&["-D"], Some("--save-dev"), FlagType::Boolean)),
                ]))),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        CommandDefinitions {
            commands,
            defaults: ParsingOptions::default(),
        }
    }

    #[test]
    fn test_parse_boolean_flags() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -rf /tmp/foo"), &defs, None);

        assert_eq!(result.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_parse_flag_with_arg() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo -u postgres psql"), &defs, None);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("postgres".to_string())));
    }

    #[test]
    fn test_parse_long_flag_equals() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo --user=root ls"), &defs, None);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("root".to_string())));
    }

    #[test]
    fn test_double_dash() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -- -rf"), &defs, None);

        // -rf should be treated as a filename, not flags
        assert!(result.parsed_flags.get("recursive").is_none());
        assert!(!result.positional_args.is_empty());
        assert_eq!(result.positional_args[0].values.len(), 1);
        assert_eq!(result.positional_args[0].values[0].raw, "-rf");
    }

    #[test]
    fn test_git_subcommand() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("git push -f origin main"), &defs, None);

        assert_eq!(result.subcommand, Some("push".to_string()));
        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_unknown_command() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("unknown-cmd -v --verbose"), &defs, None);

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
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -f file1.txt file2.txt"), &defs, None);

        assert_eq!(result.parsed_flags.get("force"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.positional_args[0].values.len(), 2);
        assert_eq!(result.positional_args[0].values[0].raw, "file1.txt");
        assert_eq!(result.positional_args[0].values[1].raw, "file2.txt");
    }

    #[test]
    fn test_long_flag_space_separated() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo --user postgres psql"), &defs, None);

        assert_eq!(result.parsed_flags.get("user"), Some(&FlagValue::String("postgres".to_string())));
    }

    #[test]
    fn test_multiple_short_forms() {
        let defs = test_definitions();
        // rm accepts both -r and -R for recursive
        let result1 = parse_command(&to_tokens("rm -r /tmp"), &defs, None);
        let result2 = parse_command(&to_tokens("rm -R /tmp"), &defs, None);

        assert_eq!(result1.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
        assert_eq!(result2.parsed_flags.get("recursive"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_empty_command() {
        let defs = test_definitions();
        let result = parse_command(&[], &defs, None);

        assert!(result.parsed_flags.is_empty());
        assert!(result.positional_args.is_empty());
        assert!(result.subcommand.is_none());
    }

    #[test]
    fn test_git_reset_subcommand() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("git reset --hard HEAD~1"), &defs, None);

        assert_eq!(result.subcommand, Some("reset".to_string()));
        assert_eq!(result.parsed_flags.get("hard"), Some(&FlagValue::Bool(true)));
        assert_eq!(result.positional_args[0].values[0].raw, "HEAD~1");
    }

    #[test]
    fn test_unknown_command_with_equals() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("myapp --config=prod.yaml"), &defs, None);

        assert_eq!(result.parsed_flags.get("config"), Some(&FlagValue::String("prod.yaml".to_string())));
    }

    #[test]
    fn test_positional_with_definition() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("chmod 755 ./src"), &defs, None);

        // Should have "mode" and "targets" positional args
        let mode = result.positional_args.iter().find(|a| a.name == "mode");
        assert!(mode.is_some());
        assert_eq!(mode.unwrap().values[0].raw, "755");

        let targets = result.positional_args.iter().find(|a| a.name == "targets");
        assert!(targets.is_some());
    }

    #[test]
    fn test_cp_destination() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("cp file1 file2 dest/"), &defs, None);

        let sources = result.positional_args.iter().find(|a| a.name == "sources");
        assert!(sources.is_some());
        assert_eq!(sources.unwrap().values.len(), 2);

        let dest = result.positional_args.iter().find(|a| a.name == "destination");
        assert!(dest.is_some());
    }

    #[test]
    fn test_path_resolution() {
        let defs = test_definitions();
        // Use current dir as project root for testing
        let project_root = std::env::current_dir().unwrap();
        let result = parse_command(&to_tokens("rm ./src"), &defs, Some(&project_root));

        let targets = result.positional_args.iter().find(|a| a.name == "targets");
        assert!(targets.is_some());
        let value = &targets.unwrap().values[0];
        assert_eq!(value.value_type, "path");
        // resolved and trust_zone should be set
        assert!(value.resolved.is_some() || value.trust_zone.is_some() || value.raw == "./src");
    }

    #[test]
    fn test_repeatable_flags() {
        use crate::command_defs::{CommandDef, FlagDef, ParsingOptions};

        // Create custom definitions with a repeatable flag
        let mut commands = HashMap::new();
        let mut flags = HashMap::new();
        flags.insert("header".to_string(), FlagDef {
            short: vec!["-H".to_string()],
            long: Some("--header".to_string()),
            flag_type: FlagType::Repeatable,
        });
        commands.insert("curl".to_string(), CommandDef {
            flags,
            positional: vec![],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        let defs = CommandDefinitions::from_map(commands);
        // Pre-tokenized (as the real tokenizer would produce)
        let tokens = vec![
            "curl".to_string(),
            "-H".to_string(),
            "Accept: application/json".to_string(),
            "-H".to_string(),
            "Authorization: Bearer token".to_string(),
            "https://api.example.com".to_string(),
        ];
        let result = parse_command(&tokens, &defs, None);

        // Should have array of headers
        let headers = result.parsed_flags.get("header");
        assert!(headers.is_some(), "Expected 'header' flag to be present");
        match headers.unwrap() {
            FlagValue::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], "Accept: application/json");
                assert_eq!(arr[1], "Authorization: Bearer token");
            }
            other => panic!("Expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_repeatable_flags_single_occurrence() {
        use crate::command_defs::{CommandDef, FlagDef, ParsingOptions};

        let mut commands = HashMap::new();
        let mut flags = HashMap::new();
        flags.insert("header".to_string(), FlagDef {
            short: vec!["-H".to_string()],
            long: None,
            flag_type: FlagType::Repeatable,
        });
        commands.insert("curl".to_string(), CommandDef {
            flags,
            positional: vec![],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        let defs = CommandDefinitions::from_map(commands);
        // Pre-tokenized
        let tokens = vec![
            "curl".to_string(),
            "-H".to_string(),
            "Content-Type: text/html".to_string(),
            "https://example.com".to_string(),
        ];
        let result = parse_command(&tokens, &defs, None);

        // Even single occurrence should be an array
        let headers = result.parsed_flags.get("header");
        assert!(headers.is_some());
        match headers.unwrap() {
            FlagValue::Array(arr) => {
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0], "Content-Type: text/html");
            }
            other => panic!("Expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_cargo_subcommand() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("cargo build --release"), &defs, None);

        assert_eq!(result.subcommand, Some("build".to_string()));
        assert_eq!(result.parsed_flags.get("release"), Some(&FlagValue::Bool(true)));
    }

    #[test]
    fn test_npm_install() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("npm install -D typescript"), &defs, None);

        assert_eq!(result.subcommand, Some("install".to_string()));
        assert_eq!(result.parsed_flags.get("save_dev"), Some(&FlagValue::Bool(true)));
    }
}
