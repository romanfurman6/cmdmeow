//! Drawing for Keep Awake: the cup-in-a-ring scene, the panel tile and card,
//! and the window page.
//!
//! The scene tells the state at a glance: a moon when the Mac may sleep,
//! a cup when it's kept awake, a ring that fills while the AI agent works and
//! drains in amber while it winds down. Nothing loops: the ring is the only
//! thing that moves, and only because time passes.

use crate::kit::sync::LockExt;
use super::engine::{Engine, Span, State, GRACE_CHOICES};
use super::{KeepAwake, KeyRec, ID};
use crate::kit::hotkeys::{listen, Heard};
use crate::core::tool::Size;
use crate::kit::glass::*;
use crate::kit::tile;
use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::f32::consts::{PI, TAU};

const HERO_H: f32 = 120.0;
const ROW_H: f32 = sp::SETTINGS_ROW_H;
const SESSION_H: f32 = 24.0;
/// Most working sessions listed in the panel card.
const CARD_SESSIONS: usize = 3;

/// Colors of one state.
struct Look {
    /// Ring, accents.
    ring: Color32,
    /// Cup, moon.
    body: Color32,
    /// Steam, second text line.
    soft: Color32,
    /// Hero background.
    bg: Color32,
}

/// The agent's time on the Today strip: the accent, it's what the tool is for.
#[allow(non_snake_case)]
fn AGENT_TINT() -> Color32 { tint::KEEP_AWAKE() }
/// Time kept awake by hand on the Today strip: a neutral step.
const HAND: Color32 = Color32::from_rgb(0x8E, 0x8E, 0x93);

fn look(st: State) -> Look {
    let cup = white(235);
    let soft = white(190);
    match st {
        State::Off => Look { ring: tint::NEUTRAL(), body: tint::NEUTRAL(), soft, bg: CARD() },
        State::Manual | State::Agent => Look { ring: GREEN(), body: cup, soft, bg: CARD() },
        State::WindDown { .. } => Look { ring: AMBER(), body: cup, soft, bg: CARD() },
    }
}

/// One word for the tile and the sidebar; `agent` is the watched agent's name.
pub fn short_state(st: State, agent: &str) -> String {
    match st {
        State::Off => "Off".into(),
        State::Manual => "On".into(),
        State::Agent => agent.into(),
        State::WindDown { left } => clock(left),
    }
}

/// 4:12
fn clock(secs: i64) -> String {
    let s = secs.max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

/// 1 h 12 m, 12 m, 40 s
fn span_text(secs: i64) -> String {
    let s = secs.max(0);
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h} h {m:02} m")
    } else if m > 0 {
        format!("{m} m")
    } else {
        format!("{s} s")
    }
}

fn ago(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s} s ago")
    } else if s < 3600 {
        format!("{} min ago", s / 60)
    } else {
        format!("{} h ago", s / 3600)
    }
}

fn time_of_day(t: i64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(t, 0).single().map(|d| d.format("%H:%M").to_string()).unwrap_or_default()
}

