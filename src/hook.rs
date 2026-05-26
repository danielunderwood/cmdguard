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

/// Detects either form of our hook entry:
/// - the legacy bare invocation: `.../cmdguard` (or with prefix env vars)
/// - the current form: `.../cmdguard hook run`
/// Both should be considered "ours" so install/uninstall act on either,
/// e.g. uninstall correctly removes a stale legacy registration.
fn is_our_entry(entry: &Value) -> bool {
    if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
        hooks.iter().any(|hook| {
            let cmd = match hook.get("command").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return false,
            };
            // Strip optional trailing `hook run` and any surrounding whitespace
            let trimmed = cmd.trim_end();
            let binary_part = trimmed
                .strip_suffix("hook run")
                .map(|s| s.trim_end())
                .unwrap_or(trimmed);
            // Last whitespace-separated token is the binary path
            let last_token = binary_part.split_whitespace().last().unwrap_or("");
            last_token.ends_with("cmdguard")
        })
    } else {
        false
    }
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

    #[test]
    fn test_is_our_entry_matches_legacy_and_current_forms() {
        // Current: `cmdguard hook run`
        let current = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "/usr/local/bin/cmdguard hook run"}]
        });
        assert!(is_our_entry(&current));

        // Legacy: bare cmdguard binary
        let legacy = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "/usr/local/bin/cmdguard"}]
        });
        assert!(is_our_entry(&legacy));

        // Legacy with env-var prefix
        let with_env = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "RUST_LOG=debug ~/.cargo/bin/cmdguard"}]
        });
        assert!(is_our_entry(&with_env));

        // Foreign hook: should not match
        let foreign = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "/usr/bin/some-other-tool"}]
        });
        assert!(!is_our_entry(&foreign));
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
