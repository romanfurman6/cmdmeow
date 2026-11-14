//! Drawing for Dictation: the pill under the caret, the panel widget and
//! the window page (history and settings).

use super::settings::{Engine, SILENCE_CHOICES};
use super::{mac, Dictation, Note, Phase, Tab, ID, TINT};
use crate::core::tool::{Cx, Size};
use crate::kit::glass::*;
use crate::kit::hotkeys::Shortcut;
use crate::kit::perms::{Perm, Status};
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, ViewportBuilder, ViewportId};

const PILL_TITLE: &str = "Cmd+Meow Dictation";
const PILL_H: f32 = 34.0;
const PILL_W: f32 = 132.0;
const ROW_H: f32 = sp::SETTINGS_ROW_H;
/// History cards: inner padding, the meta line, the gap under it, the gap
/// between cards.
const PAD: f32 = 12.0;
const META_H: f32 = 14.0;
const GAP: f32 = 6.0;
const ROW_GAP: f32 = 8.0;
/// The red of a live mic.
const LIVE: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3A);

fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn ago(at: i64) -> String {
    let s = (chrono::Local::now().timestamp() - at).max(0);
    if s < 60 {
        "now".into()
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else if s < 86_400 {
        format!("{} h", s / 3600)
    } else {
        format!("{} d", s / 86_400)
    }
}

fn words(n: usize) -> String {
    format!("{n} word{}", if n == 1 { "" } else { "s" })
}

// ---------------------------------------------------------------- the pill

fn note_text(n: &Note) -> Option<String> {
    match n {
        Note::Pasted => None,
        Note::Copied => Some("Copied · ⌘V to paste".into()),
        Note::Problem(m) => Some(m.clone()),
    }
}

/// The pill under the caret while dictating, and its short notes after.
pub fn pill(d: &mut Dictation, ctx: &egui::Context) {
    // a take in the page's try-it field shows itself there
    if matches!(d.phase, Phase::Idle) || d.trying {
        return;
    }
    let text = match &d.phase {
        Phase::Showing { note, .. } => note_text(note),
        _ => None,
    };
    let w = match &text {
        Some(t) => {
            // about 6.6 pt a character at 12 pt
            (t.chars().count() as f32 * 6.6 + 56.0).clamp(PILL_W, 360.0)
        }
        None => PILL_W,
    };
    let size = Vec2::new(w, PILL_H);
    let (spot, mouse, screen) = d.place;
    let pos = mac::pill_pos(spot, mouse, screen, size);
    let builder = ViewportBuilder::default()
        .with_title(PILL_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false)
        .with_mouse_passthrough(true)
        .with_position(pos)
        .with_inner_size(size);
    let t = ctx.input(|i| i.time);
    let levels = d.levels.clone();
    let phase_secs = match &d.phase {
        Phase::Listening { since, .. } => since.elapsed().as_secs_f32(),
        _ => 0.0,
    };
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(PILL_TITLE), builder, |ui, _| {
        mac::float(PILL_TITLE);
        let r = ui.max_rect().shrink(1.0);
        let p = ui.painter().clone();
        pill_bg(&p, r);
        let cy = r.center().y;
        match &d.phase {
            Phase::Listening { .. } => pill_listening(&p, r, t, phase_secs, &levels),
            Phase::Working { since, expect, done, .. } => {
                let f = super::work_progress(since.elapsed().as_secs_f32(), *expect, done.load(std::sync::atomic::Ordering::Relaxed));
                pill_working(&p, r, t, f, &levels);
            }
            Phase::Showing { note: Note::Pasted, .. } => {
                icon(&p, Icon::Check, Rect::from_center_size(r.center(), Vec2::splat(16.0)), GREEN());
            }
            Phase::Showing { note, .. } => {
                let (glyph, col) = match note {
                    Note::Copied => (Icon::Clipboard, TEXT_SOFT()),
                    _ => (Icon::Mic, AMBER()),
                };
                icon(&p, glyph, Rect::from_center_size(Pos2::new(r.left() + 18.0, cy), Vec2::splat(14.0)), col);
                if let Some(t) = &text {
                    text_fit(&p, Pos2::new(r.left() + 32.0, cy), Align2::LEFT_CENTER, t, font(12.0), TEXT(), r.width() - 44.0);
                }
            }
            Phase::Idle => {}
        }
    });
}

fn pill_bg(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, r.height() / 2.0, Color32::from_rgba_unmultiplied(24, 24, 26, 242));
    p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
}

/// The pill while it listens: a pulsing dot, the voice, the time.
fn pill_listening(p: &egui::Painter, r: Rect, t: f64, secs: f32, levels: &[f32]) {
    let cy = r.center().y;
    let pulse = 0.55 + 0.45 * (0.5 + 0.5 * (t * 5.0).sin()) as f32;
    p.circle_filled(Pos2::new(r.left() + 16.0, cy), 4.5, with_alpha(LIVE, pulse));
    let tr = p.text(Pos2::new(r.right() - 13.0, cy), Align2::RIGHT_CENTER, clock(secs), mono(12.0), TEXT_SOFT());
    bars(p, Rect::from_min_max(Pos2::new(r.left() + 28.0, r.top() + 8.0), Pos2::new(tr.left() - 10.0, r.bottom() - 8.0)), levels, white(225));
}

