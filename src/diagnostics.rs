use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLocation {
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub help: Option<String>,
    pub location: Option<DiagnosticLocation>,
}

impl Diagnostic {
    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            help: None,
            location: None,
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            help: None,
            location: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_location(mut self, path: impl Into<PathBuf>, line: usize) -> Self {
        self.location = Some(DiagnosticLocation {
            path: path.into(),
            line,
        });
        self
    }
}

pub fn collect_policy_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.exists() {
        return Ok(vec![]);
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read policy directory {:?}: {}", dir, e))?;

    let mut files = vec![];
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rego") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn loaded_policy_file_sets(policy_dir: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let base_dir = policy_dir.join("base");
    let base_files = collect_policy_files(&base_dir)?;

    if base_files.is_empty() {
        return Ok((vec![], collect_policy_files(policy_dir)?));
    }

    let user_files = collect_policy_files(&policy_dir.join("policies"))?;
    Ok((base_files, user_files))
}

pub fn lint_policy_sources(base_files: &[PathBuf], user_files: &[PathBuf]) -> Vec<Diagnostic> {
    let base_rule_names = collect_rule_names(base_files);
    let mut diagnostics = vec![];

    for path in user_files {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(e) => {
                diagnostics.push(
                    Diagnostic::error(
                        "policy/read-error",
                        format!("Failed to read policy file {}: {}", path.display(), e),
                    )
                    .with_location(path, 1),
                );
                continue;
            }
        };

        diagnostics.extend(lint_policy_source(path, &contents, &base_rule_names));
    }

    diagnostics
}

