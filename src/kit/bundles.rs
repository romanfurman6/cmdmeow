//! Installed app bundles: where apps live and what their Info.plist says.
//! Shared by the tools that list apps (Uninstaller, Clipboard).

use crate::kit::plist;
use std::path::{Path, PathBuf};

/// What a bundle's Info.plist says about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Other names it goes by: Info.plist name and display name, when they
    /// differ from the file name.
    pub alt: Vec<String>,
}

/// Read an app bundle's Info.plist. The display name falls back to the
/// bundle's file name, which is what Finder shows.
pub fn read_info(app: &Path) -> Option<Info> {
    let v = plist::read(&app.join("Contents/Info.plist"))?;
    let id = v.get_str("CFBundleIdentifier")?.trim().to_string();
    if id.is_empty() {
        return None;
    }
    let file_name = app.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = if file_name.is_empty() { v.get_str("CFBundleDisplayName").or(v.get_str("CFBundleName")).unwrap_or("").to_string() } else { file_name };
    let version = v.get_str("CFBundleShortVersionString").or(v.get_str("CFBundleVersion")).unwrap_or("").trim().to_string();
    let mut alt: Vec<String> = Vec::new();
    for k in ["CFBundleDisplayName", "CFBundleName"] {
        if let Some(n) = v.get_str(k).map(|s| s.trim().to_string()) {
            if !n.is_empty() && !n.eq_ignore_ascii_case(&name) && !alt.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
                alt.push(n);
            }
        }
    }
    Some(Info { id, name, version, alt })
}

/// The folders apps are installed in.
pub fn app_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/Applications"), crate::kit::fs::home().join("Applications")]
}

/// Every `.app` in `roots`, and one folder level down ("/Applications/Utilities",
/// "/Applications/Setapp", vendor folders). Sorted, no duplicates.
pub fn find_bundles(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if name.ends_with(".app") {
                out.push(p);
            } else if p.is_dir() && !p.is_symlink() {
                for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                    let n = f.file_name().to_string_lossy().into_owned();
                    if n.ends_with(".app") && !n.starts_with('.') {
                        out.push(f.path());
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
