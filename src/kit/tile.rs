//! Building blocks for panel widgets at the grid sizes (see
//! `core::tool::Size`): the card, the standard Small / Wide layouts, the
//! split Row (a summary on the left, a breakdown on the right),
//! the header of a Medium or Large widget, rings and sparklines.

use super::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

/// Corner radius of a widget.
pub const R_TILE: f32 = 12.0;
/// Inner padding of a widget.
pub const PAD: f32 = 12.0;

/// The card behind a widget. Hover lifts it; `selected` (its sheet is
/// open) adds the blue ring. A click is the widget's main action.
pub fn card(ui: &mut Ui, rect: Rect, id: egui::Id, selected: bool) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("hover"), resp.hovered());
    paint_card(ui.painter(), rect, h, selected);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn paint_card(p: &egui::Painter, rect: Rect, hover: f32, selected: bool) {
    p.rect_filled(rect, R_TILE, lerp_color(CARD(), RAISED(), hover * 0.5));
    if selected {
        p.rect_stroke(rect, R_TILE, Stroke::new(1.5, BLUE()), StrokeKind::Inside);
    }
}

/// A widget whose tool stopped after an error: its glyph and name, dimmed,
/// and "Stopped". A click restarts the app (the caller does it).
pub fn stopped(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, title: &str) -> Response {
    let resp = card(ui, rect, id, false).on_hover_text("Restart Cmd+Meow");
    let p = ui.painter();
    let inner = rect.shrink(PAD);
    icon(p, glyph, Rect::from_min_size(inner.min, Vec2::splat(18.0)), TEXT_FAINT());
    if rect.height() <= 130.0 && rect.width() <= 130.0 {
        small_text(p, rect, title, "Stopped", AMBER());
    } else {
        text_fit(p, Pos2::new(inner.left() + 26.0, inner.top() + 9.0), Align2::LEFT_CENTER, title, ty::title(), TEXT_DIM(), inner.width() - 26.0);
        let st = p.text(Pos2::new(inner.left(), inner.bottom()), Align2::LEFT_BOTTOM, "Stopped", ty::label(), AMBER());
        p.text(Pos2::new(st.right() + 8.0, inner.bottom()), Align2::LEFT_BOTTOM, "Restart", ty::label(), BLUE());
    }
    resp
}

/// Small (1:1): the glyph on top, a bold line and a dim line at the bottom.
#[allow(clippy::too_many_arguments)]
pub fn small(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, title: &str, sub: &str, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    icon(p, glyph, Rect::from_min_size(rect.min + Vec2::splat(PAD), Vec2::splat(18.0)), lift(color));
    small_text(p, rect, title, sub, TEXT_DIM());
    resp
}

/// The two lines at the bottom of a Small widget.
pub fn small_text(p: &egui::Painter, rect: Rect, title: &str, sub: &str, sub_col: Color32) {
    let w = rect.width() - 2.0 * PAD;
    // a long name ("Screenshot") steps down a size or two before it's cut
    let fid = [13.0, 12.0, 11.0].into_iter().map(bold).find(|f| p.layout_no_wrap(title.to_string(), f.clone(), TEXT()).size().x <= w).unwrap_or_else(ty::title);
    let sub_col = if sub_col == TEXT_DIM() { ty::caption_color() } else { sub_col };
    let sr = text_fit(p, Pos2::new(rect.left() + PAD, rect.bottom() - 10.0), Align2::LEFT_BOTTOM, sub, ty::caption(), sub_col, w);
    let tb = if sub.is_empty() { rect.bottom() - 10.0 } else { sr.top() - sp::LINE_GAP };
    text_fit(p, Pos2::new(rect.left() + PAD, tb), Align2::LEFT_BOTTOM, title, fid, TEXT(), w);
}

/// Small (1:1) gauge: a ring with the glyph inside and a caption under it.
#[allow(clippy::too_many_arguments)]
pub fn small_ring(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, frac: f32, color: Color32, caption: &str, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    // the ring fills the tile above its caption
    let (c, rr) = small_ring_at(rect);
    let w = (rr * 0.2).clamp(4.0, 6.0);
    ring(p, c, rr - w / 2.0, w, frac, color);
    icon(p, glyph, Rect::from_center_size(c, Vec2::splat(rr * 0.75)), TEXT_SOFT());
    text_fit(p, Pos2::new(rect.center().x, rect.bottom() - 10.0), Align2::CENTER_BOTTOM, caption, ty::title(), TEXT(), rect.width() - 10.0);
    resp
}

