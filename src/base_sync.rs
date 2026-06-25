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

/// FNV-1a 64-bit over the embedded base bundle. Content-based so identical
/// policies across releases produce identical hashes (no spurious warnings).
pub fn embedded_base_hash() -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(PRIME);
        }
    };
    let mut entries: Vec<(&str, &str)> = embedded::BASE_FILES.to_vec();
    entries.push(("builtins.ncl", embedded::BUILTINS_NCL));
    for (name, contents) in entries {
        feed(name.as_bytes());
        feed(&[0]);
        feed(contents.as_bytes());
        feed(&[0]);
    }
    hash
}

fn manifest_hex() -> String {
    format!("{:016x}", embedded_base_hash())
}

/// True iff the user is on the base layout and the on-disk base differs from
/// (or predates) the embedded bundle. Never returns true on IO error or when
/// there is no populated base/ directory.
pub fn base_is_stale(config_dir: &Path) -> bool {
    let base_dir = config_dir.join("base");
    let has_base_rego = std::fs::read_dir(&base_dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rego"))
        })
        .unwrap_or(false);
    if !has_base_rego {
        return false; // flat-layout or not installed: not "stale"
    }
    match std::fs::read_to_string(base_dir.join(".manifest")) {
        Ok(on_disk) => on_disk.trim() != manifest_hex(),
        Err(_) => true, // base present but no manifest => old install => stale
    }
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
/// Write `contents` to `path`, relaxing permissions first if it already exists
/// (re-sync), then locking it back to read-only. A failed lockdown is recorded
/// in `failures` rather than aborting, so we never claim success while leaving
/// files writable; a failed *write* is fatal.
fn write_locked_file(path: &Path, contents: &str, label: &str, failures: &mut Vec<String>) {
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
        failures.push(format!("{}: {}", label, e));
    }
    println!("  {}", label);
}

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

    for (filename, contents) in embedded::BASE_FILES {
        write_locked_file(
            &base_dir.join(filename),
            contents,
            filename,
            &mut lockdown_failures,
        );
    }
    write_locked_file(
        &base_dir.join("builtins.ncl"),
        embedded::BUILTINS_NCL,
        "builtins.ncl",
        &mut lockdown_failures,
    );
    write_locked_file(
        &base_dir.join(".manifest"),
        &manifest_hex(),
        ".manifest",
        &mut lockdown_failures,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_hash_is_stable_and_nonzero() {
        let h1 = embedded_base_hash();
        let h2 = embedded_base_hash();
        assert_eq!(h1, h2);
        assert_ne!(h1, 0);
    }

    #[test]
    fn fresh_sync_is_not_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        run(tmp.path().to_path_buf()); // writes base/ + .manifest
        assert!(!base_is_stale(tmp.path()));
    }

    #[test]
    fn missing_manifest_with_base_is_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        // a base rego file exists, but no .manifest (old install)
        std::fs::write(base.join("stdlib.rego"), "package cmdguard\n").unwrap();
        assert!(base_is_stale(tmp.path()));
    }

    #[test]
    fn no_base_dir_is_not_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(!base_is_stale(tmp.path()));
    }

    #[test]
    fn wrong_manifest_hash_is_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        run(tmp.path().to_path_buf());
        let manifest = tmp.path().join("base").join(".manifest");
        // .manifest is written read-only (0444) by sync; relax before overwrite.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        std::fs::write(&manifest, "deadbeefdeadbeef").unwrap();
        assert!(base_is_stale(tmp.path()));
    }
}
