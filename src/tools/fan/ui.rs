//! Fan Control in the panel (quick tile, card) and its detail sheet, plus the
//! drawing pieces the page shares: spinning blades, speed dials, the heat
//! scale and sparklines.

use super::data::{heat, heat_text, rpm_text, Fan, Group, Snapshot};
use super::ID;
use crate::core::tool::{Cx, Size};
use crate::kit::tile;
use crate::kit::glass::*;
use egui::epaint::Mesh;
use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2};
use std::f32::consts::{PI, TAU};

/// Height of the card's fans-and-heat part (all of Medium).
pub const CARD_H: f32 = 168.0;
/// Blades, speed rings and lines: system blue (D6 has no per-tool colours).
#[allow(non_snake_case)]
pub fn BLADE() -> Color32 { crate::kit::glass::tint::FAN() }
#[allow(non_snake_case)]
pub fn LINE() -> Color32 { crate::kit::glass::tint::FAN() }
#[allow(non_snake_case)]
pub fn HOT_LINE() -> Color32 { crate::kit::glass::AMBER() }

/// The fan glyph standing still at `angle` (turning ones go through `kit::spin::blades`).
pub fn blades(p: &egui::Painter, c: Pos2, r: f32, angle: f32, color: Color32) {
    crate::kit::spin::draw(p, c, r, angle, color);
}

/// A full ring for small fan icons: track and filled part, starting at 12 o'clock.
pub fn ring(p: &egui::Painter, c: Pos2, r: f32, width: f32, frac: f32, color: Color32) {
    crate::kit::glass::stroke_circle(&p, c, r, Stroke::new(width, white(20)));
    let a0 = -PI / 2.0;
    let pts = arc_points(c, r, a0, a0 + TAU * frac.clamp(0.02, 1.0), 48);
    p.add(Shape::line(pts, Stroke::new(width, color)));
}

/// The heat scale from 30 °C to 100 °C: the three [`heat`] steps, 2 pt apart.
pub fn heat_scale(p: &egui::Painter, r: Rect) {
    use super::data::{HOT_C, WARM_C};
    let (w, h) = (heat_pos(WARM_C), heat_pos(HOT_C));
    stacked_bar(p, r, &[(w, heat(30.0)), (h - w, heat(WARM_C)), (1.0 - h, heat(HOT_C))]);
}

/// Where a temperature sits on [`heat_scale`], 0..1.
pub fn heat_pos(c: f32) -> f32 {
    ((c - 30.0) / 70.0).clamp(0.0, 1.0)
}

/// A line through `values` scaled between `lo` and `hi`, optionally filled below.
pub fn sparkline(p: &egui::Painter, r: Rect, values: &[f32], lo: f32, hi: f32, color: Color32, fill: Option<Color32>) {
    if values.len() < 2 {
        return;
    }
    let n = (values.len() - 1) as f32;
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let f = ((v - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
            Pos2::new(r.left() + r.width() * i as f32 / n, r.bottom() - 1.0 - (r.height() - 2.0) * f)
        })
        .collect();
    if let Some(fc) = fill {
        let mut m = Mesh::default();
        for (i, q) in pts.iter().enumerate() {
            m.colored_vertex(*q, fc);
            m.colored_vertex(Pos2::new(q.x, r.bottom()), with_alpha(fc, 0.0));
            if i > 0 {
                let b = (i as u32) * 2;
                m.add_triangle(b - 2, b - 1, b);
                m.add_triangle(b - 1, b, b + 1);
            }
        }
        p.add(Shape::mesh(m));
    }
    p.add(Shape::line(pts, Stroke::new(1.8, color)));
}

/// "Left" and "Right" for two fans, otherwise "Fan 1", "Fan 2" …
pub fn fan_name(i: usize, n: usize) -> String {
    match (n, i) {
        (1, _) => ts!("fan.fan").into(),
        (2, 0) => ts!("fan.left").into(),
        (2, 1) => ts!("fan.right").into(),
        _ => t!("fan.numbered", n = i + 1).into_owned(),
    }
}

/// "P-cores 71 °C" for the hottest group.
pub fn hottest_text(s: &Snapshot) -> Option<(String, f32)> {
    s.hottest().map(|(g, c)| (g.label().to_string(), c))
}

// ---------------------------------------------------------------- panel

