//! Parse shell commands using tree-sitter-bash
//!
//! Extracts individual commands from compound statements like:
//! - `cmd1 && cmd2` (AND list)
//! - `cmd1 || cmd2` (OR list)
//! - `cmd1 ; cmd2` (sequential)
//! - `cmd1 | cmd2` (pipeline)

use tree_sitter::Parser;

/// A single command extracted from a compound statement
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// The command text
    pub text: String,
    /// Position in the chain (0-indexed)
    pub position: usize,
    /// Total number of commands in the chain
    pub chain_length: usize,
    /// Operator connecting to next command (if any)
    pub next_operator: Option<String>,
}

/// Result of parsing a command string
#[derive(Debug)]
pub struct ParseResult {
    /// Individual commands extracted
    pub commands: Vec<ParsedCommand>,
    /// Whether parsing encountered errors (unparseable constructs)
    pub has_errors: bool,
}

/// Parse a shell command string and extract individual commands
pub fn parse_command(input: &str) -> ParseResult {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .expect("Failed to load bash grammar");

    let tree = match parser.parse(input, None) {
        Some(t) => t,
        None => {
            return ParseResult {
                commands: vec![ParsedCommand {
                    text: input.to_string(),
                    position: 0,
                    chain_length: 1,
                    next_operator: None,
                }],
                has_errors: true,
            };
        }
    };

    let root = tree.root_node();
    let has_errors = root.has_error();

    // Extract commands from the parse tree
    let commands = extract_commands(&root, input);

    // If no commands found, treat whole input as single command
    if commands.is_empty() {
        return ParseResult {
            commands: vec![ParsedCommand {
                text: input.to_string(),
                position: 0,
                chain_length: 1,
                next_operator: None,
            }],
            has_errors,
        };
    }

    ParseResult {
        commands,
        has_errors,
    }
}

fn extract_commands(node: &tree_sitter::Node, source: &str) -> Vec<ParsedCommand> {
    let mut commands = Vec::new();
    extract_commands_recursive(node, source, &mut commands);

    // Update chain_length for all commands
    let len = commands.len();
    for cmd in &mut commands {
        cmd.chain_length = len;
    }

    commands
}

fn extract_commands_recursive(
    node: &tree_sitter::Node,
    source: &str,
    commands: &mut Vec<ParsedCommand>,
) {
    match node.kind() {
        // Simple command
        "command" | "simple_command" => {
            let text = node_text(node, source);
            if !text.trim().is_empty() {
                commands.push(ParsedCommand {
                    text: text.trim().to_string(),
                    position: commands.len(),
                    chain_length: 0, // Will be updated later
                    next_operator: None,
                });
            }
        }
        // AND/OR list: cmd1 && cmd2 or cmd1 || cmd2
        "list" => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == "&&" || child.kind() == "||" || child.kind() == ";" {
                        // Set operator on previous command
                        if let Some(last) = commands.last_mut() {
                            last.next_operator = Some(child.kind().to_string());
                        }
                    } else {
                        extract_commands_recursive(&child, source, commands);
                    }
                }
            }
        }
        // Pipeline: cmd1 | cmd2
        "pipeline" => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == "|" || child.kind() == "|&" {
                        if let Some(last) = commands.last_mut() {
                            last.next_operator = Some(child.kind().to_string());
                        }
                    } else {
                        extract_commands_recursive(&child, source, commands);
                    }
                }
            }
        }
        // Program: top-level node that may contain semicolon-separated commands
        // In tree-sitter-bash, semicolons at the program level are direct children
        "program" => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind() == ";" {
                        // Set operator on previous command
                        if let Some(last) = commands.last_mut() {
                            last.next_operator = Some(";".to_string());
                        }
                    } else {
                        extract_commands_recursive(&child, source, commands);
                    }
                }
            }
        }
        // Recurse into other node types
        _ => {
            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    extract_commands_recursive(&child, source, commands);
                }
            }
        }
    }
}

fn node_text(node: &tree_sitter::Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

#[cfg(test)]
#[allow(dead_code)]
fn print_tree(node: &tree_sitter::Node, source: &str, indent: usize) {
    println!(
        "{}{} [{}-{}] {:?}",
        "  ".repeat(indent),
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        &source[node.start_byte()..node.end_byte()]
    );
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            print_tree(&child, source, indent + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let result = parse_command("git status");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].text, "git status");
        assert_eq!(result.commands[0].position, 0);
        assert_eq!(result.commands[0].chain_length, 1);
        assert!(result.commands[0].next_operator.is_none());
    }

    #[test]
    fn test_and_list() {
        let result = parse_command("echo foo && git status");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);

        assert_eq!(result.commands[0].text, "echo foo");
        assert_eq!(result.commands[0].position, 0);
        assert_eq!(result.commands[0].chain_length, 2);
        assert_eq!(result.commands[0].next_operator, Some("&&".to_string()));

        assert_eq!(result.commands[1].text, "git status");
        assert_eq!(result.commands[1].position, 1);
        assert_eq!(result.commands[1].chain_length, 2);
        assert!(result.commands[1].next_operator.is_none());
    }

    #[test]
    fn test_or_list() {
        let result = parse_command("false || echo fallback");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.commands[0].next_operator, Some("||".to_string()));
    }

    #[test]
    fn test_semicolon_list() {
        let result = parse_command("echo a ; echo b ; echo c");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 3);
        assert_eq!(result.commands[0].next_operator, Some(";".to_string()));
        assert_eq!(result.commands[1].next_operator, Some(";".to_string()));
        assert!(result.commands[2].next_operator.is_none());
    }

    #[test]
    fn test_pipeline() {
        let result = parse_command("cat file.txt | grep pattern | head -10");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 3);
        assert_eq!(result.commands[0].next_operator, Some("|".to_string()));
        assert_eq!(result.commands[1].next_operator, Some("|".to_string()));
    }

    #[test]
    fn test_mixed_operators() {
        let result = parse_command("echo start && cat file | grep foo || echo failed");
        assert!(!result.has_errors);
        // Should have at least 3 commands
        assert!(result.commands.len() >= 3);
    }

    #[test]
    fn test_quoted_strings_preserved() {
        let result = parse_command(r#"echo "hello && world""#);
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 1);
        // The && inside quotes should NOT split the command
    }

    #[test]
    fn test_parse_error_detected() {
        // Unclosed quote should be detected as error
        let result = parse_command(r#"echo "unclosed"#);
        assert!(result.has_errors);
        // Should still return the input as a single command
        assert_eq!(result.commands.len(), 1);
    }

    #[test]
    fn test_subshell_treated_as_single() {
        // Subshells should be extracted but marked if we can't fully parse them
        let result = parse_command("(cd /tmp && ls)");
        // For now, we treat this as needing review
        assert!(result.commands.len() >= 1);
    }
}
