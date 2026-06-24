use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
    Defer,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
            Decision::Defer => "defer",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub hook_specific_output: HookSpecificOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    // When true, the hook should emit NOTHING on stdout (exit 0) so Claude
    // Code's normal permission flow / auto-mode classifier decides. Not
    // serialized — it controls whether we serialize at all.
    #[serde(skip)]
    pub silent: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub hook_event_name: String,
    pub permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
}

impl HookOutput {
    pub fn new(decision: Decision, reason: Option<String>) -> Self {
        HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                permission_decision: decision.as_str().to_string(),
                permission_decision_reason: reason.clone(),
                updated_input: None,
            },
            system_message: reason,
            silent: false,
        }
    }

    #[cfg(test)]
    pub fn allow() -> Self {
        Self::new(Decision::Allow, None)
    }

    pub fn deny(reason: &str) -> Self {
        Self::new(Decision::Deny, Some(reason.to_string()))
    }

    #[cfg(test)]
    pub fn ask() -> Self {
        Self::new(Decision::Ask, None)
    }

    pub fn ask_with_reason(reason: &str) -> Self {
        Self::new(Decision::Ask, Some(reason.to_string()))
    }

    /// A "no decision" output: emits nothing on stdout (exit 0) so Claude
    /// Code's normal permission flow handles the command.
    pub fn defer() -> Self {
        let mut out = Self::new(Decision::Defer, None);
        out.silent = true;
        out
    }

    pub fn is_silent(&self) -> bool {
        self.silent
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"hookSpecificOutput":{"permissionDecision":"ask"}}"#.to_string()
        })
    }

    /// Get the decision from this output
    #[cfg(test)]
    pub fn decision(&self) -> Decision {
        match self.hook_specific_output.permission_decision.as_str() {
            "allow" => Decision::Allow,
            "deny" => Decision::Deny,
            "defer" => Decision::Defer,
            _ => Decision::Ask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_output() {
        let output = HookOutput::allow();
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"allow""#));
        assert!(!json.contains("systemMessage"));
    }

    #[test]
    fn test_deny_output() {
        let output = HookOutput::deny("blocked by policy");
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"deny""#));
        assert!(json.contains(r#""systemMessage":"blocked by policy""#));
    }

    #[test]
    fn test_ask_output() {
        let output = HookOutput::ask();
        let json = output.to_json();
        assert!(json.contains(r#""permissionDecision":"ask""#));
    }

    #[test]
    fn test_decision_defer_as_str() {
        assert_eq!(Decision::Defer.as_str(), "defer");
    }

    #[test]
    fn test_defer_output_is_silent() {
        let output = HookOutput::defer();
        assert!(output.is_silent());
    }

    #[test]
    fn test_non_defer_output_is_not_silent() {
        assert!(!HookOutput::deny("x").is_silent());
        assert!(!HookOutput::allow().is_silent());
        assert!(!HookOutput::ask().is_silent());
    }
}
