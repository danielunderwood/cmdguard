use crate::output::Decision;
use crate::paths::DetectedPath;
use crate::python_analyzer;
use regorus::Engine;
use serde::Serialize;
use std::path::Path;
use tracing::{debug, warn};

/// Python code analysis results for PolicyInput
#[derive(Debug, Serialize, Clone)]
pub struct PythonAnalysisInput {
    /// Matched patterns from tree-sitter query
    pub patterns: Vec<PatternInput>,
    /// Imported modules
    pub imports: Vec<String>,
    /// Whether code appears safe for inspection mode
    pub is_inspection_safe: bool,
}

/// A matched pattern for serialization to Rego
#[derive(Debug, Serialize, Clone)]
pub struct PatternInput {
    /// Capture name from query (e.g., "dangerous_import", "file_op")
    pub capture: String,
    /// Matched text
    pub text: String,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (0-indexed)
    pub column: usize,
}

impl From<&python_analyzer::Pattern> for PatternInput {
    fn from(p: &python_analyzer::Pattern) -> Self {
        Self {
            capture: p.capture.clone(),
            text: p.text.clone(),
            line: p.line,
            column: p.column,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PolicyInput {
    pub tool: String,
    pub raw_command: String,
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
    pub flags_expanded: Vec<String>,
    pub paths: Vec<DetectedPath>,
    pub redirections: Vec<crate::parser::ShellRedirect>,
    pub cwd: String,
    pub project_root: String,
    pub session_id: String,
    // New fields for compound commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_position: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_operator: Option<String>,
    /// Operator that connected the *previous* command in the chain to this one.
    /// "|" means this command's stdin is the previous command's stdout.
    /// Useful for distinguishing `cat foo | sed 's/x/y/'` (no file mutation)
    /// from `sed 's/x/y/' foo` (rule must look at positional files).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_operator: Option<String>,
    // Trust zone fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_as_typed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_trust_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_symlink: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_source: Option<String>,
    // Parsed command fields
    /// Parsed flags from command (name -> value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_flags: Option<serde_json::Value>,
    /// Parsed positional arguments (array format for iteration)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positional_args: Option<serde_json::Value>,
    /// Parsed positional arguments (map format for direct access by name)
    /// Access as: input.positional.url[0].raw
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positional: Option<serde_json::Value>,
    /// Subcommand if present (e.g., "push" for "git push")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
    /// Flag tokens that matched no flag definition; see
    /// `ParsedCommand::unknown_flags` for when they are recorded.
    ///
    /// A rule that grants extra latitude (allowing a command that would
    /// otherwise ask) should require this to be empty. cmdguard cannot reason
    /// about the effect of an option it does not model, and for tools like curl
    /// an unmodelled option can redirect the request or write a file. Always
    /// serialized, so `count(input.unknown_flags)` is defined for every input.
    pub unknown_flags: Vec<String>,
    /// Python code analysis (for python -c commands)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_analysis: Option<PythonAnalysisInput>,
}

pub struct PolicyResult {
    pub decision: Decision,
    pub reason: Option<String>,
    pub rule: Option<String>,
    pub explicit: bool,
}

pub struct PolicyEngine {
    engine: Engine,
}

impl PolicyEngine {
    pub fn new() -> Self {
        PolicyEngine {
            engine: Engine::new(),
        }
    }

    pub fn load_policy_file(&mut self, path: &Path) -> Result<(), String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read policy file {:?}: {}", path, e))?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("policy.rego");

        self.engine
            .add_policy(filename.to_string(), contents)
            .map_err(|e| format!("Failed to parse policy {:?}: {}", path, e))?;

        Ok(())
    }

    pub fn load_policies_from_dir(&mut self, dir: &Path) -> Result<(), String> {
        if !dir.exists() {
            return Err(format!("Policy directory {:?} does not exist", dir));
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("Failed to read policy directory {:?}: {}", dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("rego") {
                debug!("Loading policy file: {:?}", path);
                self.load_policy_file(&path)?;
            }
        }

        Ok(())
    }

    /// Load policies honoring the new base/+policies/ layout when populated,
    /// falling back to a flat directory of `.rego` files otherwise. Mirrors the
    /// production loader in `main::load_all_policies` (without project-local
    /// merging) so test/eval/hook see the same rule set.
    pub fn load_policies_with_layout(&mut self, config_dir: &Path) -> Result<(), String> {
        let base_dir = config_dir.join("base");
        let has_populated_base = std::fs::read_dir(&base_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rego"))
            })
            .unwrap_or(false);

        if has_populated_base {
            self.load_policies_from_dir(&base_dir)?;
            let policies_dir = config_dir.join("policies");
            if policies_dir.exists() {
                self.load_policies_from_dir(&policies_dir)?;
            }
            Ok(())
        } else {
            self.load_policies_from_dir(config_dir)
        }
    }

    pub fn evaluate(&mut self, input: &PolicyInput) -> PolicyResult {
        // Set input data
        let input_json = match serde_json::to_value(input) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to serialize policy input: {}", e);
                return PolicyResult {
                    decision: Decision::Ask,
                    reason: Some("Internal error serializing input".to_string()),
                    rule: None,
                    explicit: false,
                };
            }
        };

        // Convert serde_json::Value to regorus::Value
        let input_value: regorus::Value = input_json.into();
        self.engine.set_input(input_value);

        // Evaluate and return result
        self.eval_result()
    }