/// How far around the ring is filled.
fn ring_frac(e: &Engine, st: State, now: i64) -> f32 {
    match st {
        State::Off => 0.0,
        State::Manual => 1.0,
        // fills once an hour
        State::Agent => {
            let mins = e.awake_since.map(|s| (now - s) as f32 / 60.0).unwrap_or(0.0);
            (0.06 + (mins % 60.0) / 60.0).min(1.0)
        }
        State::WindDown { left } => (left as f32 / (e.settings.grace_min.max(1) as f32 * 60.0)).clamp(0.02, 1.0),
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

/// The cup-in-a-ring scene drawn in the square `rect` on a `look.bg` ground.
fn scene(p: &Painter, rect: Rect, st: State, frac: f32, t: f64) {
    let lk = look(st);
    let s = rect.width() / 96.0;
    let at = |x: f32, y: f32| rect.min + Vec2::new(x * s, y * s);
    let c = at(48.0, 48.0);
    let rr = 42.0 * s;
    crate::kit::glass::stroke_circle(&p, c, rr, Stroke::new(4.0 * s, white(26)));
    let a0 = -PI / 2.0;
    let a1 = a0 + TAU * frac;
    if frac >= 0.999 {
        crate::kit::glass::stroke_circle(&p, c, rr, Stroke::new(4.0 * s, lk.ring));
    } else if frac > 0.0 {
        p.add(PathShape::line(arc_points(c, rr, a0, a1, 72), PathStroke::new(4.0 * s, lk.ring)));
        for a in [a0, a1] {
            p.circle_filled(c + Vec2::new(a.cos(), a.sin()) * rr, 2.0 * s, lk.ring);
        }
    }

    if st == State::Off {
        // crescent moon, nothing else
        crescent(p, at(52.0, 50.0), 22.0 * s, at(62.0, 41.0), 18.5 * s, lk.body);
        return;
    }

    // steam: still wisps, two while awake, one while cooling
    let wisps: &[(f32, f32)] = if matches!(st, State::WindDown { .. }) { &[(46.0, 36.0)] } else { &[(42.0, 37.0), (50.0, 37.0)] };
    for (x0, y0) in wisps.iter() {
        let (alpha, dy) = if matches!(st, State::WindDown { .. }) { (0.35, 0.0) } else { (0.6, 0.0) };
        let pts: Vec<Pos2> = (0..=12)
            .map(|i| {
                let u = i as f32 / 12.0;
                at(x0 + 2.2 * (u * PI * 1.6).sin(), y0 + dy - 11.0 * u)
            })
            .collect();
        p.add(PathShape::line(pts, PathStroke::new(2.5 * s, with_alpha(lk.soft, alpha * 0.9))));
    }

    // cup: straight top, rounded bottom
    let mut body = vec![at(31.0, 44.0), at(61.0, 44.0), at(61.0, 55.0)];
    body.extend(arc_points(at(47.0, 55.0), 14.0 * s, 0.0, PI / 2.0, 10));
    body.extend(arc_points(at(45.0, 55.0), 14.0 * s, PI / 2.0, PI, 10));
    p.add(Shape::convex_polygon(body, lk.body, Stroke::NONE));
    let mut handle = vec![at(60.0, 48.0)];
    handle.extend(arc_points(at(64.0, 54.0), 6.0 * s, -PI / 2.0, PI / 2.0, 14));
    handle.push(at(60.0, 60.0));
    p.add(PathShape::line(handle, PathStroke::new(4.0 * s, lk.body)));
    p.line_segment([at(27.0, 74.0), at(65.0, 74.0)], Stroke::new(3.0 * s, lk.body));
    p.circle_filled(at(27.0, 74.0), 1.5 * s, lk.body);
    p.circle_filled(at(65.0, 74.0), 1.5 * s, lk.body);

    let _ = t;
}

/// Lines of text beside the scene: title, reason, detail.
fn hero_text(e: &Engine, st: State, now: i64) -> (String, String, String) {
    let since = e.awake_since.map(|s| format!("since {} · {}", time_of_day(s), span_text(now - s))).unwrap_or_default();
    match st {
        State::Off => (
            "Off".into(),
            "Mac sleeps as usual".into(),
            match (e.agent_name(), e.settings.agent) {
                ("", _) => String::new(),
                (a, true) => format!("Watching {a}"),
                (a, false) => format!("{a} trigger is off"),
            },
        ),
        State::Manual => {
            let screen = if e.settings.manual_screen { "screen on" } else { "screen can sleep" };
            ("Awake".into(), "Until you turn it off".into(), format!("{since} · {screen}"))
        }
        State::Agent => {
            let n = e.working(now).len();
            let screen = if e.settings.agent_screen { "screen on" } else { "screen can sleep" };
            // says who holds it: the Keep awake switch is off in this state
            (
                format!("Awake for {}", e.agent_name()),
                format!("{n} session{} working", if n == 1 { "" } else { "s" }),
                format!("{} · {screen}", e.awake_since.map(|s| span_text(now - s)).unwrap_or_default()),
            )
        }
        State::WindDown { left } => (clock(left), format!("{} finished · sleep in", e.agent_name()), String::new()),
    }
}

/// The scene card: tinted ground, glow, scene on the left, text beside it.
/// Returns the rect of the "Keep awake" button while winding down.
fn hero(p: &Painter, r: Rect, e: &Engine, st: State, now: i64, t: f64, big: bool) -> Option<Rect> {
    let lk = look(st);
    let radius = if big { 16.0 } else { R_CARD };
    p.rect_filled(r, radius, lk.bg);
    let size = if big { 150.0 } else { 96.0 };
    let pad = if big { 28.0 } else { 14.0 };
    let sr = Rect::from_min_size(Pos2::new(r.left() + pad, r.center().y - size / 2.0), Vec2::splat(size));
    scene(p, sr, st, ring_frac(e, st, now), t);

    let (title, reason, detail) = hero_text(e, st, now);
    let x = sr.right() + if big { 28.0 } else { 14.0 };
    let w = r.right() - x - 12.0;
    let (tf, lf) = if big { (bold(34.0), font(15.0)) } else { (bold(22.0), font(12.0)) };
    // the third line: how long it has been up (window) or the detail (panel);
    // winding down puts the "Keep awake" button there
    let wind = matches!(st, State::WindDown { .. });
    let third = if big { e.awake_since.map(|s| format!("since {} · {}", time_of_day(s), span_text(now - s))).unwrap_or_default() } else { detail };
    let (h1, h2, h3) = (line_h(p, &tf), line_h(p, &lf), if wind { if big { 28.0 } else { 24.0 } } else if third.is_empty() { 0.0 } else { line_h(p, &font(12.0)) });
    let block = h1 + sp::LINE_GAP + h2 + if h3 > 0.0 { sp::M + h3 } else { 0.0 };
    let mut y = r.center().y - block / 2.0;
    text_fit(p, Pos2::new(x, y), Align2::LEFT_TOP, &title, tf, TEXT(), w);
    y += h1 + sp::LINE_GAP;
    text_fit(p, Pos2::new(x, y), Align2::LEFT_TOP, &reason, lf, lk.soft, w);
    y += h2 + sp::M;
    if wind {
        return Some(Rect::from_min_size(Pos2::new(x, y), Vec2::new(96.0, 24.0)));
    }
    text_fit(p, Pos2::new(x, y), Align2::LEFT_TOP, &third, font(12.0), TEXT_DIM(), w);
    None
}

// ---------------------------------------------------------------- panel

/// The Keep Awake widget at `size`. A click on the small ones flips the
/// switch; Large is the full card.
pub fn widget(k: &mut KeepAwake, size: Size, ui: &mut Ui, rect: Rect, cx: &mut crate::core::tool::Cx) {
    if size == Size::S44 {
        card(k, ui, rect, cx);
        return;
    }
    let now = k.now();
    let t = ui.ctx().input(|i| i.time);
    let id = ui.scope_id().with("w-awake");
    let (st, frac, (title, reason, _), manual, agent) = {
        let e = k.engine.lock_ok();
        let st = e.state(now);
        (st, ring_frac(&e, st, now), hero_text(&e, st, now), e.manual_since.is_some(), e.agent_name())
    };
    // a widget's switch shows the agent's hold itself, so its title stays
    // short ("Awake for Claude" is the page's, where the switch is manual)
    let (title, reason) = if st == State::Agent { ("Awake".to_string(), format!("{agent} · {reason}")) } else { (title, reason) };
    let lk = look(st);
    // the ring carries the state; the card stays the same as every other one
    let resp = tile::card(ui, rect, id, false);
    // the switch shows whether the Mac stays awake. While the agent keeps it up
    // the switch is on but locked: turning it off wouldn't change anything
    let (sw_on, sw_enabled) = (st.awake(), manual || !st.awake());
    let p = ui.painter().clone();
    let working = format!("{agent} is working");
    let tip = if manual { "Let the Mac sleep" } else if st.awake() { working.as_str() } else { "Keep the Mac awake" };
    match size {
        Size::S11 => {
            let sr = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 32.0), Vec2::splat(50.0));
            scene(&p, sr, st, frac, t);
            p.text(Pos2::new(rect.center().x, rect.bottom() - 9.0), Align2::CENTER_BOTTOM, short_state(st, agent), bold(11.0), if st.awake() { lk.soft } else { TEXT_DIM() });
        }
        Size::S24 => {
            let mut e = k.engine.lock_ok();
            // the state on top, today's awake time along the bottom
            let strip_h = 14.0;
            let hr = Rect::from_min_max(rect.min + Vec2::splat(1.0), Pos2::new(rect.right() - 1.0, rect.bottom() - strip_h - 22.0));
            let sr = Rect::from_min_max(Pos2::new(rect.left() + 14.0, rect.bottom() - strip_h - 12.0), Pos2::new(rect.right() - 14.0, rect.bottom() - 12.0));
            today_strip(&p, sr, &e.today(now), now);
            if let Some(br) = hero(&p, hr, &e, st, now, t, false) {
                if pill_button(ui, br, "Keep awake", BTN_SECONDARY, true).clicked() {
                    e.set_manual(true, now);
                }
            }
            let sw = Rect::from_min_size(Pos2::new(rect.right() - 12.0 - 38.0, rect.top() + 12.0), Vec2::new(38.0, 22.0));
            if switch(ui, sw, id.with("sw"), sw_on, true).on_hover_text(tip).clicked() && sw_enabled {
                e.set_manual(!manual, now);
            }
            return;
        }
        _ => {
            let side = rect.height() - 16.0;
            let sr = Rect::from_min_size(Pos2::new(rect.left() + 10.0, rect.center().y - side / 2.0), Vec2::splat(side));
            scene(&p, sr, st, frac, t);
            let x = sr.right() + 10.0;
            let right = if size == Size::S14 { rect.right() - 60.0 } else { rect.right() - 8.0 };
            let tf = bold(if size == Size::S14 { 17.0 } else { 14.0 });
            let (ly1, ly2) = two_lines_y(&p, rect.center().y, &tf, &font(11.0));
            text_fit(&p, Pos2::new(x, ly1), Align2::LEFT_CENTER, &title, tf, TEXT(), right - x);
            // Wide has room for a few words only
            let short = match st {
                State::Off => "Sleeps as usual".to_string(),
                State::Manual => "Until turned off".to_string(),
                State::Agent => format!("{agent} working"),
                State::WindDown { .. } => "Then sleeps".to_string(),
            };
            let reason = if size == Size::S12 { short } else { reason };
            text_fit(&p, Pos2::new(x, ly2), Align2::LEFT_CENTER, &reason, font(11.0), lk.soft, right - x);
            if size == Size::S14 {
                let sw = Rect::from_min_size(Pos2::new(rect.right() - 12.0 - 38.0, rect.center().y - 11.0), Vec2::new(38.0, 22.0));
                if switch(ui, sw, id.with("sw"), sw_on, true).on_hover_text(tip).clicked() && sw_enabled {
                    k.toggle();
                }
                return;
            }
        }
    }
    if resp.on_hover_text(tip).clicked() {
        k.toggle();
    }
}

