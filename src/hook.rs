use crate::cli::{Cli, Commands, HookAction, HookTarget};
use clap::Parser;
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub fn run(action: HookAction) {
    match action {
        HookAction::Install { target, policy_dir } => install(target, policy_dir),
        HookAction::Uninstall { target } => uninstall(target),
        HookAction::Status { target } => status(target),
        // The Run arm is handled directly in main.rs (it calls into the
        // stdin-reading hook handler that lives there). Reaching it here
        // means the dispatch in main is wrong.
        HookAction::Run { .. } => unreachable!("HookAction::Run dispatched here"),
    }
}

fn settings_path(target: HookTarget) -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    match target {
        HookTarget::Claude => home.join(".claude/settings.json"),
        HookTarget::Codex => home.join(".codex/hooks.json"),
    }
}

fn binary_path() -> String {
    std::env::current_exe()
        .expect("Could not determine binary path")
        .to_string_lossy()
        .to_string()
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            let settings = if trimmed.is_empty() {
                json!({})
            } else {
                serde_json::from_str(trimmed)
                    .map_err(|e| format!("Could not parse {}: {}", path.display(), e))?
            };
            if settings.is_object() {
                Ok(settings)
            } else {
                Err(format!("{} must contain a JSON object", path.display()))
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("Could not read {}: {}", path.display(), e)),
    }
}

fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    let content = serde_json::to_string_pretty(settings).expect("Failed to serialize settings");
    std::fs::write(path, format!("{}\n", content))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Shell-quote a path so install paths with spaces or shell metacharacters are
/// emitted as a single shell token. Without this, a path like
/// `/Users/foo bar/cmdguard` would be split by Claude Code's shell into
/// multiple args and the hook would silently fail. try_quote can fail on bytes
/// that are unrepresentable in shell (NULs); fall back to the unquoted form
/// rather than failing install — the shell would have rejected such a path
/// anyway.
fn shell_quote(value: &str) -> String {
    shlex::try_quote(value)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

/// The `cmdguard hook run` command line written into an agent's settings.
fn hook_command(bin_path: &str, target: HookTarget, policy_dir: Option<&Path>) -> String {
    let mut command = shell_quote(bin_path);
    command.push_str(" hook run");
    if target == HookTarget::Codex {
        command.push_str(" --target codex");
    }
    // Pinning the directory matters most for agents launched from a GUI, which
    // hand the hook no XDG_CONFIG_HOME and so would resolve a different config
    // directory than the one the install was aimed at.
    if let Some(dir) = policy_dir {
        command.push_str(" --policy-dir ");
        command.push_str(&shell_quote(&dir.to_string_lossy()));
    }
    command
}

fn make_hook_entry(
    bin_path: &str,
    target: HookTarget,
    event: &str,
    policy_dir: Option<&Path>,
) -> Value {
    let command = hook_command(bin_path, target, policy_dir);
    match target {
        HookTarget::Claude => json!({
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "command": command
            }]
        }),
        HookTarget::Codex => {
            let status_message = match event {
                "PermissionRequest" => "Checking approval request with cmdguard",
                _ => "Checking Bash command with cmdguard",
            };
            json!({
                "matcher": "^Bash$",
                "hooks": [{
                    "type": "command",
                    "command": command,
                    "statusMessage": status_message
                }]
            })
        }
    }
}

/// Returns `true` if `command` invokes our binary — that is, after shell
/// tokenization and skipping any leading `KEY=VAL` env-var prefixes, the
/// first remaining token's basename equals `"cmdguard"`.
///
/// We match the basename exactly so foreign tools whose name happens to
/// contain "cmdguard" (e.g. `mycmdguard`, `acme-cmdguard`) don't get
/// misidentified as ours.
fn is_our_command(command: &str) -> bool {
    command_args(command).is_some()
}

fn command_args(command: &str) -> Option<Vec<String>> {
    let tokens = shlex::split(command)?;
    // Skip leading env assignments like `RUST_LOG=debug`, which the shell
    // treats as variable bindings for the command, not the command itself.
    let bin_index = tokens.iter().position(|token| !is_env_assignment(token))?;
    let bin_token = &tokens[bin_index];
    let basename = std::path::Path::new(bin_token)
        .file_name()
        .and_then(|n| n.to_str());
    (basename == Some("cmdguard")).then(|| tokens[bin_index..].to_vec())
}

