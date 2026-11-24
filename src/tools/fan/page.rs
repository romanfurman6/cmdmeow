//! The Fan Control page in the full window: a big dial per fan with
//! spinning blades, a heat map of the chip, the fan mode (with the speed
//! slider and curve editor) and the last 10 minutes of speed and temperature.

use super::control::{speed, Mode, Preset, Settings, Source, CURVE_HI, CURVE_LO, RESUME_C};
use super::data::{heat, heat_cell, heat_text, rpm_text, Fan, Snapshot, HISTORY};
use super::{Cmd, HelperState, Live};
use super::ui::{blades, fan_name, heat_pos, heat_scale, is_block, sparkline, BLADE, HOT_LINE, LINE};
use crate::core::tool::Cx;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, Vec2};

const GAP: f32 = 12.0;
const ROW1_H: f32 = 244.0;
/// The fan's stat ring (SP2) across.
const RING_D: f32 = 112.0;

/// Card title (13/600) on the same baseline as on Mac Health.
fn card_title(p: &egui::Painter, inner: Rect, title: &str) {
    text_fit(p, Pos2::new(inner.left(), inner.top() + 26.0), Align2::LEFT_BOTTOM, title, ty::title(), TEXT(), inner.width());
}

pub fn page(s: &Snapshot, l: &Live, ui: &mut Ui, r: Rect, _cx: &mut Cx) -> Vec<Cmd> {
    let p = ui.painter().clone();
    page_header(&p, r, ts!("fan.name"), if s.loaded { "" } else { ts!("fan.reading_sensors") }, TEXT_DIM());

    let x0 = r.left() + 24.0;
    let x1 = r.right() - 24.0;
    let mut y = r.top() + PAGE_HEADER_H + 16.0;

    // row 1: fans + heat map
    let n = s.fans.len().min(2);
    let fan_w = 200.0;
    let mut hx = x0;
    if n > 0 {
        for (i, f) in s.fans.iter().take(n).enumerate() {
            let fr = Rect::from_min_size(Pos2::new(x0 + i as f32 * (fan_w + GAP), y), Vec2::new(fan_w, ROW1_H));
            data_card(&p, fr);
            let inner = fr.shrink(sp::L);
            let title = if s.fans.len() == 1 { ts!("fan.fan").to_string() } else { t!("fan.named_fan", name = fan_name(i, s.fans.len())).into_owned() };
            card_title(&p, inner, &title);
            // SP2: the shared stat ring, the turning glyph inside it (SP11)
            let c = Pos2::new(fr.center().x, inner.top() + 28.0 + sp::S + RING_D / 2.0);
            stat_ring(&p, c, RING_D, f.frac(), tint::FAN(), "", "");
            crate::kit::spin::blades(&p, &format!("fan-{i}"), c, 34.0, f.rpm, BLADE());
            let g = p.layout_no_wrap(rpm_text(f.rpm), ty::hero(), TEXT());
            let u = p.layout_no_wrap(ts!("fan.rpm").into(), ty::label(), TEXT_DIM());
            let tw = g.size().x + 4.0 + u.size().x;
            let base = c.y + RING_D / 2.0 + sp::S;
            let gx = fr.center().x - tw / 2.0;
            let gh = g.size().y;
            p.galley(Pos2::new(gx, base), g.clone(), TEXT());
            p.galley(Pos2::new(gx + g.size().x + 4.0, base + gh * 0.80 - u.size().y * 0.85), u, TEXT_DIM());
            p.text(Pos2::new(fr.center().x, inner.bottom()), Align2::CENTER_BOTTOM, t!("fan.rpm_range", min = rpm_text(f.min), max = rpm_text(f.max)).into_owned(), ty::caption(), ty::caption_color());
        }
        hx = x0 + n as f32 * (fan_w + GAP);
    } else if s.loaded {
        let fr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(fan_w * 2.0 + GAP, ROW1_H));
        data_card(&p, fr);
        let c = Pos2::new(fr.left() + 56.0, fr.top() + 64.0);
        crate::kit::glass::stroke_circle(&p, c, 32.0, Stroke::new(1.5, white(30)));
        blades(&p, c, 24.0, 0.3, white(40));
        let title = if s.has_smc { ts!("fan.no_fans_title") } else { ts!("fan.unavailable_title") };
        let (ly1, ly2) = two_lines_y(&p, c.y, &bold(15.0), &font(12.0));
        p.text(Pos2::new(fr.left() + 104.0, ly1), Align2::LEFT_CENTER, title, bold(15.0), TEXT());
        p.text(Pos2::new(fr.left() + 104.0, ly2), Align2::LEFT_CENTER, ts!("fan.cools_passively"), font(12.0), TEXT_DIM());
        hx = fr.right() + GAP;
    }
    let hm = Rect::from_min_max(Pos2::new(hx, y), Pos2::new(x1, y + ROW1_H));
    heat_map(&p, hm, s);
    y += ROW1_H + 14.0;

    // row 2: mode
    let mut cmds = Vec::new();
    let mr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, mode_height(s, l)));
    mode_card(ui, mr, s, l, &mut cmds);
    y = mr.bottom() + 14.0;

    // row 3: history
    let hr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, r.bottom() - 18.0));
    if hr.height() > 90.0 {
        history(&p, hr, s);
    }
    cmds
}

