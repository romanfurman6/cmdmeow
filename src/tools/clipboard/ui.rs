//! Clipboard in the shell: the quick-action tile and the "Recent clipboard"
//! card in the panel, the History and Settings page in the full window, and
//! the small toast shown outside the shell.

use super::settings::{Ignored, AGES, KEEP_MAX, KEEP_MIN};
use super::store::{parse_color, Entry, Kind};
use super::{Clipboard, Filter, Tab, ID, TINT};
use crate::core::tool::{Cx, Size};
use crate::kit::tile;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, ViewportBuilder, ViewportId};
use std::collections::HashMap;
use std::time::Instant;

/// Most recent clips listed in the Large widget.
const PANEL_ROWS: usize = 8;
/// A settings card row.
const ROW_H: f32 = sp::SETTINGS_ROW_H;
/// A History row, and the step from one row to the next.
const LIST_ROW_H: f32 = sp::ROW_H;
const LIST_STEP: f32 = LIST_ROW_H + sp::ROW_GAP;

// ---------------------------------------------------------------- shared bits

/// "now", "5 min", "2 h", "Nov 2".
pub fn ago(e: &Entry) -> String {
    let now = chrono::Local::now();
    let t = e.time();
    let mins = (now - t).num_minutes();
    if mins < 1 {
        "now".into()
    } else if mins < 60 {
        format!("{mins} min")
    } else if mins < 24 * 60 {
        format!("{} h", mins / 60)
    } else {
        t.format("%b %-d").to_string()
    }
}

/// A small mark for an app (we draw no app icons): a rounded square in a
/// color picked from its name.
pub fn app_color(app: &str) -> Color32 {
    // greens from the theme ramp and one neutral (no rainbow)
    let h = super::store::hash(&[app.as_bytes()]);
    match h % 5 {
        4 => PURPLE(),
        k => junk_ramp(2 + k as usize),
    }
}

/// The kind's glyph, drawn with strokes in `r`.
pub fn kind_glyph(p: &egui::Painter, kind: Kind, r: Rect, col: Color32, swatch: Option<Color32>) {
    let s = Stroke::new(1.5, col);
    let c = r.center();
    let u = r.width() / 16.0;
    let pt = |x: f32, y: f32| Pos2::new(c.x + x * u, c.y + y * u);
    match kind {
        Kind::Text => {
            for (k, w) in [(-4.0, 6.0), (0.0, 6.0), (4.0, 3.5)] {
                p.line_segment([pt(-6.0, k), pt(-6.0 + 2.0 * w, k)], s);
            }
        }
        Kind::Code => {
            p.line_segment([pt(-3.0, -5.0), pt(-7.0, 0.0)], s);
            p.line_segment([pt(-7.0, 0.0), pt(-3.0, 5.0)], s);
            p.line_segment([pt(3.0, -5.0), pt(7.0, 0.0)], s);
            p.line_segment([pt(7.0, 0.0), pt(3.0, 5.0)], s);
            p.line_segment([pt(1.5, -6.0), pt(-1.5, 6.0)], s);
        }
        Kind::Link => {
            let a = Rect::from_center_size(pt(-2.5, 2.5), Vec2::new(9.0 * u, 5.5 * u));
            let b = Rect::from_center_size(pt(2.5, -2.5), Vec2::new(9.0 * u, 5.5 * u));
            p.rect_stroke(a, 2.75 * u, s, StrokeKind::Middle);
            p.rect_stroke(b, 2.75 * u, s, StrokeKind::Middle);
        }
        Kind::Image => {
            let f = Rect::from_center_size(c, Vec2::new(14.0 * u, 11.0 * u));
            p.rect_stroke(f, 2.0 * u, s, StrokeKind::Middle);
            p.line_segment([pt(-7.0, 4.0), pt(-2.0, -1.0)], s);
            p.line_segment([pt(-2.0, -1.0), pt(2.0, 3.0)], s);
            p.line_segment([pt(2.0, 3.0), pt(4.0, 1.0)], s);
            p.line_segment([pt(4.0, 1.0), pt(7.0, 4.0)], s);
            p.circle_filled(pt(3.0, -2.5), 1.2 * u, col);
        }
        Kind::Color => {
            let sw = swatch.unwrap_or(col);
            p.circle_filled(c, 6.0 * u, sw);
            crate::kit::glass::stroke_circle(&p, c, 6.0 * u, Stroke::new(1.0, white(60)));
        }
        Kind::File => {
            let pts = vec![pt(-5.0, -7.0), pt(2.0, -7.0), pt(5.5, -3.5), pt(5.5, 7.0), pt(-5.0, 7.0), pt(-5.0, -7.0)];
            p.add(egui::Shape::line(pts, s));
            p.line_segment([pt(2.0, -7.0), pt(2.0, -3.5)], s);
            p.line_segment([pt(2.0, -3.5), pt(5.5, -3.5)], s);
        }
    }
}

/// The entry's small square: its image, its color, or its kind's glyph.
pub fn entry_thumb(c: &mut Clipboard, p: &egui::Painter, r: Rect, e: &Entry, radius: f32) {
    match e.kind {
        Kind::Image => match c.thumb(e.id) {
            Some(t) => {
                cover_image(p, r, t, (e.w, e.h), radius);
                p.rect_stroke(r, radius, Stroke::new(1.0, white(30)), StrokeKind::Inside);
            }
            None => {
                p.rect_filled(r, radius, with_alpha(Kind::Image.tint(), 0.35));
                kind_glyph(p, Kind::Image, r.shrink(r.width() * 0.2), TEXT_SOFT(), None);
            }
        },
        Kind::Color => {
            let col = parse_color(&e.text).unwrap_or(GRAY());
            p.rect_filled(r, radius, col);
            p.rect_stroke(r, radius, Stroke::new(1.0, white(46)), StrokeKind::Inside);
        }
        k => {
            p.rect_filled(r, radius, white(18));
            kind_glyph(p, k, r.shrink(r.width() * 0.2), TEXT_DIM(), None);
        }
    }
}

/// An image filling `r`, cropped to keep its shape.
pub fn cover_image(p: &egui::Painter, r: Rect, tex: egui::TextureId, size: (u32, u32), radius: f32) {
    let (iw, ih) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let (ra, ia) = (r.width() / r.height().max(1.0), iw / ih);
    let uv = if ia > ra {
        let w = ra / ia;
        Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ia / ra;
        Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
    };
    let mut mesh = egui::epaint::RectShape::filled(r, radius, Color32::WHITE);
    mesh.brush = Some(std::sync::Arc::new(egui::epaint::Brush { fill_texture_id: tex, uv }));
    p.add(mesh);
}

