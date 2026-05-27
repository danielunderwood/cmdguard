//! Generic tree-sitter query runner
//!
//! Runs arbitrary tree-sitter queries against code in supported languages.

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

/// Supported languages for querying
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryLanguage {
    Python,
    Bash,
}

impl QueryLanguage {
    /// Parse language from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "python" | "py" => Some(Self::Python),
            "bash" | "sh" | "shell" => Some(Self::Bash),
            _ => None,
        }
    }

    /// Get the tree-sitter language
    fn tree_sitter_language(&self) -> Language {
        match self {
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
        }
    }
}

/// A match from running a query
#[derive(Debug, Clone)]
pub struct QueryMatch {
    /// The capture name from the query
    pub capture: String,
    /// The matched text
    pub text: String,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (0-indexed)
    pub column: usize,
}

/// Run a tree-sitter query against code
pub fn run_query(
    lang: QueryLanguage,
    query_str: &str,
    code: &str,
) -> Result<Vec<QueryMatch>, String> {
    let ts_lang = lang.tree_sitter_language();

    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| format!("Failed to set language: {}", e))?;

    let tree = parser
        .parse(code, None)
        .ok_or_else(|| "Failed to parse code".to_string())?;

    let query =
        Query::new(&ts_lang, query_str).map_err(|e| format!("Failed to compile query: {:?}", e))?;

    let root = tree.root_node();
    let source = code.as_bytes();

    let mut cursor = QueryCursor::new();
    let mut matches_iter = cursor.matches(&query, root, source);

    let mut results = Vec::new();

    while let Some(m) = matches_iter.next() {
        for capture in m.captures {
            let capture_name = query.capture_names()[capture.index as usize];

            // Skip anonymous captures (those starting with _)
            if capture_name.starts_with('_') {
                continue;
            }

            let node = capture.node;
            let text = node.utf8_text(source).unwrap_or("").to_string();
            let start = node.start_position();

            results.push(QueryMatch {
                capture: capture_name.to_string(),
                text,
                line: start.row + 1,
                column: start.column,
            });
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_query() {
        let query = r#"(import_statement name: (dotted_name) @import)"#;
        let code = "import os\nimport json";

        let matches = run_query(QueryLanguage::Python, query, code).unwrap();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].capture, "import");
        assert_eq!(matches[0].text, "os");
        assert_eq!(matches[1].text, "json");
    }

    #[test]
    fn test_bash_query() {
        let query = r#"(command name: (command_name) @cmd)"#;
        let code = "ls -la && echo hello";

        let matches = run_query(QueryLanguage::Bash, query, code).unwrap();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].capture, "cmd");
        assert_eq!(matches[0].text, "ls");
        assert_eq!(matches[1].text, "echo");
    }

    #[test]
    fn test_language_from_str() {
        assert_eq!(
            QueryLanguage::from_str("python"),
            Some(QueryLanguage::Python)
        );
        assert_eq!(QueryLanguage::from_str("py"), Some(QueryLanguage::Python));
        assert_eq!(QueryLanguage::from_str("bash"), Some(QueryLanguage::Bash));
        assert_eq!(QueryLanguage::from_str("sh"), Some(QueryLanguage::Bash));
        assert_eq!(QueryLanguage::from_str("unknown"), None);
    }
}
