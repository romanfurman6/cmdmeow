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
use crate::kit::menu;
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

/// How much a device that isn't connected is dimmed.
const AWAY_ALPHA: f32 = 0.45;

fn pct_text(b: Option<u8>) -> String {
    b.map(|b| format!("{b}%")).unwrap_or_else(|| "–".into())
}

/// A device's name without its owner ("Roman's AirPods Pro" → "AirPods
/// Pro"), the way it reads in a small space.
pub fn short_name(name: &str) -> String {
    for sep in ["'s ", "’s ", "s' ", "s’ "] {
        if let Some(i) = name.find(sep) {
            let rest = name[i + sep.len()..].trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    name.to_string()
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
                // the colour is in the ring only; the value reads like every other 1:1
                pct_with_bolt(p, Pos2::new(r.center().x, r.bottom() - 10.0), Align2::CENTER_BOTTOM, b, false, bold(13.0), Tone::of(d).c(TEXT()));
                resp.on_hover_text(format!("{} · {}", d.name, note(&devs)))
            }
            Some(d) => tile::small(ui, r, wid, d.kind.icon(), tint::HEALTH(), &d.name, "Connected", sel),
            None => tile::small(ui, r, wid, Icon::Bluetooth, tint::HEALTH(), "Devices", "None", sel),
        };
    }
    // 1:2 and 1:4 (W2, W5): the weakest device as the ring, the rest as bar lines
    if matches!(size, Size::S12 | Size::S14) {
        let first = devs.first().filter(|d| d.live());
        let (glyph, ring_, label, value) = match first {
            Some(d) => (d.kind.icon(), d.battery().map(|b| (frac(Some(b)), devices::color(b))), short_name(&d.name), pct_text(d.battery())),
            None => (Icon::Bluetooth, None, "Devices".to_string(), "None".to_string()),
        };
        // the weakest device is the value; the caption says how many there are
        let caption = if first.is_some() { format!("{} connected", devs.iter().filter(|d| d.live()).count()) } else { String::new() };
        if size == Size::S12 {
            return tile::wide_full(ui, r, wid, glyph, tint::HEALTH(), ring_, &label, &value, &caption, ty::caption_color(), sel);
        }
        let (resp, right) = tile::row_split_full(ui, r, wid, glyph, tint::HEALTH(), ring_, &label, &value, &caption, ty::caption_color(), sel);
        let others: Vec<&Device> = devs.iter().filter(|d| d.live()).skip(1).collect();
        let line = |name: &str, b: Option<u8>, v: String| (name.to_string(), b.map(|b| (frac(Some(b)), devices::color(b))), v);
        let lines: Vec<(String, Option<(f32, Color32)>, String)> = match first {
            // one device: its parts (AirPods: left, right and case), not "no other devices"
            Some(d) if others.is_empty() => {
                let mut v = Vec::new();
                if d.left.is_some() {
                    v.push(line("Left", d.left, pct_text(d.left)));
                }
                if d.right.is_some() {
                    v.push(line("Right", d.right, pct_text(d.right)));
                }
                if d.case.is_some() {
                    v.push(line("Case", d.case, pct_text(d.case)));
                }
                v
            }
            // the device's kind ("Mouse", "Trackpad"), not a name cut at the bar
            _ => others.iter().map(|d| line(&d.short(), d.battery(), pct_text(d.battery()))).collect(),
        };
        tile::lines(ui.painter(), right, &lines, "");
        return resp;
    }
    let resp = tile::card(ui, r, wid, sel);
    let p = ui.painter();
    match size {
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
    device_line_sub(p, lr, d, &d.detail(), bar_w(lr.width()));
}

/// How long a device line's charge bar is in a list `w` wide: 64 in the
/// widgets, 40 in the narrower Mac Health card, none when the name would
/// have no room left. Picked from the list's width, so a row's "⋯" on hover
/// slides the bar along instead of hiding it.
fn bar_w(w: f32) -> f32 {
    if w >= 300.0 {
        64.0
    } else if w >= 200.0 {
        40.0
    } else {
        0.0
    }
}