/// Where a Wide tile's ring sits, and its radius: it takes the tile's
/// height, less a margin.
pub fn wide_ring(rect: Rect) -> (Pos2, f32) {
    let rr = (rect.height() / 2.0 - 11.0).clamp(17.0, 27.0);
    (Pos2::new(rect.left() + PAD + rr, rect.center().y), rr)
}

/// Where a Small tile's ring sits, and its radius: it fills the tile above
/// a one-line caption.
pub fn small_ring_at(rect: Rect) -> (Pos2, f32) {
    let rr = ((rect.height() - 40.0) / 2.0).min(rect.width() / 2.0 - 12.0).clamp(18.0, 30.0);
    (Pos2::new(rect.center().x, rect.top() + 8.0 + rr), rr)
}

/// Wide (1:2): a ring (or the glyph) on the left; label, value and caption
/// stacked on the right.
#[allow(clippy::too_many_arguments)]
pub fn wide(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, label: &str, value: &str, bar_or_caption: Result<(f32, Color32), &str>, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    let cy = rect.center().y;
    let (c, rr) = wide_ring(rect);
    let caption = match bar_or_caption {
        Ok((f, col)) => {
            let w = (rr * 0.2).clamp(4.0, 5.5);
            ring(p, c, rr - w / 2.0, w, f, col);
            icon(p, glyph, Rect::from_center_size(c, Vec2::splat(rr * 0.8)), TEXT_SOFT());
            ""
        }
        Err(cap) => {
            p.circle_filled(c, rr, white(14));
            icon(p, glyph, Rect::from_center_size(c, Vec2::splat(rr * 0.9)), lift(color));
            cap
        }
    };
    let x0 = c.x + rr + sp::M;
    let w = rect.right() - PAD - x0;
    stack_lines(p, x0, cy, &[(label, ty::caption(), ty::caption_color()), (value, ty::title(), TEXT()), (caption, ty::caption(), ty::caption_color())], w);
    resp
}

/// The chip of a split Row (where `row_split` draws it), for widgets that
/// paint over it.
pub fn row_chip(rect: Rect) -> Rect {
    Rect::from_min_size(Pos2::new(rect.left() + 12.0, rect.center().y - 17.0), Vec2::splat(34.0))
}

/// Row (1:4) in two halves. Left: a big chip, the label, a big value and a
/// caption. The right half is returned for a breakdown (bar lines, a
/// sparkline…), so the row shows as much as a Medium's first lines.
#[allow(clippy::too_many_arguments)]
pub fn row_split(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, label: &str, value: &str, caption: &str, selected: bool) -> (Response, Rect) {
    row_split_at(ui, rect, id, glyph, color, label, value, caption, selected, 0.47)
}

/// [`row_split`] with the left part `ratio` of the width.
#[allow(clippy::too_many_arguments)]
pub fn row_split_at(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, label: &str, value: &str, caption: &str, selected: bool, ratio: f32) -> (Response, Rect) {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    let chip = row_chip(rect);
    p.circle_filled(chip.center(), 17.0, white(14));
    icon(p, glyph, Rect::from_center_size(chip.center(), Vec2::splat(18.0)), lift(color));
    let split = rect.left() + (rect.width() * ratio).round();
    let (x0, x1) = (chip.right() + sp::M, split - sp::M);
    let cy = rect.center().y;
    // the value shrinks a step before it gets cut
    let f = if p.layout_no_wrap(value.to_string(), ty::number(), TEXT()).size().x <= x1 - x0 { ty::number() } else { ty::lead() };
    stack_lines(p, x0, cy, &[(label, ty::caption(), ty::caption_color()), (value, f, TEXT()), (caption, ty::caption(), ty::caption_color())], x1 - x0);
    p.line_segment([Pos2::new(split, rect.top() + 16.0), Pos2::new(split, rect.bottom() - 16.0)], Stroke::new(1.0, white(15)));
    (resp, Rect::from_min_max(Pos2::new(split + sp::M, rect.top() + PAD), Pos2::new(rect.right() - PAD, rect.bottom() - PAD)))
}

/// Up to three bar lines stacked in `r` (the right half of a split Row);
/// `empty` is shown when there are none.
pub fn lines(p: &egui::Painter, r: Rect, lines: &[(String, Option<(f32, Color32)>, String)], empty: &str) {
    if lines.is_empty() {
        text_fit(p, r.center(), Align2::CENTER_CENTER, empty, font(11.0), TEXT_FAINT(), r.width());
        return;
    }
    // one line each: label, a short inline bar, the value. Bars sit between
    // the words instead of under them, so lines never crowd each other
    let n = lines.len().min(3).min(((r.height() / 20.0).floor() as usize).max(1));
    let lh = (r.height() / n as f32).min(24.0);
    let top = r.center().y - lh * n as f32 / 2.0;
    for (k, (label, frac, value)) in lines.iter().take(n).enumerate() {
        let lr = Rect::from_min_size(Pos2::new(r.left(), top + k as f32 * lh), Vec2::new(r.width(), lh));
        bar_line(p, lr, label, *frac, value);
    }
}

