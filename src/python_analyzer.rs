//! Static analysis for Python inline code
//!
//! Uses tree-sitter queries to detect dangerous patterns in Python code.
//! This enables inspection mode (safe readonly operations) vs execution mode
//! (needs sandboxing).
//!
//! Queries are loaded from .scm files:
//! - config/queries/python_dangerous.scm - dangerous pattern detection
//! - config/queries/python_imports.scm - import extraction
//!
//! Users can override queries in ~/.config/cmdguard/queries/

use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// Default dangerous patterns query (fallback if file not found)
const DEFAULT_DANGEROUS_QUERY: &str = include_str!("../config/queries/python_dangerous.scm");

/// Default imports query (fallback if file not found)
const DEFAULT_IMPORTS_QUERY: &str = include_str!("../config/queries/python_imports.scm");

/// Configuration for Python analysis
#[derive(Debug, Clone)]
pub struct PythonConfig {
    /// Query for detecting dangerous patterns
    pub dangerous_query: String,
    /// Query for extracting imports
    pub imports_query: String,
}

impl Default for PythonConfig {
    fn default() -> Self {
        Self {
            dangerous_query: DEFAULT_DANGEROUS_QUERY.to_string(),
            imports_query: DEFAULT_IMPORTS_QUERY.to_string(),
        }
    }
}

impl PythonConfig {
    /// Load config from a directory, with fallback to defaults
    /// Looks for queries/*.scm files
    #[allow(dead_code)] // Will be used when integrated with main policy flow
    pub fn load(config_dir: &Path) -> Self {
        let dangerous_query = Self::load_query(config_dir, "python_dangerous.scm")
            .unwrap_or_else(|| DEFAULT_DANGEROUS_QUERY.to_string());

        let imports_query = Self::load_query(config_dir, "python_imports.scm")
            .unwrap_or_else(|| DEFAULT_IMPORTS_QUERY.to_string());

        Self {
            dangerous_query,
            imports_query,
        }
    }

    fn load_query(config_dir: &Path, filename: &str) -> Option<String> {
        let path = config_dir.join("queries").join(filename);
        std::fs::read_to_string(&path).ok()
    }
}

/// Result of analyzing Python code
#[derive(Debug)]
pub struct PythonAnalysis {
    /// Detected dangerous patterns
    pub patterns: Vec<Pattern>,
    /// All imports found
    pub imports: Vec<String>,
    /// Whether the code appears safe for inspection mode
    pub is_inspection_safe: bool,
}

/// A dangerous pattern detected in the code
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// The capture name from the tree-sitter query (e.g., "dangerous_import", "file_op")
    /// This is passed through directly to allow flexible policy decisions
    pub capture: String,
    /// The matched text
    pub text: String,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (0-indexed)
    pub column: usize,
}


/// Analyze Python code for dangerous patterns using default config
pub fn analyze(code: &str) -> Result<PythonAnalysis, String> {
    analyze_with_config(code, &PythonConfig::default())
}

/// Analyze Python code for dangerous patterns with custom config
pub fn analyze_with_config(code: &str, config: &PythonConfig) -> Result<PythonAnalysis, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| format!("Failed to load Python grammar: {}", e))?;

    let tree = parser
        .parse(code, None)
        .ok_or_else(|| "Failed to parse Python code".to_string())?;

    let root = tree.root_node();
    let source = code.as_bytes();

    // Find dangerous patterns using query from config
    let patterns = find_patterns(&root, source, &config.dangerous_query)?;

    // Extract imports using query from config
    let imports = extract_imports(&root, source, &config.imports_query)?;

    // Code is safe for inspection if no dangerous patterns found
    let is_inspection_safe = patterns.is_empty();

    Ok(PythonAnalysis {
        patterns,
        imports,
        is_inspection_safe,
    })
}