    fn eval_result(&mut self) -> PolicyResult {
        match self.engine.eval_rule("data.cmdguard.result".to_string()) {
            Ok(value) => {
                // Try to parse as object with decision, reason, rule, explicit
                let obj = match value.as_object() {
                    Ok(o) => o,
                    Err(_) => {
                        return PolicyResult {
                            decision: Decision::Ask,
                            reason: Some("Policy result is not an object".to_string()),
                            rule: None,
                            explicit: false,
                        };
                    }
                };

                let decision = obj
                    .get(&"decision".into())
                    .and_then(|v| v.as_string().ok())
                    .map(|s| match s.as_ref() {
                        "allow" => Decision::Allow,
                        "deny" => Decision::Deny,
                        "defer" => Decision::Defer,
                        _ => Decision::Ask,
                    })
                    .unwrap_or(Decision::Ask);

                let reason = obj
                    .get(&"reason".into())
                    .and_then(|v| v.as_string().ok())
                    .map(|s| s.to_string());

                let rule = obj
                    .get(&"rule".into())
                    .and_then(|v| v.as_string().ok())
                    .map(|s| s.to_string());

                let explicit = obj
                    .get(&"explicit".into())
                    .and_then(|v| v.as_bool().ok())
                    .copied()
                    .unwrap_or(false);

                PolicyResult {
                    decision,
                    reason,
                    rule,
                    explicit,
                }
            }
            Err(e) => {
                warn!("Failed to evaluate result: {}", e);
                PolicyResult {
                    decision: Decision::Ask,
                    reason: Some(format!("Policy evaluation error: {}", e)),
                    rule: None,
                    explicit: false,
                }
            }
        }
    }

