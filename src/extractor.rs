use crate::tokenizer::tokenize;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedCommand {
    pub command: Vec<String>,
    pub wrapper_chain: Vec<String>,
}

/// Extract the real command from wrapper commands
pub fn extract_command(tokens: &[String]) -> ExtractedCommand {
    let mut wrapper_chain = Vec::new();
    let mut current = tokens.to_vec();

    loop {
        match try_extract_wrapper(&current) {
            Some((wrapper, inner)) => {
                wrapper_chain.push(wrapper);
                current = inner;
            }
            None => break,
        }
    }

    ExtractedCommand {
        command: current,
        wrapper_chain,
    }
}

fn try_extract_wrapper(tokens: &[String]) -> Option<(String, Vec<String>)> {
    if tokens.is_empty() {
        return None;
    }

    let cmd = &tokens[0];

    match cmd.as_str() {
        "sudo" => extract_sudo(tokens),
        "env" => extract_env(tokens),
        "nix" => extract_nix(tokens),
        "nix-shell" => extract_nix_shell(tokens),
        "docker" => extract_docker(tokens),
        "sh" | "bash" | "zsh" => extract_shell_c(tokens),
        _ => None,
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
        if token == "--command" || token == "-c" {
            if i + 1 < tokens.len() {
                let wrapper = format!("nix {}", subcommand);
                return Some((wrapper, tokens[i + 1..].to_vec()));
            }
        }
    }
    None
}

fn extract_nix_shell(tokens: &[String]) -> Option<(String, Vec<String>)> {
    // nix-shell --run "command"
    for (i, token) in tokens.iter().enumerate() {
        if token == "--run" {
            if i + 1 < tokens.len() {
                // The next token is a quoted command string, need to re-tokenize
                if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                    return Some(("nix-shell".to_string(), inner_tokens));
                }
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
                    "-e" | "--env" | "-v" | "--volume" | "-p" | "--publish" |
                    "-w" | "--workdir" | "--name" | "-u" | "--user" |
                    "--network" | "--entrypoint" | "-m" | "--memory"
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
        if token == "-c" {
            if i + 1 < tokens.len() {
                // The next token is a quoted command string, need to re-tokenize
                if let Ok(inner_tokens) = tokenize(&tokens[i + 1]) {
                    return Some((shell.clone(), inner_tokens));
                }
            }
        }
    }
    None
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
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert!(result.wrapper_chain.is_empty());
    }

    #[test]
    fn test_sudo() {
        let tokens = to_vec(&["sudo", "rm", "-rf", "/"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["rm", "-rf", "/"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_sudo_with_flags() {
        let tokens = to_vec(&["sudo", "-u", "root", "ls"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["ls"]));
        assert_eq!(result.wrapper_chain, vec!["sudo"]);
    }

    #[test]
    fn test_env() {
        let tokens = to_vec(&["env", "FOO=bar", "BAZ=qux", "echo", "hello"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["echo", "hello"]));
        assert_eq!(result.wrapper_chain, vec!["env"]);
    }

    #[test]
    fn test_nix_develop() {
        let tokens = to_vec(&["nix", "develop", "--command", "git", "status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["nix develop"]);
    }

    #[test]
    fn test_bash_c() {
        let tokens = to_vec(&["bash", "-c", "git status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["bash"]);
    }

    #[test]
    fn test_nested_wrappers() {
        let tokens = to_vec(&["sudo", "bash", "-c", "git status"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["git", "status"]));
        assert_eq!(result.wrapper_chain, vec!["sudo", "bash"]);
    }

    #[test]
    fn test_nix_shell_run() {
        let tokens = to_vec(&["nix-shell", "--run", "cargo build"]);
        let result = extract_command(&tokens);
        assert_eq!(result.command, to_vec(&["cargo", "build"]));
        assert_eq!(result.wrapper_chain, vec!["nix-shell"]);
    }
}