/// The Fans widget at `size`; a click opens the sheet.
pub fn widget(s: &Snapshot, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    if size == Size::S24 {
        return card(s, ui, rect, cx);
    }
    if size == Size::S44 {
        card(s, ui, rect, cx);
        // every part's temperature under the card's top
        let p = ui.painter();
        let x0 = rect.left() + 12.0;
        let x1 = rect.right() - 12.0;
        let y0 = rect.top() + CARD_H + 4.0;
        hairline(p, Pos2::new(x0, y0 - 6.0), Pos2::new(x1, y0 - 6.0));
        p.text(Pos2::new(x0, y0 + 4.0), Align2::LEFT_TOP, ts!("fan.temperatures"), font(11.0), TEXT_FAINT());
        let groups = s.groups();
        let lh = 24.0;
        let fit = ((rect.bottom() - 10.0 - (y0 + 22.0)) / lh).floor().max(0.0) as usize;
        let shown: Vec<(&str, String)> = groups.iter().take(fit).map(|g| (g.group.label(), t!("unit.celsius", c = g.max : {:.0}).into_owned())).collect();
        let cols = tile::bar_cols(p, &shown.iter().map(|(l, v)| (*l, v.as_str())).collect::<Vec<_>>());
        for (i, (g, (label, v))) in groups.iter().zip(&shown).enumerate() {
            let lr = Rect::from_min_size(Pos2::new(x0, y0 + 22.0 + i as f32 * lh), Vec2::new(x1 - x0, lh));
            tile::bar_line(p, lr, label, Some((heat_pos(g.max).max(0.03), heat(g.max))), v, cols);
        }
        return;
    }
    let id = ui.scope_id().with("w-fans");
    let rpm = s.avg_rpm();
    let value = if !s.loaded {
        "…".to_string()
    } else if let Some(r) = rpm {
        t!("fan.rpm_value", rpm = rpm_text(r)).into_owned()
    } else if let Some((_, c)) = s.hottest() {
        t!("unit.celsius", c = c : {:.0}).into_owned()
    } else {
        ts!("fan.no_data").into()
    };
    let frac = s.avg_frac().unwrap_or(0.0);
    let hot = hottest_text(s).map(|(g, c)| t!("fan.part_temp", part = g, c = c : {:.0}).into_owned()).unwrap_or_default();
    // a warm part's caption takes its state colour (C12); a calm one stays a caption
    let hot_col = match s.hottest() {
        Some((_, c)) if c >= super::data::WARM_C => heat_text(c),
        _ => ty::caption_color(),
    };
    let resp = match size {
        Size::S11 => {
            let resp = tile::card(ui, rect, id, false);
            let p = ui.painter();
            let (c, rr) = tile::small_ring_at(rect);
            let w = (rr * 0.2).clamp(4.0, 6.0);
            ring(p, c, rr - w / 2.0, w, frac, LINE());
            crate::kit::spin::blades(p, "fan-avg", c, rr * 0.7, rpm.unwrap_or(0.0), BLADE());
            // W4: the value at 13/600 like every 1:1; the hottest part in the tip
            text_fit(p, Pos2::new(rect.center().x, rect.bottom() - 10.0), Align2::CENTER_BOTTOM, &value, bold(13.0), TEXT(), rect.width() - 10.0);
            if hot.is_empty() { resp } else { resp.on_hover_text(hot.clone()) }
        }
        Size::S12 => tile::wide_full(ui, rect, id, Icon::Fan, tint::FAN(), rpm.map(|_| (frac.max(0.02), LINE())), ts!("fan.widget_title"), &value, &hot, hot_col, false),
        _ => {
            // the hottest part is a line on the right; the left says how hard the fans work
            let (resp, right) = tile::row_split_full(ui, rect, id, Icon::Fan, tint::FAN(), rpm.map(|_| (frac.max(0.02), LINE())), ts!("fan.widget_title"), &value, &hot, hot_col, false);
            // each fan; the hottest part is already the caption on the left
            let lines: Vec<(String, Option<(f32, Color32)>, String)> = s.fans.iter().take(3).enumerate().map(|(i, f)| (fan_name(i, s.fans.len()), Some((f.frac().max(0.02), LINE())), t!("fan.rpm_value", rpm = rpm_text(f.rpm)).into_owned())).collect();
            let empty = if !s.loaded { ts!("fan.reading") } else if s.has_smc { ts!("fan.no_fans_passive") } else { ts!("fan.unavailable") };
            tile::lines(ui.painter(), right, &lines, empty);
            resp
        }
    };
    // spinning blades over the chip
    if let Some(r) = rpm {
        let p = ui.painter();
        // over the static glyph in the ring's centre (Wide and Row alike)
        let spot = match size {
            Size::S12 | Size::S14 => {
                let (c, rr) = tile::wide_ring(rect);
                Some((c, rr * 0.62, rr * 0.55, BG()))
            }
            _ => None,
        };
        if let Some((c, cover, br, hub)) = spot {
            p.circle_filled(c, cover, hub);
            crate::kit::spin::blades(p, "fan-avg", c, br, r, BLADE());
        }
    }
    if resp.clicked() {
        cx.open_page(ID, "");
    }
}

