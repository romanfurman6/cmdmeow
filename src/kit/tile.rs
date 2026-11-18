//! Building blocks for panel widgets at the grid sizes (see
//! `core::tool::Size`): the card, the standard Small / Wide layouts, the
//! split Row (a summary on the left, a breakdown on the right),
//! the header of a Medium or Large widget, rings and sparklines.

use super::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

/// Corner radius of a widget (R_TILE, SP13 W1).
pub const R_TILE: f32 = 10.0;
/// Inner padding of a widget.
pub const PAD: f32 = 12.0;
/// How far a pressed widget card sinks: its 1 pt base.
pub const PRESS_DROP: f32 = 1.0;

thread_local! {
    /// The card pressed this frame: its layer, the first shape of its face
    /// and how far it is in. [`finish_press`] moves the face and everything
    /// drawn on it after the widget is done.
    static PRESSED: std::cell::Cell<Option<(egui::LayerId, egui::layers::ShapeIdx, f32)>> = const { std::cell::Cell::new(None) };
}

/// Whether the pointer is over the card: true over its buttons and rows
/// too, because the whole card is what hovers (W12).
fn over(ui: &Ui, rect: Rect) -> bool {
    ui.rect_contains_pointer(rect)
}

/// The card of a widget with one action (W1): hover lifts it, a press
/// sinks it 1 pt into its base and it springs back. `selected` (its sheet
/// is open) adds the blue ring.
pub fn card(ui: &mut Ui, rect: Rect, id: egui::Id, selected: bool) -> Response {
    let k = super::key::press(ui, rect, id, true, None);
    let h = ui.ctx().animate_bool(id.with("hover"), over(ui, rect));
    let p = ui.painter();
    paint_base(p, rect);
    let face = p.ctx().graphics_mut(|g| g.entry(p.layer_id()).next_idx());
    paint_face(p, rect, h, selected);
    if k.press.abs() > 0.001 {
        PRESSED.with(|c| c.set(Some((p.layer_id(), face, k.press))));
    }
    k.resp
}

/// The card of a widget with buttons or rows inside (W1): it hovers as a
/// whole but doesn't sink; its buttons press instead.
pub fn frame(ui: &mut Ui, rect: Rect, id: egui::Id, selected: bool) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("hover"), over(ui, rect));
    paint_card(ui.painter(), rect, h, selected);
    resp
}

/// Call after a widget is drawn: a pressed card moves down with all it holds.
pub fn finish_press(ctx: &egui::Context) {
    if let Some((layer, start, press)) = PRESSED.with(|c| c.take()) {
        ctx.graphics_mut(|g| {
            let list = g.entry(layer);
            let end = list.next_idx();
            if start.0 > end.0 {
                return;
            }
            list.transform_range(start, end, egui::emath::TSTransform::from_translation(Vec2::new(0.0, PRESS_DROP * press)));
        });
    }
}

/// A widget card that isn't interactive (the update banner, a preview).
pub fn paint_card(p: &egui::Painter, rect: Rect, hover: f32, selected: bool) {
    paint_base(p, rect);
    paint_face(p, rect, hover, selected);
}

/// The 1 pt base under the card.
fn paint_base(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect.translate(Vec2::new(0.0, PRESS_DROP)), R_TILE, TILE_BASE());
}

/// The card itself: see-through CARD (the panel glow shows through), the
/// top highlight and the page cards' 1 pt edge.
fn paint_face(p: &egui::Painter, rect: Rect, hover: f32, selected: bool) {
    p.rect_filled(rect, R_TILE, lerp_color(CARD(), RAISED(), hover * 0.5));
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 1.0));
    p.with_clip_rect(top.intersect(p.clip_rect())).rect_filled(rect, R_TILE, TILE_HI());
    p.rect_stroke(rect, R_TILE, Stroke::new(1.0, CARD_EDGE()), StrokeKind::Inside);
    if selected {
        p.rect_stroke(rect, R_TILE, Stroke::new(1.5, ACTION()), StrokeKind::Inside);
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
        p.text(Pos2::new(st.right() + 8.0, inner.bottom()), Align2::LEFT_BOTTOM, "Restart", ty::label(), ACTION());
    }
    resp
}

