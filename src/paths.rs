use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DetectedPath {
    pub raw: String,
    pub resolved: String,
    pub exists: bool,
    pub is_dir: bool,
}

/// Detect and resolve paths in command arguments
pub fn detect_paths(tokens: &[String], cwd: &Path) -> Vec<DetectedPath> {
    tokens
        .iter()
        .filter(|t| looks_like_path(t))
        .map(|t| resolve_path(t, cwd))
        .collect()
}

fn looks_like_path(token: &str) -> bool {
    // Skip flags
    if token.starts_with('-') {
        return false;
    }

    // Contains path separator
    if token.contains('/') || token.contains('\\') {
        return true;
    }

    // Starts with . (relative path)
    if token.starts_with('.') {
        return true;
    }

    // Check if it exists on filesystem (but not just any word)
    // Only do this for tokens that have some path-like quality
    false
}

fn resolve_path(raw: &str, cwd: &Path) -> DetectedPath {
    let path = Path::new(raw);

    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    // Canonicalize if possible (resolves .., symlinks)
    let resolved = resolved.canonicalize().unwrap_or(resolved);

    let exists = resolved.exists();
    let is_dir = resolved.is_dir();

    DetectedPath {
        raw: raw.to_string(),
        resolved: resolved.to_string_lossy().to_string(),
        exists,
        is_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_detect_absolute_path() {
        let tokens = to_vec(&["rm", "-rf", "/tmp/foo"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "/tmp/foo");
        assert!(paths[0].resolved.starts_with("/tmp/foo"));
    }

    #[test]
    fn test_detect_relative_path() {
        let tokens = to_vec(&["rm", "-rf", "./build"]);
        let cwd = env::current_dir().unwrap();
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "./build");
    }

    #[test]
    fn test_detect_path_with_slash() {
        let tokens = to_vec(&["cat", "src/main.rs"]);
        let cwd = env::current_dir().unwrap();
        let paths = detect_paths(&tokens, &cwd);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "src/main.rs");
    }

    #[test]
    fn test_skip_flags() {
        let tokens = to_vec(&["ls", "-la", "/tmp"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        // Should only detect /tmp, not -la
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "/tmp");
    }

    #[test]
    fn test_no_paths() {
        let tokens = to_vec(&["echo", "hello", "world"]);
        let cwd = PathBuf::from("/home/user");
        let paths = detect_paths(&tokens, &cwd);

        assert!(paths.is_empty());
    }
}