/// The pill while the model works: like a video render, the take's bars
/// light up left to right.
fn pill_working(p: &egui::Painter, r: Rect, t: f64, f: f32, levels: &[f32]) {
    let cy = r.center().y;
    spinner(p, Pos2::new(r.left() + 17.0, cy), 6.5, t, TEXT_SOFT());
    let tr = p.text(Pos2::new(r.right() - 13.0, cy), Align2::RIGHT_CENTER, format!("{}%", (f * 100.0) as u32), mono(12.0), TEXT_SOFT());
    let br = Rect::from_min_max(Pos2::new(r.left() + 32.0, r.top() + 8.0), Pos2::new(tr.left() - 10.0, r.bottom() - 8.0));
    bars(p, br, levels, white(70));
    bars(&p.with_clip_rect(Rect::from_min_max(Pos2::new(br.left() - 2.0, r.top()), Pos2::new(br.left() + br.width() * f, r.bottom()))), br, levels, white(225));
}

/// The voice as bars, newest on the right, mirrored around the middle.
fn bars(p: &egui::Painter, r: Rect, levels: &[f32], color: Color32) {
    let (bw, gap) = (2.5, 2.0);
    let n = ((r.width() + gap) / (bw + gap)) as usize;
    let start = levels.len().saturating_sub(n);
    let shown = &levels[start..];
    let x0 = r.right() - shown.len() as f32 * (bw + gap) + gap;
    for (k, v) in shown.iter().enumerate() {
        let h = (r.height() * v.clamp(0.0, 1.0)).max(2.0);
        let x = x0 + k as f32 * (bw + gap);
        p.rect_filled(Rect::from_center_size(Pos2::new(x + bw / 2.0, r.center().y), Vec2::new(bw, h)), 1.0, color);
    }
    // the quiet part not heard yet: dots
    let mut x = x0 - gap - bw;
    while x >= r.left() {
        p.circle_filled(Pos2::new(x + bw / 2.0, r.center().y), 1.0, with_alpha(color, 0.35));
        x -= bw + gap;
    }
}

// ---------------------------------------------------------------- widget

pub fn widget(d: &mut Dictation, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let id = ui.scope_id().with("w-dictation");
    let listening = d.listening();
    let key = d.shortcut_label();
    let today = words(d.words_today());
    let resp = match size {
        Size::S11 => {
            if listening {
                let resp = tile::card(ui, rect, id, false);
                let p = ui.painter();
                p.rect_stroke(rect, tile::R_TILE, Stroke::new(1.5, LIVE), StrokeKind::Inside);
                icon(p, Icon::Mic, Rect::from_min_size(rect.min + Vec2::splat(tile::PAD), Vec2::splat(18.0)), LIVE);
                tile::small_text(p, rect, "Listening", &key, TEXT_DIM());
                resp
            } else {
                tile::small(ui, rect, id, Icon::Mic, TINT(), "Dictation", &key, false)
            }
        }
        Size::S14 => {
            let (resp, right) = tile::row_split(ui, rect, id, Icon::Mic, if listening { LIVE } else { TINT() }, "Dictation", &today, &key, false);
            latest(d, ui, right, 2);
            resp
        }
        _ => {
            let resp = tile::card(ui, rect, id, false);
            let p = ui.painter().clone();
            let body = tile::head(&p, rect, Icon::Mic, TINT(), "Dictation", &format!("{key} · {today}"));
            latest(d, ui, body, 4);
            resp
        }
    };
    if resp.on_hover_text("Open Dictation").clicked() {
        cx.open_page(ID, "history");
    }
}

/// The newest dictations in `r`, one per line.
fn latest(d: &Dictation, ui: &mut Ui, r: Rect, n: usize) {
    let p = ui.painter();
    let items: Vec<_> = d.history.items.iter().take(n).collect();
    if items.is_empty() {
        text_fit(p, r.center(), Align2::CENTER_CENTER, &format!("Hold {} and speak", d.shortcut_label()), font(11.0), TEXT_FAINT(), r.width());
        return;
    }
    let lh = (r.height() / n as f32).min(26.0);
    let top = r.center().y - lh * items.len() as f32 / 2.0;
    for (k, s) in items.iter().enumerate() {
        let y = top + lh * (k as f32 + 0.5);
        let ag = p.text(Pos2::new(r.right(), y), Align2::RIGHT_CENTER, ago(s.at), font(11.0), TEXT_FAINT());
        text_fit(p, Pos2::new(r.left(), y), Align2::LEFT_CENTER, &s.text, font(12.0), TEXT_SOFT(), ag.left() - r.left() - 8.0);
    }
}

// ---------------------------------------------------------------- page

pub fn page(d: &mut Dictation, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let (sub, col) = if d.listening() {
        ("Listening".to_string(), LIVE)
    } else if !d.settings.engine.ready() {
        ("Speech model not downloaded".to_string(), AMBER())
    } else {
        (format!("{} · {} · {}", d.shortcut_label(), d.settings.engine.name(), words(d.words_today())), TEXT_DIM())
    };
    let btns = page_header(&p, r, "Dictation", &sub, col);
    let tr = Rect::from_min_max(Pos2::new(btns.right() - 180.0, btns.top()), btns.max);
    let cur = (d.tab == Tab::Settings) as usize;
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("dict-tab"), &["History", "Settings"], cur) {
        d.tab = if i == 1 { Tab::Settings } else { Tab::History };
        d.recording_key = false;
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    match d.tab {
        Tab::History => history(d, ui, body, cx),
        Tab::Settings => settings_page(d, ui, body, cx),
    }
}

