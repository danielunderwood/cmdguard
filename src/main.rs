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

// Embedded base policy files
mod embedded {
    pub const STDLIB_REGO: &str = include_str!("../config/stdlib.rego");
    pub const SAFE_REGO: &str = include_str!("../config/safe.rego");
    pub const GIT_REGO: &str = include_str!("../config/git.rego");
    pub const RUST_REGO: &str = include_str!("../config/rust.rego");
    pub const GO_REGO: &str = include_str!("../config/go.rego");
    pub const PYTHON_REGO: &str = include_str!("../config/python.rego");
    pub const JAVASCRIPT_REGO: &str = include_str!("../config/javascript.rego");
    pub const GH_REGO: &str = include_str!("../config/gh.rego");
    pub const KUBECTL_REGO: &str = include_str!("../config/kubectl.rego");
    pub const FIND_REGO: &str = include_str!("../config/find.rego");
    pub const DOCKER_REGO: &str = include_str!("../config/docker.rego");
    pub const FILE_OPS_REGO: &str = include_str!("../config/file-ops.rego");
    pub const NETWORK_REGO: &str = include_str!("../config/network.rego");
    pub const SED_REGO: &str = include_str!("../config/sed.rego");
    pub const INPROJECT_REGO: &str = include_str!("../config/inproject.rego");
    pub const TOOLS_REGO: &str = include_str!("../config/tools.rego");
    pub const BUILTINS_NCL: &str = include_str!("../config/builtins.ncl");

    pub const BASE_FILES: &[(&str, &str)] = &[
        ("stdlib.rego", STDLIB_REGO),
        ("safe.rego", SAFE_REGO),
        ("git.rego", GIT_REGO),
        ("rust.rego", RUST_REGO),
        ("go.rego", GO_REGO),
        ("python.rego", PYTHON_REGO),
        ("javascript.rego", JAVASCRIPT_REGO),
        ("gh.rego", GH_REGO),
        ("kubectl.rego", KUBECTL_REGO),
        ("find.rego", FIND_REGO),
        ("docker.rego", DOCKER_REGO),
        ("file-ops.rego", FILE_OPS_REGO),
        ("network.rego", NETWORK_REGO),
        ("sed.rego", SED_REGO),
        ("inproject.rego", INPROJECT_REGO),
        ("tools.rego", TOOLS_REGO),
    ];
}

use clap::Parser;
use cli::{Cli, Commands};
use command_defs::CommandDefinitions;
use command_evaluator::{CommandEvaluator, EvaluationContext};
use extractor::extract_command;
use flags::expand_flags;
use nickel_config::NickelConfig;
use parser::parse_command;
use input::parse_input;
use logging::init_logging;
use output::HookOutput;
use paths::detect_paths;
use policy::{PatternInput, PolicyEngine, PolicyInput, PythonAnalysisInput};
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
        Some(Commands::Hook { action }) => {
            hook::run(action);
        }
        Some(Commands::Base { action }) => match action {
            cli::BaseAction::Sync => run_base_sync(),
        },
        Some(Commands::Status { policy_dir }) => {
            run_status(policy_dir);
        }
        None => {
            // Default: run as hook (read from stdin)
            run_hook();
        }
    }
}

