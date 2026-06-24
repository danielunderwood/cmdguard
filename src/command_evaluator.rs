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

/// How a winning `Defer` decision is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferMode {
    /// Emit nothing (exit 0): hand the command to Claude Code's normal flow.
    Silent,
    /// Render a winning defer as an explicit `ask` (multi-hook backstop).
    Prompt,
}

impl DeferMode {
    /// Resolve from env (`CMDGUARD_DEFER_MODE`) over config over default.
    /// Unknown values fall back to Silent.
    pub fn resolve(config: Option<&str>) -> DeferMode {
        let raw = std::env::var("CMDGUARD_DEFER_MODE")
            .ok()
            .or_else(|| config.map(|s| s.to_string()));
        match raw.as_deref() {
            Some("prompt") => DeferMode::Prompt,
            _ => DeferMode::Silent,
        }
    }
}

/// Pick the most-restrictive decision across compound segments.
/// Order: Deny > Ask > Defer > Allow.
pub fn most_restrictive(decisions: &[Decision]) -> Decision {
    fn weight(d: Decision) -> u8 {
        match d {
            Decision::Deny => 4,
            Decision::Ask => 3,
            Decision::Defer => 2,
            Decision::Allow => 1,
        }
    }
    decisions
        .iter()
        .copied()
        .max_by_key(|d| weight(*d))
        .unwrap_or(Decision::Allow)
}

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
        let positional_map_json = serde_json::to_value(parsed_cmd.positional_as_map()).ok();

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
            redirections: cmd.redirections.clone(),
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
    /// Evaluate a compound command using most-restrictive resolution.
    ///
    /// Scans all segments (deny still short-circuits for speed) and resolves
    /// the winner via `most_restrictive` (Deny > Ask > Defer > Allow). A
    /// winning `Defer` is rendered per `defer_mode`: silent (no output) or an
    /// explicit `ask`. Parse failures stay an explicit `ask`, never defer.
    pub fn evaluate_compound(
        &mut self,
        parsed: &[ParsedCommand],
        has_parse_errors: bool,
        context: &EvaluationContext,
        defer_mode: DeferMode,
    ) -> HookOutput {
        if has_parse_errors {
            // Blind, not abstaining: keep prompting.
            return HookOutput::ask_with_reason("Command contains unparseable constructs");
        }

        let mut prev_operator: Option<String> = None;
        let mut outcomes: Vec<(Decision, Option<String>, usize, usize)> = Vec::new();

        for cmd in parsed {
            let result = self.evaluate_single(cmd, context, prev_operator.clone());
            prev_operator = cmd.next_operator.clone();

            // Deny short-circuits: nothing can be more restrictive.
            if result.decision == Decision::Deny {
                let reason = result.reason.unwrap_or_else(|| {
                    format!(
                        "Denied at command {} of {}",
                        cmd.position + 1,
                        cmd.chain_length
                    )
                });
                return HookOutput::deny(&reason);
            }
            outcomes.push((
                result.decision,
                result.reason,
                cmd.position,
                cmd.chain_length,
            ));
        }

        let decisions: Vec<Decision> = outcomes.iter().map(|o| o.0).collect();
        match most_restrictive(&decisions) {
            Decision::Allow => HookOutput::new(Decision::Allow, None),
            Decision::Deny => unreachable!("deny short-circuits above"),
            Decision::Ask => {
                let (_, reason, pos, len) = outcomes
                    .iter()
                    .find(|o| o.0 == Decision::Ask)
                    .cloned()
                    .unwrap();
                let reason = reason
                    .unwrap_or_else(|| format!("Review needed for command {} of {}", pos + 1, len));
                HookOutput::ask_with_reason(&reason)
            }
            Decision::Defer => match defer_mode {
                DeferMode::Silent => HookOutput::defer(),
                DeferMode::Prompt => {
                    let (_, reason, pos, len) = outcomes
                        .iter()
                        .find(|o| o.0 == Decision::Defer)
                        .cloned()
                        .unwrap();
                    let reason = reason.unwrap_or_else(|| {
                        format!("No policy decision for command {} of {}", pos + 1, len)
                    });
                    HookOutput::ask_with_reason(&reason)
                }
            },
        }
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
            redirections: vec![],
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

        let result = evaluator.evaluate_compound(&[], true, &context, DeferMode::Prompt);
        assert_eq!(result.decision(), Decision::Ask);
    }

    #[test]
    fn test_defer_mode_resolution_default_is_silent() {
        // No env, no config -> Silent
        // (CMDGUARD_DEFER_MODE must not be set in the test environment.)
        std::env::remove_var("CMDGUARD_DEFER_MODE");
        assert_eq!(DeferMode::resolve(None), DeferMode::Silent);
    }

    #[test]
    fn test_defer_mode_resolution_config_prompt() {
        std::env::remove_var("CMDGUARD_DEFER_MODE");
        assert_eq!(DeferMode::resolve(Some("prompt")), DeferMode::Prompt);
    }

    #[test]
    fn test_defer_mode_unknown_value_falls_back_to_silent() {
        std::env::remove_var("CMDGUARD_DEFER_MODE");
        assert_eq!(DeferMode::resolve(Some("banana")), DeferMode::Silent);
    }

    #[test]
    fn test_most_restrictive_ordering() {
        // Deny > Ask > Defer > Allow
        assert_eq!(
            most_restrictive(&[Decision::Defer, Decision::Deny]),
            Decision::Deny
        );
        assert_eq!(
            most_restrictive(&[Decision::Deny, Decision::Defer]),
            Decision::Deny
        );
        assert_eq!(
            most_restrictive(&[Decision::Allow, Decision::Defer]),
            Decision::Defer
        );
        assert_eq!(
            most_restrictive(&[Decision::Defer, Decision::Ask]),
            Decision::Ask
        );
        assert_eq!(
            most_restrictive(&[Decision::Allow, Decision::Allow]),
            Decision::Allow
        );
        assert_eq!(most_restrictive(&[]), Decision::Allow);
    }

    #[test]
    fn test_compound_deny_beats_defer_regardless_of_order() {
        // A defer segment combined with a deny segment must resolve to deny,
        // not short-circuit to defer. With no policies loaded, an unmatched
        // command defers; `rm --no-preserve-root /` denies via builtin policy.
        std::env::remove_var("CMDGUARD_DEFER_MODE");

        // `most_restrictive` is the lighter, policy-independent test of the
        // core resolution invariant.
        assert_eq!(
            most_restrictive(&[Decision::Defer, Decision::Deny]),
            Decision::Deny
        );
        assert_eq!(
            most_restrictive(&[Decision::Deny, Decision::Defer]),
            Decision::Deny
        );
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
        // With no policies, an unmatched command defers; under Prompt mode
        // that surfaces as an explicit Ask.
        let result = evaluator.evaluate_compound(
            &parse_result.commands,
            parse_result.has_errors,
            &context,
            DeferMode::Prompt,
        );

        // With no policies + Prompt mode, should return Ask (no rule matched)
        assert_eq!(result.decision(), Decision::Ask);
    }
}
