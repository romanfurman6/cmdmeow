//! The installed apps: where they are, what they are called, where they came
//! from, when they were last opened, and which ones must never be removed.

pub use crate::kit::bundles::{app_roots, find_bundles, read_info};
use crate::kit::plist;
use std::path::{Path, PathBuf};

/// Where an app came from (a filter on the Apps page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Direct,
    AppStore,
    Homebrew,
    Setapp,
    Apple,
}

/// Bundle ids of what an app carries inside it (extensions, login items,
/// helpers, XPC services, system extensions) from the same vendor, beyond
/// those already under the app's own id. Their data ("~/Library/Containers/
/// com.1password.safari") belongs to the app too.
pub fn embedded_ids(app: &Path, main_id: &str) -> Vec<String> {
    let main = main_id.to_lowercase();
    let vendor: Vec<&str> = main.split('.').take(2).collect();
    if vendor.len() < 2 {
        return Vec::new();
    }
    let vendor = format!("{}.{}.", vendor[0], vendor[1]);
    let mut out = Vec::new();
    for sub in ["Contents/PlugIns", "Contents/Library/LoginItems", "Contents/Library/SystemExtensions", "Contents/XPCServices", "Contents/Helpers", "Contents/Frameworks", "Contents/Library/Helpers"] {
        let Ok(rd) = std::fs::read_dir(app.join(sub)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| ["app", "appex", "xpc", "systemextension"].iter().any(|k| x == *k)) {
                continue;
            }
            let Some(v) = plist::read(&p.join("Contents/Info.plist")) else { continue };
            let Some(id) = v.get_str("CFBundleIdentifier").map(|s| s.trim().to_lowercase()) else { continue };
            let below_main = id == main || id.starts_with(&format!("{main}."));
            if id.starts_with(&vendor) && !below_main && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out.sort();
    out
}

/// A Homebrew cask: its token, its Caskroom folder and the apps it put in
/// place ("Zoom.app").
#[derive(Clone, Debug, PartialEq)]
pub struct Cask {
    pub token: String,
    pub dir: PathBuf,
    pub apps: Vec<String>,
}

/// Where Homebrew keeps casks, under the system root.
pub const CASKROOMS: &[&str] = &["opt/homebrew/Caskroom", "usr/local/Caskroom"];

/// Every installed cask. The apps come from the version folders, where
/// Homebrew leaves a link to each app it moved to /Applications.
pub fn casks(system: &Path) -> Vec<Cask> {
    let mut out = Vec::new();
    for root in CASKROOMS {
        for cask in std::fs::read_dir(system.join(root)).into_iter().flatten().flatten() {
            let token = cask.file_name().to_string_lossy().into_owned();
            if token.starts_with('.') || !cask.path().is_dir() {
                continue;
            }
            let mut apps = Vec::new();
            for ver in std::fs::read_dir(cask.path()).into_iter().flatten().flatten() {
                if ver.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                for f in std::fs::read_dir(ver.path()).into_iter().flatten().flatten() {
                    let n = f.file_name().to_string_lossy().into_owned();
                    if n.ends_with(".app") && !apps.contains(&n) {
                        apps.push(n);
                    }
                }
            }
            out.push(Cask { token, dir: cask.path(), apps });
        }
    }
    out
}

/// App names Homebrew installed as casks ("Zoom.app").
pub fn cask_apps(casks: &[Cask]) -> Vec<String> {
    casks.iter().flat_map(|c| c.apps.iter().cloned()).collect()
}

/// Where the app came from.
pub fn source(app: &Path, id: &str, casks: &[String]) -> Source {
    let file = app.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if app.join("Contents/_MASReceipt/receipt").exists() {
        Source::AppStore
    } else if app.components().any(|c| c.as_os_str() == "Setapp") || app.join("Contents/Frameworks/Setapp.framework").exists() || id.ends_with("-setapp") {
        Source::Setapp
    } else if casks.contains(&file) {
        Source::Homebrew
    } else if id.starts_with("com.apple.") {
        Source::Apple
    } else {
        Source::Direct
    }
}

/// Apple's apps that are fine to remove even without an App Store receipt.
const APPLE_REMOVABLE: &[&str] = &["com.apple.dt.Xcode", "com.apple.dt.Instruments", "com.apple.iWork.Pages", "com.apple.iWork.Keynote", "com.apple.iWork.Numbers", "com.apple.iMovieApp", "com.apple.garageband10"];

/// Our own bundle ids (release and dev build).
const OWN_ID: &str = "com.romanfurman6.cmdmeow";

/// Why `app` may never be removed, if so.
pub fn locked(app: &Path, id: &str, source: Source) -> Option<&'static str> {
    if id == OWN_ID || id.starts_with(&format!("{OWN_ID}.")) || own_bundle().is_some_and(|b| b == app) {
        return Some("This is Cmd+Meow");
    }
    if source == Source::Apple && !APPLE_REMOVABLE.contains(&id) {
        return Some("Part of macOS");
    }
    if app.starts_with("/System") {
        return Some("Part of macOS");
    }
    None
}

