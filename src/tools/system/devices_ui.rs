//! The Devices widget in every size: the emptiest device as a ring (1:1),
//! a glyph and charge per device (1:2), a ring per device (1:4), a list
//! with AirPods split into left, right and case (2:4) and every part of
//! every device (4:4). Plus the Devices card on Mac Health (a line per
//! device with its own alert line, hide and forget) and its settings.
//!
//! A bolt marks what's charging; a device that isn't connected is dimmed
//! and says when it was seen.

use super::devices::{self, Device, Prefs, ALERT_CHOICES, CHARGED_CHOICES};
use super::monitor::Snapshot;
use crate::core::tool::Size;
use crate::kit::glass::*;
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

/// How much a device that isn't connected is dimmed.
const AWAY_ALPHA: f32 = 0.45;

fn pct_text(b: Option<u8>) -> String {
    b.map(|b| format!("{b}%")).unwrap_or_else(|| "–".into())
}

fn frac(b: Option<u8>) -> f32 {
    b.map(|b| (b as f32 / 100.0).clamp(0.02, 1.0)).unwrap_or(0.0)
}

/// The device's colours, dimmed when it isn't connected.
struct Tone {
    a: f32,
}

impl Tone {
    fn of(d: &Device) -> Tone {
        Tone { a: if d.live() { 1.0 } else { AWAY_ALPHA } }
    }
    fn c(&self, c: Color32) -> Color32 {
        if self.a < 1.0 { with_alpha(c, self.a) } else { c }
    }
    fn ring(&self, b: Option<u8>) -> Color32 {
        self.c(b.map(devices::color).unwrap_or(TEXT_FAINT()))
    }
    fn text(&self, b: Option<u8>) -> Color32 {
        self.c(b.map(devices::text_color).unwrap_or(TEXT_FAINT()))
    }
}

/// "1 low" or "3 connected".
pub fn note(devs: &[Device]) -> String {
    let low = devs.iter().filter(|d| devices::is_low(d)).count();
    if low > 0 {
        format!("{low} low")
    } else {
        format!("{} connected", devs.iter().filter(|d| d.live()).count())
    }
}

/// The device's own SF Symbol (AirPods Max, Magic Mouse…), or its kind's icon.
fn device_glyph(p: &egui::Painter, d: &Device, r: Rect, color: Color32) {
    if !d.symbols().iter().any(|n| symbol(p, n, r, color)) {
        icon(p, d.kind.icon(), r, color);
    }
}