/// [`device_line`] with its own line under the name.
fn device_line_sub(p: &egui::Painter, lr: Rect, d: &Device, sub: &str, bw: f32) {
    let b = d.battery();
    let t = Tone::of(d);
    let cy = lr.center().y;
    device_glyph(p, d, Rect::from_center_size(Pos2::new(lr.left() + 9.0, cy), Vec2::splat(17.0)), t.c(TEXT_SOFT()));
    let vr = pct_with_bolt(p, Pos2::new(lr.right(), cy), Align2::RIGHT_CENTER, b, d.charging && d.live(), bold(12.0), t.text(b));
    // the bar ends at the same x on every line ("100%" is the widest
    // charge), so bars line up (W5); its length comes from the list (bar_w)
    let col = p.layout_no_wrap("100%".into(), bold(12.0), TEXT()).size().x.max(vr.width());
    let bx1 = lr.right() - col - 10.0;
    if bw > 0.0 {
        bar(p, Rect::from_min_max(Pos2::new(bx1 - bw, cy - 2.0), Pos2::new(bx1, cy + 2.0)), frac(b).max(0.01), t.ring(b));
    }
    let tx = lr.left() + 26.0;
    let tw = bx1 - bw - tx - if bw > 0.0 { 10.0 } else { 0.0 };
    // a device's name is someone's text: it fades, never "…"
    if sub.is_empty() {
        text_fade(p, Pos2::new(tx, cy), &d.name, font(12.0), t.c(TEXT()), tw);
    } else {
        let (y1, y2) = two_lines_y(p, cy, &font(12.0), &font(11.0));
        text_fade(p, Pos2::new(tx, y1), &d.name, font(12.0), t.c(TEXT()), tw);
        text_fade(p, Pos2::new(tx, y2), sub, font(11.0), TEXT_FAINT(), tw);
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

const SWITCH_H: f32 = sp::SETTINGS_ROW_H;
const LINE_H: f32 = 44.0;
const LINE_GAP: f32 = 6.0;

/// Height of the monitoring settings.
pub fn settings_height(s: &Snapshot) -> f32 {
    let hidden = if s.hidden_devices.is_empty() { 0.0 } else { sp::XL + TITLE_H + s.hidden_devices.len() as f32 * sp::ROW_H };
    TITLE_H + 4.0 * SWITCH_H + hidden
}

/// A card title and the 12 under it (R2).
const TITLE_H: f32 = 14.0 + sp::M;

/// Mac Health settings: device alerts and the menu bar, the way to
/// Bluetooth settings, and hidden devices. One card per list (S4).
pub fn settings(ui: &mut Ui, r: Rect, s: &Snapshot) -> Option<PageAction> {
    let p = ui.painter().clone();
    let mut out = None;
    let mut y = r.top();
    crate::kit::glass::card_title(&p, Pos2::new(r.left(), y), "Bluetooth devices");
    y += TITLE_H;
    let prefs = &s.bt_prefs;
    let card = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 4.0 * SWITCH_H));
    glass_card(&p, card, R_CARD, 0.0, false);
    let rows: [(&str, bool, PageAction); 3] = [
        ("Low battery alerts", crate::kit::notify::shown(prefs.alerts), PageAction::ToggleAlerts),
        ("Charged alert", crate::kit::notify::shown(prefs.charged), PageAction::ToggleCharged),
        ("Show in menu bar", prefs.menu_bar, PageAction::ToggleMenuBar),
    ];
    let row = |k: usize| Rect::from_min_size(Pos2::new(card.left(), card.top() + k as f32 * SWITCH_H), Vec2::new(card.width(), SWITCH_H));
    for (k, (label, on, act)) in rows.into_iter().enumerate() {
        let rr = row(k);
        if k > 0 {
            hairline(&p, Pos2::new(rr.left() + sp::L, rr.top()), Pos2::new(rr.right() - sp::L, rr.top()));
        }
        let sr = Rect::from_min_size(Pos2::new(rr.right() - 42.0, rr.center().y - 9.0), Vec2::new(30.0, 18.0));
        if switch(ui, sr, ui.scope_id().with(("dev-switch", label)), on, true).clicked() {
            out = Some(act.clone());
        }
        let mut tw = sr.left() - rr.left() - 2.0 * sp::L;
        if act == PageAction::ToggleCharged && on {
            // at 80, 90 or 100%
            let labels: Vec<String> = CHARGED_CHOICES.iter().map(|c| format!("{c}%")).collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let sel = CHARGED_CHOICES.iter().position(|c| *c == prefs.charged_at).unwrap_or(CHARGED_CHOICES.len() - 1);
            let seg = Rect::from_min_size(Pos2::new(sr.left() - sp::M - 150.0, rr.center().y - sp::CONTROL_H / 2.0), Vec2::new(150.0, sp::CONTROL_H));
            if let Some(i) = segmented(ui, seg, ui.scope_id().with("dev-charged-at"), &refs, sel) {
                out = Some(PageAction::ChargedAt(CHARGED_CHOICES[i]));
            }
            tw = seg.left() - rr.left() - 2.0 * sp::L;
        }
        text_fit(&p, Pos2::new(rr.left() + sp::L, rr.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT(), tw);
    }
    // the way to macOS's own Bluetooth settings, as the card's last row
    let rr = row(3);
    hairline(&p, Pos2::new(rr.left() + sp::L, rr.top()), Pos2::new(rr.right() - sp::L, rr.top()));
    p.text(Pos2::new(rr.left() + sp::L, rr.center().y), Align2::LEFT_CENTER, "Bluetooth Settings", font(13.0), TEXT());
    let bw = button_width(&p, "Open", BTN_SECONDARY);
    let br = Rect::from_min_size(Pos2::new(rr.right() - sp::M - bw, rr.center().y - sp::CONTROL_H / 2.0), Vec2::new(bw, sp::CONTROL_H));
    if pill_button(ui, br, "Open", BTN_SECONDARY, true).clicked() {
        out = Some(PageAction::OpenBluetooth);
    }
    y = card.bottom() + sp::XL;
    if !s.hidden_devices.is_empty() {
        crate::kit::glass::card_title(&p, Pos2::new(r.left(), y), "Hidden devices");
        y += TITLE_H;
        let list = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), s.hidden_devices.len() as f32 * sp::ROW_H));
        glass_card(&p, list, R_CARD, 0.0, false);
        for (k, d) in s.hidden_devices.iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), sp::ROW_H));
            if k > 0 {
                hairline(&p, Pos2::new(rr.left() + sp::L, rr.top()), Pos2::new(rr.right() - sp::L, rr.top()));
            }
            let t = Tone { a: AWAY_ALPHA };
            device_glyph(&p, d, Rect::from_center_size(Pos2::new(rr.left() + 26.0, rr.center().y), Vec2::splat(18.0)), t.c(TEXT_SOFT()));
            let bw = button_width(&p, "Show", BTN_SECONDARY);
            let br = Rect::from_min_size(Pos2::new(rr.right() - sp::M - bw, rr.center().y - sp::CONTROL_H / 2.0), Vec2::new(bw, sp::CONTROL_H));
            if pill_button(ui, br, "Show", BTN_SECONDARY, true).clicked() {
                out = Some(PageAction::Hide(d.key(), false));
            }
            let vr = p.text(Pos2::new(br.left() - sp::M, rr.center().y), Align2::RIGHT_CENTER, pct_text(d.battery()), bold(12.0), t.c(TEXT_DIM()));
            text_fit(&p, Pos2::new(rr.left() + 46.0, rr.center().y), Align2::LEFT_CENTER, &d.name, font(13.0), t.c(TEXT()), vr.left() - rr.left() - 58.0);
            y += sp::ROW_H;
        }
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
        let br = Rect::from_min_size(Pos2::new(r.left(), r.bottom() - BTN_H_ROW), Vec2::new(bw, BTN_H_ROW));
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
        if y > r.top() {
            hairline(p, Pos2::new(lr.left(), y - LINE_GAP / 2.0), Pos2::new(lr.right(), y - LINE_GAP / 2.0));
        }
        let mr = Rect::from_center_size(Pos2::new(lr.right() - 12.0, lr.center().y), Vec2::new(24.0, 22.0));
        let mut sub = d.detail();
        if let Some(e) = d.eta {
            sub.push_str(&format!(" · {} left", devices::duration_left(e)));
        }
        let key = d.key();
        let (resp, shown) = dots(ui, mr, lr, ui.scope_id().with(("dev-card-more", &key)));
        // the value sits on the right edge and slides left while "⋯" shows
        let right = lr.right() - shown * (mr.width() + 6.0);
        device_line_sub(p, Rect::from_min_max(lr.left_top(), Pos2::new(right, lr.bottom())), d, &sub, bar_w(lr.width()));
        if let Some(a) = device_menu(&resp, d, &s.bt_prefs) {
            out = Some(a);
        }
        y += LINE_H + LINE_GAP;
    }
    if more > 0 {
        let lr = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), LINE_H));
        let names: Vec<&str> = devs.iter().skip(shown).map(|d| d.name.as_str()).collect();
        let t = format!("+{more} more");
        p.text(Pos2::new(lr.left(), lr.center().y), Align2::LEFT_CENTER, &t, font(12.0), TEXT_DIM());
        ui.interact(lr, ui.scope_id().with("dev-card-rest"), Sense::hover()).on_hover_text(names.join("\n"));
    }
    out
}