/// The full-width card: both fans with spinning dials, the hottest part
/// on the heat scale and its last 10 minutes.
pub fn card(s: &Snapshot, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let resp = card_resp(ui, rect, false);
    if resp.clicked() {
        cx.open_page(ID, "");
    }
    let p = ui.painter().clone();
    let t = ui.ctx().input(|i| i.time);
    let x0 = rect.left() + 12.0;
    let x1 = rect.right() - 12.0;
    let hy = rect.top() + 20.0;
    p.text(Pos2::new(x0, hy), Align2::LEFT_CENTER, ts!("fan.widget_title"), ty::title(), TEXT());
    let mode = if s.fanless() { ts!("fan.no_fans") } else { ts!("fan.mode.auto") };
    p.text(Pos2::new(x1, hy), Align2::RIGHT_CENTER, mode, font(12.0), TEXT_DIM());

    // fans
    let fy = rect.top() + 38.0;
    let fh = 52.0;
    if !s.loaded {
        spinner(&p, Pos2::new(rect.center().x, fy + fh / 2.0), 8.0, t, TEXT_DIM());
        crate::kit::glass::poll(ui.ctx());
    } else if s.fans.is_empty() {
        // a line on the card, no plate inside it (S2)
        let r = Rect::from_min_size(Pos2::new(x0, fy), Vec2::new(x1 - x0, fh));
        let msg = if s.has_smc { ts!("fan.no_fans_card") } else { ts!("fan.unavailable_here") };
        text_fit(&p, Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, msg, font(12.0), TEXT_DIM(), r.width());
    } else {
        let n = s.fans.len().min(2);
        let gap = 8.0;
        let bw = (x1 - x0 - gap * (n as f32 - 1.0)) / n as f32;
        for (i, f) in s.fans.iter().take(n).enumerate() {
            let r = Rect::from_min_size(Pos2::new(x0 + i as f32 * (bw + gap), fy), Vec2::new(bw, fh));
            mini_fan(&p, r, f, &fan_name(i, s.fans.len()));
        }
    }

    // hottest part
    let ty = fy + fh + 12.0;
    match hottest_text(s) {
        Some((g, c)) => {
            p.text(Pos2::new(x0, ty + 7.0), Align2::LEFT_CENTER, t!("fan.hottest_part", part = g).into_owned(), font(12.0), TEXT_DIM());
            p.text(Pos2::new(x1, ty + 7.0), Align2::RIGHT_CENTER, t!("unit.celsius", c = c : {:.0}).into_owned(), bold(13.0), heat_text(c));
            let br = Rect::from_min_max(Pos2::new(x0, ty + 20.0), Pos2::new(x1, ty + 26.0));
            heat_scale(&p, br);
            let mx = br.left() + br.width() * heat_pos(c);
            let mr = Rect::from_center_size(Pos2::new(mx, br.center().y), Vec2::new(4.0, 12.0));
            p.rect_filled(mr.expand(2.0), 3.0, CARD());
            p.rect_filled(mr, 2.0, Color32::WHITE);
            let hot: Vec<f32> = s.history.iter().map(|h| h.1).collect();
            let lo = hot.iter().copied().fold(f32::MAX, f32::min).min(c) - 4.0;
            let hi = hot.iter().copied().fold(f32::MIN, f32::max).max(c) + 4.0;
            let bottom = rect.bottom().min(rect.top() + CARD_H) - 10.0;
            sparkline(&p, Rect::from_min_max(Pos2::new(x0, ty + 34.0), Pos2::new(x1, bottom)), &hot, lo, hi, HOT_LINE(), None);
        }
        None if s.loaded => {
            p.text(Pos2::new(x0, ty + 7.0), Align2::LEFT_CENTER, ts!("fan.no_sensors"), font(12.0), TEXT_DIM());
        }
        None => {}
    }
}

fn card_resp(ui: &mut Ui, rect: Rect, selected: bool) -> egui::Response {
    tile::card(ui, rect, ui.scope_id().with("fan-card"), selected)
}

/// One fan in the panel card: ring with blades, name and speed, straight
/// on the card (S2: no plate per fan).
fn mini_fan(p: &egui::Painter, r: Rect, f: &Fan, name: &str) {
    let c = Pos2::new(r.left() + 18.0, r.center().y);
    ring(p, c, 17.0, 3.0, f.frac(), LINE());
    crate::kit::spin::blades(p, &format!("fan-card-{name}"), c, 12.0, f.rpm, BLADE());
    let x = c.x + 26.0;
    let (ly1, ly2) = two_lines_y(&p, r.center().y, &font(11.0), &bold(15.0));
    p.text(Pos2::new(x, ly1), Align2::LEFT_CENTER, name, font(11.0), TEXT_DIM());
    let g = p.text(Pos2::new(x, ly2), Align2::LEFT_CENTER, rpm_text(f.rpm), bold(15.0), TEXT());
    p.text(Pos2::new(g.right() + 4.0, g.bottom() - 2.0), Align2::LEFT_BOTTOM, ts!("fan.rpm"), font(11.0), TEXT_FAINT());
}

// ---------------------------------------------------------------- sheet

/// Hint line and colour for a thermal pressure level.
pub fn thermal_hint(level: u8) -> (&'static str, Color32) {
    match level {
        0 => (ts!("fan.thermal.no_slowdown"), GREEN()),
        1 => (ts!("fan.thermal.slightly"), AMBER()),
        2 => (ts!("fan.thermal.cooling"), AMBER()),
        _ => (ts!("fan.thermal.heavily"), RED()),
    }
}

/// Whether a group belongs in the heat map's cell blocks.
pub fn is_block(g: Group) -> bool {
    g.is_core()
}