/// A small charging bolt centred at `c`, `h` tall.
fn bolt(p: &egui::Painter, c: Pos2, h: f32, color: Color32) {
    let r = Rect::from_center_size(c, Vec2::new(h * 0.7, h));
    if symbol(p, "bolt.fill", r, color) {
        return;
    }
    let u = h / 12.0;
    let pt = |x: f32, y: f32| c + Vec2::new((x - 3.5) * u, (y - 6.0) * u);
    let pts = vec![pt(4.5, 0.0), pt(0.5, 6.8), pt(3.2, 6.8), pt(2.5, 12.0), pt(6.5, 5.2), pt(3.8, 5.2)];
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// A percentage with a bolt before it when charging. Returns the rect
/// both take.
fn pct_with_bolt(p: &egui::Painter, pos: Pos2, align: Align2, b: Option<u8>, charging: bool, font_: egui::FontId, color: Color32) -> Rect {
    let size = font_.size;
    let g = p.layout_no_wrap(pct_text(b), font_, color);
    let extra = if charging { size * 0.8 } else { 0.0 };
    let r = align.anchor_size(pos, Vec2::new(g.size().x + extra, g.size().y));
    if charging {
        bolt(p, Pos2::new(r.left() + size * 0.3, r.center().y), size * 0.85, GREEN());
    }
    p.galley(Pos2::new(r.left() + extra, r.top()), g, color);
    r
}

/// A ring with the device's glyph inside (and a bolt on top when charging).
fn ring_cell(p: &egui::Painter, c: Pos2, r: f32, d: &Device, glyph_sz: f32) {
    let b = d.battery();
    let t = Tone::of(d);
    tile::ring(p, c, r, (r * 0.24).max(3.5), frac(b), t.ring(b));
    device_glyph(p, d, Rect::from_center_size(c, Vec2::splat(glyph_sz)), t.c(TEXT_SOFT()));
    if d.charging && d.live() {
        let at = c + Vec2::new(r * 0.72, -r * 0.72);
        p.circle_filled(at, r * 0.32, BG());
        bolt(p, at, r * 0.42, GREEN());
    }
}

pub fn widget(s: &Snapshot, size: Size, ui: &mut Ui, r: Rect, wid: egui::Id, sel: bool) -> Response {
    let devs = devices::lowest_first(&s.devices);
    if size == Size::S11 {
        return match devs.first() {
            Some(d) if d.battery().is_some() => {
                let b = d.battery();
                let resp = tile::card(ui, r, wid, sel);
                let p = ui.painter();
                let c = Pos2::new(r.center().x, r.top() + 32.0);
                ring_cell(p, c, 22.0, d, 17.0);
                pct_with_bolt(p, Pos2::new(r.center().x, r.bottom() - 9.0), Align2::CENTER_BOTTOM, b, false, bold(11.0), Tone::of(d).text(b));
                resp
            }
            Some(d) => tile::small(ui, r, wid, d.kind.icon(), tint::HEALTH(), &d.name, "Connected", sel),
            None => tile::small(ui, r, wid, Icon::Bluetooth, tint::HEALTH(), "Devices", "None", sel),
        };
    }
    let resp = tile::card(ui, r, wid, sel);
    let p = ui.painter();
    match size {
        Size::S12 => {
            let x0 = r.left() + tile::PAD;
            let x1 = r.right() - tile::PAD;
            let live = devs.iter().filter(|d| d.live()).count();
            p.text(Pos2::new(x1, r.top() + 20.0), Align2::RIGHT_CENTER, live.to_string(), ty::label(), white(166));
            text_fit(p, Pos2::new(x0, r.top() + 20.0), Align2::LEFT_CENTER, "Devices", ty::title(), TEXT(), x1 - x0 - 30.0);
            if devs.is_empty() {
                text_fit(p, Pos2::new(x0, r.bottom() - 12.0), Align2::LEFT_BOTTOM, "None connected", font(12.0), TEXT_FAINT(), x1 - x0);
                return resp;
            }
            let n = devs.len().min(3);
            let cw = (x1 - x0) / n as f32;
            for (k, d) in devs.iter().take(n).enumerate() {
                let cx = x0 + cw * (k as f32 + 0.5);
                let t = Tone::of(d);
                device_glyph(p, d, Rect::from_center_size(Pos2::new(cx, r.bottom() - 34.0), Vec2::splat(16.0)), t.c(TEXT_SOFT()));
                pct_with_bolt(p, Pos2::new(cx, r.bottom() - 10.0), Align2::CENTER_BOTTOM, d.battery(), d.charging && d.live(), bold(12.0), t.text(d.battery()));
            }
        }
        Size::S14 => {
            if devs.is_empty() {
                tool_chip(p, Rect::from_min_size(Pos2::new(r.left() + 12.0, r.center().y - 17.0), Vec2::splat(34.0)), Icon::Bluetooth, tint::HEALTH());
                let x0 = r.left() + 58.0;
                text_fit(p, Pos2::new(x0, r.center().y), Align2::LEFT_CENTER, "No devices connected", bold(13.0), TEXT(), r.right() - 12.0 - x0);
                return resp;
            }
            let n = devs.len().clamp(1, 5);
            let cw = (r.width() - 2.0 * tile::PAD) / n.max(4) as f32;
            let x0 = r.left() + tile::PAD + (r.width() - 2.0 * tile::PAD - cw * n as f32) / 2.0;
            for (k, d) in devs.iter().take(n).enumerate() {
                let cx = x0 + cw * (k as f32 + 0.5);
                let t = Tone::of(d);
                // the charge inside its ring, the whole name under it (two
                // lines when it needs them: "Magic / Trackpad")
                let c = Pos2::new(cx, r.top() + tile::PAD + 16.0);
                let b = d.battery();
                tile::ring(p, c, 16.0, 4.0, frac(b), t.ring(b));
                if d.charging && d.live() {
                    let at = c + Vec2::new(16.0 * 0.72, -16.0 * 0.72);
                    p.circle_filled(at, 16.0 * 0.32, BG());
                    bolt(p, at, 16.0 * 0.42, GREEN());
                }
                p.text(c, Align2::CENTER_CENTER, b.map(|b| b.to_string()).unwrap_or_else(|| "–".into()), bold(11.0), t.text(b));
                text_wrap(p, Pos2::new(cx, c.y + 16.0 + sp::S), Align2::CENTER_TOP, &d.name, font(11.0), t.c(TEXT_DIM()), cw - sp::XS, 2);
            }
        }
        Size::S24 => {
            let body = tile::head(p, r, Icon::Bluetooth, tint::HEALTH(), "Devices", &note(&devs));
            if devs.is_empty() {
                tile::empty_note(p, body, "No Bluetooth devices connected");
                return resp;
            }
            let lh = (body.height() / 3.0).min(40.0);
            for (k, d) in devs.iter().take(3).enumerate() {
                let lr = Rect::from_min_size(Pos2::new(body.left(), body.top() + k as f32 * lh), Vec2::new(body.width(), lh));
                if k > 0 {
                    hairline(p, lr.left_top(), lr.right_top());
                }
                device_line(p, lr, d);
            }
        }
        _ => large(&devs, p, r),
    }
    resp
}

/// One device in a list: glyph, name with its parts or kind under it, a
/// bar and the charge.
fn device_line(p: &egui::Painter, lr: Rect, d: &Device) {
    device_line_sub(p, lr, d, &d.detail());
}

/// [`device_line`] with its own line under the name.
fn device_line_sub(p: &egui::Painter, lr: Rect, d: &Device, sub: &str) {
    let b = d.battery();
    let t = Tone::of(d);
    let cy = lr.center().y;
    device_glyph(p, d, Rect::from_center_size(Pos2::new(lr.left() + 9.0, cy), Vec2::splat(17.0)), t.c(TEXT_SOFT()));
    let vr = pct_with_bolt(p, Pos2::new(lr.right(), cy), Align2::RIGHT_CENTER, b, d.charging && d.live(), bold(12.0), t.text(b));
    // the bar only when the name still has room next to it
    let bw = if lr.width() >= 300.0 { 64.0 } else { 0.0 };
    let bx1 = vr.left() - 10.0;
    if bw > 0.0 {
        bar(p, Rect::from_min_max(Pos2::new(bx1 - bw, cy - 2.0), Pos2::new(bx1, cy + 2.0)), frac(b).max(0.01), t.ring(b));
    }
    let tx = lr.left() + 26.0;
    let tw = bx1 - bw - tx - if bw > 0.0 { 10.0 } else { 0.0 };
    if sub.is_empty() {
        text_fit(p, Pos2::new(tx, cy), Align2::LEFT_CENTER, &d.name, font(12.0), t.c(TEXT()), tw);
    } else {
        two_lines(p, tx, cy, &d.name, font(12.0), t.c(TEXT()), sub, font(11.0), TEXT_FAINT(), tw);
    }
}

/// 4:4: earbuds get a card with a ring per part, other devices a line each.
fn large(devs: &[Device], p: &egui::Painter, r: Rect) {
    let body = tile::head(p, r, Icon::Bluetooth, tint::HEALTH(), "Devices", &note(devs));
    if devs.is_empty() {
        tile::empty_note(p, body, "No Bluetooth devices connected");
        return;
    }
    let mut y = body.top();
    let bottom = body.bottom();
    // connected earbuds first, as a card with a ring per part
    let pods = devs.iter().find(|d| d.split() && d.live());
    if let Some(d) = pods {
        let cr = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), 112.0));
        p.rect_filled(cr, 10.0, white(10));
        device_glyph(p, d, Rect::from_center_size(Pos2::new(cr.left() + 19.0, cr.top() + 17.0), Vec2::splat(17.0)), TEXT_SOFT());
        text_fit(p, Pos2::new(cr.left() + 36.0, cr.top() + 17.0), Align2::LEFT_CENTER, &d.name, bold(13.0), TEXT(), cr.width() - 48.0);
        let parts = d.parts();
        let cw = cr.width() / parts.len() as f32;
        for (k, (label, v, charging)) in parts.iter().enumerate() {
            let c = Pos2::new(cr.left() + cw * (k as f32 + 0.5), cr.top() + 60.0);
            tile::ring(p, c, 22.0, 5.0, frac(*v), Tone { a: 1.0 }.ring(*v));
            p.text(c, Align2::CENTER_CENTER, pct_text(*v), bold(12.0), Tone { a: 1.0 }.text(*v));
            let lr = p.text(Pos2::new(c.x, cr.bottom() - 10.0), Align2::CENTER_BOTTOM, *label, font(11.0), TEXT_DIM());
            if *charging && v.is_some() {
                bolt(p, Pos2::new(lr.left() - 7.0, lr.center().y), 10.0, GREEN());
            }
        }
        y = cr.bottom() + 8.0;
    }
    let lh = 40.0;
    for d in devs.iter().filter(|d| !pods.is_some_and(|f| std::ptr::eq(f, *d))) {
        if y + lh > bottom + 0.5 {
            break;
        }
        let lr = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), lh));
        p.rect_filled(lr, 10.0, white(10));
        device_line(p, lr.shrink2(Vec2::new(10.0, 0.0)), d);
        y += lh + 6.0;
    }
}