    /// Evaluate all matching rules for debugging purposes
    /// Returns all rules that matched, not just the highest priority winner
    pub fn evaluate_all_rules(&mut self, input: &PolicyInput) -> Vec<PolicyResult> {
        // Set input data
        let input_json = match serde_json::to_value(input) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to serialize policy input: {}", e);
                return vec![];
            }
        };

        // Convert serde_json::Value to regorus::Value
        let input_value: regorus::Value = input_json.into();
        self.engine.set_input(input_value);

        // Query all rules - try both all_rules (helper) and rules (direct)
        let query_result = self
            .engine
            .eval_rule("data.cmdguard.all_rules".to_string())
            .or_else(|_| self.engine.eval_rule("data.cmdguard.rules".to_string()));

        match query_result {
            Ok(value) => {
                let obj = match value.as_object() {
                    Ok(o) => o,
                    Err(_) => return vec![],
                };

                let mut results = vec![];
                for (rule_name_val, rule_obj_val) in obj.iter() {
                    let rule_name = match rule_name_val.as_string() {
                        Ok(s) => s.to_string(),
                        Err(_) => continue,
                    };

                    let rule_obj = match rule_obj_val.as_object() {
                        Ok(o) => o,
                        Err(_) => continue,
                    };

                    let decision = rule_obj
                        .get(&"decision".into())
                        .and_then(|v| v.as_string().ok())
                        .map(|s| match s.as_ref() {
                            "allow" => Decision::Allow,
                            "deny" => Decision::Deny,
                            "defer" => Decision::Defer,
                            _ => Decision::Ask,
                        })
                        .unwrap_or(Decision::Ask);

                    let reason = rule_obj
                        .get(&"reason".into())
                        .and_then(|v| v.as_string().ok())
                        .map(|s| s.to_string());

                    results.push(PolicyResult {
                        decision,
                        reason,
                        rule: Some(rule_name),
                        explicit: true,
                    });
                }

                // Stable order so `cmdguard eval` output is reproducible.
                results.sort_by(|a, b| {
                    a.rule
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.rule.as_deref().unwrap_or(""))
                });
                results
            }
            Err(_) => {
                // It's okay if all_rules/rules doesn't exist or can't be queried
                vec![]
            }
        }
    }

    pub fn query_string_set_table(&mut self, table_name: &str) -> Vec<(String, Vec<String>)> {
        let rule_name = format!("data.cmdguard.{}", table_name);
        match self.engine.eval_rule(rule_name) {
            Ok(value) => {
                let obj = match value.as_object() {
                    Ok(o) => o,
                    Err(_) => return vec![],
                };

                let mut result = vec![];
                for (binary_val, subcmds_val) in obj.iter() {
                    if let Ok(binary) = binary_val.as_string() {
                        let mut subcmds = vec![];

                        // Try to parse as set
                        if let Ok(set) = subcmds_val.as_set() {
                            for item in set.iter() {
                                if let Ok(s) = item.as_string() {
                                    subcmds.push(s.to_string());
                                }
                            }
                        }

                        if !subcmds.is_empty() {
                            subcmds.sort();
                            result.push((binary.to_string(), subcmds));
                        }
                    }
                }

                result.sort_by(|a, b| a.0.cmp(&b.0));
                result
            }
            Err(_) => vec![],
        }
    }

    pub fn query_boolean_key_table(&mut self, table_name: &str) -> Vec<String> {
        let rule_name = format!("data.cmdguard.{}", table_name);
        match self.engine.eval_rule(rule_name) {
            Ok(value) => {
                let obj = match value.as_object() {
                    Ok(o) => o,
                    Err(_) => return vec![],
                };

                let mut result = vec![];
                for (key_val, enabled_val) in obj.iter() {
                    // Rego truthiness: any value other than `false` (or
                    // undefined) is truthy, not just the literal `true`.
                    let enabled = !matches!(
                        enabled_val,
                        regorus::Value::Bool(false) | regorus::Value::Undefined
                    );
                    if enabled {
                        if let Ok(key) = key_val.as_string() {
                            result.push(key.to_string());
                        }
                    }
                }

                result.sort();
                result
            }
            Err(_) => vec![],
        }
    }

    /// Query allowed_subcommands table from policy
    pub fn query_allowed_subcommands(&mut self) -> Vec<(String, Vec<String>)> {
        self.query_string_set_table("allowed_subcommands")
    }

    /// Query denied_subcommands table from policy
    pub fn query_denied_subcommands(&mut self) -> Vec<(String, Vec<String>)> {
        self.query_string_set_table("denied_subcommands")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_input(command: Vec<&str>) -> PolicyInput {
        PolicyInput {
            tool: "Bash".to_string(),
            raw_command: command.join(" "),
            command: command.iter().map(|s| s.to_string()).collect(),
            wrapper_chain: vec![],
            flags_expanded: vec![],
            paths: vec![],
            redirections: vec![],
            cwd: "/home/user/project".to_string(),
            project_root: "/home/user/project".to_string(),
            session_id: "test".to_string(),
            chain_position: None,
            chain_length: None,
            chain_operator: None,
            prev_operator: None,
            command_as_typed: None,
            binary_name: None,
            resolved_path: None,
            resolved_trust_zone: None,
            is_symlink: None,
            symlink_source: None,
            parsed_flags: None,
            positional_args: None,
            positional: None,
            subcommand: None,
            unknown_flags: vec![],
            python_analysis: None,
        }
    }

    #[test]
    fn test_load_and_evaluate_policy() {
        let mut engine = PolicyEngine::new();

        // Load from policies directory
        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        // Test allowed command
        let input = make_input(vec!["git", "status"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Allow);
        assert_eq!(result.reason, Some("Safe git read operation".to_string()));
    }

    #[test]
    fn test_deny_force_push() {
        let mut engine = PolicyEngine::new();

        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["git", "push", "--force", "origin", "main"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Deny);
        assert!(result.reason.unwrap().contains("Force push"));
    }

    #[test]
    fn test_defer_for_unknown() {
        let mut engine = PolicyEngine::new();

        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["curl", "https://example.com"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Defer);
    }

    #[test]
    fn test_policy_result_has_rule_name() {
        let mut engine = PolicyEngine::new();
        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["git", "status"]);
        let result = engine.evaluate(&input);

        assert_eq!(result.decision, Decision::Allow);
        assert!(result.rule.is_some());
        assert_eq!(result.rule.unwrap(), "safe_git_read");
        assert!(result.explicit);
    }

    #[test]
    fn test_policy_result_default_not_explicit() {
        let mut engine = PolicyEngine::new();
        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["curl", "https://example.com"]);
        let result = engine.evaluate(&input);

        assert_eq!(result.decision, Decision::Defer);
        assert!(!result.explicit);
    }

    /// Verify the stdlib exclusion-table mechanism: a user override that adds
    /// `denied_subcommands["bin"] := {"sub"}` must suppress an entry in
    /// `allowed_subcommands["bin"]`. Without this, the public-facing claim
    /// that users can "narrow base allowlists" silently breaks.
    #[test]
    fn test_denied_subcommands_overrides_allow() {
        use std::collections::HashMap;

        // Compose a minimal in-memory policy that exercises the stdlib
        // helpers. We need the real stdlib so the rules-with-priority
        // aggregation runs.
        let stdlib = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/stdlib.rego"),
        )
        .expect("read stdlib.rego");

        let user_policy = r#"
