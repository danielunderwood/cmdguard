mod base_sync;
mod cli;
mod command_defs;
mod command_evaluator;
mod command_parser;
mod extractor;
mod flags;
mod hook;
mod input;
mod logging;
mod nickel_config;
mod output;
mod parser;
mod paths;
mod policy;
mod python_analyzer;
mod query;
mod resolver;
mod test_runner;
mod tokenizer;

use clap::Parser;
use cli::{Cli, Commands};
use command_defs::CommandDefinitions;
use command_evaluator::{CommandEvaluator, EvaluationContext};
use extractor::extract_command;
use flags::expand_flags;
use input::parse_input;
use logging::init_logging;
use nickel_config::NickelConfig;
use output::HookOutput;
use parser::parse_command;
use paths::detect_paths;
use policy::{PatternInput, PolicyEngine, PolicyInput, PythonAnalysisInput};
use resolver::{detect_project_root, resolve_command};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
            show_input,
        }) => {
            run_eval(&command, &cwd, policy_dir, show_input);
        }
        Some(Commands::Validate { policy_dir }) => {
            run_validate(policy_dir);
        }
        Some(Commands::AnalyzePython { code }) => {
            run_analyze_python(&code);
        }
        Some(Commands::Query {
            lang,
            query,
            query_file,
            code,
            file,
        }) => {
            run_query(&lang, query, query_file, code, file);
        }
        Some(Commands::Version) => {
            println!("cmdguard {}", env!("CARGO_PKG_VERSION"));
        }
        Some(Commands::Hook { action }) => match action {
            cli::HookAction::Run => run_hook(),
            other => hook::run(other),
        },
        Some(Commands::Base { action }) => match action {
            cli::BaseAction::Sync => base_sync::run(get_policy_dir(None)),
        },
        Some(Commands::Status { policy_dir }) => {
            run_status(policy_dir);
        }
        None => {
            // No subcommand: print clap help to stderr and exit non-zero
            // so callers notice. Reading stdin in the no-arg case used to
            // be load-bearing for the hook integration, but that path is
            // now explicit (`cmdguard hook run`). Help goes to stderr to
            // keep stdout clean for scripts that pipe a wanted command's
            // output but accidentally invoke cmdguard with no args.
            use clap::CommandFactory;
            let _ = Cli::command().write_help(&mut std::io::stderr());
            eprintln!();
            std::process::exit(2);
        }
    }
}

/// List `.rego` filenames in `dir`, sorted, returning Err if `dir` exists but
/// can't be read. A non-existent directory returns an empty list.
fn read_rego_filenames(dir: &Path) -> std::io::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rego") {
                path.file_name().map(|n| n.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

fn run_status(policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);
    let base_dir = policy_dir.join("base");
    let policies_dir = policy_dir.join("policies");

    println!("Policy directory: {}", policy_dir.display());
    println!();

    // List loaded files (sorted for deterministic output)
    println!("Loaded policy files:");
    let mut file_count = 0;

    for (label, dir) in [("base", &base_dir), ("policies", &policies_dir)] {
        let names = match read_rego_filenames(dir) {
            Ok(names) => names,
            Err(e) => {
                eprintln!("Warning: failed to list {}: {}", dir.display(), e);
                continue;
            }
        };
        for name in names {
            println!("  {}/{}", label, name);
            file_count += 1;
        }
    }

    // Fallback: flat directory (legacy layout)
    if file_count == 0 && policy_dir.exists() {
        match read_rego_filenames(&policy_dir) {
            Ok(names) => {
                for name in names {
                    println!("  {}", name);
                    file_count += 1;
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to list {}: {}", policy_dir.display(), e);
            }
        }
    }

    if file_count == 0 {
        println!("  (none)");
    }
    println!();

    // Load engine and query tables
    let mut engine = PolicyEngine::new();
    if let Err(e) = load_all_policies(&mut engine, &policy_dir, None) {
        eprintln!("Error loading policies: {}", e);
        return;
    }

    // Show tables
    println!("Tables:");
    let allowed = engine.query_allowed_subcommands();
    if !allowed.is_empty() {
        print!("  allowed_subcommands: ");
        let entries: Vec<String> = allowed
            .iter()
            .map(|(binary, subcmds)| format!("{}({})", binary, subcmds.len()))
            .collect();
        println!("{}", entries.join(", "));
    }

    let denied = engine.query_denied_subcommands();
    if !denied.is_empty() {
        print!("  denied_subcommands: ");
        let entries: Vec<String> = denied
            .iter()
            .map(|(binary, subcmds)| format!("{}({})", binary, subcmds.len()))
            .collect();
        println!("{}", entries.join(", "));
    }

    if allowed.is_empty() && denied.is_empty() {
        println!("  (none)");
    }
    println!();
}

fn run_validate(policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);
    let ncl_path = policy_dir.join("commands.ncl");

    println!("Validating: {}", ncl_path.display());
    println!();

    let result = NickelConfig::validate(&policy_dir);

    if !result.errors.is_empty() {
        println!("Errors:");
        for error in &result.errors {
            println!("  - {}", error);
        }
        println!();
    }

    if !result.warnings.is_empty() {
        println!("Warnings:");
        for warning in &result.warnings {
            println!("  - {}", warning);
        }
        println!();
    }

    if !result.wrappers.is_empty() {
        println!("Wrappers defined: {}", result.wrappers.join(", "));
    }

    if !result.commands.is_empty() {
        println!("Commands defined: {}", result.commands.join(", "));
    }

    if result.valid {
        println!();
        println!("Config is valid.");
    } else {
        println!();
        println!("Config has errors.");
        std::process::exit(1);
    }
}