/// A widget whose tool needs a permission it doesn't have (W10): the glyph
/// dimmed, the widget's name, what it needs and "Needs access", all
/// readable. A click asks for the permission (the caller does it).
pub fn locked(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, title: &str, needs: &str) -> Response {
    let dim = with_alpha(TEXT_DIM(), if is_light() { 0.62 } else { 0.45 });
    if rect.width() <= 130.0 {
        let resp = card(ui, rect, id, false);
        let p = ui.painter();
        icon(p, glyph, Rect::from_min_size(rect.min + Vec2::new(SMALL_SIDE, PAD), Vec2::splat(20.0)), dim);
        small_text(p, rect, title, "Needs access", TEXT_SOFT());
        return resp;
    }
    let resp = card(ui, rect, id, false);
    let p = ui.painter();
    let bx = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - 20.0), Vec2::splat(40.0));
    icon(p, glyph, Rect::from_center_size(bx.center(), Vec2::splat(26.0)), dim);
    let x0 = bx.right() + PAD;
    stack_lines(p, x0, rect.center().y, &[(title, ty::label(), ty::caption_color()), (needs, ty::lead(), TEXT()), ("Needs access", ty::caption(), TEXT_SOFT())], rect.right() - PAD - x0);
    resp
}

/// Small (1:1): the glyph on top, a bold line and a dim line at the bottom.
#[allow(clippy::too_many_arguments)]
pub fn small(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, title: &str, sub: &str, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    icon(p, glyph, Rect::from_min_size(rect.min + Vec2::new(SMALL_SIDE, PAD), Vec2::splat(20.0)), lift(color));
    small_text(p, rect, title, sub, TEXT_DIM());
    resp
}

/// Side padding of a Small widget: "Screenshot" fits at 12 without shrinking.
pub const SMALL_SIDE: f32 = 10.0;

/// The two lines at the bottom of a Small widget.
/// (W4: name 12/600, 3 pt, caption 11; 12 from the bottom.)
pub fn small_text(p: &egui::Painter, rect: Rect, title: &str, sub: &str, sub_col: Color32) {
    let w = rect.width() - 2.0 * SMALL_SIDE;
    // a long name steps down to 11 before it's cut
    let fid = [12.0, 11.0].into_iter().map(bold).find(|f| p.layout_no_wrap(title.to_string(), f.clone(), TEXT()).size().x <= w).unwrap_or_else(|| bold(11.0));
    let sub_col = if sub_col == TEXT_DIM() { ty::caption_color() } else { sub_col };
    let sr = text_fit(p, Pos2::new(rect.left() + SMALL_SIDE, rect.bottom() - PAD), Align2::LEFT_BOTTOM, sub, ty::caption(), sub_col, w);
    let tb = if sub.is_empty() { rect.bottom() - PAD } else { sr.top() - 3.0 };
    text_fit(p, Pos2::new(rect.left() + SMALL_SIDE, tb), Align2::LEFT_BOTTOM, title, fid, TEXT(), w);
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
    // a long caption ("Session 19%") steps down a size or two before it's cut
    let w = rect.width() - 10.0;
    let fid = [13.0, 12.0, 11.0].into_iter().map(bold).find(|f| p.layout_no_wrap(caption.to_string(), f.clone(), TEXT()).size().x <= w).unwrap_or_else(ty::title);
    text_fit(p, Pos2::new(rect.center().x, rect.bottom() - 10.0), Align2::CENTER_BOTTOM, caption, fid, TEXT(), w);
    resp
}

/// Where a Wide tile's ring sits, and its radius: it takes the tile's
/// height, less a margin.
pub fn wide_ring(rect: Rect) -> (Pos2, f32) {
    // W2: 50 in 1:2 and 1:4
    let rr = (rect.height() / 2.0 - 11.0).clamp(17.0, 25.0);
    (Pos2::new(rect.left() + PAD + rr, rect.center().y), rr)
}

/// Where a Small tile's ring sits, and its radius: it fills the tile above
/// a one-line caption.
pub fn small_ring_at(rect: Rect) -> (Pos2, f32) {
    // W2: 44 in 1:1
    let rr = ((rect.height() - 40.0) / 2.0).min(rect.width() / 2.0 - 12.0).clamp(18.0, 22.0);
    (Pos2::new(rect.center().x, rect.top() + 8.0 + rr), rr)
}

/// Wide (1:2) with all three lines (W3): a ring 50 (`ring`) or the glyph 26
/// in a 40 box on the left; label, value and caption beside it.
#[allow(clippy::too_many_arguments)]
pub fn wide_full(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, ring_: Option<(f32, Color32)>, label: &str, value: &str, caption: &str, caption_col: Color32, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    let x0 = visual(p, rect, glyph, color, ring_);
    left_text(p, x0, rect.center().y, label, value, caption, caption_col, rect.right() - PAD - x0);
    resp
}