package cmdguard
import rego.v1

allowed_subcommands["mybin"] := {"safe", "blocked"}
denied_subcommands["mybin"] := {"blocked"}
"#;

        let mut input = make_input(vec!["mybin", "safe"]);
        input.binary_name = Some("mybin".to_string());
        input.subcommand = Some("safe".to_string());

        let mut engine = PolicyEngine::new();
        engine
            .engine
            .add_policy("stdlib.rego".into(), stdlib.clone())
            .unwrap();
        engine
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .unwrap();

        // "safe" is allowed (in allow set, not in deny set)
        let result = engine.evaluate(&input);
        assert_eq!(
            result.decision,
            Decision::Allow,
            "safe subcommand should be allowed"
        );

        // Now check "blocked" -- present in allow AND deny -> must NOT allow
        let mut input2 = make_input(vec!["mybin", "blocked"]);
        input2.binary_name = Some("mybin".to_string());
        input2.subcommand = Some("blocked".to_string());

        let mut engine2 = PolicyEngine::new();
        engine2
            .engine
            .add_policy("stdlib.rego".into(), stdlib)
            .unwrap();
        engine2
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .unwrap();
        let result2 = engine2.evaluate(&input2);
        assert_ne!(
            result2.decision,
            Decision::Allow,
            "denied_subcommands must suppress the allow from allowed_subcommands"
        );

        // Silence unused warning in case future changes drop HashMap usage.
        let _ = HashMap::<(), ()>::new();
    }

    /// Same idea but for `denied_with_args` -- exercises the `allowed_with_args`
    /// rule path.
    #[test]
    fn test_denied_with_args_overrides_allow() {
        let stdlib = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/stdlib.rego"),
        )
        .expect("read stdlib.rego");

        let user_policy = r#"
package cmdguard
import rego.v1

