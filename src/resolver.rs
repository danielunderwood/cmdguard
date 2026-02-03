use serde::Serialize;
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Trust zone classification for a binary
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrustZone {
    System,
    User,
    Project,
    Unknown,
}

/// Result of resolving a command to its binary location
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedCommand {
    /// The command exactly as typed (first token)
    pub command_as_typed: String,
    /// Just the binary name (basename)
    pub binary_name: String,
    /// Absolute path to actual binary after symlink resolution, None if not found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    /// Trust zone classification
    pub resolved_trust_zone: TrustZone,
    /// Whether the PATH entry was a symlink
    pub is_symlink: bool,
    /// Where found in PATH before symlink resolution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_source: Option<String>,
}

/// Holds the default paths for each trust zone
#[derive(Debug, Clone)]
pub struct TrustZonePaths {
    pub system: Vec<PathBuf>,
    pub user: Vec<PathBuf>,
}

impl TrustZonePaths {
    /// Get default paths for the current platform
    pub fn defaults() -> Self {
        let home = dirs::home_dir();

        let mut system = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/sbin"),
        ];

        let mut user = Vec::new();

        // Add user paths with home expansion
        if let Some(ref home) = home {
            user.extend([
                home.join(".local/bin"),
                home.join("bin"),
                home.join(".cargo/bin"),
                home.join(".go/bin"),
                home.join("go/bin"),
            ]);
        }

        // Platform-specific additions
        #[cfg(target_os = "macos")]
        {
            system.extend([
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/local/sbin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/opt/homebrew/sbin"),
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            system.extend([
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/local/sbin"),
                PathBuf::from("/snap/bin"),
            ]);

            if let Some(ref home) = home {
                user.extend([
                    home.join(".pyenv/shims"),
                    home.join(".rbenv/shims"),
                    home.join(".asdf/shims"),
                ]);
            }
        }

        // NixOS detection
        if Path::new("/nix/store").exists() {
            system.extend([
                PathBuf::from("/run/current-system/sw/bin"),
                PathBuf::from("/nix/var/nix/profiles/default/bin"),
            ]);

            if let Some(ref home) = home {
                user.push(home.join(".nix-profile/bin"));
            }
        }

        TrustZonePaths { system, user }
    }

    /// Check if a path is in the system zone
    pub fn is_system(&self, path: &Path) -> bool {
        self.system.iter().any(|p| path.starts_with(p))
    }

    /// Check if a path is in the user zone
    pub fn is_user(&self, path: &Path) -> bool {
        self.user.iter().any(|p| path.starts_with(p))
    }
}

/// Resolve a command to its binary location and classify trust zone
pub fn resolve_command(command: &str, project_root: Option<&Path>) -> ResolvedCommand {
    // TODO: Implement in later tasks
    ResolvedCommand {
        command_as_typed: command.to_string(),
        binary_name: Path::new(command)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| command.to_string()),
        resolved_path: None,
        resolved_trust_zone: TrustZone::Unknown,
        is_symlink: false,
        symlink_source: None,
    }
}

/// Invalid paths that should not be used as project roots
const INVALID_PROJECT_ROOTS: &[&str] = &[
    "/",
    "/usr",
    "/home",
    "/var",
    "/etc",
    "/tmp",
    "/opt",
    "/nix",
];

/// Detect project root by walking up from cwd looking for .git
/// Returns None if no valid project root found or if root is invalid
pub fn detect_project_root(cwd: &Path) -> Option<PathBuf> {
    // Walk up looking for .git
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return validate_project_root(&current);
        }

        if !current.pop() {
            // Reached filesystem root without finding .git
            // Fall back to cwd
            return validate_project_root(cwd);
        }
    }
}

fn validate_project_root(path: &Path) -> Option<PathBuf> {
    let path_str = path.to_string_lossy();

    for invalid in INVALID_PROJECT_ROOTS {
        if path_str == *invalid {
            return None;
        }
    }

    Some(path.to_path_buf())
}