fn history(d: &mut Dictation, ui: &mut Ui, body: Rect, cx: &mut Cx) {
    let x0 = body.left() + 24.0;
    let w = body.width() - 48.0;
    if d.history.items.is_empty() || d.try_last.is_some() || d.try_note.is_some() || d.trying || d.try_fake.is_some() {
        try_it(d, ui, body, cx);
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
    let mut copy: Option<String> = None;
    let mut remove: Option<usize> = None;
    ScrollArea::vertical().id_salt("dict-history").auto_shrink([false, false]).show(&mut child, |ui| {
        let p = ui.painter().clone();
        let tw = w - 24.0 - 120.0;
        // each entry is as tall as its text
        // 12 pad · meta 14 · 6 gap · text · 12 pad, then the 8 gap between cards
        let heights: Vec<f32> = d.history.items.iter().map(|s| PAD + META_H + GAP + p.layout(s.text.clone(), font(13.0), TEXT(), tw).size().y.max(18.0) + PAD + ROW_GAP).collect();
        let total: f32 = heights.iter().sum::<f32>() + 40.0;
        let (area, _) = ui.allocate_exact_size(Vec2::new(body.width(), total), Sense::hover());
        let mut y = area.top() + 16.0;
        let id = ui.scope_id().with("dict-rows");
        for (k, s) in d.history.items.iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, heights[k] - ROW_GAP));
            let resp = ui.interact(rr, id.with(k), Sense::click());
            let h = ui.ctx().animate_bool(id.with(("h", k)), resp.hovered());
            p.rect_filled(rr, R_CARD, lerp_color(CARD(), RAISED(), h * 0.5));
            let meta = format!("{} · {} · {:.0} s", ago(s.at), if s.app.is_empty() { "—" } else { &s.app }, s.secs.max(1.0));
            p.text(Pos2::new(rr.left() + PAD, rr.top() + PAD), Align2::LEFT_TOP, meta, font(11.0), TEXT_FAINT());
            paragraph(&p, Pos2::new(rr.left() + PAD, rr.top() + PAD + META_H + GAP), &s.text, font(13.0), TEXT(), tw);
            let cr = Rect::from_min_size(Pos2::new(rr.right() - PAD - 64.0, rr.center().y - 13.0), Vec2::new(64.0, 26.0));
            let xr = Rect::from_center_size(Pos2::new(cr.left() - 20.0, cr.center().y), Vec2::splat(22.0));
            if h > 0.01 && icon_button(ui, xr, Icon::Trash, "Remove").clicked() {
                remove = Some(k);
            }
            if pill_button(ui, cr, "Copy", BTN_SECONDARY, true).clicked() || resp.double_clicked() {
                copy = Some(s.text.clone());
            }
            y += heights[k];
        }
    });
    if let Some(t) = copy {
        ui.ctx().copy_text(t);
        cx.toast("Copied");
    }
    if let Some(k) = remove {
        d.history.items.remove(k);
        d.save_history();
    }
}

// ---------------------------------------------------------------- try it

/// What the try-it field shows (first visit, no history yet).
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    Idle,
    /// `tapped`: the shortcut was tapped (another tap stops), not held.
    Listening { secs: f32, tapped: bool },
    /// The model is turning the take into words.
    Working { frac: f32 },
    /// The take is done: its words in the field.
    Done(super::Said),
    Problem(String),
    /// The speech model isn't here yet; `failed`: the last download's error.
    NeedModel { failed: Option<String> },
    Downloading { frac: f32, done: u64, total: u64, unpacking: bool },
}

fn shown(d: &Dictation) -> Shown {
    use std::sync::atomic::Ordering;
    if let Some(s) = &d.try_fake {
        return s.clone();
    }
    if let Some(dl) = &d.download {
        return Shown::Downloading {
            frac: dl.progress.fraction(),
            done: dl.progress.done.load(Ordering::Relaxed),
            total: dl.progress.total.load(Ordering::Relaxed),
            unpacking: dl.unpacking.load(Ordering::Relaxed),
        };
    }
    if !d.settings.engine.ready() {
        return Shown::NeedModel { failed: d.download_error.clone() };
    }
    if d.trying {
        match &d.phase {
            Phase::Listening { since, tapped, .. } => return Shown::Listening { secs: since.elapsed().as_secs_f32(), tapped: *tapped },
            Phase::Working { since, expect, done, .. } => return Shown::Working { frac: super::work_progress(since.elapsed().as_secs_f32(), *expect, done.load(Ordering::Relaxed)) },
            _ => {}
        }
    }
    if let Some(n) = &d.try_note {
        return Shown::Problem(n.clone());
    }
    if let Some(s) = &d.try_last {
        return Shown::Done(s.clone());
    }
    Shown::Idle
}

/// Bars for sample screenshots, where no mic is listening.
fn sample_levels() -> Vec<f32> {
    (0..64).map(|k| 0.25 + 0.6 * ((k as f32 * 0.55).sin() * (k as f32 * 0.21).cos()).abs()).collect()
}

