//! SP24: after an app update that noticeably changed a tool, its sidebar
//! row shows "Updated" until the tool's first open. The release names
//! those tools in [`CHANGED`]; small fixes name none. Nothing on a fresh
//! install. What was opened is kept with the version in
//! `<data>/updated.json`, so the next release that changes a tool shows
//! it again.

use crate::kit::sync::LockExt;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Tools this release changed noticeably (ids).
pub const CHANGED: &[&str] = &["claude"];

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
struct State {
    /// The app version the list below belongs to.
    version: String,
    /// Tools opened since this version (or all of them on a fresh install).
    seen: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    crate::kit::fs::data_path("updated.json")
}

/// This version's state: a new version starts with nothing seen; a fresh
/// install (setup never finished) has seen everything.
fn load(saved: Option<State>, version: &str, fresh: bool) -> State {
    match saved {
        Some(s) if s.version == version => s,
        Some(_) => State { version: version.into(), seen: Vec::new() },
        None if fresh => State { version: version.into(), seen: CHANGED.iter().map(|s| s.to_string()).collect() },
        None => State { version: version.into(), seen: Vec::new() },
    }
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock_ok();
    let s = g.get_or_insert_with(|| {
        let saved = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice::<State>(&b).ok());
        let fresh = !crate::kit::fs::data_path("setup-done").exists();
        let s = load(saved.clone(), env!("CARGO_PKG_VERSION"), fresh);
        if saved.as_ref() != Some(&s) {
            save(&s);
        }
        s
    });
    f(s)
}

fn save(s: &State) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(path(), b);
    }
}

/// Setup just finished: this is a fresh install, so every tool this
/// release changed counts as seen. The sidebar can be drawn during setup,
/// before its marker exists, so the fresh check at load isn't enough.
pub fn setup_finished() {
    with(|s| {
        let before = s.seen.len();
        for id in CHANGED {
            if !s.seen.iter().any(|x| x == id) {
                s.seen.push(id.to_string());
            }
        }
        if s.seen.len() != before {
            save(s);
        }
    });
}

/// Whether the sidebar shows "Updated" on tool `id`.
pub fn shows(id: &str) -> bool {
    CHANGED.contains(&id) && with(|s| !s.seen.iter().any(|x| x == id))
}

/// Tool `id` was opened: no chip until the next update that changes it.
pub fn seen(id: &str) {
    if !shows(id) {
        return;
    }
    with(|s| {
        s.seen.push(id.to_string());
        save(s);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_after_an_update_until_opened_never_on_a_fresh_install() {
        let v = "0.3.0";
        // fresh install: nothing shows
        let s = load(None, v, true);
        assert!(CHANGED.iter().all(|id| s.seen.iter().any(|x| x == id)));
        // an update from before this file existed: everything changed shows
        assert!(load(None, v, false).seen.is_empty());
        // the same version keeps what was opened
        let opened = State { version: v.into(), seen: vec!["claude".into()] };
        assert_eq!(load(Some(opened.clone()), v, false), opened);
        // the next version starts over
        assert!(load(Some(opened), "0.3.1", false).seen.is_empty());
    }
}
