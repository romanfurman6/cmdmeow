//! First-visit screens for pages that make or find things (Cleanup before
//! its first scan, Screenshot and Video before the first capture,
//! Dictation before the first take): a picture in the middle and one big
//! button under it, with the shortcut on keycaps inside the button.

use crate::kit::glass::*;
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Response, Sense, Ui, Vec2};

/// The big button's height.
pub const HERO_H: f32 = 44.0;
/// The picture tile above a hero button (Screenshot, Video).
pub const TILE: Vec2 = Vec2::new(168.0, 112.0);

/// What goes before a hero button's label.
#[derive(Clone, Copy, PartialEq)]
pub enum Lead {
    None,
    Icon(Icon),
    /// A filled dot (Record).
    Dot(Color32),
    /// A stop square.
    Stop,
}

/// "⇧⌘2" as its keys: one cap per modifier, the rest as the last cap.
pub fn caps(keys: &str) -> Vec<String> {
    let mods: Vec<String> = keys.chars().take_while(|c| "⌃⌥⇧⌘".contains(*c)).map(|c| c.to_string()).collect();
    let key: String = keys.chars().skip(mods.len()).collect();
    let mut v = mods;
    if !key.is_empty() {
        v.push(key);
    }
    v
}

const CAP_H: f32 = 22.0;
const CAP_GAP: f32 = 3.0;

fn cap_w(p: &egui::Painter, c: &str, size: f32) -> f32 {
    p.layout_no_wrap(c.to_owned(), bold(size), TEXT()).size().x.max(9.0) + 12.0
}

/// How wide [`keycaps`] draws `keys`.
pub fn keycaps_width(p: &egui::Painter, keys: &str, size: f32) -> f32 {
    let caps = caps(keys);
    caps.iter().map(|c| cap_w(p, c, size)).sum::<f32>() + CAP_GAP * caps.len().saturating_sub(1) as f32
}

/// `keys` as small caps from `left` (vertical centre `left.y`). `on` is the
/// fill they sit on: a solid button gets dark caps in its own ink, anything
/// else quiet white ones. Returns the right edge.
pub fn keycaps(p: &egui::Painter, left: Pos2, keys: &str, size: f32, on: Option<Color32>) -> f32 {
    let mut x = left.x;
    let h = if size < 12.0 { CAP_H - 2.0 } else { CAP_H };
    for c in caps(keys) {
        let w = cap_w(p, &c, size);
        let r = Rect::from_min_size(Pos2::new(x, left.y - h / 2.0), Vec2::new(w, h));
        let (fill, ink) = match on {
            Some(f) => (with_alpha(on_color(f), 0.12), on_color(f)),
            None => (white(18), TEXT_DIM()),
        };
        p.rect_filled(r, 6.0, fill);
        // centre the glyph's ink, not the line box: symbols sit low otherwise
        let g = p.layout_no_wrap(c, bold(size), ink);
        let ink_box = g.mesh_bounds;
        let pos = Pos2::new(r.center().x - g.size().x / 2.0, r.center().y - ink_box.center().y);
        p.galley(pos, g, ink);
        x = r.right() + CAP_GAP;
    }
    x - CAP_GAP
}

fn lead_w(lead: Lead) -> f32 {
    match lead {
        Lead::None => 0.0,
        Lead::Icon(_) => 18.0 + sp::S + 2.0,
        Lead::Dot(_) | Lead::Stop => 12.0 + sp::S + 2.0,
    }
}

/// How wide a [`hero_button`] is for this content.
pub fn hero_width(p: &egui::Painter, label: &str, keys: Option<&str>, lead: Lead) -> f32 {
    let text = p.layout_no_wrap(label.to_owned(), bold(16.0), TEXT()).size().x;
    let k = keys.filter(|k| !k.is_empty()).map(|k| sp::M + keycaps_width(p, k, 12.0)).unwrap_or(0.0);
    let pad_r = if k > 0.0 { sp::L } else { 22.0 };
    22.0 + lead_w(lead) + text + k + pad_r
}

