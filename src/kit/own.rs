//! Cmd+Meow never offers to clean, turn off, quit, remove or uninstall
//! itself: not the app (release or Dev build), its bundle ids, login item,
//! helpers, processes or data. The one exception is its own cache folder
//! (`~/Library/Caches/<bundle id>`), which Cleanup may clear like any app's.
//!
//! Every tool and AI Check go through these checks, so a rule, a scan or an
//! agent's answer can't slip one through.

use std::path::Path;

/// Lower-case letters and digits only: "Cmd+Meow", "CmdMeow Dev.app" and
/// "com.romanfurman6.cmdmeow.dev" all read as "cmdmeow…".
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Whether a name, label or bundle id is Cmd+Meow's (also under its first
/// name, CleanMeBro).
pub fn is_own_name(s: &str) -> bool {
    let s = squash(s);
    s.contains("cmdmeow") || s.contains("cleanmebro")
}

/// Whether text an agent wrote is about Cmd+Meow. Only the current name: the
/// old one is also the name of its source folder, a plain code project.
pub fn mentions(s: &str) -> bool {
    squash(s).contains("cmdmeow")
}

/// Whether `p` is Cmd+Meow or something it put on the Mac (or inside one):
/// an app bundle named after it, an entry named after it right inside one of
/// the `Library` folders apps keep their things in (`Application Support`,
/// `Caches`, `Logs`, `Preferences`, `LaunchAgents`, `LaunchDaemons`,
/// `PrivilegedHelperTools`…), or its root-owned files in `/var/db`. Deeper
/// folders that only carry the name (Xcode's `DerivedData/CmdMeow-…`, its
/// source folder) are build output of a code project, not the app.
pub fn is_own_path(p: &Path) -> bool {
    if p.to_string_lossy().starts_with("/var/db/com.romanfurman6.") {
        return true;
    }
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    parts.iter().enumerate().any(|(i, c)| is_own_name(c) && (c.to_ascii_lowercase().ends_with(".app") || (i >= 2 && parts[i - 2] == "Library")))
}

/// Cmd+Meow's own cache folder, or something in it: safe to clear, the app
/// makes it again.
pub fn is_safe_cache(p: &Path) -> bool {
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    parts.windows(3).any(|w| w[0] == "Library" && w[1] == "Caches" && matches!(w[2].to_ascii_lowercase().as_str(), "com.romanfurman6.cmdmeow" | "com.romanfurman6.cmdmeow.dev"))
}

/// Whether `p` is Cmd+Meow's and no tool may offer to remove it.
pub fn is_kept(p: &Path) -> bool {
    is_own_path(p) && !is_safe_cache(p)
}

/// Whether an item a tool lists (by key, name and icon path) is Cmd+Meow
/// itself. A key that is a full path is judged as a path, anything else (a
/// label, a bundle id, "app:<name>", "pid:<n>") as a name.
pub fn is_own_item(key: &str, name: &str, icon: Option<&Path>) -> bool {
    let key_own = if key.starts_with('/') { is_kept(Path::new(key)) } else { is_own_name(key) };
    let name_own = !key.starts_with('/') && is_own_name(name);
    key_own || name_own || icon.is_some_and(is_own_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn names() {
        for s in ["CmdMeow", "CmdMeow Dev", "Cmd+Meow", "cmdmeow", "com.romanfurman6.cmdmeow", "com.romanfurman6.cmdmeow.dev", "com.romanfurman6.cmdmeow.helper", "CleanMeBro", "CmdMeow Dev.app"] {
            assert!(is_own_name(s), "{s}");
        }
        for s in ["Safari", "Raycast", "com.google.keystone.daemon", "Meow", "cmd", ""] {
            assert!(!is_own_name(s), "{s}");
        }
    }

    #[test]
    fn paths() {
        for p in [
            "/Applications/CmdMeow.app",
            "/Applications/CmdMeow Dev.app/Contents/MacOS/cmdmeow",
            "/Users/me/Library/Application Support/cmdmeow",
            "/Users/me/Library/Application Support/cmdmeow-dev/clipboard",
            "/Users/me/Library/Logs/CmdMeow",
            "/Users/me/Library/Preferences/com.romanfurman6.cmdmeow.plist",
            "/Users/me/Library/LaunchAgents/com.romanfurman6.cmdmeow.agent.plist",
            "/Library/LaunchDaemons/com.romanfurman6.cmdmeow.helper.plist",
            "/Library/PrivilegedHelperTools/com.romanfurman6.cmdmeow.helper",
            "/var/db/com.romanfurman6.cmdmeow.charge.json",
            "/Users/me/Library/Caches/com.romanfurman6.cmdmeow",
        ] {
            assert!(is_own_path(Path::new(p)), "{p}");
        }
        // its source folder and build output are a code project like any other
        for p in ["/Users/me/Projects/cleanmebro", "/Users/me/Projects/cleanmebro/target", "/Users/me/Library/Developer/Xcode/DerivedData/CmdMeow-abc123", "/Users/me/code/cmdmeow/site", "/Applications/Safari.app", "/Users/me/Library/Caches/com.apple.Safari", "/Users/me/CmdMeow-reset-backup"] {
            assert!(!is_own_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn only_the_cache_may_go() {
        for p in ["/Users/me/Library/Caches/com.romanfurman6.cmdmeow", "/Users/me/Library/Caches/com.romanfurman6.cmdmeow.dev", "/Users/me/Library/Caches/com.romanfurman6.cmdmeow.dev/Cache.db"] {
            assert!(is_safe_cache(Path::new(p)), "{p}");
            assert!(!is_kept(Path::new(p)), "{p}");
        }
        for p in ["/Users/me/Library/Application Support/cmdmeow", "/Applications/CmdMeow.app", "/Users/me/Library/Logs/CmdMeow", "/Users/me/Library/Caches", "/Users/me/Library/Caches/com.romanfurman6.cmdmeowXYZ"] {
            assert!(!is_safe_cache(Path::new(p)), "{p}");
        }
        assert!(is_kept(Path::new("/Users/me/Library/Application Support/cmdmeow")));
    }

    #[test]
    fn items() {
        assert!(is_own_item("app:CmdMeow Dev", "CmdMeow Dev", Some(Path::new("/Applications/CmdMeow Dev.app"))));
        assert!(is_own_item("com.romanfurman6.cmdmeow.dev", "CmdMeow Dev", None));
        assert!(is_own_item("pid:4242", "CmdMeow Dev", None));
        assert!(is_own_item("/Users/me/Library/Application Support/cmdmeow", "cmdmeow", None));
        assert!(!is_own_item("/Users/me/Library/Caches/com.romanfurman6.cmdmeow.dev", "Cmd+Meow", None));
        // a folder named after the source project, on Disk Map
        assert!(!is_own_item("/Users/me/Projects/cleanmebro", "cleanmebro", None));
        assert!(!is_own_item("app:Raycast", "Raycast", Some(Path::new("/Applications/Raycast.app"))));
    }

    #[test]
    fn mentions_only_the_current_name() {
        assert!(mentions("Stop CmdMeow Dev starting at login"));
        assert!(mentions("Quit Cmd+Meow"));
        assert!(!mentions("Clear cleanmebro build output"));
        assert!(!mentions("Stop Raycast starting at login"));
    }
}