fn row_hover(ui: &Ui, r: Rect, id: egui::Id) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

// ---------------------------------------------------------------- panel

/// The Clipboard widget at `size`: the small ones show the latest clip and
/// open the history; Medium and Large are the card with recent clips.
pub fn widget(c: &mut Clipboard, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    if matches!(size, Size::S24 | Size::S44) {
        return card(c, ui, rect, cx);
    }
    let id = ui.scope_id().with("w-clipboard");
    let latest = c.visible(Filter::All, "").first().and_then(|i| c.store.get(*i).cloned());
    let status = c.pause_label().map(|_| "Paused".to_string()).unwrap_or_else(|| "History".into());
    let (title, sub) = match &latest {
        Some(e) => (e.title(), format!("{} · {}", if e.app.is_empty() { "Unknown app" } else { &e.app }, ago(e))),
        None => ("Nothing copied yet".to_string(), status.clone()),
    };
    let resp = match size {
        Size::S11 => tile::small(ui, rect, id, Icon::Clipboard, TINT(), "Clipboard", &status, false),
        Size::S12 => tile::wide(ui, rect, id, Icon::Clipboard, TINT(), &status, &title, Err(&sub), false),
        _ => {
            // left: the count and status; right: the two latest clips
            let n = c.store.entries.len();
            let count = format!("{n} item{}", if n == 1 { "" } else { "s" });
            let (resp, right) = tile::row_split(ui, rect, id, Icon::Clipboard, TINT(), "Clipboard", &count, &status, false);
            let p = ui.painter().clone();
            let recent: Vec<Entry> = c.visible(Filter::All, "").into_iter().take(2).filter_map(|i| c.store.get(i).cloned()).collect();
            if recent.is_empty() {
                text_fit(&p, right.center(), Align2::CENTER_CENTER, "Nothing copied yet", font(11.0), TEXT_FAINT(), right.width());
            }
            let lh = right.height() / 2.0;
            for (k, e) in recent.iter().enumerate() {
                let lr = Rect::from_min_size(Pos2::new(right.left(), right.top() + k as f32 * lh), Vec2::new(right.width(), lh));
                let th = Rect::from_min_size(Pos2::new(lr.left(), lr.center().y - 10.0), Vec2::splat(20.0));
                entry_thumb(c, &p, th, e, 5.0);
                let ag = p.text(Pos2::new(lr.right(), lr.center().y), Align2::RIGHT_CENTER, ago(e), font(11.0), TEXT_FAINT());
                // two lines when the clip is long: one line cut it to a few words
                text_wrap(&p, Pos2::new(th.right() + 8.0, lr.center().y), Align2::LEFT_CENTER, &e.title(), font(12.0), TEXT_SOFT(), ag.left() - th.right() - 14.0, 2);
            }
            resp
        }
    };
    if resp.on_hover_text("Open the history").clicked() {
        c.tab = Tab::History;
        cx.open_page(ID, "history");
    }
}

/// The panel card: pause, a search row that opens the history, and the
/// latest entries (click to copy).
pub fn card(c: &mut Clipboard, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let id = ui.scope_id().with("clipboard-card");
    tile::paint_card(&p, rect, 0.0, false);
    let x0 = rect.left() + 12.0;
    let x1 = rect.right() - 12.0;
    let mut y = rect.top() + 10.0;
    tool_chip(&p, Rect::from_min_size(Pos2::new(x0, y), Vec2::splat(20.0)), Icon::Clipboard, TINT());
    p.text(Pos2::new(x0 + 28.0, y + 10.0), Align2::LEFT_CENTER, "Clipboard", bold(13.0), TEXT());
    let hl = link(ui, Pos2::new(x1, y + 10.0), Align2::RIGHT_CENTER, "History", 12.0);
    if hl.clicked() {
        c.tab = Tab::History;
        cx.open_page(ID, "history");
    }
    // pause / resume
    let paused = c.paused();
    let label = if paused { "Resume" } else { "Pause 15 min" };
    let g = p.layout_no_wrap(label.to_string(), font(11.0), TEXT());
    let pr = Rect::from_min_size(Pos2::new(hl.rect.left() - 10.0 - g.size().x - 26.0, y), Vec2::new(g.size().x + 26.0, 20.0));
    let presp = row_hover(ui, pr, id.with("pause"));
    p.rect_filled(pr, 6.0, if paused { with_alpha(AMBER(), 0.22) } else { white(if presp.hovered() { 30 } else { 18 }) });
    let bar_c = if paused { AMBER() } else { TEXT_SOFT() };
    if paused {
        // play triangle
        let cc = Pos2::new(pr.left() + 11.0, pr.center().y);
        p.add(egui::Shape::convex_polygon(vec![cc + Vec2::new(-3.0, -4.0), cc + Vec2::new(4.0, 0.0), cc + Vec2::new(-3.0, 4.0)], bar_c, Stroke::NONE));
    } else {
        for dx in [-2.0, 2.0] {
            p.line_segment([Pos2::new(pr.left() + 11.0 + dx, pr.center().y - 4.0), Pos2::new(pr.left() + 11.0 + dx, pr.center().y + 4.0)], Stroke::new(1.6, bar_c));
        }
    }
    p.text(Pos2::new(pr.left() + 19.0, pr.center().y), Align2::LEFT_CENTER, label, font(11.0), if paused { AMBER() } else { TEXT_SOFT() });
    if presp.on_hover_text(if paused { "Start recording again" } else { "Stop recording for 15 minutes" }).clicked() {
        c.toggle_pause(15);
    }
    y += 30.0;

    // search row (Large only; Medium spends the height on clips): opens the history
    if rect.height() > 250.0 {
        // search row: opens the history
        let sr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 28.0));
        let sresp = row_hover(ui, sr, id.with("search"));
        p.rect_filled(sr, 7.0, white(if sresp.hovered() { 24 } else { 15 }));
        icon(&p, Icon::Search, Rect::from_center_size(Pos2::new(sr.left() + 16.0, sr.center().y), Vec2::splat(13.0)), TEXT_FAINT());
        let n = c.store.entries.len();
        let hint = match c.pause_label() {
            Some(l) => l,
            None => format!("Search {n} item{}", if n == 1 { "" } else { "s" }),
        };
        p.text(Pos2::new(sr.left() + 30.0, sr.center().y), Align2::LEFT_CENTER, hint, font(12.0), if paused { AMBER() } else { TEXT_FAINT() });
        if sresp.on_hover_text("Search the history").clicked() {
            c.tab = Tab::History;
            cx.open_page(ID, "history");
        }
        y += 28.0 + 6.0;
    }

    // as many clips as fit; rows shrink a little rather than leave a gap
    let avail = rect.bottom() - 6.0 - y;
    let fit = (avail / 28.0).floor().max(1.0) as usize;
    let rh = (avail / fit as f32).min(32.0);
    let ids: Vec<u64> = c.visible(Filter::All, "").into_iter().take(fit.min(PANEL_ROWS)).collect();
    if ids.is_empty() {
        let r = Rect::from_min_max(Pos2::new(x0, y + 4.0), Pos2::new(x1, rect.bottom() - 12.0));
        dashed_rect(&p, r, 8.0, white(40));
        let msg = if paused { "Recording is paused" } else { "Copy something and it shows up here" };
        text_fit(&p, r.center(), Align2::CENTER_CENTER, msg, font(12.0), TEXT_FAINT(), r.width() - 16.0);
        return;
    }
    for eid in &ids {
        let Some(e) = c.store.get(*eid).cloned() else { continue };
        let r = Rect::from_min_size(Pos2::new(x0 - 6.0, y), Vec2::new(x1 - x0 + 12.0, rh));
        let resp = row_hover(ui, r, id.with(("row", e.id)));
        if resp.hovered() {
            p.rect_filled(r, 7.0, white(15));
        }
        // one line per clip: thumb, text, then when (and a pin) on the right
        let th = Rect::from_min_size(Pos2::new(x0, r.center().y - 11.0), Vec2::splat(22.0));
        entry_thumb(c, &p, th, &e, 5.0);
        let mut right = p.text(Pos2::new(x1, r.center().y), Align2::RIGHT_CENTER, ago(&e), font(11.0), TEXT_FAINT()).left() - 8.0;
        if e.pinned {
            pin_glyph(&p, Pos2::new(right - 5.0, r.center().y + 2.0), TINT());
            right -= 16.0;
        }
        let tx = th.right() + 9.0;
        let title_font = if e.kind == Kind::Code { mono(11.5) } else { font(12.0) };
        text_fit(&p, Pos2::new(tx, r.center().y), Align2::LEFT_CENTER, &e.title(), title_font, TEXT(), right - tx - 4.0);
        if resp.on_hover_text("Click to copy").clicked() && c.copy(e.id, false) {
            cx.toast("Copied");
        }
        y += rh;
    }
}