/// The first visit: just a field, as in any app. Click it, press the
/// shortcut and talk: the pill shows up over the field as it would
/// anywhere, and the words land in the field (not pasted anywhere), so
/// they see dictation work right here. Without a shortcut, a Hold to talk
/// button stands in.
fn try_it(d: &mut Dictation, ui: &mut Ui, body: Rect, cx: &mut Cx) {
    use crate::kit::empty::{hero_button, keycaps, keycaps_width, Lead, HERO_H};
    d.try_seen = Some(std::time::Instant::now());
    // our window has the keys: the shortcut pressed now is meant for this field
    // (a menu-bar app is often not the "frontmost app" even then)
    d.try_focused = ui.input(|i| i.focused);
    let state = shown(d);
    let p = ui.painter().clone();
    let t = ui.ctx().input(|i| i.time);
    let id = ui.scope_id().with("dict-try");
    let key = d.shortcut_label();
    let keys = (!key.is_empty()).then_some(key.as_str());
    let field_w = (body.width() - 2.0 * sp::PAGE_EDGE).min(480.0);
    let (field_h, hint_h) = (112.0, 24.0);
    let button = match &state {
        Shown::NeedModel { .. } | Shown::Downloading { .. } | Shown::Done(_) => true,
        _ => keys.is_none(),
    };
    // the pill's room above the field is kept, so nothing moves when it shows
    let pill_room = PILL_H + 6.0;
    let block = pill_room + field_h + sp::L + hint_h + if button { sp::XL + HERO_H } else { 0.0 };
    let top = body.center().y - block / 2.0 - sp::L;
    let cx0 = body.center().x;

    // the field: a click puts the caret in it
    let fr = Rect::from_min_size(Pos2::new(cx0 - field_w / 2.0, top + pill_room), Vec2::new(field_w, field_h));
    let quiet = matches!(state, Shown::NeedModel { .. } | Shown::Downloading { .. });
    let field = ui.interact(fr, id.with("field"), if quiet { Sense::hover() } else { Sense::click() });
    if field.clicked() {
        d.try_armed = true;
    } else if ui.input(|i| i.pointer.any_click()) && !field.hovered() {
        d.try_armed = false;
    }
    if field.hovered() && !quiet {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    let talking = matches!(state, Shown::Listening { .. } | Shown::Working { .. });
    let armed = d.try_armed || talking;
    let edge = if armed && !quiet { Some(TINT()) } else { None };
    if let Some(e) = edge {
        p.rect_stroke(fr.expand(4.0), R_CARD + 4.0, Stroke::new(4.0, with_alpha(e, 0.08)), StrokeKind::Outside);
    }
    // dimmer until the model is here
    let [cr, cg, cb, ca] = CARD().to_srgba_unmultiplied();
    p.rect_filled(fr, R_CARD, if quiet { Color32::from_rgba_unmultiplied(cr, cg, cb, ca / 2) } else { CARD() });
    let rim = match edge {
        Some(e) => with_alpha(e, 0.5),
        None if field.hovered() && !quiet => white(46),
        None => CARD_EDGE(),
    };
    p.rect_stroke(fr, R_CARD, Stroke::new(1.0, rim), StrokeKind::Inside);
    let inner = fr.shrink(sp::L);
    let line_y = inner.top() + 10.0;
    let caret = |x: f32| {
        // a blinking caret, as in any field that has the keys
        if (t * 1.8).fract() < 0.5 {
            p.rect_filled(Rect::from_min_size(Pos2::new(x, inner.top() + 1.0), Vec2::new(2.0, 18.0)), 1.0, TINT());
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    };
    match &state {
        Shown::Idle | Shown::Listening { .. } | Shown::Working { .. } if armed => {
            caret(inner.left());
            if !talking {
                p.text(Pos2::new(inner.left() + 6.0, line_y), Align2::LEFT_CENTER, "Say something", font(15.0), TEXT_MUTED());
            }
        }
        Shown::Idle | Shown::Listening { .. } | Shown::Working { .. } => {
            p.text(Pos2::new(inner.left(), line_y), Align2::LEFT_CENTER, "Click here", font(15.0), TEXT_MUTED());
        }
        Shown::NeedModel { .. } | Shown::Downloading { .. } => {
            p.text(Pos2::new(inner.left(), line_y), Align2::LEFT_CENTER, "Say something", font(15.0), with_alpha(TEXT(), 0.24));
        }
        Shown::Done(s) => {
            text_wrap(&p, Pos2::new(inner.left(), inner.top()), Align2::LEFT_TOP, &s.text, font(15.0), TEXT(), inner.width(), 4);
        }
        Shown::Problem(m) => {
            text_wrap(&p, inner.center(), Align2::CENTER_CENTER, m, font(14.0), AMBER(), inner.width(), 2);
        }
    }

    // the pill, where it shows in any app: over the field's left edge
    let pr = Rect::from_min_size(Pos2::new(fr.left(), fr.top() - 6.0 - PILL_H), Vec2::new(PILL_W, PILL_H));
    let lv = || if d.demo { sample_levels() } else { d.levels.clone() };
    match &state {
        Shown::Listening { secs, .. } => {
            pill_bg(&p, pr);
            pill_listening(&p, pr, t, *secs, &lv());
        }
        Shown::Working { frac } => {
            pill_bg(&p, pr);
            pill_working(&p, pr, t, *frac, &lv());
        }
        _ => {}
    }

    // one line under the field: what to press now
    let hy = fr.bottom() + sp::L + hint_h / 2.0;
    if let Some(k) = keys {
        // (words before, keys, words after); "Enter or" goes before ⌥D
        let line: Option<(&str, String)> = match &state {
            Shown::Idle | Shown::Problem(_) if armed => Some(("Press", "and talk".into())),
            Shown::Idle | Shown::Problem(_) => Some(("Click the field, then press", String::new())),
            Shown::Listening { tapped: false, .. } => Some(("Release", "to finish".into())),
            Shown::Listening { tapped: true, .. } => Some(("Press Enter or", "to finish".into())),
            Shown::Done(_) => Some(("Press", "to try again".into())),
            _ => None,
        };
        if let Some((before, after)) = line {
            let f = font(13.0);
            let gap = 6.0;
            let bw = p.layout_no_wrap(before.to_string(), f.clone(), TEXT_DIM()).size().x;
            let aw = if after.is_empty() { 0.0 } else { gap + p.layout_no_wrap(after.clone(), f.clone(), TEXT_DIM()).size().x };
            let kw = keycaps_width(&p, k, 12.0);
            let x = cx0 - (bw + gap + kw + aw) / 2.0;
            p.text(Pos2::new(x, hy), Align2::LEFT_CENTER, before, f.clone(), TEXT_DIM());
            let kr = keycaps(&p, Pos2::new(x + bw + gap, hy), k, 12.0, None);
            if !after.is_empty() {
                p.text(Pos2::new(kr + gap, hy), Align2::LEFT_CENTER, after, f, TEXT_DIM());
            }
        }
    }

    // a button only where one is needed
    let bc = Pos2::new(cx0, fr.bottom() + sp::L + hint_h + sp::XL + HERO_H / 2.0);
    match &state {
        Shown::NeedModel { failed } => {
            let label = if failed.is_some() { "Try the download again".to_string() } else { format!("Get speech model · {}", human_bytes(d.settings.engine.bytes())) };
            if hero_button(ui, bc, id.with("get"), &label, None, Lead::Icon(Icon::Down), Some(BLUE()), true, 220.0).clicked() {
                d.download_error = None;
                d.start_download(d.settings.engine);
            }
        }
        Shown::Downloading { frac, done, total, unpacking } => {
            let r = Rect::from_center_size(bc, Vec2::new(260.0, HERO_H));
            p.rect_filled(r, R_BUTTON, with_alpha(BLUE(), 0.14));
            let f = if *unpacking { 1.0 } else { frac.clamp(0.0, 1.0) };
            p.with_clip_rect(Rect::from_min_max(r.min, Pos2::new(r.left() + r.width() * f, r.bottom()))).rect_filled(r, R_BUTTON, with_alpha(BLUE(), 0.30));
            p.rect_stroke(r, R_BUTTON, Stroke::new(1.0, with_alpha(BLUE(), 0.30)), StrokeKind::Inside);
            let what = if *unpacking { "Unpacking…".to_string() } else { format!("{} of {}", human_bytes(*done), human_bytes(*total)) };
            p.text(r.center(), Align2::CENTER_CENTER, what, bold(15.0), ACCENT());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        Shown::Done(s) => {
            if hero_button(ui, bc, id.with("copy"), "Copy", None, Lead::None, None, true, 0.0).clicked() {
                ui.ctx().copy_text(s.text.clone());
                cx.toast("Copied");
            }
        }
        // no shortcut set: hold the button instead
        Shown::Listening { secs, .. } if keys.is_none() => {
            let resp = hero_button(ui, bc, id.with("talk"), &format!("Stop · {}", clock(*secs)), None, Lead::Stop, Some(LIVE), true, 160.0);
            hold(d, ui, &resp);
        }
        Shown::Working { .. } if keys.is_none() => {
            hero_button(ui, bc, id.with("talk"), "Hold to talk", None, Lead::Icon(Icon::Mic), Some(BLUE()), false, 0.0);
        }
        Shown::Idle | Shown::Problem(_) if keys.is_none() => {
            let resp = hero_button(ui, bc, id.with("talk"), "Hold to talk", None, Lead::Icon(Icon::Mic), Some(BLUE()), true, 0.0);
            hold(d, ui, &resp);
        }
        _ => {}
    }
}

/// Hold to talk: down starts a take (into the field), up ends it unless
/// it was a short press, which keeps listening until the next press.
fn hold(d: &mut Dictation, ui: &Ui, resp: &egui::Response) {
    if d.demo {
        return;
    }
    // egui stops calling a long press "down on" the button once it can't
    // be a click any more (under a second), so after the press starts on
    // the button, the hold lasts while the mouse button stays down
    let down = if d.try_down { ui.input(|i| i.pointer.primary_down()) } else { resp.is_pointer_button_down_on() };
    if down && !d.try_down {
        d.press_talk(true);
    } else if !down && d.try_down {
        d.release_talk();
    }
    d.try_down = down;
}

fn card_rows(p: &egui::Painter, x: f32, y: f32, w: f32, rows: usize) -> Rect {
    let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rows as f32 * ROW_H + 8.0));
    glass_card(p, r, R_CARD, 0.0, false);
    for k in 1..rows {
        let ly = r.top() + 4.0 + k as f32 * ROW_H;
        p.line_segment([Pos2::new(r.left() + 12.0, ly), Pos2::new(r.right() - 12.0, ly)], Stroke::new(1.0, white(13)));
    }
    r
}

fn row_y(card: Rect, k: usize) -> f32 {
    card.top() + 4.0 + k as f32 * ROW_H + ROW_H / 2.0
}

fn label(p: &egui::Painter, card: Rect, k: usize, text: &str) {
    p.text(Pos2::new(card.left() + 12.0, row_y(card, k)), Align2::LEFT_CENTER, text, font(13.0), TEXT());
}

fn settings_page(d: &mut Dictation, ui: &mut Ui, body: Rect, cx: &mut Cx) {
    record_key(d, ui, cx);
    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
    ScrollArea::vertical().id_salt("dict-settings").auto_shrink([false, false]).show(&mut child, |ui| {
        let gap = 20.0;
        let colw = (body.width() - 48.0 - gap) / 2.0;
        // the Speaking card, then Words, which grows with the list
        let left_h = 16.0 + 28.0 + 4.0 * ROW_H + 8.0 + 28.0 + 28.0 + words_card_h(ui.painter(), &d.settings.words, colw) + 24.0;
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), left_h.max(520.0)), Sense::hover());
        let p = ui.painter().clone();
        let x0 = area.left() + 24.0;
        let mut changed = left_col(d, ui, &p, Rect::from_min_size(Pos2::new(x0, area.top() + 16.0), Vec2::new(colw, area.height())));
        changed |= right_col(d, ui, &p, Rect::from_min_size(Pos2::new(x0 + colw + gap, area.top() + 16.0), Vec2::new(colw, area.height())));
        if changed {
            d.save_settings();
        }
    });
}