/// Label, value and caption stacked at `x0` (W3, W5). The value is the
/// app's number and keeps its width; label and caption can be a name (a
/// device's), so they fade at `tw` rather than end in "…".
#[allow(clippy::too_many_arguments)]
fn left_text(p: &egui::Painter, x0: f32, cy: f32, label: &str, value: &str, caption: &str, caption_col: Color32, tw: f32) {
    let y = stack_lines(p, x0, cy, &[(label, ty::label(), Color32::TRANSPARENT), (value, ty::lead(), TEXT()), (caption, ty::caption(), Color32::TRANSPARENT)], tw);
    let mut k = 0;
    for (t, f, c) in [(label, ty::label(), ty::caption_color()), (value, ty::lead(), Color32::TRANSPARENT), (caption, ty::caption(), caption_col)] {
        if t.is_empty() {
            continue;
        }
        if c != Color32::TRANSPARENT {
            text_fade(p, Pos2::new(x0, y[k].center().y), t, f, c, tw);
        }
        k += 1;
    }
}

/// Wide (1:2) for a widget whose value is someone else's text (W3): the
/// glyph, the label, the text at 13/400 fading at the edge, the caption.
#[allow(clippy::too_many_arguments)]
pub fn wide_clip(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, label: &str, clip: &str, caption: &str, selected: bool) -> Response {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    let x0 = visual(p, rect, glyph, color, None);
    clip_lines(p, x0, rect.center().y, rect.right() - PAD - x0, label, clip, caption);
    resp
}

/// Label, a faded clip and a caption stacked at `x0` around `cy`.
pub fn clip_lines(p: &egui::Painter, x0: f32, cy: f32, w: f32, label: &str, clip: &str, caption: &str) {
    let fonts = [ty::label(), font(13.0), ty::caption()];
    let hs: Vec<f32> = fonts.iter().map(|f| line_h(p, f)).collect();
    let n = if caption.is_empty() { 2 } else { 3 };
    let total: f32 = hs[..n].iter().sum::<f32>() + sp::LINE_GAP * (n - 1) as f32;
    let mut y = cy - total / 2.0;
    text_fit(p, Pos2::new(x0, y + hs[0] / 2.0), Align2::LEFT_CENTER, label, fonts[0].clone(), ty::caption_color(), w);
    y += hs[0] + sp::LINE_GAP;
    text_fade(p, Pos2::new(x0, y + hs[1] / 2.0), clip, fonts[1].clone(), TEXT(), w);
    y += hs[1] + sp::LINE_GAP;
    if n == 3 {
        text_fit(p, Pos2::new(x0, y + hs[2] / 2.0), Align2::LEFT_CENTER, caption, fonts[2].clone(), ty::caption_color(), w);
    }
}

/// The one visual on the left of a Wide or Row widget (W2): a ring 50 with
/// the glyph in it, or the glyph 26 in a 40 box in the group colour, no
/// plate. Returns where the text starts.
pub fn visual(p: &egui::Painter, rect: Rect, glyph: Icon, color: Color32, ring_: Option<(f32, Color32)>) -> f32 {
    match ring_ {
        Some((f, col)) => {
            let (c, rr) = wide_ring(rect);
            let w = (rr * 0.22).clamp(4.0, 5.5);
            ring(p, c, rr - w / 2.0, w, f, col);
            icon(p, glyph, Rect::from_center_size(c, Vec2::splat(19.0)), TEXT_SOFT());
            c.x + rr + PAD
        }
        None => {
            let bx = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - 20.0), Vec2::splat(40.0));
            icon(p, glyph, Rect::from_center_size(bx.center(), Vec2::splat(26.0)), lift(color));
            bx.right() + PAD
        }
    }
}

/// Width kept for a Row's left text (W5): with ring 50 and PAD round it,
/// every Row's divider sits on one line, [`ROW_SPLIT`] from the left edge.
pub const ROW_TEXT: f32 = 86.0;
/// Where every Row's divider stands, from the card's left edge (W5).
pub const ROW_SPLIT: f32 = PAD + 50.0 + PAD + ROW_TEXT + PAD;