const MODE_HINTS: [&str; 5] = [""; 5];
const INSTALL_H: f32 = 52.0;
const FIXED_H: f32 = 40.0;
const CURVE_H: f32 = 222.0;

fn mode_height(s: &Snapshot, l: &Live) -> f32 {
    if s.fans.is_empty() {
        return 64.0;
    }
    let st = &l.settings;
    let hint = (st.mode != Mode::Auto && l.guard.tripped) || st.mode == Mode::Curve;
    let mut h = if hint { 76.0 } else { 56.0 };
    if l.needs_helper() || !l.error.is_empty() {
        h += INSTALL_H;
    }
    match l.settings.mode {
        Mode::Fixed => h += FIXED_H,
        Mode::Curve => h += CURVE_H,
        _ => {}
    }
    h
}

/// The mode card: Auto / Quiet / Fixed / Curve / Max, the helper's state,
/// and for Fixed a speed slider, for Curve the curve editor.
fn mode_card(ui: &mut Ui, mr: Rect, s: &Snapshot, l: &Live, cmds: &mut Vec<Cmd>) {
    let p = ui.painter().clone();
    data_card(&p, mr);
    let lx = mr.left() + sp::L;
    let rx = mr.right() - sp::L;
    if s.fans.is_empty() {
        p.text(Pos2::new(lx, mr.top() + 20.0), Align2::LEFT_CENTER, ts!("fan.mode_title"), bold(13.0), TEXT());
        let chip = Rect::from_min_size(Pos2::new(mr.left() + 62.0, mr.top() + 8.0), Vec2::new(64.0, 24.0));
        p.rect_filled(chip, 7.0, white(16));
        p.text(chip.center(), Align2::CENTER_CENTER, ts!("fan.passive"), bold(12.0), TEXT());
        text_fit(&p, Pos2::new(lx, mr.top() + 46.0), Align2::LEFT_CENTER, ts!("fan.passive_hint"), font(12.0), TEXT_DIM(), mr.width() - 28.0);
        return;
    }
    let st = &l.settings;
    let fan0 = s.fans[0].clone();
    // title + modes + helper state
    let ty = mr.top() + 24.0;
    let tr = p.text(Pos2::new(lx, ty), Align2::LEFT_CENTER, ts!("fan.mode_title"), ty::title(), TEXT());
    let labels: Vec<&str> = Mode::ALL.iter().map(|m| m.label()).collect();
    let seg = Rect::from_min_size(Pos2::new(tr.right() + sp::L, ty - 14.0), Vec2::new(360.0f32.min(mr.width() - 300.0), 28.0));
    if let Some(i) = segmented(ui, seg, ui.scope_id().with("fan-mode-page"), &labels, st.mode.index()) {
        let mut n = st.clone();
        n.mode = Mode::ALL[i];
        cmds.push(Cmd::Set(n, true));
    }
    // the helper only shows up when it can be removed; problems get the
    // install box below
    if matches!(l.helper, HelperState::Ready | HelperState::Outdated) && !l.busy {
        // a named button, not a lone icon (B labels)
        let label = ts!("fan.remove_helper");
        let bw = button_width(&p, label, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(rx - bw, ty - BTN_H_ROW / 2.0), Vec2::new(bw, BTN_H_ROW));
        if pill_button(ui, br, label, BTN_SECONDARY, true).clicked() {
            cmds.push(Cmd::Uninstall);
        }
    }

    // what the mode does now
    let mut y = mr.top() + 48.0;
    let temp = st.curve.source.read(s);
    let hint = if st.mode != Mode::Auto && l.guard.tripped {
        t!("fan.guard_hint", resume = RESUME_C : {:.0}, peak = l.guard.peak : {:.0}).into_owned()
    } else if st.mode == Mode::Curve {
        match temp {
            Some(t) => t!("fan.curve_hint", temp = t : {:.0}, rpm = rpm_text(speed(&fan0, st.curve.at(t)))).into_owned(),
            None => ts!("fan.no_temperature").into(),
        }
    } else {
        MODE_HINTS[st.mode.index()].into()
    };
    let hint_col = if l.guard.tripped && st.mode != Mode::Auto { AMBER() } else { TEXT_DIM() };
    if !hint.is_empty() {
        text_fit(&p, Pos2::new(lx, y + 8.0), Align2::LEFT_CENTER, &hint, ty::label(), hint_col, rx - lx);
        y += 20.0;
    }

    // the helper, when the mode needs it
    if l.needs_helper() || !l.error.is_empty() {
        // a line of the card under a hairline, not a box inside it (S2)
        let ir = Rect::from_min_size(Pos2::new(lx, y), Vec2::new(rx - lx, INSTALL_H - 8.0));
        hairline(&p, Pos2::new(mr.left(), ir.top()), Pos2::new(mr.right(), ir.top()));
        let (label, msg) = match l.helper {
            _ if l.busy => (ts!("fan.waiting"), ts!("fan.enter_password")),
            HelperState::Outdated => (ts!("fan.update_helper"), ts!("fan.helper_outdated")),
            _ => (ts!("fan.install_helper"), ts!("fan.helper_needed")),
        };
        let bw = button_width(&p, label, BTN_PRIMARY);
        let br = Rect::from_min_size(Pos2::new(ir.right() - bw, ir.center().y - BTN_H_ROW / 2.0 + 4.0), Vec2::new(bw, BTN_H_ROW));
        let (msg, mcol) = if l.error.is_empty() { (msg.to_string(), TEXT_SOFT()) } else { (t!("fan.failed", error = &l.error).into_owned(), RED()) };
        text_fit(&p, Pos2::new(ir.left(), br.center().y), Align2::LEFT_CENTER, &msg, ty::label(), mcol, ir.width() - bw - sp::L);
        if l.needs_helper() && pill_button(ui, br, label, BTN_PRIMARY, !l.busy).clicked() {
            cmds.push(Cmd::Install);
        }
        y += INSTALL_H;
    }

    match st.mode {
        Mode::Fixed => {
            fixed_slider(ui, Rect::from_min_size(Pos2::new(lx, y), Vec2::new(rx - lx, FIXED_H - 8.0)), st, &fan0, cmds);
        }
        Mode::Curve => {
            curve_editor(ui, Rect::from_min_size(Pos2::new(lx, y), Vec2::new(rx - lx, CURVE_H - 12.0)), st, &fan0, temp, cmds);
        }
        _ => {}
    }

}