/// A small push pin.
pub fn pin_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.5, col);
    let pts = vec![c + Vec2::new(-3.0, -6.0), c + Vec2::new(3.0, -6.0), c + Vec2::new(2.0, -1.0), c + Vec2::new(5.0, 2.0), c + Vec2::new(-5.0, 2.0), c + Vec2::new(-2.0, -1.0), c + Vec2::new(-3.0, -6.0)];
    p.add(egui::Shape::line(pts, s));
    p.line_segment([c + Vec2::new(0.0, 2.0), c + Vec2::new(0.0, 7.0)], s);
}

// ---------------------------------------------------------------- page

pub fn page(c: &mut Clipboard, ui: &mut Ui, r: Rect, _cx: &mut Cx) {
    let p = ui.painter().clone();
    let n = c.store.entries.len();
    let pinned = c.store.entries.iter().filter(|e| e.pinned).count();
    let (sub, sub_col) = match (c.tab, c.pause_label()) {
        (Tab::Settings, _) => ("Stays on this Mac".to_string(), TEXT_DIM()),
        (_, Some(l)) => (format!("{l} · {n} items"), AMBER()),
        // the normal state needs no word; only Paused is said
        _ => (format!("{n} item{} · {pinned} pinned · {}", if n == 1 { "" } else { "s" }, human_bytes(c.store.bytes_on_disk())), TEXT_DIM()),
    };
    let btns = page_header(&p, r, "Clipboard", &sub, sub_col);
    let mut right = btns.right();
    let label = if c.paused() { "Resume" } else { "Pause 15 min" };
    let w = button_width(&p, label, BTN_SECONDARY);
    let br = Rect::from_min_max(Pos2::new(right - w, btns.top() - 1.0), Pos2::new(right, btns.bottom() + 1.0));
    if pill_button(ui, br, label, BTN_SECONDARY, true).clicked() {
        c.toggle_pause(15);
    }
    right = br.left() - 12.0;
    let tr = Rect::from_min_max(Pos2::new(right - 180.0, btns.top()), Pos2::new(right, btns.bottom()));
    let cur = (c.tab == Tab::Settings) as usize;
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("clip-tab"), &["History", "Settings"], cur) {
        c.tab = if i == 1 { Tab::Settings } else { Tab::History };
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    match c.tab {
        Tab::History => history(c, ui, body),
        Tab::Settings => settings_page(c, ui, body),
    }
}

const FILTERS: [(&str, Filter); 8] = [
    ("All", Filter::All),
    ("Text", Filter::Kind(Kind::Text)),
    ("Code", Filter::Kind(Kind::Code)),
    ("Links", Filter::Kind(Kind::Link)),
    ("Images", Filter::Kind(Kind::Image)),
    ("Colors", Filter::Kind(Kind::Color)),
    ("Files", Filter::Kind(Kind::File)),
    ("Pinned", Filter::Pinned),
];

/// Filter chips in a row from `x`; returns the right edge.
pub fn chips(ui: &mut Ui, p: &egui::Painter, x: f32, y: f32, cur: &mut Filter, salt: &str) -> f32 {
    let mut x = x;
    for (i, (label, f)) in FILTERS.iter().enumerate() {
        let g = p.layout_no_wrap(label.to_string(), font(12.0), TEXT());
        let dot = matches!(f, Filter::Kind(_));
        let w = g.size().x + 22.0 + if dot { 12.0 } else { 0.0 };
        let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, 26.0));
        let resp = ui.interact(r, ui.scope_id().with((salt, i)), Sense::click());
        let on = *cur == *f;
        p.rect_filled(r, 13.0, if on { with_alpha(BLUE(), 0.22) } else if resp.hovered() { white(10) } else { Color32::TRANSPARENT });
        p.rect_stroke(r, 13.0, Stroke::new(1.0, if on { with_alpha(ACCENT(), 0.45) } else { white(26) }), StrokeKind::Inside);
        let mut tx = r.left() + 11.0;
        if let Filter::Kind(k) = f {
            p.circle_filled(Pos2::new(tx + 3.5, r.center().y), 3.5, k.tint());
            tx += 12.0;
        }
        p.text(Pos2::new(tx, r.center().y), Align2::LEFT_CENTER, *label, font(12.0), if on { TEXT() } else { TEXT_DIM() });
        if resp.clicked() {
            *cur = *f;
        }
        x = r.right() + 6.0;
    }
    x
}

