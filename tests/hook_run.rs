use std::io::Write;
use std::process::{Command, Stdio};

/// Spawn `cmdguard hook run` against the repo's `config/` policy bundle,
/// feed a PreToolUse payload on stdin, capture stdout and exit code.
/// `env` is extra environment variables to set.
fn run_hook(command: &str, cwd: &str, env: &[(&str, &str)]) -> (String, String, i32) {
    run_hook_for(command, cwd, env, "claude", None)
}

fn run_hook_for(
    command: &str,
    cwd: &str,
    env: &[(&str, &str)],
    target: &str,
    event: Option<&str>,
) -> (String, String, i32) {
    let mut payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": cwd,
    });
    if let Some(event) = event {
        payload["hook_event_name"] = serde_json::Value::String(event.to_string());
    }
    let payload = payload.to_string();

    // Repo-local source-of-truth policies, so tests don't depend on whatever
    // is synced into the developer's ~/.config/cmdguard.
    let policy_dir = format!("{}/config", env!("CARGO_MANIFEST_DIR"));

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cmdguard"));
    cmd.arg("hook")
        .arg("run")
        .arg("--policy-dir")
        .arg(&policy_dir)
        .arg("--target")
        .arg(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().expect("failed to spawn cmdguard");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn allow_is_silent_pass_with_allow_json() {
    let (stdout, _stderr, code) = run_hook("git status", ".", &[]);
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""permissionDecision":"allow""#),
        "expected allow JSON, got: {stdout}"
    );
}

#[test]
fn deny_emits_deny_json() {
    let (stdout, _stderr, code) = run_hook("rm --no-preserve-root /", ".", &[]);
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""permissionDecision":"deny""#),
        "expected deny JSON, got: {stdout}"
    );
}

#[test]
fn intentional_ask_emits_ask_json_with_reason() {
    // git push is a deliberate policy ask, not a fallthrough.
    let (stdout, _stderr, code) = run_hook("git push", ".", &[]);
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""permissionDecision":"ask""#),
        "expected ask JSON, got: {stdout}"
    );
    assert!(
        stdout.contains("systemMessage"),
        "intentional ask should carry a reason, got: {stdout}"
    );
}

#[test]
fn fallthrough_defers_silently_by_default() {
    // A command no policy recognizes: must produce EMPTY stdout, exit 0.
    let (stdout, _stderr, code) = run_hook("totallyunknownbinary123 --wat", ".", &[]);
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(
        stdout.trim(),
        "",
        "expected silent defer (no stdout), got: {stdout}"
    );
}

#[test]
fn fallthrough_prompts_when_defer_mode_prompt() {
    let (stdout, _stderr, code) = run_hook(
        "totallyunknownbinary123 --wat",
        ".",
        &[("CMDGUARD_DEFER_MODE", "prompt")],
    );
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""permissionDecision":"ask""#),
        "expected ask JSON under prompt mode, got: {stdout}"
    );
}

#[test]
fn codex_pre_tool_use_allow_falls_through() {
    let (stdout, _stderr, code) = run_hook_for("git status", ".", &[], "codex", Some("PreToolUse"));
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(
        stdout.trim(),
        "",
        "expected Codex fallthrough, got: {stdout}"
    );
}

#[test]
fn codex_pre_tool_use_deny_blocks() {
    let (stdout, _stderr, code) = run_hook_for(
        "rm --no-preserve-root /",
        ".",
        &[],
        "codex",
        Some("PreToolUse"),
    );
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""hookEventName":"PreToolUse""#)
            && stdout.contains(r#""permissionDecision":"deny""#),
        "expected Codex PreToolUse deny JSON, got: {stdout}"
    );
}

#[test]
fn codex_pre_tool_use_ask_falls_through() {
    let (stdout, _stderr, code) = run_hook_for("git push", ".", &[], "codex", Some("PreToolUse"));
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(
        stdout.trim(),
        "",
        "Codex PreToolUse cannot force ask; expected fallthrough, got: {stdout}"
    );
}

#[test]
fn codex_pre_tool_use_prompt_mode_still_falls_through() {
    let (stdout, _stderr, code) = run_hook_for(
        "totallyunknownbinary123 --wat",
        ".",
        &[("CMDGUARD_DEFER_MODE", "prompt")],
        "codex",
        Some("PreToolUse"),
    );
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(stdout.trim(), "");
}

#[test]
fn codex_pre_tool_use_parse_error_falls_through() {
    let (stdout, _stderr, code) =
        run_hook_for(r#"echo "unclosed"#, ".", &[], "codex", Some("PreToolUse"));
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(stdout.trim(), "");
}

#[test]
fn codex_permission_request_allows_policy_allow() {
    let (stdout, _stderr, code) =
        run_hook_for("git status", ".", &[], "codex", Some("PermissionRequest"));
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""hookEventName":"PermissionRequest""#)
            && stdout.contains(r#""behavior":"allow""#),
        "expected Codex PermissionRequest allow JSON, got: {stdout}"
    );
}

#[test]
fn codex_permission_request_denies_policy_deny() {
    let (stdout, _stderr, code) = run_hook_for(
        "rm --no-preserve-root /",
        ".",
        &[],
        "codex",
        Some("PermissionRequest"),
    );
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert!(
        stdout.contains(r#""behavior":"deny""#),
        "expected Codex PermissionRequest deny JSON, got: {stdout}"
    );
}

#[test]
fn codex_permission_request_ask_preserves_prompt() {
    let (stdout, _stderr, code) =
        run_hook_for("git push", ".", &[], "codex", Some("PermissionRequest"));
    assert_eq!(code, 0, "exit code; stdout={stdout}");
    assert_eq!(
        stdout.trim(),
        "",
        "expected no decision so Codex keeps its approval prompt, got: {stdout}"
    );
}

#[test]
fn codex_missing_event_fails_closed_without_emitting_invalid_output() {
    let (stdout, stderr, code) = run_hook_for("git status", ".", &[], "codex", None);
    assert_eq!(code, 2, "exit code; stdout={stdout}; stderr={stderr}");
    assert_eq!(stdout.trim(), "");
    assert!(stderr.contains("Unsupported or missing Codex hook_event_name"));
}