fn command_target(command: &str) -> Option<HookTarget> {
    let cli = Cli::try_parse_from(command_args(command)?).ok()?;
    match cli.command {
        Some(Commands::Hook {
            action: HookAction::Run { target, .. },
        }) => Some(target),
        _ => None,
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

#[cfg(test)]
fn is_our_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .map(is_our_command)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn is_target_entry(entry: &Value, target: HookTarget) -> bool {
    our_command_for(entry, target).is_some()
}

/// Whether `command` is exactly what `hook_command` would emit for the binary
/// and `--policy-dir` it names — i.e. it carries no hand-added decoration such
/// as an env prefix or a redirection. Only such an entry can be regenerated
/// without silently dropping a user's edits.
fn is_plain_our_command(command: &str, target: HookTarget) -> bool {
    let Some(args) = command_args(command) else {
        return false;
    };
    let Ok(cli) = Cli::try_parse_from(&args) else {
        return false;
    };
    let Some(Commands::Hook {
        action:
            HookAction::Run {
                target: parsed_target,
                policy_dir,
            },
    }) = cli.command
    else {
        return false;
    };
    let Some(bin) = args.first() else {
        return false;
    };
    parsed_target == target && hook_command(bin, parsed_target, policy_dir.as_deref()) == command
}

/// The command of the entry's hook that is ours and speaks `target`'s protocol.
fn our_command_for(entry: &Value, target: HookTarget) -> Option<String> {
    entry
        .get("hooks")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|hook| {
            let command = hook.get("command").and_then(Value::as_str)?;
            (command_entry_target(command) == Some(target)).then(|| command.to_string())
        })
}

/// Determine the hook target a stored command entry corresponds to.
///
/// Tries `command_target` (a full clap parse) first. That fails for entries
/// customized with shell decoration clap can't handle -- e.g. an env prefix
/// plus trailing redirection appended after our own args by hand-editing
/// the settings file, such as
/// `RUST_LOG=debug /path/cmdguard hook run 2>>/tmp/log`. Rather than
/// treating a parse failure as "not ours" and letting `hook install`
/// duplicate/clobber the entry, fall back to the same binary detection
/// `is_our_command` uses, confirm the entry actually invokes `hook run`,
/// and infer the target from whether `--target codex` appears in the
/// command string (absent means claude).
fn command_entry_target(command: &str) -> Option<HookTarget> {
    if let Some(target) = command_target(command) {
        return Some(target);
    }
    if !is_our_command(command) {
        return None;
    }
    let args = command_args(command)?;
    let mentions_hook_run = args
        .windows(2)
        .any(|pair| pair[0] == "hook" && pair[1] == "run");
    if !mentions_hook_run {
        return None;
    }
    Some(if command.contains("--target codex") {
        HookTarget::Codex
    } else {
        HookTarget::Claude
    })
}

fn strip_our_hooks(mut entry: Value) -> (Option<Value>, usize) {
    let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
        return (Some(entry), 0);
    };
    let original_len = hooks.len();
    hooks.retain(|hook| {
        !hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(is_our_command)
    });
    let removed = original_len - hooks.len();
    if removed > 0 && hooks.is_empty() {
        (None, removed)
    } else {
        (Some(entry), removed)
    }
}

fn remove_our_hooks(entries: Vec<Value>) -> (Vec<Value>, usize) {
    let mut kept = Vec::with_capacity(entries.len());
    let mut removed = 0;
    for entry in entries {
        let (entry, entry_removed) = strip_our_hooks(entry);
        removed += entry_removed;
        if let Some(entry) = entry {
            kept.push(entry);
        }
    }
    (kept, removed)
}

fn target_events(target: HookTarget) -> &'static [&'static str] {
    match target {
        HookTarget::Claude => &["PreToolUse"],
        HookTarget::Codex => &["PreToolUse", "PermissionRequest"],
    }
}