fn day_label(d: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    if d == today {
        "Today".into()
    } else if Some(d) == today.pred_opt() {
        "Yesterday".into()
    } else {
        d.format("%A, %b %-d").to_string()
    }
}

fn history(c: &mut Clipboard, ui: &mut Ui, body: Rect) {
    let p = ui.painter().clone();
    let x0 = body.left() + 24.0;
    let x1 = body.right() - 24.0;
    let fy = body.top() + 12.0;
    let sr = Rect::from_min_size(Pos2::new(x0, fy - 1.0), Vec2::new(220.0, 28.0));
    let mut q = std::mem::take(&mut c.search);
    search_field(ui, sr, &mut q, "Search", false);
    c.search = q;
    let mut f = c.filter;
    chips(ui, &p, sr.right() + 12.0, fy, &mut f, "clip-filter");
    c.filter = f;

    // clear, with a second click to confirm
    let confirming = c.confirm_clear.is_some_and(|t| t.elapsed().as_secs_f32() < 3.0);
    let label = if confirming { "Click again to clear" } else { "Clear unpinned" };
    let cw = button_width(&p, label, BTN_DANGER) - 10.0;
    let cr = Rect::from_min_max(Pos2::new(x1 - cw, fy), Pos2::new(x1, fy + 26.0));
    let unpinned = c.store.entries.iter().any(|e| !e.pinned);
    if pill_button(ui, cr, label, if confirming { BTN_DANGER } else { BTN_SECONDARY }, unpinned).clicked() {
        if confirming {
            c.store.clear_unpinned();
            c.forget_dropped_thumbs();
            c.confirm_clear = None;
            c.selected = None;
        } else {
            c.confirm_clear = Some(Instant::now());
        }
    }
    if confirming {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }

    let area = Rect::from_min_max(Pos2::new(x0, fy + 40.0), Pos2::new(x1, body.bottom() - 20.0));
    let detail_w = 320.0;
    let list = Rect::from_min_max(area.min, Pos2::new(area.right() - detail_w - 16.0, area.bottom()));
    let aside = Rect::from_min_max(Pos2::new(list.right() + 16.0, area.top()), area.max);
    let ids = c.visible(c.filter, &c.search);
    if c.selected.is_none_or(|s| !ids.contains(&s)) {
        c.selected = ids.first().copied();
    }
    glass_card(&p, list, R_CARD, 0.0, false);
    if ids.is_empty() {
        let msg = match (c.store.entries.is_empty(), c.store.saves()) {
            (false, _) => "Nothing matches",
            (true, true) => "Nothing copied yet",
            // the Keychain gave no key: this run's history isn't written down
            (true, false) => "Keychain locked · history not saved",
        };
        p.text(list.center(), Align2::CENTER_CENTER, msg, bold(15.0), TEXT_SOFT());
    } else {
        list_rows(c, ui, list, &ids);
    }
    glass_card(&p, aside, R_CARD, 0.0, false);
    if let Some(e) = c.selected.and_then(|id| c.store.get(id).cloned()) {
        detail(c, ui, aside, &e);
    }
}

/// A line of the history list.
pub(super) enum Row {
    Head(String),
    /// An entry, its one-line title and the title's search matches (char
    /// ranges), worked out once per change.
    Item(u64, String, Vec<(usize, usize)>),
}

/// The list's rows for the visible ids: Pinned, then one group per day.
/// Rebuilt only when the history, filter, search or day changes, not on
/// every frame of a scroll.
fn grouped_rows(c: &Clipboard, ids: &[u64]) -> std::sync::Arc<Vec<Row>> {
    let today = chrono::Local::now().date_naive();
    let key = (c.store.rev(), c.filter, c.search.clone(), today);
    if let Some((k, rows)) = c.rows_memo.borrow().as_ref() {
        if *k == key {
            return rows.clone();
        }
    }
    let by_id: HashMap<u64, &Entry> = c.store.entries.iter().map(|e| (e.id, e)).collect();
    let mut rows = Vec::new();
    let mut last: Option<String> = None;
    let q = super::search::Query::new(&c.search);
    for id in ids {
        let Some(e) = by_id.get(id) else { continue };
        let head = if e.pinned { "Pinned".to_string() } else { day_label(e.time().date_naive()) };
        if last.as_ref() != Some(&head) {
            rows.push(Row::Head(head.clone()));
            last = Some(head);
        }
        let title = e.title();
        let marks = if q.is_empty() { Vec::new() } else { super::search::marks(&title, &q) };
        rows.push(Row::Item(*id, title, marks));
    }
    let rows = std::sync::Arc::new(rows);
    *c.rows_memo.borrow_mut() = Some((key, rows.clone()));
    rows
}

