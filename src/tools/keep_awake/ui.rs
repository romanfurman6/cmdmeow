//! Drawing for Keep Awake: the cup-in-a-ring scene, the 1:1 and 1:2
//! widgets, and the page (state, the Off · Auto · On control, Today).
//!
//! The scene tells the state at a glance: a moon when the Mac may sleep,
//! a cup when it's kept awake, a ring in the mode's colour, draining in
//! amber while Auto winds down. Nothing loops: the ring is the only thing
//! that moves, and only because time passes.

use super::engine::{names, today_start, Engine, Mode, Span, State, Via, GRACE_SECS};
use super::{KeepAwake, KeyRec, ID};
use crate::core::tool::Size;
use crate::kit::glass::*;
use crate::kit::hotkeys::{listen, Heard};
use crate::kit::sync::LockExt;
use crate::kit::tile;
use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, CornerRadius, FontId, Mesh, Painter, Pos2, Rect, ScrollArea, Sense, Shape, Stroke, Ui, UiBuilder, Vec2};
use std::f32::consts::{PI, TAU};

/// On's colour: the tool's own tint (C3, C5).
#[allow(non_snake_case)]
fn ON() -> Color32 {
    series::util(0)
}
/// Auto's colour: the next lavender step.
#[allow(non_snake_case)]
fn AUTO() -> Color32 {
    series::util(1)
}

/// A Today row.
const LOG_ROW_H: f32 = sp::ROW_H;
/// The hourly chart's bars.
const CHART_H: f32 = 72.0;
/// Room right of the chart for its "60 m" and "30 m".
const CHART_LABELS_W: f32 = 36.0;

// ---------------------------------------------------------------- words