/// The header of a Medium or Large widget: the title (13/600) with its
/// caption under it, and an optional `note` on the right. The glyph is not
/// drawn: the title names the widget. Returns the rect left for the body.
pub fn head(p: &egui::Painter, rect: Rect, _glyph: Icon, _color: Color32, title: &str, note: &str) -> Rect {
    head_with(p, rect, title, "", note)
}

/// [`head`] with a caption line under the title.
pub fn head_with(p: &egui::Painter, rect: Rect, title: &str, caption: &str, note: &str) -> Rect {
    let x0 = rect.left() + PAD;
    let x1 = rect.right() - PAD;
    let top = rect.top() + PAD;
    let nr = text_fit(p, Pos2::new(x1, top + 8.0), Align2::RIGHT_CENTER, note, ty::label(), white(166), (x1 - x0) * 0.5);
    let tw = if note.is_empty() { x1 - x0 } else { nr.left() - x0 - 8.0 };
    text_fit(p, Pos2::new(x0, top + 8.0), Align2::LEFT_CENTER, title, ty::title(), TEXT(), tw);
    let mut y = top + 16.0;
    if !caption.is_empty() {
        text_fit(p, Pos2::new(x0, y + 2.0), Align2::LEFT_TOP, caption, ty::caption(), ty::caption_color(), tw);
        y += 16.0;
    }
    Rect::from_min_max(Pos2::new(x0, y + sp::S), Pos2::new(x1, rect.bottom() - PAD))
}

/// A progress ring from 12 o'clock, clockwise, on a faint track.
pub fn ring(p: &egui::Painter, c: Pos2, r: f32, width: f32, frac: f32, color: Color32) {
    crate::kit::glass::stroke_circle(&p, c, r, Stroke::new(width, white(26)));
    let f = frac.clamp(0.0, 1.0);
    if f > 0.005 {
        let a0 = -std::f32::consts::FRAC_PI_2;
        let n = ((f * 64.0) as usize).max(3);
        p.add(Shape::line(arc_points(c, r, a0, a0 + f * std::f32::consts::TAU, n), Stroke::new(width, color)));
        for q in [c + Vec2::new(a0.cos(), a0.sin()) * r, c + Vec2::angled(a0 + f * std::f32::consts::TAU) * r] {
            p.circle_filled(q, width / 2.0, color);
        }
    }
}

/// A bar graph of `values` (newest on the right) scaled from zero into `r`.
/// Bars well above the rest (a busy spike) turn orange. `fill` is unused
/// and kept for callers.
pub fn spark(p: &egui::Painter, r: Rect, values: &[f32], color: Color32, _fill: Option<Color32>) {
    let hi = values.iter().copied().fold(0.0f32, f32::max);
    spark_to(p, r, values, color, hi);
}

/// [`spark`] with the top of the graph at `top` (e.g. 100 for a percentage).
pub fn spark_to(p: &egui::Painter, r: Rect, values: &[f32], color: Color32, top: f32) {
    if values.is_empty() {
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, white(20)));
        return;
    }
    let hi = top.max(values.iter().copied().fold(0.0f32, f32::max)).max(1e-3);
    let mut sorted: Vec<f32> = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    let gap = 2.0;
    let n = values.len().min(((r.width() + gap) / (3.0 + gap)) as usize).max(1);
    let bw = (r.width() - gap * (n as f32 - 1.0)) / n as f32;
    for (k, v) in values[values.len() - n..].iter().enumerate() {
        let h = ((v / hi) * r.height()).max(2.0);
        let x = r.left() + k as f32 * (bw + gap);
        let hot = *v > median * 1.6 && *v >= hi * 0.6 && *v > 1.0;
        let c = if hot { AMBER() } else { with_alpha(color, 0.8) };
        p.rect_filled(Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + bw, r.bottom())), egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, c);
    }
}

