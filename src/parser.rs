//! Parse shell commands using tree-sitter-bash
//!
//! Extracts individual commands from compound statements like:
//! - `cmd1 && cmd2` (AND list)
//! - `cmd1 || cmd2` (OR list)
//! - `cmd1 ; cmd2` (sequential)
//! - `cmd1 | cmd2` (pipeline)

use serde::Serialize;
use tree_sitter::Parser;

/// Shell redirection attached to a parsed command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellRedirect {
    /// Raw redirection text, e.g. `> out.txt` or `2>&1`.
    pub raw: String,
    /// Redirection operator, e.g. `>`, `>>`, `<`, `>&`.
    pub operator: String,
    /// Optional file descriptor prefix, e.g. `2` in `2>err.log`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<String>,
    /// Parsed redirection target, if one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Coarse redirection class for policy rules.
    pub kind: ShellRedirectKind,
    /// True when this redirect can write to a filesystem target.
    pub writes_to_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellRedirectKind {
    Read,
    Write,
    Append,
    ReadWrite,
    Heredoc,
    HereString,
    FdDuplicate,
    Unknown,
}

/// A single command extracted from a compound statement
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// The command text
    pub text: String,
    /// Redirections attached to this command, excluded from `text` but
    /// preserved for policy evaluation.
    pub redirections: Vec<ShellRedirect>,
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
                    redirections: vec![],
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
                redirections: vec![],
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
        // Redirected statement: command with redirects (e.g., echo foo >> file)
        // Keep command text clean for tokenization while preserving redirects
        // so policy can reason about shell-level writes.
        "redirected_statement" => {
            let mut redirects = Vec::new();
            let start_len = commands.len();

            for i in 0..node.child_count() as u32 {
                if let Some(child) = node.child(i) {
                    if child.kind().contains("redirect") {
                        collect_redirections(&child, source, &mut redirects);
                    } else {
                        extract_commands_recursive(&child, source, commands);
                    }
                }
            }

            if commands.len() > start_len {
                // A redirect on a pipeline applies to the pipeline's final
                // command stdout; for a simple command there is only one.
                if let Some(last) = commands.last_mut() {
                    last.redirections.extend(redirects);
                }
                return; // Don't recurse further
            }

            // Fallback: if no command child found, use full text
            let text = node_text(node, source);
            if !text.trim().is_empty() {
                commands.push(ParsedCommand {
                    text: text.trim().to_string(),
                    redirections: redirects,
                    position: commands.len(),
                    chain_length: 0, // Will be updated later
                    next_operator: None,
                });
            }
        }
        // Simple command
        "command" | "simple_command" => {
            let text = node_text(node, source);
            if !text.trim().is_empty() {
                commands.push(ParsedCommand {
                    text: text.trim().to_string(),
                    redirections: vec![],
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

fn collect_redirections(
    node: &tree_sitter::Node,
    source: &str,
    redirects: &mut Vec<ShellRedirect>,
) {
    if node.kind().contains("redirect") {
        let raw = node_text(node, source);
        if let Some(redirect) = parse_redirect(&raw) {
            redirects.push(redirect);
        }
        return;
    }

    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            collect_redirections(&child, source, redirects);
        }
    }
}

fn parse_redirect(raw: &str) -> Option<ShellRedirect> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let (fd, operator, rest) = split_redirect(raw)?;
    let target = parse_redirect_target(rest);
    let kind = classify_redirect(&operator, target.as_deref());
    let writes_to_file = matches!(
        kind,
        ShellRedirectKind::Write | ShellRedirectKind::Append | ShellRedirectKind::ReadWrite
    );

    Some(ShellRedirect {
        raw: raw.to_string(),
        operator,
        fd,
        target,
        kind,
        writes_to_file,
    })
}

fn split_redirect(raw: &str) -> Option<(Option<String>, String, &str)> {
    let fd_end = raw
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    let fd = if fd_end > 0 {
        Some(raw[..fd_end].to_string())
    } else {
        None
    };

    let rest = raw[fd_end..].trim_start();
    const OPERATORS: &[&str] = &[
        "&>>", "&>", "<<<", "<<-", "<<", "<>", ">>", ">|", ">&", "<&", ">", "<",
    ];

    for operator in OPERATORS {
        if let Some(target) = rest.strip_prefix(operator) {
            return Some((fd, operator.to_string(), target));
        }
    }

    None
}

fn parse_redirect_target(rest: &str) -> Option<String> {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return None;
    }

    shlex::split(trimmed)
        .and_then(|tokens| tokens.into_iter().next())
        .or_else(|| trimmed.split_whitespace().next().map(|s| s.to_string()))
}