fn install_to(
    path: &Path,
    bin: &str,
    target: HookTarget,
    policy_dir: Option<&Path>,
) -> Result<usize, String> {
    let mut settings = read_settings(path)?;
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    } else if !settings["hooks"].is_object() {
        return Err(format!(
            "{} contains a non-object `hooks` value",
            path.display()
        ));
    }

    let mut changed = 0;
    for event in target_events(target) {
        let entries = match settings["hooks"].get(*event) {
            Some(value) => value.as_array().cloned().ok_or_else(|| {
                format!(
                    "{} contains a non-array `hooks.{}` value",
                    path.display(),
                    event
                )
            })?,
            None => Vec::new(),
        };
        // Reinstalling refreshes our own entries so a moved binary or a changed
        // --policy-dir takes effect, but only when every one of them is a
        // command we generated verbatim. An entry already running the desired
        // command needs no write, and a hand-decorated one (env prefix,
        // redirection) is left alone because regenerating it would drop the
        // decoration.
        let desired = hook_command(bin, target, policy_dir);
        let ours: Vec<String> = entries
            .iter()
            .filter_map(|entry| our_command_for(entry, target))
            .collect();
        let stale = !ours.is_empty()
            && !ours.contains(&desired)
            && ours
                .iter()
                .all(|command| is_plain_our_command(command, target));
        if !ours.is_empty() && !stale {
            continue;
        }

        // Replace stale cmdguard entries that use another target's wire
        // protocol; leaving both active could emit invalid decisions.
        let (mut entries, _) = remove_our_hooks(entries);
        entries.push(make_hook_entry(bin, target, event, policy_dir));
        settings["hooks"][*event] = Value::Array(entries);
        changed += 1;
    }

    if changed > 0 {
        write_settings(path, &settings)?;
    }
    Ok(changed)
}

fn uninstall_from(path: &Path, target: HookTarget) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }

    let mut settings = read_settings(path)?;
    if settings
        .get("hooks")
        .is_some_and(|hooks| !hooks.is_object())
    {
        return Err(format!(
            "{} contains a non-object `hooks` value",
            path.display()
        ));
    }
    let mut removed = 0;
    for event in target_events(target) {
        let entries = match settings.get("hooks").and_then(|h| h.get(*event)) {
            Some(value) => value.as_array().cloned().ok_or_else(|| {
                format!(
                    "{} contains a non-array `hooks.{}` value",
                    path.display(),
                    event
                )
            })?,
            None => continue,
        };

        let (filtered, event_removed) = remove_our_hooks(entries);
        removed += event_removed;
        settings["hooks"][*event] = Value::Array(filtered);
    }

    if removed > 0 {
        write_settings(path, &settings)?;
    }
    Ok(removed)
}

fn registered_events(path: &Path, target: HookTarget) -> Result<Vec<&'static str>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }

    let settings = read_settings(path)?;
    if settings
        .get("hooks")
        .is_some_and(|hooks| !hooks.is_object())
    {
        return Err(format!(
            "{} contains a non-object `hooks` value",
            path.display()
        ));
    }

    let mut registered = Vec::new();
    for event in target_events(target) {
        let Some(value) = settings.get("hooks").and_then(|hooks| hooks.get(*event)) else {
            continue;
        };
        let entries = value.as_array().ok_or_else(|| {
            format!(
                "{} contains a non-array `hooks.{}` value",
                path.display(),
                event
            )
        })?;
        if entries.iter().any(|entry| is_target_entry(entry, target)) {
            registered.push(*event);
        }
    }
    Ok(registered)
}

#[derive(Debug, PartialEq, Eq)]
enum RegistrationStatus {
    Registered,
    NotRegistered,
    Partial { missing: Vec<&'static str> },
}

fn registration_status(path: &Path, target: HookTarget) -> Result<RegistrationStatus, String> {
    let registered = registered_events(path, target)?;
    let expected = target_events(target);

    if registered.len() == expected.len() {
        Ok(RegistrationStatus::Registered)
    } else if registered.is_empty() {
        Ok(RegistrationStatus::NotRegistered)
    } else {
        let missing = expected
            .iter()
            .copied()
            .filter(|event| !registered.contains(event))
            .collect();
        Ok(RegistrationStatus::Partial { missing })
    }
}

fn target_label(target: HookTarget) -> &'static str {
    match target {
        HookTarget::Claude => "Claude Code",
        HookTarget::Codex => "Codex",
    }
}