/// Row (1:4) per W5: the Wide layout on the left (ring 50 or glyph 26, then
/// label, value, caption), a hairline at [`ROW_SPLIT`] in every Row (it
/// moves right only when the value is wider, so the value is never cut),
/// and the right part returned for [`lines`].
#[allow(clippy::too_many_arguments)]
pub fn row_split_full(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, ring_: Option<(f32, Color32)>, label: &str, value: &str, caption: &str, caption_col: Color32, selected: bool) -> (Response, Rect) {
    let resp = card(ui, rect, id, selected);
    let p = ui.painter();
    let x0 = visual(p, rect, glyph, color, ring_);
    let vw = p.layout_no_wrap(value.to_string(), ty::lead(), TEXT()).size().x;
    let split = (rect.left() + ROW_SPLIT).max(x0 + vw + PAD).ceil();
    left_text(p, x0, rect.center().y, label, value, caption, caption_col, split - PAD - x0);
    let inset = rect.height() * 0.06;
    p.line_segment([Pos2::new(split, rect.top() + inset), Pos2::new(split, rect.bottom() - inset)], Stroke::new(1.0, white(HAIRLINE_STRONG)));
    (resp, Rect::from_min_max(Pos2::new(split + PAD, rect.top() + PAD), Pos2::new(rect.right() - PAD, rect.bottom() - PAD)))
}

/// One action of a widget's action strip ([`actions`]).
pub struct Action<'a> {
    pub label: &'a str,
    /// The shortcut (or a short note) under the name; empty for none.
    pub sub: &'a str,
    /// Colour of the glyph and name (the one red Record).
    pub color: Color32,
    pub tip: &'a str,
}

/// Actions side by side in `r` (W5): equal shares, no frame or fill, the
/// content centred, one hairline at 88% of the height between them. No
/// hover inside the widget (W12): a held action dims and drops 1 pt.
/// `glyph` draws action `k`'s icon in a rect in a colour. Returns the
/// clicked action.
pub fn actions(ui: &mut Ui, p: &egui::Painter, id: egui::Id, r: Rect, acts: &[Action], glyph: impl Fn(&egui::Painter, usize, Rect, Color32)) -> Option<usize> {
    let n = acts.len().max(1) as f32;
    let w = r.width() / n;
    let mut out = None;
    for (k, a) in acts.iter().enumerate() {
        let share = Rect::from_min_size(Pos2::new(r.left() + w * k as f32, r.top()), Vec2::new(w, r.height()));
        if k > 0 {
            let inset = r.height() * 0.06;
            p.line_segment([Pos2::new(share.left(), r.top() + inset), Pos2::new(share.left(), r.bottom() - inset)], Stroke::new(1.0, white(HAIRLINE_STRONG)));
        }
        let b = share.shrink(4.0);
        let key = super::key::press(ui, b, id.with(("act", k)), true, None);
        let dy = PRESS_DROP * key.press.max(0.0);
        let fg = press_dim(a.color, key.press);
        let sub_col = press_dim(TEXT_FAINT(), key.press);
        // icon 18, 4, name 12, sub 11, as one block in the middle
        let block = 18.0 + 4.0 + 16.0 + if a.sub.is_empty() { 0.0 } else { 14.0 };
        let top = b.center().y - block / 2.0 + dy;
        glyph(p, k, Rect::from_center_size(Pos2::new(b.center().x, top + 9.0), Vec2::splat(18.0)), fg);
        text_fit(p, Pos2::new(b.center().x, top + 22.0 + 8.0), Align2::CENTER_CENTER, a.label, font(12.0), fg, b.width());
        if !a.sub.is_empty() {
            text_fit(p, Pos2::new(b.center().x, top + 38.0 + 7.0), Align2::CENTER_CENTER, a.sub, ty::caption(), sub_col, b.width());
        }
        let resp = if a.tip.is_empty() { key.resp } else { key.resp.on_hover_text(a.tip) };
        if resp.clicked() {
            out = Some(k);
        }
    }
    out
}

/// Row (1:4) of a list widget (W5: Clipboard, Dictation): the left part is
/// the 1:1 layout (glyph on top, name and count at the bottom) so the list
/// on the right gets most of the width. Returns the right part.
#[allow(clippy::too_many_arguments)]
pub fn row_split_list(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, title: &str, sub: &str, selected: bool) -> (Response, Rect) {
    // the same left part and divider as every Row (W5)
    row_split_full(ui, rect, id, glyph, color, None, title, sub, "", ty::caption_color(), selected)
}

