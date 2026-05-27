use crate::cli::HookAction;
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn run(action: HookAction) {
    match action {
        HookAction::Install => install(),
        HookAction::Uninstall => uninstall(),
        HookAction::Status => status(),
        // The Run arm is handled directly in main.rs (it calls into the
        // stdin-reading hook handler that lives there). Reaching it here
        // means the dispatch in main is wrong.
        HookAction::Run => unreachable!("HookAction::Run dispatched here"),
    }
}

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".claude/settings.json")
}

fn binary_path() -> String {
    std::env::current_exe()
        .expect("Could not determine binary path")
        .to_string_lossy()
        .to_string()
}

fn read_settings(path: &PathBuf) -> Value {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                json!({})
            } else {
                serde_json::from_str(trimmed).unwrap_or_else(|e| {
                    eprintln!("Warning: could not parse {}: {}", path.display(), e);
                    eprintln!("Creating backup and starting fresh.");
                    let backup = path.with_extension("json.bak");
                    let _ = std::fs::copy(path, &backup);
                    json!({})
                })
            }
        }
        Err(_) => json!({}),
    }
}

fn write_settings(path: &PathBuf, settings: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("Failed to create directory {}: {}", parent.display(), e);
            std::process::exit(1);
        });
    }
    let content = serde_json::to_string_pretty(settings).expect("Failed to serialize settings");
    std::fs::write(path, format!("{}\n", content)).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", path.display(), e);
        std::process::exit(1);
    });
}

fn make_hook_entry(bin_path: &str) -> Value {
    json!({
        "matcher": "Bash",
        "hooks": [
            {
                "type": "command",
                "command": format!("{} hook run", bin_path)
            }
        ]
    })
}

/// Returns `Some(bin_basename)` if `command` invokes our binary — that is,
/// the executable's basename (after stripping any leading `KEY=VAL`
/// environment-variable prefixes from the shell-tokenized command) equals
/// "cmdguard".
///
/// This is the shape settings.json hook commands take. We use shlex so
/// quoted paths with spaces and other shell-style escapes parse correctly,
/// and we match the basename exactly so foreign tools like
/// `mycmdguard` / `acme-cmdguard` don't get mistaken for ours.
fn cmdguard_basename_in(command: &str) -> Option<&'static str> {
    let tokens = shlex::split(command)?;
    // Skip leading env assignments like `RUST_LOG=debug`, which the shell
    // treats as variable bindings for the command, not the command itself.
    let bin_token = tokens.iter().find(|t| !is_env_assignment(t))?;

    let basename = std::path::Path::new(bin_token)
        .file_name()
        .and_then(|n| n.to_str())?;

    if basename == "cmdguard" {
        Some("cmdguard")
    } else {
        None
    }
}

fn is_env_assignment(token: &str) -> bool {
    // POSIX-style env prefix: identifier followed by '='. We only need to
    // recognize, not fully validate; treat any token containing '=' before
    // any '/' as an env assignment.
    match (token.find('='), token.find('/')) {
        (Some(eq), Some(slash)) => eq < slash,
        (Some(_), None) => true,
        _ => false,
    }
}

fn is_our_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .and_then(cmdguard_basename_in)
                    .is_some()
            })
        })
        .unwrap_or(false)
}

fn install() {
    let path = settings_path();
    let bin = binary_path();
    let mut settings = read_settings(&path);

    // Ensure hooks.PreToolUse exists as an array
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }
    if settings["hooks"].get("PreToolUse").is_none() {
        settings["hooks"]["PreToolUse"] = json!([]);
    }

    let pre_tool_use = settings["hooks"]["PreToolUse"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Check if already registered
    if pre_tool_use.iter().any(|e| is_our_entry(e)) {
        println!("Hook already registered in {}", path.display());
        return;
    }

    // Append our entry
    let mut entries = pre_tool_use;
    entries.push(make_hook_entry(&bin));
    settings["hooks"]["PreToolUse"] = Value::Array(entries);

    write_settings(&path, &settings);
    println!("Hook registered in {}", path.display());
}

fn uninstall() {
    let path = settings_path();

    if !path.exists() {
        println!("No settings file found at {}", path.display());
        return;
    }

    let mut settings = read_settings(&path);

    let pre_tool_use = match settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
    {
        Some(arr) => arr.clone(),
        None => {
            println!("Hook not registered (no PreToolUse hooks found)");
            return;
        }
    };

    let filtered: Vec<Value> = pre_tool_use
        .into_iter()
        .filter(|e| !is_our_entry(e))
        .collect();

    let removed = settings["hooks"]["PreToolUse"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0)
        != filtered.len();

    if !removed {
        println!("Hook not registered (nothing to remove)");
        return;
    }

    settings["hooks"]["PreToolUse"] = Value::Array(filtered);
    write_settings(&path, &settings);
    println!("Hook removed from {}", path.display());
}

