use crate::command_defs::CommandDefinitions;
use crate::command_evaluator::{CommandEvaluator, EvaluationContext};
use crate::nickel_config::NickelConfig;
use crate::output::Decision;
use crate::policy::PolicyEngine;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct TestFile {
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub command: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    pub expect: ExpectedDecision,
    #[serde(default)]
    pub reason_contains: Option<String>,
}

fn default_cwd() -> String {
    "/home/user/project".to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedDecision {
    Allow,
    Deny,
    Ask,
}

impl ExpectedDecision {
    fn matches(&self, decision: Decision) -> bool {
        match (self, decision) {
            (ExpectedDecision::Allow, Decision::Allow) => true,
            (ExpectedDecision::Deny, Decision::Deny) => true,
            (ExpectedDecision::Ask, Decision::Ask) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub expected: ExpectedDecision,
    pub actual: Decision,
    pub reason: Option<String>,
    pub error: Option<String>,
}

pub struct TestRunner {
    engine: PolicyEngine,
    command_defs: CommandDefinitions,
    nickel_config: NickelConfig,
}

impl TestRunner {
    pub fn new(policy_dir: &Path) -> Result<Self, String> {
        let mut engine = PolicyEngine::new();
        engine.load_policies_with_layout(policy_dir)?;

        // Load Nickel config for custom wrappers and command definitions
        let nickel_config = NickelConfig::load(policy_dir);

        // Load command definitions (built-in + custom from Nickel)
        let mut command_defs = CommandDefinitions::builtin();
        let custom_commands = nickel_config.get_command_definitions();
        if !custom_commands.is_empty() {
            command_defs.merge(custom_commands);
        }

        Ok(TestRunner {
            engine,
            command_defs,
            nickel_config,
        })
    }

    pub fn run_tests(&mut self, test_file: &TestFile) -> Vec<TestResult> {
        test_file
            .tests
            .iter()
            .map(|tc| self.run_single_test(tc))
            .collect()
    }

    fn run_single_test(&mut self, test: &TestCase) -> TestResult {
        use crate::parser::parse_command;

        // Parse for compound commands
        let parse_result = parse_command(&test.command);

        // If has errors, be conservative
        if parse_result.has_errors {
            let decision_matches = test.expect.matches(Decision::Ask);
            return TestResult {
                name: test.name.clone(),
                passed: decision_matches,
                expected: test.expect,
                actual: Decision::Ask,
                reason: Some("Command contains unparseable constructs".to_string()),
                error: None,
            };
        }

        // Set up evaluation context
        let cwd_path = PathBuf::from(&test.cwd);
        let context = EvaluationContext {
            cwd: &test.cwd,
            cwd_path: &cwd_path,
            session_id: "test",
            project_root_str: &test.cwd,
            project_root_path: None,
        };

        // Create evaluator with shared logic
        let mut evaluator = CommandEvaluator::new(
            &mut self.engine,
            &self.command_defs,
            &mut self.nickel_config,
        );

        // Evaluate each command, short-circuit on non-allow
        let mut prev_operator: Option<String> = None;
        for cmd in &parse_result.commands {
            let result = evaluator.evaluate_single(cmd, &context, prev_operator.clone());
            prev_operator = cmd.next_operator.clone();

            // Short-circuit on non-allow
            if result.decision != Decision::Allow {
                let decision_matches = test.expect.matches(result.decision);
                let reason_matches = test.reason_contains.as_ref().map_or(true, |expected| {
                    result
                        .reason
                        .as_ref()
                        .map_or(false, |r| r.contains(expected))
                });

                let error = if !reason_matches {
                    Some(format!(
                        "Reason '{}' does not contain '{}'",
                        result.reason.as_deref().unwrap_or("(none)"),
                        test.reason_contains.as_deref().unwrap_or("")
                    ))
                } else {
                    None
                };

                return TestResult {
                    name: test.name.clone(),
                    passed: decision_matches && reason_matches,
                    expected: test.expect,
                    actual: result.decision,
                    reason: result.reason,
                    error,
                };
            }
        }

        // All commands allowed
        let decision_matches = test.expect.matches(Decision::Allow);
        let reason_matches = test.reason_contains.as_ref().map_or(true, |_| false);

        let error = if !reason_matches && test.reason_contains.is_some() {
            Some(format!(
                "Expected reason containing '{}' but all commands allowed (no reason)",
                test.reason_contains.as_deref().unwrap_or("")
            ))
        } else {
            None
        };

        TestResult {
            name: test.name.clone(),
            passed: decision_matches && (error.is_none()),
            expected: test.expect,
            actual: Decision::Allow,
            reason: None,
            error,
        }
    }
}

pub fn load_test_file(path: &Path) -> Result<TestFile, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read test file {:?}: {}", path, e))?;

    serde_yaml::from_str(&contents)
        .map_err(|e| format!("Failed to parse test file {:?}: {}", path, e))
}

pub fn print_results(results: &[TestResult], verbose: bool) {
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();

    if verbose {
        for result in results {
            let status = if result.passed { "✓" } else { "✗" };
            let decision_str = format!("{:?}", result.actual).to_lowercase();

            println!(
                "{} {} -> {} (expected {:?})",
                status, result.name, decision_str, result.expected
            );

            if !result.passed {
                if let Some(ref err) = result.error {
                    println!("    Error: {}", err);
                }
                if let Some(ref reason) = result.reason {
                    println!("    Reason: {}", reason);
                }
            }
        }
        println!();
    }

    if passed == total {
        println!("✓ {}/{} tests passed", passed, total);
    } else {
        println!("✗ {}/{} tests passed", passed, total);

        if !verbose {
            println!("\nFailed tests:");
            for result in results.iter().filter(|r| !r.passed) {
                println!(
                    "  - {} (expected {:?}, got {:?})",
                    result.name, result.expected, result.actual
                );
                if let Some(ref err) = result.error {
                    println!("    {}", err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
tests:
  - name: "test allow"
    command: "git status"
    expect: allow
  - name: "test deny"
    command: "rm -rf /"
    expect: deny
    reason_contains: "blocked"
"#;
        let test_file: TestFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(test_file.tests.len(), 2);
        assert_eq!(test_file.tests[0].name, "test allow");
        assert_eq!(test_file.tests[0].expect, ExpectedDecision::Allow);
        assert_eq!(
            test_file.tests[1].reason_contains,
            Some("blocked".to_string())
        );
    }
}
