//! Static analysis for Python inline code
//!
//! Uses tree-sitter queries to detect dangerous patterns in Python code.
//! This enables inspection mode (safe readonly operations) vs execution mode
//! (needs sandboxing).

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// Result of analyzing Python code
#[derive(Debug)]
pub struct PythonAnalysis {
    /// Detected dangerous patterns
    pub dangerous_patterns: Vec<DangerousPattern>,
    /// All imports found
    pub imports: Vec<String>,
    /// Whether the code appears safe for inspection mode
    pub is_inspection_safe: bool,
}

/// A dangerous pattern detected in the code
#[derive(Debug)]
pub struct DangerousPattern {
    pub kind: DangerKind,
    #[allow(dead_code)] // Used for reporting
    pub text: String,
    pub line: usize,
    #[allow(dead_code)] // Used for reporting
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants will be used as detection expands
pub enum DangerKind {
    DangerousImport,
    DangerousCall,
    FileOperation,
    NetworkOperation,
    SubprocessOperation,
    DynamicExecution,
}

impl std::fmt::Display for DangerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DangerKind::DangerousImport => write!(f, "dangerous import"),
            DangerKind::DangerousCall => write!(f, "dangerous call"),
            DangerKind::FileOperation => write!(f, "file operation"),
            DangerKind::NetworkOperation => write!(f, "network operation"),
            DangerKind::SubprocessOperation => write!(f, "subprocess operation"),
            DangerKind::DynamicExecution => write!(f, "dynamic execution"),
        }
    }
}

/// Tree-sitter query for detecting dangerous patterns
const DANGEROUS_PATTERNS_QUERY: &str = r#"
; Dangerous imports
(import_statement
  name: (dotted_name) @dangerous_import
  (#match? @dangerous_import "^(os|subprocess|socket|shutil|tempfile)$"))

(import_from_statement
  module_name: (dotted_name) @dangerous_import
  (#match? @dangerous_import "^(os|subprocess|socket|shutil|tempfile)$"))

; Dynamic execution functions
(call
  function: (identifier) @dynamic_exec
  (#match? @dynamic_exec "^(eval|exec|compile|__import__|execfile)$"))

; File operations
(call
  function: (identifier) @file_op
  (#match? @file_op "^(open|file)$"))

; os.system, os.popen, etc.
(call
  function: (attribute
    object: (identifier) @obj
    attribute: (identifier) @method)
  (#eq? @obj "os")
  (#match? @method "^(system|popen|exec|execv|execve|spawn)"))

; subprocess calls
(call
  function: (attribute
    object: (identifier) @obj
    attribute: (identifier) @method)
  (#eq? @obj "subprocess"))
"#;

/// Query for extracting all imports
const IMPORTS_QUERY: &str = r#"
; Simple import: import foo
(import_statement
  name: (dotted_name) @import)

; Aliased import: import foo as bar
(import_statement
  name: (aliased_import
    name: (dotted_name) @import))

; From import: from foo import bar
(import_from_statement
  module_name: (dotted_name) @import)
"#;

/// Analyze Python code for dangerous patterns
pub fn analyze(code: &str) -> Result<PythonAnalysis, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| format!("Failed to load Python grammar: {}", e))?;

    let tree = parser
        .parse(code, None)
        .ok_or_else(|| "Failed to parse Python code".to_string())?;

    let root = tree.root_node();
    let source = code.as_bytes();

    // Find dangerous patterns
    let dangerous_patterns = find_dangerous_patterns(&root, source)?;

    // Extract imports
    let imports = extract_imports(&root, source)?;

    // Code is safe for inspection if no dangerous patterns found
    let is_inspection_safe = dangerous_patterns.is_empty();

    Ok(PythonAnalysis {
        dangerous_patterns,
        imports,
        is_inspection_safe,
    })
}

fn find_dangerous_patterns(
    root: &tree_sitter::Node,
    source: &[u8],
) -> Result<Vec<DangerousPattern>, String> {
    let query = Query::new(&tree_sitter_python::LANGUAGE.into(), DANGEROUS_PATTERNS_QUERY)
        .map_err(|e| format!("Failed to compile query: {:?}", e))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, source);

    let mut patterns = Vec::new();

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_name = query.capture_names()[capture.index as usize];
            let text = node.utf8_text(source).unwrap_or("").to_string();
            let start = node.start_position();

            let kind = match capture_name {
                "dangerous_import" => DangerKind::DangerousImport,
                "dynamic_exec" => DangerKind::DynamicExecution,
                "file_op" => DangerKind::FileOperation,
                "obj" | "method" => {
                    // For attribute access, we need to determine the kind
                    // based on the object name
                    let obj_text = if capture_name == "obj" {
                        text.clone()
                    } else {
                        continue; // Skip method captures, we handle via obj
                    };

                    if obj_text == "os" {
                        DangerKind::SubprocessOperation
                    } else if obj_text == "subprocess" {
                        DangerKind::SubprocessOperation
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Avoid duplicates for the same location
            if !patterns.iter().any(|p: &DangerousPattern| p.line == start.row + 1 && p.column == start.column) {
                patterns.push(DangerousPattern {
                    kind,
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
) -> Result<Vec<String>, String> {
    let query = Query::new(&tree_sitter_python::LANGUAGE.into(), IMPORTS_QUERY)
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
        assert!(result.is_inspection_safe, "Expected safe, got: {:?}", result.dangerous_patterns);
        assert!(result.imports.contains(&"json".to_string()));
        assert!(result.imports.contains(&"pandas".to_string()));
    }

    #[test]
    fn test_dangerous_import_os() {
        let code = "import os";
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns.len(), 1);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::DangerousImport);
    }

    #[test]
    fn test_dangerous_import_subprocess() {
        let code = "from subprocess import run";
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::DangerousImport);
    }

    #[test]
    fn test_dangerous_eval() {
        let code = r#"x = eval("1 + 1")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::DynamicExecution);
    }

    #[test]
    fn test_dangerous_exec() {
        let code = r#"exec("print('hello')")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::DynamicExecution);
    }

    #[test]
    fn test_dangerous_open() {
        let code = r#"f = open("file.txt", "w")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::FileOperation);
    }

    #[test]
    fn test_dangerous_os_system() {
        let code = r#"os.system("rm -rf /")"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::SubprocessOperation);
    }

    #[test]
    fn test_dangerous_subprocess_run() {
        let code = r#"subprocess.run(["ls", "-la"])"#;
        let result = analyze(code).unwrap();
        assert!(!result.is_inspection_safe);
        assert_eq!(result.dangerous_patterns[0].kind, DangerKind::SubprocessOperation);
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
        assert!(result.dangerous_patterns.len() >= 3);
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
}
