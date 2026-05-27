//! Shared command evaluation logic used by both the main hook and test runner.
//!
//! This module provides a single source of truth for command evaluation,
//! ensuring that tests and production code behave identically.

use crate::command_defs::CommandDefinitions;
use crate::command_parser;
use crate::extractor::extract_command;
use crate::flags::expand_flags;
use crate::nickel_config::NickelConfig;
use crate::output::{Decision, HookOutput};
use crate::parser::ParsedCommand;
use crate::paths::detect_paths;
use crate::policy::{PatternInput, PolicyEngine, PolicyInput, PolicyResult, PythonAnalysisInput};
use crate::python_analyzer;
use crate::resolver::resolve_command;
use crate::tokenizer;
use std::path::Path;
use tracing::debug;

/// Context for evaluating commands - encapsulates shared dependencies
pub struct CommandEvaluator<'a> {
    engine: &'a mut PolicyEngine,
    command_defs: &'a CommandDefinitions,
    nickel_config: &'a mut NickelConfig,
}

/// Configuration for how to evaluate commands
pub struct EvaluationContext<'a> {
    pub cwd: &'a str,
    pub cwd_path: &'a Path,
    pub session_id: &'a str,
    pub project_root_str: &'a str,
    pub project_root_path: Option<&'a Path>,
}

impl<'a> CommandEvaluator<'a> {
    /// Create a new evaluator with the given dependencies
    pub fn new(
        engine: &'a mut PolicyEngine,
        command_defs: &'a CommandDefinitions,
        nickel_config: &'a mut NickelConfig,
    ) -> Self {
        Self {
            engine,
            command_defs,
            nickel_config,
        }
    }