// ------------------------------------------------------------------- page

/// What the Devices page asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageAction {
    ToggleAlerts,
    ToggleCharged,
    ChargedAt(u8),
    ToggleMenuBar,
    OpenBluetooth,
    /// Alert below this charge for one device (0 = never).
    AlertBelow(String, u8),
    Hide(String, bool),
    Forget(String),
}

const ROW_GAP: f32 = 8.0;
const SWITCH_H: f32 = sp::SETTINGS_ROW_H;
const LINE_H: f32 = 44.0;
const LINE_GAP: f32 = 6.0;

/// Height of the monitoring settings.
pub fn settings_height(s: &Snapshot) -> f32 {
    let hidden = if s.hidden_devices.is_empty() { 0.0 } else { 26.0 + s.hidden_devices.len() as f32 * (44.0 + ROW_GAP) + 18.0 };
    26.0 + 8.0 + 3.0 * SWITCH_H + hidden + 18.0 + 30.0
}

/// Mac Health settings: device alerts and the menu bar, hidden devices,
/// and the way to Bluetooth settings.
pub fn settings(ui: &mut Ui, r: Rect, s: &Snapshot) -> Option<PageAction> {
    let p = ui.painter().clone();
    let mut out = None;
    let mut y = r.top();
    p.text(Pos2::new(r.left() + 4.0, y + 10.0), Align2::LEFT_CENTER, "Bluetooth devices", bold(12.0), TEXT_DIM());
    y += 26.0;
    let prefs = &s.bt_prefs;
    let card = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 8.0 + 3.0 * SWITCH_H));
    glass_card(&p, card, R_CARD, 0.0, false);
    let rows: [(&str, bool, PageAction); 3] = [
        ("Low battery alerts", crate::kit::notify::shown(prefs.alerts), PageAction::ToggleAlerts),
        ("Charged alert", crate::kit::notify::shown(prefs.charged), PageAction::ToggleCharged),
        ("Show in menu bar", prefs.menu_bar, PageAction::ToggleMenuBar),
    ];
    for (k, (label, on, act)) in rows.into_iter().enumerate() {
        let rr = Rect::from_min_size(Pos2::new(card.left(), card.top() + 4.0 + k as f32 * SWITCH_H), Vec2::new(card.width(), SWITCH_H));
        if k > 0 {
            hairline(&p, Pos2::new(rr.left() + 12.0, rr.top()), Pos2::new(rr.right() - 12.0, rr.top()));
        }
        let sr = Rect::from_min_size(Pos2::new(rr.right() - 12.0 - 36.0, rr.center().y - 10.0), Vec2::new(36.0, 20.0));
        if switch(ui, sr, ui.scope_id().with(("dev-switch", label)), on, true).clicked() {
            out = Some(act.clone());
        }
        let mut tw = sr.left() - rr.left() - 24.0;
        if act == PageAction::ToggleCharged && on {
            // at 80, 90 or 100%
            let labels: Vec<String> = CHARGED_CHOICES.iter().map(|c| format!("{c}%")).collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let sel = CHARGED_CHOICES.iter().position(|c| *c == prefs.charged_at).unwrap_or(CHARGED_CHOICES.len() - 1);
            let seg = Rect::from_min_size(Pos2::new(sr.left() - 14.0 - 150.0, rr.center().y - 13.0), Vec2::new(150.0, 26.0));
            if let Some(i) = segmented(ui, seg, ui.scope_id().with("dev-charged-at"), &refs, sel) {
                out = Some(PageAction::ChargedAt(CHARGED_CHOICES[i]));
            }
            tw = seg.left() - rr.left() - 24.0;
        }
        text_fit(&p, Pos2::new(rr.left() + 12.0, rr.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT(), tw);
    }
    y = card.bottom() + 18.0;
    if !s.hidden_devices.is_empty() {
        p.text(Pos2::new(r.left() + 4.0, y + 10.0), Align2::LEFT_CENTER, "Hidden devices", bold(12.0), TEXT_DIM());
        y += 26.0;
        for d in &s.hidden_devices {
            let rr = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 44.0));
            glass_card(&p, rr, R_CARD, 0.0, false);
            let t = Tone { a: AWAY_ALPHA };
            device_glyph(&p, d, Rect::from_center_size(Pos2::new(rr.left() + 22.0, rr.center().y), Vec2::splat(18.0)), t.c(TEXT_SOFT()));
            let bw = button_width(&p, "Show", BTN_SECONDARY);
            let br = Rect::from_min_size(Pos2::new(rr.right() - 10.0 - bw, rr.center().y - 13.0), Vec2::new(bw, 26.0));
            if pill_button(ui, br, "Show", BTN_SECONDARY, true).clicked() {
                out = Some(PageAction::Hide(d.key(), false));
            }
            let vr = p.text(Pos2::new(br.left() - 12.0, rr.center().y), Align2::RIGHT_CENTER, pct_text(d.battery()), bold(12.0), t.c(TEXT_DIM()));
            text_fit(&p, Pos2::new(rr.left() + 42.0, rr.center().y), Align2::LEFT_CENTER, &d.name, font(13.0), t.c(TEXT()), vr.left() - rr.left() - 54.0);
            y += 44.0 + ROW_GAP;
        }
        y += 18.0 - ROW_GAP;
    }
    let bw = button_width(&p, "Bluetooth Settings", BTN_SECONDARY);
    let br = Rect::from_min_size(Pos2::new(r.right() - bw, y), Vec2::new(bw, 30.0));
    if pill_button(ui, br, "Bluetooth Settings", BTN_SECONDARY, true).clicked() {
        out = Some(PageAction::OpenBluetooth);
    }
    out
}

