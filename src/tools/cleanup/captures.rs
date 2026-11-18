//! Old captures: screenshots and recordings this app saved, its history
//! copies, and uploads to the person's storage, older than
//! [`m::CAPTURE_MIN_AGE_DAYS`]. Local files go to the Trash; an upload is
//! deleted from the storage and its link stops working, so it is never
//! picked by default.

use super::engine::{Action, Ctx, Group, Verdict};
use crate::kit::mechanics as m;
use crate::kit::old_captures::{self as old, Kind};
use std::path::{Path, PathBuf};

pub const SCREENSHOTS: &str = "Screenshots";
pub const RECORDINGS: &str = "Recordings";
pub const HISTORY: &str = "Capture History";
pub const UPLOADS: &str = "Uploads";

pub(super) fn scan(c: &mut Ctx) {
    let Some(src) = old::source() else { return };
    for f in (src.local)(m::CAPTURE_MIN_AGE_DAYS) {
        let days = f.age_days as u64;
        let (kind, verdict, reason) = match f.kind {
            Kind::Screenshot => (SCREENSHOTS, Verdict::Ask, format!("Taken {days} days ago")),
            Kind::Recording => (RECORDINGS, Verdict::Ask, format!("Recorded {days} days ago")),
            Kind::History => (HISTORY, Verdict::Safe, format!("History copy from {days} days ago")),
        };
        let title = if f.kind == Kind::History { history_title(&f.path) } else { super::engine::fname(&f.path) };
        c.add(Group::Captures, &title, f.path, kind, verdict, &reason);
    }
    if c.stopped() {
        return;
    }
    for u in (src.uploads)(m::CAPTURE_MIN_AGE_DAYS) {
        let title = super::engine::fname(&u.local);
        let reason = format!("Shared {} days ago; the link stops working", u.age_days as u64);
        c.add_action(Group::Captures, &title, upload_path(&u.link, &u.key), UPLOADS, Verdict::Ask, &reason, Action::Upload { key: u.key, local: u.local }, u.bytes);
    }
}

/// A history copy is named by when it was taken (its file stem, in
/// milliseconds), like the saved copy would be.
fn history_title(p: &Path) -> String {
    use chrono::TimeZone;
    let ms = p.file_stem().and_then(|s| s.to_string_lossy().parse::<i64>().ok()).unwrap_or(0);
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(t) => format!("Capture {}", t.format("%Y-%m-%d at %H.%M.%S")),
        None => super::engine::fname(p),
    }
}

/// An upload's place in the list (and in the kept list): its link, which
/// is also what hovering shows.
fn upload_path(link: &str, key: &str) -> PathBuf {
    PathBuf::from(if link.is_empty() { key } else { link })
}

/// Why `p` must not be removed as an old capture, if so.
pub(super) fn guard(p: &Path) -> Option<&'static str> {
    (!old::source().is_some_and(|s| (s.still_ours)(p))).then_some("Not a capture saved by Cmd+Meow")
}

pub(super) fn delete_upload(local: &Path, key: &str) -> Result<(), String> {
    let s = old::source().ok_or("Capture isn't running")?;
    (s.delete_upload)(local, key)
}

/// The bundle an old capture shows in.
pub(super) fn bundle(kind: &str) -> Option<&'static str> {
    match kind {
        SCREENSHOTS => Some("Screenshots"),
        RECORDINGS => Some("Screen recordings"),
        HISTORY => Some("Capture history"),
        UPLOADS => Some("Uploads"),
        _ => None,
    }
}