fn run_base_sync() {
    let config_dir = get_policy_dir(None);
    let base_dir = config_dir.join("base");

    // Create base directory with restricted permissions
    std::fs::create_dir_all(&base_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create base directory: {}", e);
        std::process::exit(1);
    });

    // Make base directory writable for re-sync
    #[cfg(unix)]
    if base_dir.exists() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base_dir, std::fs::Permissions::from_mode(0o755));
    }

    println!("Syncing base policies to {}", base_dir.display());
    println!();

    // Write each base file
    for (filename, contents) in embedded::BASE_FILES {
        let path = base_dir.join(filename);

        // Make file writable if it exists (for re-sync)
        #[cfg(unix)]
        if path.exists() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
        }

        std::fs::write(&path, contents).unwrap_or_else(|e| {
            eprintln!("Failed to write {}: {}", filename, e);
            std::process::exit(1);
        });

        // Set read-only permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444));
        }
        println!("  {}", filename);
    }

    // Write builtins.ncl
    let builtins_path = base_dir.join("builtins.ncl");

    // Make file writable if it exists (for re-sync)
    #[cfg(unix)]
    if builtins_path.exists() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&builtins_path, std::fs::Permissions::from_mode(0o644));
    }

    std::fs::write(&builtins_path, embedded::BUILTINS_NCL).unwrap_or_else(|e| {
        eprintln!("Failed to write builtins.ncl: {}", e);
        std::process::exit(1);
    });

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&builtins_path, std::fs::Permissions::from_mode(0o444));
    }
    println!("  builtins.ncl");

    // Set base directory permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base_dir, std::fs::Permissions::from_mode(0o555));
    }

    // Create policies directory if it doesn't exist
    let policies_dir = config_dir.join("policies");
    if !policies_dir.exists() {
        std::fs::create_dir_all(&policies_dir).unwrap_or_else(|e| {
            eprintln!("Failed to create policies directory: {}", e);
            std::process::exit(1);
        });

        // Create starter custom.rego
        let custom_path = policies_dir.join("custom.rego");
        let custom_content = r#"package cmdguard

import rego.v1

# Add your custom rules here. These override base rules via priority.
#
# Examples:
#
# Deny a subcommand that base allows:
#   denied_subcommands["git"] := {"push"}
#
# Add an allow rule for a tool not in base:
#   allowed_with_args["make"] := {"build", "test", "clean"}
#
# Add a conditional rule:
#   rules["my_rule"] := ask("Please confirm") if {
#       input.binary_name == "dangerous-tool"
#   }
"#;
        std::fs::write(&custom_path, custom_content).unwrap_or_else(|e| {
            eprintln!("Failed to write custom.rego: {}", e);
            std::process::exit(1);
        });
        println!("  policies/custom.rego (starter template)");
    }

    println!();
    println!("Base policies synced to {}", base_dir.display());
}

fn run_status(policy_dir: Option<PathBuf>) {
    let policy_dir = get_policy_dir(policy_dir);
    let base_dir = policy_dir.join("base");
    let policies_dir = policy_dir.join("policies");

    println!("Policy directory: {}", policy_dir.display());
    println!();

    // List loaded files
    println!("Loaded policy files:");
    let mut file_count = 0;

    for dir_info in [("base", &base_dir), ("policies", &policies_dir)] {
        let (label, dir) = dir_info;
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("rego") {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        println!("  {}/{}", label, name);
                        file_count += 1;
                    }
                }
            }
        }
    }

    // Fallback: flat directory
    if file_count == 0 && policy_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&policy_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("rego") {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    println!("  {}", name);
                    file_count += 1;
                }
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
        let entries: Vec<String> = allowed.iter()
            .map(|(binary, subcmds)| format!("{}({})", binary, subcmds.len()))
            .collect();
        println!("{}", entries.join(", "));
    }

    let denied = engine.query_denied_subcommands();
    if !denied.is_empty() {
        print!("  denied_subcommands: ");
        let entries: Vec<String> = denied.iter()
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
                    println!("  - @{} \"{}\" at line {}:{}", pattern.capture, pattern.text, pattern.line, pattern.column);
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
                    println!("  @{} \"{}\" at line {}:{}", m.capture, m.text, m.line, m.column);
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
            let patterns: Vec<PatternInput> = analysis
                .patterns
                .iter()
                .map(PatternInput::from)
                .collect();

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
    project_root.map(|root| root.join(".cmdguard")).filter(|p| p.exists())
}

/// Load policies from global directory and optionally from project-local directory
fn load_all_policies(
    engine: &mut PolicyEngine,
    global_dir: &PathBuf,
    project_root: Option<&PathBuf>,
) -> Result<(), String> {
    let base_dir = global_dir.join("base");
    let policies_dir = global_dir.join("policies");

    if base_dir.exists() {
        // New structure: base/ + policies/
        debug!("Using new base/policies structure");
        engine.load_policies_from_dir(&base_dir)?;
        if policies_dir.exists() {
            engine.load_policies_from_dir(&policies_dir)?;
        }
    } else {
        // Legacy flat structure: all .rego in config dir
        debug!("Using legacy flat structure");
        engine.load_policies_from_dir(global_dir)?;
    }

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

    // Load engine with global and project-local policies
    let mut engine = PolicyEngine::new();
    if let Err(e) = load_all_policies(&mut engine, &policy_dir, project_root_detected.as_ref()) {
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
        let python_analysis = analyze_python_if_applicable(&resolved.binary_name, &extracted.command);

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