pub fn card(k: &mut KeepAwake, ui: &mut Ui, rect: Rect, cx: &mut crate::core::tool::Cx) {
    let now = k.now();
    let t = ui.ctx().input(|i| i.time);
    let p = ui.painter().clone();
    let id = ui.scope_id().with("awake-card");
    tile::paint_card(&p, rect, 0.0, false);
    let x0 = rect.left() + 12.0;
    let x1 = rect.right() - 12.0;
    let mut y = rect.top() + 10.0;

    let mut e = k.engine.lock_ok();
    let st = e.state(now);
    let manual = e.manual_since.is_some();

    // header: chip, name (opens the page), the switch
    tool_chip(&p, Rect::from_min_size(Pos2::new(x0, y), Vec2::splat(20.0)), Icon::Cup, tint::KEEP_AWAKE());
    let name = p.text(Pos2::new(x0 + 28.0, y + 10.0), Align2::LEFT_CENTER, "Keep Awake", bold(13.0), TEXT());
    if ui.interact(name, id.with("name"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        cx.open_page(ID, "");
    }
    let sw = Rect::from_min_size(Pos2::new(x1 - 38.0, y - 1.0), Vec2::new(38.0, 22.0));
    p.text(Pos2::new(sw.left() - 8.0, sw.center().y), Align2::RIGHT_CENTER, if manual { "On" } else { "Off" }, font(12.0), TEXT_DIM());
    if switch(ui, sw, id.with("manual"), manual, true).on_hover_text("Keep the Mac awake until you turn this off").clicked() {
        e.set_manual(!manual, now);
    }
    y += 30.0;

    let hr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + HERO_H));
    if let Some(br) = hero(&p, hr, &e, st, now, t, false) {
        if pill_button(ui, br, "Keep awake", BTN_SECONDARY, true).clicked() {
            e.set_manual(true, now);
        }
    }
    y += HERO_H + 8.0;

    // the main agent, when it's on this Mac
    let row = |y: f32| Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + ROW_H));
    let agent = e.agent_name();
    if !agent.is_empty() {
        let r = row(y);
        icon(&p, Icon::Gauge, Rect::from_center_size(Pos2::new(x0 + 8.0, r.center().y), Vec2::splat(16.0)), TEXT_DIM());
        p.text(Pos2::new(x0 + 26.0, r.center().y), Align2::LEFT_CENTER, format!("While {agent} works"), font(13.0), TEXT());
        let on = e.settings.agent;
        if switch(ui, Rect::from_min_size(Pos2::new(x1 - 30.0, r.center().y - 9.0), Vec2::new(30.0, 18.0)), id.with("claude"), on, true).clicked() {
            e.change(now, |s| s.agent = !on);
        }
        y += ROW_H;
    }
    if e.watching() {
        let working = e.working(now);
        if working.is_empty() {
            p.text(Pos2::new(x0 + 26.0, y + SESSION_H / 2.0), Align2::LEFT_CENTER, "No session working right now", font(12.0), TEXT_FAINT());
            y += SESSION_H;
        } else {
            for s in working.iter().take(CARD_SESSIONS) {
                let cy = y + SESSION_H / 2.0;
                p.circle_filled(Pos2::new(x0 + 29.0, cy), 3.0, GREEN());
                text_fit(&p, Pos2::new(x0 + 40.0, cy), Align2::LEFT_CENTER, &s.project, font(12.0), TEXT_SOFT(), x1 - x0 - 170.0);
                p.text(Pos2::new(x1, cy), Align2::RIGHT_CENTER, format!("working · {}", ago(now - s.last_write)), font(12.0), TEXT_FAINT());
                y += SESSION_H;
            }
        }
    }
    y += 4.0;
    hairline(&p, Pos2::new(x0, y), Pos2::new(x1, y));

    // screen: the setting that applies to what's keeping the Mac up
    let r = row(y);
    icon(&p, Icon::Monitor, Rect::from_center_size(Pos2::new(x0 + 8.0, r.center().y), Vec2::splat(16.0)), TEXT_DIM());
    let for_agent = matches!(st, State::Agent | State::WindDown { .. });
    let label = if for_agent { format!("Keep screen on while {agent} works") } else { "Keep screen on".into() };
    p.text(Pos2::new(x0 + 26.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT());
    let on = if for_agent { e.settings.agent_screen } else { e.settings.manual_screen };
    if switch(ui, Rect::from_min_size(Pos2::new(x1 - 30.0, r.center().y - 9.0), Vec2::new(30.0, 18.0)), id.with("screen"), on, true).clicked() {
        e.change(now, |s| if for_agent { s.agent_screen = !on } else { s.manual_screen = !on });
    }
}

