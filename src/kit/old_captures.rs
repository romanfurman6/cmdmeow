//! Old captures, between the tool that makes them (Capture) and the one that
//! cleans them up (Cleanup), which never use each other directly: Capture
//! registers how to find and remove its files, Cleanup asks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Screenshot,
    Recording,
    /// A copy kept for Capture's History tab.
    History,
}

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub path: PathBuf,
    pub kind: Kind,
    pub age_days: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    /// The file that was shared (may be gone here).
    pub local: PathBuf,
    pub key: String,
    pub link: String,
    pub age_days: f64,
    pub bytes: u64,
}

pub struct Source {
    /// Screenshots, recordings and history copies older than so many days.
    pub local: fn(f64) -> Vec<File>,
    /// Uploads older than so many days.
    pub uploads: fn(f64) -> Vec<Upload>,
    /// Delete an upload (local file, key) from the storage.
    pub delete_upload: fn(&Path, &str) -> Result<(), String>,
    /// Why this file may not be removed now (not one the capturing tool
    /// made, pinned since), if so.
    pub keep_why: fn(&Path) -> Option<&'static str>,
}

static SOURCE: OnceLock<Source> = OnceLock::new();
static HISTORY_CHANGED: AtomicBool = AtomicBool::new(false);

pub fn register(s: Source) {
    let _ = SOURCE.set(s);
}

pub fn source() -> Option<&'static Source> {
    SOURCE.get()
}

/// History copies were removed: the capturing tool reads its history again.
pub fn history_changed() {
    HISTORY_CHANGED.store(true, Ordering::SeqCst);
}

pub fn take_history_changed() -> bool {
    HISTORY_CHANGED.swap(false, Ordering::SeqCst)
}