/// Rows grouped as Pinned, then by day.
fn list_rows(c: &mut Clipboard, ui: &mut Ui, list: Rect, ids: &[u64]) {
    let rows = grouped_rows(c, ids);
    let h: f32 = rows.iter().map(|r| if matches!(r, Row::Head(_)) { 28.0 } else { LIST_STEP }).sum::<f32>() + 8.0;
    let mut child = ui.new_child(UiBuilder::new().max_rect(list.shrink2(Vec2::new(0.0, 4.0))));
    ScrollArea::vertical().id_salt("clip-list").auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        let p = ui.painter().clone();
        let mut y = area.top();
        for row in rows.iter() {
            match row {
                Row::Head(t) => {
                    p.text(Pos2::new(area.left() + 14.0, y + 18.0), Align2::LEFT_CENTER, t, bold(11.0), TEXT_FAINT());
                    y += 28.0;
                }
                Row::Item(id, title, marks) => {
                    let r = Rect::from_min_size(Pos2::new(area.left() + 4.0, y), Vec2::new(area.width() - 8.0, LIST_ROW_H));
                    // up to a thousand rows: only those in view are drawn
                    if !ui.is_rect_visible(r) {
                        y += LIST_STEP;
                        continue;
                    }
                    let Some(e) = c.store.get(*id).map(Entry::light) else {
                        y += LIST_STEP;
                        continue;
                    };
                    let resp = ui.interact(r, ui.scope_id().with(("clip-row", e.id)), Sense::click());
                    let sel = c.selected == Some(e.id);
                    if sel {
                        p.rect_filled(r, 7.0, with_alpha(BLUE(), 0.30));
                        p.rect_stroke(r, 7.0, Stroke::new(1.0, with_alpha(ACCENT(), 0.55)), StrokeKind::Inside);
                    } else if resp.hovered() {
                        p.rect_filled(r, 7.0, white(10));
                    }
                    let th = Rect::from_min_size(Pos2::new(r.left() + 10.0, r.center().y - 12.0), Vec2::splat(24.0));
                    entry_thumb(c, &p, th, &e, 6.0);
                    let tr = p.text(Pos2::new(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, e.time().format("%-I:%M %P").to_string(), font(12.0), TEXT_FAINT());
                    let ax = tr.left() - 110.0;
                    p.rect_filled(Rect::from_center_size(Pos2::new(ax + 6.0, r.center().y), Vec2::splat(12.0)), 3.0, app_color(&e.app));
                    text_fit(&p, Pos2::new(ax + 18.0, r.center().y), Align2::LEFT_CENTER, &e.app, font(12.0), TEXT_DIM(), 86.0);
                    let mut right = ax - 10.0;
                    if e.pinned {
                        pin_glyph(&p, Pos2::new(right - 6.0, r.center().y), TINT());
                        right -= 18.0;
                    }
                    let tx = th.right() + 10.0;
                    let f = if e.kind == Kind::Code { mono(12.0) } else { font(13.0) };
                    if marks.is_empty() {
                        text_fit(&p, Pos2::new(tx, r.center().y), Align2::LEFT_CENTER, title, f, TEXT(), right - tx);
                    } else {
                        marked_fit(&p, Pos2::new(tx, r.center().y), title, marks, f, right - tx);
                    }
                    if resp.clicked() {
                        c.selected = Some(e.id);
                    }
                    if resp.double_clicked() && c.copy(e.id, false) {
                        c.hud("Copied");
                    }
                    y += LIST_STEP;
                }
            }
        }
    });
}

/// One line of text with its search matches highlighted, cut with "…" to `max_w`.
fn marked_fit(p: &egui::Painter, pos: Pos2, text: &str, marks: &[(usize, usize)], f: egui::FontId, max_w: f32) {
    let chars: Vec<char> = text.chars().collect();
    let job = |n: usize| {
        let mut job = egui::text::LayoutJob::default();
        let plain = egui::TextFormat { font_id: f.clone(), color: TEXT(), ..Default::default() };
        let hit = egui::TextFormat { font_id: f.clone(), color: TEXT(), background: with_alpha(TINT(), 0.35), ..Default::default() };
        let mut i = 0;
        for &(a, b) in marks {
            let (a, b) = (a.min(n), b.min(n));
            if a > i {
                job.append(&chars[i..a].iter().collect::<String>(), 0.0, plain.clone());
            }
            if b > a {
                job.append(&chars[a..b].iter().collect::<String>(), 0.0, hit.clone());
            }
            i = i.max(b);
        }
        if n > i {
            job.append(&chars[i..n].iter().collect::<String>(), 0.0, plain.clone());
        }
        if n < chars.len() {
            job.append("…", 0.0, plain);
        }
        p.layout_job(job)
    };
    // same cap and binary search as text_fit: never lay out a whole long clip
    let cap = ((max_w.max(0.0) / (f.size * 0.25).max(1.0)) as usize).saturating_add(2).min(chars.len());
    let mut g = job(cap);
    if g.size().x > max_w {
        let (mut lo, mut hi) = (1usize, cap);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if job(mid).size().x <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        g = job(lo);
    }
    let rect = Align2::LEFT_CENTER.anchor_size(pos, g.size());
    p.galley(rect.min, g, TEXT());
}

/// The selected entry: a big preview, facts and actions.
fn detail(c: &mut Clipboard, ui: &mut Ui, r: Rect, e: &Entry) {
    let p = ui.painter().clone();
    let x0 = r.left() + 14.0;
    let x1 = r.right() - 14.0;
    let mut y = r.top() + 14.0;
    p.circle_filled(Pos2::new(x0 + 4.0, y + 8.0), 4.0, e.kind.tint());
    p.text(Pos2::new(x0 + 14.0, y + 8.0), Align2::LEFT_CENTER, e.kind.label(), bold(13.0), TEXT());
    p.text(Pos2::new(x1, y + 8.0), Align2::RIGHT_CENTER, e.time().format("%b %-d, %-I:%M %P").to_string(), font(12.0), TEXT_FAINT());
    y += 26.0;
    let pv = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 200.0));
    preview(c, ui, &p, pv, e, 10.0);
    y = pv.bottom() + 14.0;
    let facts = [
        ("From", if e.app.is_empty() { "Unknown app".to_string() } else { e.app.clone() }),
        ("Copied", if e.count > 1 { format!("{} times", e.count) } else { "once".into() }),
        ("Content", e.summary()),
    ];
    for (k, v) in facts {
        p.text(Pos2::new(x0, y + 8.0), Align2::LEFT_CENTER, k, font(12.0), TEXT_FAINT());
        text_fit(&p, Pos2::new(x0 + 70.0, y + 8.0), Align2::LEFT_CENTER, &v, font(12.0), TEXT(), x1 - x0 - 70.0);
        y += 22.0;
    }
    // actions, bottom up
    let bw = (x1 - x0 - 6.0) / 2.0;
    let del = Rect::from_min_max(Pos2::new(x0, r.bottom() - 40.0), Pos2::new(x1, r.bottom() - 12.0));
    let dresp = ui.interact(del, ui.scope_id().with(("del", e.id)), Sense::click());
    p.text(del.center(), Align2::CENTER_CENTER, "Delete from history", font(12.0), if dresp.hovered() { RED() } else { RED() });
    if dresp.clicked() {
        c.store.remove(e.id);
        c.forget_dropped_thumbs();
        c.selected = None;
        return;
    }
    let row2 = del.top() - 8.0 - 30.0;
    let row1 = row2 - 6.0 - 30.0;
    let b = |k: usize, top: f32| Rect::from_min_size(Pos2::new(x0 + k as f32 * (bw + 6.0), top), Vec2::new(bw, 30.0));
    if pill_button(ui, b(0, row1), "Copy", BTN_PRIMARY, true).clicked() && c.copy(e.id, false) {
        c.hud("Copied");
    }
    let plain_ok = matches!(e.kind, Kind::Text | Kind::Code | Kind::Link) && (e.rtf || e.html) || e.kind == Kind::File;
    if pill_button(ui, b(1, row1), "Copy as plain text", BTN_SECONDARY, plain_ok).clicked() && c.copy(e.id, true) {
        c.hud("Copied as plain text");
    }
    let open = match e.kind {
        Kind::Link => Some("Open link"),
        Kind::File => Some("Show in Finder"),
        _ => None,
    };
    match open {
        Some(l) => {
            if pill_button(ui, b(0, row2), l, BTN_SECONDARY, true).clicked() {
                let arg = if e.kind == Kind::Link {
                    let t = e.text.trim();
                    if t.starts_with("http") { t.to_string() } else { format!("https://{t}") }
                } else {
                    e.files().first().map(|f| f.display().to_string()).unwrap_or_default()
                };
                let mut cmd = std::process::Command::new("open");
                if e.kind == Kind::File {
                    cmd.arg("-R");
                }
                let _ = cmd.arg(arg).spawn();
            }
        }
        None => {
            pill_button(ui, b(0, row2), "Open", BTN_SECONDARY, false);
        }
    }
    if pill_button(ui, b(1, row2), if e.pinned { "Unpin" } else { "Pin" }, BTN_SECONDARY, true).clicked() {
        c.store.set_pinned(e.id, !e.pinned);
    }
}

