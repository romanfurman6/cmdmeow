//! The speed test as a modal over the whole window: a dial that follows
//! the transfer live, then ping, download and upload side by side.

use super::monitor::{SpeedMode, SpeedPhase, SpeedTest};
use crate::kit::glass::*;
use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Id, Key, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::f32::consts::PI;

fn id() -> Id {
    Id::unique("cmdmeow-speed-modal")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(id(), true));
}

fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(id(), false));
}

/// Dial stops: equal steps on the dial for these Mbps.
const STOPS: [f64; 8] = [0.0, 10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0];
/// The dial runs clockwise from bottom left to bottom right (270°).
const A0: f32 = 0.75 * PI;
const SPAN: f32 = 1.5 * PI;

const W: f32 = 440.0;
const PAD: f32 = 20.0;
const DIAL_R: f32 = 104.0;
const DIAL_W: f32 = 12.0;
const WINDOW_R: u8 = 14;

/// 0..1 position of `mbps` on the dial.
fn dial(mbps: f64) -> f32 {
    for i in 1..STOPS.len() {
        if mbps <= STOPS[i] {
            let t = (mbps - STOPS[i - 1]) / (STOPS[i] - STOPS[i - 1]);
            return ((i - 1) as f64 + t.max(0.0)) as f32 / (STOPS.len() - 1) as f32;
        }
    }
    1.0
}

fn stop_label(v: f64) -> String {
    if v >= 1000.0 {
        format!("{}G", v / 1000.0)
    } else {
        format!("{v}")
    }
}

fn mbps_text(v: f64) -> String {
    if v < 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    }
}

fn down_color() -> Color32 {
    tint::HEALTH()
}

fn up_color() -> Color32 {
    BLUE()
}

/// Draw the modal while it is open (and while it fades out). Returns true
/// the mode to run when the user asks for another test.
pub fn modal(ui: &mut Ui, s: &SpeedTest) -> Option<SpeedMode> {
    let ctx = ui.ctx().clone();
    let open = ctx.data(|d| d.get_temp::<bool>(id())).unwrap_or(false);
    let a = ctx.animate_bool_with_time(id().with("fade"), open, 0.18);
    if a <= 0.0 {
        return None;
    }
    let ease = 1.0 - (1.0 - a).powi(3);
    let full = ctx.content_rect();
    let mut again = None;
    egui::Area::new(id().with("area"))
        .order(Order::Foreground)
        .fixed_pos(full.min)
        .constrain(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.set_min_size(full.size());
            // the scrim swallows clicks; a click on it closes the modal
            let scrim = ui.interact(full, id().with("scrim"), Sense::click());
            ui.painter().rect_filled(full, CornerRadius::same(WINDOW_R), Color32::from_black_alpha((115.0 * ease) as u8));
            ui.multiply_opacity(ease);
            let h = PAD + 28.0 + sp::M + 2.0 * DIAL_R + sp::L + 56.0 + sp::L + 30.0 + PAD;
            let m = Rect::from_center_size(full.center() + Vec2::new(0.0, (1.0 - ease) * 18.0), Vec2::new(W.min(full.width() - 32.0), h));
            let card = ui.interact(m, id().with("card"), Sense::click());
            card_bg(ui.painter(), m);
            let (rerun, closed) = content(ui, m, s);
            again = rerun.filter(|_| open);
            let esc = ctx.input(|i| i.key_pressed(Key::Escape));
            if open && (closed || esc || (scrim.clicked() && !card.contains_pointer())) {
                close(&ctx);
            }
        });
    if s.running || a < 1.0 {
        animate(&ctx);
    }
    again
}

fn card_bg(p: &egui::Painter, m: Rect) {
    let shadow = egui::epaint::Shadow { offset: [0, 20], blur: 60, spread: 0, color: Color32::from_black_alpha(128) };
    p.add(shadow.as_shape(m, CornerRadius::same(16)));
    p.rect_filled(m, 16.0, POPOVER());
    p.rect_stroke(m, 16.0, Stroke::new(1.0, CARD_EDGE()), StrokeKind::Inside);
}

