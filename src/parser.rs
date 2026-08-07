//! Parse shell commands using tree-sitter-bash
//!
//! Extracts individual commands from compound statements like:
//! - `cmd1 && cmd2` (AND list)
//! - `cmd1 || cmd2` (OR list)
//! - `cmd1 ; cmd2` (sequential)
//! - `cmd1 | cmd2` (pipeline)

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use tree_sitter::{Node, Parser};

/// Whether a shell-derived value is exact, has multiple possible values, or
/// cannot be resolved safely from the submitted command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Known,
    Ambiguous,
    Unknown,
}

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
    /// Absolute nominal paths the target may resolve to at execution time.
    /// Empty when the target is dynamic or the effective cwd is unknown.
    pub resolved_targets: Vec<String>,
    /// Confidence in `resolved_targets`.
    pub target_resolution: ResolutionStatus,
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
    /// Absolute nominal working directories in which this command may run.
    pub effective_cwds: Vec<String>,
    /// Confidence in `effective_cwds`.
    pub effective_cwd_resolution: ResolutionStatus,
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
pub fn parse_command(input: &str, initial_cwd: &Path) -> ParseResult {
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
                    effective_cwds: vec![absolute_initial_cwd(initial_cwd)
                        .to_string_lossy()
                        .to_string()],
                    effective_cwd_resolution: ResolutionStatus::Known,
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

    let mut analyzer = ShellAnalyzer::new(input);
    let initial_state = CwdState::one(absolute_initial_cwd(initial_cwd));
    analyzer.analyze_node(root, initial_state, vec![]);
    let unsupported = analyzer.unsupported;
    let commands = analyzer.finish();

    // If no commands found, treat whole input as single command
    if commands.is_empty() {
        return ParseResult {
            commands: vec![ParsedCommand {
                text: input.to_string(),
                redirections: vec![],
                effective_cwds: vec![absolute_initial_cwd(initial_cwd)
                    .to_string_lossy()
                    .to_string()],
                effective_cwd_resolution: ResolutionStatus::Known,
                position: 0,
                chain_length: 1,
                next_operator: None,
            }],
            has_errors: has_errors || unsupported,
        };
    }

    ParseResult {
        commands,
        has_errors: has_errors || unsupported,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CwdState {
    Known(BTreeSet<PathBuf>),
    Unknown,
}

impl CwdState {
    fn one(path: PathBuf) -> Self {
        Self::Known(BTreeSet::from([normalize_path(&path)]))
    }

    fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Known(left), Self::Known(right)) => {
                let mut paths = left.clone();
                paths.extend(right.iter().cloned());
                Self::Known(paths)
            }
            _ => Self::Unknown,
        }
    }

    fn descriptor(&self) -> (Vec<String>, ResolutionStatus) {
        match self {
            Self::Known(paths) if paths.len() == 1 => (
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
                ResolutionStatus::Known,
            ),
            Self::Known(paths) if !paths.is_empty() => (
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
                ResolutionStatus::Ambiguous,
            ),
            _ => (vec![], ResolutionStatus::Unknown),
        }
    }
}

#[derive(Debug, Clone)]
struct Flow {
    success: CwdState,
    failure: CwdState,
    first_command: Option<usize>,
    last_command: Option<usize>,
    mutates_cwd: bool,
}

impl Flow {
    fn unchanged(state: CwdState) -> Self {
        Self {
            success: state.clone(),
            failure: state,
            first_command: None,
            last_command: None,
            mutates_cwd: false,
        }
    }

    fn unknown(first_command: Option<usize>, last_command: Option<usize>) -> Self {
        Self {
            success: CwdState::Unknown,
            failure: CwdState::Unknown,
            first_command,
            last_command,
            mutates_cwd: true,
        }
    }
}

struct ShellAnalyzer<'a> {
    source: &'a str,
    commands: Vec<ParsedCommand>,
    unsupported: bool,
}

