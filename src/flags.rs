/// Expand combined short flags (-rf -> -r, -f)
pub fn expand_flags(tokens: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for token in tokens {
        if is_combined_short_flag(token) {
            // Skip the leading '-' and expand each char
            for c in token[1..].chars() {
                expanded.push(format!("-{}", c));
            }
        } else if token.starts_with('-') {
            expanded.push(token.clone());
        }
    }
    expanded
}

fn is_combined_short_flag(token: &str) -> bool {
    // Must start with single dash, have multiple chars after dash,
    // and not be a long flag (--) or contain =
    token.starts_with('-')
        && !token.starts_with("--")
        && token.len() > 2
        && !token.contains('=')
        && token[1..].chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_expand_combined_flags() {
        let tokens = to_vec(&["rm", "-rf", "build/"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-r", "-f"]);
    }

    #[test]
    fn test_preserve_separate_flags() {
        let tokens = to_vec(&["rm", "-r", "-f", "build/"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-r", "-f"]);
    }

    #[test]
    fn test_preserve_long_flags() {
        let tokens = to_vec(&["git", "push", "--force"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["--force"]);
    }

    #[test]
    fn test_mixed_flags() {
        let tokens = to_vec(&["cmd", "-abc", "--verbose", "-x"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-a", "-b", "-c", "--verbose", "-x"]);
    }

    #[test]
    fn test_flag_with_value() {
        // -o=file should not be expanded
        let tokens = to_vec(&["gcc", "-o=output", "-Wall"]);
        let flags = expand_flags(&tokens);
        assert_eq!(flags, vec!["-o=output", "-W", "-a", "-l", "-l"]);
    }

    #[test]
    fn test_no_flags() {
        let tokens = to_vec(&["echo", "hello"]);
        let flags = expand_flags(&tokens);
        assert!(flags.is_empty());
    }
}
