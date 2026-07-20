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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PermissionRequest,
}

impl HookEvent {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "PreToolUse" => Some(Self::PreToolUse),
            "PermissionRequest" => Some(Self::PermissionRequest),
            _ => None,
        }
    }
}

/// A policy outcome before it is rendered for a specific agent hook protocol.
#[derive(Debug)]
pub struct HookOutput {
    decision: Decision,
    reason: Option<String>,
    // A winning defer can explicitly request no output in Claude's protocol.
    // Codex rendering decides fallthrough from the decision and event instead.
    silent: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeWireOutput<'a> {
    hook_specific_output: ClaudePreToolUseOutput<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_message: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudePreToolUseOutput<'a> {
    hook_event_name: &'static str,
    permission_decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision_reason: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPreToolUseWireOutput<'a> {
    hook_specific_output: CodexPreToolUseOutput<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPreToolUseOutput<'a> {
    hook_event_name: &'static str,
    permission_decision: &'static str,
    permission_decision_reason: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPermissionRequestWireOutput<'a> {
    hook_specific_output: CodexPermissionRequestOutput<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPermissionRequestOutput<'a> {
    hook_event_name: &'static str,
    decision: CodexPermissionDecision<'a>,
}

#[derive(Debug, Serialize)]
struct CodexPermissionDecision<'a> {
    behavior: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

impl HookOutput {
    pub fn new(decision: Decision, reason: Option<String>) -> Self {
        Self {
            decision,
            reason,
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

    /// A no-decision outcome for Claude: emit nothing so its normal
    /// permission flow handles the command.
    pub fn defer() -> Self {
        let mut out = Self::new(Decision::Defer, None);
        out.silent = true;
        out
    }

    pub fn is_silent(&self) -> bool {
        self.silent
    }

    /// Render the existing Claude Code PreToolUse protocol.
    pub fn render_claude(&self) -> Option<String> {
        if self.silent {
            return None;
        }

        let output = ClaudeWireOutput {
            hook_specific_output: ClaudePreToolUseOutput {
                hook_event_name: "PreToolUse",
                permission_decision: self.decision.as_str(),
                permission_decision_reason: self.reason.as_deref(),
            },
            system_message: self.reason.as_deref(),
        };
        Some(serde_json::to_string(&output).expect("Claude hook output is serializable"))
    }

    pub fn render_claude_pretty(&self) -> Option<String> {
        self.render_claude().map(|json| {
            let value: serde_json::Value =
                serde_json::from_str(&json).expect("rendered Claude output is valid JSON");
            serde_json::to_string_pretty(&value).expect("Claude hook output is serializable")
        })
    }

    /// Render Codex's event-specific hook protocol.
    ///
    /// PreToolUse only receives an explicit deny. Allow, ask, and defer all
    /// fall through so Codex can apply its sandbox and approval policy.
    /// PermissionRequest receives allow/deny decisions; ask and defer leave
    /// the normal approval prompt in place.
    pub fn render_codex(&self, event: HookEvent) -> Option<String> {
        match (event, self.decision) {
            (HookEvent::PreToolUse, Decision::Deny) => {
                let reason = self
                    .reason
                    .as_deref()
                    .unwrap_or("Blocked by cmdguard policy");
                let output = CodexPreToolUseWireOutput {
                    hook_specific_output: CodexPreToolUseOutput {
                        hook_event_name: "PreToolUse",
                        permission_decision: "deny",
                        permission_decision_reason: reason,
                    },
                };
                Some(serde_json::to_string(&output).expect("Codex hook output is serializable"))
            }
            (HookEvent::PermissionRequest, Decision::Allow) => {
                let output = CodexPermissionRequestWireOutput {
                    hook_specific_output: CodexPermissionRequestOutput {
                        hook_event_name: "PermissionRequest",
                        decision: CodexPermissionDecision {
                            behavior: "allow",
                            message: None,
                        },
                    },
                };
                Some(serde_json::to_string(&output).expect("Codex hook output is serializable"))
            }
            (HookEvent::PermissionRequest, Decision::Deny) => {
                let output = CodexPermissionRequestWireOutput {
                    hook_specific_output: CodexPermissionRequestOutput {
                        hook_event_name: "PermissionRequest",
                        decision: CodexPermissionDecision {
                            behavior: "deny",
                            message: self.reason.as_deref(),
                        },
                    },
                };
                Some(serde_json::to_string(&output).expect("Codex hook output is serializable"))
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn decision(&self) -> Decision {
        self.decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_allow_output() {
        let json = HookOutput::allow().render_claude().unwrap();
        assert!(json.contains(r#""permissionDecision":"allow""#));
        assert!(!json.contains("systemMessage"));
    }

    #[test]
    fn claude_deny_output() {
        let json = HookOutput::deny("blocked by policy")
            .render_claude()
            .unwrap();
        assert!(json.contains(r#""permissionDecision":"deny""#));
        assert!(json.contains(r#""systemMessage":"blocked by policy""#));
    }

    #[test]
    fn claude_ask_output() {
        let json = HookOutput::ask().render_claude().unwrap();
        assert!(json.contains(r#""permissionDecision":"ask""#));
    }

    #[test]
    fn claude_defer_is_silent() {
        assert!(HookOutput::defer().render_claude().is_none());
    }

    #[test]
    fn codex_pre_tool_use_only_emits_deny() {
        assert!(HookOutput::allow()
            .render_codex(HookEvent::PreToolUse)
            .is_none());
        assert!(HookOutput::ask()
            .render_codex(HookEvent::PreToolUse)
            .is_none());
        assert!(HookOutput::defer()
            .render_codex(HookEvent::PreToolUse)
            .is_none());

        let json = HookOutput::deny("blocked")
            .render_codex(HookEvent::PreToolUse)
            .unwrap();
        assert!(json.contains(r#""hookEventName":"PreToolUse""#));
        assert!(json.contains(r#""permissionDecision":"deny""#));
    }

    #[test]
    fn codex_permission_request_emits_allow_and_deny() {
        let allow = HookOutput::allow()
            .render_codex(HookEvent::PermissionRequest)
            .unwrap();
        assert!(allow.contains(r#""behavior":"allow""#));

        let deny = HookOutput::deny("blocked")
            .render_codex(HookEvent::PermissionRequest)
            .unwrap();
        assert!(deny.contains(r#""behavior":"deny""#));
        assert!(deny.contains(r#""message":"blocked""#));

        assert!(HookOutput::ask()
            .render_codex(HookEvent::PermissionRequest)
            .is_none());
        assert!(HookOutput::defer()
            .render_codex(HookEvent::PermissionRequest)
            .is_none());
    }
}