/// The body of the Devices card on Mac Health: a line per device, lowest
/// first, each with its menu; "+N more" when they don't all fit.
pub fn card_body(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<PageAction> {
    let mut out = None;
    let devs = devices::lowest_first(&s.devices);
    if devs.is_empty() {
        let cy = r.center().y - 16.0;
        icon(p, Icon::Bluetooth, Rect::from_center_size(Pos2::new(r.center().x, cy - 18.0), Vec2::splat(26.0)), TEXT_FAINT());
        p.text(Pos2::new(r.center().x, cy + 10.0), Align2::CENTER_CENTER, "No devices connected", font(13.0), TEXT_DIM());
        let bw = button_width(p, "Bluetooth Settings", BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(r.left(), r.bottom() - 26.0), Vec2::new(bw, 26.0));
        if pill_button(ui, br, "Bluetooth Settings", BTN_SECONDARY, true).clicked() {
            out = Some(PageAction::OpenBluetooth);
        }
        return out;
    }
    let fit = (((r.height() + LINE_GAP) / (LINE_H + LINE_GAP)).floor() as usize).max(1);
    let (shown, more) = if devs.len() > fit { (fit - 1, devs.len() - (fit - 1)) } else { (devs.len(), 0) };
    let mut y = r.top();
    for d in devs.iter().take(shown) {
        let lr = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), LINE_H));
        p.rect_filled(lr, 10.0, white(10));
        let mr = Rect::from_center_size(Pos2::new(lr.right() - 18.0, lr.center().y), Vec2::new(24.0, 22.0));
        let mut sub = d.detail();
        if let Some(e) = d.eta {
            sub.push_str(&format!(" · {} left", devices::duration_left(e)));
        }
        device_line_sub(p, Rect::from_min_max(Pos2::new(lr.left() + 10.0, lr.top()), Pos2::new(mr.left() - 6.0, lr.bottom())), d, &sub);
        let key = d.key();
        let resp = dots(ui, mr, ui.scope_id().with(("dev-card-more", &key)));
        if let Some(a) = device_menu(&resp, d, &s.bt_prefs) {
            out = Some(a);
        }
        y += LINE_H + LINE_GAP;
    }
    if more > 0 {
        let lr = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), LINE_H));
        let names: Vec<&str> = devs.iter().skip(shown).map(|d| d.name.as_str()).collect();
        let t = format!("+{more} more");
        p.text(Pos2::new(lr.left() + 10.0, lr.center().y), Align2::LEFT_CENTER, &t, font(12.0), TEXT_DIM());
        ui.interact(lr, ui.scope_id().with("dev-card-rest"), Sense::hover()).on_hover_text(names.join("\n"));
    }
    out
}

/// A device's menu: its own alert line, hide and (when away) forget.
fn device_menu(resp: &Response, d: &Device, prefs: &Prefs) -> Option<PageAction> {
    let mut out = None;
    let key = d.key();
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(180.0);
        let below = prefs.alert_below(&key);
        ui.menu_button("Alert below", |ui| {
            for c in ALERT_CHOICES {
                let label = if c == 0 { "Never".to_string() } else { format!("{c}%") };
                if ui.selectable_label(below == c, label).clicked() {
                    out = Some(PageAction::AlertBelow(key.clone(), c));
                }
            }
        });
        if ui.button("Hide").clicked() {
            out = Some(PageAction::Hide(key.clone(), true));
        }
        if !d.live() && ui.button("Forget").clicked() {
            out = Some(PageAction::Forget(key.clone()));
        }
    });
    out
}

/// A "⋯" button.
fn dots(ui: &mut Ui, r: Rect, id: egui::Id) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered());
    let p = ui.painter();
    if h > 0.0 {
        p.rect_filled(r, 6.0, white((18.0 * h) as u8));
    }
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(r.center() + Vec2::new(dx, 0.0), 1.6, TEXT_FAINT());
    }
    resp.on_hover_text("More")
}