/// Name column of a Row's right part (W5), the same in every Row: fits
/// "Applications" and "Compressed" in SF at 12 (about 66 pt, light weight
/// a little wider); a longer name fades.
pub const ROW_NAME: f32 = 70.0;
/// Value column of a Row's right part (W5), the same in every Row: fits
/// "65.6 GB" (about 40 pt), leaving the bar 26 pt in the 160 pt right part.
pub const ROW_VALUE: f32 = 44.0;

/// The lines of a Row's right part (W5): name, bar, value in three columns
/// shared by every Row, so bars start and end on the same two lines across
/// Rows. A longer name or value pushes its own bar in rather than being cut.
/// Up to three lines; `empty` is shown when there are none.
pub fn lines(p: &egui::Painter, r: Rect, lines: &[(String, Option<(f32, Color32)>, String)], empty: &str) {
    if lines.is_empty() {
        text_fit(p, r.center(), Align2::CENTER_CENTER, empty, font(11.0), TEXT_FAINT(), r.width());
        return;
    }
    let n = lines.len().min(3).min(((r.height() / 18.0).floor() as usize).max(1));
    let lh = (r.height() / n as f32).min(22.0);
    let top = r.center().y - lh * n as f32 / 2.0;
    // W5: one grid for every Row, name ROW_NAME · bar · value ROW_VALUE; a
    // longer name (a device's) fades, a wider value moves the bar's end
    let w = |t: &str| p.layout_no_wrap(t.to_string(), font(12.0), TEXT()).size().x.ceil();
    let vcol = lines.iter().take(n).map(|l| w(&l.2)).fold(ROW_VALUE, f32::max);
    for (k, (label, frac, value)) in lines.iter().take(n).enumerate() {
        let lr = Rect::from_min_size(Pos2::new(r.left(), top + k as f32 * lh), Vec2::new(r.width(), lh));
        bar_line(p, lr, label, *frac, value, (ROW_NAME, vcol));
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
    // W6: the note on the right is a caption
    let nr = text_fit(p, Pos2::new(x1, top + 8.0), Align2::RIGHT_CENTER, note, ty::caption(), ty::caption_color(), (x1 - x0) * 0.5);
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
    let gap = 2.0;
    let n = values.len().min(((r.width() + gap) / (3.0 + gap)) as usize).max(1);
    let bw = (r.width() - gap * (n as f32 - 1.0)) / n as f32;
    for (k, v) in values[values.len() - n..].iter().enumerate() {
        let h = ((v / hi) * r.height()).max(2.0);
        let x = r.left() + k as f32 * (bw + gap);
        // one series, one colour: a burst is not a warning (C4)
        let c = with_alpha(color, 0.8);
        p.rect_filled(Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + bw, r.bottom())), egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, c);
    }
}

/// A history graph with its time span under it ("2 min ago" … "now", W5),
/// for widgets and the Mac Health page alike. `top` is the top of the scale
/// (None: the highest value). The captions take the bottom 13 pt of `r`.
pub fn spark_span(p: &egui::Painter, r: Rect, values: &[f32], color: Color32, top: Option<f32>) {
    let gr = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - 13.0));
    match top {
        Some(t) => spark_to(p, gr, values, color, t),
        None => spark(p, gr, values, color, None),
    }
    let span = values.len() as u64 * crate::kit::mechanics::MONITOR_REFRESH_MS / 1000;
    let start = if span >= 60 { format!("{} min ago", span / 60) } else { format!("{span} s ago") };
    p.text(Pos2::new(r.left(), r.bottom()), Align2::LEFT_BOTTOM, start, ty::caption(), ty::caption_color());
    p.text(Pos2::new(r.right(), r.bottom()), Align2::RIGHT_BOTTOM, "now", ty::caption(), ty::caption_color());
}

/// The shared columns of a Medium or Large widget's list (W5): the widest
/// name and the widest value of its lines, so every bar in the card starts
/// and ends on the same two lines.
pub fn bar_cols(p: &egui::Painter, lines: &[(&str, &str)]) -> (f32, f32) {
    let w = |t: &str| p.layout_no_wrap(t.to_string(), font(12.0), TEXT()).size().x;
    let lw = lines.iter().map(|l| w(l.0)).fold(0.0f32, f32::max);
    let vw = lines.iter().map(|l| w(l.1)).fold(0.0f32, f32::max);
    (lw.ceil(), vw.ceil())
}