/// While recording: the next key press with a modifier becomes the shortcut.
fn record_key(d: &mut Dictation, ui: &mut Ui, cx: &mut Cx) {
    if !d.recording_key {
        return;
    }
    d.recording_seen = std::time::Instant::now();
    let mut result: Option<Option<Shortcut>> = None;
    let mut cancel = false;
    ui.input(|i| {
        for e in &i.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                let plain = !modifiers.command && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd;
                if *key == egui::Key::Escape && plain {
                    cancel = true;
                } else if (*key == egui::Key::Backspace || *key == egui::Key::Delete) && plain {
                    result = Some(None);
                } else if let Some(s) = Shortcut::from_press(*key, *modifiers) {
                    result = Some(Some(s));
                }
            }
        }
    });
    if cancel {
        d.recording_key = false;
        return;
    }
    if let Some(s) = result {
        if let Some(w) = s.as_ref().and_then(|s| s.conflict()) {
            cx.toast(format!("{}: {w}", s.as_ref().map(|s| s.display()).unwrap_or_default()));
        }
        d.settings.shortcut = s;
        d.save_settings();
        d.recording_key = false;
    }
    ui.ctx().request_repaint();
}

/// Shortcut, model, silence. Returns whether a setting changed.
fn left_col(d: &mut Dictation, ui: &mut Ui, p: &egui::Painter, col: Rect) -> bool {
    let id = ui.scope_id().with("dict-left");
    let mut changed = false;
    let x0 = col.left();
    p.text(Pos2::new(x0, col.top()), Align2::LEFT_TOP, "Speaking", bold(15.0), TEXT());
    let card = card_rows(p, x0, col.top() + 28.0, col.width(), 4);

    // shortcut
    label(p, card, 0, "Shortcut");
    let y = row_y(card, 0);
    let rec = d.recording_key;
    let cur = d.settings.shortcut.clone();
    let text = if rec { "Type a shortcut…".to_string() } else { cur.as_ref().map(|s| s.display()).unwrap_or_else(|| "Record".into()) };
    let g = p.layout_no_wrap(text.clone(), font(12.0), TEXT());
    let bw = (g.size().x + 20.0).max(76.0);
    let br = Rect::from_min_max(Pos2::new(card.right() - 12.0 - bw, y - 12.0), Pos2::new(card.right() - 12.0, y + 12.0));
    let resp = ui.interact(br, id.with("key"), Sense::click());
    if rec {
        p.rect_filled(br, 6.0, with_alpha(BLUE(), 0.22));
        p.rect_stroke(br, 6.0, Stroke::new(1.0, ACCENT()), StrokeKind::Inside);
    } else if cur.is_some() {
        p.rect_filled(br, 6.0, white(if resp.hovered() { 30 } else { 20 }));
        p.rect_stroke(br, 6.0, Stroke::new(1.0, white(26)), StrokeKind::Inside);
    } else {
        dashed_rect(p, br, 6.0, white(60));
    }
    p.text(br.center(), Align2::CENTER_CENTER, text, font(12.0), if cur.is_some() || rec { TEXT() } else { TEXT_FAINT() });
    if resp.on_hover_text("Click, then press the keys. Esc cancels, ⌫ removes it.").clicked() {
        d.recording_key = !rec;
        // counts as drawn now, so the next tick doesn't take it for abandoned
        d.recording_seen = std::time::Instant::now();
    }
    if let Some(e) = &d.key_error {
        text_fit(p, Pos2::new(br.left() - 10.0, y), Align2::RIGHT_CENTER, e, font(11.0), AMBER(), br.left() - card.left() - 110.0);
    }

    // model
    label(p, card, 1, "Model");
    let y = row_y(card, 1);
    let mr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 230.0, y - 13.0), Pos2::new(card.right() - 12.0, y + 13.0));
    let ci = Engine::ALL.iter().position(|e| *e == d.settings.engine).unwrap_or(0);
    let names: Vec<&str> = Engine::ALL.iter().map(|e| e.name()).collect();
    if let Some(i) = segmented(ui, mr, id.with("engine"), &names, ci) {
        d.settings.engine = Engine::ALL[i];
        changed = true;
    }

    // download
    label(p, card, 2, "Speech model");
    let y = row_y(card, 2);
    let engine = d.settings.engine;
    let right = card.right() - 12.0;
    match &d.download {
        Some(dl) if dl.engine == engine => {
            let unpacking = dl.unpacking.load(std::sync::atomic::Ordering::Relaxed);
            let frac = dl.progress.fraction();
            let sr = Rect::from_min_max(Pos2::new(right - 60.0, y - 12.0), Pos2::new(right, y + 12.0));
            if pill_button(ui, sr, "Stop", BTN_SECONDARY, true).clicked() {
                d.stop_download();
            }
            let what = if unpacking { "Unpacking…".to_string() } else { format!("{}%", (frac * 100.0) as u32) };
            let tr = p.text(Pos2::new(sr.left() - 10.0, y), Align2::RIGHT_CENTER, what, font(12.0), TEXT_DIM());
            bar(p, Rect::from_min_max(Pos2::new(tr.left() - 120.0, y - 2.0), Pos2::new(tr.left() - 10.0, y + 2.0)), if unpacking { 1.0 } else { frac }, BLUE());
        }
        _ if engine.ready() => {
            let rr = link(ui, Pos2::new(right, y), Align2::RIGHT_CENTER, "Remove", 12.0);
            p.text(Pos2::new(rr.rect.left() - 12.0, y), Align2::RIGHT_CENTER, "Ready", font(12.0), GREEN());
            let tip = match engine {
                Engine::Parakeet => format!("Delete the model ({})", human_bytes(engine.bytes())),
                Engine::Whisper => format!("Delete the model ({}); Capture's transcripts use it too", human_bytes(engine.bytes())),
            };
            if rr.on_hover_text(tip).clicked() {
                match engine {
                    Engine::Parakeet => crate::kit::parakeet::remove(),
                    Engine::Whisper => crate::kit::whisper::Model::Accurate.remove(),
                }
            }
        }
        _ => {
            let label = format!("Download {}", human_bytes(engine.bytes()));
            let bw = button_width(p, &label, BTN_PRIMARY);
            let dr = Rect::from_min_max(Pos2::new(right - bw, y - 12.0), Pos2::new(right, y + 12.0));
            if pill_button(ui, dr, &label, BTN_PRIMARY, d.download.is_none()).clicked() {
                d.start_download(engine);
            }
            if let Some(e) = &d.download_error {
                text_fit(p, Pos2::new(dr.left() - 10.0, y), Align2::RIGHT_CENTER, e, font(11.0), AMBER(), dr.left() - card.left() - 130.0);
            }
        }
    }

    // hands-free stop
    label(p, card, 3, "Stop after silence");
    let y = row_y(card, 3);
    let sr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 230.0, y - 13.0), Pos2::new(card.right() - 12.0, y + 13.0));
    let si = SILENCE_CHOICES.iter().position(|s| *s == d.settings.silence_secs).unwrap_or(2);
    let names: Vec<String> = SILENCE_CHOICES.iter().map(|s| if *s == 0 { "Never".to_string() } else { format!("{s} s") }).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Some(i) = segmented(ui, sr, id.with("silence"), &names, si) {
        d.settings.silence_secs = SILENCE_CHOICES[i];
        changed = true;
    }

    changed |= words_card(d, ui, p, x0, card.bottom() + 28.0, col.width());
    changed
}

