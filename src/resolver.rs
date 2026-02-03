use serde::Serialize;
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
}
