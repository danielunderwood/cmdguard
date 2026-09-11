use std::path::Path;
use std::process::Command;

/// Spawn `cmdguard lint` against the repo's `config/` policy bundle with the
/// process cwd set to `cwd`, capture stdout and exit code.
fn run_lint(cwd: &Path, fail_on: &str) -> (String, i32) {
    let policy_dir = format!("{}/config", env!("CARGO_MANIFEST_DIR"));
    run_lint_with_policy_dir(cwd, Path::new(&policy_dir), fail_on)
}

/// Same as `run_lint` but with an explicit `--policy-dir`, for cases that
/// lint a standalone flat policy directory rather than the repo's config.
fn run_lint_with_policy_dir(cwd: &Path, policy_dir: &Path, fail_on: &str) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_cmdguard"))
        .arg("lint")
        .arg("--policy-dir")
        .arg(policy_dir)
        .arg("--fail-on")
        .arg(fail_on)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn cmdguard");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn lint_reports_findings_from_project_local_policy_dir() {
    // `cmdguard lint` must load and lint the project-local `.cmdguard/`
    // directory (the same way the hook path does), not just the global
    // policy dir passed via --policy-dir.
    let temp = tempfile::tempdir().expect("create temp dir");
    let project_root = temp.path();

    // Mark this as a project root so `detect_project_root` finds it.
    std::fs::create_dir(project_root.join(".git")).expect("create .git");

    let cmdguard_dir = project_root.join(".cmdguard");
    std::fs::create_dir(&cmdguard_dir).expect("create .cmdguard");
    std::fs::write(
        cmdguard_dir.join("custom.rego"),
        r#"
package cmdguard
import rego.v1

rules["allow_bad"] := allow_at("bad idea", 90) if {
	input.binary_name == "bad"
}
"#,
    )
    .expect("write custom.rego");

    let (stdout, code) = run_lint(project_root, "warning");

    assert!(
        stdout.contains("policy/high-priority-allow"),
        "expected project-local policy to be linted, got: {stdout}"
    );
    assert!(
        stdout.contains("custom.rego"),
        "expected finding to point at custom.rego, got: {stdout}"
    );
    assert_eq!(
        code, 1,
        "warning-level finding should fail the lint under --fail-on warning; stdout={stdout}"
    );
}

#[test]
fn lint_ignores_commented_out_allow_at_and_exits_clean() {
    // A commented-out allow_at call must not be flagged, and with only
    // clean findings, --fail-on warning must still exit 0.
    let policy_dir = tempfile::tempdir().expect("create temp policy dir");
    std::fs::write(
        policy_dir.path().join("custom.rego"),
        r#"
package cmdguard
import rego.v1

# rules["allow_bad"] := allow_at("bad idea", 99) if {
#	input.binary_name == "bad"
# }
"#,
    )
    .expect("write custom.rego");

    let cwd = tempfile::tempdir().expect("create temp cwd");
    let (stdout, code) = run_lint_with_policy_dir(cwd.path(), policy_dir.path(), "warning");

    assert!(
        !stdout.contains("policy/high-priority-allow"),
        "expected no findings for commented-out call, got: {stdout}"
    );
    assert_eq!(code, 0, "clean lint should exit 0; stdout={stdout}");
}
