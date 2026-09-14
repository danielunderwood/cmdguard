//! Resolution of cmdguard's global configuration directory.
//!
//! `$XDG_CONFIG_HOME/cmdguard` is preferred when that variable points somewhere
//! usable, otherwise `~/.config/cmdguard`. Both candidates are probed for an
//! existing directory first: a config dir that resolves to a path cmdguard was
//! not installed into cannot load any policy, and a policy load error degrades
//! every Claude decision to "ask" and blocks every Codex call, so switching
//! silently is worse than ignoring the variable.

use std::path::{Path, PathBuf};

const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";

/// Fallback used only when the home directory cannot be determined.
const SYSTEM_CONFIG_DIR: &str = "/etc/cmdguard";

/// Resolve the global config directory from the process environment.
pub fn global_config_dir() -> PathBuf {
    resolve(
        std::env::var_os(XDG_CONFIG_HOME).map(PathBuf::from),
        dirs::home_dir(),
        |path| path.is_dir(),
    )
}

/// Precedence rules behind [`global_config_dir`], with directory probing
/// injected so they can be tested without touching the filesystem or mutating
/// process-wide environment state (`set_var` races other tests).
fn resolve(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
    is_dir: impl Fn(&Path) -> bool,
) -> PathBuf {
    // The XDG spec requires a relative (or empty) value to be treated as unset.
    let xdg = xdg_config_home
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("cmdguard"));
    let legacy = home.map(|dir| dir.join(".config").join("cmdguard"));

    // An existing install wins over preference order, in either direction:
    // setting XDG_CONFIG_HOME must not orphan `~/.config/cmdguard`, and
    // unsetting it must not orphan a dir that was created under it.
    for candidate in [xdg.as_ref(), legacy.as_ref()].into_iter().flatten() {
        if is_dir(candidate) {
            return candidate.clone();
        }
    }

    // Fresh install: honor the variable, then `~/.config`.
    xdg.or(legacy)
        .unwrap_or_else(|| PathBuf::from(SYSTEM_CONFIG_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn none_exist(_: &Path) -> bool {
        false
    }

    fn only(existing: &'static str) -> impl Fn(&Path) -> bool {
        move |path| path == Path::new(existing)
    }

    #[test]
    fn prefers_existing_xdg_dir() {
        let resolved = resolve(
            Some(path("/home/u/xdg")),
            Some(path("/home/u")),
            only("/home/u/xdg/cmdguard"),
        );
        assert_eq!(resolved, path("/home/u/xdg/cmdguard"));
    }

    #[test]
    fn existing_legacy_dir_survives_xdg_being_set() {
        let resolved = resolve(
            Some(path("/home/u/xdg")),
            Some(path("/home/u")),
            only("/home/u/.config/cmdguard"),
        );
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }

    #[test]
    fn existing_xdg_dir_survives_xdg_being_unset() {
        // The variable is gone from this process, but the dir it pointed at is
        // not: `~/.config/cmdguard` is absent, so nothing is orphaned.
        let resolved = resolve(None, Some(path("/home/u")), only("/home/u/xdg/cmdguard"));
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }

    #[test]
    fn fresh_install_honors_xdg() {
        let resolved = resolve(Some(path("/home/u/xdg")), Some(path("/home/u")), none_exist);
        assert_eq!(resolved, path("/home/u/xdg/cmdguard"));
    }

    #[test]
    fn fresh_install_without_xdg_uses_dot_config() {
        let resolved = resolve(None, Some(path("/home/u")), none_exist);
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }

    #[test]
    fn relative_xdg_value_is_ignored() {
        let resolved = resolve(
            Some(path("relative/xdg")),
            Some(path("/home/u")),
            none_exist,
        );
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }

    #[test]
    fn empty_xdg_value_is_ignored() {
        let resolved = resolve(Some(path("")), Some(path("/home/u")), none_exist);
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }

    #[test]
    fn xdg_covers_a_missing_home_dir() {
        let resolved = resolve(Some(path("/home/u/xdg")), None, none_exist);
        assert_eq!(resolved, path("/home/u/xdg/cmdguard"));
    }

    #[test]
    fn falls_back_to_system_dir_without_home_or_xdg() {
        assert_eq!(resolve(None, None, none_exist), path(SYSTEM_CONFIG_DIR));
    }
}
