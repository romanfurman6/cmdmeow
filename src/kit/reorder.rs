//! H10: reorder rows by dragging. Rows sense `click_and_drag`; a grip shows
//! on the hovered row, the dragged row lifts, and a lime line marks where it
//! lands. One helper for the AI Usage menu and Settings.
use crate::kit::glass::*;
use egui::{Painter, Pos2, Rect, Response, Stroke, Ui, Vec2};

/// The grip (two columns of three dots) centred at `c`.
pub fn grip(p: &Painter, c: Pos2) {
    for dx in [-2.0, 2.0] {
        for dy in [-4.0, 0.0, 4.0] {
            p.circle_filled(c + Vec2::new(dx, dy), 1.0, TEXT_FAINT());
        }
    }
}

/// Call after the rows (top to bottom, equal height, touching) are drawn.
/// Draws the lifted row and the insert line while one is dragged. Returns
/// `(from, gap)` when it's dropped somewhere new: row `from` goes to the gap
/// before row `gap` (0..=rows.len()).
pub fn rows(ui: &Ui, rows: &[Response], inset: f32) -> Option<(usize, usize)> {
    let from = rows.iter().position(|r| r.dragged() || r.drag_stopped())?;
    let (first, last) = (rows.first()?.rect, rows.last()?.rect);
    let h = first.height().max(1.0);
    let y = ui.ctx().input(|i| i.pointer.latest_pos())?.y;
    let gap = ((y - first.top()) / h).round().clamp(0.0, rows.len() as f32) as usize;
    let moves = gap != from && gap != from + 1;
    if rows[from].drag_stopped() {
        return moves.then_some((from, gap));
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let p = ui.painter();
    // the lifted row: the selection shape, inset 4, radius 8
    let r = rows[from].rect;
    let lift = r.shrink2(Vec2::new(4.0, 0.0));
    p.rect_filled(lift, 8.0, white(ROW_HOVER));
    p.rect_stroke(lift, 8.0, Stroke::new(1.0, white(HAIRLINE_STRONG)), egui::StrokeKind::Inside);
    if moves {
        let gy = (first.top() + gap as f32 * h).min(last.bottom());
        p.line_segment([Pos2::new(r.left() + inset, gy), Pos2::new(r.right() - inset, gy)], Stroke::new(2.0, ACCENT()));
    }
    None
}

/// The hovered row's grip, `x` from its left edge.
pub fn hover_grip(p: &Painter, resp: &Response, x: f32) {
    if resp.hovered() || resp.dragged() {
        let r: Rect = resp.rect;
        grip(p, Pos2::new(r.left() + x, r.center().y));
    }
}
