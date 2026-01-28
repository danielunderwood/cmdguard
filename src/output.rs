use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub hook_specific_output: HookSpecificOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
}

impl HookOutput {
    pub fn new(decision: Decision, reason: Option<String>) -> Self {
        HookOutput {
            hook_specific_output: HookSpecificOutput {
                permission_decision: decision.as_str().to_string(),
                updated_input: None,
            },
            system_message: reason,
        }
    }

    pub fn allow() -> Self {
        Self::new(Decision::Allow, None)
    }

    pub fn deny(reason: &str) -> Self {
        Self::new(Decision::Deny, Some(reason.to_string()))
    }

    pub fn ask() -> Self {
        Self::new(Decision::Ask, None)
    }

    pub fn ask_with_reason(reason: &str) -> Self {
        Self::new(Decision::Ask, Some(reason.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"hookSpecificOutput":{"permissionDecision":"ask"}}"#.to_string()
        })
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
}
