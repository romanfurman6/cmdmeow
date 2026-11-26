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

/// Call after the rows (top to bottom, touching; heights may differ, e.g.
/// a provider with its accounts under it) are drawn. Draws the lifted row
/// and the insert line while one is dragged. Returns `(from, gap)` when
/// it's dropped somewhere new: row `from` goes to the gap before row `gap`
/// (0..=rows.len()).
pub fn rows(ui: &Ui, rows: &[Response], inset: f32) -> Option<(usize, usize)> {
    let from = rows.iter().position(|r| r.dragged() || r.drag_stopped())?;
    let y = ui.ctx().input(|i| i.pointer.latest_pos())?.y;
    let gap = gap_at(&rows.iter().map(|r| r.rect).collect::<Vec<_>>(), y);
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
        let gy = if gap == 0 { rows[0].rect.top() } else { rows[gap - 1].rect.bottom() };
        p.line_segment([Pos2::new(r.left() + inset, gy), Pos2::new(r.right() - inset, gy)], Stroke::new(2.0, ACCENT()));
    }
    None
}

/// The gap the pointer at `y` is nearest: the number of rows whose middle
/// is above it.
fn gap_at(rects: &[Rect], y: f32) -> usize {
    rects.iter().filter(|r| r.center().y < y).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_follows_rows_of_any_height() {
        let r = |t: f32, h: f32| Rect::from_min_size(Pos2::new(0.0, t), Vec2::new(100.0, h));
        // a 120-tall row (a provider and its accounts), then two 40s
        let rows = [r(0.0, 120.0), r(120.0, 40.0), r(160.0, 40.0)];
        assert_eq!(gap_at(&rows, -5.0), 0);
        assert_eq!(gap_at(&rows, 59.0), 0);
        assert_eq!(gap_at(&rows, 61.0), 1);
        assert_eq!(gap_at(&rows, 150.0), 2);
        assert_eq!(gap_at(&rows, 400.0), 3);
    }
}
