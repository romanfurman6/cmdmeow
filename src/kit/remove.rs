//! "Remove Cmd+Meow": everything the app put on this Mac, then the app
//! itself. Files the person saved (screenshots and recordings in their own
//! folder) stay. The dev build removes only its own data.

use std::path::PathBuf;

/// The bundle id of this build (the dev build has its own).
pub fn bundle_id() -> &'static str {
    if super::fs::is_dev() { "com.romanfurman6.cmdmeow.dev" } else { "com.romanfurman6.cmdmeow" }
}

/// Folders and files of this build's own data under `home`.
pub fn data_paths(home: &std::path::Path) -> Vec<PathBuf> {
    let id = bundle_id();
    let lib = home.join("Library");
    let mut v = vec![
        super::fs::data_path(""),
        // window state and widget layout (eframe keeps them by app id)
        lib.join("Application Support").join(id),
        lib.join("Caches").join(id),
        lib.join("HTTPStorages").join(id),
        lib.join("Preferences").join(format!("{id}.plist")),
        lib.join("Saved Application State").join(format!("{id}.savedState")),
    ];
    let logs = lib.join("Logs/CmdMeow");
    if super::fs::is_dev() {
        v.extend(["dev.log", "crash-dev.log"].map(|f| logs.join(f)));
    } else {
        v.push(logs);
    }
    v
}

/// Where a removal started from Settings is.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Idle,
    Working,
    Failed(String),
}

static RUN: std::sync::Mutex<Run> = std::sync::Mutex::new(Run::Idle);

pub fn run_state() -> Run {
    RUN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Start [`everything`] on its own thread (the admin prompt blocks). When
/// it's done the process exits right there: a normal quit would save window
/// state and settings again, into the folders just removed.
pub fn start(ctx: egui::Context) {
    {
        let mut r = RUN.lock().unwrap_or_else(|e| e.into_inner());
        if *r == Run::Working {
            return;
        }
        *r = Run::Working;
    }
    std::thread::spawn(move || {
        match everything() {
            Ok(()) => std::process::exit(0),
            Err(e) => *RUN.lock().unwrap_or_else(|e| e.into_inner()) = Run::Failed(e),
        }
        ctx.request_repaint();
    });
}

/// Remove everything, step by step. Stops at the first step the person
/// refused (the admin prompt for the fan helper), before anything is gone.
/// On success the app is in the Trash and should quit.
pub fn everything() -> Result<(), String> {
    // the only step that can be refused goes first
    if std::path::Path::new(super::helper::PLIST).exists() && !super::fs::is_dev() {
        super::helper::uninstall().map_err(|e| format!("the fan helper stayed: {e}"))?;
    }
    let _ = super::login::set(false);
    imp::forget_keys();
    imp::reset_permissions(bundle_id());
    let home = super::fs::home();
    for p in data_paths(&home) {
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    imp::trash_app();
    Ok(())
}

#[cfg(target_os = "macos")]
mod imp {
    /// Keychain services this build saved items under.
    fn services() -> [&'static str; 2] {
        if crate::kit::fs::is_dev() { ["Cmd+Meow Dev history", "Cmd+Meow Dev sharing"] } else { ["Cmd+Meow history", "Cmd+Meow sharing"] }
    }

    pub fn forget_keys() {
        use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
        for service in services() {
            let found = ItemSearchOptions::new().class(ItemClass::generic_password()).service(service).load_attributes(true).limit(Limit::All).search().unwrap_or_default();
            for r in found {
                let Some(account) = r.simplify_dict().and_then(|d| d.get("acct").cloned()) else { continue };
                let _ = security_framework::passwords::delete_generic_password(service, &account);
            }
        }
    }

    /// Every privacy permission macOS gave this app, back to "not asked".
    pub fn reset_permissions(bundle_id: &str) {
        let _ = std::process::Command::new("/usr/bin/tccutil").args(["reset", "All", bundle_id]).status();
    }

    /// The running app's bundle into the Trash (it keeps running until it quits).
    pub fn trash_app() {
        let Some(app) = crate::kit::perms::app_bundle() else { return };
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&app.to_string_lossy()));
        let fm = NSFileManager::defaultManager();
        if let Err(e) = fm.trashItemAtURL_resultingItemURL_error(&url, None) {
            eprintln!("[remove] couldn't move {} to the Trash: {e:?}", app.display());
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn forget_keys() {}
    pub fn reset_permissions(_bundle_id: &str) {}
    pub fn trash_app() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_its_own_things() {
        let home = std::path::Path::new("/Users/someone");
        let v = data_paths(home);
        assert!(v.contains(&super::super::fs::data_path("")));
        assert!(v.iter().any(|p| p.ends_with("Library/Preferences/com.romanfurman6.cmdmeow.plist")));
        // nothing the person made, nothing above the app's own folders
        for p in &v {
            assert!(p.starts_with(home.join("Library")) || p.starts_with(super::super::fs::data_path("")), "{}", p.display());
            assert!(!p.ends_with("Library") && !p.ends_with("Application Support") && !p.ends_with("Desktop"), "{}", p.display());
        }
    }
}