allowed_with_args["mytool"] := {"foo", "bar"}
denied_with_args["mytool"] := {"bar"}
"#;

        let mut engine = PolicyEngine::new();
        engine
            .engine
            .add_policy("stdlib.rego".into(), stdlib.clone())
            .unwrap();
        engine
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .unwrap();
        let result = engine.evaluate(&make_input(vec!["mytool", "foo"]));
        assert_eq!(
            result.decision,
            Decision::Allow,
            "non-denied arg should allow"
        );

        let mut engine2 = PolicyEngine::new();
        engine2
            .engine
            .add_policy("stdlib.rego".into(), stdlib)
            .unwrap();
        engine2
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .unwrap();
        let result2 = engine2.evaluate(&make_input(vec!["mytool", "bar"]));
        assert_ne!(
            result2.decision,
            Decision::Allow,
            "denied_with_args must suppress the allow from allowed_with_args"
        );
    }

    #[test]
    fn test_allowed_redirect_targets_suppresses_only_redirect_ask() {
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let stdlib =
            std::fs::read_to_string(config_dir.join("stdlib.rego")).expect("read stdlib.rego");
        let file_ops =
            std::fs::read_to_string(config_dir.join("file-ops.rego")).expect("read file-ops.rego");
        let user_policy = r#"
package cmdguard
import rego.v1

allowed_redirect_targets["/dev/null"] := true

rules["ask_other_reason"] := ask("Other ask still wins") if {
	input.binary_name == "dangerous"
}

rules["allow_echo"] := allow("Echo allowed") if {
	input.binary_name == "echo"
}

rules["allow_dangerous"] := allow("Dangerous allowed") if {
	input.binary_name == "dangerous"
}
"#;

        let mut engine = PolicyEngine::new();
        engine
            .engine
            .add_policy("stdlib.rego".into(), stdlib)
            .unwrap();
        engine
            .engine
            .add_policy("file-ops.rego".into(), file_ops)
            .unwrap();
        engine
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .unwrap();

        let null_redirect = crate::parser::ShellRedirect {
            raw: "> /dev/null".to_string(),
            operator: ">".to_string(),
            fd: None,
            target: Some("/dev/null".to_string()),
            kind: crate::parser::ShellRedirectKind::Write,
            writes_to_file: true,
        };

        let mut allowed_input = make_input(vec!["echo", "hi"]);
        allowed_input.binary_name = Some("echo".to_string());
        allowed_input.redirections = vec![null_redirect.clone()];

        let result = engine.evaluate(&allowed_input);
        assert_eq!(result.decision, Decision::Allow);
        assert_eq!(result.rule.as_deref(), Some("allow_echo"));

        let mut other_ask_input = make_input(vec!["dangerous"]);
        other_ask_input.binary_name = Some("dangerous".to_string());
        other_ask_input.redirections = vec![null_redirect];

        let result = engine.evaluate(&other_ask_input);
        assert_eq!(result.decision, Decision::Ask);
        assert_eq!(result.rule.as_deref(), Some("ask_other_reason"));

        let mut file_redirect_input = make_input(vec!["echo", "hi"]);
        file_redirect_input.binary_name = Some("echo".to_string());
        file_redirect_input.redirections = vec![crate::parser::ShellRedirect {
            raw: "> out.txt".to_string(),
            operator: ">".to_string(),
            fd: None,
            target: Some("out.txt".to_string()),
            kind: crate::parser::ShellRedirectKind::Write,
            writes_to_file: true,
        }];

        let result = engine.evaluate(&file_redirect_input);
        assert_eq!(result.decision, Decision::Ask);
        assert_eq!(result.rule.as_deref(), Some("ask_shell_output_redirection"));
    }

    #[test]
    fn test_query_boolean_key_table_reports_any_truthy_value() {
        // Rego truthiness for an extension table entry means "not false"
        // (and not undefined); it isn't limited to the literal `true`.
        let config_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config");
        let file_ops =
            std::fs::read_to_string(config_dir.join("file-ops.rego")).expect("read file-ops.rego");
        let user_policy = r#"
package cmdguard
import rego.v1

allowed_redirect_targets["/dev/null"] := "yes"
allowed_redirect_targets["/dev/zero"] := false
"#;
        let mut engine = PolicyEngine::new();
        engine
            .engine
            .add_policy("file-ops.rego".into(), file_ops)
            .expect("load file-ops.rego");
        engine
            .engine
            .add_policy("user.rego".into(), user_policy.into())
            .expect("load policy");

        let entries = engine.query_boolean_key_table("allowed_redirect_targets");
        assert_eq!(entries, vec!["/dev/null".to_string()]);
    }

    #[test]
    fn test_eval_maps_defer_string_to_defer_decision() {
        // Minimal policy whose result is a defer decision.
        let policy = r#"
package cmdguard
result := {"decision": "defer", "reason": "no opinion", "rule": "x", "explicit": false}
"#;
        let mut engine = PolicyEngine::new();
        engine
            .engine
            .add_policy("test.rego".into(), policy.into())
            .expect("load policy");
        let input = make_input(vec!["somecmd"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Defer);
    }
}
