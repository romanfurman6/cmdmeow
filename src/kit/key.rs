//! The raised-key press every pressable control shares (docs/ui-rules.md,
//! Buttons): the face sits on a darker base and sinks into it while held,
//! then springs back with a small overshoot. Hover only changes colour,
//! a keyboard focus ring sits 2 pt outside the shape, and the click sound
//! plays on mouse down. Disabled controls keep their base (the shape
//! doesn't jump) but don't hover, press or sound.
//!
//! Kit helpers use it as: [`press`] for the interaction, then [`paint`]
//! with a [`Look`]; the face rect it returns is where content goes, so
//! labels and glyphs move with the face.

use egui::{epaint::Mesh, Color32, Id, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

use super::glass::{lerp_color, pick, white, with_alpha, ACTION, DANGER, R_BUTTON};
use super::sound::{self, Click};

/// Base under a lime or red face, and how far that face sinks.
pub const KEY_BASE: f32 = 3.0;
pub const KEY_DROP: f32 = 2.0;
/// Base and sink of quiet kinds (secondary, side, icon, round, tile).
pub const KEY_BASE_QUIET: f32 = 2.0;
pub const KEY_DROP_QUIET: f32 = 1.0;
/// Press down (ease-out) and release (spring) in seconds.
pub const PRESS_IN: f32 = 0.05;
pub const PRESS_OUT: f32 = 0.16;

/// Opacity of a disabled control (one value for every kind).
pub fn disabled_alpha() -> f32 {
    if super::glass::is_light() { 0.62 } else { 0.45 }
}

/// A pressable control this frame.
pub struct Key {
    pub resp: Response,
    /// 0..1, eased.
    pub hover: f32,
    /// How far the face is in: 0 at rest, 1 fully down, a little under 0
    /// while it springs back.
    pub press: f32,
    pub enabled: bool,
    /// Draw the keyboard focus ring.
    pub focus: bool,
}

impl Key {
    /// The face offset for a control that sinks `drop` points.
    pub fn offset(&self, drop: f32) -> Vec2 {
        Vec2::new(0.0, drop * self.press)
    }
}

/// Interact with a key-like control at `rect`: hover, press animation,
/// click sound (`sound`, None = silent), pointer cursor, focus.
pub fn press(ui: &mut Ui, rect: Rect, id: Id, enabled: bool, sound: Option<Click>) -> Key {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let ctx = ui.ctx().clone();
    let hover = ctx.animate_bool(id.with("key-h"), resp.hovered() && enabled);
    let down = enabled && resp.is_pointer_button_down_on();
    let st = ctx.data(|d| d.get_temp::<Anim>(id.with("key-p"))).unwrap_or_default();
    let now = ctx.input(|i| i.time);
    let mut next = st;
    if down != st.down {
        next = Anim { down, t0: now, from: st.value(now) };
        if down {
            if let Some(s) = sound {
                sound::click(s);
            }
        }
    } else if enabled && !down && resp.clicked() && st.value(now) == 0.0 {
        // a click from the keyboard (Space): a full tap, sound and all
        next = Anim { down: false, t0: now, from: 1.0 };
        if let Some(s) = sound {
            sound::click(s);
        }
    }
    if next != st {
        ctx.data_mut(|d| d.insert_temp(id.with("key-p"), next));
    }
    let press = next.value(now);
    if now - next.t0 < f64::from(if next.down { PRESS_IN } else { PRESS_OUT }) {
        ctx.request_repaint();
    }
    if enabled && resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let focus = enabled && resp.has_focus() && keyboard_focus(&ctx);
    Key { resp, hover, press, enabled, focus }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Anim {
    down: bool,
    t0: f64,
    from: f32,
}

impl Anim {
    fn value(&self, now: f64) -> f32 {
        let t = (now - self.t0) as f32;
        if self.down {
            let k = (t / PRESS_IN).clamp(0.0, 1.0);
            self.from + (1.0 - self.from) * (1.0 - (1.0 - k) * (1.0 - k))
        } else if self.from == 0.0 {
            0.0
        } else {
            let k = (t / PRESS_OUT).clamp(0.0, 1.0);
            if k >= 1.0 { 0.0 } else { self.from * (1.0 - spring(k)) }
        }
    }
}

/// `cubic-bezier(.34, 1.8, .64, 1)`: rises past 1 then settles (the overshoot).
pub fn spring(x: f32) -> f32 {
    cubic_bezier(0.34, 1.8, 0.64, 1.0, x)
}

/// `cubic-bezier(.34, 1.56, .64, 1)`: a softer overshoot (the switch knob).
pub fn ease_back(x: f32) -> f32 {
    cubic_bezier(0.34, 1.56, 0.64, 1.0, x)
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let bez = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    // solve x(t) = x by bisection (x(t) is monotonic for x1, x2 in 0..1)
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < x { lo = mid } else { hi = mid }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

/// Whether focus came from the keyboard: Tab was pressed after the last
/// mouse press. Clicking a button gives it focus too, but shows no ring.
fn keyboard_focus(ctx: &egui::Context) -> bool {
    let id = Id::NULL.with("key-keyboard-focus");
    let (tab, mouse) = ctx.input(|i| (i.key_pressed(egui::Key::Tab), i.pointer.any_pressed()));
    let mut kb = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if tab || mouse {
        kb = tab && !mouse;
        ctx.data_mut(|d| d.insert_temp(id, kb));
    }
    kb
}

// ------------------------------------------------------------------- looks

/// How a key is drawn. Build one with [`Look::lime`], [`Look::danger`],
/// [`Look::tint`] or [`Look::quiet`], then adjust fields.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub fill: Color32,
    pub fill_hover: Color32,
    /// The base under the face and how deep it is.
    pub base: Color32,
    pub depth: f32,
    /// How far the face sinks while held.
    pub drop: f32,
    /// 1 pt line inside the top edge.
    pub highlight: Color32,
    /// Hairline (rest, hover) and its width (1 pt; 1.5 on check and radio).
    pub edge: Option<(Color32, Color32)>,
    pub edge_w: f32,
    /// Soft glow of its own colour under the face.
    pub glow: Option<Color32>,
    /// Focus ring colour.
    pub ring: Color32,
    pub radius: f32,
}

impl Look {
    /// A solid coloured key (lime, red, a tool tint): base mixed toward
    /// black, highlight, glow. `hover_white` = how far hover mixes toward white.
    pub fn tint(c: Color32, base_black: f32, hover_white: f32) -> Look {
        Look {
            fill: c,
            fill_hover: lerp_color(c, Color32::WHITE, hover_white),
            base: lerp_color(c, Color32::BLACK, base_black),
            depth: KEY_BASE,
            drop: KEY_DROP,
            highlight: Color32::from_white_alpha(97),
            edge: None,
            edge_w: 1.0,
            glow: Some(with_alpha(c, 0.70)),
            ring: c,
            radius: R_BUTTON,
        }
    }

    /// Primary and hero: lime.
    pub fn lime() -> Look {
        Look::tint(ACTION(), 0.38, 0.26)
    }

    /// Destructive confirm: [`DANGER`] (the state red stays `RED()`).
    pub fn danger() -> Look {
        let mut l = Look::tint(DANGER(), 0.40, 0.22);
        l.glow = Some(with_alpha(DANGER(), 0.65));
        l
    }

    /// A quiet key: ink at `fill` (0..1), `fill_hover` on hover, hairline,
    /// a 2 pt base at ink 14%.
    pub fn quiet(fill: f32, fill_hover: f32) -> Look {
        Look {
            fill: ink(fill),
            fill_hover: ink(fill_hover),
            base: ink(0.14),
            depth: KEY_BASE_QUIET,
            drop: KEY_DROP_QUIET,
            highlight: pick(Color32::from_white_alpha(20), Color32::from_white_alpha(191)),
            edge: Some((ink(0.10), ink(0.18))),
            edge_w: 1.0,
            glow: None,
            ring: ink(1.0),
            radius: R_BUTTON,
        }
    }

    /// A small lime key with a 1 pt base (checked box, picked radio, a tool that is on).
    pub fn lime_small(depth: f32) -> Look {
        let mut l = Look::lime();
        l.depth = depth;
        l.drop = 1.0;
        l.glow = None;
        l
    }

    pub fn radius(mut self, r: f32) -> Look {
        self.radius = r;
        self
    }
}

/// Ink (white in dark mode) at `a` 0..1.
pub fn ink(a: f32) -> Color32 {
    white((a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Draw `key` at rest rect `rect` in `look`; returns the face rect (where
/// the content goes, it moves with the press).
pub fn paint(p: &Painter, rect: Rect, look: &Look, key: &Key) -> Rect {
    let a = if key.enabled { 1.0 } else { disabled_alpha() };
    let press = if key.enabled { key.press } else { 0.0 };
    // W12: inside a panel widget only the whole card hovers
    let hover = if key.enabled && !super::glass::in_widget() { key.hover } else { 0.0 };
    paint_at(p, rect, look, hover, press, a, key.focus)
}

/// [`paint`] from raw values: `hover` 0..1, `press` 0..1, `alpha` 0..1.
/// For a key that isn't interactive (a picture of one) pass zeros.
pub fn paint_at(p: &Painter, rect: Rect, look: &Look, hover: f32, press: f32, alpha: f32, focus: bool) -> Rect {
    let r = look.radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    let face = rect.translate(Vec2::new(0.0, look.drop * press));
    let fade = |c: Color32| with_alpha(c, c.a() as f32 / 255.0 * alpha);
    if let Some(g) = look.glow.filter(|_| alpha >= 1.0) {
        // 0 6 18 -5 at rest, 0 3 10 -5 held
        let k = press.clamp(0.0, 1.0);
        let glow = egui::epaint::Shadow {
            offset: [0, (6.0 - 3.0 * k).round() as i8],
            blur: (18.0 - 8.0 * k).round() as u8,
            spread: 0,
            color: with_alpha(g, g.a() as f32 / 255.0 * (1.0 - 0.2 * k)),
        };
        p.add(glow.as_shape(rect.shrink(5.0), r));
    }
    // the base shows below the face: what's left of it after the face sank
    base(p, face, r, look.depth - look.drop * press.max(0.0), fade(look.base));
    let fill = lerp_color(look.fill, look.fill_hover, hover);
    p.rect_filled(face, r, fade(fill));
    if look.highlight.a() > 0 {
        let top = Rect::from_min_max(face.min, Pos2::new(face.right(), face.top() + 1.0));
        p.with_clip_rect(top.intersect(p.clip_rect())).rect_filled(face, r, fade(look.highlight));
    }
    if let Some((rest, hov)) = look.edge {
        p.rect_stroke(face, r, Stroke::new(look.edge_w, fade(lerp_color(rest, hov, hover))), StrokeKind::Inside);
    }
    if focus {
        p.rect_stroke(face.expand(2.5), r + 2.5, Stroke::new(1.0, look.ring), StrokeKind::Middle);
    }
    face
}

/// The visible part of a key's base: a band `depth` tall that follows the
/// bottom edge of the face (rounded corners included).
fn base(p: &Painter, face: Rect, r: f32, depth: f32, color: Color32) {
    if depth <= 0.05 || color.a() == 0 {
        return;
    }
    let mut edge = Vec::with_capacity(20);
    let n = 8;
    let cy = face.bottom() - r;
    for i in 0..=n {
        // bottom-left arc, from the left side down to the bottom
        let t = std::f32::consts::PI - std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        edge.push(Pos2::new(face.left() + r + r * t.cos(), cy + r * t.sin()));
    }
    for i in 0..=n {
        let t = std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        edge.push(Pos2::new(face.right() - r + r * t.cos(), cy + r * t.sin()));
    }
    let mut mesh = Mesh::default();
    for (i, e) in edge.iter().enumerate() {
        mesh.colored_vertex(*e, color);
        mesh.colored_vertex(*e + Vec2::new(0.0, depth), color);
        if i > 0 {
            let k = (i * 2) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_overshoots_then_settles() {
        assert!(spring(0.0).abs() < 1e-3);
        assert!((spring(1.0) - 1.0).abs() < 1e-3);
        let peak = (1..100).map(|i| spring(i as f32 / 100.0)).fold(0.0f32, f32::max);
        assert!(peak > 1.05 && peak < 1.3, "peak {peak}");
    }

    #[test]
    fn press_goes_down_fast_and_springs_back() {
        let down = Anim { down: true, t0: 0.0, from: 0.0 };
        assert_eq!(down.value(0.0), 0.0);
        assert!((down.value(PRESS_IN as f64) - 1.0).abs() < 1e-4);
        let up = Anim { down: false, t0: 1.0, from: 1.0 };
        assert!((up.value(1.0) - 1.0).abs() < 1e-4);
        // it passes rest (a small rise above) before settling
        let min = (1..40).map(|i| up.value(1.0 + i as f64 * 0.004)).fold(1.0f32, f32::min);
        assert!(min < 0.0 && min > -0.3, "min {min}");
        assert_eq!(up.value(1.0 + PRESS_OUT as f64), 0.0);
        assert_eq!(Anim::default().value(5.0), 0.0);
    }
}
