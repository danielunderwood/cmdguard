use crate::nickel_config::NickelConfig;
use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedCommand {
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
}

/// Extract the real command from wrapper commands
///
/// If a NickelConfig is provided, it will try user-defined extractors first
/// before falling back to built-in extractors.
pub fn extract_command(
    tokens: &[String],
    mut nickel_config: Option<&mut NickelConfig>,
) -> ExtractedCommand {
    let mut wrapper_chain = Vec::new();
    let mut current = tokens.to_vec();

    while let Some((wrapper, inner)) = try_extract_wrapper(&current, nickel_config.as_deref_mut()) {
        wrapper_chain.push(wrapper);
        current = inner;
    }

    ExtractedCommand {
        command: current,
        wrapper_chain,
    }
}

fn try_extract_wrapper(
    tokens: &[String],
    nickel_config: Option<&mut NickelConfig>,
) -> Option<(String, Vec<String>)> {
    if tokens.is_empty() {
        return None;
    }

    let cmd = &tokens[0];

    // Handle inline environment variables: VAR=value command
    // This is a shell feature where VAR=value sets env for just that command
    if is_env_assignment(cmd) {
        return extract_inline_env(tokens);
    }

    // Try Nickel-defined extractor first
    if let Some(config) = nickel_config {
        if let Some(result) = config.extract_wrapper(cmd, tokens) {
            return Some((result.wrapper_name, result.remaining));
        }
    }

    // Fall back to built-in extractors
    match cmd.as_str() {
        "sudo" => extract_sudo(tokens),
        "env" => extract_env(tokens),
        "nix" => extract_nix(tokens),
        "nix-shell" => extract_nix_shell(tokens),
        "docker" => extract_docker(tokens),
        "find" => extract_find_exec(tokens),
        "sh" | "bash" | "zsh" => extract_shell_c(tokens),
        "poetry" => extract_poetry(tokens),
        _ => None,
    }
}

/// Check if a token is an environment variable assignment (VAR=value)
fn is_env_assignment(token: &str) -> bool {
    // Pattern: starts with letter or underscore, followed by alphanumeric/underscore, then =
    // Examples: FOO=bar, RUST_LOG=debug, _VAR=value
    if let Some(eq_pos) = token.find('=') {
        if eq_pos == 0 {
            return false; // Can't start with =
        }
        let name = &token[..eq_pos];
        let mut chars = name.chars();
        // First char must be letter or underscore
        if let Some(first) = chars.next() {
            if !first.is_ascii_alphabetic() && first != '_' {
                return false;
            }
            // Rest must be alphanumeric or underscore
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        } else {
            false
        }
    } else {
        false
    }
}

/// Extract inline environment variable assignments: VAR=value [VAR2=value2...] command
fn extract_inline_env(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // Skip all leading VAR=value tokens
    let mut idx = 0;
    while idx < tokens.len() && is_env_assignment(&tokens[idx]) {
        idx += 1;
    }

    if idx > 0 && idx < tokens.len() {
        // Collect the env vars for the wrapper name
        let env_vars: Vec<_> = tokens[..idx]
            .iter()
            .map(|s| {
                // Just show the variable name, not the value
                s.split('=').next().unwrap_or(s)
            })
            .collect();
        let wrapper_name = format!("env:{}", env_vars.join(","));
        Some((wrapper_name, tokens[idx..].to_vec()))
    } else {
        None
    }
}

fn extract_sudo(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // sudo [options] command
    // Skip sudo and any flags starting with -
    let mut idx = 1;
    while idx < tokens.len() && tokens[idx].starts_with('-') {
        idx += 1;
        // Skip the value for flags that take arguments
        // Common sudo flags that take values: -u, -g, -h, -p, -r, -t, -C
        if idx > 1 && idx < tokens.len() && !tokens[idx].starts_with('-') {
            let prev_flag = &tokens[idx - 1];
            let takes_value = matches!(
                prev_flag.as_str(),
                "-u" | "-g" | "-h" | "-p" | "-r" | "-t" | "-C"
            );
            if takes_value {
                idx += 1;
            }
        }
    }
    if idx < tokens.len() {
        Some(("sudo".to_string(), tokens[idx..].to_vec()))
    } else {
        None
    }
}

fn extract_env(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // env [VAR=val]... command
    let mut idx = 1;
    while idx < tokens.len() && tokens[idx].contains('=') {
        idx += 1;
    }
    if idx < tokens.len() {
        Some(("env".to_string(), tokens[idx..].to_vec()))
    } else {
        None
    }
}

