//! Data of deleted apps, between the tool that finds it (Uninstaller's
//! Leftovers) and the one that offers it for review (Cleanup), which never
//! use each other directly: Uninstaller registers how to find it, Cleanup asks.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Per deleted app, its name and its files in the home folder, when nothing
/// in them changed for the given seconds.
pub type Find = fn(i64) -> Vec<(String, Vec<PathBuf>)>;

static FIND: OnceLock<Find> = OnceLock::new();

pub fn register(f: Find) {
    let _ = FIND.set(f);
}

/// Nothing when no tool registered (Uninstaller not loaded, demo).
pub fn find(min_age_secs: i64) -> Vec<(String, Vec<PathBuf>)> {
    FIND.get().map(|f| f(min_age_secs)).unwrap_or_default()
}