/// One cell of the ping · download · upload row.
struct Col {
    label: &'static str,
    value: Option<f64>,
    unit: &'static str,
    color: Color32,
    phase: SpeedPhase,
    glyph: Option<Icon>,
}

/// Returns (the mode to run again, close).
fn content(ui: &mut Ui, m: Rect, s: &SpeedTest) -> (Option<SpeedMode>, bool) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let t = ctx.input(|i| i.time) as f32;
    let x0 = m.left() + PAD;
    let x1 = m.right() - PAD;
    let mut y = m.top() + PAD;

    // header
    let tr = p.text(Pos2::new(x0, y + 14.0), Align2::LEFT_CENTER, "Speed test", ty::lead(), TEXT());
    p.text(Pos2::new(tr.right() + 8.0, y + 15.0), Align2::LEFT_CENTER, s.mode.label(), ty::caption(), ty::caption_color());
    let cr = Rect::from_center_size(Pos2::new(x1 - 13.0, y + 14.0), Vec2::splat(26.0));
    let mut closed = round_button(ui, cr, Icon::Close, "Close").clicked();
    y += 28.0 + sp::M;

    // dial
    let c = Pos2::new(m.center().x, y + DIAL_R + 6.0);
    let (col, target) = match s.phase {
        SpeedPhase::Download => (down_color(), s.live_mbps),
        SpeedPhase::Upload => (up_color(), s.live_mbps),
        SpeedPhase::Done => (down_color(), s.down_mbps.unwrap_or(0.0)),
        _ => (down_color(), 0.0),
    };
    let frac = ctx.animate_value_with_time(id().with("dial"), dial(target), 0.35);
    // the finished number lands at once; only live readings glide
    let glide = ctx.animate_value_with_time(id().with("num"), target as f32, 0.35) as f64;
    let shown = if s.phase == SpeedPhase::Done { target } else { glide };
    // track and stops
    p.add(PathShape::line(arc_points(c, DIAL_R, A0, A0 + SPAN, 96), PathStroke::new(DIAL_W, white(18))));
    for end in [A0, A0 + SPAN] {
        p.circle_filled(c + Vec2::angled(end) * DIAL_R, DIAL_W / 2.0, white(18));
    }
    for (i, v) in STOPS.iter().enumerate() {
        let k = i as f32 / (STOPS.len() - 1) as f32;
        let ang = A0 + SPAN * k;
        let dir = Vec2::angled(ang);
        let lit = k <= frac + 1e-3 && frac > 0.0;
        p.text(c + dir * (DIAL_R - 26.0), Align2::CENTER_CENTER, stop_label(*v), font(11.0), if lit { TEXT_SOFT() } else { TEXT_MUTED() });
    }
    // the filled arc fades in from the start; a soft glow rides the tip
    if frac > 0.002 {
        let a1 = A0 + SPAN * frac;
        let n = ((frac * 96.0) as usize).max(2);
        let pts = arc_points(c, DIAL_R, A0, a1, n);
        for w in 0..pts.len() - 1 {
            let k = w as f32 / (pts.len() - 1) as f32;
            let segc = lerp_color(with_alpha(col, 0.35), col, k);
            p.line_segment([pts[w], pts[w + 1]], Stroke::new(DIAL_W, segc));
        }
        p.circle_filled(pts[0], DIAL_W / 2.0, with_alpha(col, 0.35));
        let tip = *pts.last().unwrap();
        let pulse = if s.running { 0.5 + 0.5 * (t * 6.0).sin() } else { 0.0 };
        p.circle_filled(tip, DIAL_W / 2.0 + 6.0 + 3.0 * pulse, with_alpha(col, 0.18 + 0.10 * pulse));
        p.circle_filled(tip, DIAL_W / 2.0, col);
        p.circle_filled(tip, 3.0, POPOVER());
    }
    // ping: a comet runs around the empty dial
    if s.phase == SpeedPhase::Ping {
        let head = (t * 1.6).rem_euclid(1.0);
        let len = 0.22;
        let n = 24;
        for i in 0..n {
            let k0 = head - len * i as f32 / n as f32;
            let k1 = head - len * (i + 1) as f32 / n as f32;
            if k1 < 0.0 {
                break;
            }
            let alpha = 1.0 - i as f32 / n as f32;
            let pa = c + Vec2::angled(A0 + SPAN * k0) * DIAL_R;
            let pb = c + Vec2::angled(A0 + SPAN * k1) * DIAL_R;
            p.line_segment([pa, pb], Stroke::new(DIAL_W, with_alpha(down_color(), 0.8 * alpha)));
        }
    }
    // the number in the middle
    let big = FontId::new(46.0, FontFamily::Name("hero".into()));
    let num = match s.phase {
        SpeedPhase::Ping => s.latency_ms.map(|l| format!("{l:.0}")).unwrap_or_else(|| "—".into()),
        SpeedPhase::Idle => "—".into(),
        SpeedPhase::Done if s.down_mbps.is_none() => "—".into(),
        _ => mbps_text(shown.max(0.0)),
    };
    p.text(c + Vec2::new(0.0, -8.0), Align2::CENTER_CENTER, num, big, TEXT());
    let unit = if s.phase == SpeedPhase::Ping { "ms" } else { "Mbps" };
    p.text(c + Vec2::new(0.0, 24.0), Align2::CENTER_CENTER, unit, ty::body(), TEXT_DIM());
    let (glyph, label, lcol) = match s.phase {
        SpeedPhase::Ping => (None, "Ping", TEXT_DIM()),
        SpeedPhase::Download => (Some(Icon::Down), "Download", down_color()),
        SpeedPhase::Upload => (Some(Icon::Up), "Upload", up_color()),
        SpeedPhase::Done if s.error.is_some() => (None, "", TEXT_DIM()),
        SpeedPhase::Done if s.down_mbps.is_none() || s.up_mbps.is_none() => (None, "Stopped", TEXT_DIM()),
        _ => (Some(Icon::Down), "Download", down_color()),
    };
    if !label.is_empty() {
        let lp = c + Vec2::new(0.0, DIAL_R * 0.62);
        let g = p.layout_no_wrap(label.to_string(), ty::label(), lcol);
        let iw = if glyph.is_some() { 14.0 } else { 0.0 };
        let total = iw + g.size().x;
        let left = lp.x - total / 2.0;
        if let Some(gl) = glyph {
            icon(&p, gl, Rect::from_center_size(Pos2::new(left + 5.0, lp.y), Vec2::splat(11.0)), lcol);
        }
        p.galley(Pos2::new(left + iw, lp.y - g.size().y / 2.0), g, lcol);
    }
    y += 2.0 * DIAL_R + sp::L;

    // ping · download · upload
    let row = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 56.0));
    glass_card(&p, row, R_CARD, 0.0, false);
    let cw = row.width() / 3.0;
    let cols = [
        Col { label: "Ping", value: s.latency_ms, unit: "ms", color: TEXT(), phase: SpeedPhase::Ping, glyph: None },
        Col { label: "Download", value: s.down_mbps, unit: "Mbps", color: down_color(), phase: SpeedPhase::Download, glyph: Some(Icon::Down) },
        Col { label: "Upload", value: s.up_mbps, unit: "Mbps", color: up_color(), phase: SpeedPhase::Upload, glyph: Some(Icon::Up) },
    ];
    for (k, Col { label, value: v, unit, color: vc, phase: ph, glyph: gl }) in cols.into_iter().enumerate() {
        let cx = row.left() + k as f32 * cw + 14.0;
        if k > 0 {
            let lx = row.left() + k as f32 * cw;
            p.line_segment([Pos2::new(lx, row.top() + 12.0), Pos2::new(lx, row.bottom() - 12.0)], Stroke::new(1.0, white(HAIRLINE)));
        }
        let active = s.running && s.phase == ph;
        let lc = if active { vc } else { TEXT_DIM() };
        let mut lx = cx;
        if let Some(gl) = gl {
            icon(&p, gl, Rect::from_center_size(Pos2::new(lx + 5.0, row.top() + 17.0), Vec2::splat(10.0)), lc);
            lx += 14.0;
        }
        p.text(Pos2::new(lx, row.top() + 17.0), Align2::LEFT_CENTER, label, ty::label(), lc);
        match v {
            Some(v) => {
                let txt = if unit == "ms" { format!("{v:.0}") } else { mbps_text(v) };
                let g = p.layout_no_wrap(txt, ty::number(), TEXT());
                let gw = g.size().x;
                p.galley(Pos2::new(cx, row.top() + 26.0), g, TEXT());
                let ur = p.text(Pos2::new(cx + gw + 4.0, row.top() + 46.0), Align2::LEFT_BOTTOM, unit, ty::label(), TEXT_DIM());
                // jitter rides along with ping
                if let (SpeedPhase::Ping, Some(j)) = (ph, s.jitter_ms) {
                    p.text(Pos2::new(ur.right() + 6.0, row.top() + 46.0), Align2::LEFT_BOTTOM, format!("±{j:.0}"), ty::label(), TEXT_MUTED());
                }
            }
            None if active => {
                // a short bar that fills with this phase's share of the test
                let part = match ph {
                    SpeedPhase::Download => ((s.progress - 0.1) / 0.5).clamp(0.0, 1.0),
                    SpeedPhase::Upload => ((s.progress - 0.6) / 0.4).clamp(0.0, 1.0),
                    _ => 0.5 + 0.5 * (t * 4.0).sin(),
                };
                let br = Rect::from_min_size(Pos2::new(cx, row.top() + 36.0), Vec2::new(cw - 28.0, 4.0));
                bar(&p, br, part, vc);
            }
            None => {
                p.text(Pos2::new(cx, row.top() + 26.0), Align2::LEFT_TOP, "—", ty::number(), TEXT_MUTED());
            }
        }
    }
    y += 56.0 + sp::L;

    // buttons, or the error
    let by = y;
    let mut again = None;
    let left;
    if !s.running {
        let done_w = button_width(&p, "Done", BTN_PRIMARY);
        let dr = Rect::from_min_size(Pos2::new(x1 - done_w, by), Vec2::new(done_w, 30.0));
        if pill_button(ui, dr, "Done", BTN_PRIMARY, true).clicked() {
            closed = true;
        }
        let mut l = dr.left();
        for mode in [SpeedMode::Full, SpeedMode::Quick] {
            let label = mode.label();
            let w = button_width(&p, label, BTN_SECONDARY);
            let r = Rect::from_min_size(Pos2::new(l - sp::S - w, by), Vec2::new(w, 30.0));
            let tip = match mode {
                SpeedMode::Quick => "About 6 seconds",
                SpeedMode::Full => "About 20 seconds, more streams",
            };
            if pill_button(ui, r, label, BTN_SECONDARY, true).on_hover_text(tip).clicked() {
                again = Some(mode);
            }
            l = r.left();
        }
        left = l;
    } else {
        let sw = button_width(&p, "Stop", BTN_SECONDARY);
        let sr = Rect::from_min_size(Pos2::new(x1 - sw, by), Vec2::new(sw, 30.0));
        if pill_button(ui, sr, "Stop", BTN_SECONDARY, true).clicked() {
            s.stop();
        }
        left = sr.left();
    }
    let tw = left - sp::M - x0;
    if let Some(e) = &s.error {
        text_fit(&p, Pos2::new(x0, by + 15.0), Align2::LEFT_CENTER, e, ty::label(), AMBER(), tw);
    } else if let Some(server) = &s.server {
        text_fit(&p, Pos2::new(x0, by + 15.0), Align2::LEFT_CENTER, &server.line(), ty::label(), TEXT_DIM(), tw);
    }
    (again, closed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dial_is_monotonic_and_bounded() {
        assert_eq!(dial(0.0), 0.0);
        assert_eq!(dial(5000.0), 1.0);
        let mut last = 0.0;
        for v in [1.0, 10.0, 49.0, 100.0, 431.0, 999.0, 1500.0] {
            let d = dial(v);
            assert!(d > last && d <= 1.0, "{v} -> {d}");
            last = d;
        }
    }
}
