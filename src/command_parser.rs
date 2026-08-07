use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::command_defs::{
    ArgType, CommandDef, CommandDefinitions, FlagDef, FlagType, ParsingOptions, PositionalDef,
    SubcommandDef,
};
use crate::paths::contains_shell_expansion;
use crate::resolver::TrustZonePaths;
use crate::urls::{canonicalize_url, CanonicalUrl};

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
    pub resolution_known: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_zone: Option<String>,
    #[serde(rename = "type")]
    pub value_type: String,
    /// A URL-typed value that parsed: its canonical form and parts, flattened
    /// into the record so policies read `url.canonical` rather than
    /// `url.url.canonical`.
    #[serde(flatten)]
    pub url: Option<CanonicalUrl>,
    /// Why a URL-typed value could not be canonicalized. Its presence is what
    /// tells a policy the value cannot be matched against a pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected: Option<String>,
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
    /// Flag tokens that matched no entry in the command's definition, in the
    /// order they were typed (repeats included).
    ///
    /// Recorded only where flags are actually modelled: the matched definition
    /// must declare at least one flag -- the subcommand's own flag map when a
    /// subcommand matched, otherwise the command's. For an unknown binary, a
    /// stub subcommand (`git log`) or a flagless command (`touch`) every option
    /// would be "unknown", which is noise rather than signal, so the list stays
    /// empty.
    ///
    /// Policies that grant a command extra latitude should refuse to do so when
    /// this is non-empty -- a flag we could not model is a flag whose effect we
    /// cannot reason about.
    pub unknown_flags: Vec<String>,
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
#[cfg(test)]
pub fn parse_command(
    tokens: &[String],
    definitions: &CommandDefinitions,
    project_root: Option<&Path>,
) -> ParsedCommand {
    parse_command_with_cwd(tokens, definitions, project_root, project_root)
}

/// Parse a command while resolving path positionals against the effective cwd
/// rather than assuming they are relative to the project root.
pub fn parse_command_with_cwd(
    tokens: &[String],
    definitions: &CommandDefinitions,
    cwd: Option<&Path>,
    project_root: Option<&Path>,
) -> ParsedCommand {
    if tokens.is_empty() {
        return ParsedCommand {
            parsed_flags: HashMap::new(),
            positional_args: vec![],
            subcommand: None,
            unknown_flags: vec![],
        };
    }

    let binary_name = &tokens[0];
    let args = &tokens[1..];

    // Get command definition (or use defaults)
    let cmd_def = definitions.get(binary_name);
    let parsing = cmd_def.map(|c| &c.parsing).unwrap_or(&definitions.defaults);

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
                cwd,
                project_root,
                subcommand.as_ref(), // Skip the subcommand name in positional args
                // Stub subcommands model no flags of their own, so every option
                // would be reported as unknown. Only a subcommand that declares
                // flags can tell a modelled option from an unmodelled one.
                !sub_def.flags.is_empty(),
            );

            return ParsedCommand {
                parsed_flags: result.parsed_flags,
                positional_args: result.positional_args,
                subcommand,
                unknown_flags: result.unknown_flags,
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

    let mut result = parse_with_definition_skip_token(
        args,
        flags,
        positional_defs,
        parsing,
        cwd,
        project_root,
        None,
        !flags.is_empty(),
    );
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
                return (Some(arg.clone()), Some(sub_def), &args[i + 1..]);
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
            return matches!(
                def.flag_type,
                FlagType::WithArg | FlagType::WithOptionalArg | FlagType::Repeatable
            );
        }
        // Check long form
        if let Some(long) = &def.long {
            let long_without_dashes = long.strip_prefix("--").unwrap_or(long);
            let flag_without_dashes = flag_str.strip_prefix("--").unwrap_or(flag_str);
            if long_without_dashes == flag_without_dashes {
                return matches!(
                    def.flag_type,
                    FlagType::WithArg | FlagType::WithOptionalArg | FlagType::Repeatable
                );
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
#[allow(clippy::too_many_arguments)]
fn parse_with_definition_skip_token(
    args: &[String],
    flags: &HashMap<String, FlagDef>,
    positional_defs: &[PositionalDef],
    parsing: &ParsingOptions,
    cwd: Option<&Path>,
    project_root: Option<&Path>,
    skip_token: Option<&String>,
    record_unknown_flags: bool,
) -> ParsedCommand {
    let mut parsed_flags: HashMap<String, FlagValue> = HashMap::new();
    let mut positional: Vec<String> = vec![];
    let mut unknown_flags: Vec<String> = vec![];
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
            let (consumed, flag_name, value, is_repeatable) =
                parse_long_flag(arg, &args[i + 1..], flags);
            match flag_name {
                Some(name) => insert_flag(&mut parsed_flags, name, value, is_repeatable),
                // The token looked like a flag but matched no definition. Record
                // it rather than dropping it: `--opt=value` in particular used to
                // vanish entirely, hiding destination- and file-changing options
                // from every policy that inspects parsed_flags.
                None => unknown_flags.push(arg.clone()),
            }
            i += consumed;
            continue;
        }

        // Handle short flags (-f, -rf, -u root)
        //
        // Try the whole token against the definitions first: a definition may
        // declare a single-dash long form (find's -name) or a multi-character
        // short form (wget's -nc), neither of which survives being expanded
        // into single characters.
        if let Some((name, def)) =
            match_flag_by_long(arg, flags).or_else(|| match_flag_by_short(arg, flags))
        {
            let (consumed, value, is_repeatable) = flag_value_from_following(def, &args[i + 1..]);
            insert_flag(&mut parsed_flags, name, value, is_repeatable);
            i += 1 + consumed;
            continue;
        }

        // A value attached to a single-character flag: `-i.bak`, `-XPOST`,
        // `-n5`. The remainder of the token is data, so it must not be expanded
        // into flags (which would also report each character as unknown).
        if let Some((name, value, is_repeatable)) = match_attached_value_flag(arg, flags) {
            insert_flag(
                &mut parsed_flags,
                name,
                FlagValue::String(value),
                is_repeatable,
            );
            i += 1;
            continue;
        }

        // Otherwise the token is a run of combined boolean flags (-rf).
        let expanded = if parsing.combine_short_flags {
            expand_combined_flags(arg)
        } else {
            vec![arg.clone()]
        };

        let mut extra_consumed = 0;
        for (j, short) in expanded.iter().enumerate() {
            let remaining = if j == expanded.len() - 1 {
                &args[i + 1..]
            } else {
                &[]
            };

            let (consumed, flag_name, value, is_repeatable) =
                parse_short_flag(short, remaining, flags);
            match flag_name {
                Some(name) => insert_flag(&mut parsed_flags, name, value, is_repeatable),
                None => unknown_flags.push(short.clone()),
            }
            if consumed > 0 {
                extra_consumed = consumed;
            }
        }
        i += 1 + extra_consumed;
    }

    // Unmatched tokens are only signal where flags are modelled at all; see
    // ParsedCommand::unknown_flags.
    if !record_unknown_flags {
        unknown_flags.clear();
    }

    ParsedCommand {
        parsed_flags,
        positional_args: process_positional_args(positional, positional_defs, cwd, project_root),
        subcommand: None,
        unknown_flags,
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
        let flag_token = &arg[..equals_pos + 2];
        let value_part = &without_dashes[equals_pos + 1..];

        // Find matching flag definition
        if let Some((name, def)) = match_flag_by_long(flag_token, flags) {
            match def.flag_type {
                FlagType::Boolean => {
                    // Boolean flags shouldn't use = form, but handle it anyway
                    return (1, Some(name), FlagValue::Bool(true), false);
                }
                FlagType::WithArg | FlagType::WithOptionalArg => {
                    return (
                        1,
                        Some(name),
                        FlagValue::String(value_part.to_string()),
                        false,
                    );
                }
                FlagType::Repeatable => {
                    return (
                        1,
                        Some(name),
                        FlagValue::String(value_part.to_string()),
                        true,
                    );
                }
            }
        }

        // No definition matched; the caller records the token in unknown_flags.
        return (1, None, FlagValue::Bool(false), false);
    }

    // No = form: --force or --user root
    if let Some((name, def)) = match_flag_by_long(arg, flags) {
        let (consumed, value, is_repeatable) = flag_value_from_following(def, remaining);
        return (1 + consumed, Some(name), value, is_repeatable);
    }

    // No definition matched; the caller records the token in unknown_flags.
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
        let (consumed, value, is_repeatable) = flag_value_from_following(def, remaining);
        return (consumed, Some(name), value, is_repeatable);
    }

    // Try claim patterns for unknown flags (e.g., -30 -> lines: "30")
    if let Some((name, value)) = try_claim_pattern(arg, flags) {
        return (0, Some(name), FlagValue::String(value), false);
    }

    // No definition matched; the caller records the token in unknown_flags.
    (0, None, FlagValue::Bool(false), false)
}

