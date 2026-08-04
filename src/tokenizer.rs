/// Tokenize a command string respecting quotes
pub fn tokenize(command: &str) -> Result<Vec<String>, String> {
    let normalized = remove_line_continuations(command);
    shlex::split(&normalized).ok_or_else(|| "Failed to tokenize command".to_string())
}

/// Remove shell line continuations before tokenization.
///
/// Bash removes an unquoted or double-quoted backslash followed by a newline
/// before splitting the command into words. `shlex` otherwise exposes those
/// continuations as empty arguments, which can shift positional assignments.
/// Backslash-newline remains literal inside single quotes.
fn remove_line_continuations(command: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum QuoteMode {
        Unquoted,
        Single,
        Double,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FrameKind {
        TopLevel,
        CommandSubstitution { paren_depth: usize },
        Backticks,
    }

    #[derive(Clone, Copy)]
    struct Frame {
        kind: FrameKind,
        quote: QuoteMode,
    }

    let mut result = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    let mut frames = vec![Frame {
        kind: FrameKind::TopLevel,
        quote: QuoteMode::Unquoted,
    }];

    while let Some(ch) = chars.next() {
        let frame = frames.last_mut().expect("top-level tokenizer frame");

        match frame.quote {
            QuoteMode::Single => {
                result.push(ch);
                if ch == '\'' {
                    frame.quote = QuoteMode::Unquoted;
                }
            }
            QuoteMode::Unquoted | QuoteMode::Double if ch == '\\' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    continue;
                }

                // Consume the escaped character with the backslash so an
                // escaped quote cannot change our quote state. This also
                // preserves the parity of consecutive backslashes.
                result.push(ch);
                if let Some(next) = chars.next() {
                    result.push(next);
                }
            }
            QuoteMode::Unquoted | QuoteMode::Double if ch == '$' && chars.peek() == Some(&'(') => {
                result.push(ch);
                result.push(chars.next().expect("peeked opening parenthesis"));
                frames.push(Frame {
                    kind: FrameKind::CommandSubstitution { paren_depth: 1 },
                    quote: QuoteMode::Unquoted,
                });
            }
            QuoteMode::Unquoted | QuoteMode::Double if ch == '`' => {
                result.push(ch);
                if frame.kind == FrameKind::Backticks {
                    frames.pop();
                } else {
                    frames.push(Frame {
                        kind: FrameKind::Backticks,
                        quote: QuoteMode::Unquoted,
                    });
                }
            }
            QuoteMode::Unquoted => {
                result.push(ch);

                if let FrameKind::CommandSubstitution { paren_depth } = &mut frame.kind {
                    match ch {
                        '(' => {
                            *paren_depth += 1;
                            continue;
                        }
                        ')' => {
                            *paren_depth -= 1;
                            if *paren_depth == 0 {
                                frames.pop();
                            }
                            continue;
                        }
                        _ => {}
                    }
                }

                match ch {
                    '\'' => frame.quote = QuoteMode::Single,
                    '"' => frame.quote = QuoteMode::Double,
                    _ => {}
                }
            }
            QuoteMode::Double => {
                result.push(ch);
                if ch == '"' {
                    frame.quote = QuoteMode::Unquoted;
                }
            }
        }
    }

    result
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

    #[test]
    fn test_unquoted_line_continuations_do_not_create_empty_tokens() {
        let tokens =
            tokenize("curl https://example.com \\\n  -H 'Accept: application/json' \\\n  -d '{}'")
                .unwrap();

        assert_eq!(
            tokens,
            vec![
                "curl",
                "https://example.com",
                "-H",
                "Accept: application/json",
                "-d",
                "{}"
            ]
        );
    }

    #[test]
    fn test_double_quoted_line_continuation_joins_word() {
        let tokens = tokenize("echo \"hello\\\nworld\"").unwrap();
        assert_eq!(tokens, vec!["echo", "helloworld"]);
    }

    #[test]
    fn test_unquoted_line_continuation_joins_word() {
        let tokens = tokenize("curl -X PO\\\nST https://example.com").unwrap();
        assert_eq!(tokens, vec!["curl", "-X", "POST", "https://example.com"]);
    }

    #[test]
    fn test_single_quoted_backslash_newline_remains_literal() {
        let tokens = tokenize("printf '%s' 'hello\\\nworld'").unwrap();
        assert_eq!(tokens, vec!["printf", "%s", "hello\\\nworld"]);
    }

    #[test]
    fn test_escaped_backslash_does_not_continue_line() {
        let tokens = tokenize("printf '%s' foo\\\\\nbar").unwrap();
        assert_eq!(tokens, vec!["printf", "%s", "foo\\", "bar"]);
    }

    #[test]
    fn test_intentional_empty_argument_is_preserved() {
        let tokens = tokenize("printf '%s' \"\"").unwrap();
        assert_eq!(tokens, vec!["printf", "%s", ""]);
    }

    #[test]
    fn test_backslash_spaces_newline_is_not_a_continuation() {
        let tokens = tokenize("printf '%s' foo\\  \nbar").unwrap();
        assert_eq!(tokens, vec!["printf", "%s", "foo ", "bar"]);
    }

    #[test]
    fn test_inner_single_quotes_in_command_substitution_preserve_continuation() {
        let command = "echo \"$(printf '%s' 'hello\\\nworld')\"";
        assert_eq!(remove_line_continuations(command), command);
    }

    #[test]
    fn test_command_substitution_removes_its_unquoted_continuation() {
        let command = "echo \"$(printf '%s' hello\\\nworld)\"";
        assert_eq!(
            remove_line_continuations(command),
            "echo \"$(printf '%s' helloworld)\""
        );
    }

    #[test]
    fn test_inner_single_quotes_in_backticks_preserve_continuation() {
        let command = "echo \"`printf '%s' 'hello\\\nworld'`\"";
        assert_eq!(remove_line_continuations(command), command);
    }
}
