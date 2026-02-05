use crate::output::Decision;
use crate::paths::DetectedPath;
use regorus::Engine;
use serde::Serialize;
use std::path::Path;
use tracing::{debug, warn};

#[derive(Debug, Serialize)]
pub struct PolicyInput {
    pub tool: String,
    pub raw_command: String,
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
    pub flags_expanded: Vec<String>,
    pub paths: Vec<DetectedPath>,
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

        let filename = path.file_name()
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
        match self.engine.eval_rule("data.claude.permissions.result".to_string()) {
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
            cwd: "/home/user/project".to_string(),
            project_root: "/home/user/project".to_string(),
            session_id: "test".to_string(),
            chain_position: None,
            chain_length: None,
            chain_operator: None,
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
    fn test_ask_for_unknown() {
        let mut engine = PolicyEngine::new();

        let policy_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies");
        engine.load_policies_from_dir(&policy_dir).unwrap();

        let input = make_input(vec!["curl", "https://example.com"]);
        let result = engine.evaluate(&input);
        assert_eq!(result.decision, Decision::Ask);
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

        assert_eq!(result.decision, Decision::Ask);
        assert!(!result.explicit);
    }
}