/// 4:12
fn clock(secs: i64) -> String {
    let s = secs.max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

/// 1 h 07 m, 12 m, <1 m
fn span_text(secs: i64) -> String {
    let m = secs.max(0) / 60;
    if m == 0 && secs > 0 {
        "<1 m".into()
    } else if m >= 60 {
        format!("{} h {:02} m", m / 60, m % 60)
    } else {
        format!("{m} m")
    }
}

fn time_of_day(t: i64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(t, 0).single().map(|d| d.format("%H:%M").to_string()).unwrap_or_default()
}

/// The big line: "Can sleep", "Awake", "Sleeps in 4:12".
fn title(st: State) -> String {
    match st {
        State::Off => "Can sleep".into(),
        State::On | State::Agent => "Awake".into(),
        State::WindDown { left } => format!("Sleeps in {}", clock(left)),
    }
}

/// The line under it on the page.
fn detail(e: &Engine, st: State, now: i64) -> String {
    let since = e.awake_since.map(time_of_day).unwrap_or_default();
    match st {
        State::Off if e.mode() == Mode::Auto => "No agent working".into(),
        State::Off => String::new(),
        State::Agent => format!("{} working · since {since}", names(&e.working_agents(now))),
        State::WindDown { .. } => format!("{} finished", names(&e.last_agents)),
        State::On => format!("Since {since}"),
    }
}

/// The 1:2 widget's second line, the longest that fits first.
fn detail_short(e: &Engine, st: State, now: i64) -> Vec<String> {
    let since = e.awake_since.map(time_of_day).unwrap_or_default();
    match st {
        State::Off if e.mode() == Mode::Auto => vec!["No agent working".into(), "No agent".into()],
        State::Off => vec![],
        State::Agent => {
            let a = e.working_agents(now);
            vec![names(&a), format!("{} agents", a.len())]
        }
        State::WindDown { .. } => vec![format!("{} finished", names(&e.last_agents)), "Finished".into()],
        State::On => vec![format!("Since {since}")],
    }
}

/// The first of `options` that fits `max_w`; the last one fitted.
fn text_first_fit(p: &Painter, pos: Pos2, align: Align2, options: &[String], f: FontId, color: Color32, max_w: f32) {
    let Some(last) = options.last() else { return };
    let pick = options.iter().find(|o| p.layout_no_wrap((*o).clone(), f.clone(), color).size().x <= max_w).unwrap_or(last);
    text_fit(p, pos, align, pick, f, color, max_w);
}

/// How far around the ring is filled, and its colour.
fn ring(st: State) -> (f32, Color32) {
    match st {
        State::Off => (0.0, Color32::TRANSPARENT),
        State::On => (1.0, ON()),
        State::Agent => (1.0, AUTO()),
        State::WindDown { left } => ((left as f32 / GRACE_SECS as f32).clamp(0.02, 1.0), AMBER()),
    }
}

// ---------------------------------------------------------------- the scene

/// A crescent: the part of circle (c1, r1) outside circle (c2, r2).
fn crescent(p: &Painter, c1: Pos2, r1: f32, c2: Pos2, r2: f32, color: Color32) {
    let dv = c2 - c1;
    let d = dv.length();
    if d <= 0.0 || d >= r1 + r2 || d <= (r1 - r2).abs() {
        p.circle_filled(c1, r1, color);
        return;
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let base = c1 + dv / d * a;
    let perp = Vec2::new(-dv.y, dv.x) / d;
    let (ta, tb) = (base + perp * h, base - perp * h);
    // both arcs run from tip A to tip B through the direction away from c2
    let away = (c1 - c2).angle();
    let arc = |c: Pos2, r: f32, n: usize| -> Vec<Pos2> {
        let s = (ta - c).angle();
        let e = (tb - c).angle();
        let ccw = (e - s).rem_euclid(TAU);
        let sweep = if (away - s).rem_euclid(TAU) < ccw { ccw } else { ccw - TAU };
        (0..=n).map(|i| c + Vec2::angled(s + sweep * i as f32 / n as f32) * r).collect()
    };
    let n = 40;
    let (outer, inner) = (arc(c1, r1, n), arc(c2, r2, n));
    let mut mesh = Mesh::default();
    for (o, i) in outer.iter().zip(&inner) {
        mesh.colored_vertex(*o, color);
        mesh.colored_vertex(*i, color);
    }
    for i in 0..n as u32 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    p.add(Shape::mesh(mesh));
    // meshes have no anti-aliasing; a hairline along both edges smooths them
    p.add(PathShape::line(outer, PathStroke::new(1.0, color)));
    p.add(PathShape::line(inner, PathStroke::new(1.0, color)));
}

/// The cup-in-a-ring scene in the square `rect`, on a 96 grid.
fn scene(p: &Painter, rect: Rect, st: State) {
    let s = rect.width() / 96.0;
    let at = |x: f32, y: f32| rect.min + Vec2::new(x * s, y * s);
    let c = at(48.0, 48.0);
    let rr = 42.0 * s;
    stroke_circle(p, c, rr, Stroke::new(4.0 * s, white(26)));
    let (frac, col) = ring(st);
    let a0 = -PI / 2.0;
    if frac >= 0.999 {
        stroke_circle(p, c, rr, Stroke::new(4.0 * s, col));
    } else if frac > 0.0 {
        let a1 = a0 + TAU * frac;
        p.add(PathShape::line(arc_points(c, rr, a0, a1, 72), PathStroke::new(4.0 * s, col)));
        for a in [a0, a1] {
            p.circle_filled(c + Vec2::new(a.cos(), a.sin()) * rr, 2.0 * s, col);
        }
    }

    if st == State::Off {
        // crescent moon, nothing else
        crescent(p, at(52.0, 50.0), 22.0 * s, at(62.0, 41.0), 18.5 * s, TEXT_FAINT());
        return;
    }

    // steam: still wisps, two while awake, one fainter while cooling
    let cool = matches!(st, State::WindDown { .. });
    let wisps: &[(f32, f32)] = if cool { &[(46.0, 36.0)] } else { &[(42.0, 37.0), (50.0, 37.0)] };
    for (x0, y0) in wisps.iter() {
        let pts: Vec<Pos2> = (0..=12)
            .map(|i| {
                let u = i as f32 / 12.0;
                at(x0 + 2.2 * (u * PI * 1.6).sin(), y0 - 11.0 * u)
            })
            .collect();
        p.add(PathShape::line(pts, PathStroke::new(2.5 * s, white(if cool { 51 } else { 102 }))));
    }

    // cup: straight top, rounded bottom
    let body = white(235);
    let mut cup = vec![at(31.0, 44.0), at(61.0, 44.0), at(61.0, 55.0)];
    cup.extend(arc_points(at(47.0, 55.0), 14.0 * s, 0.0, PI / 2.0, 10));
    cup.extend(arc_points(at(45.0, 55.0), 14.0 * s, PI / 2.0, PI, 10));
    p.add(Shape::convex_polygon(cup, body, Stroke::NONE));
    let mut handle = vec![at(60.0, 48.0)];
    handle.extend(arc_points(at(64.0, 54.0), 6.0 * s, -PI / 2.0, PI / 2.0, 14));
    handle.push(at(60.0, 60.0));
    p.add(PathShape::line(handle, PathStroke::new(4.0 * s, body)));
    p.line_segment([at(27.0, 74.0), at(65.0, 74.0)], Stroke::new(3.0 * s, body));
    p.circle_filled(at(27.0, 74.0), 1.5 * s, body);
    p.circle_filled(at(65.0, 74.0), 1.5 * s, body);
}

/// The Off · Auto · On control. Returns the mode picked this frame.
fn control(ui: &mut Ui, r: Rect, id: egui::Id, mode: Mode, f: FontId) -> Option<Mode> {
    let labels: Vec<&str> = Mode::ALL.iter().map(|m| m.label()).collect();
    let sel = Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
    segmented_font(ui, r, id, &labels, sel, f).map(|i| Mode::ALL[i])
}

// ---------------------------------------------------------------- panel

/// The Keep Awake widget: 1:1 (a tap turns On on or off) or 1:2 (the
/// control).
pub fn widget(k: &mut KeepAwake, size: Size, ui: &mut Ui, rect: Rect, cx: &mut crate::core::tool::Cx) {
    let now = k.now();
    let id = ui.scope_id().with("w-awake");
    let (st, mode, line2) = {
        let e = k.engine.lock_ok();
        let st = e.state(now);
        (st, e.mode(), detail_short(&e, st, now))
    };
    // 1:1 is one button and sinks; 1:2 holds the switch, so only the switch presses (W1)
    let resp = if size == Size::S11 { tile::card(ui, rect, id, false) } else { tile::frame(ui, rect, id, false) };
    let p = ui.painter().clone();
    if size == Size::S11 {
        let sr = Rect::from_min_size(Pos2::new(rect.center().x - 25.0, rect.top() + 7.0), Vec2::splat(50.0));
        scene(&p, sr, st);
        let (word, col) = match st {
            State::WindDown { left } => (clock(left), AMBER()),
            _ => (mode.label().to_string(), if mode == Mode::Off { TEXT_DIM() } else { TEXT_SOFT() }),
        };
        p.text(Pos2::new(rect.center().x, rect.bottom() - 10.0), Align2::CENTER_BOTTOM, word, bold(13.0), col);
        if resp.on_hover_text(if mode == Mode::On { "Turn off" } else { "Keep awake" }).clicked() {
            k.toggle(Via::Panel);
        }
        return;
    }
    // 1:2: the state on top, the control along the bottom
    let inner = rect.shrink2(Vec2::new(12.0, 10.0));
    let sr = Rect::from_min_size(inner.min, Vec2::splat(32.0));
    scene(&p, sr, st);
    let x = sr.right() + 8.0;
    let w = inner.right() - x;
    let tf = bold(13.0);
    if line2.is_empty() {
        text_fit(&p, Pos2::new(x, sr.center().y), Align2::LEFT_CENTER, &title(st), tf, TEXT(), w);
    } else {
        let (y1, y2) = two_lines_y(&p, sr.center().y, &tf, &font(11.0));
        text_fit(&p, Pos2::new(x, y1), Align2::LEFT_CENTER, &title(st), tf, TEXT(), w);
        text_first_fit(&p, Pos2::new(x, y2), Align2::LEFT_CENTER, &line2, font(11.0), TEXT_DIM(), w);
    }
    // W7: the panel's segmented control at its own size, 28 with 12 labels
    let cr = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - sp::CONTROL_H), inner.max);
    if let Some(m) = control(ui, cr, id.with("mode"), mode, font(12.0)) {
        k.set_mode(m, Via::Panel);
    } else if resp.clicked() {
        cx.open_page(ID, "");
    }
}