fn overview_is_healthy(statuses: &[Result<RegistrationStatus, String>]) -> bool {
    statuses
        .iter()
        .any(|status| status == &Ok(RegistrationStatus::Registered))
        && statuses.iter().all(|status| {
            matches!(
                status,
                Ok(RegistrationStatus::Registered | RegistrationStatus::NotRegistered)
            )
        })
}

fn install(target: HookTarget, policy_dir: Option<PathBuf>) {
    let path = settings_path(target);
    let added =
        install_to(&path, &binary_path(), target, policy_dir.as_deref()).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
    if added == 0 {
        println!("Hooks already registered in {}", path.display());
    } else {
        println!("Hooks registered in {}", path.display());
    }
}

fn uninstall(target: HookTarget) {
    let path = settings_path(target);
    if !path.exists() {
        println!("No hook file found at {}", path.display());
        return;
    }

    let removed = uninstall_from(&path, target).unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    });
    if removed == 0 {
        println!("Hooks not registered (nothing to remove)");
    } else {
        println!("Hooks removed from {}", path.display());
    }
}

fn status(target: Option<HookTarget>) {
    if let Some(target) = target {
        status_one(target);
        return;
    }

    let statuses: Vec<(HookTarget, PathBuf, Result<RegistrationStatus, String>)> =
        [HookTarget::Claude, HookTarget::Codex]
            .into_iter()
            .map(|target| {
                let path = settings_path(target);
                let status = registration_status(&path, target);
                (target, path, status)
            })
            .collect();

    for (target, path, status) in &statuses {
        match status {
            Ok(RegistrationStatus::Registered) => {
                println!(
                    "{}: registered in {}",
                    target_label(*target),
                    path.display()
                )
            }
            Ok(RegistrationStatus::NotRegistered) => {
                println!("{}: not registered", target_label(*target))
            }
            Ok(RegistrationStatus::Partial { missing }) => println!(
                "{}: partially registered in {} (missing: {})",
                target_label(*target),
                path.display(),
                missing.join(", ")
            ),
            Err(error) => println!("{}: error ({})", target_label(*target), error),
        }
    }

    let states: Vec<Result<RegistrationStatus, String>> =
        statuses.into_iter().map(|(_, _, status)| status).collect();
    if !overview_is_healthy(&states) {
        std::process::exit(1);
    }
}

