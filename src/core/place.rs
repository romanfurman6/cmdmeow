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

use super::panel;
use super::shell::App;
use crate::kit::glass;
use crate::kit::mechanics as m;
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
    let ch = (cw * 0.9).min(body.height() - title_h - sp::L);
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
        mini(app, ui, r, place, a, k);
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
    if place.menubar() {
        // the icon on the right of the menu bar, lit; the panel opens under it
        let icon = Rect::from_center_size(Pos2::new(r.right() - 22.0, bar.center().y), Vec2::new(10.0, 8.0));
        p.rect_filled(icon.expand(2.0), 3.0, f(Color32::from_white_alpha(60)));
        p.rect_filled(icon, 2.0, f(Color32::WHITE));
    }
}

/// How small the real panel is drawn on a card.
const MINI: f32 = 0.6;

/// The user's own panel on a card, at 60 %: in the menu bar card the themed
/// panel with its header under the icon, in the notch card the same widgets
/// in the black island from the notch. Drawn for real (its own layer,
/// scaled), but nothing on it takes a click.
fn mini(app: &mut App, ui: &mut Ui, r: Rect, place: Place, a: f32, k: usize) {
    // before Goals nothing is on yet: show a few everyday widgets for the
    // picture, then put the user's (empty) set back
    let saved = panel::place(app, panel::inner_width()).0.is_empty().then(|| {
        let keep = (app.widgets.clone(), std::mem::take(&mut app.tools_off));
        for e in app.widgets.iter_mut() {
            e.on = SAMPLE.contains(&e.key.as_str());
        }
        keep
    });
    mini_panel(app, ui, r, place, a, k);
    if let Some((widgets, off)) = saved {
        app.widgets = widgets;
        app.tools_off = off;
    }
}

/// The widgets a card shows while the user has none on yet.
const SAMPLE: &[&str] = &["system/overview", "cleanup/clean", "capture/capture", "keep_awake/card", "claude/limits"];

fn mini_panel(app: &mut App, ui: &mut Ui, r: Rect, place: Place, a: f32, k: usize) {
    let ctx = ui.ctx().clone();
    let pw = m::PANEL_WIDTH;
    let content = panel::content_height(app);
    let (head, foot) = if place.notch() { (32.0, panel::PAD) } else { (panel::HEADER, panel::PAD) };
    let full_h = head + content + foot;
    // where it sits on the card, in card points
    let size = Vec2::new(pw, full_h) * MINI;
    let at = if place.notch() { Pos2::new(r.center().x - size.x / 2.0, r.top()) } else { Pos2::new(r.right() - 6.0 - size.x, r.top() + 20.0) };
    // the card shows it down to just above the label
    let clip = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.bottom() - 44.0)).shrink(1.5);
    let layer = egui::LayerId::new(egui::Order::Foreground, ui.id().with(("place-mini", k)));
    let to = egui::emath::TSTransform::new(at.to_vec2(), MINI);
    ctx.set_transform_layer(layer, to);
    let local = Rect::from_min_size(Pos2::ZERO, Vec2::new(pw, full_h));
    let mut child = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(local).disabled());
    child.set_clip_rect(to.inverse().mul_rect(clip));
    child.set_opacity(a);
    let was = glass::theme();
    if place.notch() {
        if was == glass::Theme::Light {
            glass::set_theme(glass::Theme::Dark);
        }
        child.painter().rect_filled(local, egui::CornerRadius { nw: 0, ne: 0, sw: 26, se: 26 }, Color32::BLACK);
    } else {
        glass::backdrop_panel(child.painter(), local, 16.0, true);
        child.painter().rect_stroke(local, 16.0, Stroke::new(1.0, glass::backdrop_edge()), StrokeKind::Inside);
        panel::header_at(app, &mut child, local.left() + 16.0, f32::INFINITY, local.right() - 14.0, local.top() + 31.0);
    }
    let grid = Rect::from_min_size(Pos2::new(panel::PAD, head), Vec2::new(panel::inner_width(), content));
    let (placed, _) = panel::place(app, grid.width());
    let mut cx = super::tool::Cx::new();
    for w in &placed {
        panel::draw_widget(app, &mut child, w, w.rect.translate(grid.min.to_vec2()), &mut cx);
    }
    glass::set_theme(was);
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