const CHIP_H: f32 = 24.0;
const CHIP_GAP: f32 = 6.0;
/// Room in a chip for its ×.
const CHIP_X: f32 = 20.0;

/// Where each word's chip goes in a `w`-wide area: wrapped into rows.
fn chip_rects(p: &egui::Painter, words: &[String], w: f32) -> Vec<Rect> {
    let (mut x, mut y) = (0.0f32, 0.0f32);
    words
        .iter()
        .map(|t| {
            let cw = (p.layout_no_wrap(t.clone(), font(12.0), TEXT()).size().x + 10.0 + CHIP_X).min(w);
            if x > 0.0 && x + cw > w {
                x = 0.0;
                y += CHIP_H + CHIP_GAP;
            }
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(cw, CHIP_H));
            x += cw + CHIP_GAP;
            r
        })
        .collect()
}

fn words_card_h(p: &egui::Painter, words: &[String], colw: f32) -> f32 {
    let chips = chip_rects(p, words, colw - 24.0);
    ROW_H + 8.0 + chips.last().map(|r| r.bottom() + 12.0).unwrap_or(0.0)
}

/// The person's own words: a field to add one, the list as chips with ×.
/// Returns whether the list changed.
fn words_card(d: &mut Dictation, ui: &mut Ui, p: &egui::Painter, x0: f32, top: f32, w: f32) -> bool {
    let id = ui.scope_id().with("dict-words");
    let mut changed = false;
    let tr = p.text(Pos2::new(x0, top), Align2::LEFT_TOP, "Words", bold(15.0), TEXT());
    // what it's for, beside the title
    text_fit(p, Pos2::new(tr.right() + 10.0, tr.center().y + 1.0), Align2::LEFT_CENTER, "Names and terms to spell your way: \"клод\" or \"Clode\" becomes \"Claude\"", font(12.0), TEXT_FAINT(), x0 + w - tr.right() - 10.0);
    let card = Rect::from_min_size(Pos2::new(x0, top + 28.0), Vec2::new(w, words_card_h(p, &d.settings.words, w)));
    glass_card(p, card, R_CARD, 0.0, false);

    let y = card.top() + 4.0 + ROW_H / 2.0;
    let ar = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 56.0, y - 12.0), Pos2::new(card.right() - 12.0, y + 12.0));
    let fr = Rect::from_min_max(Pos2::new(card.left() + 12.0, y - sp::CONTROL_H / 2.0), Pos2::new(ar.left() - 8.0, y + sp::CONTROL_H / 2.0));
    let resp = text_input(ui, fr, id.with("field"), &mut d.word_draft, "Name, brand or term", FieldStyle::default());
    let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let can_add = !d.word_draft.trim().is_empty();
    if (pill_button(ui, ar, "Add", BTN_SECONDARY, can_add).clicked() || entered) && d.settings.add_word(&d.word_draft) {
        d.word_draft.clear();
        changed = true;
        if entered {
            resp.request_focus();
        }
    }

    let origin = Pos2::new(card.left() + 12.0, card.top() + 4.0 + ROW_H + 4.0);
    let mut remove = None;
    for (k, (word, r)) in d.settings.words.iter().zip(chip_rects(p, &d.settings.words, w - 24.0)).enumerate() {
        let r = r.translate(origin.to_vec2());
        let xr = Rect::from_center_size(Pos2::new(r.right() - CHIP_X / 2.0 - 2.0, r.center().y), Vec2::splat(16.0));
        let x = ui.interact(xr, id.with(("x", k)), Sense::click());
        p.rect_filled(r, CHIP_H / 2.0, white(if x.hovered() { 26 } else { 18 }));
        text_fit(p, Pos2::new(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, word, font(12.0), TEXT(), r.width() - 10.0 - CHIP_X);
        p.text(xr.center(), Align2::CENTER_CENTER, "×", font(13.0), if x.hovered() { TEXT() } else { TEXT_FAINT() });
        if x.on_hover_text("Remove").clicked() {
            remove = Some(k);
        }
    }
    if let Some(k) = remove {
        d.settings.words.remove(k);
        changed = true;
    }
    changed
}