// ---------------------------------------------------------------- page

pub fn page(k: &mut KeepAwake, ui: &mut Ui, r: Rect) {
    let now = k.now();
    let t = ui.ctx().input(|i| i.time);
    let p = ui.painter().clone();
    let id = ui.scope_id().with("awake-page");
    let mut e = k.engine.lock_ok();
    let st = e.state(now);
    let x0 = r.left() + 24.0;
    let x1 = r.right() - 24.0;
    let mut y = r.top() + 20.0;

    // hero with the big switch
    let hr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 194.0));
    if let Some(br) = hero(&p, hr, &e, st, now, t, true) {
        let br = Rect::from_min_size(br.min, Vec2::new(110.0, 28.0));
        if pill_button(ui, br, "Keep awake", BTN_SECONDARY, true).clicked() {
            e.set_manual(true, now);
        }
    }
    let manual = e.manual_since.is_some();
    let sw = Rect::from_min_size(Pos2::new(hr.right() - 28.0 - 64.0, hr.center().y - 18.0), Vec2::new(64.0, 36.0));
    p.text(Pos2::new(sw.left() - 12.0, sw.center().y), Align2::RIGHT_CENTER, "Keep awake", font(13.0), TEXT_SOFT());
    if switch(ui, sw, id.with("manual"), manual, true).on_hover_text("Keep the Mac awake until you turn this off").clicked() {
        e.set_manual(!manual, now);
    }
    y = hr.bottom() + 18.0;

    // today
    let spans = e.today(now);
    let total: i64 = spans.iter().map(|s| s.end - s.start).sum();
    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, "Today", bold(15.0), TEXT());
    // each agent's own time, also after the main agent changed
    let mut summary = if total == 0 { "Not kept awake yet".to_string() } else { format!("{} awake", span_text(total)) };
    for (a, secs) in super::engine::agent_time(&spans) {
        summary += &format!(" · {} for {}", span_text(secs), a.name());
    }
    p.text(Pos2::new(x1, y + 2.0), Align2::RIGHT_TOP, summary, font(12.0), TEXT_DIM());
    y += 26.0;
    let strip = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 28.0));
    today_strip(&p, strip, &spans, now);
    y = strip.bottom() + 6.0;
    for (i, l) in ["00:00", "06:00", "12:00", "18:00", "24:00"].iter().enumerate() {
        let x = strip.left() + strip.width() * i as f32 / 4.0;
        let align = match i {
            0 => Align2::LEFT_TOP,
            4 => Align2::RIGHT_TOP,
            _ => Align2::CENTER_TOP,
        };
        p.text(Pos2::new(x, y), align, *l, font(11.0), TEXT_FAINT());
    }
    y += 20.0;
    let mut lx = x0;
    // one tint for agent time, named after every agent in it (and the
    // watched one, before it has any)
    let mut agents = e.week_agents(now);
    if let Some(a) = e.agent.filter(|a| !agents.contains(a)) {
        agents.push(a);
    }
    let names = agents.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ");
    for (label, c) in [("You", HAND), (names.as_str(), AGENT_TINT())].into_iter().take(if agents.is_empty() { 1 } else { 2 }) {
        p.rect_filled(Rect::from_min_size(Pos2::new(lx, y + 1.0), Vec2::splat(10.0)), 3.0, c);
        let tr = p.text(Pos2::new(lx + 16.0, y + 6.0), Align2::LEFT_CENTER, label, font(12.0), TEXT_DIM());
        lx = tr.right() + 16.0;
    }
    y += 30.0;

    // two columns: options, the agent's sessions; options alone when the
    // main agent isn't on this Mac
    let gap = sp::L + sp::XS;
    let has_agent = e.agent.is_some();
    let lw = if has_agent { (x1 - x0 - gap) * 0.5 } else { x1 - x0 };
    let left = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x0 + lw, r.bottom() - 20.0));
    let right = Rect::from_min_max(Pos2::new(left.right() + gap, y), Pos2::new(x1, r.bottom() - 20.0));
    options(ui, &p, left, &mut e, &mut k.keys, now, id);
    let rows = option_rows(&e);
    let mut h = ROW * rows;
    if has_agent {
        sessions(&p, right, &e, now, rows);
        h = h.max(e.sessions.len().clamp(1, 6) as f32 * 32.0 + 12.0);
    }
    // the rest of the page: the last 7 days
    let wy = y + 26.0 + h + 20.0;
    if r.bottom() - 20.0 - wy > 100.0 {
        week(&p, Rect::from_min_max(Pos2::new(x0, wy), Pos2::new(x1, r.bottom() - 20.0)), &e.week(now));
    }
}