/// The entry's content, as large as `r` allows.
pub fn preview(c: &mut Clipboard, _ui: &mut Ui, p: &egui::Painter, r: Rect, e: &Entry, radius: f32) {
    p.rect_filled(r, radius, POPOVER());
    p.rect_stroke(r, radius, Stroke::new(1.0, white(HAIRLINE)), StrokeKind::Inside);
    let inner = r.shrink(12.0);
    let clip = p.with_clip_rect(r.shrink(1.0));
    match e.kind {
        Kind::Image => match c.thumb(e.id) {
            Some(t) => {
                // fit inside, centred
                let (iw, ih) = (e.w.max(1) as f32, e.h.max(1) as f32);
                let k = (r.width() / iw).min(r.height() / ih);
                let ir = Rect::from_center_size(r.center(), Vec2::new(iw * k, ih * k));
                cover_image(&clip, ir, t, (e.w, e.h), radius);
            }
            None => kind_glyph(&clip, Kind::Image, Rect::from_center_size(r.center(), Vec2::splat(48.0)), TEXT_FAINT(), None),
        },
        Kind::Color => {
            let col = parse_color(&e.text).unwrap_or(GRAY());
            let sw = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - 54.0));
            clip.rect_filled(sw, egui::CornerRadius { nw: radius as u8, ne: radius as u8, sw: 0, se: 0 }, col);
            let txt = if col.r() as u32 + col.g() as u32 + col.b() as u32 > 420 { Color32::from_rgb(20, 20, 22) } else { Color32::WHITE };
            clip.text(Pos2::new(sw.left() + 14.0, sw.bottom() - 14.0), Align2::LEFT_BOTTOM, e.text.trim(), mono(20.0), txt);
            let (h, s, l) = hsl(col);
            clip.text(Pos2::new(inner.left(), sw.bottom() + 14.0), Align2::LEFT_TOP, format!("rgb({}, {}, {})", col.r(), col.g(), col.b()), mono(11.0), TEXT_DIM());
            clip.text(Pos2::new(inner.left(), sw.bottom() + 30.0), Align2::LEFT_TOP, format!("hsl({h:.0}°, {s:.0}%, {l:.0}%)"), mono(11.0), TEXT_DIM());
        }
        Kind::Link => {
            let t = e.text.trim();
            let host = t.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(t).to_string();
            let band = Rect::from_min_max(r.min, Pos2::new(r.right(), r.top() + r.height() * 0.48));
            clip.rect_filled(band, egui::CornerRadius { nw: radius as u8, ne: radius as u8, sw: 0, se: 0 }, lerp_color(POPOVER(), BLUE(), 0.15));
            let letter = host.trim_start_matches("www.").chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default();
            let chip = Rect::from_center_size(band.center(), Vec2::splat(44.0));
            clip.rect_filled(chip, 12.0, app_color(&host));
            clip.text(chip.center(), Align2::CENTER_CENTER, letter, bold(22.0), Color32::WHITE);
            text_fit(&clip, Pos2::new(inner.left(), band.bottom() + 12.0), Align2::LEFT_TOP, &host, bold(13.0), TEXT(), inner.width());
            paragraph(&clip, Pos2::new(inner.left(), band.bottom() + 34.0), t, font(12.0), ACCENT(), inner.width());
        }
        Kind::File => {
            let mut y = inner.top();
            for f in e.files().iter().take(6) {
                kind_glyph(&clip, Kind::File, Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::splat(18.0)), TINT(), None);
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                text_fit(&clip, Pos2::new(inner.left() + 26.0, y + 1.0), Align2::LEFT_TOP, &name, bold(13.0), TEXT(), inner.width() - 26.0);
                let dir = f.parent().map(|d| d.display().to_string()).unwrap_or_default();
                text_fit(&clip, Pos2::new(inner.left() + 26.0, y + 18.0), Align2::LEFT_TOP, &middle_ellipsis(&dir, 46), font(11.0), TEXT_FAINT(), inner.width() - 26.0);
                y += 38.0;
            }
        }
        Kind::Code => {
            let g = clip.layout(prefix_chars(&e.text, PARAGRAPH_MAX).to_string(), mono(12.0), TEXT_SOFT(), inner.width());
            clip.galley(inner.min, g, TEXT());
        }
        Kind::Text => {
            paragraph(&clip, inner.min, &e.text, font(13.0), TEXT_SOFT(), inner.width());
        }
    }
}

/// Hue (degrees), saturation and lightness (percent) of a color.
pub fn hsl(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l * 100.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, s * 100.0, l * 100.0)
}

// ---------------------------------------------------------------- settings

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

fn settings_page(c: &mut Clipboard, ui: &mut Ui, body: Rect) {
    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
    ScrollArea::vertical().id_salt("clip-settings").auto_shrink([false, false]).show(&mut child, |ui| {
        let rows = c.settings.ignore.len().max(1);
        let h = (500.0f32).max(330.0 + (rows + 2) as f32 * ROW_H + 120.0);
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        let p = ui.painter().clone();
        let gap = 20.0;
        let x0 = area.left() + 24.0;
        let colw = (area.width() - 48.0 - gap) / 2.0;
        let changed_l = left_col(c, ui, &p, Rect::from_min_size(Pos2::new(x0, area.top() + 16.0), Vec2::new(colw, area.height())));
        let changed_r = right_col(c, ui, &p, Rect::from_min_size(Pos2::new(x0 + colw + gap, area.top() + 16.0), Vec2::new(colw, area.height())));
        if changed_l || changed_r {
            c.save_settings();
        }
    });
}