// ---------------------------------------------------------------- page

pub fn page(k: &mut KeepAwake, ui: &mut Ui, r: Rect) {
    let now = k.now();
    let p = ui.painter().clone();
    let id = ui.scope_id().with("awake-page");
    let btns = page_header(&p, r, "Keep Awake", "", TEXT_DIM());
    {
        let mut e = k.engine.lock_ok();
        shortcut(ui, &p, btns, &mut e, &mut k.keys, now, id.with("key"));
    }
    let x0 = r.left() + sp::PAGE_EDGE;
    let x1 = r.right() - sp::PAGE_EDGE;
    let mut y = r.top() + PAGE_HEADER_H + sp::XXL;

    // the state: scene, title, detail, the control. The scene grows with the
    // window, so the smallest one and full screen both breathe
    let (st, mode, det) = {
        let e = k.engine.lock_ok();
        let st = e.state(now);
        (st, e.mode(), detail(&e, st, now))
    };
    let side = (136.0 + (r.height() - 740.0) * 0.16).clamp(112.0, 184.0).round();
    let cx = (x0 + x1) / 2.0;
    scene(&p, Rect::from_min_size(Pos2::new(cx - side / 2.0, y), Vec2::splat(side)), st);
    y += side + sp::XL;
    p.text(Pos2::new(cx, y + 17.0), Align2::CENTER_CENTER, title(st), ty::hero(), TEXT());
    y += 34.0 + 6.0;
    text_fit(&p, Pos2::new(cx, y + 9.0), Align2::CENTER_CENTER, &det, ty::body(), TEXT_DIM(), x1 - x0);
    y += 18.0 + sp::XL;
    let cr = Rect::from_min_size(Pos2::new(cx - 138.0, y), Vec2::new(276.0, sp::HEADER_CONTROL_H));
    if let Some(m) = control(ui, cr, id.with("mode"), mode, font(12.0)) {
        k.set_mode(m, Via::Page);
    }
    y = cr.bottom();

    let e = k.engine.lock_ok();
    today(ui, &p, Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, r.bottom() - sp::XL)), &e, now, id.with("today"));
}