/// The .app we are running from, if any.
fn own_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|a| a.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// When each app was last opened, from Spotlight (one `mdls` for all).
/// Unix seconds; None where Spotlight doesn't know.
#[cfg(target_os = "macos")]
pub fn last_used(apps: &[PathBuf]) -> Vec<Option<i64>> {
    let mut out = Vec::with_capacity(apps.len());
    for chunk in apps.chunks(200) {
        let res = std::process::Command::new("/usr/bin/mdls").args(["-raw", "-name", "kMDItemLastUsedDate"]).args(chunk).output();
        let vals: Vec<Option<i64>> = match res {
            Ok(o) => String::from_utf8_lossy(&o.stdout).split('\0').map(parse_mdls_date).collect(),
            Err(_) => Vec::new(),
        };
        for k in 0..chunk.len() {
            out.push(vals.get(k).copied().flatten());
        }
    }
    out
}

#[cfg(not(target_os = "macos"))]
pub fn last_used(apps: &[PathBuf]) -> Vec<Option<i64>> {
    vec![None; apps.len()]
}

/// "2026-11-02 09:41:07 +0000" → unix seconds; "(null)" → None.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_mdls_date(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z").ok().map(|d| d.timestamp())
}

/// Every bundle id Spotlight knows an app for, wherever it is installed (used
/// so data of an app that lives in ~/Downloads isn't called an orphan).
#[cfg(target_os = "macos")]
pub fn all_installed_ids() -> Vec<String> {
    let Ok(o) = std::process::Command::new("/usr/bin/mdfind").arg("kMDItemContentType == 'com.apple.application-bundle'").output() else { return Vec::new() };
    let paths: Vec<PathBuf> = String::from_utf8_lossy(&o.stdout).lines().map(PathBuf::from).collect();
    let mut out = Vec::new();
    for chunk in paths.chunks(300) {
        let Ok(o) = std::process::Command::new("/usr/bin/mdls").args(["-raw", "-name", "kMDItemCFBundleIdentifier"]).args(chunk).output() else { continue };
        out.extend(String::from_utf8_lossy(&o.stdout).split('\0').map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "(null)"));
    }
    out
}

#[cfg(not(target_os = "macos"))]
pub fn all_installed_ids() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::bundles::Info;

    fn bundle(root: &Path, rel: &str, id: &str, extra: &str) -> PathBuf {
        let app = root.join(rel);
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!("<plist><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleName</key><string>Inner</string><key>CFBundleShortVersionString</key><string>1.2</string>{extra}</dict></plist>"),
        )
        .unwrap();
        app
    }

    #[test]
    fn finds_bundles_one_level_down() {
        let root = std::env::temp_dir().join(format!("cmb-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = bundle(&root, "Docker.app", "com.docker.docker", "");
        let b = bundle(&root, "Utilities/Tool.app", "com.x.tool", "");
        bundle(&root, "Deep/er/No.app", "com.x.no", "");
        std::fs::create_dir_all(root.join(".Hidden.app")).unwrap();
        let found = find_bundles(&[root.clone(), root.join("missing")]);
        assert_eq!(found, vec![a.clone(), b]);
        let info = read_info(&a).unwrap();
        assert_eq!(info, Info { id: "com.docker.docker".into(), name: "Docker".into(), version: "1.2".into(), alt: vec!["Inner".into()] });
        assert!(read_info(&root.join("Deep")).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sources_and_locks() {
        let root = std::env::temp_dir().join(format!("cmb-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mas = bundle(&root, "Pages.app", "com.apple.iWork.Pages", "");
        std::fs::create_dir_all(mas.join("Contents/_MASReceipt")).unwrap();
        std::fs::write(mas.join("Contents/_MASReceipt/receipt"), "r").unwrap();
        let zoom = bundle(&root, "zoom.us.app", "us.zoom.xos", "");
        let setapp = bundle(&root, "Setapp/CleanShot X.app", "pl.maketheweb.cleanshotx-setapp", "");
        let safari = bundle(&root, "Safari.app", "com.apple.Safari", "");
        let casks = vec!["zoom.us.app".to_string()];
        assert_eq!(source(&mas, "com.apple.iWork.Pages", &casks), Source::AppStore);
        assert_eq!(source(&zoom, "us.zoom.xos", &casks), Source::Homebrew);
        assert_eq!(source(&setapp, "pl.maketheweb.cleanshotx-setapp", &casks), Source::Setapp);
        assert_eq!(source(&safari, "com.apple.Safari", &casks), Source::Apple);
        assert_eq!(locked(&safari, "com.apple.Safari", Source::Apple), Some("Part of macOS"));
        assert_eq!(locked(&mas, "com.apple.iWork.Pages", Source::AppStore), None);
        assert_eq!(locked(Path::new("/Applications/Xcode.app"), "com.apple.dt.Xcode", Source::Apple), None);
        assert_eq!(locked(Path::new("/Applications/CmdMeow Dev.app"), "com.romanfurman6.cmdmeow.dev", Source::Direct), Some("This is Cmd+Meow"));
        assert_eq!(locked(&zoom, "us.zoom.xos", Source::Homebrew), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn mdls_dates() {
        assert_eq!(parse_mdls_date("2026-11-02 09:41:07 +0000"), Some(1793612467));
        assert_eq!(parse_mdls_date("(null)"), None);
    }
}
