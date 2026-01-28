mod extractor;
mod flags;
mod input;
mod logging;
mod output;
mod paths;
mod policy;
mod tokenizer;

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
use tracing::{debug, error, info};

fn main() {
    let _guard = init_logging();

    let start = Instant::now();

    let result = run();

    let elapsed = start.elapsed();
    debug!(total_ms = elapsed.as_secs_f64() * 1000.0, "Completed");

    match result {
        Ok(output) => {
            println!("{}", output.to_json());
        }
        Err(e) => {
            error!("Error: {}", e);
            // Fail safe: return ask on any error
            println!("{}", HookOutput::ask_with_reason(&e).to_json());
        }
    }
}

fn run() -> Result<HookOutput, String> {
    // Read input from stdin
    let mut input_str = String::new();
    io::stdin()
        .read_to_string(&mut input_str)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    debug!(input = %input_str, "Received input");

    // Parse input JSON
    let hook_input = parse_input(&input_str)
        .map_err(|e| format!("Failed to parse input: {}", e))?;

    // Only handle Bash tool
    if hook_input.tool_name != "Bash" {
        return Ok(HookOutput::ask_with_reason("Not a Bash command"));
    }

    let raw_command = &hook_input.tool_input.command;
    let cwd = hook_input.cwd.clone().unwrap_or_else(|| ".".to_string());
    let cwd_path = PathBuf::from(&cwd);
    let session_id = hook_input.session_id.clone().unwrap_or_default();

    // Tokenize command
    let tokens = tokenizer::tokenize(raw_command)
        .map_err(|e| format!("Failed to tokenize: {}", e))?;

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
        project_root: cwd, // For now, assume cwd is project root
        session_id,
    };

    // Load and evaluate policy
    let compile_start = Instant::now();
    let mut engine = PolicyEngine::new();

    // Load policies from config directory
    let config_dir = dirs::config_dir()
        .map(|d| d.join("claude-permissions"))
        .unwrap_or_else(|| PathBuf::from("/etc/claude-permissions"));

    if config_dir.exists() {
        engine.load_policies_from_dir(&config_dir)?;
    } else {
        info!("Config directory {:?} not found, using defaults", config_dir);
        return Ok(HookOutput::ask_with_reason("No policy configured"));
    }

    let compile_elapsed = compile_start.elapsed();
    debug!(compile_ms = compile_elapsed.as_secs_f64() * 1000.0, "Compiled policies");

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