fn find_patterns(
    root: &tree_sitter::Node,
    source: &[u8],
    query_str: &str,
) -> Result<Vec<Pattern>, String> {
    let query = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str)
        .map_err(|e| format!("Failed to compile query: {:?}", e))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, source);

    let mut patterns = Vec::new();

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_name = query.capture_names()[capture.index as usize];

            // Skip anonymous captures (those starting with _)
            if capture_name.starts_with('_') {
                continue;
            }

            let text = node.utf8_text(source).unwrap_or("").to_string();
            let start = node.start_position();

            // Avoid duplicates for the same location
            if !patterns.iter().any(|p: &Pattern| p.line == start.row + 1 && p.column == start.column) {
                patterns.push(Pattern {
                    capture: capture_name.to_string(),
                    text,
                    line: start.row + 1,
                    column: start.column,
                });
            }
        }
    }

    Ok(patterns)
}

fn extract_imports(
    root: &tree_sitter::Node,
    source: &[u8],
    query_str: &str,
) -> Result<Vec<String>, String> {
    let query = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str)
        .map_err(|e| format!("Failed to compile imports query: {:?}", e))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, source);

    let mut imports = Vec::new();

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let text = capture.node.utf8_text(source).unwrap_or("").to_string();
            // Get root module (e.g., "os" from "os.path")
            let root_module = text.split('.').next().unwrap_or(&text).to_string();
            if !imports.contains(&root_module) {
                imports.push(root_module);
            }
        }
    }

    Ok(imports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_inspection_code() {
        let code = r#"
import json
import pandas as pd

print(pd.DataFrame.__doc__)
x = json.dumps({"a": 1})
"#;
        let result = analyze(code).unwrap();
        assert!(result.is_inspection_safe, "Expected safe, got: {:?}", result.patterns);
        assert!(result.imports.contains(&"json".to_string()));
        assert!(result.imports.contains(&"pandas".to_string()));
    }

    #[test]
    fn test_dangerous_import_os() {
        let code = "import os";
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns.len(), 1);
        assert_eq!(result.patterns[0].capture, "dangerous_import");
    }

    #[test]
    fn test_dangerous_import_subprocess() {
        let code = "from subprocess import run";
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "dangerous_import");
    }

    #[test]
    fn test_dangerous_eval() {
        let code = r#"x = eval("1 + 1")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "dynamic_exec");
    }

    #[test]
    fn test_dangerous_exec() {
        let code = r#"exec("print('hello')")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "dynamic_exec");
    }

    #[test]
    fn test_dangerous_open() {
        let code = r#"f = open("file.txt", "w")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "file_op");
    }

    #[test]
    fn test_dangerous_os_system() {
        let code = r#"os.system("rm -rf /")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "subprocess_op");
    }

    #[test]
    fn test_dangerous_subprocess_run() {
        let code = r#"subprocess.run(["ls", "-la"])"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.patterns[0].capture, "subprocess_op");
    }

    #[test]
    fn test_multiple_dangers() {
        let code = r#"
import os
eval("1+1")
open("file.txt")
"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert!(result.patterns.len() >= 3);
    }

    #[test]
    fn test_safe_print_doc() {
        let code = r#"print(some_class.__doc__)"#;
        let result = analyze(code).unwrap();
        assert!(result.is_inspection_safe);
    }

    #[test]
    fn test_safe_type_check() {
        let code = r#"print(type(x).__name__)"#;
        let result = analyze(code).unwrap();
        assert!(result.is_inspection_safe);
    }

    #[test]
    fn test_custom_config() {
        // With default config, 'os' is dangerous
        let code = "import os";
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);

        // With custom config that doesn't include 'os', it's safe
        // Only detect 'subprocess' as dangerous import
        let config = PythonConfig {
            dangerous_query: r#"
(import_statement
  name: (dotted_name) @dangerous_import
  (#match? @dangerous_import "^subprocess$"))
"#.to_string(),
            imports_query: DEFAULT_IMPORTS_QUERY.to_string(),
        };
        let result = analyze_with_config(code, &config).unwrap();
        assert!(result.is_inspection_safe);
    }
}
