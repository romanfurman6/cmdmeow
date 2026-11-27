//! Where CmdMeow lives (Settings › General, and a setup step): under its
//! menu bar icon, or in a black island that opens from the notch. One or
//! the other, never both. One widget layout serves both (the panel's).

use std::sync::atomic::{AtomicU8, Ordering};

/// Saved form: "menubar" or "notch".
pub const PLACE_KEY: &str = "app.place";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Menubar,
    Notch,
}

impl Place {
    pub const ALL: [Place; 2] = [Place::Menubar, Place::Notch];
    pub fn label(self) -> &'static str {
        match self {
            Place::Menubar => ts!("app.place.menubar"),
            Place::Notch => ts!("app.place.notch"),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Place::Menubar => "menubar",
            Place::Notch => "notch",
        }
    }
    pub fn parse(s: &str) -> Option<Place> {
        Place::ALL.into_iter().find(|p| p.key() == s)
    }
    /// The island under the notch shows.
    pub fn notch(self) -> bool {
        self != Place::Menubar
    }
    /// The menu bar icon shows.
    pub fn menubar(self) -> bool {
        self != Place::Notch
    }
}

static PLACE: AtomicU8 = AtomicU8::new(0);

pub fn get() -> Place {
    Place::ALL[(PLACE.load(Ordering::Relaxed) as usize).min(Place::ALL.len() - 1)]
}

/// Pick a place: the menu bar icon follows (the recording clock still
/// shows in the menu bar either way).
pub fn set(p: Place) {
    PLACE.store(Place::ALL.iter().position(|x| *x == p).unwrap_or(0) as u8, Ordering::Relaxed);
    crate::kit::native::set_show_in_menu_bar_for(p.menubar(), p.notch());
}

