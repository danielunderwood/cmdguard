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
    todo!()
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
}
