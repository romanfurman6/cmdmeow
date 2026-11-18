//! Old captures for Cleanup: screenshots and recordings this app saved,
//! its history copies, and uploads to the person's storage. Only files the
//! app made, found by their names ("Capture 2026-11-08 at 17.41.12.png",
//! "Recording … .mp4" and their " 2", " edited", " framed" copies).

use super::{settings, share, store::Store};
use crate::kit::old_captures::{File, Kind, Upload};
use std::path::{Path, PathBuf};

/// Let Cleanup find and remove old captures.
pub(super) fn register() {
    crate::kit::old_captures::register(crate::kit::old_captures::Source { local, uploads, delete_upload, still_ours });
}

fn history_dir() -> PathBuf {
    settings::data_dir().join("history")
}

/// Screenshots, recordings and history copies older than `min_days`.
/// Pinned captures stay. The save folder is read only when macOS won't ask
/// about it (a capture was saved there, or Full Disk Access is on).
fn local(min_days: f64) -> Vec<File> {
    let s = settings::load(&settings::data_dir());
    let st = Store::open(history_dir());
    let pinned = pinned_stamps(&st);
    let mut out = Vec::new();
    let saved_there = st.shots.iter().any(|x| x.saved.as_deref().is_some_and(|p| p.starts_with(&s.folder)));
    if saved_there || !crate::kit::fs::behind_folder_prompt(&s.folder) {
        let rd = std::fs::read_dir(&s.folder).map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default();
        for p in rd {
            let Some(kind) = ours(&p) else { continue };
            if stamp(&p).is_some_and(|t| pinned.contains(&t)) || !p.symlink_metadata().is_ok_and(|m| m.is_file()) {
                continue;
            }
            let age = age_days(&p);
            if age >= min_days {
                out.push(File { path: p, kind, age_days: age });
            }
        }
    }
    let now = chrono::Local::now();
    for x in st.shots.iter().filter(|x| !x.pinned) {
        let age = (now - x.time()).num_seconds() as f64 / 86_400.0;
        if age >= min_days {
            out.push(File { path: st.path(x.id), kind: Kind::History, age_days: age });
        }
    }
    out
}

/// Uploads older than `min_days`, when sharing is set up. Sizes come from
/// what was recorded at upload, the file here, or the storage itself.
fn uploads(min_days: f64) -> Vec<Upload> {
    let s = settings::load(&settings::data_dir());
    if !s.share.set_up(share::VIDEO_ACCOUNT) {
        return Vec::new();
    }
    let now = chrono::Utc::now().timestamp();
    let mut out: Vec<Upload> = share::all()
        .into_iter()
        .filter(|(_, sh)| sh.at > 0 && (now - sh.at) as f64 / 86_400.0 >= min_days)
        .map(|(local, sh)| {
            let bytes = if sh.bytes > 0 { sh.bytes } else { std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0) };
            Upload { local, key: sh.key, link: sh.link, age_days: (now - sh.at) as f64 / 86_400.0, bytes }
        })
        .collect();
    // sizes nobody knows: ask the storage (a few at a time)
    if out.iter().any(|u| u.bytes == 0) {
        let b = s.share.bucket(share::VIDEO_ACCOUNT);
        if b.ready() {
            std::thread::scope(|sc| {
                for chunk in out.chunks_mut(8) {
                    let b = &b;
                    sc.spawn(move || {
                        for u in chunk.iter_mut().filter(|u| u.bytes == 0) {
                            // gone from the storage already (or no answer): nothing to free
                            if let Ok(Some(n)) = share::s3::size(b, &u.key) {
                                u.bytes = n;
                            }
                        }
                    });
                }
            });
        }
    }
    out.retain(|u| u.bytes > 0);
    out
}

/// Remove an upload from the storage, then forget it here.
fn delete_upload(local: &Path, key: &str) -> Result<(), String> {
    let s = settings::load(&settings::data_dir());
    let b = s.share.bucket(share::VIDEO_ACCOUNT);
    if !b.ready() {
        return Err("Sharing isn't set up".into());
    }
    share::s3::delete(&b, key)?;
    share::forget(local);
    Ok(())
}