/// The big button of a first-visit screen, centred on `center`, at least
/// `min_w` wide. `tint` None draws a quiet (secondary) one. The response
/// senses clicks; `is_pointer_button_down_on` works for hold-to-talk.
#[allow(clippy::too_many_arguments)]
pub fn hero_button(ui: &mut Ui, center: Pos2, id: Id, label: &str, keys: Option<&str>, lead: Lead, tint: Option<Color32>, enabled: bool, min_w: f32) -> Response {
    let p = ui.painter().clone();
    let w = hero_width(&p, label, keys, lead).max(min_w);
    let r = Rect::from_center_size(center, Vec2::new(w, HERO_H));
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    let hov = ui.ctx().animate_bool(id.with("h"), resp.hovered() && enabled);
    let a = if enabled { 1.0 } else { 0.45 };
    liquid_capsule(&p, r, tint, if resp.is_pointer_button_down_on() && enabled { 1.6 } else { hov }, a);
    let ink = tint.map(on_color).unwrap_or(TEXT());
    let ink = if enabled { ink } else { with_alpha(ink, 0.5) };
    // the content is centred as one row: lead, label, keys
    let text = p.layout_no_wrap(label.to_owned(), bold(16.0), ink);
    let k = keys.filter(|k| !k.is_empty());
    let kw = k.map(|k| sp::M + keycaps_width(&p, k, 12.0)).unwrap_or(0.0);
    let total = lead_w(lead) + text.size().x + kw;
    let mut x = r.center().x - total / 2.0;
    let cy = r.center().y;
    match lead {
        Lead::None => {}
        Lead::Icon(ic) => icon(&p, ic, Rect::from_center_size(Pos2::new(x + 9.0, cy), Vec2::splat(18.0)), ink),
        Lead::Dot(c) => {
            p.circle_filled(Pos2::new(x + 6.0, cy), 6.0, c);
        }
        Lead::Stop => {
            p.rect_filled(Rect::from_center_size(Pos2::new(x + 6.0, cy), Vec2::splat(11.0)), CornerRadius::same(2), ink);
        }
    }
    x += lead_w(lead);
    let tw = text.size().x;
    p.galley(Pos2::new(x, cy - text.size().y / 2.0), text, ink);
    x += tw;
    if let Some(k) = k {
        keycaps(&p, Pos2::new(x + sp::M, cy), k, 12.0, tint);
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A quiet 34 pt button with a glyph, a label and the shortcut after it
/// (Window and Screen under Capture area). `glyph` draws the icon in the
/// square it's given. Returns the response and the width used.
pub fn side_button(ui: &mut Ui, left: Pos2, id: Id, label: &str, keys: Option<&str>, enabled: bool, glyph: impl FnOnce(&egui::Painter, Rect)) -> (Response, f32) {
    let p = ui.painter().clone();
    let w = side_width(&p, label, keys);
    let r = Rect::from_min_size(Pos2::new(left.x, left.y - 17.0), Vec2::new(w, 34.0));
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    let hov = ui.ctx().animate_bool(id.with("h"), resp.hovered() && enabled);
    liquid_capsule(&p, r, None, if resp.is_pointer_button_down_on() && enabled { 1.6 } else { hov }, if enabled { 1.0 } else { 0.45 });
    let cy = r.center().y;
    glyph(&p, Rect::from_center_size(Pos2::new(r.left() + 12.0 + 7.5, cy), Vec2::splat(15.0)));
    let lr = p.text(Pos2::new(r.left() + 12.0 + 15.0 + sp::S, cy), Align2::LEFT_CENTER, label, font(13.0), TEXT_SOFT());
    if let Some(k) = keys.filter(|k| !k.is_empty()) {
        keycaps(&p, Pos2::new(lr.right() + sp::S, cy), k, 11.0, None);
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (resp, w)
}

/// How wide a [`side_button`] is.
pub fn side_width(p: &egui::Painter, label: &str, keys: Option<&str>) -> f32 {
    let text = p.layout_no_wrap(label.to_owned(), font(13.0), TEXT()).size().x;
    let k = keys.filter(|k| !k.is_empty()).map(|k| sp::S + keycaps_width(p, k, 11.0)).unwrap_or(0.0);
    12.0 + 15.0 + sp::S + text + k + 12.0
}

/// A round on/off chip with a glyph and a label (System audio, Microphone).
/// Returns the response and its width.
pub fn toggle_chip(ui: &mut Ui, left: Pos2, id: Id, label: &str, on: bool, glyph: impl FnOnce(&egui::Painter, Rect, Color32)) -> (Response, f32) {
    let p = ui.painter().clone();
    let w = toggle_width(&p, label);
    let r = Rect::from_min_size(Pos2::new(left.x, left.y - 17.0), Vec2::new(w, 34.0));
    let resp = ui.interact(r, id, Sense::click());
    let hov = ui.ctx().animate_bool(id.with("h"), resp.hovered());
    let (fill, edge, ink) = if on { (with_alpha(BLUE(), 0.16 + 0.06 * hov), with_alpha(BLUE(), 0.45), ACCENT()) } else { (white((8.0 + 10.0 * hov) as u8), white(32), TEXT_DIM()) };
    p.rect_filled(r, 17.0, fill);
    p.rect_stroke(r, 17.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    let cy = r.center().y;
    glyph(&p, Rect::from_center_size(Pos2::new(r.left() + 12.0 + 7.5, cy), Vec2::splat(15.0)), ink);
    p.text(Pos2::new(r.left() + 12.0 + 15.0 + sp::S, cy), Align2::LEFT_CENTER, label, font(13.0), ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (resp, w)
}

/// How wide a [`toggle_chip`] is.
pub fn toggle_width(p: &egui::Painter, label: &str) -> f32 {
    12.0 + 15.0 + sp::S + p.layout_no_wrap(label.to_owned(), font(13.0), TEXT()).size().x + 14.0
}

/// The picture tile above a hero button: a soft tinted card.
pub fn tile(p: &egui::Painter, r: Rect, tint: Color32) {
    p.rect_filled(r, 14.0, with_alpha(tint, 0.08));
    p.rect_stroke(r, 14.0, egui::Stroke::new(1.0, with_alpha(tint, 0.22)), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::caps;

    #[test]
    fn shortcuts_split_into_one_cap_per_key() {
        assert_eq!(caps("⇧⌘2"), vec!["⇧", "⌘", "2"]);
        assert_eq!(caps("⌥D"), vec!["⌥", "D"]);
        assert_eq!(caps("⌥Space"), vec!["⌥", "Space"]);
        assert_eq!(caps("F5"), vec!["F5"]);
        assert!(caps("").is_empty());
    }
}
