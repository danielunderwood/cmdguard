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

/// Detect project root by walking up from cwd looking for .git
/// Returns None if no valid project root found or if root is invalid (/, /usr, etc.)
pub fn detect_project_root(cwd: &Path) -> Option<PathBuf> {
    // TODO: Implement in later tasks
    None
}