/// A list line inside a Medium or Large widget: name, bar, value, on the
/// card's shared columns from [`bar_cols`]. The value is never cut; the bar
/// takes what's between the columns.
pub fn bar_line(p: &egui::Painter, r: Rect, label: &str, frac: Option<(f32, Color32)>, value: &str, cols: (f32, f32)) {
    let (lw, vw) = cols;
    let cy = r.center().y;
    let vr = p.text(Pos2::new(r.right(), cy), Align2::RIGHT_CENTER, value, font(12.0), TEXT());
    let x0 = r.left() + lw + 10.0;
    let x1 = r.right() - vw - 10.0;
    if let (Some((f, c)), true) = (frac, x1 - x0 >= 24.0) {
        bar(p, Rect::from_min_max(Pos2::new(x0, cy - 2.0), Pos2::new(x1, cy + 2.0)), f, c);
    }
    // the name column is the widest name, so a name fits; without a bar it may run on
    let room = if frac.is_some() { lw.min(vr.left() - r.left() - 8.0) } else { vr.left() - r.left() - 8.0 };
    text_fade(p, Pos2::new(r.left(), cy), label, font(12.0), white(166), room);
}

/// A quiet line where the data would be (W11): no frame round it.
pub fn empty_note(p: &egui::Painter, r: Rect, msg: &str) {
    text_fit(p, r.center(), Align2::CENTER_CENTER, msg, font(12.0), TEXT_FAINT(), r.width() - 16.0);
}

/// A widget whose data hasn't arrived yet (W9): a still skeleton in its
/// shape, with its glyph and name. Nothing repaints for it.
pub fn loading(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, title: &str) -> Response {
    let resp = card(ui, rect, id, false);
    let p = ui.painter();
    let bar = |r: Rect| {
        p.rect_filled(r, r.height() / 2.0, white(18));
    };
    let inner = rect.shrink(PAD);
    let glyph_col = with_alpha(lift(color), 0.7);
    if rect.height() <= 130.0 && rect.width() <= 130.0 {
        // Small: glyph on top, name at the bottom, a bar for the value
        icon(p, glyph, Rect::from_min_size(rect.min + Vec2::new(SMALL_SIDE, PAD), Vec2::splat(20.0)), glyph_col);
        text_fit(p, Pos2::new(rect.left() + SMALL_SIDE, rect.bottom() - 26.0), Align2::LEFT_BOTTOM, title, bold(12.0), TEXT_DIM(), rect.width() - 2.0 * SMALL_SIDE);
        bar(Rect::from_min_size(Pos2::new(rect.left() + SMALL_SIDE, rect.bottom() - 20.0), Vec2::new(inner.width() * 0.6, 8.0)));
    } else if rect.height() <= 130.0 {
        // Wide / Row: the glyph in its 40 box, name and a bar beside it
        let x = visual(p, rect, glyph, with_alpha(color, 0.7), None);
        let w = (rect.right() - PAD - x).min(160.0);
        text_fit(p, Pos2::new(x, rect.center().y - 4.0), Align2::LEFT_BOTTOM, title, ty::label(), ty::caption_color(), w);
        bar(Rect::from_min_size(Pos2::new(x, rect.center().y + 2.0), Vec2::new(w * 0.75, 10.0)));
    } else {
        // Medium and up: the name, then bars down the card
        text_fit(p, Pos2::new(inner.left(), inner.top() + 9.0), Align2::LEFT_CENTER, title, ty::title(), TEXT_DIM(), inner.width());
        let mut y = inner.top() + 32.0;
        let widths = [0.7, 0.45, 0.85, 0.55, 0.65, 0.4];
        let mut k = 0;
        while y + 10.0 <= inner.bottom() + 0.5 && k < widths.len() {
            bar(Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width() * widths[k], 10.0)));
            y += 22.0;
            k += 1;
        }
    }
    resp
}

#[cfg(test)]
mod row_grid_tests {
    use super::*;

    #[test]
    fn row_grid_leaves_room_for_a_bar() {
        // the 1:4 right part: 380 panel, 12 margins, divider at ROW_SPLIT, PAD each side
        let right = crate::kit::mechanics::PANEL_WIDTH - 2.0 * 12.0 - ROW_SPLIT - 2.0 * PAD;
        assert_eq!(right, 160.0);
        assert_eq!(ROW_SPLIT, 172.0);
        assert!(right - ROW_NAME - ROW_VALUE - 2.0 * 10.0 >= 24.0, "bar under 24 pt");
    }
}
