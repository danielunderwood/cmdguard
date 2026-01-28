use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    pub tool_input: ToolInput,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToolInput {
    pub command: String,
}

pub fn parse_input(json: &str) -> Result<HookInput, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_input() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let input = parse_input(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.tool_input.command, "git status");
    }

    #[test]
    fn test_parse_input_with_cwd() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},"cwd":"/home/user"}"#;
        let input = parse_input(json).unwrap();
        assert_eq!(input.cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "not json";
        assert!(parse_input(json).is_err());
    }
}