fn extract_nix(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // nix develop --command <cmd>
    // nix shell --command <cmd>
    if tokens.len() < 2 {
        return None;
    }

    let subcommand = &tokens[1];
    if subcommand != "develop" && subcommand != "shell" {
        return None;
    }

    // Find --command flag
    for (i, token) in tokens.iter().enumerate() {
        if (token == "--command" || token == "-c") && i + 1 < tokens.len() {
            let wrapper = format!("nix {}", subcommand);
            return Some((wrapper, tokens[i + 1..].to_vec()));
        }
    }
    None
}

fn extract_nix_shell(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // nix-shell --run "command"
    for (i, token) in tokens.iter().enumerate() {
        if token == "--run" && i + 1 < tokens.len() {
            // The next token is a quoted command string, need to re-tokenize
            if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                return Some(("nix-shell".to_string(), inner_tokens));
            }
        }
    }
    None
}

fn extract_docker(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // docker run [options] image [command]
    // docker exec [options] container command
    if tokens.len() < 2 {
        return None;
    }

    let subcommand = &tokens[1];
    if subcommand != "run" && subcommand != "exec" {
        return None;
    }

    // Find where options end and command begins
    // This is tricky - we look for patterns that indicate end of docker args
    let mut idx = 2;
    while idx < tokens.len() {
        let token = &tokens[idx];

        // Skip known docker flags that take values
        if token.starts_with('-') {
            idx += 1;
            // If it's a flag that takes a value (not --flag=value form), skip the value too
            if !token.contains('=') && idx < tokens.len() && !tokens[idx].starts_with('-') {
                // Heuristic: common docker flags that take values
                let takes_value = matches!(
                    token.as_str(),
                    "-e" | "--env"
                        | "-v"
                        | "--volume"
                        | "-p"
                        | "--publish"
                        | "-w"
                        | "--workdir"
                        | "--name"
                        | "-u"
                        | "--user"
                        | "--network"
                        | "--entrypoint"
                        | "-m"
                        | "--memory"
                );
                if takes_value {
                    idx += 1;
                }
            }
            continue;
        }

        // First non-flag is the image (for run) or container (for exec)
        // The rest is the command
        if idx + 1 < tokens.len() {
            let wrapper = format!("docker {}", subcommand);
            return Some((wrapper, tokens[idx + 1..].to_vec()));
        }
        break;
    }
    None
}

fn extract_shell_c(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // bash -c "command"
    // sh -c "command"
    let shell = &tokens[0];

    for (i, token) in tokens.iter().enumerate() {
        if token == "-c" && i + 1 < tokens.len() {
            // The next token is a quoted command string, need to re-tokenize
            if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                return Some((shell.clone(), inner_tokens));
            }
        }
    }
    None
}

fn extract_poetry(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // poetry run <command>
    if tokens.len() >= 3 && tokens[1] == "run" {
        return Some(("poetry run".to_string(), tokens[2..].to_vec()));
    }
    None
}