impl<'a> ShellAnalyzer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            commands: vec![],
            unsupported: false,
        }
    }

    fn finish(mut self) -> Vec<ParsedCommand> {
        let len = self.commands.len();
        for (position, command) in self.commands.iter_mut().enumerate() {
            command.position = position;
            command.chain_length = len;
        }
        self.commands
    }

    fn analyze_node(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        redirects: Vec<ShellRedirect>,
    ) -> Flow {
        if node_contains_kind(node, "process_substitution") {
            self.unsupported = true;
        }
        match node.kind() {
            "program" => self.analyze_sequence(node, input, redirects),
            "list" => self.analyze_list(node, input, redirects),
            "pipeline" => self.analyze_pipeline(node, input, redirects),
            "redirected_statement" => self.analyze_redirected(node, input, redirects),
            "subshell" => self.analyze_subshell(node, input, redirects),
            "compound_statement" => self.analyze_compound(node, input, redirects),
            "command" | "simple_command" => self.analyze_command(node, input, redirects),
            "comment" => Flow::unchanged(input),
            _ => self.analyze_unsupported(node, input, redirects),
        }
    }

    fn analyze_command(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let text = node_text(&node, self.source).trim().to_string();
        if text.is_empty() {
            return Flow::unchanged(input);
        }

        let index = self.commands.len();
        let (effective_cwds, effective_cwd_resolution) = input.descriptor();
        let redirections = redirects
            .into_iter()
            .map(|redirect| resolve_redirect(redirect, &input))
            .collect();
        self.commands.push(ParsedCommand {
            text: text.clone(),
            redirections,
            effective_cwds,
            effective_cwd_resolution,
            position: index,
            chain_length: 0,
            next_operator: None,
        });

        let tokens = crate::tokenizer::tokenize(&text).unwrap_or_default();
        let (success, mutates_cwd) = command_success_state(&tokens, &input);
        Flow {
            success,
            // A failed command or redirection leaves the incoming cwd intact.
            failure: input,
            first_command: Some(index),
            last_command: Some(index),
            mutates_cwd,
        }
    }

    fn analyze_redirected(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        mut inherited_redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let mut body = None;
        for i in 0..node.child_count() as u32 {
            let Some(child) = node.child(i) else {
                continue;
            };
            if child.kind().contains("redirect") {
                collect_redirections(&child, self.source, &mut inherited_redirects);
            } else if child.is_named() {
                body = Some(child);
            }
        }

        match body {
            Some(body) => self.analyze_node(body, input, inherited_redirects),
            None => self.analyze_unsupported(node, input, inherited_redirects),
        }
    }

    fn analyze_list(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        trailing_redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let (items, operators) = statement_children(node);
        if items.is_empty() {
            return Flow::unchanged(input);
        }

        let mut current = self.analyze_node(
            items[0],
            input,
            if items.len() == 1 {
                trailing_redirects.clone()
            } else {
                vec![]
            },
        );

        for (index, item) in items.iter().enumerate().skip(1) {
            let operator = operators.get(index - 1).map(String::as_str).unwrap_or(";");
            self.set_next_operator(current.last_command, operator);
            let right_input = match operator {
                "&&" => current.success.clone(),
                "||" => current.failure.clone(),
                _ => current.success.union(&current.failure),
            };
            let right = self.analyze_node(
                *item,
                right_input,
                if index + 1 == items.len() {
                    trailing_redirects.clone()
                } else {
                    vec![]
                },
            );

            current = match operator {
                "&&" => Flow {
                    success: right.success,
                    failure: current.failure.union(&right.failure),
                    first_command: current.first_command.or(right.first_command),
                    last_command: right.last_command.or(current.last_command),
                    mutates_cwd: current.mutates_cwd || right.mutates_cwd,
                },
                "||" => Flow {
                    success: current.success.union(&right.success),
                    failure: right.failure,
                    first_command: current.first_command.or(right.first_command),
                    last_command: right.last_command.or(current.last_command),
                    mutates_cwd: current.mutates_cwd || right.mutates_cwd,
                },
                _ => Flow {
                    success: right.success,
                    failure: right.failure,
                    first_command: current.first_command.or(right.first_command),
                    last_command: right.last_command.or(current.last_command),
                    mutates_cwd: current.mutates_cwd || right.mutates_cwd,
                },
            };
        }

        current
    }

    fn analyze_sequence(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        trailing_redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let (items, operators) = statement_children(node);
        if items.is_empty() {
            return Flow::unchanged(input);
        }

        let mut next_input = input;
        let mut first_command = None;
        let mut last_flow = Flow::unchanged(next_input.clone());
        let mut mutates_cwd = false;

        for (index, item) in items.iter().enumerate() {
            let following_operator = operators.get(index).map(String::as_str);
            let mut flow = self.analyze_node(
                *item,
                next_input.clone(),
                if index + 1 == items.len() {
                    trailing_redirects.clone()
                } else {
                    vec![]
                },
            );
            first_command = first_command.or(flow.first_command);

            if following_operator == Some("&") {
                self.set_next_operator(flow.last_command, "&");
                // Background commands run in a subshell and cannot change the
                // parent shell's cwd.
                flow.success = next_input.clone();
                flow.failure = next_input.clone();
                flow.mutates_cwd = false;
            } else if index + 1 < items.len() {
                self.set_next_operator(flow.last_command, ";");
            }

            mutates_cwd |= flow.mutates_cwd;
            next_input = flow.success.union(&flow.failure);
            last_flow = flow;
        }

        Flow {
            success: last_flow.success,
            failure: last_flow.failure,
            first_command,
            last_command: last_flow.last_command,
            mutates_cwd,
        }
    }

    fn analyze_subshell(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let resolved_redirects: Vec<_> = redirects
            .into_iter()
            .map(|redirect| resolve_redirect(redirect, &input))
            .collect();
        let start = self.commands.len();
        let inner = first_named_statement(node)
            .map(|child| self.analyze_node(child, input.clone(), vec![]))
            .unwrap_or_else(|| Flow::unchanged(input.clone()));
        self.attach_redirects(start, inner.first_command, resolved_redirects);

        Flow {
            success: input.clone(),
            failure: input,
            first_command: inner.first_command,
            last_command: inner.last_command,
            mutates_cwd: false,
        }
    }

    fn analyze_compound(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let resolved_redirects: Vec<_> = redirects
            .into_iter()
            .map(|redirect| resolve_redirect(redirect, &input))
            .collect();
        let start = self.commands.len();
        let mut inner = self.analyze_sequence(node, input.clone(), vec![]);
        self.attach_redirects(start, inner.first_command, resolved_redirects.clone());
        if !resolved_redirects.is_empty() {
            inner.failure = inner.failure.union(&input);
        }
        inner
    }

    fn analyze_pipeline(
        &mut self,
        node: Node<'_>,
        input: CwdState,
        trailing_redirects: Vec<ShellRedirect>,
    ) -> Flow {
        let (items, operators) = statement_children(node);
        let mut first_command = None;
        let mut last_command = None;
        let mut mutates_cwd = false;

        for (index, item) in items.iter().enumerate() {
            let flow = self.analyze_node(
                *item,
                input.clone(),
                if index + 1 == items.len() {
                    trailing_redirects.clone()
                } else {
                    vec![]
                },
            );
            first_command = first_command.or(flow.first_command);
            if index + 1 < items.len() {
                let operator = operators.get(index).map(String::as_str).unwrap_or("|");
                self.set_next_operator(flow.last_command, operator);
            }
            last_command = flow.last_command.or(last_command);
            mutates_cwd |= flow.mutates_cwd;
        }

        if mutates_cwd {
            // Whether the last pipeline component can mutate the parent shell
            // depends on shell dialect/options (e.g. Bash `lastpipe`).
            Flow::unknown(first_command, last_command)
        } else {
            Flow {
                success: input.clone(),
                failure: input,
                first_command,
                last_command,
                mutates_cwd: false,
            }
        }
    }

    fn analyze_unsupported(
        &mut self,
        node: Node<'_>,
        _input: CwdState,
        redirects: Vec<ShellRedirect>,
    ) -> Flow {
        self.unsupported = true;
        let start = self.commands.len();
        self.extract_unknown_commands(node);
        let first = (self.commands.len() > start).then_some(start);
        let last = self
            .commands
            .len()
            .checked_sub(1)
            .filter(|_| first.is_some());
        let resolved_redirects: Vec<_> = redirects
            .into_iter()
            .map(|redirect| resolve_redirect(redirect, &CwdState::Unknown))
            .collect();
        self.attach_redirects(start, first, resolved_redirects);
        Flow::unknown(first, last)
    }

    fn extract_unknown_commands(&mut self, node: Node<'_>) {
        match node.kind() {
            "command" | "simple_command" => {
                let text = node_text(&node, self.source).trim().to_string();
                if !text.is_empty() {
                    let index = self.commands.len();
                    self.commands.push(ParsedCommand {
                        text,
                        redirections: vec![],
                        effective_cwds: vec![],
                        effective_cwd_resolution: ResolutionStatus::Unknown,
                        position: index,
                        chain_length: 0,
                        next_operator: None,
                    });
                }
            }
            _ => {
                for i in 0..node.child_count() as u32 {
                    if let Some(child) = node.child(i) {
                        if child.is_named() {
                            self.extract_unknown_commands(child);
                        }
                    }
                }
            }
        }
    }

    fn attach_redirects(
        &mut self,
        fallback_index: usize,
        command_index: Option<usize>,
        redirects: Vec<ShellRedirect>,
    ) {
        if redirects.is_empty() {
            return;
        }
        let index = command_index.unwrap_or(fallback_index);
        if let Some(command) = self.commands.get_mut(index) {
            command.redirections.extend(redirects);
        }
    }

    fn set_next_operator(&mut self, command_index: Option<usize>, operator: &str) {
        if let Some(command) = command_index.and_then(|index| self.commands.get_mut(index)) {
            command.next_operator = Some(operator.to_string());
        }
    }
}