/// Awake time per day for the last week, the agent's part and yours stacked.
fn week(p: &Painter, r: Rect, days: &[(i64, i64, i64)]) {
    let total: i64 = days.iter().map(|d| d.1 + d.2).sum();
    p.text(r.min, Align2::LEFT_TOP, "Last 7 days", bold(15.0), TEXT());
    p.text(Pos2::new(r.right(), r.top() + 2.0), Align2::RIGHT_TOP, format!("{} awake", span_text(total)), font(12.0), TEXT_DIM());
    let card = Rect::from_min_max(r.min + Vec2::new(0.0, 26.0), r.max);
    section_card(p, card);
    let ci = card.shrink2(Vec2::new(16.0, 14.0));
    let chart = Rect::from_min_max(ci.min + Vec2::new(0.0, 16.0 + sp::S), ci.max - Vec2::new(0.0, 18.0 + sp::XS));
    let max = days.iter().map(|d| d.1 + d.2).max().unwrap_or(0).max(3600) as f32;
    let n = days.len().max(1) as f32;
    let bw = ((chart.width() - (n - 1.0) * 12.0) / n).min(72.0);
    let step = (chart.width() - bw) / (n - 1.0).max(1.0);
    for (i, (d0, c, h)) in days.iter().enumerate() {
        let x = chart.left() + i as f32 * step;
        let hh = chart.height() * (*h as f32 / max);
        let ch = chart.height() * (*c as f32 / max);
        if c + h > 0 {
            p.rect_filled(Rect::from_min_max(Pos2::new(x, chart.bottom() - ch - hh), Pos2::new(x + bw, chart.bottom() - ch)), 4.0, HAND);
            p.rect_filled(Rect::from_min_max(Pos2::new(x, chart.bottom() - ch), Pos2::new(x + bw, chart.bottom())), 4.0, AGENT_TINT());
            text_fit(p, Pos2::new(x + bw / 2.0, chart.bottom() - ch - hh - sp::S), Align2::CENTER_BOTTOM, &span_text(c + h), font(11.0), TEXT_DIM(), bw + 10.0);
        }
        let day = chrono::DateTime::from_timestamp(*d0, 0).map(|d| d.with_timezone(&chrono::Local).format("%a").to_string()).unwrap_or_default();
        p.text(Pos2::new(x + bw / 2.0, chart.bottom() + sp::S), Align2::CENTER_TOP, if i + 1 == days.len() { "Today".to_string() } else { day }, font(11.0), TEXT_FAINT());
    }
}

