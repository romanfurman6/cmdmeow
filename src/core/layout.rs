//! Packs widgets into the panel's grid: each widget spans 1–4 columns, rows
//! fill left to right, and a widget that doesn't fit starts the next row.
//! A widget keeps its own height; a row is as tall as its tallest widget.

use super::tool::GRID_COLUMNS;
use egui::{Pos2, Rect, Vec2};

/// Rects relative to (0, 0) for widgets given as (span, height), and the
/// total height used.
pub fn flow(items: &[(u8, f32)], width: f32, gap: f32) -> (Vec<Rect>, f32) {
    let cols = GRID_COLUMNS as f32;
    let col_w = ((width - gap * (cols - 1.0)) / cols).max(0.0);
    let mut out = Vec::with_capacity(items.len());
    let (mut col, mut y, mut row_h) = (0u8, 0.0f32, 0.0f32);
    for &(span, h) in items {
        let span = span.clamp(1, GRID_COLUMNS);
        if col + span > GRID_COLUMNS {
            y += row_h + gap;
            col = 0;
            row_h = 0.0;
        }
        let x = col as f32 * (col_w + gap);
        let w = span as f32 * col_w + (span - 1) as f32 * gap;
        out.push(Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)));
        row_h = row_h.max(h);
        col += span;
    }
    let total = if items.is_empty() { 0.0 } else { y + row_h };
    (out, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_share_a_row_and_full_starts_a_new_one() {
        let (r, h) = flow(
            &[
                (4, 70.0),
                (2, 104.0),
                (2, 90.0),
                (2, 130.0),
                (2, 112.0),
                (4, 50.0),
            ],
            412.0,
            12.0,
        );
        assert_eq!(r.len(), 6);
        assert_eq!(
            r[0],
            Rect::from_min_size(Pos2::ZERO, Vec2::new(412.0, 70.0))
        );
        // two halves side by side
        assert_eq!(r[1].min, Pos2::new(0.0, 82.0));
        assert_eq!(r[1].width(), 200.0);
        assert_eq!(r[2].min, Pos2::new(212.0, 82.0));
        // row height is the taller of the two
        assert_eq!(r[3].min.y, 82.0 + 104.0 + 12.0);
        assert_eq!(r[4].height(), 112.0);
        assert_eq!(r[5].min.y, r[3].min.y + 130.0 + 12.0);
        assert_eq!(h, r[5].max.y);
    }

    #[test]
    fn quarters_and_overflow() {
        let (r, h) = flow(
            &[(1, 40.0), (1, 40.0), (1, 40.0), (1, 40.0), (1, 40.0)],
            100.0,
            0.0,
        );
        assert_eq!(r[3].min, Pos2::new(75.0, 0.0));
        assert_eq!(r[4].min, Pos2::new(0.0, 40.0));
        assert_eq!(h, 80.0);
        // a half that doesn't fit after three quarters wraps
        let (r, _) = flow(&[(1, 10.0), (1, 10.0), (1, 10.0), (2, 10.0)], 100.0, 0.0);
        assert_eq!(r[3].min, Pos2::new(0.0, 10.0));
    }

    #[test]
    fn odd_spans_and_empty() {
        assert_eq!(flow(&[], 400.0, 12.0).1, 0.0);
        let (r, _) = flow(&[(0, 10.0), (9, 10.0)], 100.0, 0.0);
        assert_eq!(r[0].width(), 25.0); // clamped up to 1 column
        assert_eq!(r[1].width(), 100.0); // clamped down to the full row
        assert_eq!(r[1].min.y, 10.0);
    }
}