fn statement_children(node: Node<'_>) -> (Vec<Node<'_>>, Vec<String>) {
    let mut items = vec![];
    let mut operators = vec![];
    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else {
            continue;
        };
        if child.is_named() {
            items.push(child);
        } else if matches!(child.kind(), "&&" | "||" | ";" | "&" | "|" | "|&") {
            operators.push(child.kind().to_string());
        }
    }
    (items, operators)
}

fn first_named_statement(node: Node<'_>) -> Option<Node<'_>> {
    (0..node.child_count() as u32)
        .filter_map(|index| node.child(index))
        .find(|child| child.is_named())
}

fn command_success_state(tokens: &[String], input: &CwdState) -> (CwdState, bool) {
    let mut command_index = tokens
        .iter()
        .take_while(|token| is_shell_assignment(token))
        .count();

    loop {
        match tokens.get(command_index).map(String::as_str) {
            Some("builtin") => {
                command_index += 1;
                if tokens
                    .get(command_index)
                    .is_some_and(|token| token.starts_with('-'))
                {
                    return (CwdState::Unknown, true);
                }
            }
            Some("command") => {
                command_index += 1;
                while matches!(
                    tokens.get(command_index).map(String::as_str),
                    Some("-p" | "--")
                ) {
                    command_index += 1;
                }
                if matches!(
                    tokens.get(command_index).map(String::as_str),
                    Some("-v" | "-V")
                ) {
                    return (input.clone(), false);
                }
                if tokens
                    .get(command_index)
                    .is_some_and(|token| token.starts_with('-'))
                {
                    return (CwdState::Unknown, true);
                }
            }
            _ => break,
        }
    }

    let Some(command) = tokens.get(command_index).map(String::as_str) else {
        return (input.clone(), false);
    };
    if command != "cd" {
        let mutates = matches!(
            command,
            "pushd" | "popd" | "eval" | "source" | "." | "trap" | "set" | "shopt" | "enable"
        );
        return (
            if mutates {
                CwdState::Unknown
            } else {
                input.clone()
            },
            mutates,
        );
    }

    let args = &tokens[command_index + 1..];
    let target = match args {
        [target] => Some(target.as_str()),
        [double_dash, target] if double_dash == "--" => Some(target.as_str()),
        _ => None,
    };
    match target.and_then(static_absolute_cd_target) {
        Some(path) => (CwdState::one(path), true),
        None => (CwdState::Unknown, true),
    }
}

fn is_shell_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let identifier = match name.split_once('[') {
        Some((identifier, subscript)) if subscript.ends_with(']') => identifier,
        Some(_) => return false,
        None => name,
    };
    let mut characters = identifier.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn static_absolute_cd_target(target: &str) -> Option<PathBuf> {
    if contains_shell_expansion(target) {
        return None;
    }
    let path = Path::new(target);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return None;
    }
    Some(normalize_path(path))
}