/// Is `p` a file this app made that Cleanup may remove? A capture or
/// recording in the save folder, or a history copy.
fn still_ours(p: &Path) -> bool {
    if !p.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return false;
    }
    // pinned since the scan: stays
    let st = Store::open(history_dir());
    if p.parent() == Some(history_dir().as_path()) {
        let id = p.file_stem().and_then(|s| s.to_string_lossy().parse::<u64>().ok());
        return p.extension().is_some_and(|e| e == "png") && id.is_some_and(|id| !st.shots.iter().any(|x| x.id == id && x.pinned));
    }
    let folder = settings::load(&settings::data_dir()).folder;
    p.parent() == Some(folder.as_path()) && stamp(p).is_some_and(|t| !pinned_stamps(&st).contains(&t))
}

/// The name a capture was saved under, without the " 2", " edited",
/// " framed" its copies add: "Capture 2026-11-08 at 17.41.12".
fn stamp(p: &Path) -> Option<String> {
    let kind = ours(p)?;
    let prefix = if kind == Kind::Screenshot { "Capture " } else { "Recording " };
    p.file_name()?.to_string_lossy().get(..prefix.len() + 22).map(str::to_string)
}

/// Stamps of pinned captures: their copies stay too.
fn pinned_stamps(st: &Store) -> Vec<String> {
    st.shots.iter().filter(|x| x.pinned).filter_map(|x| x.saved.as_deref()).filter_map(stamp).collect()
}

/// What a file in the save folder is, by its name, if this app made it.
fn ours(p: &Path) -> Option<Kind> {
    let name = p.file_name()?.to_string_lossy().to_string();
    let ext = p.extension()?.to_string_lossy().to_lowercase();
    let (kind, rest) = name.strip_prefix("Capture ").map(|r| (Kind::Screenshot, r)).or_else(|| name.strip_prefix("Recording ").map(|r| (Kind::Recording, r)))?;
    let ok_ext = match kind {
        Kind::Screenshot => matches!(ext.as_str(), "png" | "jpg" | "jpeg"),
        _ => ext == "mp4",
    };
    // "2026-11-08 at 17.41.12", then only what the app adds: " 2", " edited", " framed"
    let date = rest.get(..22)?;
    let parsed = chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d at %H.%M.%S").is_ok();
    let tail = rest.get(22..)?.rsplit_once('.').map(|(t, _)| t)?;
    let app_tail = tail.split(' ').skip(1).all(|w| w == "edited" || w == "framed" || (!w.is_empty() && w.chars().all(|c| c.is_ascii_digit())));
    (ok_ext && parsed && (tail.is_empty() || tail.starts_with(' ')) && app_tail).then_some(kind)
}

fn age_days(p: &Path) -> f64 {
    p.symlink_metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs_f64() / 86_400.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_names_this_app_gives() {
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12.png")), Some(Kind::Screenshot));
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12 2.jpg")), Some(Kind::Screenshot));
        assert_eq!(ours(Path::new("/d/Recording 2026-11-09 at 17.26.12.mp4")), Some(Kind::Recording));
        assert_eq!(ours(Path::new("/d/Recording 2026-11-09 at 17.26.12 framed.mp4")), Some(Kind::Recording));
        // macOS's own, the person's files, wrong kinds
        assert_eq!(ours(Path::new("/d/Screenshot 2026-11-08 at 17.41.12.png")), None);
        assert_eq!(ours(Path::new("/d/Capture of my notes.png")), None);
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12.mp4")), None);
        assert_eq!(ours(Path::new("/d/Recording 2026-11-09 at 17.26.12.png")), None);
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12.png.zip")), None);
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12 my notes.png")), None);
        assert_eq!(ours(Path::new("/d/Capture 2026-11-08 at 17.41.12x.png")), None);
        assert_eq!(ours(Path::new("/d/Recording 2026-11-09 at 17.26.12 edited 2.mp4")), Some(Kind::Recording));
    }

    #[test]
    fn copies_share_the_original_stamp() {
        let t = stamp(Path::new("/d/Capture 2026-11-08 at 17.41.12.png"));
        assert_eq!(t.as_deref(), Some("Capture 2026-11-08 at 17.41.12"));
        for copy in ["/d/Capture 2026-11-08 at 17.41.12 2.png", "/d/Capture 2026-11-08 at 17.41.12 edited.png"] {
            assert_eq!(stamp(Path::new(copy)), t, "{copy}");
        }
        assert_eq!(stamp(Path::new("/d/Recording 2026-11-09 at 17.26.12 framed.mp4")).as_deref(), Some("Recording 2026-11-09 at 17.26.12"));
    }
}