/// Clipboard, history, permissions. Returns whether a setting changed.
fn right_col(d: &mut Dictation, ui: &mut Ui, p: &egui::Painter, col: Rect) -> bool {
    let id = ui.scope_id().with("dict-right");
    let mut changed = false;
    let x0 = col.left();
    p.text(Pos2::new(x0, col.top()), Align2::LEFT_TOP, "After", bold(15.0), TEXT());
    let card = card_rows(p, x0, col.top() + 28.0, col.width(), 2);
    let rows: [(&str, &mut bool); 2] = [("Put the clipboard back", &mut d.settings.restore_clipboard), ("Keep history", &mut d.settings.keep_history)];
    for (k, (l, on)) in rows.into_iter().enumerate() {
        label(p, card, k, l);
        let y = row_y(card, k);
        let sr = Rect::from_min_size(Pos2::new(card.right() - 12.0 - 40.0, y - 12.0), Vec2::new(40.0, 24.0));
        if switch(ui, sr, id.with(("sw", k)), *on, true).clicked() {
            *on = !*on;
            changed = true;
        }
    }
    if !d.history.items.is_empty() {
        let y = row_y(card, 1);
        let lr = link(ui, Pos2::new(card.right() - 12.0 - 40.0 - 14.0, y), Align2::RIGHT_CENTER, "Clear", 12.0);
        if lr.clicked() {
            d.history.items.clear();
            d.save_history();
        }
    }

    let y0 = card.bottom() + 28.0;
    p.text(Pos2::new(x0, y0), Align2::LEFT_TOP, "Permissions", bold(15.0), TEXT());
    let card = card_rows(p, x0, y0 + 28.0, col.width(), 2);
    for (k, perm) in [Perm::Microphone, Perm::Accessibility].into_iter().enumerate() {
        let y = row_y(card, k);
        icon(p, perm.icon(), Rect::from_center_size(Pos2::new(card.left() + 21.0, y), Vec2::splat(15.0)), TEXT_DIM());
        p.text(Pos2::new(card.left() + 38.0, y), Align2::LEFT_CENTER, perm.name(), font(13.0), TEXT());
        let right = card.right() - 12.0;
        if perm.status() == Status::Allowed {
            p.text(Pos2::new(right, y), Align2::RIGHT_CENTER, "Allowed", font(12.0), GREEN());
        } else {
            let label = perm.ask_label();
            let bw = button_width(p, label, BTN_PRIMARY);
            let br = Rect::from_min_max(Pos2::new(right - bw, y - 12.0), Pos2::new(right, y + 12.0));
            if pill_button(ui, br, label, BTN_PRIMARY, true).clicked() {
                perm.ask();
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
    changed
}