/// "Speed" with a slider over the fan's range and the rpm it gives.
fn fixed_slider(ui: &mut Ui, r: Rect, st: &Settings, fan: &Fan, cmds: &mut Vec<Cmd>) {
    let p = ui.painter().clone();
    let cy = r.center().y;
    p.text(Pos2::new(r.left(), cy), Align2::LEFT_CENTER, ts!("fan.speed"), font(12.0), TEXT_DIM());
    let value = t!("fan.rpm_value", rpm = rpm_text(speed(fan, st.fixed))).into_owned();
    let vr = p.text(Pos2::new(r.right(), cy), Align2::RIGHT_CENTER, value, bold(13.0), TEXT());
    let track = Rect::from_min_max(Pos2::new(r.left() + 56.0, cy - 3.0), Pos2::new(vr.left() - 18.0, cy + 3.0));
    let hit = track.expand2(Vec2::new(8.0, 12.0));
    let resp = ui.interact(hit, ui.scope_id().with("fan-fixed"), egui::Sense::click_and_drag());
    let mut v = st.fixed;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() || resp.drag_stopped() {
            v = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        }
    }
    p.rect_filled(track, 3.0, white(24));
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * v, track.bottom())), 3.0, LINE());
    let knob = Pos2::new(track.left() + track.width() * v, cy);
    p.circle_filled(knob, 9.0, Color32::WHITE);
    crate::kit::glass::stroke_circle(&p, knob, 9.0, Stroke::new(1.0, with_alpha(Color32::BLACK, 0.3)));
    p.text(Pos2::new(track.left(), cy + 15.0), Align2::LEFT_CENTER, rpm_text(fan.min), font(11.0), TEXT_FAINT());
    p.text(Pos2::new(track.right(), cy + 15.0), Align2::RIGHT_CENTER, rpm_text(fan.max), font(11.0), TEXT_FAINT());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if (v - st.fixed).abs() > f32::EPSILON || resp.drag_stopped() || resp.clicked() {
        let mut n = st.clone();
        n.fixed = v;
        cmds.push(Cmd::Set(n, resp.drag_stopped() || resp.clicked()));
    }
}

