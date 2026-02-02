mod cli;
mod extractor;
mod flags;
mod input;
mod logging;
mod output;
mod paths;
mod policy;
mod test_runner;
mod tokenizer;

use clap::Parser;
use cli::{Cli, Commands};
use extractor::extract_command;
use flags::expand_flags;
use input::parse_input;
use logging::init_logging;
use output::HookOutput;
use paths::detect_paths;
use policy::{PolicyEngine, PolicyInput};
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use test_runner::{load_test_file, print_results, TestRunner};
use tracing::{debug, error, info};

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

    // Process command
    let tokens = match tokenizer::tokenize(command) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tokenize error: {}", e);
            std::process::exit(1);
        }
    };

    let extracted = extract_command(&tokens);
    let flags_expanded = expand_flags(&extracted.command);
    let cwd_path = PathBuf::from(cwd);
    let paths = detect_paths(&extracted.command, &cwd_path);

    let policy_input = PolicyInput {
        tool: "Bash".to_string(),
        raw_command: command.to_string(),
        command: extracted.command.clone(),
        wrapper_chain: extracted.wrapper_chain.clone(),
        flags_expanded: flags_expanded.clone(),
        paths: paths.clone(),
        cwd: cwd.to_string(),
        project_root: cwd.to_string(),
        session_id: "eval".to_string(),
    };

    // Load and evaluate
    let mut engine = PolicyEngine::new();
    if let Err(e) = engine.load_policies_from_dir(&policy_dir) {
        eprintln!("Error loading policies: {}", e);
        std::process::exit(1);
    }

    let result = engine.evaluate(&policy_input);

    // Print results
    println!("Command:    {}", command);
    println!("Extracted:  {:?}", extracted.command);
    if !extracted.wrapper_chain.is_empty() {
        println!("Wrappers:   {:?}", extracted.wrapper_chain);
    }
    if !flags_expanded.is_empty() {
        println!("Flags:      {:?}", flags_expanded);
    }
    if !paths.is_empty() {
        println!("Paths:      {:?}", paths.iter().map(|p| &p.raw).collect::<Vec<_>>());
    }
    println!();
    println!("Decision:   {:?}", result.decision);
    if let Some(reason) = result.reason {
        println!("Reason:     {}", reason);
    }
    if let Some(rule) = result.rule {
        println!("Rule:       {}", rule);
    }
    println!("Explicit:   {}", result.explicit);
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

    // Tokenize command
    let tokens =
        tokenizer::tokenize(raw_command).map_err(|e| format!("Failed to tokenize: {}", e))?;

    if tokens.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty command"));
    }

    // Extract from wrappers
    let extracted = extract_command(&tokens);
    debug!(
        raw = ?tokens,
        extracted = ?extracted.command,
        wrappers = ?extracted.wrapper_chain,
        "Extracted command"
    );

    if extracted.command.is_empty() {
        return Ok(HookOutput::ask_with_reason("Empty extracted command"));
    }

    // Expand flags
    let flags_expanded = expand_flags(&extracted.command);
    debug!(flags = ?flags_expanded, "Expanded flags");

    // Detect paths
    let paths = detect_paths(&extracted.command, &cwd_path);
    debug!(paths = ?paths, "Detected paths");

    // Build policy input
    let policy_input = PolicyInput {
        tool: hook_input.tool_name,
        raw_command: raw_command.clone(),
        command: extracted.command,
        wrapper_chain: extracted.wrapper_chain,
        flags_expanded,
        paths,
        cwd: cwd.clone(),
        project_root: cwd,
        session_id,
    };

    // Load and evaluate policy
    let compile_start = Instant::now();
    let mut engine = PolicyEngine::new();

    let config_dir = get_policy_dir(None);

    if config_dir.exists() {
        engine.load_policies_from_dir(&config_dir)?;
    } else {
        info!("Config directory {:?} not found, using defaults", config_dir);
        return Ok(HookOutput::ask_with_reason("No policy configured"));
    }

    let compile_elapsed = compile_start.elapsed();
    debug!(
        compile_ms = compile_elapsed.as_secs_f64() * 1000.0,
        "Compiled policies"
    );

    // Evaluate
    let eval_start = Instant::now();
    let result = engine.evaluate(&policy_input);
    let eval_elapsed = eval_start.elapsed();

    info!(
        decision = ?result.decision,
        reason = ?result.reason,
        compile_ms = compile_elapsed.as_secs_f64() * 1000.0,
        eval_ms = eval_elapsed.as_secs_f64() * 1000.0,
        command = ?policy_input.command,
        "Policy evaluation complete"
    );

    Ok(HookOutput::new(result.decision, result.reason))
}