fn status() {
    let path = settings_path();

    if !path.exists() {
        println!("Not registered (no settings file)");
        std::process::exit(1);
    }

    let settings = read_settings(&path);

    let registered = settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().any(|e| is_our_entry(e)))
        .unwrap_or(false);

    if registered {
        println!("Registered in {}", path.display());
    } else {
        println!("Not registered");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_env(tmp: &TempDir) -> PathBuf {
        let settings_path = tmp.path().join(".claude/settings.json");
        settings_path
    }

    fn install_to(path: &PathBuf, bin: &str) {
        let mut settings = read_settings(path);

        if settings.get("hooks").is_none() {
            settings["hooks"] = json!({});
        }
        if settings["hooks"].get("PreToolUse").is_none() {
            settings["hooks"]["PreToolUse"] = json!([]);
        }

        let pre_tool_use = settings["hooks"]["PreToolUse"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if pre_tool_use.iter().any(|e| is_our_entry(e)) {
            return;
        }

        let mut entries = pre_tool_use;
        entries.push(make_hook_entry(bin));
        settings["hooks"]["PreToolUse"] = Value::Array(entries);

        write_settings(path, &settings);
    }

    fn uninstall_from(path: &PathBuf) -> bool {
        if !path.exists() {
            return false;
        }

        let mut settings = read_settings(path);

        let pre_tool_use = match settings
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|p| p.as_array())
        {
            Some(arr) => arr.clone(),
            None => return false,
        };

        let original_len = pre_tool_use.len();
        let filtered: Vec<Value> = pre_tool_use
            .into_iter()
            .filter(|e| !is_our_entry(e))
            .collect();

        if filtered.len() == original_len {
            return false;
        }

        settings["hooks"]["PreToolUse"] = Value::Array(filtered);
        write_settings(path, &settings);
        true
    }

    fn is_registered(path: &PathBuf) -> bool {
        if !path.exists() {
            return false;
        }
        let settings = read_settings(path);
        settings
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().any(|e| is_our_entry(e)))
            .unwrap_or(false)
    }

    #[test]
    fn test_install_creates_new_settings() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        assert!(!path.exists());
        install_to(&path, bin);
        assert!(path.exists());
        assert!(is_registered(&path));

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hooks = &settings["hooks"]["PreToolUse"];
        assert_eq!(hooks.as_array().unwrap().len(), 1);
        assert_eq!(hooks[0]["matcher"], "Bash");
        assert_eq!(hooks[0]["hooks"][0]["command"], format!("{} hook run", bin));
    }

    fn entry(cmd: &str) -> Value {
        json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": cmd}]
        })
    }

    #[test]
    fn test_is_our_entry_current_form() {
        assert!(is_our_entry(&entry("/usr/local/bin/cmdguard hook run")));
    }

    #[test]
    fn test_is_our_entry_legacy_bare_form() {
        assert!(is_our_entry(&entry("/usr/local/bin/cmdguard")));
    }

    #[test]
    fn test_is_our_entry_with_env_prefix() {
        // POSIX-style env-var prefix is a common debugging shape:
        assert!(is_our_entry(&entry("RUST_LOG=debug ~/.cargo/bin/cmdguard")));
        assert!(is_our_entry(&entry(
            "RUST_LOG=debug /usr/local/bin/cmdguard hook run"
        )));
        // Multiple env vars
        assert!(is_our_entry(&entry(
            "FOO=1 BAR=2 /usr/local/bin/cmdguard hook run"
        )));
    }

    #[test]
    fn test_is_our_entry_quoted_path_with_spaces() {
        // Path containing a space, properly quoted: must still match.
        assert!(is_our_entry(&entry(
            "\"/Users/my user/bin/cmdguard\" hook run"
        )));
        assert!(is_our_entry(&entry(
            "'/Users/my user/bin/cmdguard' hook run"
        )));
    }

    #[test]
    fn test_is_our_entry_with_trailing_redirection() {
        // shlex preserves the redirection token so it stays in the token
        // list, but the binary token is still the first non-env token —
        // so detection should still work.
        assert!(is_our_entry(&entry(
            "/usr/local/bin/cmdguard hook run 2>>/tmp/log"
        )));
    }

    #[test]
    fn test_is_our_entry_rejects_foreign_binaries() {
        // Different name entirely
        assert!(!is_our_entry(&entry("/usr/bin/some-other-tool")));
        // Substring match must NOT be enough — basename has to be exactly
        // `cmdguard`. Otherwise `cmdguard hook uninstall` would happily
        // remove some unrelated tool's hook.
        assert!(!is_our_entry(&entry("/usr/local/bin/mycmdguard")));
        assert!(!is_our_entry(&entry("/opt/acme-cmdguard")));
        assert!(!is_our_entry(&entry("/usr/local/bin/cmdguardx")));
    }

    #[test]
    fn test_is_our_entry_unparseable_command_safe() {
        // Unbalanced quote — shlex returns None, we should not panic and
        // not falsely claim it's ours.
        assert!(!is_our_entry(&entry("/usr/local/bin/cmdguard \"unclosed")));
    }

    #[test]
    fn test_install_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        install_to(&path, bin);
        install_to(&path, bin);

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hooks = &settings["hooks"]["PreToolUse"];
        assert_eq!(hooks.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_install_preserves_existing_settings() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        // Create settings with existing content
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let existing = json!({
            "someKey": "someValue",
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Write",
                        "hooks": [{"type": "command", "command": "/usr/bin/other-hook"}]
                    }
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        install_to(&path, bin);

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(settings["someKey"], "someValue");
        let hooks = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0]["matcher"], "Write");
        assert_eq!(hooks[1]["matcher"], "Bash");
    }

    #[test]
    fn test_uninstall_removes_entry() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        install_to(&path, bin);
        assert!(is_registered(&path));

        let removed = uninstall_from(&path);
        assert!(removed);
        assert!(!is_registered(&path));
    }

    #[test]
    fn test_uninstall_preserves_other_hooks() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Write",
                        "hooks": [{"type": "command", "command": "/usr/bin/other-hook"}]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "/usr/local/bin/cmdguard"}]
                    }
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let removed = uninstall_from(&path);
        assert!(removed);

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hooks = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["matcher"], "Write");
    }

    #[test]
    fn test_uninstall_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        assert!(!uninstall_from(&path));
    }

    #[test]
    fn test_status_not_registered() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        assert!(!is_registered(&path));
    }

    #[test]
    fn test_status_registered() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        install_to(&path, bin);
        assert!(is_registered(&path));
    }
}