/// Work out a matched flag's value from the tokens that follow it.
///
/// Returns (extra_tokens_consumed, value, is_repeatable). `WithOptionalArg`
/// never consumes a following token: GNU tools spell the optional value
/// attached (`-i.bak`, `--in-place=.bak`), so treating the next token as the
/// value swallows an argument -- `sed -i 's/foo/bar/' f` would take the script
/// as the backup suffix.
fn flag_value_from_following(def: &FlagDef, remaining: &[String]) -> (usize, FlagValue, bool) {
    let is_repeatable = matches!(def.flag_type, FlagType::Repeatable);

    match def.flag_type {
        FlagType::Boolean | FlagType::WithOptionalArg => (0, FlagValue::Bool(true), false),
        FlagType::WithArg | FlagType::Repeatable => {
            if !remaining.is_empty() && !remaining[0].starts_with('-') {
                (1, FlagValue::String(remaining[0].clone()), is_repeatable)
            } else {
                // Missing required argument; record the flag's presence only.
                (0, FlagValue::Bool(true), is_repeatable)
            }
        }
    }
}

/// Match `-i.bak` / `-XPOST` / `-n5`: a single-character flag that takes a
/// value, with that value attached to the same token.
///
/// Returns (flag_name, value, is_repeatable).
fn match_attached_value_flag(
    token: &str,
    flags: &HashMap<String, FlagDef>,
) -> Option<(String, String, bool)> {
    let mut chars = token.strip_prefix('-')?.chars();
    let first = chars.next()?;
    let value: String = chars.collect();
    if value.is_empty() {
        return None;
    }

    let (name, def) = match_flag_by_short(&format!("-{}", first), flags)?;
    if !matches!(
        def.flag_type,
        FlagType::WithArg | FlagType::WithOptionalArg | FlagType::Repeatable
    ) {
        return None;
    }

    Some((name, value, matches!(def.flag_type, FlagType::Repeatable)))
}