fn extract_find_exec(tokens: &[String]) -> Option<(String, Vec<String>)> {
    let exec_idx = tokens.iter().position(|token| token == "-exec")?;

    // Keep find-specific side effects and interactive variants on the
    // existing find policy path until the model can represent their context.
    if tokens[..exec_idx]
        .iter()
        .any(|token| matches!(token.as_str(), "-delete" | "-execdir" | "-ok" | "-okdir"))
    {
        return None;
    }

    let command_start = exec_idx + 1;
    if command_start >= tokens.len() {
        return None;
    }

    let mut end_idx = command_start;
    while end_idx < tokens.len() {
        if tokens[end_idx] == ";" {
            break;
        }

        if tokens[end_idx] == "+" && end_idx > command_start && tokens[end_idx - 1] == "{}" {
            // Batch mode appends matched paths to a single command. That is
            // different enough from normal wrapper semantics to leave as ask.
            return None;
        }

        end_idx += 1;
    }

    if end_idx == tokens.len() || end_idx == command_start {
        return None;
    }

    if end_idx != tokens.len() - 1 {
        // Do not unwrap if find continues evaluating more expression terms.
        return None;
    }

    Some((
        "find -exec".to_string(),
        tokens[command_start..end_idx].to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_no_wrapper() {
        let tokens = to_vec(&["git", "status"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_sudo() {
        let tokens = to_vec(&["sudo", "rm", "-rf", "/"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["rm", "-rf", "/"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_sudo_with_flags() {
        let tokens = to_vec(&["sudo", "-u", "root", "ls"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["ls"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_env() {
        let tokens = to_vec(&["env", "FOO=bar", "BAZ=qux", "echo", "hello"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["echo", "hello"]));
        assert_eq!(result.wrapper_chain, vec!["env"]);
    }

    #[test]
    fn test_nix_develop() {
        let tokens = to_vec(&["nix", "develop", "--command", "git", "status"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["nix develop"]);
    }

    #[test]
    fn test_bash_c() {
        let tokens = to_vec(&["bash", "-c", "git status"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["bash"]);
    }

    #[test]
    fn test_nested_wrappers() {
        let tokens = to_vec(&["sudo", "bash", "-c", "git status"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["sudo", "bash"]);
    }

    #[test]
    fn test_nix_shell_run() {
        let tokens = to_vec(&["nix-shell", "--run", "cargo build"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["cargo", "build"]));
        assert_eq!(result.wrapper_chain, vec!["nix-shell"]);
    }

    #[test]
    fn test_poetry_run() {
        let tokens = to_vec(&["poetry", "run", "pytest", "-v"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["pytest", "-v"]));
        assert_eq!(result.wrapper_chain, vec!["poetry run"]);
    }

    #[test]
    fn test_find_exec() {
        let tokens = to_vec(&[
            "find", ".", "-name", "*.rs", "-exec", "grep", "TODO", "{}", ";",
        ]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["grep", "TODO", "{}"]));
        assert_eq!(result.wrapper_chain, vec!["find -exec"]);
    }

    #[test]
    fn test_find_exec_allows_inner_find_like_args() {
        let tokens = to_vec(&["find", ".", "-exec", "tool", "-exec", "literal", ";"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["tool", "-exec", "literal"]));
        assert_eq!(result.wrapper_chain, vec!["find -exec"]);
    }

    #[test]
    fn test_find_exec_with_delete_not_extracted() {
        let tokens = to_vec(&["find", ".", "-delete", "-exec", "echo", "{}", ";"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, tokens);
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_find_exec_batch_not_extracted() {
        let tokens = to_vec(&["find", ".", "-exec", "grep", "TODO", "{}", "+"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, tokens);
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_find_exec_with_trailing_expression_not_extracted() {
        let tokens = to_vec(&["find", ".", "-exec", "grep", "TODO", "{}", ";", "-print"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, tokens);
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_find_execdir_not_extracted() {
        let tokens = to_vec(&["find", ".", "-execdir", "grep", "TODO", "{}", ";"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, tokens);
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_find_ok_not_extracted() {
        let tokens = to_vec(&["find", ".", "-ok", "grep", "TODO", "{}", ";"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, tokens);
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_inline_env_single() {
        let tokens = to_vec(&["RUST_LOG=debug", "cargo", "run"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["cargo", "run"]));
        assert_eq!(result.wrapper_chain, vec!["env:RUST_LOG"]);
    }

    #[test]
    fn test_inline_env_multiple() {
        let tokens = to_vec(&["FOO=bar", "BAZ=qux", "echo", "hello"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["echo", "hello"]));
        assert_eq!(result.wrapper_chain, vec!["env:FOO,BAZ"]);
    }

    #[test]
    fn test_inline_env_with_wrapper() {
        let tokens = to_vec(&["RUST_LOG=debug", "sudo", "cargo", "build"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["cargo", "build"]));
        assert_eq!(result.wrapper_chain, vec!["env:RUST_LOG", "sudo"]);
    }

    #[test]
    fn test_inline_env_underscore_prefix() {
        let tokens = to_vec(&["_MY_VAR=value", "command"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["command"]));
        assert_eq!(result.wrapper_chain, vec!["env:_MY_VAR"]);
    }

    #[test]
    fn test_not_env_assignment_no_equals() {
        let tokens = to_vec(&["echo", "hello"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["echo", "hello"]));
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_not_env_assignment_starts_with_equals() {
        let tokens = to_vec(&["=value", "command"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["=value", "command"]));
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_not_env_assignment_starts_with_number() {
        let tokens = to_vec(&["1VAR=value", "command"]);
        let result = extract_command(&tokens, None);
        assert_eq!(result.command, to_vec(&["1VAR=value", "command"]));
        assert!(result.wrapper_chain.is_empty());
    }
}
