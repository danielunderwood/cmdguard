//! Resolution of cmdguard's global configuration directory.
//!
//! `$XDG_CONFIG_HOME/cmdguard` is preferred when that variable points somewhere
//! usable, otherwise `~/.config/cmdguard`. Both candidates are probed for an
//! existing *install* first: a config dir that resolves to a path cmdguard was
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
        is_populated_config_dir,
    )
}

/// Whether `dir` holds an install cmdguard could actually load rules from.
///
/// Mirrors `PolicyEngine::load_policies_with_layout`: either the `base/` +
/// `policies/` layout or a flat directory of `.rego` files. Mere existence is
/// deliberately not enough. An empty `cmdguard/` left behind by a dotfiles
/// manager would otherwise outrank a fully populated install, and because
/// loading an existing-but-ruleless directory succeeds, enforcement would be
/// silently disabled rather than reported as an error.
fn is_populated_config_dir(dir: &Path) -> bool {
    contains_rego(dir) || contains_rego(&dir.join("base")) || contains_rego(&dir.join("policies"))
}

fn contains_rego(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rego"))
    })
}

/// Precedence rules behind [`global_config_dir`], with install probing
/// injected so they can be tested without touching the filesystem or mutating
/// process-wide environment state (`set_var` races other tests).
fn resolve(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
    is_installed: impl Fn(&Path) -> bool,
) -> PathBuf {
    // The XDG spec requires a relative (or empty) value to be treated as unset.
    let xdg = xdg_config_home
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("cmdguard"));
    let legacy = home.map(|dir| dir.join(".config").join("cmdguard"));

    // An existing install wins over preference order, so setting
    // XDG_CONFIG_HOME does not orphan a populated `~/.config/cmdguard`. The
    // reverse is not achievable: once the variable is unset its value is
    // unknowable, so an install made under it is reachable only by setting the
    // variable again.
    for candidate in [xdg.as_ref(), legacy.as_ref()].into_iter().flatten() {
        if is_installed(candidate) {
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
    fn unsetting_xdg_does_strand_an_install_made_under_it() {
        // Documents a real limitation rather than a guarantee: with the
        // variable unset there is nothing left to point at `/home/u/xdg`, so
        // that install becomes unreachable until it is set again. Naming this
        // the other way round would claim behavior `resolve` cannot implement.
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

    // --- probing for an actually-installed config dir ---

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn empty_dir_is_not_populated() {
        let dir = tmp();
        assert!(!is_populated_config_dir(dir.path()));
    }

    #[test]
    fn missing_dir_is_not_populated() {
        let dir = tmp();
        assert!(!is_populated_config_dir(&dir.path().join("absent")));
    }

    #[test]
    fn base_layout_with_rego_is_populated() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join("base")).unwrap();
        std::fs::write(dir.path().join("base/core.rego"), "package cmdguard\n").unwrap();
        assert!(is_populated_config_dir(dir.path()));
    }

    #[test]
    fn policies_dir_with_rego_is_populated() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join("policies")).unwrap();
        std::fs::write(
            dir.path().join("policies/custom.rego"),
            "package cmdguard\n",
        )
        .unwrap();
        assert!(is_populated_config_dir(dir.path()));
    }

    #[test]
    fn flat_rego_layout_is_populated() {
        let dir = tmp();
        std::fs::write(dir.path().join("rules.rego"), "package cmdguard\n").unwrap();
        assert!(is_populated_config_dir(dir.path()));
    }

    #[test]
    fn commands_ncl_alone_is_not_populated() {
        // Nothing here yields a rule, so this dir cannot serve as the config
        // dir; treating it as installed is what silently disables enforcement.
        let dir = tmp();
        std::fs::write(dir.path().join("commands.ncl"), "{}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("policies")).unwrap();
        assert!(!is_populated_config_dir(dir.path()));
    }

    #[test]
    fn empty_xdg_dir_does_not_outrank_populated_legacy_dir() {
        // The regression: `$XDG_CONFIG_HOME/cmdguard` exists but holds no
        // policy, while `~/.config/cmdguard` is a full install. Probing for
        // mere existence picks the empty one and loads zero rules.
        let resolved = resolve(
            Some(path("/home/u/xdg")),
            Some(path("/home/u")),
            only("/home/u/.config/cmdguard"),
        );
        assert_eq!(resolved, path("/home/u/.config/cmdguard"));
    }
}
