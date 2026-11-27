//! SP28: on the first open of the full window after an app update, a
//! "What's new" popup lists what this version changed, one row per tool.
//! The tools are the ones [`super::updated::CHANGED`] lists for this
//! version (the SP24 chips), plus rows that aren't tools (Settings). A row
//! click opens that place and clears its chip. Shown once per version;
//! never on a fresh install. Settings › About reopens it.
//! What was shown is kept in `<data>/whats-new.json`.

use super::shell::App;
use crate::kit::glass::*;
use crate::kit::sync::LockExt;
use egui::{Align2, Context, Id, Pos2, Rect, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Not a tool: the row opens Settings › About.
const SETTINGS: &str = "settings";

/// What a version changed: (version, tool id or [`SETTINGS`], its text).
/// Every `CHANGED` row needs one here (a test checks). Append per release.
type Text = fn() -> &'static str;
const NOTES: &[(&str, &str, Text)] = &[
    ("0.2.2", "claude", || ts!("new.v022_claude")),
    ("0.2.2", "cleanup", || ts!("new.v022_cleanup")),
    ("0.2.2", "uninstaller", || ts!("new.v022_uninstaller")),
    ("0.2.2", SETTINGS, || ts!("new.v022_settings")),
];

/// This build's version; screenshots can show another one's list
/// (`CMDMEOW_WHATS_NEW=0.2.2`).
fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| forced().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()))
}

fn notes_for(ver: &str) -> impl Iterator<Item = (&'static str, Text)> + '_ {
    NOTES.iter().filter(move |(v, _, _)| *v == ver).map(|(_, id, k)| (*id, *k))
}

/// Whether this version has a list (Settings › About shows the button).
pub fn has_notes() -> bool {
    notes_for(version()).next().is_some()
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
struct State {
    /// The last version whose list was shown (or skipped on a fresh install).
    shown: String,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// The popup is on screen.
static OPEN: AtomicBool = AtomicBool::new(false);

fn path() -> std::path::PathBuf {
    crate::kit::fs::data_path("whats-new.json")
}

fn save(s: &State) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(path(), b);
    }
}

/// Saved state, or a new one: a fresh install counts as already shown.
fn load(saved: Option<State>, ver: &str, fresh: bool) -> State {
    saved.unwrap_or_else(|| State { shown: if fresh { ver.to_string() } else { String::new() } })
}

fn due(s: &State, ver: &str) -> bool {
    s.shown != ver && notes_for(ver).next().is_some()
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock_ok();
    let s = g.get_or_insert_with(|| {
        let saved = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice::<State>(&b).ok());
        let fresh = !crate::kit::fs::data_path("setup-done").exists();
        let s = load(saved.clone(), version(), fresh);
        if saved.as_ref() != Some(&s) {
            save(&s);
        }
        s
    });
    f(s)
}

/// Setup just finished: a fresh install, nothing is new.
pub fn setup_finished() {
    with(|s| {
        if s.shown != version() {
            s.shown = version().to_string();
            save(s);
        }
    });
}

/// Settings › About: show the list again.
pub fn reopen() {
    OPEN.store(true, Ordering::Relaxed);
}

/// Screenshots: `CMDMEOW_WHATS_NEW=<version>` opens that version's list
/// without touching the saved state.
fn forced() -> Option<String> {
    std::env::var("CMDMEOW_WHATS_NEW").ok().filter(|v| !v.is_empty())
}

fn id() -> Id {
    Id::unique("whats-new")
}