fn resolve_redirect(mut redirect: ShellRedirect, cwd: &CwdState) -> ShellRedirect {
    let Some(target) = redirect.target.as_deref() else {
        return redirect;
    };
    if contains_shell_expansion(target) {
        return redirect;
    }

    let path = Path::new(target);
    let resolved: BTreeSet<PathBuf> = if path.is_absolute() {
        BTreeSet::from([normalize_path(path)])
    } else {
        match cwd {
            CwdState::Known(cwds) => cwds
                .iter()
                .map(|cwd| normalize_path(&cwd.join(path)))
                .collect(),
            CwdState::Unknown => return redirect,
        }
    };

    redirect.resolved_targets = resolved
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    redirect.target_resolution = if resolved.len() == 1 {
        ResolutionStatus::Known
    } else if resolved.is_empty() {
        ResolutionStatus::Unknown
    } else {
        ResolutionStatus::Ambiguous
    };
    redirect
}

fn contains_shell_expansion(value: &str) -> bool {
    value.contains("<(")
        || value.contains(">(")
        || value.chars().any(|character| {
            matches!(
                character,
                '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}' | '~'
            )
        })
}

fn node_contains_kind(node: Node<'_>, kind: &str) -> bool {
    node.kind() == kind
        || (0..node.child_count() as u32)
            .filter_map(|index| node.child(index))
            .any(|child| node_contains_kind(child, kind))
}