/// Read back what was saved (nothing saved: the menu bar).
pub fn load(saved: Option<String>) -> Place {
    saved.as_deref().and_then(Place::parse).unwrap_or(Place::Menubar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_flags() {
        for p in Place::ALL {
            assert_eq!(Place::parse(p.key()), Some(p));
        }
        assert!(Place::Menubar.menubar() && !Place::Menubar.notch());
        assert!(!Place::Notch.menubar() && Place::Notch.notch());
        // an old "both" falls back to the menu bar
        assert_eq!(load(Some("both".into())), Place::Menubar);
        assert_eq!(load(None), Place::Menubar);
        assert_eq!(load(Some("notch".into())), Place::Notch);
        assert_eq!(load(Some("bogus".into())), Place::Menubar);
    }
}

// ---------------------------------------------------------------- setup step

use super::shell::App;
use crate::kit::glass::*;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

/// The setup step (drawn by onboarding): "Where CmdMeow lives", two
/// picture cards like the Theme step. A click picks it at once.
pub fn setup_place(app: &mut App, ui: &mut Ui, body: Rect, t: f32) {
    let p = ui.painter().clone();
    let gap = sp::M;
    let n = Place::ALL.len() as f32;
    let cw = (body.width() - (n - 1.0) * gap) / n;
    let title_h = 30.0 + 20.0;
    let ch = (cw * 0.75).min(body.height() - title_h - sp::L);
    let head = body.center().y - (title_h + ch) / 2.0;
    p.text(Pos2::new(body.left(), head), Align2::LEFT_TOP, ts!("app.place.title"), bold(22.0), TEXT());
    let top = head + title_h;
    let cur = get();
    for (k, place) in Place::ALL.into_iter().enumerate() {
        let a = ease((t - 0.06 * k as f32) / 0.4);
        let r = Rect::from_min_size(Pos2::new(body.left() + k as f32 * (cw + gap), top + (1.0 - a) * 8.0), Vec2::new(cw, ch));
        let id = ui.scope_id().with(("setup-place", k));
        let resp = ui.interact(r, id, Sense::click());
        let on = place == cur;
        let s = ui.ctx().animate_bool_with_time(id.with("on"), on, 0.18);
        let hov = ui.ctx().animate_bool(id.with("h"), resp.hovered());
        let r = r.translate(Vec2::new(0.0, -hov));
        art(&p, r, place, a);
        let rad = CornerRadius::same(12);
        // H4: the picked card gets the 1.5 action ring
        p.rect_stroke(r, rad, Stroke::new(1.0 + 0.5 * s, lerp_color(CARD_EDGE(), ACTION(), s).gamma_multiply(a)), StrokeKind::Inside);
        let inner = r.shrink(16.0);
        let ink = Color32::WHITE;
        let label_y = inner.bottom() - 10.0;
        text_fit(&p, Pos2::new(inner.left(), label_y), Align2::LEFT_CENTER, place.label(), bold(17.0), ink.gamma_multiply(a), inner.width() - 28.0);
        let cc = Pos2::new(inner.right() - 9.0, label_y);
        p.circle_stroke(cc, 8.5, Stroke::new(1.5, lerp_color(with_alpha(ink, 0.55), ACTION(), s).gamma_multiply(a)));
        if s > 0.01 {
            let z = pop(s);
            p.circle_filled(cc, 9.0 * z, ACTION());
            let sz = 7.0 * z;
            let pts = vec![cc + Vec2::new(-0.45, 0.0) * sz, cc + Vec2::new(-0.12, 0.33) * sz, cc + Vec2::new(0.45, -0.32) * sz];
            p.add(egui::Shape::line(pts, Stroke::new((sz * 0.22).max(1.2), ON_ACTION)));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && !on && !app.demo {
            set(place);
            ui.ctx().request_repaint();
        }
    }
}

/// A card's picture: the top of a screen with its menu bar, and where the
/// widgets open (under the icon, or from the notch).
fn art(p: &egui::Painter, r: Rect, place: Place, a: f32) {
    let rad = CornerRadius::same(12);
    let f = |c: Color32| c.gamma_multiply(a);
    // a dusk wallpaper, the same in every theme (like the theme pictures)
    p.rect_filled(r, rad, f(Color32::from_rgb(0x16, 0x26, 0x2C)));
    let glow = Rect::from_min_max(Pos2::new(r.left(), r.center().y), r.max);
    p.rect_filled(glow, CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }, f(Color32::from_rgba_unmultiplied(0x3A, 0x22, 0x48, 120)));
    // the menu bar
    let bar = Rect::from_min_size(r.min, Vec2::new(r.width(), 14.0));
    p.rect_filled(bar, CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, f(Color32::from_white_alpha(28)));
    let notch_w = r.width() * 0.26;
    let notch = Rect::from_center_size(Pos2::new(r.center().x, bar.top() + 6.0), Vec2::new(notch_w, 12.0));
    p.rect_filled(notch, CornerRadius { nw: 0, ne: 0, sw: 5, se: 5 }, f(Color32::BLACK));
    // a few mini widget cards in a grid
    let cards = |area: Rect, bg: Color32, edge: bool| {
        p.rect_filled(area, 8.0, f(bg));
        if edge {
            p.rect_stroke(area, 8.0, Stroke::new(1.0, f(Color32::from_white_alpha(30))), StrokeKind::Inside);
        }
        let pad = 5.0;
        let g = 4.0;
        let cell = (area.width() - 2.0 * pad - 3.0 * g) / 4.0;
        let tint = [Color32::from_rgb(0x6F, 0xD6, 0xE8), Color32::from_rgb(0xB9, 0xA7, 0xFF), Color32::from_rgb(0x6F, 0xD6, 0xE8), Color32::from_rgb(0x8F, 0xD1, 0x4F)];
        let mut y = area.top() + pad;
        for row in 0..2 {
            for c in 0..4 {
                let cr = Rect::from_min_size(Pos2::new(area.left() + pad + c as f32 * (cell + g), y), Vec2::splat(cell));
                if cr.bottom() > area.bottom() - pad + 0.5 {
                    continue;
                }
                p.rect_filled(cr, 3.0, f(Color32::from_white_alpha(18)));
                if row == 0 {
                    p.circle_stroke(cr.center(), cell * 0.28, Stroke::new(1.5, f(tint[c])));
                }
            }
            y += cell + g;
        }
    };
    let w = r.width() * 0.62;
    let h = (w - 10.0) / 4.0 * 2.0 + 14.0;
    if place.notch() {
        let isl = Rect::from_min_size(Pos2::new(r.center().x - w / 2.0, bar.top()), Vec2::new(w, h + 12.0));
        p.rect_filled(isl, CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, f(Color32::BLACK));
        cards(Rect::from_min_max(Pos2::new(isl.left(), isl.top() + 12.0), isl.max), Color32::BLACK, false);
    }
    if place.menubar() {
        // the icon on the right of the menu bar, lit, and the panel under it
        let icon = Rect::from_center_size(Pos2::new(r.right() - 22.0, bar.center().y), Vec2::new(10.0, 8.0));
        p.rect_filled(icon.expand(2.0), 3.0, f(Color32::from_white_alpha(60)));
        p.rect_filled(icon, 2.0, f(Color32::WHITE));
        let pr = Rect::from_min_size(Pos2::new(r.right() - 8.0 - w, bar.bottom() + 4.0), Vec2::new(w, h));
        cards(pr, Color32::from_rgb(0x13, 0x20, 0x0F), true);
    }
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn pop(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = 1.7;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}