/// Drawn every window frame, after the page: opens the popup once per
/// version, and draws it while open.
pub fn window_frame(app: &mut App, ctx: &Context) {
    if !OPEN.load(Ordering::Relaxed) {
        let first = if forced().is_some() {
            !ctx.data(|d| d.get_temp::<bool>(id().with("forced")).unwrap_or(false))
        } else if app.demo {
            false
        } else {
            with(|s| {
                let d = due(s, version());
                if d {
                    s.shown = version().to_string();
                    save(s);
                }
                d
            })
        };
        if !first {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(id().with("forced"), true));
        OPEN.store(true, Ordering::Relaxed);
    }
    // rows: the tools this version changed that are on, then the rest
    let rows: Vec<(Option<usize>, Text)> = notes_for(version())
        .filter_map(|(tid, k)| {
            if tid == SETTINGS {
                return Some((None, k));
            }
            let i = app.page_tools().into_iter().find(|&i| app.tools[i].id() == tid)?;
            Some((Some(i), k))
        })
        .collect();
    if rows.is_empty() {
        OPEN.store(false, Ordering::Relaxed);
        return;
    }
    const W: f32 = 440.0;
    const EDGE: f32 = 16.0; // P8 text column
    const HEAD: f32 = 52.0;
    let text_x = EDGE + 16.0 + 10.0;
    let text_w = W - text_x - EDGE - 20.0;
    // each row: name line, its changes (wrapped, never cut), 10 above and below
    let galleys: Vec<_> = rows
        .iter()
        .map(|(_, k)| {
            let job = egui::text::LayoutJob::simple(k().to_owned(), font(13.0), TEXT_DIM(), text_w);
            ctx.fonts_mut(|f| f.layout_job(job))
        })
        .collect();
    let row_h: Vec<f32> = galleys.iter().map(|g| 10.0 + 16.0 + 3.0 + g.size().y + 10.0).collect();
    let h = HEAD + row_h.iter().sum::<f32>() + 8.0;
    let mut go: Option<Option<usize>> = None;
    let (x, close) = crate::kit::modal::details(ctx, id(), W, h, |ui, m| {
        let p = ui.painter().clone();
        p.text(Pos2::new(m.left() + EDGE, m.top() + 26.0), Align2::LEFT_CENTER, ts!("new.title"), bold(15.0), TEXT());
        let tw = p.layout_no_wrap(ts!("new.title").to_string(), bold(15.0), TEXT()).size().x;
        p.text(Pos2::new(m.left() + EDGE + tw + 8.0, m.top() + 26.0), Align2::LEFT_CENTER, version(), font(13.0), TEXT_FAINT());
        let xr = Rect::from_min_size(Pos2::new(m.right() - 12.0 - 28.0, m.top() + 12.0), Vec2::splat(28.0));
        let x = icon_button(ui, xr, Icon::Close, ts!("kit.close")).clicked();
        let mut y = m.top() + HEAD;
        for (((tool, k), g), rh) in rows.iter().zip(&galleys).zip(&row_h) {
            let r = Rect::from_min_size(Pos2::new(m.left() + 4.0, y), Vec2::new(W - 8.0, *rh));
            let resp = ui.interact(r, id().with(k()), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, LIT_R, white(ROW_HOVER));
            }
            let (ic, col, name) = match tool {
                Some(i) => {
                    let t = &app.tools[*i];
                    let b = t.badge();
                    (b.icon, b.color, t.name().to_string())
                }
                None => (Icon::Gear, tint::NEUTRAL(), ts!("app.settings").to_string()),
            };
            let top = y + 10.0;
            icon(&p, ic, Rect::from_min_size(Pos2::new(m.left() + EDGE, top), Vec2::splat(16.0)), col);
            p.text(Pos2::new(m.left() + text_x, top + 8.0), Align2::LEFT_CENTER, name, bold(13.0), TEXT());
            p.galley(Pos2::new(m.left() + text_x, top + 16.0 + 3.0), g.clone(), TEXT_DIM());
            icon(&p, Icon::Forward, Rect::from_center_size(Pos2::new(r.right() - 16.0, r.center().y), Vec2::splat(12.0)), TEXT_MUTED());
            if resp.clicked() {
                go = Some(*tool);
            }
            y += rh;
        }
        x
    });
    if let Some(target) = go {
        match target {
            Some(i) => {
                app.page = i;
                app.settings_open = false;
                app.drag = None;
                super::updated::seen(app.tools[i].id());
            }
            None => {
                app.settings_open = true;
                app.settings_tab = super::editor::TAB_ABOUT;
            }
        }
    }
    if x || close || go.is_some() {
        OPEN.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_shows_nothing() {
        assert!(!due(&load(None, "0.2.2", true), "0.2.2"));
    }

    #[test]
    fn an_update_shows_once() {
        let mut s = load(None, "0.2.2", false);
        assert!(due(&s, "0.2.2"));
        s.shown = "0.2.2".into();
        assert!(!due(&s, "0.2.2"));
    }

    #[test]
    fn a_version_without_notes_shows_nothing() {
        assert!(!due(&load(None, "9.9.9", false), "9.9.9"));
    }

    /// Every SP24 chip has its line in the list.
    #[test]
    fn every_changed_tool_has_notes() {
        for (tool, ver) in super::super::updated::CHANGED {
            assert!(NOTES.iter().any(|(v, t, _)| v == ver && t == tool), "{tool}@{ver}");
        }
    }

    #[test]
    fn every_note_has_text() {
        for (v, t, f) in NOTES {
            assert!(!f().is_empty() && !f().starts_with("new."), "{t}@{v}");
        }
        assert_ne!(ts!("new.title"), "new.title");
    }
}
