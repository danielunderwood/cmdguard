mod cli;
mod command_defs;
mod command_parser;
mod extractor;
mod flags;
mod input;
mod logging;
mod nickel_config;
mod output;
mod paths;
mod policy;
mod resolver;
mod test_runner;
mod tokenizer;
mod parser;

use clap::Parser;
use cli::{Cli, Commands};
use command_defs::CommandDefinitions;
use extractor::extract_command;
use nickel_config::NickelConfig;
use parser::{parse_command, ParsedCommand};
use flags::expand_flags;
use input::parse_input;
use logging::init_logging;
use output::HookOutput;
use paths::detect_paths;
use policy::{PolicyEngine, PolicyInput};
use resolver::{detect_project_root, resolve_command};
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use test_runner::{load_test_file, print_results, TestRunner};
use tracing::{debug, error};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Test {
            file,
            verbose,
            policy_dir,
        }) => {
            run_tests(file, verbose, policy_dir);
        }
        Some(Commands::Eval {
            command,
            cwd,
            policy_dir,
        }) => {
            run_eval(&command, &cwd, policy_dir);
        }
        Some(Commands::Version) => {
            println!("claude-permissions {}", env!("CARGO_PKG_VERSION"));
        }
        None => {
            // Default: run as hook (read from stdin)
            run_hook();
        }
    }
}

fn get_policy_dir(override_dir: Option<PathBuf>) -> PathBuf {
    override_dir.unwrap_or_else(|| {
        // Always use ~/.config/claude-permissions for consistency across platforms
        dirs::home_dir()
            .map(|d| d.join(".config/claude-permissions"))
            .unwrap_or_else(|| PathBuf::from("/etc/claude-permissions"))
    })
}

fn run_tests(file: Option<PathBuf>, verbose: bool, policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);

    // Find test file
    let test_file_path = file.unwrap_or_else(|| policy_dir.join("policy_tests.yaml"));

    if !test_file_path.exists() {
        eprintln!("Test file not found: {:?}", test_file_path);
        eprintln!("Create a test file or specify one with: claude-permissions test <file>");
        std::process::exit(1);
    }

    // Load tests
    let test_file = match load_test_file(&test_file_path) {
        Ok(tf) => tf,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Create runner
    let mut runner = match TestRunner::new(&policy_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading policies: {}", e);
            std::process::exit(1);
        }
    };

    // Run tests
    let results = runner.run_tests(&test_file);
    print_results(&results, verbose);

    // Exit with error if any failed
    if results.iter().any(|r| !r.passed) {
        std::process::exit(1);
    }
}