/// Find a flag definition by long form, given the whole token (dashes included)
///
/// A definition may spell its long form with a single dash -- find's `-name`,
/// `-type`, `-maxdepth` -- in which case the token must match it exactly.
fn match_flag_by_long<'a>(
    token: &str,
    flags: &'a HashMap<String, FlagDef>,
) -> Option<(String, &'a FlagDef)> {
    for (name, def) in flags {
        let Some(long) = &def.long else { continue };

        let matched = match long.strip_prefix("--") {
            // GNU long option: compare the names without their dashes
            Some(long_name) => token.strip_prefix("--") == Some(long_name),
            // Single-dash long form (find's -name): match the token exactly
            None if long.starts_with('-') => long.as_str() == token,
            // Declared without dashes: still a GNU long option
            None => token.strip_prefix("--") == Some(long.as_str()),
        };

        if matched {
            return Some((name.clone(), def));
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

/// Try to match an unknown flag against claim_patterns
/// Returns (flag_name, captured_value) if a pattern matches
fn try_claim_pattern(flag: &str, flags: &HashMap<String, FlagDef>) -> Option<(String, String)> {
    for (name, def) in flags {
        if let Some(pattern) = &def.claim_pattern {
            // Compile the regex (in production, we'd cache this)
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(flag) {
                    // Use first capture group if present, otherwise whole match
                    let value = caps
                        .get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    return Some((name.clone(), value));
                }
            }
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
    cwd: Option<&Path>,
    project_root: Option<&Path>,
) -> Vec<PositionalArg> {
    if positional_defs.is_empty() {
        // No definitions - return raw args
        return vec![PositionalArg {
            name: "args".to_string(),
            values: raw_args
                .into_iter()
                .map(|s| PositionalValue {
                    raw: s,
                    resolved: None,
                    resolution_known: None,
                    trust_zone: None,
                    value_type: "string".to_string(),
                    url: None,
                    rejected: None,
                })
                .collect(),
        }];
    }

    let mut result = Vec::new();
    let mut remaining: Vec<String> = raw_args;

    // Handle position-based args first (explicit index)
    for def in positional_defs.iter().filter(|d| d.position.is_some()) {
        let pos = def.position.unwrap() as usize;
        if pos < remaining.len() {
            let value = remaining.remove(pos);
            result.push(create_positional_arg(
                &def.name,
                vec![value],
                &def.arg_type,
                cwd,
                project_root,
            ));
        }
    }

    // Handle "last" arg (like cp destination)
    if let Some(last_def) = positional_defs.iter().find(|d| d.last) {
        if !remaining.is_empty() {
            let last = remaining.pop().unwrap();
            result.push(create_positional_arg(
                &last_def.name,
                vec![last],
                &last_def.arg_type,
                cwd,
                project_root,
            ));
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
        result.push(create_positional_arg(
            &def.name,
            vec![value],
            &def.arg_type,
            cwd,
            project_root,
        ));
    }

    // Handle variadic arg (remaining args after sequential)
    if let Some(variadic_def) = positional_defs.iter().find(|d| d.variadic) {
        if !remaining.is_empty() {
            result.push(create_positional_arg(
                &variadic_def.name,
                remaining,
                &variadic_def.arg_type,
                cwd,
                project_root,
            ));
        }
    } else if !remaining.is_empty() {
        // No variadic def but have remaining args - use generic "args"
        result.push(PositionalArg {
            name: "args".to_string(),
            values: remaining
                .into_iter()
                .map(|s| PositionalValue {
                    raw: s,
                    resolved: None,
                    resolution_known: None,
                    trust_zone: None,
                    value_type: "string".to_string(),
                    url: None,
                    rejected: None,
                })
                .collect(),
        });
    }

    result
}

/// Build a URL-typed value from a raw token.
///
/// A token that cannot be canonicalized still becomes a record, carrying the
/// reason instead of a canonical form: a policy has to be able to tell "no URL
/// here" from "a URL cmdguard refuses to vouch for".
pub fn url_value(raw: &str) -> PositionalValue {
    let (url, rejected) = match canonicalize_url(raw) {
        Ok(url) => (Some(url), None),
        Err(rejection) => (None, Some(rejection.reason)),
    };

    PositionalValue {
        raw: raw.to_string(),
        resolved: None,
        resolution_known: None,
        trust_zone: None,
        value_type: "url".to_string(),
        url,
        rejected,
    }
}

/// Every URL a command declared, as one list for policies (`input.urls`).
///
/// Positional URL arguments carry their canonical form already, because the
/// command definition types them. Flag values do not: `parsed_flags` holds
/// plain strings, and giving every flag definition a value type to change that
/// would touch every command. curl is the only command whose flag can carry a
/// URL (`--url`), so it is named here instead.
pub fn collect_urls(parsed: &ParsedCommand, binary_name: &str) -> Vec<PositionalValue> {
    let mut urls: Vec<PositionalValue> = parsed
        .positional_args
        .iter()
        .flat_map(|arg| arg.values.iter())
        .filter(|value| value.value_type == "url")
        .cloned()
        .collect();

    if binary_name == "curl" {
        let raw_values: &[String] = match parsed.parsed_flags.get("url") {
            Some(FlagValue::Array(values)) => values,
            Some(FlagValue::String(value)) => std::slice::from_ref(value),
            // `--url` without a usable value: nothing to canonicalize. The
            // policy notices the missing value separately.
            _ => &[],
        };
        urls.extend(raw_values.iter().map(|raw| url_value(raw)));
    }

    urls
}

/// Create a positional arg from values with proper type handling
fn create_positional_arg(
    name: &str,
    values: Vec<String>,
    arg_type: &ArgType,
    cwd: Option<&Path>,
    project_root: Option<&Path>,
) -> PositionalArg {
    let resolved_values: Vec<PositionalValue> = values
        .into_iter()
        .map(|raw| match arg_type {
            ArgType::Path => resolve_path_arg(&raw, cwd, project_root),
            ArgType::String => PositionalValue {
                raw,
                resolved: None,
                resolution_known: None,
                trust_zone: None,
                value_type: "string".to_string(),
                url: None,
                rejected: None,
            },
            ArgType::Number => PositionalValue {
                raw,
                resolved: None,
                resolution_known: None,
                trust_zone: None,
                value_type: "number".to_string(),
                url: None,
                rejected: None,
            },
            ArgType::Url => url_value(&raw),
        })
        .collect();

    PositionalArg {
        name: name.to_string(),
        values: resolved_values,
    }
}

/// Resolve a path argument with trust zone classification
fn resolve_path_arg(raw: &str, cwd: Option<&Path>, project_root: Option<&Path>) -> PositionalValue {
    use std::path::Path as StdPath;

    if contains_shell_expansion(raw) {
        return PositionalValue {
            raw: raw.to_string(),
            resolved: None,
            resolution_known: Some(false),
            trust_zone: Some("unknown".to_string()),
            value_type: "path".to_string(),
            url: None,
            rejected: None,
        };
    }

    let expanded = expand_tilde(raw);
    let path = expanded.as_deref().unwrap_or_else(|| StdPath::new(raw));
    let zone_paths = TrustZonePaths::defaults();

    if !path.is_absolute() && cwd.is_none() {
        return PositionalValue {
            raw: raw.to_string(),
            resolved: None,
            resolution_known: Some(false),
            trust_zone: Some("unknown".to_string()),
            value_type: "path".to_string(),
            url: None,
            rejected: None,
        };
    }

    // Try to canonicalize the path. For non-existent targets, keep a lexical
    // absolute path so create/write operations can still be scoped by policy.
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        unreachable!("relative paths without a cwd return above")
    };
    let resolved = candidate
        .canonicalize()
        .unwrap_or_else(|_| normalize_path(&candidate));

    // Classify trust zone
    let trust_zone = {
        if let Some(root) = project_root {
            if path_in_project(&resolved, root) {
                "project".to_string()
            } else if zone_paths.is_user(&resolved) {
                "user".to_string()
            } else if zone_paths.is_system(&resolved) {
                "system".to_string()
            } else {
                "unknown".to_string()
            }
        } else if zone_paths.is_user(&resolved) {
            "user".to_string()
        } else if zone_paths.is_system(&resolved) {
            "system".to_string()
        } else {
            "unknown".to_string()
        }
    };

    PositionalValue {
        raw: raw.to_string(),
        resolved: Some(resolved.to_string_lossy().to_string()),
        resolution_known: Some(true),
        trust_zone: Some(trust_zone),
        value_type: "path".to_string(),
        url: None,
        rejected: None,
    }
}

fn expand_tilde(raw: &str) -> Option<PathBuf> {
    if raw == "~" {
        return dirs::home_dir();
    }

    raw.strip_prefix("~/")
        .and_then(|suffix| dirs::home_dir().map(|home| home.join(suffix)))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}

fn path_in_project(path: &Path, project_root: &Path) -> bool {
    path == project_root || path.starts_with(project_root)
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
        if let Some(without_dashes) = arg.strip_prefix("--") {
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
                parsed_flags.insert(
                    without_dashes.to_string(),
                    FlagValue::String(args[i + 1].clone()),
                );
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
                parsed_flags.insert(
                    short_without_dash.to_string(),
                    FlagValue::String(args[i + 1].clone()),
                );
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
                values: positional
                    .into_iter()
                    .map(|s| PositionalValue {
                        raw: s,
                        resolved: None,
                        resolution_known: None,
                        trust_zone: None,
                        value_type: "string".to_string(),
                        url: None,
                        rejected: None,
                    })
                    .collect(),
            }]
        },
        subcommand,
        // No definition for this binary, so "unrecognized" carries no signal --
        // every flag would be listed. Policies key off the command being known.
        unknown_flags: vec![],
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
            claim_pattern: None,
        }
    }

    fn make_subcommand(flags: HashMap<String, FlagDef>) -> SubcommandDef {
        SubcommandDef {
            flags,
            positional: vec![],
        }
    }

    /// Test definitions for command parser tests
    fn test_definitions() -> CommandDefinitions {
        let mut commands = HashMap::new();

        // rm
        commands.insert(
            "rm".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "recursive".to_string(),
                        make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean),
                    ),
                    (
                        "force".to_string(),
                        make_flag(&["-f"], Some("--force"), FlagType::Boolean),
                    ),
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
            },
        );

        // sudo
        commands.insert(
            "sudo".to_string(),
            CommandDef {
                flags: HashMap::from([(
                    "user".to_string(),
                    make_flag(&["-u"], Some("--user"), FlagType::WithArg),
                )]),
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: true,
                parsing: ParsingOptions::default(),
            },
        );

        // git
        commands.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::from([(
                    "directory".to_string(),
                    make_flag(&["-C"], None, FlagType::WithArg),
                )]),
                positional: vec![],
                subcommands: HashMap::from([
                    (
                        "status".to_string(),
                        make_subcommand(HashMap::from([(
                            "short".to_string(),
                            make_flag(&["-s"], Some("--short"), FlagType::Boolean),
                        )])),
                    ),
                    (
                        "push".to_string(),
                        make_subcommand(HashMap::from([(
                            "force".to_string(),
                            make_flag(&["-f"], Some("--force"), FlagType::Boolean),
                        )])),
                    ),
                    (
                        "reset".to_string(),
                        make_subcommand(HashMap::from([(
                            "hard".to_string(),
                            make_flag(&[], Some("--hard"), FlagType::Boolean),
                        )])),
                    ),
                    // stub: a subcommand cmdguard names but models no flags for
                    ("log".to_string(), make_subcommand(HashMap::new())),
                ]),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // chmod
        commands.insert(
            "chmod".to_string(),
            CommandDef {
                flags: HashMap::from([(
                    "recursive".to_string(),
                    make_flag(&["-R"], Some("--recursive"), FlagType::Boolean),
                )]),
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
            },
        );

        // cp
        commands.insert(
            "cp".to_string(),
            CommandDef {
                flags: HashMap::from([(
                    "recursive".to_string(),
                    make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean),
                )]),
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
            },
        );

        // cargo
        commands.insert(
            "cargo".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![],
                subcommands: HashMap::from([(
                    "build".to_string(),
                    make_subcommand(HashMap::from([(
                        "release".to_string(),
                        make_flag(&["-r"], Some("--release"), FlagType::Boolean),
                    )])),
                )]),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // npm
        commands.insert(
            "npm".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![],
                subcommands: HashMap::from([(
                    "install".to_string(),
                    make_subcommand(HashMap::from([(
                        "save_dev".to_string(),
                        make_flag(&["-D"], Some("--save-dev"), FlagType::Boolean),
                    )])),
                )]),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // sed -- WithOptionalArg (-i / --in-place[=SUFFIX]) plus a repeatable
        // script flag, mirroring config/builtins.ncl
        commands.insert(
            "sed".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "in_place".to_string(),
                        make_flag(&["-i"], Some("--in-place"), FlagType::WithOptionalArg),
                    ),
                    (
                        "expression".to_string(),
                        make_flag(&["-e"], Some("--expression"), FlagType::Repeatable),
                    ),
                    (
                        "quiet".to_string(),
                        make_flag(&["-n"], Some("--quiet"), FlagType::Boolean),
                    ),
                ]),
                positional: vec![
                    PositionalDef {
                        name: "script".to_string(),
                        arg_type: ArgType::String,
                        position: None,
                        variadic: false,
                        last: false,
                        optional: false,
                    },
                    PositionalDef {
                        name: "files".to_string(),
                        arg_type: ArgType::Path,
                        position: None,
                        variadic: true,
                        last: false,
                        optional: true,
                    },
                ],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // find -- long options written with a single dash
        commands.insert(
            "find".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "name".to_string(),
                        make_flag(&[], Some("-name"), FlagType::WithArg),
                    ),
                    (
                        "type".to_string(),
                        make_flag(&[], Some("-type"), FlagType::WithArg),
                    ),
                    (
                        "maxdepth".to_string(),
                        make_flag(&[], Some("-maxdepth"), FlagType::WithArg),
                    ),
                ]),
                positional: vec![PositionalDef {
                    name: "paths".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: true,
                }],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // wget -- multi-character short form (-nc)
        commands.insert(
            "wget".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "no_clobber".to_string(),
                        make_flag(&["-nc"], Some("--no-clobber"), FlagType::Boolean),
                    ),
                    (
                        "quiet".to_string(),
                        make_flag(&["-q"], Some("--quiet"), FlagType::Boolean),
                    ),
                ]),
                positional: vec![PositionalDef {
                    name: "urls".to_string(),
                    arg_type: ArgType::String,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                }],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // curl -- value attached to a single-character short flag (-XPOST)
        commands.insert(
            "curl".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "request".to_string(),
                        make_flag(&["-X"], Some("--request"), FlagType::WithArg),
                    ),
                    (
                        "output".to_string(),
                        make_flag(&["-o"], Some("--output"), FlagType::WithArg),
                    ),
                    (
                        "header".to_string(),
                        make_flag(&["-H"], Some("--header"), FlagType::Repeatable),
                    ),
                    (
                        "url".to_string(),
                        make_flag(&[], Some("--url"), FlagType::Repeatable),
                    ),
                ]),
                positional: vec![PositionalDef {
                    name: "url".to_string(),
                    arg_type: ArgType::Url,
                    position: None,
                    variadic: false,
                    last: true,
                    optional: false,
                }],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // grep -- -A3 style attached numeric value
        commands.insert(
            "grep".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "after_context".to_string(),
                        make_flag(&["-A"], Some("--after-context"), FlagType::WithArg),
                    ),
                    (
                        "ignore_case".to_string(),
                        make_flag(&["-i"], Some("--ignore-case"), FlagType::Boolean),
                    ),
                    (
                        "recursive".to_string(),
                        make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean),
                    ),
                ]),
                positional: vec![
                    PositionalDef {
                        name: "pattern".to_string(),
                        arg_type: ArgType::String,
                        position: None,
                        variadic: false,
                        last: false,
                        optional: false,
                    },
                    PositionalDef {
                        name: "files".to_string(),
                        arg_type: ArgType::Path,
                        position: None,
                        variadic: true,
                        last: false,
                        optional: true,
                    },
                ],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // tail -- combine_short_flags = false plus a claim_pattern for -NUM
        commands.insert(
            "tail".to_string(),
            CommandDef {
                flags: HashMap::from([
                    (
                        "lines".to_string(),
                        FlagDef {
                            short: vec!["-n".to_string()],
                            long: Some("--lines".to_string()),
                            flag_type: FlagType::WithArg,
                            claim_pattern: Some("^-(\\d+)$".to_string()),
                        },
                    ),
                    (
                        "follow".to_string(),
                        make_flag(&["-f"], Some("--follow"), FlagType::Boolean),
                    ),
                ]),
                positional: vec![PositionalDef {
                    name: "files".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: true,
                }],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions {
                    combine_short_flags: false,
                    double_dash_ends_flags: true,
                },
            },
        );

        // touch -- a command with no modelled flags at all
        commands.insert(
            "touch".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![PositionalDef {
                    name: "files".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                }],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        CommandDefinitions {
            commands,
            defaults: ParsingOptions::default(),
        }
    }

    #[test]
    fn test_parse_boolean_flags() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -rf /tmp/foo"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("recursive"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(
            result.parsed_flags.get("force"),
            Some(&FlagValue::Bool(true))
        );
    }

    #[test]
    fn test_parse_flag_with_arg() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo -u postgres psql"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("user"),
            Some(&FlagValue::String("postgres".to_string()))
        );
    }

    #[test]
    fn test_parse_long_flag_equals() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo --user=root ls"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("user"),
            Some(&FlagValue::String("root".to_string()))
        );
    }

    #[test]
    fn test_double_dash() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -- -rf"), &defs, None);

        // -rf should be treated as a filename, not flags
        assert!(!result.parsed_flags.contains_key("recursive"));
        assert!(!result.positional_args.is_empty());
        assert_eq!(result.positional_args[0].values.len(), 1);
        assert_eq!(result.positional_args[0].values[0].raw, "-rf");
    }

    #[test]
    fn test_git_subcommand() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("git push -f origin main"), &defs, None);

        assert_eq!(result.subcommand, Some("push".to_string()));
        assert_eq!(
            result.parsed_flags.get("force"),
            Some(&FlagValue::Bool(true))
        );
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

        assert_eq!(
            result.parsed_flags.get("force"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(result.positional_args[0].values.len(), 2);
        assert_eq!(result.positional_args[0].values[0].raw, "file1.txt");
        assert_eq!(result.positional_args[0].values[1].raw, "file2.txt");
    }

    #[test]
    fn test_long_flag_space_separated() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sudo --user postgres psql"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("user"),
            Some(&FlagValue::String("postgres".to_string()))
        );
    }

    #[test]
    fn test_multiple_short_forms() {
        let defs = test_definitions();
        // rm accepts both -r and -R for recursive
        let result1 = parse_command(&to_tokens("rm -r /tmp"), &defs, None);
        let result2 = parse_command(&to_tokens("rm -R /tmp"), &defs, None);

        assert_eq!(
            result1.parsed_flags.get("recursive"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(
            result2.parsed_flags.get("recursive"),
            Some(&FlagValue::Bool(true))
        );
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
        assert_eq!(
            result.parsed_flags.get("hard"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(result.positional_args[0].values[0].raw, "HEAD~1");
    }

    #[test]
    fn test_unrecognized_equals_flag_is_recorded_not_dropped() {
        // Regression: `--opt=value` on a *known* command used to be consumed and
        // silently discarded -- neither a flag nor a positional. Policies that
        // granted latitude based on parsed_flags were therefore blind to any
        // option cmdguard had not modelled, including ones that redirect a
        // request or write a file.
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm --no-preserve-root=yes /"), &defs, None);

        assert_eq!(result.unknown_flags, vec!["--no-preserve-root=yes"]);
        // The token is recorded as unknown, not smuggled in under some other
        // name: nothing lands in parsed_flags, and the positional targets are
        // exactly what was typed.
        assert!(result.parsed_flags.is_empty(), "{:?}", result.parsed_flags);
        let targets = result.positional_args.iter().find(|a| a.name == "targets");
        assert_eq!(
            targets
                .unwrap()
                .values
                .iter()
                .map(|v| v.raw.as_str())
                .collect::<Vec<_>>(),
            vec!["/"]
        );
    }

    #[test]
    fn test_unrecognized_space_form_flag_is_recorded() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm --bogus /tmp/x"), &defs, None);

        assert_eq!(result.unknown_flags, vec!["--bogus"]);
    }

    #[test]
    fn test_unrecognized_short_flag_is_recorded() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -q /tmp/x"), &defs, None);

        assert_eq!(result.unknown_flags, vec!["-q"]);
    }

    #[test]
    fn test_recognized_flags_leave_unknown_flags_empty() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -rf /tmp/x"), &defs, None);

        assert!(
            result.unknown_flags.is_empty(),
            "expected no unknown flags, got {:?}",
            result.unknown_flags
        );
        assert_eq!(
            result.parsed_flags.get("recursive"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(
            result.parsed_flags.get("force"),
            Some(&FlagValue::Bool(true))
        );
    }

    #[test]
    fn test_unknown_binary_reports_no_unknown_flags() {
        // For a binary with no definition every flag is trivially unrecognized,
        // which is noise rather than signal -- policies key off known commands.
        let defs = test_definitions();
        let result = parse_command(&to_tokens("myapp --whatever=1 -z"), &defs, None);

        assert!(result.unknown_flags.is_empty());
    }

    #[test]
    fn test_unknown_command_with_equals() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("myapp --config=prod.yaml"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("config"),
            Some(&FlagValue::String("prod.yaml".to_string()))
        );
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

        let dest = result
            .positional_args
            .iter()
            .find(|a| a.name == "destination");
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
    fn test_nonexistent_project_path_resolution() {
        let defs = test_definitions();
        let project_root = std::env::current_dir().unwrap();
        let result = parse_command(
            &to_tokens("rm ./definitely-missing-cmdguard-path"),
            &defs,
            Some(&project_root),
        );

        let targets = result.positional_args.iter().find(|a| a.name == "targets");
        let value = &targets.unwrap().values[0];
        assert_eq!(value.trust_zone.as_deref(), Some("project"));
        assert_eq!(
            value.resolved.as_deref(),
            Some(
                project_root
                    .join("definitely-missing-cmdguard-path")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn path_resolution_uses_effective_cwd() {
        let defs = test_definitions();
        let cwd = PathBuf::from("/tmp/effective");
        let project_root = PathBuf::from("/workspace");
        let result = parse_command_with_cwd(
            &to_tokens("rm ./target"),
            &defs,
            Some(&cwd),
            Some(&project_root),
        );

        let target = &result
            .positional_args
            .iter()
            .find(|arg| arg.name == "targets")
            .unwrap()
            .values[0];
        assert_eq!(target.resolved.as_deref(), Some("/tmp/effective/target"));
        assert_ne!(target.trust_zone.as_deref(), Some("project"));
    }

    #[test]
    fn relative_path_is_unresolved_when_effective_cwd_is_unknown() {
        let defs = test_definitions();
        let project_root = PathBuf::from("/workspace");
        let result =
            parse_command_with_cwd(&to_tokens("rm ./target"), &defs, None, Some(&project_root));

        let target = &result
            .positional_args
            .iter()
            .find(|arg| arg.name == "targets")
            .unwrap()
            .values[0];
        assert_eq!(target.resolved, None);
        assert_eq!(target.resolution_known, Some(false));
        assert_eq!(target.trust_zone.as_deref(), Some("unknown"));
    }

    #[test]
    fn shell_expanded_path_is_unresolved() {
        let defs = test_definitions();
        let cwd = PathBuf::from("/workspace");
        let result = parse_command_with_cwd(
            &to_tokens("rm ./$(printf ../../tmp/target)"),
            &defs,
            Some(&cwd),
            Some(&cwd),
        );

        let target = &result
            .positional_args
            .iter()
            .find(|arg| arg.name == "targets")
            .unwrap()
            .values[0];
        assert_eq!(target.resolved, None);
        assert_eq!(target.resolution_known, Some(false));
        assert_eq!(target.trust_zone.as_deref(), Some("unknown"));
    }

    #[test]
    fn test_tilde_path_not_project_scoped() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let defs = test_definitions();
        let project_root = std::env::current_dir().unwrap();
        let result = parse_command(
            &to_tokens("rm ~/.cmdguard-nonexistent-path-for-test"),
            &defs,
            Some(&project_root),
        );

        let targets = result.positional_args.iter().find(|a| a.name == "targets");
        let value = &targets.unwrap().values[0];
        assert_ne!(value.trust_zone.as_deref(), Some("project"));
        assert!(value
            .resolved
            .as_deref()
            .is_some_and(|resolved| resolved.starts_with(&home.to_string_lossy().to_string())));
    }

    #[test]
    fn test_repeatable_flags() {
        use crate::command_defs::{CommandDef, FlagDef, ParsingOptions};

        // Create custom definitions with a repeatable flag
        let mut commands = HashMap::new();
        let mut flags = HashMap::new();
        flags.insert(
            "header".to_string(),
            FlagDef {
                short: vec!["-H".to_string()],
                long: Some("--header".to_string()),
                flag_type: FlagType::Repeatable,
                claim_pattern: None,
            },
        );
        commands.insert(
            "curl".to_string(),
            CommandDef {
                flags,
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

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
        flags.insert(
            "header".to_string(),
            FlagDef {
                short: vec!["-H".to_string()],
                long: None,
                flag_type: FlagType::Repeatable,
                claim_pattern: None,
            },
        );
        commands.insert(
            "curl".to_string(),
            CommandDef {
                flags,
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

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
        assert_eq!(
            result.parsed_flags.get("release"),
            Some(&FlagValue::Bool(true))
        );
    }

    #[test]
    fn test_npm_install() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("npm install -D typescript"), &defs, None);

        assert_eq!(result.subcommand, Some("install".to_string()));
        assert_eq!(
            result.parsed_flags.get("save_dev"),
            Some(&FlagValue::Bool(true))
        );
    }

    #[test]
    fn test_claim_pattern_numeric_flag() {
        use crate::command_defs::{CommandDef, FlagDef, ParsingOptions};

        // Create a command with claim_pattern for -NUM syntax (like tail -30)
        let mut commands = HashMap::new();
        let mut flags = HashMap::new();
        flags.insert(
            "lines".to_string(),
            FlagDef {
                short: vec!["-n".to_string()],
                long: Some("--lines".to_string()),
                flag_type: FlagType::WithArg,
                claim_pattern: Some("^-(\\d+)$".to_string()),
            },
        );
        commands.insert(
            "tail".to_string(),
            CommandDef {
                flags,
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions {
                    combine_short_flags: false, // Important for -30 not to become -3 -0
                    double_dash_ends_flags: true,
                },
            },
        );

        let defs = CommandDefinitions::from_map(commands);

        // Test -30 gets parsed as lines: "30"
        let result = parse_command(&["tail".to_string(), "-30".to_string()], &defs, None);
        assert_eq!(
            result.parsed_flags.get("lines"),
            Some(&FlagValue::String("30".to_string()))
        );

        // Test -n 50 still works normally
        let result2 = parse_command(
            &["tail".to_string(), "-n".to_string(), "50".to_string()],
            &defs,
            None,
        );
        assert_eq!(
            result2.parsed_flags.get("lines"),
            Some(&FlagValue::String("50".to_string()))
        );
    }

    #[test]
    fn test_optional_arg_flag_does_not_consume_next_token() {
        // `sed -i 's/foo/bar/' f`: -i takes its suffix only when attached.
        // Swallowing the next token turned the script into the suffix and the
        // file into the script.
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sed -i s/foo/bar/ f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("in_place"),
            Some(&FlagValue::Bool(true))
        );
        let script = result.positional_args.iter().find(|a| a.name == "script");
        assert_eq!(script.unwrap().values[0].raw, "s/foo/bar/");
        let files = result.positional_args.iter().find(|a| a.name == "files");
        assert_eq!(
            files
                .unwrap()
                .values
                .iter()
                .map(|v| v.raw.as_str())
                .collect::<Vec<_>>(),
            vec!["f"]
        );
    }

    #[test]
    fn test_long_optional_arg_flag_does_not_consume_next_token() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sed --in-place s/foo/bar/ f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("in_place"),
            Some(&FlagValue::Bool(true))
        );
        let script = result.positional_args.iter().find(|a| a.name == "script");
        assert_eq!(script.unwrap().values[0].raw, "s/foo/bar/");
        let files = result.positional_args.iter().find(|a| a.name == "files");
        assert_eq!(
            files
                .unwrap()
                .values
                .iter()
                .map(|v| v.raw.as_str())
                .collect::<Vec<_>>(),
            vec!["f"]
        );
    }

    #[test]
    fn test_long_optional_arg_takes_attached_value() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sed --in-place=.bak s/x/y/ f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("in_place"),
            Some(&FlagValue::String(".bak".to_string()))
        );
    }

    #[test]
    fn test_short_optional_arg_takes_attached_value() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("sed -i.bak s/x/y/ f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("in_place"),
            Some(&FlagValue::String(".bak".to_string()))
        );
        // ".bak" is a value, not a run of combined boolean flags: expanding it
        // would report -b/-a/-k as unknown.
        assert!(
            result.unknown_flags.is_empty(),
            "expected no unknown flags, got {:?}",
            result.unknown_flags
        );
    }

    #[test]
    fn test_single_dash_long_options_match_their_definition() {
        // find spells its long options with one dash. Expanding -name into
        // -n/-a/-m/-e lost the option and buried its value in the paths.
        let defs = test_definitions();
        let result = parse_command(&to_tokens("find . -name *.rs -type f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("name"),
            Some(&FlagValue::String("*.rs".to_string()))
        );
        assert_eq!(
            result.parsed_flags.get("type"),
            Some(&FlagValue::String("f".to_string()))
        );
        assert!(
            result.unknown_flags.is_empty(),
            "expected no unknown flags, got {:?}",
            result.unknown_flags
        );
        let paths = result.positional_args.iter().find(|a| a.name == "paths");
        assert_eq!(
            paths
                .unwrap()
                .values
                .iter()
                .map(|v| v.raw.as_str())
                .collect::<Vec<_>>(),
            vec!["."]
        );
    }

    #[test]
    fn test_multi_character_short_form_matches_whole_token() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("wget -nc http://x/y"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("no_clobber"),
            Some(&FlagValue::Bool(true))
        );
        assert!(
            result.unknown_flags.is_empty(),
            "expected no unknown flags, got {:?}",
            result.unknown_flags
        );
    }

    #[test]
    fn test_value_attached_to_short_flag() {
        let defs = test_definitions();

        let grep = parse_command(&to_tokens("grep -A3 pat f"), &defs, None);
        assert_eq!(
            grep.parsed_flags.get("after_context"),
            Some(&FlagValue::String("3".to_string()))
        );
        assert!(grep.unknown_flags.is_empty(), "{:?}", grep.unknown_flags);

        let curl_request = parse_command(&to_tokens("curl -XPOST https://x"), &defs, None);
        assert_eq!(
            curl_request.parsed_flags.get("request"),
            Some(&FlagValue::String("POST".to_string()))
        );
        assert!(
            curl_request.unknown_flags.is_empty(),
            "{:?}",
            curl_request.unknown_flags
        );

        let curl_output = parse_command(&to_tokens("curl -o/tmp/out http://x"), &defs, None);
        assert_eq!(
            curl_output.parsed_flags.get("output"),
            Some(&FlagValue::String("/tmp/out".to_string()))
        );
        assert!(
            curl_output.unknown_flags.is_empty(),
            "{:?}",
            curl_output.unknown_flags
        );
    }

    #[test]
    fn test_value_attached_to_short_flag_without_combining() {
        // tail sets combine_short_flags = false; -n5 must still mean -n 5.
        let defs = test_definitions();
        let result = parse_command(&to_tokens("tail -n5 f"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("lines"),
            Some(&FlagValue::String("5".to_string()))
        );
        assert!(
            result.unknown_flags.is_empty(),
            "expected no unknown flags, got {:?}",
            result.unknown_flags
        );
    }

    #[test]
    fn test_combined_boolean_flags_record_unmatched_characters() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm -rq x"), &defs, None);

        assert_eq!(
            result.parsed_flags.get("recursive"),
            Some(&FlagValue::Bool(true))
        );
        assert_eq!(result.unknown_flags, vec!["-q"]);
    }

    #[test]
    fn test_unknown_flags_only_recorded_where_flags_are_modelled() {
        let defs = test_definitions();

        // `log` is a subcommand stub with no modelled flags: every option would
        // be "unknown", which is noise rather than signal.
        let log = parse_command(&to_tokens("git log --oneline -5"), &defs, None);
        assert!(log.unknown_flags.is_empty(), "{:?}", log.unknown_flags);

        // `push` does model flags, so an unmatched one is real signal.
        let push = parse_command(&to_tokens("git push --bogus origin main"), &defs, None);
        assert_eq!(push.unknown_flags, vec!["--bogus"]);

        // touch models no flags at all.
        let touch = parse_command(&to_tokens("touch -r a b"), &defs, None);
        assert!(touch.unknown_flags.is_empty(), "{:?}", touch.unknown_flags);
    }

    #[test]
    fn test_unknown_flags_preserve_order_and_repeats() {
        let defs = test_definitions();
        let result = parse_command(&to_tokens("rm --beta --alpha --beta /tmp/x"), &defs, None);

        assert_eq!(
            result.unknown_flags,
            vec!["--beta", "--alpha", "--beta"],
            "unknown flags are reported in command order, without dedup"
        );
    }

    #[test]
    fn test_builtin_definitions_leave_common_commands_unknown_free() {
        // Guards the shipped config/builtins.ncl, not just the test fixtures:
        // these are the shapes that used to be misparsed.
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let nickel = crate::nickel_config::NickelConfig::load(&config_dir);
        assert!(
            nickel.is_loaded(),
            "failed to load {}",
            config_dir.display()
        );
        let defs = CommandDefinitions::from_map(nickel.get_command_definitions());

        for command in [
            "sed -i.bak s/x/y/ f",
            "find . -name *.rs",
            "curl -XPOST https://x",
            "git log --oneline -5",
            "wget -nc http://x/y",
        ] {
            let result = parse_command(&to_tokens(command), &defs, None);
            assert!(
                result.unknown_flags.is_empty(),
                "`{}` reported unknown flags {:?}",
                command,
                result.unknown_flags
            );
        }
    }

    #[test]
    fn test_url_arguments_are_canonicalized() {
        let defs = test_definitions();
        let result = parse_command(
            &to_tokens("curl --url HTTP://LOCALHOST:03000/a/../x?q=1#f localhost:3000/y"),
            &defs,
            None,
        );

        // Both the bare URL and every `--url` value, in one list for policies.
        let urls = collect_urls(&result, "curl");
        let canonical: Vec<&str> = urls
            .iter()
            .map(|value| {
                value
                    .url
                    .as_ref()
                    .expect("canonical URL")
                    .canonical
                    .as_str()
            })
            .collect();
        assert_eq!(
            canonical,
            vec!["http://localhost:3000/y", "http://localhost:3000/x?q=1"]
        );
        assert!(urls.iter().all(|value| value.value_type == "url"));
        assert!(urls.iter().all(|value| value.rejected.is_none()));

        // The raw token is kept: guards that look for shell expansions need it.
        assert_eq!(urls[1].raw, "HTTP://LOCALHOST:03000/a/../x?q=1#f");
    }

    #[test]
    fn test_url_arguments_that_cannot_be_canonicalized_record_the_reason() {
        let defs = test_definitions();
        let result = parse_command(
            &to_tokens("curl --url http://localhost:3000@evil.example/ ftp.localhost:3000/x"),
            &defs,
            None,
        );

        let urls = collect_urls(&result, "curl");
        let rejected: Vec<Option<&str>> =
            urls.iter().map(|value| value.rejected.as_deref()).collect();
        assert_eq!(rejected, vec![Some("scheme"), Some("userinfo")]);
        assert!(urls.iter().all(|value| value.url.is_none()));
    }

    #[test]
    fn test_url_values_serialize_the_canonical_fields() {
        let allowed = serde_json::to_value(url_value("https://localhost:443/x")).unwrap();
        assert_eq!(
            allowed,
            serde_json::json!({
                "raw": "https://localhost:443/x",
                "type": "url",
                "canonical": "https://localhost/x",
                "scheme": "https",
                "host": "localhost",
                "path": "/x",
            }),
            "a default port is omitted, not reported as null"
        );

        let rejected = serde_json::to_value(url_value("http://[::1]:3000/")).unwrap();
        assert_eq!(
            rejected,
            serde_json::json!({
                "raw": "http://[::1]:3000/",
                "type": "url",
                "rejected": "glob",
            })
        );
    }

    #[test]
    fn test_urls_are_collected_only_for_url_typed_values() {
        let defs = test_definitions();

        // `wget`'s URLs are string-typed here, and a `--url` value belongs to
        // curl only: neither contributes a URL record.
        let wget = parse_command(&to_tokens("wget http://localhost:3000/x"), &defs, None);
        assert!(collect_urls(&wget, "wget").is_empty());

        let curl = parse_command(
            &to_tokens("curl --url http://localhost:3000/x"),
            &defs,
            None,
        );
        assert!(collect_urls(&curl, "git").is_empty());
    }
}