fn label(p: &egui::Painter, card: Rect, k: usize, text: &str) {
    p.text(Pos2::new(card.left() + 12.0, row_y(card, k)), Align2::LEFT_CENTER, text, font(13.0), TEXT());
}

fn left_col(c: &mut Clipboard, ui: &mut Ui, p: &egui::Painter, col: Rect) -> bool {
    let x0 = col.left();
    let w = col.width();
    let id = ui.scope_id().with("clip-left");
    let mut changed = false;
    let y = col.top();

    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, "History", bold(15.0), TEXT());
    let card = card_rows(p, x0, y + 28.0, w, 5);
    // keep N
    label(p, card, 0, "Keep the last");
    let ry = row_y(card, 0);
    let unit = p.text(Pos2::new(card.right() - 12.0, ry), Align2::RIGHT_CENTER, "items", font(12.0), TEXT_FAINT());
    let plus = Rect::from_center_size(Pos2::new(unit.left() - 20.0, ry), Vec2::new(24.0, 24.0));
    let num = Rect::from_center_size(Pos2::new(plus.left() - 22.0, ry), Vec2::new(44.0, 24.0));
    let minus = Rect::from_center_size(Pos2::new(num.left() - 12.0, ry), Vec2::new(24.0, 24.0));
    for (r, s, delta) in [(minus, "−", -50i64), (plus, "+", 50)] {
        let resp = ui.interact(r, id.with(("step", s)), Sense::click());
        p.rect_filled(r, 6.0, white(if resp.hovered() { 34 } else { 20 }));
        p.text(r.center(), Align2::CENTER_CENTER, s, bold(13.0), TEXT());
        if resp.clicked() {
            let step = if c.settings.keep <= 50 && delta < 0 || c.settings.keep < 50 { delta.signum() * 10 } else { delta };
            c.settings.keep = (c.settings.keep as i64 + step).clamp(KEEP_MIN as i64, KEEP_MAX as i64) as usize;
            changed = true;
        }
    }
    p.text(num.center(), Align2::CENTER_CENTER, c.settings.keep.to_string(), bold(13.0), TEXT());
    // age
    label(p, card, 1, "Remove older than");
    let ry = row_y(card, 1);
    let ar = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 230.0, ry - 13.0), Pos2::new(card.right() - 12.0, ry + 13.0));
    let cur = AGES.iter().position(|a| *a == c.settings.max_days).unwrap_or(2);
    if let Some(i) = segmented(ui, ar, id.with("age"), &["1 day", "7 days", "30 days", "Never"], cur) {
        c.settings.max_days = AGES[i];
        changed = true;
    }
    let switches: [(&str, &str, bool); 2] = [("Keep images", "up to 10 MB each", c.settings.keep_images), ("Move repeats to the top", "", c.settings.dedupe)];
    for (k, (l, hint, on)) in switches.iter().enumerate() {
        label(p, card, 2 + k, l);
        let ry = row_y(card, 2 + k);
        let sw = Rect::from_center_size(Pos2::new(card.right() - 27.0, ry), Vec2::new(30.0, 18.0));
        if !hint.is_empty() {
            p.text(Pos2::new(sw.left() - 10.0, ry), Align2::RIGHT_CENTER, *hint, font(12.0), TEXT_FAINT());
        }
        if switch(ui, sw, id.with(("hsw", k)), *on, true).clicked() {
            if k == 0 {
                c.settings.keep_images = !c.settings.keep_images;
            } else {
                c.settings.dedupe = !c.settings.dedupe;
            }
            changed = true;
        }
    }
    let ry = row_y(card, 4);
    let n = c.store.entries.len();
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, format!("{n} items · {} on disk", human_bytes(c.store.bytes_on_disk())), font(13.0), TEXT_DIM());
    let confirming = c.confirm_clear.is_some_and(|t| t.elapsed().as_secs_f32() < 3.0);
    let cl = if confirming { "Click again to clear" } else { "Clear unpinned" };
    let cw = button_width(p, cl, BTN_DANGER) - 6.0;
    let cr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - cw, ry - 12.0), Pos2::new(card.right() - 12.0, ry + 12.0));
    if pill_button(ui, cr, cl, if confirming { BTN_DANGER } else { BTN_SECONDARY }, c.store.entries.iter().any(|e| !e.pinned)).clicked() {
        if confirming {
            c.store.clear_unpinned();
            c.forget_dropped_thumbs();
            c.confirm_clear = None;
        } else {
            c.confirm_clear = Some(Instant::now());
        }
    }
    if confirming {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    changed
}