fn run_eval(command: &str, cwd: &str, policy_dir: Option<PathBuf>) {
    let _guard = init_logging();
    let policy_dir = get_policy_dir(policy_dir);
    let cwd_path = PathBuf::from(cwd);

    // Load Nickel config for custom wrappers and command definitions
    let mut nickel_config = NickelConfig::load(&policy_dir);

    // Load command definitions (built-in + custom from Nickel)
    let mut command_defs = CommandDefinitions::builtin();
    let custom_commands = nickel_config.get_command_definitions();
    if !custom_commands.is_empty() {
        debug!("Merging {} custom command definitions", custom_commands.len());
        command_defs.merge(custom_commands);
    }

    // Detect project root
    let project_root_detected = detect_project_root(&cwd_path);
    let project_root_str = project_root_detected
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.to_string());

    // Parse for compound operators
    let parse_result = parse_command(command);

    println!("=== Compound Command Analysis ===");
    println!("Commands:   {} in chain", parse_result.commands.len());
    println!("Has errors: {}", parse_result.has_errors);

    if parse_result.commands.len() > 1 {
        println!("\nChain breakdown:");
        for (i, cmd) in parse_result.commands.iter().enumerate() {
            let op = cmd.next_operator.as_deref().unwrap_or("(end)");
            println!("  [{}] {} {}", i + 1, cmd.text, op);
        }
    }
    println!();

    // Load engine
    let mut engine = PolicyEngine::new();
    if let Err(e) = engine.load_policies_from_dir(&policy_dir) {
        eprintln!("Error loading policies: {}", e);
        return;
    }

    // Evaluate each command in chain
    println!("=== Per-Command Evaluation ===");
    for cmd in &parse_result.commands {
        println!("\n--- Command {}/{}: {} ---", cmd.position + 1, cmd.chain_length, cmd.text);

        let tokens = match tokenizer::tokenize(&cmd.text) {
            Ok(t) => t,
            Err(e) => {
                println!("Tokenize error: {}", e);
                continue;
            }
        };

        if tokens.is_empty() {
            println!("(empty command)");
            continue;
        }

        let extracted = extract_command(&tokens, Some(&mut nickel_config));
        let flags_expanded = expand_flags(&extracted.command);
        let paths = detect_paths(&extracted.command, &cwd_path);

        // Resolve the command binary and trust zone
        let resolved = if !extracted.command.is_empty() {
            resolve_command(&extracted.command[0], project_root_detected.as_ref().map(|p| p.as_path()))
        } else {
            resolve_command("", None)
        };

        println!("Command:    {:?}", extracted.command);
        if !extracted.wrapper_chain.is_empty() {
            println!("Wrappers:   {:?}", extracted.wrapper_chain);
        }
        if !flags_expanded.is_empty() {
            println!("Flags:      {:?}", flags_expanded);
        }
        if !paths.is_empty() {
            println!("Paths:      {:?}", paths.iter().map(|p| &p.raw).collect::<Vec<_>>());
        }
        println!("Binary:     {}", resolved.binary_name);
        if let Some(path) = &resolved.resolved_path {
            println!("Resolved:   {}", path);
        }
        println!("Trust Zone: {:?}", resolved.resolved_trust_zone);
        if resolved.is_symlink {
            if let Some(source) = &resolved.symlink_source {
                println!("Symlink:    {}", source);
            }
        }

        // Parse command for structured flags and args
        let parsed_cmd = if !extracted.command.is_empty() {
            command_parser::parse_command(
                &extracted.command,
                &command_defs,
                project_root_detected.as_ref().map(|p| p.as_path()),
            )
        } else {
            command_parser::ParsedCommand {
                parsed_flags: std::collections::HashMap::new(),
                positional_args: vec![],
                subcommand: None,
            }
        };

        // Display parsed structure
        println!("Parsed:");

        // Flags
        println!("  Flags:");
        if parsed_cmd.parsed_flags.is_empty() {
            println!("    (none)");
        } else {
            for (name, value) in &parsed_cmd.parsed_flags {
                match value {
                    command_parser::FlagValue::Bool(b) => println!("    {}: {}", name, b),
                    command_parser::FlagValue::String(s) => println!("    {}: \"{}\"", name, s),
                }
            }
        }

        // Positional arguments
        println!("  Positional:");
        if parsed_cmd.positional_args.is_empty() {
            println!("    (none)");
        } else {
            for arg in &parsed_cmd.positional_args {
                println!("    {}:", arg.name);
                for value in &arg.values {
                    if let Some(zone) = &value.trust_zone {
                        println!("      - {} ({}, {})", value.raw, value.value_type, zone);
                    } else {
                        println!("      - {} ({})", value.raw, value.value_type);
                    }
                }
            }
        }

        // Subcommand
        println!("  Subcommand:");
        if let Some(subcommand) = &parsed_cmd.subcommand {
            println!("    {}", subcommand);
        } else {
            println!("    (none)");
        }

        // Serialize to JSON for PolicyInput
        let parsed_flags_json = serde_json::to_value(&parsed_cmd.parsed_flags).ok();
        let positional_args_json = serde_json::to_value(&parsed_cmd.positional_args).ok();

        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: cmd.text.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: cwd.to_string(),
            project_root: project_root_str.clone(),
            session_id: "eval".to_string(),
            chain_position: Some(cmd.position),
            chain_length: Some(cmd.chain_length),
            chain_operator: cmd.next_operator.clone(),
            command_as_typed: Some(resolved.command_as_typed),
            binary_name: Some(resolved.binary_name),
            resolved_path: resolved.resolved_path,
            resolved_trust_zone: Some(format!("{:?}", resolved.resolved_trust_zone).to_lowercase()),
            is_symlink: Some(resolved.is_symlink),
            symlink_source: resolved.symlink_source,
            parsed_flags: parsed_flags_json,
            positional_args: positional_args_json,
            subcommand: parsed_cmd.subcommand,
        };

        let result = engine.evaluate(&policy_input);
        println!("Decision:   {:?}", result.decision);
        if let Some(reason) = result.reason {
            println!("Reason:     {}", reason);
        }
        if let Some(rule) = result.rule {
            println!("Rule:       {}", rule);
        }
        println!("Explicit:   {}", result.explicit);
    }

    // Show final result
    println!("\n=== Final Result (Short-Circuit) ===");
    let final_output = evaluate_compound(
        &parse_result.commands,
        parse_result.has_errors,
        cwd,
        &cwd_path,
        "eval",
        &project_root_str,
        project_root_detected.as_ref(),
        &mut engine,
        &command_defs,
        &mut nickel_config,
    );
    println!("{}", serde_json::to_string_pretty(&final_output).unwrap_or_default());
}

fn run_hook() {
    let _guard = init_logging();
    let start = Instant::now();

    let result = run_hook_inner();

    let elapsed = start.elapsed();
    debug!(total_ms = elapsed.as_secs_f64() * 1000.0, "Completed");

    match result {
        Ok(output) => {
            println!("{}", output.to_json());
        }
        Err(e) => {
            error!("Error: {}", e);
            println!("{}", HookOutput::ask_with_reason(&e).to_json());
        }
    }
}

