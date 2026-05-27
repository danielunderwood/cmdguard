//! `cmdguard base sync` — write the embedded base policy bundle to disk.
//!
//! The base layer is shipped inside the binary via `include_str!` so the tool
//! works without any external file. `base sync` materializes those files in
//! `~/.config/cmdguard/base/` (read-only) and seeds an empty
//! `~/.config/cmdguard/policies/custom.rego` for user overrides.

use std::path::{Path, PathBuf};

/// The embedded policy bundle baked into the binary at build time.
mod embedded {
    pub const STDLIB_REGO: &str = include_str!("../config/stdlib.rego");
    pub const SAFE_REGO: &str = include_str!("../config/safe.rego");
    pub const GIT_REGO: &str = include_str!("../config/git.rego");
    pub const RUST_REGO: &str = include_str!("../config/rust.rego");
    pub const GO_REGO: &str = include_str!("../config/go.rego");
    pub const PYTHON_REGO: &str = include_str!("../config/python.rego");
    pub const JAVASCRIPT_REGO: &str = include_str!("../config/javascript.rego");
    pub const GH_REGO: &str = include_str!("../config/gh.rego");
    pub const KUBECTL_REGO: &str = include_str!("../config/kubectl.rego");
    pub const FIND_REGO: &str = include_str!("../config/find.rego");
    pub const DOCKER_REGO: &str = include_str!("../config/docker.rego");
    pub const FILE_OPS_REGO: &str = include_str!("../config/file-ops.rego");
    pub const NETWORK_REGO: &str = include_str!("../config/network.rego");
    pub const SED_REGO: &str = include_str!("../config/sed.rego");
    pub const INPROJECT_REGO: &str = include_str!("../config/inproject.rego");
    pub const TOOLS_REGO: &str = include_str!("../config/tools.rego");
    pub const BUILTINS_NCL: &str = include_str!("../config/builtins.ncl");
    pub const CUSTOM_REGO_TEMPLATE: &str = include_str!("../config/policies/custom.rego");

    pub const BASE_FILES: &[(&str, &str)] = &[
        ("stdlib.rego", STDLIB_REGO),
        ("safe.rego", SAFE_REGO),
        ("git.rego", GIT_REGO),
        ("rust.rego", RUST_REGO),
        ("go.rego", GO_REGO),
        ("python.rego", PYTHON_REGO),
        ("javascript.rego", JAVASCRIPT_REGO),
        ("gh.rego", GH_REGO),
        ("kubectl.rego", KUBECTL_REGO),
        ("find.rego", FIND_REGO),
        ("docker.rego", DOCKER_REGO),
        ("file-ops.rego", FILE_OPS_REGO),
        ("network.rego", NETWORK_REGO),
        ("sed.rego", SED_REGO),
        ("inproject.rego", INPROJECT_REGO),
        ("tools.rego", TOOLS_REGO),
    ];
}

/// Cross-platform helpers for the `base sync` filesystem layout.
/// On Unix the base/ directory and its contents are locked to read-only
/// (0444 / 0555) after writing. On non-Unix these are no-ops — Windows
/// doesn't have a Unix mode to set, and the security claim is documented
/// as Unix-only in the README.
mod permissions {
    use std::path::Path;

    #[cfg(unix)]
    fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    #[cfg(not(unix))]
    fn chmod(_path: &Path, _mode: u32) -> std::io::Result<()> {
        Ok(())
    }

    /// Lock a file to read-only for owner/group/other (0o444). No-op on non-Unix.
    pub fn lock_readonly_file(path: &Path) -> std::io::Result<()> {
        chmod(path, 0o444)
    }

    /// Lock a directory to read+execute (0o555) so nothing can be added.
    /// No-op on non-Unix.
    pub fn lock_readonly_dir(path: &Path) -> std::io::Result<()> {
        chmod(path, 0o555)
    }