fn status_one(target: HookTarget) {
    let path = settings_path(target);
    match registration_status(&path, target).unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }) {
        RegistrationStatus::Registered => {
            println!("Registered in {}", path.display());
            return;
        }
        RegistrationStatus::NotRegistered => println!("Not registered"),
        RegistrationStatus::Partial { missing } => {
            println!("Partially registered; missing: {}", missing.join(", "))
        }
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_env(tmp: &TempDir) -> PathBuf {
        tmp.path().join(".claude/settings.json")
    }

    fn install_to(path: &Path, bin: &str) {
        super::install_to(path, bin, HookTarget::Claude, None).unwrap();
    }

    fn uninstall_from(path: &Path) -> bool {
        super::uninstall_from(path, HookTarget::Claude).unwrap() > 0
    }

    fn is_registered(path: &Path) -> bool {
        super::registered_events(path, HookTarget::Claude)
            .unwrap()
            .len()
            == 1
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
    fn test_command_target_defaults_to_claude_and_reads_codex_flag() {
        assert_eq!(
            command_target("/usr/local/bin/cmdguard hook run"),
            Some(HookTarget::Claude)
        );
        assert_eq!(
            command_target("/usr/local/bin/cmdguard hook run --target codex"),
            Some(HookTarget::Codex)
        );
        assert_eq!(
            command_target("/usr/local/bin/cmdguard hook run --target=codex"),
            Some(HookTarget::Codex)
        );
        assert_eq!(
            command_target("/usr/local/bin/cmdguard hook status --target codex"),
            None
        );
        assert_eq!(
            command_target("/usr/local/bin/cmdguard eval --target codex"),
            None
        );
    }

    #[test]
    fn test_is_target_entry_recognizes_customized_claude_entry() {
        // A hand-edited entry: env prefix plus trailing redirection that
        // command_target's clap parse can't handle (unexpected positional
        // argument). Confirm this really is the clap-failure path, then
        // confirm is_target_entry still recognizes it as the claude entry
        // it obviously is, through the production detection path.
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run 2>>/tmp/cmdguard.log";
        assert_eq!(command_target(cmd), None);
        assert!(is_target_entry(&entry(cmd), HookTarget::Claude));
        assert!(!is_target_entry(&entry(cmd), HookTarget::Codex));
    }

    #[test]
    fn test_is_target_entry_recognizes_customized_codex_entry() {
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run --target codex 2>>/tmp/cg.log";
        assert_eq!(command_target(cmd), None);
        assert!(is_target_entry(&entry(cmd), HookTarget::Codex));
        assert!(!is_target_entry(&entry(cmd), HookTarget::Claude));
    }

    #[test]
    fn test_is_target_entry_customized_entry_without_hook_run_not_recognized() {
        // Fallback only fires for entries that actually mention `hook run`;
        // a customized `hook status` invocation must not be misclassified.
        let cmd =
            "RUST_LOG=debug /usr/local/bin/cmdguard hook status --target codex 2>>/tmp/cg.log";
        assert_eq!(command_target(cmd), None);
        assert!(!is_target_entry(&entry(cmd), HookTarget::Codex));
        assert!(!is_target_entry(&entry(cmd), HookTarget::Claude));
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
    fn test_make_hook_entry_quotes_paths_with_spaces() {
        // current_exe() can resolve to a path with spaces (e.g. when the
        // user's home or install dir contains a space). The generated
        // command must shell-quote so Claude Code parses it as a single
        // token; otherwise the leading slice would be invoked as the
        // binary and the rest passed as args.
        let entry = make_hook_entry(
            "/Users/Some User/bin/cmdguard",
            HookTarget::Claude,
            "PreToolUse",
            None,
        );
        let cmd = entry["hooks"][0]["command"].as_str().unwrap();
        // Round-trip: the entry we just generated must be detectable as
        // ours, which proves shlex parses it back to a single bin token.
        assert!(
            is_our_command(cmd),
            "round-trip detection failed for spaced path; got command: {:?}",
            cmd
        );
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

    fn claude_command(path: &Path) -> String {
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn test_install_pins_policy_dir_in_command() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let dir = PathBuf::from("/home/u/.config/cmdguard");

        super::install_to(
            &path,
            "/usr/local/bin/cmdguard",
            HookTarget::Claude,
            Some(&dir),
        )
        .unwrap();

        assert_eq!(
            claude_command(&path),
            "/usr/local/bin/cmdguard hook run --policy-dir /home/u/.config/cmdguard"
        );
    }

    #[test]
    fn test_install_shell_quotes_spaced_policy_dir() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let dir = PathBuf::from("/home/Some User/.config/cmdguard");

        super::install_to(
            &path,
            "/usr/local/bin/cmdguard",
            HookTarget::Claude,
            Some(&dir),
        )
        .unwrap();

        let command = claude_command(&path);
        // Round-trip through the tokenizer: the dir must survive as one arg,
        // otherwise the hook would run against a truncated path.
        let args = command_args(&command).unwrap();
        assert_eq!(args.last().unwrap(), "/home/Some User/.config/cmdguard");
        assert!(is_plain_our_command(&command, HookTarget::Claude));
    }

    #[test]
    fn test_reinstall_with_same_policy_dir_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";
        let dir = PathBuf::from("/home/u/.config/cmdguard");

        super::install_to(&path, bin, HookTarget::Claude, Some(&dir)).unwrap();
        assert_eq!(
            super::install_to(&path, bin, HookTarget::Claude, Some(&dir)),
            Ok(0)
        );
    }

    #[test]
    fn test_reinstall_refreshes_changed_policy_dir() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        super::install_to(
            &path,
            bin,
            HookTarget::Claude,
            Some(Path::new("/old/cmdguard")),
        )
        .unwrap();
        super::install_to(
            &path,
            bin,
            HookTarget::Claude,
            Some(Path::new("/new/cmdguard")),
        )
        .unwrap();

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Refreshed in place: exactly one entry, pointing at the new dir.
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            claude_command(&path),
            "/usr/local/bin/cmdguard hook run --policy-dir /new/cmdguard"
        );
    }

    #[test]
    fn test_reinstall_refreshes_moved_binary() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);

        install_to(&path, "/nix/store/old-cmdguard/bin/cmdguard");
        install_to(&path, "/nix/store/new-cmdguard/bin/cmdguard");

        assert_eq!(
            claude_command(&path),
            "/nix/store/new-cmdguard/bin/cmdguard hook run"
        );
    }

    #[test]
    fn test_reinstall_preserves_decorated_entry_with_moved_binary() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        // Decoration clap cannot reproduce: rewriting would drop the env
        // prefix and the redirection the user added by hand.
        let cmd = "RUST_LOG=debug /nix/store/old-cmdguard/bin/cmdguard hook run 2>>/tmp/cg.log";
        write_settings(&path, &json!({"hooks": {"PreToolUse": [entry(cmd)]}})).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            super::install_to(
                &path,
                "/nix/store/new-cmdguard/bin/cmdguard",
                HookTarget::Claude,
                None
            ),
            Ok(0)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn test_install_leaves_customized_claude_entry_untouched() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run 2>>/tmp/cmdguard.log";
        let settings = json!({"hooks": {"PreToolUse": [entry(cmd)]}});
        write_settings(&path, &settings).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        // Zero entries added means install() prints "already registered"
        // and, crucially, install_to never calls write_settings — the file
        // on disk must be byte-for-byte unchanged.
        assert_eq!(
            super::install_to(&path, bin, HookTarget::Claude, None),
            Ok(0)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn test_install_leaves_customized_codex_entry_untouched() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run --target codex 2>>/tmp/cg.log";
        let settings = json!({
            "hooks": {
                "PreToolUse": [entry(cmd)],
                "PermissionRequest": [entry(cmd)],
            }
        });
        write_settings(&path, &settings).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            super::install_to(&path, bin, HookTarget::Codex, None),
            Ok(0)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn test_status_reports_customized_claude_entry_as_registered() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run 2>>/tmp/cmdguard.log";
        let settings = json!({"hooks": {"PreToolUse": [entry(cmd)]}});
        write_settings(&path, &settings).unwrap();

        assert_eq!(
            super::registration_status(&path, HookTarget::Claude),
            Ok(RegistrationStatus::Registered)
        );
    }

    #[test]
    fn test_status_reports_customized_codex_entry_as_registered() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let cmd = "RUST_LOG=debug /usr/local/bin/cmdguard hook run --target codex 2>>/tmp/cg.log";
        let settings = json!({
            "hooks": {
                "PreToolUse": [entry(cmd)],
                "PermissionRequest": [entry(cmd)],
            }
        });
        write_settings(&path, &settings).unwrap();

        assert_eq!(
            super::registration_status(&path, HookTarget::Codex),
            Ok(RegistrationStatus::Registered)
        );
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
    fn test_install_rejects_malformed_settings_without_overwriting() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{ this is not valid json\n";
        std::fs::write(&path, original).unwrap();

        let error = super::install_to(&path, "/usr/local/bin/cmdguard", HookTarget::Claude, None)
            .unwrap_err();

        assert!(error.contains("Could not parse"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_install_rejects_non_array_event_without_overwriting() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"hooks":{"PreToolUse":{"matcher":"Bash"}}}"#;
        std::fs::write(&path, original).unwrap();

        let error = super::install_to(&path, "/usr/local/bin/cmdguard", HookTarget::Claude, None)
            .unwrap_err();

        assert!(error.contains("non-array `hooks.PreToolUse`"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
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
    fn test_uninstall_preserves_sibling_handler_in_same_matcher_group() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "/usr/local/bin/cmdguard hook run"},
                        {"type": "command", "command": "/usr/bin/other-hook"}
                    ]
                }]
            }
        });
        write_settings(&path, &existing).unwrap();

        assert!(uninstall_from(&path));

        let settings = read_settings(&path).unwrap();
        let groups = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "/usr/bin/other-hook");
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
        assert_eq!(
            registration_status(&path, HookTarget::Claude),
            Ok(RegistrationStatus::NotRegistered)
        );
    }

    #[test]
    fn test_status_registered() {
        let tmp = TempDir::new().unwrap();
        let path = setup_env(&tmp);
        let bin = "/usr/local/bin/cmdguard";

        install_to(&path, bin);
        assert_eq!(
            registration_status(&path, HookTarget::Claude),
            Ok(RegistrationStatus::Registered)
        );
    }

    #[test]
    fn test_codex_status_reports_missing_event() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";
        let settings = json!({
            "hooks": {
                "PreToolUse": [make_hook_entry(bin, HookTarget::Codex, "PreToolUse", None)]
            }
        });
        write_settings(&path, &settings).unwrap();

        assert_eq!(
            registration_status(&path, HookTarget::Codex),
            Ok(RegistrationStatus::Partial {
                missing: vec!["PermissionRequest"]
            })
        );
    }

    #[test]
    fn test_overview_health_requires_one_registration_and_no_partial_state() {
        assert!(!overview_is_healthy(&[
            Ok(RegistrationStatus::NotRegistered),
            Ok(RegistrationStatus::NotRegistered),
        ]));
        assert!(overview_is_healthy(&[
            Ok(RegistrationStatus::Registered),
            Ok(RegistrationStatus::NotRegistered),
        ]));
        assert!(overview_is_healthy(&[
            Ok(RegistrationStatus::Registered),
            Ok(RegistrationStatus::Registered),
        ]));
        assert!(!overview_is_healthy(&[
            Ok(RegistrationStatus::Registered),
            Ok(RegistrationStatus::Partial {
                missing: vec!["PermissionRequest"]
            }),
        ]));
        assert!(!overview_is_healthy(&[
            Ok(RegistrationStatus::Registered),
            Err("malformed config".to_string()),
        ]));
    }

    #[test]
    fn test_codex_install_registers_both_events() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";

        assert_eq!(
            super::install_to(&path, bin, HookTarget::Codex, None),
            Ok(2)
        );
        assert_eq!(
            super::registered_events(&path, HookTarget::Codex),
            Ok(vec!["PreToolUse", "PermissionRequest"])
        );

        let settings = read_settings(&path).unwrap();
        for event in ["PreToolUse", "PermissionRequest"] {
            let entry = &settings["hooks"][event][0];
            assert_eq!(entry["matcher"], "^Bash$");
            assert_eq!(
                entry["hooks"][0]["command"],
                format!("{} hook run --target codex", bin)
            );
        }
    }

    #[test]
    fn test_codex_install_repairs_partial_registration() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";

        let mut settings = json!({"hooks": {}});
        settings["hooks"]["PreToolUse"] =
            json!([make_hook_entry(bin, HookTarget::Codex, "PreToolUse", None)]);
        write_settings(&path, &settings).unwrap();

        assert_eq!(
            super::install_to(&path, bin, HookTarget::Codex, None),
            Ok(1)
        );
        assert_eq!(
            super::registered_events(&path, HookTarget::Codex),
            Ok(vec!["PreToolUse", "PermissionRequest"])
        );
    }

    #[test]
    fn test_codex_install_replaces_wrong_target_entry() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";
        let settings = json!({
            "hooks": {
                "PreToolUse": [make_hook_entry(bin, HookTarget::Claude, "PreToolUse", None)]
            }
        });
        write_settings(&path, &settings).unwrap();

        assert_eq!(
            super::install_to(&path, bin, HookTarget::Codex, None),
            Ok(2)
        );
        let settings = read_settings(&path).unwrap();
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(is_target_entry(&entries[0], HookTarget::Codex));
    }

    #[test]
    fn test_codex_uninstall_removes_both_events_and_preserves_others() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".codex/hooks.json");
        let bin = "/usr/local/bin/cmdguard";
        super::install_to(&path, bin, HookTarget::Codex, None).unwrap();

        let mut settings = read_settings(&path).unwrap();
        settings["hooks"]["PreToolUse"]
            .as_array_mut()
            .unwrap()
            .insert(0, entry("/usr/bin/other-hook"));
        write_settings(&path, &settings).unwrap();

        assert_eq!(super::uninstall_from(&path, HookTarget::Codex), Ok(2));
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "/usr/bin/other-hook"
        );
        assert!(settings["hooks"]["PermissionRequest"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