/// On's shortcut in the header: click, then press the keys (or a mouse
/// button). Esc keeps the old one, ⌫ removes it.
fn shortcut(ui: &mut Ui, p: &Painter, btns: Rect, e: &mut Engine, keys: &mut KeyRec, now: i64, id: egui::Id) {
    if keys.recording {
        keys.seen = Some(std::time::Instant::now());
        match listen(ui.ctx()) {
            Heard::Nothing => {}
            Heard::Cancel => keys.recording = false,
            Heard::Remove => {
                e.change(now, |s| s.shortcut = None);
                keys.recording = false;
            }
            Heard::Set(k) => {
                e.change(now, |s| s.shortcut = Some(k));
                keys.recording = false;
            }
        }
        ui.ctx().request_repaint();
    }
    let rec = keys.recording;
    let cur = e.settings.shortcut.clone();
    let text = if rec { "Press keys".to_string() } else { cur.as_ref().map(|s| s.display()).unwrap_or_else(|| "Record".into()) };
    let g = p.layout_no_wrap(text.clone(), font(12.0), TEXT());
    let bw = (g.size().x + 24.0).max(76.0);
    let br = Rect::from_min_max(Pos2::new(btns.right() - bw, btns.top()), btns.max);
    let resp = ui.interact(br, id, Sense::click());
    if rec {
        p.rect_filled(br, R_SMALL, with_alpha(ACTION(), 0.22));
        p.rect_stroke(br, R_SMALL, Stroke::new(1.0, ACCENT()), egui::StrokeKind::Inside);
    } else if cur.is_some() {
        p.rect_filled(br, R_SMALL, white(if resp.hovered() { 31 } else { 20 }));
        p.rect_stroke(br, R_SMALL, Stroke::new(1.0, white(26)), egui::StrokeKind::Inside);
    } else {
        dashed_rect(p, br, R_SMALL, white(60));
    }
    p.text(br.center(), Align2::CENTER_CENTER, text, font(12.0), if cur.is_some() || rec { TEXT() } else { TEXT_FAINT() });
    if resp.on_hover_text("Click, then press the keys or a mouse button. Esc cancels, ⌫ removes it.").clicked() {
        keys.recording = !rec;
        keys.seen = Some(std::time::Instant::now());
    }
    let lr = p.text(Pos2::new(br.left() - sp::S, br.center().y), Align2::RIGHT_CENTER, "Shortcut", font(12.0), TEXT_DIM());
    if let Some(err) = &keys.error {
        text_fit(p, Pos2::new(lr.left() - sp::M, br.center().y), Align2::RIGHT_CENTER, err, font(11.0), AMBER(), 260.0);
    }
}