    /// Make a previously locked file writable so it can be overwritten on
    /// re-sync. No-op on non-Unix.
    pub fn relax_file_for_rewrite(path: &Path) -> std::io::Result<()> {
        chmod(path, 0o644)
    }

    /// Make a previously locked directory writable so files can be replaced
    /// on re-sync. No-op on non-Unix.
    pub fn relax_dir_for_rewrite(path: &Path) -> std::io::Result<()> {
        chmod(path, 0o755)
    }
}

/// Write the embedded base bundle into `config_dir`. Equivalent to running
/// `cmdguard base sync` with `~/.config/cmdguard` as `config_dir`. Exits the
/// process on a fatal failure (write error or directory creation); returns
/// normally and exits with status 2 if the post-write read-only lockdown
/// fails on any file. The exit-on-error pattern matches the rest of the CLI.
pub fn run(config_dir: PathBuf) {
    let base_dir = config_dir.join("base");

    std::fs::create_dir_all(&base_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create base directory: {}", e);
        std::process::exit(1);
    });

    // Track non-fatal lockdown failures so we don't claim success when files
    // remain writable on Unix.
    let mut lockdown_failures: Vec<String> = Vec::new();

    // Make base directory writable for re-sync. If this fails, the writes below
    // will fail too, so don't pre-empt with a hard error here.
    if base_dir.exists() {
        if let Err(e) = permissions::relax_dir_for_rewrite(&base_dir) {
            eprintln!(
                "Warning: could not relax base/ permissions for re-sync: {}",
                e
            );
        }
    }

    println!("Syncing base policies to {}", base_dir.display());
    println!();

    // Helper to write a file, relaxing permissions on re-sync, then locking
    // it back to read-only. Tracks lockdown failures in the outer Vec so we
    // don't claim success while leaving files writable.
    let mut write_locked_file = |path: &Path, contents: &str, label: &str| {
        if path.exists() {
            if let Err(e) = permissions::relax_file_for_rewrite(path) {
                eprintln!(
                    "Warning: could not relax {} permissions for re-sync: {}",
                    label, e
                );
            }
        }
        std::fs::write(path, contents).unwrap_or_else(|e| {
            eprintln!("Failed to write {}: {}", label, e);
            std::process::exit(1);
        });
        if let Err(e) = permissions::lock_readonly_file(path) {
            lockdown_failures.push(format!("{}: {}", label, e));
        }
        println!("  {}", label);
    };

    for (filename, contents) in embedded::BASE_FILES {
        write_locked_file(&base_dir.join(filename), contents, filename);
    }
    write_locked_file(
        &base_dir.join("builtins.ncl"),
        embedded::BUILTINS_NCL,
        "builtins.ncl",
    );

    if let Err(e) = permissions::lock_readonly_dir(&base_dir) {
        lockdown_failures.push(format!("base/: {}", e));
    }

    // Create policies directory if it doesn't exist
    let policies_dir = config_dir.join("policies");
    if !policies_dir.exists() {
        std::fs::create_dir_all(&policies_dir).unwrap_or_else(|e| {
            eprintln!("Failed to create policies directory: {}", e);
            std::process::exit(1);
        });

        // Create starter custom.rego from the embedded template.
        let custom_path = policies_dir.join("custom.rego");
        std::fs::write(&custom_path, embedded::CUSTOM_REGO_TEMPLATE).unwrap_or_else(|e| {
            eprintln!("Failed to write custom.rego: {}", e);
            std::process::exit(1);
        });
        println!("  policies/custom.rego (starter template)");
    }

    println!();
    println!("Base policies synced to {}", base_dir.display());

    if !lockdown_failures.is_empty() {
        eprintln!();
        eprintln!("Warning: failed to lock down base file permissions to read-only:");
        for failure in &lockdown_failures {
            eprintln!("  - {}", failure);
        }
        eprintln!("Base files may be writable. The sandbox layer (filesystem ACLs/sandbox) is the");
        eprintln!("only thing protecting them from modification — review your environment.");
        std::process::exit(2);
    }
}