/// The curve: temperature across, fan speed up, three draggable points,
/// the live reading as a white dot, and Auto above 95 °C. Presets and the
/// temperature to follow on the right.
fn curve_editor(ui: &mut Ui, r: Rect, st: &Settings, fan: &Fan, temp: Option<f32>, cmds: &mut Vec<Cmd>) {
    let p = ui.painter().clone();
    let side_w = 230.0;
    let chart = Rect::from_min_max(Pos2::new(r.left() + 44.0, r.top() + 6.0), Pos2::new(r.right() - side_w - 24.0, r.bottom() - 20.0));
    const T_MAX: f32 = 100.0;
    let x_of = |t: f32| chart.left() + (t - CURVE_LO) / (T_MAX - CURVE_LO) * chart.width();
    let y_of = |f: f32| chart.bottom() - f * chart.height();
    // grid lines only, no plate under the chart (S2)
    for k in 0..=4 {
        let yy = y_of(k as f32 / 4.0);
        p.line_segment([Pos2::new(chart.left(), yy), Pos2::new(chart.right(), yy)], Stroke::new(1.0, white(10)));
        p.text(Pos2::new(chart.left() - 6.0, yy), Align2::RIGHT_CENTER, rpm_text(speed(fan, k as f32 / 4.0)), font(11.0), TEXT_FAINT());
    }
    for t in (30..=100).step_by(10) {
        p.text(Pos2::new(x_of(t as f32), chart.bottom() + 10.0), Align2::CENTER_CENTER, t!("unit.degrees", t = t : {:.0}).into_owned(), font(11.0), TEXT_FAINT());
    }
    // Auto zone
    let zone = Rect::from_min_max(Pos2::new(x_of(CURVE_HI), chart.top()), chart.max);
    p.rect_filled(zone, 0.0, with_alpha(RED(), 0.10));
    p.text(Pos2::new(zone.center().x, chart.top() + 10.0), Align2::CENTER_CENTER, ts!("fan.mode.auto"), bold(11.0), with_alpha(RED(), 0.9));
    // the line and its fill
    let c = &st.curve;
    let mut pts = vec![Pos2::new(x_of(CURVE_LO), y_of(c.pts[0].1))];
    pts.extend(c.pts.iter().map(|(t, f)| Pos2::new(x_of(*t), y_of(*f))));
    pts.push(Pos2::new(x_of(CURVE_HI), y_of(c.pts[2].1)));
    let mut fill = pts.clone();
    fill.push(Pos2::new(x_of(CURVE_HI), chart.bottom()));
    fill.push(Pos2::new(x_of(CURVE_LO), chart.bottom()));
    let mut mesh = egui::epaint::Mesh::default();
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        let col = with_alpha(LINE(), 0.18);
        mesh.colored_vertex(w[0], col);
        mesh.colored_vertex(w[1], col);
        mesh.colored_vertex(Pos2::new(w[1].x, chart.bottom()), col);
        mesh.colored_vertex(Pos2::new(w[0].x, chart.bottom()), col);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    p.add(egui::Shape::line(pts, Stroke::new(2.5, LINE())));
    // live reading
    if let Some(t) = temp {
        let tx = t.clamp(CURVE_LO, T_MAX);
        let (dx, dy) = (x_of(tx), y_of(c.at(t)));
        p.add(egui::Shape::dashed_line(&[Pos2::new(dx, chart.bottom()), Pos2::new(dx, dy)], Stroke::new(1.0, white(70)), 3.0, 3.0));
        p.circle_filled(Pos2::new(dx, dy), 6.0, Color32::WHITE);
        crate::kit::glass::stroke_circle(&p, Pos2::new(dx, dy), 9.0, Stroke::new(3.0, with_alpha(Color32::WHITE, 0.2)));
    }
    // handles
    let mut n = st.clone();
    let mut changed = false;
    let mut done = false;
    for i in 0..3 {
        let hp = Pos2::new(x_of(c.pts[i].0), y_of(c.pts[i].1));
        let resp = ui.interact(Rect::from_center_size(hp, Vec2::splat(24.0)), ui.scope_id().with(("fan-pt", i)), egui::Sense::drag());
        if resp.dragged() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let t = CURVE_LO + (pos.x - chart.left()) / chart.width() * (T_MAX - CURVE_LO);
                let f = (chart.bottom() - pos.y) / chart.height();
                n.curve.drag(i, t, f);
                changed = true;
            }
        }
        done |= resp.drag_stopped();
        let hot = resp.hovered() || resp.dragged();
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            p.text(hp + Vec2::new(0.0, -18.0), Align2::CENTER_BOTTOM, t!("fan.curve_point", t = c.pts[i].0 : {:.0}, rpm = rpm_text(speed(fan, c.pts[i].1))).into_owned(), bold(11.0), TEXT());
        }
        p.circle_filled(hp, 7.5, BG());
        crate::kit::glass::stroke_circle(&p, hp, 7.5, Stroke::new(3.0, if hot { Color32::WHITE } else { LINE() }));
    }

    // side: presets and source
    let sx = r.right() - side_w;
    let mut sy = r.top() + 4.0;
    p.text(Pos2::new(sx, sy), Align2::LEFT_TOP, ts!("fan.presets"), bold(12.0), TEXT());
    sy += 20.0;
    let names: Vec<&str> = Preset::ALL.iter().map(|p| p.label()).collect();
    let cur = c.preset().map(|k| Preset::ALL.iter().position(|x| *x == k).unwrap_or(0)).unwrap_or(usize::MAX);
    if let Some(i) = segmented(ui, Rect::from_min_size(Pos2::new(sx, sy), Vec2::new(side_w, 28.0)), ui.scope_id().with("fan-preset"), &names, cur) {
        n.curve.pts = Preset::ALL[i].points();
        changed = true;
        done = true;
    }
    sy += 42.0;
    p.text(Pos2::new(sx, sy), Align2::LEFT_TOP, ts!("fan.follow"), bold(12.0), TEXT());
    sy += 20.0;
    let srcs: Vec<&str> = Source::ALL.iter().map(|s| s.label()).collect();
    let si = Source::ALL.iter().position(|x| *x == c.source).unwrap_or(0);
    if let Some(i) = segmented(ui, Rect::from_min_size(Pos2::new(sx, sy), Vec2::new(side_w, 28.0)), ui.scope_id().with("fan-source"), &srcs, si) {
        n.curve.source = Source::ALL[i];
        changed = true;
        done = true;
    }
    sy += 44.0;
    p.text(Pos2::new(sx, sy), Align2::LEFT_TOP, ts!("fan.auto_above"), font(12.0), TEXT_SOFT());
    if changed || done {
        cmds.push(Cmd::Set(n, done));
    }
}