fn collect_rule_names(files: &[PathBuf]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let rule_re = Regex::new(r#"rules\s*\[\s*"([^"]+)"\s*\]"#).expect("valid rule regex");

    for path in files {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        let stripped = strip_comments(&contents);

        for captures in rule_re.captures_iter(&stripped) {
            names.insert(captures[1].to_string());
        }
    }

    names
}

fn lint_policy_source(
    path: &Path,
    contents: &str,
    base_rule_names: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    // Blank out comments (preserving length/line numbers) so none of the
    // lints below can mistake commented-out policy text for live rules.
    let stripped = strip_comments(contents);
    let mut diagnostics = vec![];
    diagnostics.extend(lint_high_priority_allow_calls(path, &stripped));
    diagnostics.extend(lint_high_priority_allow_objects(path, &stripped));
    diagnostics.extend(lint_rule_name_collisions(path, &stripped, base_rule_names));
    diagnostics
}

/// Replace `#` comments (line-start or trailing) with spaces, leaving
/// newlines and all other byte offsets untouched so callers can still map
/// positions in the returned string back to line numbers in the original.
/// `#` characters inside quoted string literals are left alone.
fn strip_comments(contents: &str) -> String {
    let bytes = contents.as_bytes();
    let mut out = bytes.to_vec();
    let mut in_string: Option<u8> = None;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        if let Some(quote) = in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match c {
            b'"' | b'\'' => {
                in_string = Some(c);
                i += 1;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| contents.to_string())
}

/// Find each call site of `name(` in `contents`, respecting word boundaries
/// (so e.g. `xallow_at(` is not mistaken for a call to `allow_at`).
/// Returns, for each call, the byte offset of the start of `name`, the byte
/// offset of its matching closing paren, and the argument-list text between
/// its own matching parentheses (quote-aware, so nested `(`/`)` inside
/// string literals don't confuse the depth count and the scan never runs
/// past this call's own closing paren into later text).
fn find_call_sites<'a>(contents: &'a str, name: &str) -> Vec<(usize, usize, &'a str)> {
    let bytes = contents.as_bytes();
    let mut sites = vec![];
    let mut search_from = 0;

    while let Some(rel) = contents[search_from..].find(name) {
        let name_start = search_from + rel;
        let before_ok = name_start == 0 || !is_ident_byte(bytes[name_start - 1]);
        let name_end = name_start + name.len();

        if !before_ok {
            search_from = name_end;
            continue;
        }

        let mut idx = name_end;
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }

        if idx >= bytes.len() || bytes[idx] != b'(' {
            search_from = name_end;
            continue;
        }

        let args_start = idx + 1;
        match matching_close_paren(contents, args_start) {
            Some(args_end) => {
                sites.push((name_start, args_end, &contents[args_start..args_end]));
                search_from = args_end + 1;
            }
            None => {
                // Unbalanced call; nothing further to salvage from here.
                break;
            }
        }
    }

    sites
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Given the byte offset just past an opening `(`, return the offset of the
/// matching `)` (depth-aware, quote-aware). Never crosses into a later,
/// unrelated call because it only advances through this call's own nesting.
fn matching_close_paren(contents: &str, start: usize) -> Option<usize> {
    let bytes = contents.as_bytes();
    let mut depth = 1i32;
    let mut i = start;
    let mut in_string: Option<u8> = None;

    while i < bytes.len() {
        let c = bytes[i];
        if let Some(quote) = in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match c {
            b'"' | b'\'' => in_string = Some(c),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Split a call's own argument-list text on top-level commas (quote- and
/// paren-depth-aware), returning the trimmed segments.
fn split_top_level_args(args: &str) -> Vec<&str> {
    let bytes = args.as_bytes();
    let mut parts = vec![];
    let mut start = 0;
    let mut depth = 0i32;
    let mut in_string: Option<u8> = None;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        if let Some(quote) = in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match c {
            b'"' | b'\'' => in_string = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(args[start..].trim());

    parts
}

fn lint_high_priority_allow_calls(path: &Path, contents: &str) -> Vec<Diagnostic> {
    find_call_sites(contents, "allow_at")
        .into_iter()
        .filter_map(|(start, close_paren, args)| {
            let last_arg = split_top_level_args(args).into_iter().next_back()?;
            let priority = last_arg.parse::<u32>().ok()?;
            if priority <= 50 {
                return None;
            }

            let call_text = &contents[start..=close_paren];
            Some(high_priority_allow_diagnostic(
                path, contents, start, call_text, priority,
            ))
        })
        .collect()
}

fn lint_high_priority_allow_objects(path: &Path, contents: &str) -> Vec<Diagnostic> {
    let rule_object_re = Regex::new(r#"(?s)rules\s*\[\s*"[^"]+"\s*\]\s*:=\s*\{(.*?)\}\s*if"#)
        .expect("valid rule object regex");
    let decision_re = Regex::new(r#""decision"\s*:\s*"allow""#).expect("valid decision regex");
    let priority_re = Regex::new(r#""priority"\s*:\s*([0-9]+)"#).expect("valid priority regex");

    rule_object_re
        .captures_iter(contents)
        .filter_map(|captures| {
            let body = captures.get(1)?;
            if !decision_re.is_match(body.as_str()) {
                return None;
            }

            let priority = priority_re
                .captures(body.as_str())?
                .get(1)?
                .as_str()
                .parse::<u32>()
                .ok()?;
            if priority <= 50 {
                return None;
            }

            let matched = captures.get(0)?;
            Some(high_priority_allow_diagnostic(
                path,
                contents,
                matched.start(),
                matched.as_str(),
                priority,
            ))
        })
        .collect()
}

fn high_priority_allow_diagnostic(
    path: &Path,
    contents: &str,
    offset: usize,
    context: &str,
    priority: u32,
) -> Diagnostic {
    let message = if priority > 100 {
        format!(
            "allow priority {} is above deny priority 100 and can suppress base deny rules",
            priority
        )
    } else {
        format!(
            "allow priority {} is above ask priority 50 and can suppress unrelated ask rules",
            priority
        )
    };

    let mut help =
        "Prefer extension tables for base-policy tuning; use high-priority allow rules only as an expert escape hatch."
            .to_string();

    if context.to_ascii_lowercase().contains("redirect") {
        help.push_str(
            " For /dev/null redirects, use allowed_redirect_targets[\"/dev/null\"] := true.",
        );
    }

    Diagnostic::warning("policy/high-priority-allow", message)
        .with_help(help)
        .with_location(path, line_number(contents, offset))
}

fn lint_rule_name_collisions(
    path: &Path,
    contents: &str,
    base_rule_names: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    if base_rule_names.is_empty() {
        return vec![];
    }

    let rule_re = Regex::new(r#"rules\s*\[\s*"([^"]+)"\s*\]"#).expect("valid rule regex");
    rule_re
        .captures_iter(contents)
        .filter_map(|captures| {
            let rule_name = captures.get(1)?.as_str();
            if !base_rule_names.contains(rule_name) {
                return None;
            }

            Some(
                Diagnostic::warning(
                    "policy/rule-name-collision",
                    format!("user policy defines rule {:?}, which is also defined by base policy", rule_name),
                )
                .with_help(
                    "Use a distinct user rule name unless you intentionally want to replace a base rule key.",
                )
                .with_location(path, line_number(contents, captures.get(0)?.start())),
            )
        })
        .collect()
}

fn line_number(contents: &str, offset: usize) -> usize {
    contents[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_priority_allow_call_warns() {
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

rules["allow_redirect"] := allow_at("redirect allowed", 51) if {
	input.redirections
}
"#,
            &BTreeSet::new(),
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "policy/high-priority-allow");
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert_eq!(diagnostics[0].location.as_ref().unwrap().line, 5);
        assert!(diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("allowed_redirect_targets"));
    }

    #[test]
    fn low_priority_allow_call_is_ok() {
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

rules["allow_safe"] := allow_at("safe", 25) if {
	input.binary_name == "safe"
}
"#,
            &BTreeSet::new(),
        );

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn allow_at_with_non_literal_priority_does_not_leak_into_next_call() {
        // Regression: a lazy, unbounded regex used to scan past this call's
        // own closing paren and pick up the priority literal from the
        // unrelated deny_at call below it.
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

rules["allow_var"] := allow_at("reason", some_prio) if {
	input.something
}

rules["deny_thing"] := deny_at("deny it", 100) if {
	input.other
}
"#,
            &BTreeSet::new(),
        );

        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "policy/high-priority-allow"),
            "expected no high-priority-allow warning, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn commented_out_allow_at_is_ignored() {
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

# rules["allow_redirect"] := allow_at("redirect allowed", 99) if {
#	input.redirections
# }

rules["allow_safe"] := allow_at("safe", 25) if {
	input.binary_name == "safe"
}
"#,
            &BTreeSet::new(),
        );

        assert!(
            diagnostics.is_empty(),
            "expected no findings for commented-out call, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn hash_inside_string_literal_is_not_treated_as_comment() {
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

rules["allow_hash"] := allow_at("uses a # inside the reason string", 99) if {
	input.binary_name == "safe"
}
"#,
            &BTreeSet::new(),
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "policy/high-priority-allow");
    }

    #[test]
    fn rule_name_collision_warns() {
        let base_rule_names = BTreeSet::from(["ask_shell_output_redirection".to_string()]);
        let diagnostics = lint_policy_source(
            Path::new("custom.rego"),
            r#"
package cmdguard
import rego.v1

rules["ask_shell_output_redirection"] := allow("replace") if {
	input.binary_name == "echo"
}
"#,
            &base_rule_names,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "policy/rule-name-collision");
    }
}