/// A list line inside a Medium or Large widget: label, bar, value.
pub fn bar_line(p: &egui::Painter, r: Rect, label: &str, frac: Option<(f32, Color32)>, value: &str) {
    // the value keeps its full width ("62% · tomorrow 23:40"): app text, never cut
    let lw = p.layout_no_wrap(label.to_string(), font(12.0), TEXT()).size().x;
    let vw = p.layout_no_wrap(value.to_string(), font(12.0), TEXT()).size().x.max(40.0);
    // the label keeps its full width when it can; the bar takes what's left
    let room = r.width() - vw - lw - 22.0;
    let bw = if frac.is_some() && room >= 24.0 { room.min((r.width() * 0.3).min(110.0)) } else { 0.0 };
    p.text(Pos2::new(r.right(), r.center().y), Align2::RIGHT_CENTER, value, font(12.0), TEXT());
    if let (Some((f, c)), true) = (frac, bw > 0.0) {
        bar(p, Rect::from_min_max(Pos2::new(r.right() - vw - 10.0 - bw, r.center().y - 2.0), Pos2::new(r.right() - vw - 10.0, r.center().y + 2.0)), f, c);
    }
    text_fit(p, Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, font(12.0), white(166), r.width() - vw - bw - 22.0);
}

/// A dashed box with a quiet message (nothing to show yet).
pub fn empty_note(p: &egui::Painter, r: Rect, msg: &str) {
    dashed_rect(p, r, 8.0, white(40));
    text_fit(p, r.center(), Align2::CENTER_CENTER, msg, font(12.0), TEXT_FAINT(), r.width() - 16.0);
}

/// A widget whose data hasn't arrived yet: its glyph and name, and soft
/// pulsing bars where the numbers will be. Keeps repainting while shown.
pub fn loading(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, title: &str) -> Response {
    let resp = card(ui, rect, id, false);
    let p = ui.painter();
    let t = ui.input(|i| i.time) as f32;
    // a slow breath, 0..1
    let pulse = 0.5 - 0.5 * (t * 3.0).cos();
    let bar = |r: Rect, k: f32| {
        let a = 10.0 + 14.0 * (0.5 - 0.5 * (t * 3.0 - k * 0.6).cos());
        p.rect_filled(r, r.height() / 2.0, white(a as u8));
    };
    let inner = rect.shrink(PAD);
    let glyph_col = with_alpha(lift(color), 0.55 + 0.35 * pulse);
    if rect.height() <= 130.0 && rect.width() <= 130.0 {
        // Small: glyph on top, name at the bottom, a bar for the value
        icon(p, glyph, Rect::from_min_size(inner.min, Vec2::splat(18.0)), glyph_col);
        text_fit(p, Pos2::new(inner.left(), rect.bottom() - 26.0), Align2::LEFT_BOTTOM, title, ty::title(), TEXT_DIM(), inner.width());
        bar(Rect::from_min_size(Pos2::new(inner.left(), rect.bottom() - 20.0), Vec2::new(inner.width() * 0.6, 8.0)), 0.0);
    } else if rect.height() <= 130.0 {
        // Wide / Row: the ring's place on the left, name and a bar beside it
        let (c, rr) = wide_ring(rect);
        stroke_circle(p, c, rr - 3.0, Stroke::new(5.0, white((12.0 + 12.0 * pulse) as u8)));
        icon(p, glyph, Rect::from_center_size(c, Vec2::splat(rr * 0.75)), glyph_col);
        let x = c.x + rr + 12.0;
        let w = rect.right() - PAD - x;
        text_fit(p, Pos2::new(x, rect.center().y - 4.0), Align2::LEFT_BOTTOM, title, ty::caption(), ty::caption_color(), w);
        bar(Rect::from_min_size(Pos2::new(x, rect.center().y + 2.0), Vec2::new(w * 0.75, 10.0)), 0.0);
    } else {
        // Medium and up: glyph and name, then bars down the card
        icon(p, glyph, Rect::from_min_size(inner.min, Vec2::splat(18.0)), glyph_col);
        let note = if inner.width() >= 220.0 { p.text(Pos2::new(inner.right(), inner.top() + 9.0), Align2::RIGHT_CENTER, "Loading…", ty::caption(), TEXT_FAINT()).width() + 10.0 } else { 0.0 };
        text_fit(p, Pos2::new(inner.left() + 26.0, inner.top() + 9.0), Align2::LEFT_CENTER, title, ty::title(), TEXT_DIM(), inner.width() - 26.0 - note);
        let mut y = inner.top() + 32.0;
        let widths = [0.7, 0.45, 0.85, 0.55, 0.65, 0.4];
        let mut k = 0;
        while y + 10.0 <= inner.bottom() + 0.5 && k < widths.len() {
            bar(Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width() * widths[k], 10.0)), k as f32);
            y += 22.0;
            k += 1;
        }
    }
    // the shimmer moves only where someone sees it
    if crate::kit::activity::visible() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    resp
}