/// Seconds kept awake in each hour of today: (On, Auto).
fn hours(spans: &[Span], day: i64) -> [(i64, i64); 24] {
    let mut out = [(0, 0); 24];
    for (h, slot) in out.iter_mut().enumerate() {
        let (a, b) = (day + h as i64 * 3600, day + (h as i64 + 1) * 3600);
        for s in spans {
            let len = (s.end.min(b) - s.start.max(a)).max(0);
            if s.agent { slot.1 += len } else { slot.0 += len }
        }
    }
    out
}

/// A Today row's words: what kept it awake, and the dim part after it.
/// On rows say only "On": the time range already says when, and where it
/// was switched on is noise.
fn row_words(s: &Span) -> (String, String) {
    if !s.agent {
        return ("On".into(), String::new());
    }
    (names(&s.agents()), s.projects().join(", "))
}

/// Today: minutes awake per hour (On and Auto stacked), the time in each,
/// and what kept the Mac awake, newest first. Hovering an hour or a row
/// lights up the other.
fn today(ui: &mut Ui, p: &Painter, r: Rect, e: &Engine, now: i64, id: egui::Id) {
    let day = today_start(now);
    let spans = e.today(now);
    let live = e.current(now).is_some();
    let per_hour = hours(&spans, day);
    let on_t: i64 = spans.iter().filter(|s| !s.agent).map(|s| s.end - s.start).sum();
    let auto_t: i64 = spans.iter().filter(|s| s.agent).map(|s| s.end - s.start).sum();

    // the section title and the time in each mode
    let ty_ = r.top() + sp::SECTION_H - sp::M;
    p.text(Pos2::new(r.left(), ty_), Align2::LEFT_BOTTOM, "Today", ty::section(), TEXT());
    let mut lx = r.right();
    for (label, secs, c) in [("Auto", auto_t, AUTO()), ("On", on_t, ON())] {
        let v = p.text(Pos2::new(lx, ty_), Align2::RIGHT_BOTTOM, span_text(secs), bold(12.0), TEXT_SOFT());
        let l = p.text(Pos2::new(v.left() - sp::XS, ty_), Align2::RIGHT_BOTTOM, label, font(12.0), TEXT_DIM());
        let sw = Rect::from_center_size(Pos2::new(l.left() - 6.0 - 5.0, l.center().y), Vec2::splat(10.0));
        p.rect_filled(sw, 3.0, c);
        lx = sw.left() - sp::L;
    }

    let card = Rect::from_min_max(Pos2::new(r.left(), r.top() + sp::SECTION_H), r.max);
    glass_card(p, card, R_CARD, 0.0, false);
    let inner = Rect::from_min_max(card.min + Vec2::splat(sp::L), Pos2::new(card.right() - sp::L, card.bottom() - sp::XS));

    // the hourly chart; the row hovered last frame lights its hours
    let hover_key = id.with("row-hover");
    let row_hover: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_key)).flatten();
    let chart = Rect::from_min_size(inner.min, Vec2::new(inner.width() - CHART_LABELS_W, CHART_H));
    for (f, label) in [(0.0, "60 m"), (0.5, "30 m")] {
        let ly = chart.top() + chart.height() * f;
        p.extend(Shape::dashed_line(&[Pos2::new(chart.left(), ly), Pos2::new(chart.right(), ly)], Stroke::new(1.0, white(26)), 3.0, 3.0));
        p.text(Pos2::new(inner.right(), ly), Align2::RIGHT_CENTER, label, font(11.0), TEXT_FAINT());
    }
    let gap = 4.0;
    let cw = (chart.width() - 23.0 * gap) / 24.0;
    let cur_h = ((now - day) / 3600).clamp(0, 23) as usize;
    let mut hour_hover: Option<usize> = None;
    let bar_h = chart.height() - 2.0;
    for (h, (on, auto)) in per_hour.iter().enumerate() {
        let col = Rect::from_min_size(Pos2::new(chart.left() + h as f32 * (cw + gap), chart.top()), Vec2::new(cw, chart.height()));
        let base = if h == cur_h { TEXT() } else if h > cur_h { white(13) } else { white(26) };
        p.rect_filled(Rect::from_min_max(Pos2::new(col.left(), col.bottom() - 2.0), col.max), 0.0, base);
        let ho = bar_h * (*on as f32 / 3600.0).min(1.0);
        let ha = bar_h * (*auto as f32 / 3600.0).min(1.0 - ho / bar_h);
        let bottom = col.bottom() - 2.0;
        let top_r = CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 };
        if ho > 0.0 {
            p.rect_filled(Rect::from_min_max(Pos2::new(col.left(), bottom - ho), Pos2::new(col.right(), bottom)), if ha > 0.0 { CornerRadius::ZERO } else { top_r }, ON());
        }
        if ha > 0.0 {
            p.rect_filled(Rect::from_min_max(Pos2::new(col.left(), bottom - ho - ha), Pos2::new(col.right(), bottom - ho)), top_r, AUTO());
        }
        let (a, b) = (day + h as i64 * 3600, day + (h as i64 + 1) * 3600);
        let row_lit = row_hover.and_then(|i| spans.get(i)).is_some_and(|s| s.start < b && s.end > a);
        let resp = ui.interact(col, id.with(("hour", h)), Sense::hover());
        let lit = resp.hovered() && on + auto > 0;
        if lit {
            hour_hover = Some(h);
        }
        if (lit || row_lit) && ho + ha > 0.0 {
            p.rect_stroke(Rect::from_min_max(Pos2::new(col.left(), bottom - ho - ha), Pos2::new(col.right(), bottom)), top_r, Stroke::new(1.5, TEXT_SOFT()), egui::StrokeKind::Outside);
        }
        if h <= cur_h {
            resp.on_hover_text(format!("{:02}:00 · {} awake", h, span_text(on + auto)));
        }
    }
    let ly = chart.bottom() + 6.0;
    for (i, l) in ["00:00", "06:00", "12:00", "18:00"].iter().enumerate() {
        p.text(Pos2::new(chart.left() + chart.width() * i as f32 / 4.0, ly), Align2::LEFT_TOP, *l, font(11.0), TEXT_FAINT());
    }

    // the list, newest first. Rows run edge to edge in the card (H2): the
    // hover fill has no inset and no corners of its own
    let top = ly + line_h(p, &font(11.0)) + sp::M;
    // whole rows only: a row cut through its text at the bottom reads as
    // broken (the rest scrolls)
    let rows_fit = ((inner.bottom() - top) / LOG_ROW_H).floor().max(1.0);
    let list = Rect::from_min_max(Pos2::new(card.left() + 1.0, top), Pos2::new(card.right() - 1.0, top + rows_fit * LOG_ROW_H + 2.0));
    hairline(p, Pos2::new(inner.left(), top), Pos2::new(inner.right(), top));
    if spans.is_empty() {
        p.text(list.center(), Align2::CENTER_CENTER, "Nothing yet today", ty::body(), TEXT_FAINT());
        ui.ctx().data_mut(|d| d.insert_temp::<Option<usize>>(hover_key, None));
        return;
    }
    let order: Vec<usize> = (0..spans.len()).rev().collect();
    let last = spans.len() - 1;
    let mut new_hover: Option<usize> = None;
    let mut child = ui.new_child(UiBuilder::new().max_rect(list.shrink2(Vec2::new(0.0, 1.0))));
    child.set_clip_rect(list.shrink2(Vec2::new(0.0, 1.0)));
    ScrollArea::vertical().id_salt(id.with("list")).auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), order.len() as f32 * LOG_ROW_H), Sense::hover());
        let p = ui.painter().clone();
        for (k, &i) in order.iter().enumerate() {
            let s = &spans[i];
            let row = Rect::from_min_size(Pos2::new(area.left(), area.top() + k as f32 * LOG_ROW_H), Vec2::new(area.width(), LOG_ROW_H));
            let resp = ui.interact(row, id.with(("row", i)), Sense::hover());
            let hour_lit = hour_hover.is_some_and(|h| {
                let (a, b) = (day + h as i64 * 3600, day + (h as i64 + 1) * 3600);
                s.start < b && s.end > a
            });
            if k > 0 {
                // H2: no hairline touches a lit row
                crate::kit::glass::row_divider(ui, &p, Pos2::new(inner.left(), row.top()), Pos2::new(inner.right(), row.top()), resp.hovered() || hour_lit, row.bottom());
            } else if resp.hovered() || hour_lit {
                crate::kit::glass::row_divider(ui, &p, row.left_top(), row.left_top(), true, row.bottom());
            }
            if resp.hovered() {
                new_hover = Some(i);
            }
            if resp.hovered() || hour_lit {
                crate::kit::glass::row_hover_fill(&p, row);
            }
            let c = Pos2::new(inner.left(), row.center().y);
            p.rect_filled(Rect::from_min_size(Pos2::new(c.x, c.y - 5.0), Vec2::splat(10.0)), 3.0, if s.agent { AUTO() } else { ON() });
            let now_row = live && i == last;
            let x1 = inner.right();
            let du = p.text(Pos2::new(x1, c.y), Align2::RIGHT_CENTER, span_text(s.end - s.start), ty::title(), TEXT());
            let du_left = du.left().min(x1 - 64.0);
            let end = if now_row { "now".to_string() } else { time_of_day(s.end) };
            let tm = p.text(Pos2::new(du_left - sp::COL_GAP, c.y), Align2::RIGHT_CENTER, format!("{} – {end}", time_of_day(s.start)), font(12.0), TEXT_FAINT());
            let (what, from) = row_words(s);
            let nx = c.x + 10.0 + sp::S + 2.0;
            let room = tm.left() - sp::COL_GAP - nx;
            let w = text_fit(&p, Pos2::new(nx, c.y), Align2::LEFT_CENTER, &what, ty::body(), TEXT(), room);
            if !from.is_empty() && room - w.width() > 24.0 {
                text_fit(&p, Pos2::new(w.right(), c.y), Align2::LEFT_CENTER, &format!(" · {from}"), ty::body(), TEXT_FAINT(), room - w.width());
            }
        }
    });
    if new_hover != row_hover {
        ui.ctx().data_mut(|d| d.insert_temp(hover_key, new_hover));
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(span_text(42 * 60), "42 m");
        assert_eq!(span_text(73 * 60), "1 h 13 m");
        assert_eq!(span_text(67 * 60), "1 h 07 m");
        assert_eq!(clock(252), "4:12");
        assert_eq!(span_text(40), "<1 m");
        assert_eq!(span_text(0), "0 m");
    }

    #[test]
    fn hours_split_on_and_auto() {
        let day = 0;
        let spans = vec![
            Span { start: 9 * 3600 + 1800, end: 10 * 3600 + 600, agent: false, by: None, who: vec![], via: Some(Via::Page) },
            Span { start: 10 * 3600 + 600, end: 10 * 3600 + 1200, agent: true, by: None, who: vec![], via: None },
        ];
        let h = hours(&spans, day);
        assert_eq!(h[9], (1800, 0));
        assert_eq!(h[10], (600, 600));
        assert_eq!(h[11], (0, 0));
    }

    #[test]
    fn rows_say_who() {
        use super::super::sessions::Source;
        let on = Span { start: 0, end: 1, agent: false, by: None, who: vec![], via: Some(Via::Shortcut) };
        assert_eq!(row_words(&on), ("On".to_string(), String::new()));
        let auto = Span { start: 0, end: 1, agent: true, by: Some(Source::Codex), who: vec![(Source::Codex, "api-server".into())], via: None };
        assert_eq!(row_words(&auto), ("Codex".to_string(), "api-server".to_string()));
    }
}