/// The chip heat map: one grid of cells per core group (P-cores, E-cores,
/// GPU), each cell a sensor coloured on the heat scale, straight on the card
/// (S2: no frame inside a frame); the other parts as name · bar · value lines
/// (W5) beside it; the scale under both.
fn heat_map(p: &egui::Painter, r: Rect, s: &Snapshot) {
    data_card(p, r);
    let inner = r.shrink(sp::L);
    card_title(p, inner, ts!("fan.heat_map"));
    let groups = s.groups();
    if groups.is_empty() {
        let msg = if s.loaded { ts!("fan.no_sensors_dot") } else { ts!("fan.reading_sensors") };
        p.text(r.center(), Align2::CENTER_CENTER, msg, ty::label(), TEXT_DIM());
        return;
    }
    let blocks: Vec<_> = groups.iter().filter(|g| is_block(g.group)).take(4).collect();
    let rest: Vec<_> = groups.iter().filter(|g| !is_block(g.group)).take(4).collect();
    let scale_h = 16.0;
    let body = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 28.0 + sp::M), Pos2::new(inner.right(), inner.bottom() - scale_h - sp::M));
    let side_w = if rest.is_empty() { 0.0 } else { 200.0 };
    let die = Rect::from_min_max(body.min, Pos2::new(body.right() - if side_w > 0.0 { side_w + 24.0 } else { 0.0 }, body.bottom()));
    if !blocks.is_empty() {
        let cols = blocks.len().min(2);
        let rows = blocks.len().div_ceil(cols);
        let (gx, gy) = (24.0, sp::M);
        let bw = (die.width() - gx * (cols as f32 - 1.0)) / cols as f32;
        let bh = (die.height() - gy * (rows as f32 - 1.0)) / rows as f32;
        for (k, g) in blocks.iter().enumerate() {
            let (cx, cy) = (k % cols, k / cols);
            let b = Rect::from_min_size(Pos2::new(die.left() + cx as f32 * (bw + gx), die.top() + cy as f32 * (bh + gy)), Vec2::new(bw, bh));
            let hy = b.top() + 8.0;
            p.text(Pos2::new(b.left(), hy), Align2::LEFT_CENTER, g.group.label(), ty::label(), white(166));
            p.text(Pos2::new(b.right(), hy), Align2::RIGHT_CENTER, t!("unit.celsius", c = g.max : {:.0}).into_owned(), ty::label(), heat_text(g.max));
            let cells = Rect::from_min_max(Pos2::new(b.left(), b.top() + 16.0 + sp::S), b.max);
            let nc = g.cells.len().max(1);
            let ccols = ((nc as f32 * cells.width() / cells.height().max(1.0)).sqrt().ceil() as usize).clamp(1, nc);
            let crows = nc.div_ceil(ccols);
            let gap = 3.0;
            let cw = (cells.width() - gap * (ccols as f32 - 1.0)) / ccols as f32;
            let ch = (cells.height() - gap * (crows as f32 - 1.0)) / crows as f32;
            for (i, c) in g.cells.iter().enumerate() {
                let cr = Rect::from_min_size(Pos2::new(cells.left() + (i % ccols) as f32 * (cw + gap), cells.top() + (i / ccols) as f32 * (ch + gap)), Vec2::new(cw, ch));
                p.rect_filled(cr, 3.0, heat_cell(*c));
            }
        }
    } else {
        p.text(die.center(), Align2::CENTER_CENTER, ts!("fan.no_core_sensors"), ty::label(), TEXT_DIM());
    }
    if side_w > 0.0 {
        let side = Rect::from_min_max(Pos2::new(body.right() - side_w, body.top()), body.max);
        let lines: Vec<(String, String)> = rest.iter().map(|g| (g.group.label().to_string(), t!("unit.celsius", c = g.max : {:.0}).into_owned())).collect();
        let cols = crate::kit::tile::bar_cols(p, &lines.iter().map(|l| (l.0.as_str(), l.1.as_str())).collect::<Vec<_>>());
        for (i, g) in rest.iter().enumerate() {
            let lr = Rect::from_min_size(Pos2::new(side.left(), side.top() + i as f32 * sp::LEGEND_H), Vec2::new(side.width(), sp::LEGEND_H));
            crate::kit::tile::bar_line(p, lr, &lines[i].0, Some((heat_pos(g.max).max(0.03), heat(g.max))), &lines[i].1, cols);
        }
    }
    let ly = inner.bottom() - scale_h / 2.0;
    let lo = p.text(Pos2::new(inner.left(), ly), Align2::LEFT_CENTER, t!("unit.celsius", c = 30.0 : {:.0}).into_owned(), ty::caption(), ty::caption_color());
    let hi = p.text(Pos2::new(inner.right(), ly), Align2::RIGHT_CENTER, t!("unit.celsius", c = 100.0 : {:.0}).into_owned(), ty::caption(), ty::caption_color());
    let sr = Rect::from_min_max(Pos2::new(lo.right() + sp::S, ly - 2.0), Pos2::new(hi.left() - sp::S, ly + 2.0));
    heat_scale(p, sr);
}