    /// Evaluate a single parsed command and return the policy result.
    /// `prev_operator` is the operator (e.g. "|", "&&") that connected the
    /// previous command in the chain to this one — used by rules that care
    /// whether stdin is a pipe.
    pub fn evaluate_single(
        &mut self,
        cmd: &ParsedCommand,
        context: &EvaluationContext,
        prev_operator: Option<String>,
    ) -> PolicyResult {
        // Tokenize
        let tokens = match tokenizer::tokenize(&cmd.text) {
            Ok(t) if !t.is_empty() => t,
            _ => {
                return PolicyResult {
                    decision: Decision::Ask,
                    reason: Some("Failed to tokenize command".to_string()),
                    rule: None,
                    explicit: false,
                };
            }
        };

        // Extract from wrappers
        let extracted = extract_command(&tokens, Some(self.nickel_config));
        if extracted.command.is_empty() {
            return PolicyResult {
                decision: Decision::Allow,
                reason: Some("Empty command after extraction".to_string()),
                rule: None,
                explicit: false,
            };
        }

        // Expand flags
        let flags_expanded = expand_flags(&extracted.command);

        // Detect paths
        let paths = detect_paths(&extracted.command, context.cwd_path);

        // Resolve command binary and trust zone
        let resolved = resolve_command(&extracted.command[0], context.project_root_path);

        // Parse command for structured flags and args
        let parsed_cmd = command_parser::parse_command(
            &extracted.command,
            self.command_defs,
            context.project_root_path,
        );

        // Serialize to JSON for PolicyInput
        let parsed_flags_json = serde_json::to_value(&parsed_cmd.parsed_flags).ok();
        let positional_args_json = serde_json::to_value(&parsed_cmd.positional_args).ok();
        let positional_map_json = serde_json::to_value(&parsed_cmd.positional_as_map()).ok();

        // Check for python -c and analyze inline code
        let python_analysis =
            self.analyze_python_if_applicable(&resolved.binary_name, &extracted.command);

        // Build policy input
        let policy_input = PolicyInput {
            tool: "Bash".to_string(),
            raw_command: cmd.text.clone(),
            command: extracted.command,
            wrapper_chain: extracted.wrapper_chain,
            flags_expanded,
            paths,
            cwd: context.cwd.to_string(),
            project_root: context.project_root_str.to_string(),
            session_id: context.session_id.to_string(),
            chain_position: Some(cmd.position),
            chain_length: Some(cmd.chain_length),
            chain_operator: cmd.next_operator.clone(),
            prev_operator,
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

        // Evaluate
        self.engine.evaluate(&policy_input)
    }

    /// Check if this is a python -c command and analyze the code if so
    fn analyze_python_if_applicable(
        &self,
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
}

/// Extract code from python -c command
/// Python only supports -c (no long form like --code)
fn extract_python_c_code(command: &[String]) -> Option<String> {
    let mut iter = command.iter();
    while let Some(arg) = iter.next() {
        if arg == "-c" {
            return iter.next().cloned();
        }
    }
    None
}

impl<'a> CommandEvaluator<'a> {
    /// Evaluate a compound command with short-circuit logic
    pub fn evaluate_compound(
        &mut self,
        parsed: &[ParsedCommand],
        has_parse_errors: bool,
        context: &EvaluationContext,
    ) -> HookOutput {
        // If parsing had errors, be conservative
        if has_parse_errors {
            return HookOutput::ask_with_reason("Command contains unparseable constructs");
        }

        // Evaluate each command, short-circuit on non-allow
        let mut prev_operator: Option<String> = None;
        for cmd in parsed {
            let result = self.evaluate_single(cmd, context, prev_operator.clone());
            prev_operator = cmd.next_operator.clone();

            match result.decision {
                Decision::Allow => continue,
                Decision::Deny => {
                    let reason = result.reason.unwrap_or_else(|| {
                        format!(
                            "Denied at command {} of {}",
                            cmd.position + 1,
                            cmd.chain_length
                        )
                    });
                    return HookOutput::deny(&reason);
                }
                Decision::Ask => {
                    let reason = result.reason.unwrap_or_else(|| {
                        format!(
                            "Review needed for command {} of {}",
                            cmd.position + 1,
                            cmd.chain_length
                        )
                    });
                    return HookOutput::ask_with_reason(&reason);
                }
            }
        }

        // All commands allowed
        HookOutput::new(Decision::Allow, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_command;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_context<'a>(cwd: &'a str, cwd_path: &'a Path) -> EvaluationContext<'a> {
        EvaluationContext {
            cwd,
            cwd_path,
            session_id: "test",
            project_root_str: cwd,
            project_root_path: None,
        }
    }

    #[test]
    fn test_evaluate_single_empty_command() {
        let dir = TempDir::new().unwrap();
        let mut engine = PolicyEngine::new();
        let command_defs = CommandDefinitions::builtin();
        let mut nickel_config = NickelConfig::empty();

        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);

        // Empty command text
        let cmd = ParsedCommand {
            text: "".to_string(),
            position: 0,
            chain_length: 1,
            next_operator: None,
        };

        let result = evaluator.evaluate_single(&cmd, &context, None);
        // Empty tokenization should result in Ask
        assert_eq!(result.decision, Decision::Ask);
        drop(dir);
    }

    #[test]
    fn test_evaluate_compound_with_parse_errors() {
        let mut engine = PolicyEngine::new();
        let command_defs = CommandDefinitions::builtin();
        let mut nickel_config = NickelConfig::empty();

        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);

        let result = evaluator.evaluate_compound(&[], true, &context);
        assert_eq!(result.decision(), Decision::Ask);
    }

    #[test]
    fn test_evaluate_compound_no_policy_returns_ask() {
        // With no policies loaded, all commands should return Ask
        let mut engine = PolicyEngine::new();
        let command_defs = CommandDefinitions::builtin();
        let mut nickel_config = NickelConfig::empty();

        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);

        let parse_result = parse_command("echo hello");
        let result =
            evaluator.evaluate_compound(&parse_result.commands, parse_result.has_errors, &context);

        // With no policies, should return Ask (no rule matched)
        assert_eq!(result.decision(), Decision::Ask);
    }
}