/// Evaluate a compound command, short-circuiting on first non-allow
fn evaluate_compound(
    parsed: &[ParsedCommand],
    has_parse_errors: bool,
    cwd: &str,
    cwd_path: &PathBuf,
    session_id: &str,
    project_root: &str,
    project_root_path: Option<&PathBuf>,
    engine: &mut PolicyEngine,
    command_defs: &CommandDefinitions,
    nickel_config: &mut NickelConfig,
) -> HookOutput {
    // If parsing had errors, be conservative and ask
    if has_parse_errors {
        return HookOutput::ask_with_reason("Command contains unparseable constructs");
    }

    for cmd in parsed {
        // Tokenize this individual command
        let tokens = match tokenizer::tokenize(&cmd.text) {
            Ok(t) if !t.is_empty() => t,
            _ => continue, // Skip empty/invalid
        };

        // Extract from wrappers
        let extracted = extract_command(&tokens, Some(nickel_config));
        if extracted.command.is_empty() {
            continue;
        }

        // Expand flags
        let flags_expanded = expand_flags(&extracted.command);

        // Detect paths
        let paths = detect_paths(&extracted.command, cwd_path);

        // Resolve the command binary and trust zone
        let resolved = if !extracted.command.is_empty() {
            resolve_command(&extracted.command[0], project_root_path.map(|p| p.as_path()))
        } else {
            resolve_command("", None)
        };

        // Parse command for structured flags and args
        let parsed_cmd = if !extracted.command.is_empty() {
            command_parser::parse_command(
                &extracted.command,
                command_defs,
                project_root_path.map(|p| p.as_path()),
            )
        } else {
            command_parser::ParsedCommand {
                parsed_flags: std::collections::HashMap::new(),
                positional_args: vec![],
                subcommand: None,
            }
        };

        // Serialize to JSON for PolicyInput
        let parsed_flags_json = serde_json::to_value(&parsed_cmd.parsed_flags).ok();
        let positional_args_json = serde_json::to_value(&parsed_cmd.positional_args).ok();

        // Build policy input with chain info
        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: cmd.text.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: cwd.to_string(),
            project_root: project_root.to_string(),
            session_id: session_id.to_string(),
            chain_position: Some(cmd.position),
            chain_length: Some(cmd.chain_length),
            chain_operator: cmd.next_operator.clone(),
            command_as_typed: Some(resolved.command_as_typed),
            binary_name: Some(resolved.binary_name),
            resolved_path: resolved.resolved_path,
            resolved_trust_zone: Some(format!("{:?}", resolved.resolved_trust_zone).to_lowercase()),
            is_symlink: Some(resolved.is_symlink),
            symlink_source: resolved.symlink_source,
            parsed_flags: parsed_flags_json,
            positional_args: positional_args_json,
            subcommand: parsed_cmd.subcommand,
        };

        // Evaluate
        let result = engine.evaluate(&policy_input);

        // Short-circuit on non-allow
        match result.decision {
            output::Decision::Allow => continue,
            output::Decision::Deny => {
                let reason = result.reason.unwrap_or_else(|| {
                    format!("Denied at command {} of {}", cmd.position + 1, cmd.chain_length)
                });
                return HookOutput::deny(&reason);
            }
            output::Decision::Ask => {
                let reason = result.reason.unwrap_or_else(|| {
                    format!("Review needed for command {} of {}", cmd.position + 1, cmd.chain_length)
                });
                return HookOutput::ask_with_reason(&reason);
            }
        }
    }

    // All commands allowed
    HookOutput::new(output::Decision::Allow, None)
}

fn run_hook_inner() -> Result<HookOutput, String> {
    // Read input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    debug!(input = %input_str, "Received input");

    // Parse input JSON
    let hook_input =
        parse_input(&input_str).map_err(|e| format!("Failed to parse input: {}", e))?;

    // Only handle Bash tool
    if hook_input.tool_name != "Bash" {
        return Ok(HookOutput::ask_with_reason("Not a Bash command"));
    }

    let raw_command = &hook_input.tool_input.command;
    let cwd = hook_input.cwd.clone().unwrap_or_else(|| ".".to_string());
    let cwd_path = PathBuf::from(&cwd);
    let session_id = hook_input.session_id.clone().unwrap_or_default();

    // Load policy engine and Nickel config
    let policy_dir = get_policy_dir(None);

    // Load Nickel config for custom wrappers and command definitions
    let mut nickel_config = NickelConfig::load(&policy_dir);

    // Load command definitions (built-in + custom from Nickel)
    let mut command_defs = CommandDefinitions::builtin();
    let custom_commands = nickel_config.get_command_definitions();
    if !custom_commands.is_empty() {
        debug!("Merging {} custom command definitions", custom_commands.len());
        command_defs.merge(custom_commands);
    }

    // Detect project root
    let project_root_detected = detect_project_root(&cwd_path);
    let project_root_str = project_root_detected
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.clone());

    // Parse command for compound operators
    let parse_result = parse_command(raw_command);
    debug!(
        commands = ?parse_result.commands,
        has_errors = parse_result.has_errors,
        "Parsed command"
    );

    // Load policy engine
    let mut engine = PolicyEngine::new();
    engine
        .load_policies_from_dir(&policy_dir)
        .map_err(|e| format!("Failed to load policies: {}", e))?;

    // Evaluate compound command
    Ok(evaluate_compound(
        &parse_result.commands,
        parse_result.has_errors,
        &cwd,
        &cwd_path,
        &session_id,
        &project_root_str,
        project_root_detected.as_ref(),
        &mut engine,
        &command_defs,
        &mut nickel_config,
    ))
}