/// Last 10 minutes: average fan speed (filled) and the hottest sensor.
fn history(p: &egui::Painter, r: Rect, s: &Snapshot) {
    data_card(p, r);
    let inner = r.shrink(sp::L);
    card_title(p, inner, ts!("fan.last_10_min"));
    let ty = inner.top() + 18.0;
    let mut lx = inner.right();
    let legend: Vec<(&str, Color32)> = if s.fans.is_empty() { vec![(ts!("fan.hottest_sensor"), HOT_LINE())] } else { vec![(ts!("fan.hottest_sensor"), HOT_LINE()), (ts!("fan.fan_speed_avg"), LINE())] };
    for (label, c) in legend {
        let tr = p.text(Pos2::new(lx, ty), Align2::RIGHT_CENTER, label, ty::label(), TEXT_DIM());
        p.rect_filled(Rect::from_center_size(Pos2::new(tr.left() - 12.0, ty), Vec2::new(12.0, 3.0)), 1.5, c);
        lx = tr.left() - 30.0;
    }
    let cr = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 28.0 + sp::M), Pos2::new(inner.right(), inner.bottom() - 13.0 - sp::S));
    for k in 1..4 {
        let yy = cr.top() + cr.height() * k as f32 / 4.0;
        p.line_segment([Pos2::new(cr.left(), yy), Pos2::new(cr.right(), yy)], Stroke::new(1.0, white(10)));
    }
    p.line_segment([Pos2::new(cr.left(), cr.bottom() + 0.5), Pos2::new(cr.right(), cr.bottom() + 0.5)], Stroke::new(1.0, white(26)));
    // the line fills from the right as samples arrive
    let have = s.history.len();
    let frac = have as f32 / HISTORY as f32;
    let lr = Rect::from_min_max(Pos2::new(cr.right() - cr.width() * frac.max(0.01), cr.top()), cr.max);
    if have >= 2 {
        if !s.fans.is_empty() {
            // the scale follows what the fans did, as CPU load on Mac Health:
            // at idle they sit on their minimum, and a 0..max scale laid that
            // flat on the floor of the chart
            let lo = s.fans.iter().map(|f| f.min).fold(f32::MAX, f32::min).min(s.history.iter().map(|h| h.0).fold(f32::MAX, f32::min));
            let peak = s.history.iter().map(|h| h.0).fold(0.0, f32::max);
            let top = s.fans.iter().map(|f| f.max).fold(0.0, f32::max);
            let hi = (peak * 1.25).max(lo * 1.6).min(top.max(lo * 1.6));
            let rpm: Vec<f32> = s.history.iter().map(|h| h.0).collect();
            sparkline(p, lr, &rpm, lo * 0.9, hi, LINE(), Some(with_alpha(LINE(), 0.22)));
        }
        let hot: Vec<f32> = s.history.iter().map(|h| h.1).collect();
        sparkline(p, lr, &hot, 30.0, 100.0, HOT_LINE(), None);
    } else {
        p.text(cr.center(), Align2::CENTER_CENTER, ts!("fan.collecting"), font(12.0), TEXT_FAINT());
    }
    let by = inner.bottom() - 6.0;
    p.text(Pos2::new(cr.left(), by), Align2::LEFT_CENTER, ts!("fan.ten_min_ago"), font(11.0), TEXT_FAINT());
    p.text(Pos2::new(cr.right(), by), Align2::RIGHT_CENTER, ts!("fan.now"), font(11.0), TEXT_FAINT());
    // the hottest moment, marked on the line itself
    if have >= 2 {
        if let Some((i, (_, c))) = s.history.iter().enumerate().max_by(|a, b| (a.1).1.total_cmp(&(b.1).1)) {
            let x = lr.left() + lr.width() * i as f32 / (have - 1) as f32;
            let y = lr.bottom() - 1.0 - (lr.height() - 2.0) * ((c - 30.0) / 70.0).clamp(0.0, 1.0);
            let at = Pos2::new(x, y);
            p.circle_filled(at, 3.5, HOT_LINE());
            let align = if x > cr.right() - 40.0 { Align2::RIGHT_BOTTOM } else if x < cr.left() + 40.0 { Align2::LEFT_BOTTOM } else { Align2::CENTER_BOTTOM };
            p.text(at + Vec2::new(0.0, -7.0), align, t!("unit.celsius", c = c : {:.0}).into_owned(), bold(11.0), HOT_LINE());
        }
    }
}