fn right_col(c: &mut Clipboard, ui: &mut Ui, p: &egui::Painter, col: Rect) -> bool {
    let x0 = col.left();
    let w = col.width();
    let id = ui.scope_id().with("clip-right");
    let mut changed = false;
    let mut y = col.top();

    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, "Privacy", bold(15.0), TEXT());
    let card = card_rows(p, x0, y + 28.0, w, 4);
    let switches = [
        ("Record passwords (until quit)", c.settings.record_concealed),
        ("Record tokens and card numbers", c.settings.record_secrets),
        ("Record from other devices", c.settings.record_remote),
    ];
    for (k, (l, on)) in switches.into_iter().enumerate() {
        label(p, card, k, l);
        let sw = Rect::from_center_size(Pos2::new(card.right() - 27.0, row_y(card, k)), Vec2::new(30.0, 18.0));
        if switch(ui, sw, id.with(("psw", k)), on, true).clicked() {
            let s = &mut c.settings;
            match k {
                0 => s.record_concealed = !s.record_concealed,
                1 => s.record_secrets = !s.record_secrets,
                _ => s.record_remote = !s.record_remote,
            }
            changed = true;
        }
    }
    let ry = row_y(card, 3);
    label(p, card, 3, "Pause recording");
    let mut x = card.right() - 12.0;
    if c.paused() {
        let bw = button_width(p, "Resume", BTN_PRIMARY) - 6.0;
        let br = Rect::from_min_max(Pos2::new(x - bw, ry - 12.0), Pos2::new(x, ry + 12.0));
        if pill_button(ui, br, "Resume", BTN_PRIMARY, true).clicked() {
            c.toggle_pause(0);
        }
        if let Some(l) = c.pause_label() {
            p.text(Pos2::new(br.left() - 10.0, ry), Align2::RIGHT_CENTER, l, font(12.0), AMBER());
        }
    } else {
        for (l, mins) in [("Until I resume", 0u64), ("1 hour", 60), ("15 min", 15)] {
            let bw = button_width(p, l, BTN_SECONDARY) - 10.0;
            let br = Rect::from_min_max(Pos2::new(x - bw, ry - 12.0), Pos2::new(x, ry + 12.0));
            if pill_button(ui, br, l, BTN_SECONDARY, true).clicked() {
                c.toggle_pause(mins);
            }
            x = br.left() - 6.0;
        }
    }
    y = card.bottom() + 16.0;

    // excluded apps
    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, "Never record from", bold(15.0), TEXT());
    let aw = button_width(p, "Add app…", BTN_SECONDARY) - 6.0;
    let ar = Rect::from_min_max(Pos2::new(col.right() - aw, y - 3.0), Pos2::new(col.right(), y + 21.0));
    if pill_button(ui, ar, "Add app…", BTN_SECONDARY, true).clicked() {
        c.choose_app();
    }
    let n = c.settings.ignore.len();
    let card = card_rows(p, x0, y + 28.0, w, n.max(1));
    if n == 0 {
        p.text(Pos2::new(card.left() + 12.0, row_y(card, 0)), Align2::LEFT_CENTER, "No apps excluded", font(13.0), TEXT_FAINT());
    }
    let mut remove = None;
    let mut budget = 4;
    for k in 0..n {
        let ry = row_y(card, k);
        let ig = c.settings.ignore[k].clone();
        let ir = Rect::from_center_size(Pos2::new(card.left() + sp::M + 10.0, ry), Vec2::splat(20.0));
        c.ignored_icon(ui.ctx(), p, ir, &ig, &mut budget);
        let clear = Rect::from_center_size(Pos2::new(card.right() - sp::M - 10.0, ry), Vec2::splat(20.0));
        let name_w = clear.left() - sp::S - (ir.right() + sp::S);
        text_fit(p, Pos2::new(ir.right() + sp::S, ry), Align2::LEFT_CENTER, &ig.name, font(13.0), TEXT(), name_w);
        let cr = ui.interact(clear, id.with(("rm", k)), Sense::click());
        if cr.hovered() {
            p.circle_filled(clear.center(), 10.0, white(26));
        }
        icon(p, Icon::Close, clear.shrink(5.0), TEXT_FAINT());
        if cr.on_hover_text(format!("Record from {} again", ig.name)).clicked() {
            remove = Some(k);
        }
    }
    if let Some(k) = remove {
        c.settings.ignore.remove(k);
        changed = true;
    }
    changed
}

impl Clipboard {
    /// Ask macOS for an app to exclude (the standard "Choose Application" dialog).
    fn choose_app(&mut self) {
        let tx = self.app_tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let script = "set a to choose application with prompt \"Never record copies from\" as alias\nreturn POSIX path of a";
            let out = std::process::Command::new("osascript").args(["-e", script]).output();
            let got = out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches('/').to_string());
            let app = got.filter(|p| !p.is_empty()).and_then(|p| {
                let path = std::path::PathBuf::from(p);
                let info = crate::kit::bundles::read_info(&path)?;
                Some(super::secret_apps::Found { bundle: info.id, name: info.name, path })
            });
            let _ = tx.send(app);
            ctx.request_repaint();
        });
    }

    /// An excluded app's own icon, else a colored square. Icons load a few
    /// per frame (`budget`).
    fn ignored_icon(&mut self, ctx: &egui::Context, p: &egui::Painter, r: Rect, ig: &Ignored, budget: &mut usize) {
        let key = ig.bundle.to_lowercase();
        if !self.icons.contains_key(&key) && *budget > 0 && !self.demo {
            if let Some(path) = self.app_paths.get(&key) {
                *budget -= 1;
                let tex = crate::kit::macos::app_icon(path, 64).map(|(n, rgba)| ctx.load_texture(format!("clip-ignore-icon:{key}"), egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba), egui::TextureOptions::LINEAR));
                self.icons.insert(key.clone(), tex);
                ctx.request_repaint();
            }
        }
        if let Some(Some(t)) = self.icons.get(&key) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            p.rect_filled(r.shrink(1.0), 5.0, app_color(&ig.name));
            let l: String = ig.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            p.text(r.center(), Align2::CENTER_CENTER, l, bold(11.0), Color32::WHITE);
        }
    }
}

// ---------------------------------------------------------------- toast

pub fn hud(c: &mut Clipboard, ctx: &egui::Context) {
    let Some((msg, at)) = &c.hud else { return };
    if at.elapsed().as_secs_f32() > 2.2 {
        c.hud = None;
        return;
    }
    let msg = msg.clone();
    let screen = ctx.input(|i| i.viewport().monitor_size).filter(|s| s.x > 100.0).unwrap_or(Vec2::new(1512.0, 982.0));
    let w = (msg.chars().count() as f32 * 7.4 + 56.0).clamp(160.0, 560.0);
    let size = Vec2::new(w, 40.0);
    let pos = Pos2::new((screen.x - size.x) / 2.0, 44.0);
    let title = format!("{}clipboard-hud", crate::kit::native::PLAIN_WINDOW);
    let vid = ViewportId::from_hash_of(("clipboard-hud", at));
    let b = ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_position(pos)
        .with_inner_size(size)
        .with_active(false)
        .with_mouse_passthrough(true);
    crate::kit::viewport::show(ctx, vid, b, |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(r, r.height() / 2.0, POPOVER());
        p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        text_fit(p, r.center(), Align2::CENTER_CENTER, &msg, font(13.0), TEXT(), r.width() - 24.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_of_the_accent() {
        let (h, s, l) = hsl(Color32::from_rgb(11, 107, 216));
        assert!((h - 211.9).abs() < 0.5, "{h}");
        assert!((s - 90.3).abs() < 0.5, "{s}");
        assert!((l - 44.5).abs() < 0.5, "{l}");
        assert_eq!(hsl(Color32::WHITE), (0.0, 0.0, 100.0));
    }

    #[test]
    fn filters_pass() {
        let mut e = Entry { id: 1, kind: Kind::Link, text: "x".into(), app: String::new(), bundle: String::new(), first: 0, last: 0, count: 1, pinned: false, w: 0, h: 0, rtf: false, html: false, bytes: 0, hash: 0, secret: false };
        assert!(Filter::All.passes(&e));
        assert!(Filter::Kind(Kind::Link).passes(&e));
        assert!(!Filter::Kind(Kind::Text).passes(&e));
        assert!(!Filter::Pinned.passes(&e));
        e.pinned = true;
        assert!(Filter::Pinned.passes(&e));
    }
}