fn absolute_initial_cwd(cwd: &Path) -> PathBuf {
    if cwd.is_absolute() {
        normalize_path(cwd)
    } else {
        std::env::current_dir()
            .map(|current| normalize_path(&current.join(cwd)))
            .unwrap_or_else(|_| normalize_path(cwd))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
        resolved_targets: vec![],
        target_resolution: ResolutionStatus::Unknown,
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

    fn parse(input: &str) -> ParseResult {
        parse_command(input, Path::new("/workspace"))
    }

    #[test]
    fn test_simple_command() {
        let result = parse("git status");
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
        let result = parse("echo foo && git status");
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
        let result = parse("false || echo fallback");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.commands[0].next_operator, Some("||".to_string()));
    }

    #[test]
    fn test_semicolon_list() {
        let result = parse("echo a ; echo b ; echo c");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 3);
        assert_eq!(result.commands[0].next_operator, Some(";".to_string()));
        assert_eq!(result.commands[1].next_operator, Some(";".to_string()));
        assert!(result.commands[2].next_operator.is_none());
    }

    #[test]
    fn test_pipeline() {
        let result = parse("cat file.txt | grep pattern | head -10");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 3);
        assert_eq!(result.commands[0].next_operator, Some("|".to_string()));
        assert_eq!(result.commands[1].next_operator, Some("|".to_string()));
    }

    #[test]
    fn test_mixed_operators() {
        let result = parse("echo start && cat file | grep foo || echo failed");
        assert!(!result.has_errors);
        // Should have at least 3 commands
        assert!(result.commands.len() >= 3);
    }

    #[test]
    fn test_quoted_strings_preserved() {
        let result = parse(r#"echo "hello && world""#);
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 1);
        // The && inside quotes should NOT split the command
    }

    #[test]
    fn test_parse_error_detected() {
        // Unclosed quote should be detected as error
        let result = parse(r#"echo "unclosed"#);
        assert!(result.has_errors);
        // Should still return the input as a single command
        assert_eq!(result.commands.len(), 1);
    }

    #[test]
    fn test_subshell_treated_as_single() {
        // Subshells should be extracted but marked if we can't fully parse them
        let result = parse("(cd /tmp && ls)");
        // For now, we treat this as needing review
        assert!(!result.commands.is_empty());
    }

    // Integration tests for real-world compound command scenarios

    #[test]
    fn test_dangerous_compound_detected() {
        // This is the motivating example from the design doc
        let result = parse("echo foo && rm -rf /");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.commands[0].text, "echo foo");
        assert_eq!(result.commands[1].text, "rm -rf /");
    }

    #[test]
    fn test_redirect_excluded() {
        let result = parse("echo '.gitignore' >> .gitignore && git add .");
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
        let result = parse("python -m pytest 2>&1 | tail -30");
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
        let result = parse("cat /etc/passwd > secrets.txt");
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
        let result = parse("git log | grep TODO > /tmp/todos.txt");
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
        let result = parse("cd /tmp && git clone repo && cd repo && make");
        assert!(!result.has_errors);
        assert_eq!(result.commands.len(), 4);
    }

    fn redirect(result: &ParseResult) -> &ShellRedirect {
        result
            .commands
            .iter()
            .find_map(|command| command.redirections.first())
            .expect("expected a redirect")
    }

    #[test]
    fn resolves_redirect_after_successful_absolute_cd() {
        let result = parse("cd /tmp && echo test > test.txt");

        assert!(!result.has_errors);
        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/tmp/test.txt".to_string()]
        );
        assert_eq!(redirect(&result).target_resolution, ResolutionStatus::Known);
    }

    #[test]
    fn command_builtin_cd_updates_effective_cwd() {
        let result = parse("command -p cd /tmp && echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(redirect(&result).resolved_targets, ["/tmp/test.txt"]);
    }

    #[test]
    fn assignment_prefixed_cd_updates_effective_cwd() {
        let result = parse("CDPATH=/elsewhere cd /tmp && echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(redirect(&result).resolved_targets, ["/tmp/test.txt"]);
    }

    #[test]
    fn array_assignment_prefixed_cd_updates_effective_cwd() {
        let result = parse("A[0]=value cd /tmp && echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(redirect(&result).resolved_targets, ["/tmp/test.txt"]);
    }

    #[test]
    fn wrapped_directory_stack_mutation_makes_cwd_unknown() {
        let result = parse("builtin pushd /tmp && echo test > test.txt");

        assert_eq!(
            result.commands[1].effective_cwd_resolution,
            ResolutionStatus::Unknown
        );
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Unknown
        );
    }

    #[test]
    fn wrapped_eval_makes_cwd_unknown() {
        let result = parse("builtin eval 'cd /tmp' && echo test > test.txt");

        assert_eq!(
            result.commands[1].effective_cwd_resolution,
            ResolutionStatus::Unknown
        );
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Unknown
        );
    }

    #[test]
    fn failed_cd_or_branch_keeps_incoming_cwd() {
        let result = parse("cd /tmp || echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/workspace"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/workspace/test.txt".to_string()]
        );
    }

    #[test]
    fn sequential_command_preserves_both_cd_outcomes() {
        let result = parse("cd /tmp; echo test > test.txt");

        assert_eq!(
            result.commands[1].effective_cwd_resolution,
            ResolutionStatus::Ambiguous
        );
        assert_eq!(result.commands[1].effective_cwds, ["/tmp", "/workspace"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/tmp/test.txt", "/workspace/test.txt"]
        );
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Ambiguous
        );
    }

    #[test]
    fn subshell_cd_does_not_escape_to_following_command() {
        let result = parse("(cd /tmp) && echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/workspace"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/workspace/test.txt".to_string()]
        );
    }

    #[test]
    fn redirect_inside_subshell_uses_inner_cwd() {
        let result = parse("(cd /tmp && echo test > test.txt)");

        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/tmp/test.txt".to_string()]
        );
    }

    #[test]
    fn redirect_on_subshell_is_resolved_before_inner_cd() {
        let result = parse("(cd /tmp && echo test) > test.txt");

        assert!(result.commands[0].redirections.len() == 1);
        assert!(result.commands[1].redirections.is_empty());
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/workspace/test.txt".to_string()]
        );
    }

    #[test]
    fn brace_group_propagates_cd_to_inner_redirect() {
        let result = parse("{ cd /tmp && echo test > test.txt; }");

        assert_eq!(result.commands[1].effective_cwds, ["/tmp"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/tmp/test.txt".to_string()]
        );
    }

    #[test]
    fn background_cd_does_not_change_parent_cwd() {
        let result = parse("cd /tmp & echo test > test.txt");

        assert_eq!(result.commands[1].effective_cwds, ["/workspace"]);
        assert_eq!(
            redirect(&result).resolved_targets,
            ["/workspace/test.txt".to_string()]
        );
    }

    #[test]
    fn dynamic_cd_makes_relative_redirect_unknown() {
        let result = parse("cd \"$TARGET\" && echo test > test.txt");

        assert_eq!(
            result.commands[1].effective_cwd_resolution,
            ResolutionStatus::Unknown
        );
        assert!(redirect(&result).resolved_targets.is_empty());
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Unknown
        );
    }

    #[test]
    fn absolute_redirect_is_known_even_when_cwd_is_unknown() {
        let result = parse("cd \"$TARGET\" && echo test > /dev/null");

        assert_eq!(redirect(&result).resolved_targets, ["/dev/null"]);
        assert_eq!(redirect(&result).target_resolution, ResolutionStatus::Known);
    }

    #[test]
    fn dynamic_redirect_target_is_unknown() {
        let result = parse("echo test > \"$TARGET\"");

        assert!(redirect(&result).resolved_targets.is_empty());
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Unknown
        );
    }

    #[test]
    fn process_substitution_redirect_is_unknown_and_requires_review() {
        let result = parse("echo test > >(cat > /tmp/out)");

        assert!(result.has_errors);
        assert_eq!(
            redirect(&result).target_resolution,
            ResolutionStatus::Unknown
        );
    }
}