fn today_strip(p: &Painter, r: Rect, spans: &[Span], now: i64) {
    p.rect_filled(r, 8.0, white(12));
    let day = super::engine::today_start(now);
    let x_of = |t: i64| r.left() + r.width() * ((t - day) as f32 / 86_400.0).clamp(0.0, 1.0);
    for s in spans {
        let (a, b) = (x_of(s.start), x_of(s.end).max(x_of(s.start) + 3.0));
        let c = if s.agent { AGENT_TINT() } else { HAND };
        p.rect_filled(Rect::from_min_max(Pos2::new(a, r.top() + 4.0), Pos2::new(b, r.bottom() - 4.0)), 4.0, c);
    }
    let nx = x_of(now);
    p.line_segment([Pos2::new(nx, r.top()), Pos2::new(nx, r.bottom())], Stroke::new(2.0, TEXT()));
}

fn section_card(p: &Painter, r: Rect) {
    glass_card(p, r, R_CARD, 0.0, false);
}

/// A row with a label and a switch. Returns true when clicked.
fn switch_row(ui: &mut Ui, p: &Painter, r: Rect, id: egui::Id, label: &str, on: bool) -> bool {
    text_fit(p, Pos2::new(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT(), r.width() - 70.0);
    switch(ui, Rect::from_min_size(Pos2::new(r.right() - 42.0, r.center().y - 9.0), Vec2::new(30.0, 18.0)), id, on, true).clicked()
}

const ROW: f32 = sp::ROW_H;

/// Rows in the options card: the agent's three only when there's an agent.
fn option_rows(e: &Engine) -> f32 {
    if e.agent.is_some() { 5.0 } else { 2.0 }
}

#[allow(clippy::too_many_arguments)]
fn options(ui: &mut Ui, p: &Painter, r: Rect, e: &mut Engine, keys: &mut KeyRec, now: i64, id: egui::Id) {
    p.text(r.min, Align2::LEFT_TOP, "Options", bold(15.0), TEXT());
    let card = Rect::from_min_size(r.min + Vec2::new(0.0, 26.0), Vec2::new(r.width(), ROW * option_rows(e)));
    section_card(p, card);
    let row = |i: usize| Rect::from_min_size(Pos2::new(card.left(), card.top() + ROW * i as f32), Vec2::new(card.width(), ROW));
    let line = |i: usize| {
        let y = card.top() + ROW * i as f32;
        hairline(p, Pos2::new(card.left() + sp::M, y), Pos2::new(card.right(), y));
    };
    // the agent's rows only when there's an agent to watch
    let agent = e.agent_name();
    let mut i = 0;
    if !agent.is_empty() {
        let on = e.settings.agent;
        if switch_row(ui, p, row(i), id.with("claude"), &format!("Stay awake while {agent} works"), on) {
            e.change(now, |s| s.agent = !on);
        }
        i += 1;
        line(i);
        // its screen switch right under it, so the two screen rows don't read
        // as one setting twice
        let c = e.settings.agent_screen;
        if switch_row(ui, p, row(i), id.with("cscreen"), &format!("Keep the screen on for {agent}"), c) {
            e.change(now, |s| s.agent_screen = !c);
        }
        i += 1;
        line(i);
        let r1 = row(i);
        let labels: Vec<String> = GRACE_CHOICES.iter().map(|m| format!("{m} min")).collect();
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let sel = GRACE_CHOICES.iter().position(|m| *m == e.settings.grace_min).unwrap_or(1);
        let seg = Rect::from_min_size(Pos2::new(r1.right() - sp::M - 168.0, r1.center().y - 12.0), Vec2::new(168.0, 24.0));
        text_fit(p, Pos2::new(r1.left() + sp::M, r1.center().y), Align2::LEFT_CENTER, &format!("Sleep after {agent} stops"), font(13.0), TEXT(), seg.left() - r1.left() - sp::M - sp::COL_GAP);
        if let Some(k) = segmented(ui, seg, id.with("grace"), &refs, sel) {
            e.change(now, |s| s.grace_min = GRACE_CHOICES[k]);
        }
        i += 1;
        line(i);
    }
    let m = e.settings.manual_screen;
    if switch_row(ui, p, row(i), id.with("mscreen"), "Keep the screen on with Keep awake", m) {
        e.change(now, |s| s.manual_screen = !m);
    }
    i += 1;
    line(i);
    shortcut_row(ui, p, row(i), e, keys, now, id.with("key"));
}

/// The switch's shortcut: click, then press the keys. Esc keeps the old
/// one, ⌫ removes it.
fn shortcut_row(ui: &mut Ui, p: &Painter, r: Rect, e: &mut Engine, keys: &mut KeyRec, now: i64, id: egui::Id) {
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
    p.text(Pos2::new(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, "Shortcut", font(13.0), TEXT());
    let rec = keys.recording;
    let cur = e.settings.shortcut.clone();
    let text = if rec { "Type a shortcut…".to_string() } else { cur.as_ref().map(|s| s.display()).unwrap_or_else(|| "Record".into()) };
    let g = p.layout_no_wrap(text.clone(), font(12.0), TEXT());
    let bw = (g.size().x + 20.0).max(76.0);
    let br = Rect::from_min_max(Pos2::new(r.right() - 12.0 - bw, r.center().y - 12.0), Pos2::new(r.right() - 12.0, r.center().y + 12.0));
    let resp = ui.interact(br, id, Sense::click());
    if rec {
        p.rect_filled(br, 6.0, with_alpha(BLUE(), 0.22));
        p.rect_stroke(br, 6.0, Stroke::new(1.0, ACCENT()), egui::StrokeKind::Inside);
    } else if cur.is_some() {
        p.rect_filled(br, 6.0, white(if resp.hovered() { 30 } else { 20 }));
        p.rect_stroke(br, 6.0, Stroke::new(1.0, white(26)), egui::StrokeKind::Inside);
    } else {
        dashed_rect(p, br, 6.0, white(60));
    }
    p.text(br.center(), Align2::CENTER_CENTER, text, font(12.0), if cur.is_some() || rec { TEXT() } else { TEXT_FAINT() });
    if resp.on_hover_text("Click, then press the keys. Esc cancels, ⌫ removes it.").clicked() {
        keys.recording = !rec;
        keys.seen = Some(std::time::Instant::now());
    }
    if let Some(err) = &keys.error {
        text_fit(p, Pos2::new(br.left() - 10.0, r.center().y), Align2::RIGHT_CENTER, err, font(11.0), AMBER(), br.left() - r.left() - 90.0);
    }
}

fn sessions(p: &Painter, r: Rect, e: &Engine, now: i64, option_rows: f32) {
    p.text(r.min, Align2::LEFT_TOP, format!("{} sessions", e.agent_name()), bold(15.0), TEXT());
    let shown: Vec<_> = e.sessions.iter().take(6).cloned().collect();
    let rows = shown.len().max(1) as f32;
    let h = (rows * 32.0 + 12.0).max(ROW * option_rows);
    let card = Rect::from_min_size(r.min + Vec2::new(0.0, 26.0), Vec2::new(r.width(), h));
    section_card(p, card);
    let mut y = card.top() + 6.0;
    if shown.is_empty() {
        p.text(card.center(), Align2::CENTER_CENTER, if e.settings.agent { "None today" } else { "Monitoring off" }, font(12.0), TEXT_FAINT());
    }
    for s in &shown {
        let cy = y + 16.0;
        let working = s.working(now);
        p.circle_filled(Pos2::new(card.left() + 16.0, cy), 3.0, if working { GREEN() } else { Color32::from_rgb(0x63, 0x63, 0x66) });
        text_fit(p, Pos2::new(card.left() + 28.0, cy), Align2::LEFT_CENTER, &s.project, mono(11.5), TEXT_SOFT(), card.width() - 140.0);
        let when = if working { "working".to_string() } else { ago(now - s.last_write) };
        p.text(Pos2::new(card.right() - 12.0, cy), Align2::RIGHT_CENTER, when, font(12.0), TEXT_FAINT());
        y += 32.0;
    }
}
