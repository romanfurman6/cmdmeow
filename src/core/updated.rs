//! SP24: after an app update that noticeably changed a tool, its sidebar
//! row shows "Updated" until the tool's first open. Each release that
//! changes a tool adds `(tool, version)` to [`CHANGED`]; small fixes add
//! nothing. Nothing on a fresh install. What was opened is kept in
//! `<data>/updated.json` as `tool@version`, so a later release that
//! changes the same tool again shows the chip again, and old entries can
//! stay in the list without coming back.

use crate::kit::sync::LockExt;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Tools a release changed noticeably: (tool id, version that ships it).
/// Append one row per tool for each release; never edit shipped rows.
pub const CHANGED: &[(&str, &str)] = &[
    ("claude", "0.2.2"),
    ("cleanup", "0.2.2"),
    ("uninstaller", "0.2.2"),
];

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
struct State {
    /// `tool@version` entries already seen (opened, or present at a fresh install).
    seen: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn key(id: &str, ver: &str) -> String {
    format!("{id}@{ver}")
}

fn path() -> std::path::PathBuf {
    crate::kit::fs::data_path("updated.json")
}

/// Marks every entry in `changed` seen; returns whether anything was added.
fn see_all(s: &mut State, changed: &[(&str, &str)], only: Option<&str>) -> bool {
    let mut added = false;
    for (id, ver) in changed {
        if only.is_some_and(|o| o != *id) {
            continue;
        }
        let k = key(id, ver);
        if !s.seen.contains(&k) {
            s.seen.push(k);
            added = true;
        }
    }
    added
}

/// Saved state, or a new one: a fresh install (setup never finished) has
/// seen everything listed today.
fn load(saved: Option<State>, changed: &[(&str, &str)], fresh: bool) -> State {
    match saved {
        Some(s) => s,
        None => {
            let mut s = State::default();
            if fresh {
                see_all(&mut s, changed, None);
            }
            s
        }
    }
}

fn shows_in(s: &State, changed: &[(&str, &str)], id: &str) -> bool {
    changed.iter().any(|(t, v)| *t == id && !s.seen.contains(&key(t, v)))
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock_ok();
    let s = g.get_or_insert_with(|| {
        let saved = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice::<State>(&b).ok());
        let fresh = !crate::kit::fs::data_path("setup-done").exists();
        let s = load(saved.clone(), CHANGED, fresh);
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

/// Setup just finished: this is a fresh install, so every listed change
/// counts as seen. The sidebar can be drawn during setup, before its
/// marker exists, so the fresh check at load isn't enough.
pub fn setup_finished() {
    with(|s| {
        if see_all(s, CHANGED, None) {
            save(s);
        }
    });
}

/// Whether the sidebar shows "Updated" on tool `id`.
pub fn shows(id: &str) -> bool {
    CHANGED.iter().any(|(t, _)| *t == id) && with(|s| shows_in(s, CHANGED, id))
}

/// Tool `id` was opened: no chip until a later release changes it again.
pub fn seen(id: &str) {
    if !shows(id) {
        return;
    }
    with(|s| {
        if see_all(s, CHANGED, Some(id)) {
            save(s);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &[(&str, &str)] = &[("claude", "0.2.2"), ("cleanup", "0.2.2")];
    const V2: &[(&str, &str)] = &[("claude", "0.2.2"), ("cleanup", "0.2.2"), ("claude", "0.3.0")];

    #[test]
    fn fresh_install_shows_nothing() {
        let s = load(None, V1, true);
        assert!(!shows_in(&s, V1, "claude") && !shows_in(&s, V1, "cleanup"));
    }

    #[test]
    fn update_shows_until_opened() {
        let mut s = load(None, V1, false);
        assert!(shows_in(&s, V1, "claude") && shows_in(&s, V1, "cleanup"));
        see_all(&mut s, V1, Some("claude"));
        assert!(!shows_in(&s, V1, "claude"));
        assert!(shows_in(&s, V1, "cleanup"));
        assert!(!shows_in(&s, V1, "lens"));
    }

    #[test]
    fn a_later_release_shows_a_tool_again_but_not_old_rows() {
        let mut s = load(None, V1, false);
        see_all(&mut s, V1, None);
        // next release lists claude again; cleanup's old row stays seen
        assert!(shows_in(&s, V2, "claude"));
        assert!(!shows_in(&s, V2, "cleanup"));
        see_all(&mut s, V2, Some("claude"));
        assert!(!shows_in(&s, V2, "claude"));
    }

    #[test]
    fn old_file_format_still_loads() {
        let s: State = serde_json::from_str(r#"{"version":"0.2.1","seen":["claude"]}"#).unwrap();
        assert!(shows_in(&s, V1, "claude"));
    }
}
