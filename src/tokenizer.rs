/// Tokenize a command string respecting quotes
pub fn tokenize(command: &str) -> Result<Vec<String>, String> {
    shlex::split(command).ok_or_else(|| "Failed to tokenize command".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let tokens = tokenize("git status").unwrap();
        assert_eq!(tokens, vec!["git", "status"]);
    }

    #[test]
    fn test_command_with_flags() {
        let tokens = tokenize("rm -rf build/").unwrap();
        assert_eq!(tokens, vec!["rm", "-rf", "build/"]);
    }

    #[test]
    fn test_command_with_quotes() {
        let tokens = tokenize(r#"echo "hello world""#).unwrap();
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn test_command_with_single_quotes() {
        let tokens = tokenize("bash -c 'git status'").unwrap();
        assert_eq!(tokens, vec!["bash", "-c", "git status"]);
    }

    #[test]
    fn test_nested_quotes() {
        let tokens = tokenize(r#"bash -c "echo 'hello'""#).unwrap();
        assert_eq!(tokens, vec!["bash", "-c", "echo 'hello'"]);
    }
}
