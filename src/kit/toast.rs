//! The app's toast, posted from anywhere (a tool, a worker thread): one
//! line at the bottom of the panel and the window. An action that failed for
//! lack of access posts "Needs <what>" with one button that opens the place
//! to allow it (P11), never the system's raw error.

use crate::kit::perms::Perm;
use std::sync::Mutex;

/// Where a toast's button goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// Ask for (or open the Settings pane of) a permission.
    Perm(Perm),
    /// Open this System Settings link.
    Url(&'static str),
}

/// Privacy & Security › Automation.
pub const AUTOMATION: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";
/// Privacy & Security › Files and Folders.
pub const FILES: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

impl Fix {
    pub fn go(&self) {
        match self {
            Fix::Perm(p) => p.ask(),
            Fix::Url(u) => {
                let _ = std::process::Command::new("open").arg(u).spawn();
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Fix::Perm(p) => p.fix_label(),
            Fix::Url(_) => ts!("kit.open_settings"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub msg: String,
    pub fix: Option<Fix>,
}

static NEXT: Mutex<Option<Toast>> = Mutex::new(None);

fn post(t: Toast) {
    if let Ok(mut n) = NEXT.lock() {
        *n = Some(t);
    }
}

/// A plain line.
pub fn say(msg: impl Into<String>) {
    post(Toast { msg: msg.into(), fix: None });
}

/// "Needs <what>" with the button that fixes it.
pub fn needs(what: &str, fix: Fix) {
    post(Toast { msg: t!("app.needs", perm = what).into_owned(), fix: Some(fix) });
}

/// The toast posted since the last call (the shell shows it).
pub fn take() -> Option<Toast> {
    NEXT.lock().ok().and_then(|mut n| n.take())
}

/// The protected folder (Desktop, Documents, Downloads) `path` is in, by
/// the name Finder shows: writing or listing there without the folder's
/// permission fails with "Operation not permitted".
pub fn protected_folder(path: &std::path::Path) -> Option<&'static str> {
    let home = dirs::home_dir()?;
    let rest = path.strip_prefix(&home).ok()?;
    match rest.components().next()?.as_os_str().to_str()? {
        "Desktop" => Some("Desktop"),
        "Documents" => Some("Documents"),
        "Downloads" => Some("Downloads"),
        _ => None,
    }
}

/// An I/O error that means macOS refused access (not a full disk, not a
/// missing folder).
pub fn refused(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(1)
}

/// What a failed read or write at `path` needs, in the words of the fix:
/// "access to Desktop" (the folder's own permission), else Full Disk Access.
pub fn needs_for(path: &std::path::Path) -> (String, Fix) {
    match protected_folder(path) {
        Some(f) => (t!("kit.access_to", folder = f).into_owned(), Fix::Url(FILES)),
        None => (Perm::FullDisk.name().to_string(), Fix::Perm(Perm::FullDisk)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_behind_their_own_permission() {
        let home = dirs::home_dir().expect("home");
        assert_eq!(protected_folder(&home.join("Desktop/Capture.png")), Some("Desktop"));
        assert_eq!(protected_folder(&home.join("Downloads")), Some("Downloads"));
        assert_eq!(protected_folder(&home.join("Pictures/a.png")), None);
        assert_eq!(protected_folder(std::path::Path::new("/tmp/a.png")), None);
    }

    #[test]
    fn only_a_refusal_counts() {
        assert!(refused(&std::io::Error::from_raw_os_error(1)));
        assert!(refused(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        assert!(!refused(&std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert!(!refused(&std::io::Error::from_raw_os_error(28)));
    }

    #[test]
    fn a_toast_is_read_once() {
        say("hi");
        assert_eq!(take().map(|t| t.msg), Some("hi".to_string()));
        assert!(take().is_none());
    }
}