fn run_analyze_python(code: &str) {
    match python_analyzer::analyze(code) {
        Ok(result) => {
            println!("=== Python Analysis ===");
            println!("Code: {}", code);
            println!();
            println!("Imports found: {:?}", result.imports);
            println!();

            if result.patterns.is_empty() {
                println!("Matched patterns: none");
            } else {
                println!("Matched patterns:");
                for pattern in &result.patterns {
                    println!(
                        "  - @{} \"{}\" at line {}:{}",
                        pattern.capture, pattern.text, pattern.line, pattern.column
                    );
                }
            }
            println!();
            println!("Safe for inspection mode: {}", result.is_inspection_safe);
        }
        Err(e) => {
            eprintln!("Error analyzing Python code: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_query(
    lang: &str,
    query_arg: Option<String>,
    query_file: Option<PathBuf>,
    code_arg: Option<String>,
    code_file: Option<PathBuf>,
) {
    // Parse language
    let language = match query::QueryLanguage::from_str(lang) {
        Some(l) => l,
        None => {
            eprintln!("Unsupported language: {}", lang);
            eprintln!("Supported: python, bash");
            std::process::exit(1);
        }
    };

    // Get query string
    let query_str = match (query_arg, query_file) {
        (Some(q), _) => q,
        (_, Some(path)) => match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read query file {:?}: {}", path, e);
                std::process::exit(1);
            }
        },
        (None, None) => {
            eprintln!("Must provide either --query or --query-file");
            std::process::exit(1);
        }
    };

    // Get code to analyze
    let code = match (code_arg, code_file) {
        (Some(c), _) => c,
        (_, Some(path)) => match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read code file {:?}: {}", path, e);
                std::process::exit(1);
            }
        },
        (None, None) => {
            eprintln!("Must provide either code argument or --file");
            std::process::exit(1);
        }
    };

    // Run query
    match query::run_query(language, &query_str, &code) {
        Ok(matches) => {
            println!("=== Query Results ===");
            println!("Language: {:?}", language);
            println!("Matches:  {}", matches.len());
            println!();

            if matches.is_empty() {
                println!("No matches found.");
            } else {
                for m in &matches {
                    println!(
                        "  @{} \"{}\" at line {}:{}",
                        m.capture, m.text, m.line, m.column
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Query error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Check if this is a python -c command and analyze the code if so
fn analyze_python_if_applicable(
    binary_name: &str,
    command: &[String],
) -> Option<PythonAnalysisInput> {
    // Check if this is a python command
    if !binary_name.starts_with("python") {
        return None;
    }

    // Look for -c flag directly in command tokens (Python has no long form)
    let code = extract_python_c_code(command)?;

    debug!(code = %code, "Analyzing python -c code");

    // Run Python analyzer
    match python_analyzer::analyze(&code) {
        Ok(analysis) => {
            let patterns: Vec<PatternInput> =
                analysis.patterns.iter().map(PatternInput::from).collect();

            Some(PythonAnalysisInput {
                patterns,
                imports: analysis.imports,
                is_inspection_safe: analysis.is_inspection_safe,
            })
        }
        Err(e) => {
            debug!(error = %e, "Failed to analyze Python code");
            None
        }
    }
}

/// Extract code from python -c command
fn extract_python_c_code(command: &[String]) -> Option<String> {
    let mut iter = command.iter();
    while let Some(arg) = iter.next() {
        if arg == "-c" {
            return iter.next().cloned();
        }
    }
    None
}

fn get_policy_dir(override_dir: Option<PathBuf>) -> PathBuf {
    override_dir.unwrap_or_else(|| {
        // Always use ~/.config/cmdguard for consistency across platforms
        dirs::home_dir()
            .map(|d| d.join(".config/cmdguard"))
            .unwrap_or_else(|| PathBuf::from("/etc/cmdguard"))
    })
}

/// Get the project-local policy directory if it exists
fn get_project_policy_dir(project_root: Option<&PathBuf>) -> Option<PathBuf> {
    project_root
        .map(|root| root.join(".cmdguard"))
        .filter(|p| p.exists())
}

/// Load policies from global directory and optionally from project-local directory
fn load_all_policies(
    engine: &mut PolicyEngine,
    global_dir: &PathBuf,
    project_root: Option<&PathBuf>,
) -> Result<(), String> {
    engine.load_policies_with_layout(global_dir)?;

    // Load project-local policies if they exist (they merge with global via shared 'rules' map)
    if let Some(project_policy_dir) = get_project_policy_dir(project_root) {
        debug!(?project_policy_dir, "Loading project-local policies");
        engine.load_policies_from_dir(&project_policy_dir)?;
    }

    Ok(())
}

fn run_tests(file: Option<PathBuf>, verbose: bool, policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);

    // Find test file
    let test_file_path = file.unwrap_or_else(|| policy_dir.join("policy_tests.yaml"));

    if !test_file_path.exists() {
        eprintln!("Test file not found: {:?}", test_file_path);
        eprintln!("Create a test file or specify one with: cmdguard test <file>");
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

fn run_eval(command: &str, cwd: &str, policy_dir: Option<PathBuf>, show_input: bool) {
    let _guard = init_logging();
    let policy_dir = get_policy_dir(policy_dir);
    let cwd_path = PathBuf::from(cwd);

    // Load Nickel config for custom wrappers and command definitions
    let mut nickel_config = NickelConfig::load(&policy_dir);

    // Load command definitions (built-in + custom from Nickel)
    let mut command_defs = CommandDefinitions::builtin();
    let custom_commands = nickel_config.get_command_definitions();
    if !custom_commands.is_empty() {
        debug!(
            "Merging {} custom command definitions",
            custom_commands.len()
        );
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

    // Load engine with global and project-local policies
    let mut engine = PolicyEngine::new();
    if let Err(e) = load_all_policies(&mut engine, &policy_dir, project_root_detected.as_ref()) {
        eprintln!("Error loading policies: {}", e);
        return;
    }

    // Evaluate each command in chain
    println!("=== Per-Command Evaluation ===");
    let mut prev_operator: Option<String> = None;
    for cmd in &parse_result.commands {
        println!(
            "\n--- Command {}/{}: {} ---",
            cmd.position + 1,
            cmd.chain_length,
            cmd.text
        );
        let this_prev_operator = prev_operator.clone();
        prev_operator = cmd.next_operator.clone();

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
            resolve_command(
                &extracted.command[0],
                project_root_detected.as_ref().map(|p| p.as_path()),
            )
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
            println!(
                "Paths:      {:?}",
                paths.iter().map(|p| &p.raw).collect::<Vec<_>>()
            );
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
                    command_parser::FlagValue::Array(arr) => println!("    {}: {:?}", name, arr),
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
        let positional_map_json = serde_json::to_value(&parsed_cmd.positional_as_map()).ok();

        // Check for python -c and analyze inline code
        let python_analysis =
            analyze_python_if_applicable(&resolved.binary_name, &extracted.command);

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
            prev_operator: this_prev_operator,
            command_as_typed: Some(resolved.command_as_typed),
            binary_name: Some(resolved.binary_name),
            resolved_path: resolved.resolved_path,
            resolved_trust_zone: Some(format!("{:?}", resolved.resolved_trust_zone).to_lowercase()),
            is_symlink: Some(resolved.is_symlink),
            symlink_source: resolved.symlink_source,
            parsed_flags: parsed_flags_json,
            positional_args: positional_args_json,
            positional: positional_map_json,
            subcommand: parsed_cmd.subcommand,
            python_analysis,
        };

        // Show Rego input if requested
        if show_input {
            println!("Rego Input:");
            if let Ok(json) = serde_json::to_string_pretty(&policy_input) {
                println!("{}", json);
            }
        }

        let result = engine.evaluate(&policy_input);
        println!("Decision:   {:?}", result.decision);
        if let Some(reason) = &result.reason {
            println!("Reason:     {}", reason);
        }
        if let Some(rule) = &result.rule {
            println!("Rule:       {}", rule);
        }
        println!("Explicit:   {}", result.explicit);

        // Show all matching rules for debugging
        let all_rules = engine.evaluate_all_rules(&policy_input);
        if all_rules.len() > 1 {
            println!();
            println!("Also matched:");
            for other_rule in &all_rules {
                // Skip the winning rule (already displayed)
                if other_rule.rule == result.rule {
                    continue;
                }

                if let (Some(rule_name), Some(reason)) = (&other_rule.rule, &other_rule.reason) {
                    println!("  {} ({:?}) — {}", rule_name, other_rule.decision, reason);
                }
            }
        }
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
    println!(
        "{}",
        serde_json::to_string_pretty(&final_output).unwrap_or_default()
    );
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
    parsed: &[parser::ParsedCommand],
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
    let mut evaluator = CommandEvaluator::new(engine, command_defs, nickel_config);

    let context = EvaluationContext {
        cwd,
        cwd_path,
        session_id,
        project_root_str: project_root,
        project_root_path: project_root_path.map(|p| p.as_path()),
    };

    evaluator.evaluate_compound(parsed, has_parse_errors, &context)
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
        debug!(
            "Merging {} custom command definitions",
            custom_commands.len()
        );
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

    // Load policy engine with global and project-local policies
    let mut engine = PolicyEngine::new();
    load_all_policies(&mut engine, &policy_dir, project_root_detected.as_ref())
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