fn classify_redirect(operator: &str, target: Option<&str>) -> ShellRedirectKind {
    match operator {
        ">" | ">|" | "&>" => ShellRedirectKind::Write,
        ">>" | "&>>" => ShellRedirectKind::Append,
        "<>" => ShellRedirectKind::ReadWrite,
        "<" => ShellRedirectKind::Read,
        "<<" | "<<-" => ShellRedirectKind::Heredoc,
        "<<<" => ShellRedirectKind::HereString,
        "<&" => ShellRedirectKind::FdDuplicate,
        ">&" if target.map(is_fd_target).unwrap_or(false) => ShellRedirectKind::FdDuplicate,
        ">&" => ShellRedirectKind::Write,
        _ => ShellRedirectKind::Unknown,
    }
}

fn is_fd_target(target: &str) -> bool {
    target == "-" || target.chars().all(|c| c.is_ascii_digit())
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
        assert!(result.commands[0].redirections.is_empty());
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

    // Integration tests for real-world compound command scenarios

    #[test]
    fn test_dangerous_compound_detected() {
        // This is the motivating example from the design doc
        let result = parse_command("echo foo && rm -rf /");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.commands[0].text, "echo foo");
        assert_eq!(result.commands[1].text, "rm -rf /");
    }

    #[test]
    fn test_redirect_excluded() {
        let result = parse_command("echo '.gitignore' >> .gitignore && git add .");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        // Redirect should NOT be part of command text, but it must remain
        // visible to policy evaluation.
        assert!(!result.commands[0].text.contains(">>"));
        assert_eq!(result.commands[0].text, "echo '.gitignore'");
        assert_eq!(result.commands[0].redirections.len(), 1);
        assert_eq!(result.commands[0].redirections[0].operator, ">>");
        assert_eq!(
            result.commands[0].redirections[0].target.as_deref(),
            Some(".gitignore")
        );
        assert_eq!(
            result.commands[0].redirections[0].kind,
            ShellRedirectKind::Append
        );
        assert!(result.commands[0].redirections[0].writes_to_file);
        assert_eq!(result.commands[1].text, "git add .");
        assert!(result.commands[1].redirections.is_empty());
    }

    #[test]
    fn test_stderr_redirect_excluded() {
        let result = parse_command("python -m pytest 2>&1 | tail -30");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        // 2>&1 should not appear in command text
        assert!(!result.commands[0].text.contains("2>&1"));
        assert_eq!(result.commands[0].text, "python -m pytest");
        assert_eq!(result.commands[0].redirections.len(), 1);
        assert_eq!(result.commands[0].redirections[0].operator, ">&");
        assert_eq!(result.commands[0].redirections[0].fd.as_deref(), Some("2"));
        assert_eq!(
            result.commands[0].redirections[0].target.as_deref(),
            Some("1")
        );
        assert_eq!(
            result.commands[0].redirections[0].kind,
            ShellRedirectKind::FdDuplicate
        );
        assert!(!result.commands[0].redirections[0].writes_to_file);
        assert_eq!(result.commands[1].text, "tail -30");
    }

    #[test]
    fn test_file_write_redirection_visible() {
        let result = parse_command("cat /etc/passwd > secrets.txt");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].text, "cat /etc/passwd");
        assert_eq!(result.commands[0].redirections.len(), 1);

        let redirect = &result.commands[0].redirections[0];
        assert_eq!(redirect.raw, "> secrets.txt");
        assert_eq!(redirect.operator, ">");
        assert_eq!(redirect.target.as_deref(), Some("secrets.txt"));
        assert_eq!(redirect.kind, ShellRedirectKind::Write);
        assert!(redirect.writes_to_file);
    }

    #[test]
    fn test_pipeline_redirect_attached_to_redirected_command() {
        let result = parse_command("git log | grep TODO > /tmp/todos.txt");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        assert!(result.commands[0].redirections.is_empty());
        assert_eq!(result.commands[1].text, "grep TODO");
        assert_eq!(result.commands[1].redirections.len(), 1);
        assert_eq!(
            result.commands[1].redirections[0].target.as_deref(),
            Some("/tmp/todos.txt")
        );
        assert!(result.commands[1].redirections[0].writes_to_file);
    }

    #[test]
    fn test_complex_real_world() {
        let result = parse_command("cd /tmp && git clone repo && cd repo && make");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 4);
    }
}