/// Find a command in PATH, returning the full path where it was found
/// Returns None if not found
///
/// If command contains '/', treats it as a direct path (not searched in PATH)
pub fn find_in_path(command: &str) -> Option<PathBuf> {
    // If command contains a path separator, treat as direct path
    if command.contains('/') {
        let path = PathBuf::from(command);
        if is_executable(&path) {
            return Some(path);
        }
        return None;
    }

    // Get PATH environment variable
    let path_var = env::var("PATH").ok()?;

    // Search each directory in PATH
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Check if a path exists and is executable
fn is_executable(path: &Path) -> bool {
    match path.metadata() {
        Ok(metadata) => {
            // Check if it's a file and has executable permission
            metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0)
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_detect_project_root_in_git_repo() {
        // This test runs in the claude-hooks repo which has .git
        let cwd = env::current_dir().unwrap();
        let root = detect_project_root(&cwd);
        assert!(root.is_some());
        let root = root.unwrap();
        assert!(root.join(".git").exists());
    }

    #[test]
    fn test_invalid_root_rejected() {
        let root = validate_project_root(Path::new("/"));
        assert!(root.is_none());

        let root = validate_project_root(Path::new("/usr"));
        assert!(root.is_none());

        let root = validate_project_root(Path::new("/home"));
        assert!(root.is_none());
    }

    #[test]
    fn test_valid_root_accepted() {
        let root = validate_project_root(Path::new("/home/user/project"));
        assert!(root.is_some());

        let root = validate_project_root(Path::new("/Users/dev/myapp"));
        assert!(root.is_some());
    }

    #[test]
    fn test_trust_zone_paths_defaults() {
        let paths = TrustZonePaths::defaults();

        // System paths should always include /usr/bin
        assert!(paths.system.contains(&PathBuf::from("/usr/bin")));

        // Should have some user paths if home dir exists
        if dirs::home_dir().is_some() {
            assert!(!paths.user.is_empty());
        }
    }

    #[test]
    fn test_is_system_path() {
        let paths = TrustZonePaths::defaults();

        assert!(paths.is_system(Path::new("/usr/bin/git")));
        assert!(paths.is_system(Path::new("/bin/ls")));
        assert!(!paths.is_system(Path::new("/home/user/bin/tool")));
    }

    #[test]
    fn test_is_user_path() {
        let paths = TrustZonePaths::defaults();

        if let Some(home) = dirs::home_dir() {
            let user_bin = home.join(".local/bin/mytool");
            assert!(paths.is_user(&user_bin));

            let cargo_bin = home.join(".cargo/bin/rustfmt");
            assert!(paths.is_user(&cargo_bin));
        }
    }

    #[test]
    fn test_find_in_path_common_commands() {
        // These commands should exist on most Unix systems
        let git = find_in_path("git");
        // git might not be installed, so just check the logic works
        if git.is_some() {
            let git_path = git.unwrap();
            assert!(git_path.exists());
            assert!(git_path.to_string_lossy().contains("git"));
        }

        // ls should definitely exist
        let ls = find_in_path("ls");
        assert!(ls.is_some());
        let ls_path = ls.unwrap();
        assert!(ls_path.exists());
    }

    #[test]
    fn test_find_in_path_not_found() {
        let result = find_in_path("this_command_definitely_does_not_exist_12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_in_path_direct_path() {
        // Test with a direct path
        let result = find_in_path("/bin/ls");
        if Path::new("/bin/ls").exists() {
            assert!(result.is_some());
            assert_eq!(result.unwrap(), PathBuf::from("/bin/ls"));
        }
    }

    #[test]
    fn test_find_in_path_relative_path() {
        // Relative paths should be treated as direct paths
        let result = find_in_path("./nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_is_executable() {
        // /bin/ls should be executable
        if Path::new("/bin/ls").exists() {
            assert!(is_executable(Path::new("/bin/ls")));
        }

        // A non-existent file should not be executable
        assert!(!is_executable(Path::new("/nonexistent/path")));
    }
}