/// A device's menu: its own alert line, hide and (when away) forget.
fn device_menu(resp: &Response, d: &Device, prefs: &Prefs) -> Option<PageAction> {
    let mut out = None;
    let key = d.key();
    menu::popup(resp).show(|ui| {
        let below = prefs.alert_below(&key);
        menu::sub(ui, "Alert below", |ui| {
            for c in ALERT_CHOICES {
                let label = if c == 0 { "Never".to_string() } else { format!("{c}%") };
                if menu::choice(ui, &label, below == c).clicked() {
                    out = Some(PageAction::AlertBelow(key.clone(), c));
                }
            }
        });
        if menu::item(ui, "Hide").clicked() {
            out = Some(PageAction::Hide(key.clone(), true));
        }
        if !d.live() && menu::item(ui, "Forget").clicked() {
            out = Some(PageAction::Forget(key.clone()));
        }
    });
    out
}

/// A "⋯" button, shown while its row is hovered or its menu is open.
/// Returns how far it is shown, 0 to 1.
fn dots(ui: &mut Ui, r: Rect, row: Rect, id: egui::Id) -> (Response, f32) {
    let resp = ui.interact(r, id, Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let shown = ui.ctx().animate_bool_with_time(id.with("show"), open || ui.rect_contains_pointer(row), 0.12);
    if shown <= 0.0 {
        return (resp, 0.0);
    }
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered());
    let p = ui.painter();
    if h > 0.0 {
        p.rect_filled(r, 6.0, white((18.0 * h) as u8));
    }
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(r.center() + Vec2::new(dx, 0.0), 1.6, TEXT_FAINT().gamma_multiply(shown));
    }
    (resp.on_hover_text("More"), shown)
}

#[cfg(test)]
mod short_name_tests {
    #[test]
    fn owner_is_dropped() {
        assert_eq!(super::short_name("Roman's AirPods Pro"), "AirPods Pro");
        assert_eq!(super::short_name("Roman’s AirPods Pro"), "AirPods Pro");
        assert_eq!(super::short_name("Magic Mouse"), "Magic Mouse");
    }
}
