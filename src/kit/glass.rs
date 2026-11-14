//! Drawing kit for the D6 look: a native dark Mac window, cards sunk a
//! little darker than the window, system blue for actions, monochrome tool
//! icons, one spacing scale ([`sp`]) and one type ramp ([`ty`]).

use egui::{
    epaint::{PathShape, PathStroke},
    Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2,
};
use std::f32::consts::{PI, TAU};

// ---------------------------------------------------------------- palette
//
// "Moss & Lime": a deep moss window with a lime action colour in dark mode,
// a pale sage window with darker lime in light mode. The app follows the
// system appearance ([`set_light`] is called once per frame by the shell).
// Every colour is a function so it can answer for the current mode.

static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True while the app is drawn in light mode.
pub fn is_light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switch the palette. Returns true when the mode changed.
pub fn set_light(light: bool) -> bool {
    LIGHT.swap(light, std::sync::atomic::Ordering::Relaxed) != light
}

/// `dark` in dark mode, `light` in light mode.
pub fn pick(dark: Color32, light: Color32) -> Color32 {
    if is_light() { light } else { dark }
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Ink: white in dark mode, near-black in light mode.
const INK_LIGHT: (u8, u8, u8) = (0x14, 0x18, 0x1A);

fn ink(a: f32) -> Color32 {
    let (r, g, b) = if is_light() { INK_LIGHT } else { (255, 255, 255) };
    Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0) as u8)
}

/// Solid window colour (the middle of the backdrop gradient).
#[allow(non_snake_case)]
pub fn BG() -> Color32 { pick(hex(0x0E1B0D), hex(0xE6F0DA)) }
/// Card surface.
#[allow(non_snake_case)]
pub fn CARD() -> Color32 { pick(Color32::from_rgba_unmultiplied(255, 255, 255, 14), Color32::from_rgba_unmultiplied(255, 255, 255, 191)) }
/// Card edge.
#[allow(non_snake_case)]
pub fn CARD_EDGE() -> Color32 { pick(Color32::from_rgba_unmultiplied(255, 255, 255, 20), Color32::from_rgba_unmultiplied(20, 24, 26, 20)) }
/// Hovered card, quick-action tiles and selected rows.
#[allow(non_snake_case)]
pub fn RAISED() -> Color32 { pick(Color32::from_rgba_unmultiplied(255, 255, 255, 28), Color32::from_rgba_unmultiplied(255, 255, 255, 242)) }
/// Floating surfaces: menus, toasts, drop targets, previews.
#[allow(non_snake_case)]
pub fn POPOVER() -> Color32 { pick(hex(0x1A2818), hex(0xFFFFFF)) }
/// Window sidebar.
#[allow(non_snake_case)]
pub fn SIDEBAR() -> Color32 { pick(Color32::from_rgba_unmultiplied(3, 8, 3, 128), Color32::from_rgba_unmultiplied(255, 255, 255, 128)) }
/// Window footer strip.
#[allow(non_snake_case)]
pub fn FOOTER() -> Color32 { pick(Color32::from_rgba_unmultiplied(3, 8, 3, 90), Color32::from_rgba_unmultiplied(255, 255, 255, 110)) }

#[allow(non_snake_case)]
pub fn TEXT() -> Color32 { ink(1.0) }
/// Secondary text.
#[allow(non_snake_case)]
pub fn TEXT_SOFT() -> Color32 { ink(0.84) }
#[allow(non_snake_case)]
pub fn TEXT_DIM() -> Color32 { ink(0.68) }
#[allow(non_snake_case)]
pub fn TEXT_FAINT() -> Color32 { ink(0.56) }
/// Chevrons, drag handles.
#[allow(non_snake_case)]
pub fn TEXT_MUTED() -> Color32 { ink(0.40) }

/// The action colour (lime): primary buttons, selection, checkboxes,
/// switches, the active sidebar icon. Text on it is [`ON_ACTION`].
#[allow(non_snake_case)]
pub fn BLUE() -> Color32 { pick(hex(0xC8F05A), hex(0xA8D83A)) }
/// Text and glyphs drawn on [`BLUE`].
pub const ON_ACTION: Color32 = hex(0x0E1405);
/// Links and lime text on cards.
#[allow(non_snake_case)]
pub fn ACCENT() -> Color32 { pick(hex(0xD9F78A), hex(0x3F6B0A)) }
#[allow(non_snake_case)]
pub fn GREEN() -> Color32 { pick(hex(0x94D45A), hex(0x4B8625)) }
/// Apricot: attention (busy spikes, "suggested", warnings).
#[allow(non_snake_case)]
pub fn AMBER() -> Color32 { pick(hex(0xF7B267), hex(0xC06A12)) }
/// Problems and low battery.
#[allow(non_snake_case)]
pub fn RED() -> Color32 { pick(hex(0xFF9B7A), hex(0xC2391D)) }
/// Claude's limit bars and rings: the Monitor cyan.
#[allow(non_snake_case)]
pub fn CLAUDE() -> Color32 { tint::HEALTH() }
/// Second series in charts: a paler step of the Monitor cyan.
#[allow(non_snake_case)]
pub fn INDIGO() -> Color32 { pick(hex(0xB4EAF3), hex(0x5AB4C6)) }
#[allow(non_snake_case)]
pub fn TEAL() -> Color32 { tint::HEALTH() }
/// Third series / "other": a neutral step, not a hue.
#[allow(non_snake_case)]
pub fn PURPLE() -> Color32 { pick(hex(0x4A5A47), hex(0xA9B5A0)) }
#[allow(non_snake_case)]
pub fn GRAY() -> Color32 { pick(hex(0x98A096), hex(0x6E766B)) }

/// Junk ring segments, largest to smallest: lime stepping into green.
pub fn junk_ramp(i: usize) -> Color32 {
    const DARK: [u32; 7] = [0xC8F05A, 0xAEE35A, 0x94D45A, 0x7CC25A, 0x66AE57, 0x549852, 0x46824B];
    const LIGHT: [u32; 7] = [0x7DB519, 0x6AA61E, 0x5A9622, 0x4B8625, 0x3E7626, 0x336626, 0x2A5724];
    let i = i.min(6);
    pick(hex(DARK[i]), hex(LIGHT[i]))
}

/// Tool tints by group: Clean (lime), Monitor (cyan), Create (apricot),
/// Utilities (lavender).
pub mod tint {
    use super::{hex, pick};
    use egui::Color32;
    #[allow(non_snake_case)]
    pub fn CLEANUP() -> Color32 { pick(hex(0xC8F05A), hex(0x7DB519)) }
    #[allow(non_snake_case)]
    pub fn DISK_MAP() -> Color32 { CLEANUP() }
    #[allow(non_snake_case)]
    pub fn STARTUP() -> Color32 { CLEANUP() }
    #[allow(non_snake_case)]
    pub fn HEALTH() -> Color32 { pick(hex(0x6FD6E8), hex(0x1F8AA0)) }
    #[allow(non_snake_case)]
    pub fn CLAUDE() -> Color32 { HEALTH() }
    #[allow(non_snake_case)]
    pub fn FAN() -> Color32 { HEALTH() }
    #[allow(non_snake_case)]
    pub fn DISPLAYS() -> Color32 { HEALTH() }
    #[allow(non_snake_case)]
    pub fn CAPTURE() -> Color32 { pick(hex(0xF7B267), hex(0xC06A12)) }
    #[allow(non_snake_case)]
    pub fn KEEP_AWAKE() -> Color32 { pick(hex(0xE8E1FF), hex(0x6A5AC8)) }
    #[allow(non_snake_case)]
    pub fn NEUTRAL() -> Color32 { super::TEXT_FAINT() }
}

/// The spacing scale. Only these six values; nothing in between.
pub mod sp {
    /// Label ↔ its caption.
    pub const XS: f32 = 4.0;
    /// Items in a group.
    pub const S: f32 = 8.0;
    /// Title block ↔ content.
    pub const M: f32 = 12.0;
    /// Between blocks, card padding, gap between cards.
    pub const L: f32 = 16.0;
    /// Between sections.
    pub const XL: f32 = 24.0;
    /// Around empty states, between page sections.
    pub const XXL: f32 = 32.0;
    /// A row in a list (icon, name, detail).
    pub const ROW_H: f32 = 44.0;
    /// A settings row: label left, control right, hairline between.
    pub const SETTINGS_ROW_H: f32 = 40.0;
    /// A row with a bar under its name and value (models, projects, folders).
    pub const BAR_ROW_H: f32 = 36.0;
    /// Buttons, segmented controls and fields inside cards…
    pub const CONTROL_H: f32 = 28.0;
    /// …and in page headers.
    pub const HEADER_CONTROL_H: f32 = 32.0;
    /// Content starts this far from the window edge on every page.
    pub const PAGE_EDGE: f32 = 24.0;
    /// A legend row.
    pub const LEGEND_H: f32 = 22.0;
    /// Between a title and the detail line under it ("Ollama" over
    /// "com.ollama.ollama"); see [`super::two_lines`].
    pub const LINE_GAP: f32 = 6.0;
    /// A list row with two lines of text (name over detail).
    pub const ROW2_H: f32 = 52.0;
    /// Between rows of a list whose rows light up on hover or selection,
    /// so two lit rows never touch.
    pub const ROW_GAP: f32 = XS;
    /// Between columns of a row (name, schedule, state, buttons).
    pub const COL_GAP: f32 = 16.0;
    /// The slim scroll bar, at its widest (hovered).
    pub const SCROLL_BAR_W: f32 = 6.0;
    /// Room a scrolling list leaves on its right for the bar: the bar sits
    /// in the middle of it and never on the content.
    pub const SCROLL_GUTTER: f32 = 14.0;
    /// A list's section title row ("Open at login"): L above, the title
    /// (and its link), M below to the first row.
    pub const SECTION_H: f32 = 44.0;
    /// A table's column-head row inside its card ("Name · Last used · Size"),
    /// with M above and below the heads; the hairline sits at its bottom.
    pub const TABLE_HEAD_H: f32 = 36.0;
}

/// The type ramp: seven styles, used everywhere instead of ad-hoc sizes.
pub mod ty {
    use super::{bold, font, white};
    use egui::{Color32, FontId};
    /// 28/600: one per card at most ("4.2 GB").
    pub fn hero() -> FontId { FontId::new(28.0, egui::FontFamily::Name("hero".into())) }
    /// 22/600: secondary numbers ("3.2 MB/s").
    pub fn number() -> FontId { FontId::new(22.0, egui::FontFamily::Name("hero".into())) }
    /// 15/600: page titles and lead lines ("Clean Up finished").
    pub fn lead() -> FontId { bold(15.0) }
    /// 13/600: card titles.
    pub fn title() -> FontId { bold(13.0) }
    /// 13/400: body text and list rows.
    pub fn body() -> FontId { font(13.0) }
    /// 12/400: labels and legends.
    pub fn label() -> FontId { font(12.0) }
    /// 12/600: section titles of a list ("Open at login").
    pub fn section() -> FontId { bold(12.0) }
    /// 11/400: captions, in [`caption_color`].
    pub fn caption() -> FontId { font(11.0) }
    /// White at 50%: the caption colour.
    pub fn caption_color() -> Color32 { white(128) }
}

/// Status chip colours: (background, text).
#[allow(non_snake_case)]
pub fn CHIP_ASK() -> (Color32, Color32) { (with_alpha(AMBER(), 0.16), AMBER()) }
#[allow(non_snake_case)]
pub fn CHIP_RISK() -> (Color32, Color32) { (with_alpha(RED(), 0.16), RED()) }

/// Radii and spacing on the 8 pt grid.
pub const R_CARD: f32 = 12.0;
pub const R_BUTTON: f32 = 10.0;
pub const R_SMALL: f32 = 7.0;
pub const HAIRLINE: u8 = 15; // white at ~6%
pub const HAIRLINE_STRONG: u8 = 26; // white at ~10%

/// An overlay at alpha `a`: white in dark mode, ink in light mode.
pub fn white(a: u8) -> Color32 {
    if is_light() {
        Color32::from_rgba_unmultiplied(INK_LIGHT.0, INK_LIGHT.1, INK_LIGHT.2, a)
    } else {
        Color32::from_white_alpha(a)
    }
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let a = a.to_srgba_unmultiplied();
    let b = b.to_srgba_unmultiplied();
    let m = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(m(0), m(1), m(2), m(3))
}

// ---------------------------------------------------------------- fonts

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

// ---------------------------------------------------------------- shapes

/// Points of a rounded rectangle outline, clockwise from the top-left arc.
pub fn rounded_points(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let mut pts = Vec::with_capacity(4 * 9);
    let corners = [
        (Pos2::new(rect.left() + r, rect.top() + r), PI),
        (Pos2::new(rect.right() - r, rect.top() + r), 1.5 * PI),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 0.5 * PI),
    ];
    let steps = 8;
    for (c, start) in corners {
        for i in 0..=steps {
            let a = start + (i as f32 / steps as f32) * (PI / 2.0);
            pts.push(Pos2::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// The window surface: a radial glow from the top centre (moss in dark
/// mode, sage in light mode) with a 1 pt hairline edge. Live, the system
/// blur behind it shows through a little; `opaque` (screenshots)
/// paints it solid.
pub fn backdrop(painter: &Painter, rect: Rect, radius: f32, opaque: bool, _t: f64) {
    let light = is_light();
    let alpha = if opaque { 1.0 } else { backdrop_alpha() };
    // the mesh only changes with the window's size and the theme: build it
    // once, not every frame (it's ~1.4k vertices of gradient math)
    type Key = ([u32; 5], bool, u32);
    static CACHE: std::sync::Mutex<Vec<(Key, std::sync::Arc<egui::epaint::Mesh>)>> = std::sync::Mutex::new(Vec::new());
    let key: Key = ([rect.min.x.to_bits(), rect.min.y.to_bits(), rect.max.x.to_bits(), rect.max.y.to_bits(), radius.to_bits()], light, alpha.to_bits());
    let mesh = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
            let hit = cache.remove(i);
            let mesh = hit.1.clone();
            cache.push(hit);
            mesh
        } else {
            let mesh = std::sync::Arc::new(backdrop_mesh(rect, radius, light, alpha));
            // a few windows (panel, app window, setup), newest last
            if cache.len() >= 4 {
                cache.remove(0);
            }
            cache.push((key, mesh.clone()));
            mesh
        }
    };
    painter.add(Shape::Mesh(mesh));
    painter.rect_stroke(rect, radius, Stroke::new(1.0, white(HAIRLINE_STRONG)), StrokeKind::Inside);
}

fn backdrop_mesh(rect: Rect, radius: f32, light: bool, alpha: f32) -> egui::epaint::Mesh {
    let stops: &[(f32, Color32)] = if light {
        &[(0.0, hex(0xFFFFFF)), (0.55, hex(0xEAF2DF)), (1.0, hex(0xD9E8C8))]
    } else {
        &[(0.0, hex(0x22401D)), (0.35, hex(0x132610)), (0.7, hex(0x0A150A)), (1.0, hex(0x070D07))]
    };
    let glow = Pos2::new(rect.center().x, rect.top());
    // the glow is an ellipse: wide across, reaching the bottom edge
    let (rx, ry) = (rect.width() * 0.75, rect.height() * 1.05);
    let color_at = |q: Pos2| {
        let d = (((q.x - glow.x) / rx).powi(2) + ((q.y - glow.y) / ry).powi(2)).sqrt().min(1.0);
        let k = stops.iter().position(|(t, _)| *t >= d).unwrap_or(stops.len() - 1).max(1);
        let (t0, c0) = stops[k - 1];
        let (t1, c1) = stops[k];
        with_alpha(lerp_color(c0, c1, (d - t0) / (t1 - t0).max(1e-3)), alpha)
    };
    // a fan of rings from the centre out to the rounded outline
    let outline = densify(&rounded_points(rect, radius), 24.0);
    let c = rect.center();
    let rings: usize = 14;
    let n = outline.len() as u32;
    let mut mesh = egui::epaint::Mesh::default();
    mesh.reserve_vertices(1 + rings * n as usize);
    mesh.reserve_triangles(n as usize * (2 * rings - 1));
    mesh.colored_vertex(c, color_at(c));
    for k in 1..=rings {
        let f = k as f32 / rings as f32;
        for q in &outline {
            let v = c + (*q - c) * f;
            mesh.colored_vertex(v, color_at(v));
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        mesh.add_triangle(0, 1 + i, 1 + j);
    }
    for k in 1..rings as u32 {
        let (a, b) = (1 + (k - 1) * n, 1 + k * n);
        for i in 0..n {
            let j = (i + 1) % n;
            mesh.add_triangle(a + i, b + i, b + j);
            mesh.add_triangle(a + i, b + j, a + j);
        }
    }
    mesh
}

/// How much the backdrop covers what's behind live windows: solid by
/// default (`CMDMEOW_BACKDROP_ALPHA` lets the blur through when tuning).
fn backdrop_alpha() -> f32 {
    static A: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var("CMDMEOW_BACKDROP_ALPHA").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0))
}

/// `pts` (a closed outline) with extra points so no edge is longer than `step`.
fn densify(pts: &[Pos2], step: f32) -> Vec<Pos2> {
    let mut out = Vec::with_capacity(pts.len() * 2);
    for (i, a) in pts.iter().enumerate() {
        let b = pts[(i + 1) % pts.len()];
        let n = ((b - *a).length() / step).ceil().max(1.0) as usize;
        for k in 0..n {
            out.push(*a + (b - *a) * (k as f32 / n as f32));
        }
    }
    out
}

/// The colour of a sidebar section: Clean, Monitor, Create, Utilities.
pub fn group_color(section: &str) -> Color32 {
    match section {
        "Clean" => tint::CLEANUP(),
        "Monitor" => tint::HEALTH(),
        "Create" => tint::CAPTURE(),
        "Utilities" => tint::KEEP_AWAKE(),
        _ => TEXT_FAINT(),
    }
}

/// A flat card: `CARD()` fill with a hairline. `hover` 0..1 lifts it a little,
/// `selected` adds the blue ring.
pub fn glass_card(painter: &Painter, rect: Rect, r: f32, hover: f32, selected: bool) {
    painter.rect_filled(rect, r, lerp_color(CARD(), RAISED(), hover * 0.5));
    painter.rect_stroke(rect, r, Stroke::new(1.0, CARD_EDGE()), StrokeKind::Inside);
    if selected {
        painter.rect_stroke(rect, r, Stroke::new(1.5, BLUE()), StrokeKind::Inside);
    }
}

/// Button kinds for [`pill_button`].
pub const BTN_SECONDARY: u8 = 0;
pub const BTN_PRIMARY: u8 = 1;
pub const BTN_DANGER: u8 = 2;

/// Standard push button (28–34 pt tall, radius 8). `kind`: 0 = secondary
/// (quiet fill + hairline), 1 = primary (blue), 2 = destructive (red).
pub fn pill_button(ui: &mut Ui, rect: Rect, label: &str, kind: u8, enabled: bool) -> Response {
    let id = ui.scope_id().with(("pill", label, rect.min.x as i32, rect.min.y as i32));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, id, sense);
    let hover = ui.ctx().animate_bool(id.with("h"), resp.hovered() && enabled);
    let pressed = resp.is_pointer_button_down_on() && enabled;
    let p = ui.painter();
    let tint = match kind {
        BTN_PRIMARY => Some(BLUE()),
        BTN_DANGER => Some(RED()),
        _ => None,
    };
    let a = if enabled { 1.0 } else { 0.45 };
    liquid_capsule(p, rect, tint, if pressed { 1.6 } else { hover }, a);
    let (f, col) = match tint {
        Some(c) => (bold(13.0), on_color(c)),
        None => (font(13.0), TEXT()),
    };
    let col = if enabled { col } else { with_alpha(col, 0.5) };
    text_fit(p, rect.center(), Align2::CENTER_CENTER, label, f, col, rect.width() - 12.0);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Width a [`pill_button`] needs for `label` (14 pt padding each side).
/// Text and glyph colour on a solid `fill`: near-black on light fills
/// (lime, apricot), white on dark ones.
pub fn on_color(fill: Color32) -> Color32 {
    let [r, g, b, _] = fill.to_srgba_unmultiplied();
    if 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 150.0 { ON_ACTION } else { Color32::WHITE }
}

pub fn button_width(p: &Painter, label: &str, kind: u8) -> f32 {
    let f = if kind == BTN_SECONDARY { font(13.0) } else { bold(13.0) };
    p.layout_no_wrap(label.to_owned(), f, TEXT()).size().x + 28.0
}

/// The body of a standard button: flat tint (or quiet white) with a hairline.
/// `hover` 0..1 lightens it; values above 1 read as pressed.
pub fn liquid_capsule(p: &Painter, rect: Rect, tint: Option<Color32>, hover: f32, alpha: f32) {
    let r = R_BUTTON.min(rect.height() / 2.0);
    match tint {
        Some(c) => {
            let fill = if hover > 1.0 { lerp_color(c, Color32::BLACK, 0.12) } else { lerp_color(c, Color32::WHITE, 0.12 * hover) };
            // a soft glow of its own colour below, a darker lip inside
            let glow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: with_alpha(c, 0.25 * alpha) };
            p.add(glow.as_shape(rect, r));
            p.rect_filled(rect, r, with_alpha(fill, alpha));
            let lip = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 2.0), rect.max);
            p.with_clip_rect(lip).rect_filled(rect, r, Color32::from_black_alpha((46.0 * alpha) as u8));
        }
        None => {
            let a = if hover > 1.0 { 26.0 } else { 10.0 + 10.0 * hover };
            p.rect_filled(rect, r, white((a * alpha) as u8));
            p.rect_stroke(rect, r, Stroke::new(1.0, white((36.0 * alpha) as u8)), StrokeKind::Inside);
        }
    }
}

/// A 30 pt square icon button (panel header, sheet actions).
pub fn icon_button(ui: &mut Ui, rect: Rect, ic: Icon, tip: &str) -> Response {
    let id = ui.scope_id().with(("icon-btn", tip, rect.min.x as i32, rect.min.y as i32));
    let resp = ui.interact(rect, id, Sense::click());
    let h = ui.ctx().animate_bool(id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, R_BUTTON, white((15.0 + 13.0 * h) as u8));
    icon(p, ic, Rect::from_center_size(rect.center(), Vec2::splat(rect.height() * 0.53)), TEXT_SOFT());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

/// The round 26 pt close / refresh button used in sheet headers.
pub fn round_button(ui: &mut Ui, rect: Rect, ic: Icon, tip: &str) -> Response {
    let id = ui.scope_id().with(("round-btn", tip, rect.min.x as i32, rect.min.y as i32));
    let resp = ui.interact(rect, id, Sense::click());
    let h = ui.ctx().animate_bool(id, resp.hovered());
    let p = ui.painter();
    p.circle_filled(rect.center(), rect.height() / 2.0, white((20.0 + 18.0 * h) as u8));
    icon(p, ic, Rect::from_center_size(rect.center(), Vec2::splat(rect.height() * 0.46)), TEXT_SOFT());
    resp.on_hover_text(tip)
}

/// Text link in the accent blue.
pub fn link(ui: &mut Ui, pos: Pos2, align: Align2, label: &str, size: f32) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), bold(size), ACCENT());
    let rect = align.anchor_size(pos, galley.size());
    let id = ui.scope_id().with(("link", label, pos.x as i32, pos.y as i32));
    let resp = ui.interact(rect.expand(4.0), id, Sense::click());
    let col = if resp.hovered() { Color32::from_rgb(0x7D, 0xBB, 0xFF) } else { ACCENT() };
    ui.painter().galley(rect.min, galley, col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Thin flat progress bar on a faint track.
pub fn bar(painter: &Painter, rect: Rect, frac: f32, color: Color32) {
    let r = rect.height() / 2.0;
    painter.rect_filled(rect, r, white(20));
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        let w = (rect.width() * f).max(rect.height());
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(w, rect.height())), r, color);
    }
}

/// A stacked bar: segments are (fraction of the whole, colour), 2 pt apart,
/// the rest is the faint track.
pub fn stacked_bar(painter: &Painter, rect: Rect, segments: &[(f32, Color32)]) {
    let r = rect.height() / 2.0;
    painter.rect_filled(rect, r, white(20));
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left();
    let mut shapes = Vec::new();
    for (f, c) in segments {
        let w = rect.width() * f.clamp(0.0, 1.0);
        if w < 0.5 {
            continue;
        }
        let seg = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new((x + w).min(rect.right()), rect.bottom()));
        shapes.push((seg, *c));
        x += w + 2.0;
    }
    // round only the outer ends: draw the run as one rounded mask, then cut gaps
    for (seg, c) in shapes {
        let cr = CornerRadius {
            nw: if seg.left() <= rect.left() + 0.5 { r as u8 } else { 0 },
            sw: if seg.left() <= rect.left() + 0.5 { r as u8 } else { 0 },
            ne: if seg.right() >= rect.right() - 0.5 { r as u8 } else { 0 },
            se: if seg.right() >= rect.right() - 0.5 { r as u8 } else { 0 },
        };
        clip.rect_filled(seg, cr, c);
    }
}

pub fn arc_points(center: Pos2, radius: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// Frame interval for things that move by themselves. 15 ms lands on an
/// even beat with vsync on common displays: every frame at 60 Hz, every
/// 2nd at 100 Hz (50 fps) and 120 Hz (60 fps), every 3rd at 144 Hz. The
/// old 33 ms didn't divide 100/120 Hz evenly and stuttered; every frame
/// at 100 Hz cost ~20% CPU with the panel open (fan blades redraw it all).
pub const ANIM_FRAME: std::time::Duration = std::time::Duration::from_millis(15);

/// Ask for the next frame of something that moves by itself (spinners, fan
/// blades, steam). Only call it while the thing is on screen.
#[track_caller]
pub fn animate(ctx: &egui::Context) {
    ctx.request_repaint_after(ANIM_FRAME);
}

/// Beat for loops that only decorate (fan blades, the setup cards): 30 fps,
/// on a fixed clock so frames land evenly on 60 and 120 Hz displays. A
/// frame redraws the whole window, so at 60 fps one turning fan kept an
/// open panel at ~24% CPU on an M1.
const AMBIENT_FPS: f64 = 30.0;
/// Decorative loops rest after this long without mouse, keys or focus
/// changes in the app's windows, and carry on with the next move: a panel left
/// open costs next to nothing.
const AMBIENT_REST: f64 = 10.0;

#[derive(Clone, Copy)]
struct Ambient {
    clock: f64,
    seen: f64,
    input: f64,
}

/// Advance the decorative clock (once per pass; one clock for all windows,
/// so a blade drawn in two keeps one angle) and say whether loops run.
fn ambient_state(ctx: &egui::Context) -> (f64, bool) {
    let (now, moved) = ctx.input(|i| (i.time, i.pointer.delta() != Vec2::ZERO || !i.events.is_empty()));
    let id = egui::Id::NULL.with("ambient-clock");
    ctx.data_mut(|d| {
        let a = d.get_temp_mut_or_insert_with(id, || Ambient { clock: 0.0, seen: now, input: now });
        if moved {
            a.input = now;
        }
        let live = now - a.input < ambient_tuning().1;
        if live && now > a.seen {
            // a long gap (window was hidden) doesn't jump the loops ahead
            a.clock += (now - a.seen).min(0.1);
        }
        a.seen = now;
        (a.clock, live)
    })
}

/// Seconds on the decorative clock: it stops while the loops rest,
/// so they hold still instead of jumping when something else redraws.
pub fn ambient_clock(ctx: &egui::Context) -> f64 {
    ambient_state(ctx).0
}

/// Whether decorative loops run now (the app saw input lately).
pub fn ambient_live(ctx: &egui::Context) -> bool {
    ambient_state(ctx).1
}

/// Ask for the next frame of a decorative loop (see [`AMBIENT_FPS`]); only
/// while it's on screen and the app saw input lately.
#[track_caller]
pub fn ambient(ctx: &egui::Context) {
    if ambient_state(ctx).1 {
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(next_beat(now, ambient_tuning().0)));
    }
}

/// Time to just past the next whole second of the wall clock. Live numbers
/// and clocks that change by the second all ask for this one frame, so an
/// open window that nothing else moves draws about once a second.
pub fn next_second() -> std::time::Duration {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0);
    std::time::Duration::from_millis(1000 - ms as u64 + 5)
}

/// (fps, rest seconds) for decorative loops. `CMDMEOW_AMBIENT_FPS` and
/// `CMDMEOW_AMBIENT_REST` override them, to measure what a frame costs.
fn ambient_tuning() -> (f64, f64) {
    static T: std::sync::OnceLock<(f64, f64)> = std::sync::OnceLock::new();
    *T.get_or_init(|| {
        let var = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(d);
        (var("CMDMEOW_AMBIENT_FPS", AMBIENT_FPS), var("CMDMEOW_AMBIENT_REST", AMBIENT_REST))
    })
}

/// Seconds from `now` to the next tick of a `fps` clock (never under 4 ms).
fn next_beat(now: f64, fps: f64) -> f64 {
    let period = 1.0 / fps;
    let left = period - now.rem_euclid(period);
    if left < 0.004 { left + period } else { left }
}

/// A spinning arc used while something is working. It turns by itself: a
/// Core Animation layer where it can (no frames of ours, smooth while the
/// app is busy), else egui at the animation pace. `_t` is unused (callers
/// used to pass the clock).
#[track_caller]
pub fn spinner(painter: &Painter, center: Pos2, radius: f32, _t: f64, color: Color32) {
    crate::kit::spin::arc_at(painter, center, radius, color, std::panic::Location::caller());
}

pub fn checkbox(ui: &mut Ui, rect: Rect, id: egui::Id, on: bool, enabled: bool) -> Response {
    let resp = ui.interact(rect.expand(3.0), id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let r = 4.0;
    if on && enabled {
        p.rect_filled(rect, r, BLUE());
        let a = rect.left_top() + Vec2::new(rect.width() * 0.24, rect.height() * 0.52);
        let b = rect.left_top() + Vec2::new(rect.width() * 0.43, rect.height() * 0.71);
        let c = rect.left_top() + Vec2::new(rect.width() * 0.78, rect.height() * 0.31);
        p.add(PathShape::line(vec![a, b, c], PathStroke::new(2.0, ON_ACTION)));
    } else {
        p.rect_stroke(rect, r, Stroke::new(1.5, white(if enabled { 90 } else { 30 })), StrokeKind::Inside);
    }
    resp
}

/// One choice of several (a filled dot when picked), the size of a checkbox.
pub fn radio(ui: &mut Ui, rect: Rect, id: egui::Id, on: bool, enabled: bool) -> Response {
    let resp = ui.interact(rect.expand(3.0), id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    if on && enabled {
        p.circle_filled(c, r, BLUE());
        p.circle_filled(c, r * 0.4, ON_ACTION);
    } else {
        stroke_circle(p, c, r - 0.75, Stroke::new(1.5, white(if enabled { 90 } else { 30 })));
    }
    resp
}

/// A small rounded status chip ("Safe", "Ask", "Kept"). `colors` = (fill, text).
/// Returns the chip's rect.
pub fn chip(p: &Painter, pos: Pos2, align: Align2, label: &str, colors: (Color32, Color32)) -> Rect {
    let g = p.layout_no_wrap(label.to_owned(), bold(11.0), colors.1);
    let r = align.anchor_size(pos, g.size() + Vec2::new(16.0, 5.0));
    p.rect_filled(r, 6.0, colors.0);
    p.galley(r.center() - g.size() / 2.0, g, colors.1);
    r
}

/// An amber count badge (e.g. "2" next to "Needs your OK").
pub fn badge(p: &Painter, pos: Pos2, align: Align2, label: &str) -> Rect {
    let g = p.layout_no_wrap(label.to_owned(), bold(11.0), BG());
    let r = align.anchor_size(pos, Vec2::new((g.size().x + 14.0).max(20.0), 18.0));
    p.rect_filled(r, 9.0, AMBER());
    p.galley(r.center() - g.size() / 2.0, g, BG());
    r
}

/// Two-or-more option segmented control. Returns the index clicked this frame.
pub fn segmented(ui: &mut Ui, rect: Rect, id: egui::Id, labels: &[&str], selected: usize) -> Option<usize> {
    let p = ui.painter().clone();
    p.rect_filled(rect, R_SMALL, white(15));
    let inner = rect.shrink(2.0);
    let w = inner.width() / labels.len().max(1) as f32;
    let mut out = None;
    for (i, l) in labels.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(inner.left() + w * i as f32, inner.top()), Vec2::new(w, inner.height()));
        let resp = ui.interact(r, id.with(i), Sense::click());
        if i == selected {
            p.rect_filled(r, 6.0, pick(Color32::from_white_alpha(36), Color32::WHITE));
        }
        let col = if i == selected || resp.hovered() { TEXT() } else { TEXT_DIM() };
        text_fit(&p, r.center(), Align2::CENTER_CENTER, l, font(12.0), col, r.width() - 8.0);
        if resp.clicked() && i != selected {
            out = Some(i);
        }
    }
    out
}

/// An on/off switch (30×18).
pub fn switch(ui: &mut Ui, rect: Rect, id: egui::Id, on: bool, enabled: bool) -> Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if !enabled {
        dashed_rect(p, rect, rect.height() / 2.0, white(64));
        return resp;
    }
    let t = ui.ctx().animate_bool(id.with("t"), on);
    p.rect_filled(rect, rect.height() / 2.0, lerp_color(white(41), BLUE(), t));
    let d = rect.height() - 4.0;
    let x = rect.left() + 2.0 + d / 2.0 + (rect.width() - 4.0 - d) * t;
    p.circle_filled(Pos2::new(x, rect.center().y), d / 2.0, Color32::WHITE);
    resp
}

/// A dashed rounded outline: the "planned, not built yet" look.
pub fn dashed_rect(p: &Painter, rect: Rect, r: f32, color: Color32) {
    let pts = rounded_points(rect.shrink(0.5), r);
    let mut path = pts.clone();
    path.push(pts[0]);
    p.extend(Shape::dashed_line(&path, Stroke::new(1.0, color), 3.0, 2.5));
}

/// One tool's chip: a quiet rounded square with the glyph in `color`
/// (system blue for every tool; no rainbow of filled squircles).
pub fn tool_chip(p: &Painter, rect: Rect, glyph: Icon, color: Color32) {
    let r = (rect.width() * 0.27).round();
    if color == tint::NEUTRAL() || color == BLUE() {
        p.rect_filled(rect, r, white(18));
    } else {
        // a group tile: its colour faint behind, a little stronger as the edge
        p.rect_filled(rect, r, with_alpha(color, 0.12));
        p.rect_stroke(rect, r, Stroke::new(1.0, with_alpha(color, 0.28)), StrokeKind::Inside);
    }
    icon(p, glyph, Rect::from_center_size(rect.center(), rect.size() * 0.6), lift(color));
}

/// A tint made readable as a glyph or text on a dark card (blue → lighter blue).
pub fn lift(c: Color32) -> Color32 {
    if c == BLUE() { ACCENT() } else { c }
}

/// 1 pt separator line.
pub fn hairline(p: &Painter, a: Pos2, b: Pos2) {
    p.line_segment([a, b], Stroke::new(1.0, white(HAIRLINE)));
}

/// Small section header ("Clean", "Monitor", table column titles).
/// A sidebar section title with its group's 6 pt dot in front.
pub fn group_header(p: &Painter, pos: Pos2, label: &str) -> Rect {
    p.circle_filled(Pos2::new(pos.x + 3.0, pos.y + 7.0), 3.0, group_color(label));
    section_header(p, Pos2::new(pos.x + 12.0, pos.y), Align2::LEFT_TOP, label)
}

pub fn section_header(p: &Painter, pos: Pos2, align: Align2, label: &str) -> Rect {
    p.text(pos, align, label, bold(11.0), white(120))
}

/// The title of a section of a list ("Open at login", "Background agents"),
/// in a row [`sp::SECTION_H`] tall starting at `top`. Returns the title's
/// centre y, for a link on the same line.
pub fn section_title(p: &Painter, x: f32, top: f32, label: &str) -> f32 {
    let cy = section_title_y(p, top);
    p.text(Pos2::new(x, cy), Align2::LEFT_CENTER, label, ty::section(), white(150));
    cy
}

/// The centre y of a section title in a row starting at `top`.
pub fn section_title_y(p: &Painter, top: f32) -> f32 {
    top + sp::SECTION_H - sp::M - line_h(p, &ty::section()) / 2.0
}

/// The header of a detail sheet: tint chip, title, subtitle, close button.
/// Returns true when the close button was clicked.
pub fn sheet_header(ui: &mut Ui, rect: Rect, glyph: Icon, color: Color32, title: &str, sub: &str) -> bool {
    let p = ui.painter().clone();
    tool_chip(&p, Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::splat(30.0)), glyph, color);
    let cy = rect.top() + 29.0;
    if sub.is_empty() {
        text_fit(&p, Pos2::new(rect.left() + 56.0, cy), Align2::LEFT_CENTER, title, bold(15.0), TEXT(), rect.width() - 110.0);
    } else {
        two_lines(&p, rect.left() + 56.0, cy, title, bold(15.0), TEXT(), sub, font(12.0), TEXT_DIM(), rect.width() - 110.0);
    }
    hairline(&p, Pos2::new(rect.left(), rect.top() + 57.0), Pos2::new(rect.right(), rect.top() + 57.0));
    let cr = Rect::from_center_size(Pos2::new(rect.right() - 27.0, rect.top() + 29.0), Vec2::splat(26.0));
    round_button(ui, cr, Icon::Close, "Close").clicked()
}

// ---------------------------------------------------------------- panel widgets


/// A sheet's big number: "1.44 TB free of 2 TB". Returns the height used.
pub fn hero_number(p: &Painter, pos: Pos2, big: &str, unit: &str, caption: &str) -> f32 {
    let g = p.layout_no_wrap(big.to_owned(), bold(34.0), TEXT());
    let (w, h) = (g.size().x, g.size().y);
    let base = pos.y + h * 0.80;
    p.galley(pos, g, TEXT());
    let u = p.text(Pos2::new(pos.x + w + 6.0, base), Align2::LEFT_BOTTOM, unit, bold(15.0), TEXT_SOFT());
    p.text(Pos2::new(u.right() + if unit.is_empty() { 0.0 } else { 8.0 }, base), Align2::LEFT_BOTTOM, caption, font(13.0), TEXT_DIM());
    h
}

/// A window sidebar row: tint chip (or dashed chip when `planned`), label and
/// a small right-aligned `meta` text. Selected rows get the blue fill.
pub fn sidebar_item(ui: &mut Ui, rect: Rect, id: egui::Id, glyph: Icon, color: Color32, label: &str, meta: &str, selected: bool, planned: bool) -> Response {
    let resp = ui.interact(rect, id, if planned { Sense::hover() } else { Sense::click() });
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered() && !planned);
    let p = ui.painter();
    let active_text = pick(hex(0xD9F78A), hex(0x3F6B0A));
    if selected {
        p.rect_filled(rect, 8.0, pick(Color32::from_rgba_unmultiplied(200, 240, 90, 41), Color32::from_rgba_unmultiplied(125, 181, 25, 41)));
    } else if h > 0.0 {
        p.rect_filled(rect, 6.0, white((12.0 * h) as u8));
    }
    let chip_r = Rect::from_min_size(Pos2::new(rect.left() + 8.0, rect.center().y - 8.0), Vec2::splat(16.0));
    let glyph_col = if planned {
        TEXT_MUTED()
    } else if selected {
        active_text
    } else if color == tint::NEUTRAL() || color == Color32::TRANSPARENT {
        TEXT_DIM()
    } else {
        color
    };
    icon(p, glyph, chip_r, glyph_col);
    let mut right = rect.right() - 8.0;
    if !meta.is_empty() {
        right = p.text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, meta, font(11.0), if selected { with_alpha(active_text, 0.8) } else { white(120) }).left() - 6.0;
    }
    text_fit(p, Pos2::new(chip_r.right() + 9.0, rect.center().y), Align2::LEFT_CENTER, label, font(13.0), if planned { TEXT_FAINT() } else if selected { active_text } else { white(217) }, right - chip_r.right() - 9.0);
    if planned {
        return resp.on_hover_text("Planned: not built yet");
    }
    resp
}

/// Height of a [`sidebar_item`] row.
pub const SIDEBAR_ROW_H: f32 = 28.0;
/// Width of the window sidebar.
pub const SIDEBAR_W: f32 = 210.0;

/// Height of a window page header ([`page_header`]).
pub const PAGE_HEADER_H: f32 = 50.0;
/// Height of a window page footer ([`page_footer`]).
pub const PAGE_FOOTER_H: f32 = 60.0;

/// A window page's header: 20 pt title, one line of status under it and a
/// hairline. Returns the rect for buttons on the right (vertically centred).
pub fn page_header(p: &Painter, r: Rect, title: &str, sub: &str, sub_col: Color32) -> Rect {
    page_header_room(p, r, title, sub, sub_col, 0.0, r.right() - 260.0).0
}

/// [`page_header`] keeping `room` points free right after the title (for a
/// chip such as AI Check's); the status line moves right. Also returns the
/// left-centre point of that room. The status line ends before `sub_right`
/// (where the page's own header controls start).
pub fn page_header_room(p: &Painter, r: Rect, title: &str, sub: &str, sub_col: Color32, room: f32, sub_right: f32) -> (Rect, Pos2) {
    let cy = r.top() + PAGE_HEADER_H / 2.0;
    let t = p.text(Pos2::new(r.left() + sp::PAGE_EDGE, cy), Align2::LEFT_CENTER, title, ty::lead(), TEXT());
    let at = Pos2::new(t.right() + 10.0, cy);
    let sx = if room > 0.0 { at.x + room + 10.0 } else { at.x };
    // status sits on the same line as the title, like a macOS toolbar
    let sub_col = if sub_col == TEXT_DIM() { ty::caption_color() } else { sub_col };
    // never cut the status: drop its trailing " · " parts until it fits,
    // or leave it out
    let avail = sub_right - sx;
    let mut parts: Vec<&str> = sub.split(" · ").collect();
    while !parts.is_empty() {
        let s = parts.join(" · ");
        if p.layout_no_wrap(s.clone(), ty::label(), sub_col).size().x <= avail {
            p.text(Pos2::new(sx, cy + 1.0), Align2::LEFT_CENTER, s, ty::label(), sub_col);
            break;
        }
        parts.pop();
    }
    p.line_segment([Pos2::new(r.left(), r.top() + PAGE_HEADER_H), Pos2::new(r.right(), r.top() + PAGE_HEADER_H)], Stroke::new(1.0, Color32::from_black_alpha(115)));
    let h = sp::HEADER_CONTROL_H / 2.0;
    (Rect::from_min_max(Pos2::new(r.right() - sp::PAGE_EDGE, cy - h), Pos2::new(r.right() - sp::PAGE_EDGE, cy + h)), at)
}

/// A window page's footer strip (darker, with a top hairline). Returns its rect.
pub fn page_footer(p: &Painter, r: Rect, window_radius: f32) -> Rect {
    let f = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - PAGE_FOOTER_H), r.max);
    let rr = window_radius.round().clamp(0.0, 255.0) as u8;
    p.rect_filled(f, CornerRadius { nw: 0, ne: 0, sw: 0, se: rr }, FOOTER());
    p.line_segment([f.left_top(), f.right_top()], Stroke::new(1.0, white(20)));
    f
}

/// Height of [`sheet_header`].
pub const SHEET_HEADER_H: f32 = 58.0;

/// A label/value row inside a group card, with an optional hint line under it.
/// Returns the height used.
pub fn fact_row(p: &Painter, r: Rect, label: &str, value: &str, value_col: Color32, hint: &str) -> f32 {
    let top = r.top() + 8.0;
    let vr = p.text(Pos2::new(r.right() - 12.0, top), Align2::RIGHT_TOP, value, bold(13.0), value_col);
    text_fit(p, Pos2::new(r.left() + 12.0, top), Align2::LEFT_TOP, label, font(13.0), TEXT_DIM(), (vr.left() - r.left() - 24.0).max(40.0));
    if hint.is_empty() {
        return 32.0;
    }
    let h = paragraph(p, Pos2::new(r.left() + 12.0, top + 19.0), hint, font(12.0), TEXT_FAINT(), r.width() - 24.0);
    27.0 + h + 8.0
}

/// A legend row: colour square, label, value.
pub fn legend_row(p: &Painter, r: Rect, color: Color32, label: &str, value: &str, value_col: Color32) {
    let c = Pos2::new(r.left() + 16.5, r.center().y);
    p.rect_filled(Rect::from_center_size(c, Vec2::splat(9.0)), 3.0, color);
    let vr = p.text(Pos2::new(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, value, font(13.0), value_col);
    text_fit(p, Pos2::new(r.left() + 31.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT(), (vr.left() - r.left() - 40.0).max(40.0));
}

/// The height of one line of `font`.
pub fn line_h(p: &Painter, font: &FontId) -> f32 {
    p.fonts_mut(|f| f.row_height(font))
}

/// A title with a detail line under it, the pair centred on `cy` with
/// [`sp::LINE_GAP`] between them; both cut to `max_w`. Returns both rects.
#[allow(clippy::too_many_arguments)]
#[track_caller]
pub fn two_lines(p: &Painter, x: f32, cy: f32, title: &str, title_font: FontId, title_col: Color32, detail: &str, detail_font: FontId, detail_col: Color32, max_w: f32) -> (Rect, Rect) {
    let (t, d) = two_lines_y(p, cy, &title_font, &detail_font);
    let tr = text_fit(p, Pos2::new(x, t), Align2::LEFT_CENTER, title, title_font, title_col, max_w);
    let dr = text_fit(p, Pos2::new(x, d), Align2::LEFT_CENTER, detail, detail_font, detail_col, max_w);
    (tr, dr)
}

/// Lines stacked [`sp::LINE_GAP`] apart, the block centred on `cy`, each
/// starting at `x` and cut to `max_w` ("Memory" / "17%" / "normal").
/// Empty lines are skipped. Returns the rect of each line drawn.
#[track_caller]
pub fn stack_lines(p: &Painter, x: f32, cy: f32, lines: &[(&str, FontId, Color32)], max_w: f32) -> Vec<Rect> {
    let lines: Vec<&(&str, FontId, Color32)> = lines.iter().filter(|l| !l.0.is_empty()).collect();
    let hs: Vec<f32> = lines.iter().map(|l| line_h(p, &l.1)).collect();
    let total = hs.iter().sum::<f32>() + sp::LINE_GAP * (hs.len().max(1) - 1) as f32;
    let mut y = cy - total / 2.0;
    let mut out = Vec::new();
    for (l, h) in lines.iter().zip(hs) {
        out.push(text_fit(p, Pos2::new(x, y + h / 2.0), Align2::LEFT_CENTER, l.0, l.1.clone(), l.2, max_w));
        y += h + sp::LINE_GAP;
    }
    out
}

/// The centre lines of [`two_lines`]: (title y, detail y), for rows that
/// draw chips or values on the same lines.
pub fn two_lines_y(p: &Painter, cy: f32, title_font: &FontId, detail_font: &FontId) -> (f32, f32) {
    let (h1, h2) = (line_h(p, title_font), line_h(p, detail_font));
    let top = cy - (h1 + sp::LINE_GAP + h2) / 2.0;
    (top + h1 / 2.0, top + h1 + sp::LINE_GAP + h2 / 2.0)
}

/// A name on the left and a value on the right of one line (`left` and
/// `right` are its two ends, `align` picks top / centre / bottom). The value
/// the app writes (a time, a count) keeps its full width; the name gets
/// what is left and is cut if it must. Returns (name, value) rects.
#[allow(clippy::too_many_arguments)]
#[track_caller]
pub fn split_line(p: &Painter, left: Pos2, right: Pos2, v: egui::Align, name: &str, name_font: FontId, name_col: Color32, value: &str, value_font: FontId, value_col: Color32) -> (Rect, Rect) {
    let vr = p.text(right, Align2([egui::Align::Max, v]), value, value_font, value_col);
    let gap = if value.is_empty() { 0.0 } else { sp::M };
    let nr = text_fit(p, left, Align2([egui::Align::Min, v]), name, name_font, name_col, vr.left() - gap - left.x);
    (nr, vr)
}

/// Text that wraps onto up to `max_rows` lines at word breaks instead of
/// being cut (status lines like "Out tomorrow 23:40" in a narrow card).
#[allow(clippy::too_many_arguments)]
#[track_caller]
pub fn text_wrap(p: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32, max_w: f32, max_rows: usize) -> Rect {
    let caller = std::panic::Location::caller();
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w.max(1.0));
    job.wrap.max_rows = max_rows;
    job.wrap.overflow_character = Some('…');
    if align.x() == egui::Align::Center {
        // each line centred on pos.x
        job.halign = egui::Align::Center;
    }
    let g = p.layout_job(job);
    if g.elided {
        fit_log(caller, text, max_w);
    }
    let r = align.anchor_size(pos, g.size());
    let at = if align.x() == egui::Align::Center { Pos2::new(pos.x, r.top()) } else { r.min };
    let out = g.rect.translate(at.to_vec2());
    p.galley(at, g, color);
    out
}

/// Text that is cut with an ellipsis to fit `max_w`.
#[track_caller]
pub fn text_fit(painter: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32, max_w: f32) -> Rect {
    let caller = std::panic::Location::caller();
    // No glyph is narrower than about a quarter of the font size, so more
    // characters than this can never fit: don't lay out a whole long clip.
    let cap = ((max_w.max(0.0) / (font.size * 0.25).max(1.0)) as usize).saturating_add(2);
    let chars: Vec<char> = text.chars().take(cap + 1).collect();
    let mut g = if chars.len() > cap {
        painter.layout_no_wrap(chars[..cap].iter().collect::<String>() + "…", font.clone(), color)
    } else {
        painter.layout_no_wrap(text.to_owned(), font.clone(), color)
    };
    if g.size().x > max_w {
        // the longest prefix (plus "…") that fits: binary search, not one
        // layout per dropped character
        let cut = |n: usize| painter.layout_no_wrap(chars[..n].iter().collect::<String>() + "…", font.clone(), color);
        let (mut lo, mut hi) = (1usize, chars.len().min(cap));
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if cut(mid).size().x <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        g = cut(lo);
        fit_log(caller, text, max_w);
    }
    let rect = align.anchor_size(pos, g.size());
    painter.galley(rect.min, g, color);
    rect
}

/// `CMDMEOW_FIT_LOG=1`: print each text `text_fit` had to cut, once per
/// place and text, to find labels that don't fit (screenshot runs).
fn fit_log(at: &std::panic::Location, text: &str, max_w: f32) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Option<Mutex<std::collections::HashSet<String>>>> = OnceLock::new();
    let Some(seen) = SEEN.get_or_init(|| std::env::var_os("CMDMEOW_FIT_LOG").map(|_| Mutex::new(Default::default()))) else { return };
    let key = format!("{}:{} {text:?} (max {max_w:.0})", at.file(), at.line());
    if seen.lock().is_ok_and(|mut s| s.insert(key.clone())) {
        eprintln!("[fit] {key}");
    }
}

/// At most `max_chars` characters of `s` (a prefix, no copy).
pub fn prefix_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Middle-ellipsis for long paths: "/Users/me/Library/…/Cache".
pub fn middle_ellipsis(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars || max_chars < 8 {
        return s.to_owned();
    }
    let head = max_chars * 2 / 5;
    let tail = max_chars - head - 1;
    let a: String = chars[..head].iter().collect();
    let b: String = chars[chars.len() - tail..].iter().collect();
    format!("{a}…{b}")
}

/// Wrapped paragraph; returns the height used.
pub fn paragraph(painter: &Painter, pos: Pos2, text: &str, font: FontId, color: Color32, width: f32) -> f32 {
    // a paragraph sits in a box: a few thousand characters fill any of them,
    // and laying out a megabyte clip every frame stalls scrolling
    let g = painter.layout(prefix_chars(text, PARAGRAPH_MAX).to_owned(), font, color, width);
    let h = g.size().y;
    painter.galley(pos, g, color);
    h
}

/// Characters a [`paragraph`] lays out at most.
pub const PARAGRAPH_MAX: usize = 4000;

// ---------------------------------------------------------------- icons
#[allow(dead_code)] // the full symbol set: some glyphs wait for views not drawn yet

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Disk,
    Memory,
    Battery(u8),
    Cpu,
    Wifi,
    Bluetooth,
    Headphones,
    Shield,
    Sparkle,
    Broom,
    Gear,
    Trash,
    Folder,
    Lens,
    Laptop,
    Heart,
    Thermo,
    Clock,
    /// Sharing: a chain link.
    Link,
    Up,
    Down,
    Back,
    Forward,
    Info,
    Check,
    Question,
    Lock,
    Monitor,
    Chart,
    Power,
    // Control Center additions
    Map,
    Pulse,
    Crop,
    Clipboard,
    Fan,
    Cup,
    Home,
    Window,
    Close,
    Refresh,
    Search,
    Grip,
    // Bluetooth devices
    Earbuds,
    Mouse,
    Keyboard,
    Trackpad,
    Gamepad,
    Phone,
    Tablet,
    // permissions
    ScreenRec,
    Access,
    Script,
    Mic,
    Camera,
    Bell,
    /// A gauge with a needle: Claude usage limits.
    Gauge,
    /// An app with a cross: the Uninstaller.
    AppRemove,
    /// A file.
    Doc,
    /// An external monitor.
    Display,
    Sun,
    Speaker,
    SpeakerOff,
}

/// The SF Symbol behind each icon (outline variants: icons are monochrome strokes).
pub fn symbol_name(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Disk => "internaldrive",
        Icon::Memory => "memorychip",
        Icon::Battery(l) => match l {
            88..=255 => "battery.100percent",
            63..=87 => "battery.75percent",
            38..=62 => "battery.50percent",
            13..=37 => "battery.25percent",
            _ => "battery.0percent",
        },
        Icon::Cpu => "cpu",
        Icon::Wifi => "wifi",
        Icon::Bluetooth => "dot.radiowaves.left.and.right",
        Icon::Headphones => "headphones",
        Icon::Shield => "checkmark.shield",
        Icon::Sparkle => "sparkle",
        Icon::Broom => "wind",
        Icon::Gear => "gearshape",
        Icon::Trash => "trash",
        Icon::Folder => "folder",
        Icon::Doc => "doc",
        Icon::Display => "display",
        Icon::Sun => "sun.max",
        Icon::Speaker => "speaker.wave.2",
        Icon::SpeakerOff => "speaker.slash",
        Icon::Lens => "magnifyingglass",
        Icon::Laptop => "laptopcomputer",
        Icon::Heart => "heart",
        Icon::Thermo => "thermometer.medium",
        Icon::Clock => "clock",
        Icon::Link => "link",
        Icon::Up => "arrow.up",
        Icon::Down => "arrow.down",
        Icon::Back => "chevron.left",
        Icon::Forward => "chevron.right",
        Icon::Info => "info.circle",
        Icon::Check => "checkmark",
        Icon::Question => "questionmark",
        Icon::Lock => "lock",
        Icon::Monitor => "menubar.rectangle",
        Icon::Chart => "chart.bar",
        Icon::Power => "power",
        Icon::Map => "chart.pie",
        Icon::Pulse => "waveform.path.ecg",
        Icon::Crop => "crop",
        Icon::Clipboard => "doc.on.clipboard",
        Icon::Fan => "fanblades",
        Icon::Cup => "cup.and.saucer",
        Icon::Home => "house",
        Icon::Window => "macwindow",
        Icon::Close => "xmark",
        Icon::Refresh => "arrow.clockwise",
        Icon::Search => "magnifyingglass",
        Icon::Grip => "circle.grid.2x3.fill",
        Icon::Earbuds => "airpodspro",
        Icon::Mouse => "magicmouse",
        Icon::Keyboard => "keyboard",
        Icon::Trackpad => "rectangle.and.hand.point.up.left",
        Icon::Gamepad => "gamecontroller",
        Icon::Phone => "iphone",
        Icon::Tablet => "ipad",
        Icon::ScreenRec => "rectangle.dashed.badge.record",
        Icon::Access => "accessibility",
        Icon::Script => "applescript",
        Icon::Mic => "mic",
        Icon::Camera => "video",
        Icon::Bell => "bell",
        Icon::Gauge => "gauge.with.needle",
        Icon::AppRemove => "xmark.app",
    })
}

thread_local! {
    static SYMBOLS: std::cell::RefCell<std::collections::HashMap<(String, u32), Option<(egui::TextureHandle, Vec2)>>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Draw an SF Symbol tinted `color`, fitted (aspect kept) inside `rect`.
/// Returns false when the symbol isn't available (then a drawn icon is used).
pub fn symbol(p: &Painter, name: &str, rect: Rect, color: Color32) -> bool {
    let ppp = p.ctx().pixels_per_point();
    let px = (rect.height() * ppp * 0.92).round().max(8.0) as u32;
    let key = (name.to_string(), px);
    let entry = SYMBOLS.with(|m| m.borrow().get(&key).cloned());
    let entry = match entry {
        Some(e) => e,
        None => {
            let made = crate::kit::native::render_symbol(name, px as f64, crate::kit::native::weight_semibold()).map(|(w, h, rgba)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                let tex = p.ctx().load_texture(format!("sf:{name}:{px}"), img, egui::TextureOptions::LINEAR);
                (tex, Vec2::new(w as f32 / ppp, h as f32 / ppp))
            });
            SYMBOLS.with(|m| m.borrow_mut().insert(key, made.clone()));
            made
        }
    };
    let Some((tex, size)) = entry else { return false };
    let scale = (rect.width() / size.x).min(rect.height() / size.y).min(1.15);
    let r = Rect::from_center_size(rect.center(), size * scale);
    p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), color);
    true
}

/// Draw a small icon: Apple's SF Symbol when available, otherwise a drawn one.
pub fn icon(p: &Painter, icon: Icon, rect: Rect, color: Color32) {
    if let Some(name) = symbol_name(icon) {
        if symbol(p, name, rect, color) {
            return;
        }
    }
    icon_drawn(p, icon, rect, color);
}

/// Stroke-drawn fallbacks on a 24-unit grid (same shapes as the design's icons).
fn icon_drawn(p: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let u = s / 24.0;
    let pt = |x: f32, y: f32| c + Vec2::new((x - 12.0) * u, (y - 12.0) * u);
    let w = (1.8 * u).max(1.1);
    let st = Stroke::new(w, color);
    let open = |pts: &[(f32, f32)]| {
        p.add(PathShape::line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), PathStroke::new(w, color)));
    };
    let closed = |pts: &[(f32, f32)]| {
        p.add(PathShape::closed_line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), PathStroke::new(w, color)));
    };
    let boxr = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        p.rect_stroke(Rect::from_min_max(pt(x0, y0), pt(x1, y1)), r * u, st, StrokeKind::Middle);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| {
        p.add(PathShape::line(arc_points(pt(cx, cy), r * u, a0, a1, 24), PathStroke::new(w, color)));
    };
    let dot = |x: f32, y: f32, r: f32| p.circle_filled(pt(x, y), r * u, color);
    match icon {
        Icon::AppRemove => icon_drawn(p, Icon::Trash, rect, color),
        Icon::Disk => {
            boxr(3.0, 13.0, 21.0, 19.0, 1.5);
            open(&[(3.0, 13.0), (6.0, 5.0), (18.0, 5.0), (21.0, 13.0)]);
            dot(7.0, 16.0, 1.1);
        }
        Icon::Memory => {
            boxr(4.0, 7.0, 20.0, 17.0, 1.5);
            for x in [8.0, 12.0, 16.0] {
                open(&[(x, 7.0), (x, 17.0)]);
            }
        }
        Icon::Battery(level) => {
            boxr(3.0, 8.0, 18.0, 16.0, 2.0);
            open(&[(21.0, 11.0), (21.0, 13.0)]);
            let f = level.min(100) as f32 / 100.0;
            let fill = if level <= 20 { RED() } else { color };
            p.rect_filled(Rect::from_min_max(pt(5.0, 10.0), pt(5.0 + 11.0 * f, 14.0)), 1.0 * u, fill);
        }
        Icon::Cpu => {
            boxr(7.0, 7.0, 17.0, 17.0, 1.5);
            boxr(10.0, 10.0, 14.0, 14.0, 0.5);
            for o in [9.0, 15.0] {
                open(&[(o, 3.0), (o, 7.0)]);
                open(&[(o, 17.0), (o, 21.0)]);
                open(&[(3.0, o), (7.0, o)]);
                open(&[(17.0, o), (21.0, o)]);
            }
        }
        Icon::Wifi => {
            for r in [4.0f32, 8.5, 13.0] {
                arc(12.0, 19.5, r, -PI * 0.78, -PI * 0.22);
            }
            dot(12.0, 19.5, 1.3);
        }
        Icon::Bluetooth => open(&[(7.0, 7.5), (17.0, 16.0), (12.0, 20.5), (12.0, 3.5), (17.0, 8.0), (7.0, 16.5)]),
        Icon::Headphones => {
            arc(12.0, 12.0, 8.0, PI, TAU);
            open(&[(4.0, 12.0), (4.0, 15.0)]);
            open(&[(20.0, 12.0), (20.0, 15.0)]);
            boxr(4.0, 15.0, 7.0, 20.0, 1.0);
            boxr(17.0, 15.0, 20.0, 20.0, 1.0);
        }
        Icon::Shield => {
            closed(&[(12.0, 3.0), (19.5, 6.0), (18.5, 13.0), (12.0, 21.0), (5.5, 13.0), (4.5, 6.0)]);
            open(&[(8.5, 12.0), (11.0, 14.5), (15.5, 9.5)]);
        }
        Icon::Gauge => {
            arc(12.0, 14.0, 8.0, PI, TAU);
            open(&[(12.0, 14.0), (16.0, 9.0)]);
            p.circle_filled(pt(12.0, 14.0), 1.4 * u, color);
        }
        Icon::Sparkle => closed(&[(12.0, 3.0), (13.8, 8.2), (19.0, 10.0), (13.8, 11.8), (12.0, 17.0), (10.2, 11.8), (5.0, 10.0), (10.2, 8.2)]),
        Icon::Broom => {
            open(&[(18.0, 3.0), (12.0, 9.0)]);
            closed(&[(9.0, 8.0), (16.0, 15.0), (13.0, 21.0), (3.0, 11.0)]);
        }
        Icon::Gear => {
            stroke_circle(&p, pt(12.0, 12.0), 3.0 * u, st);
            stroke_circle(&p, pt(12.0, 12.0), 7.0 * u, st);
            for i in 0..8 {
                let a = i as f32 / 8.0 * TAU;
                let d = Vec2::new(a.cos(), a.sin());
                p.line_segment([pt(12.0, 12.0) + d * 7.0 * u, pt(12.0, 12.0) + d * 9.5 * u], Stroke::new(w * 1.4, color));
            }
        }
        Icon::Trash => {
            open(&[(4.0, 6.0), (20.0, 6.0)]);
            open(&[(9.0, 6.0), (9.0, 3.5), (15.0, 3.5), (15.0, 6.0)]);
            open(&[(6.0, 6.0), (7.0, 20.5), (17.0, 20.5), (18.0, 6.0)]);
        }
        Icon::Folder => closed(&[(3.0, 6.0), (9.5, 6.0), (11.5, 8.0), (21.0, 8.0), (21.0, 19.0), (3.0, 19.0)]),
        Icon::Doc => closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]),
        Icon::Lens | Icon::Search => {
            stroke_circle(&p, pt(11.0, 11.0), 7.0 * u, st);
            open(&[(16.0, 16.0), (21.0, 21.0)]);
        }
        Icon::Laptop => {
            boxr(5.0, 5.0, 19.0, 15.0, 1.5);
            open(&[(2.5, 18.5), (21.5, 18.5)]);
        }
        Icon::Heart => {
            let mut pts = Vec::new();
            for i in 0..48 {
                let t = i as f32 / 48.0 * TAU;
                let x = 16.0 * t.sin().powi(3);
                let y = -(13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos());
                pts.push(pt(12.0 + x * 0.5, 12.0 + y * 0.5));
            }
            p.add(PathShape::closed_line(pts, PathStroke::new(w, color)));
        }
        Icon::Thermo => {
            boxr(10.0, 3.0, 14.0, 15.0, 2.0);
            stroke_circle(&p, pt(12.0, 18.0), 3.0 * u, st);
            open(&[(12.0, 8.0), (12.0, 16.0)]);
        }
        Icon::Clock => {
            stroke_circle(&p, pt(12.0, 12.0), 9.0 * u, st);
            open(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Icon::Link => {
            // two links at 45°, as two short open loops
            open(&[(10.5, 13.5), (13.5, 10.5)]);
            open(&[(9.0, 11.0), (6.5, 13.5), (6.5, 17.5), (10.5, 17.5), (13.0, 15.0)]);
            open(&[(15.0, 13.0), (17.5, 10.5), (17.5, 6.5), (13.5, 6.5), (11.0, 9.0)]);
        }
        Icon::Up => {
            open(&[(12.0, 20.0), (12.0, 4.5)]);
            open(&[(6.5, 10.0), (12.0, 4.5), (17.5, 10.0)]);
        }
        Icon::Down => {
            open(&[(12.0, 4.0), (12.0, 19.5)]);
            open(&[(6.5, 14.0), (12.0, 19.5), (17.5, 14.0)]);
        }
        Icon::Back => open(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
        Icon::Forward => open(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
        Icon::Info => {
            stroke_circle(&p, pt(12.0, 12.0), 9.0 * u, st);
            dot(12.0, 7.8, 1.2);
            open(&[(12.0, 11.0), (12.0, 16.5)]);
        }
        Icon::Check => open(&[(5.0, 12.0), (10.0, 17.0), (19.0, 7.0)]),
        Icon::Question => {
            arc(12.0, 9.0, 4.0, PI * 1.05, PI * 2.45);
            open(&[(13.5, 12.8), (12.0, 14.0), (12.0, 15.5)]);
            dot(12.0, 19.0, 1.2);
        }
        Icon::Lock => {
            arc(12.0, 10.0, 4.0, PI, TAU);
            open(&[(8.0, 10.0), (8.0, 11.0)]);
            open(&[(16.0, 10.0), (16.0, 11.0)]);
            boxr(5.0, 11.0, 19.0, 20.0, 2.0);
        }
        Icon::Monitor | Icon::Window => {
            boxr(3.0, 5.0, 21.0, 19.0, 2.0);
            open(&[(3.0, 9.0), (21.0, 9.0)]);
        }
        Icon::Chart => {
            for (i, h) in [6.0f32, 11.0, 8.0, 14.0].iter().enumerate() {
                let x = 5.0 + i as f32 * 4.5;
                open(&[(x, 20.0), (x, 20.0 - h)]);
            }
        }
        Icon::Power => {
            arc(12.0, 13.0, 7.5, -PI * 0.3, PI * 1.3);
            open(&[(12.0, 3.0), (12.0, 12.0)]);
        }
        Icon::Map => {
            boxr(3.0, 4.0, 21.0, 20.0, 1.5);
            open(&[(3.0, 12.0), (12.0, 12.0)]);
            open(&[(12.0, 4.0), (12.0, 20.0)]);
            open(&[(12.0, 9.0), (21.0, 9.0)]);
        }
        Icon::Pulse => open(&[(3.0, 12.0), (7.0, 12.0), (10.0, 5.0), (14.0, 19.0), (17.0, 12.0), (21.0, 12.0)]),
        Icon::Crop => {
            open(&[(6.0, 2.0), (6.0, 18.0), (22.0, 18.0)]);
            open(&[(2.0, 6.0), (18.0, 6.0), (18.0, 22.0)]);
        }
        Icon::Clipboard => {
            boxr(9.0, 3.5, 15.0, 6.5, 0.8);
            open(&[(7.0, 5.0), (5.0, 5.0), (5.0, 21.0), (19.0, 21.0), (19.0, 5.0), (17.0, 5.0)]);
        }
        Icon::Fan => {
            // six curved blades around a hub (four straight arms read wrong)
            stroke_circle(&p, pt(12.0, 12.0), 1.8 * u, st);
            for k in 0..6 {
                let a0 = k as f32 * TAU / 6.0;
                let pts: Vec<Pos2> = (0..=6)
                    .map(|i| {
                        let t = i as f32 / 6.0;
                        let rr = (2.6 + 6.6 * t) * u;
                        let a = a0 + t * 0.9;
                        pt(12.0, 12.0) + Vec2::new(a.cos(), a.sin()) * rr
                    })
                    .collect();
                p.add(PathShape::line(pts, PathStroke::new(w, color)));
            }
        }
        Icon::Cup => {
            open(&[(4.0, 9.0), (16.0, 9.0), (16.0, 14.0)]);
            arc(11.0, 14.0, 5.0, 0.0, PI);
            open(&[(4.0, 9.0), (4.0, 14.0)]);
            arc(18.0, 13.0, 2.0, -PI / 2.0, PI / 2.0);
            open(&[(8.0, 3.0), (8.0, 6.0)]);
            open(&[(12.0, 3.0), (12.0, 6.0)]);
        }
        Icon::Home => closed(&[(3.0, 11.0), (12.0, 4.0), (21.0, 11.0), (21.0, 20.0), (3.0, 20.0)]),
        Icon::Close => {
            open(&[(6.0, 6.0), (18.0, 18.0)]);
            open(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Refresh => {
            arc(12.0, 12.0, 8.0, -PI * 0.25, PI * 1.6);
            open(&[(20.0, 4.0), (20.0, 9.0), (15.0, 9.0)]);
        }
        Icon::Grip => {
            for (x, y) in [(8.0, 6.0), (16.0, 6.0), (8.0, 12.0), (16.0, 12.0), (8.0, 18.0), (16.0, 18.0)] {
                dot(x, y, 1.6);
            }
        }
        Icon::Earbuds => {
            boxr(5.0, 4.0, 10.0, 11.0, 2.5);
            open(&[(8.0, 11.0), (8.0, 20.0)]);
            boxr(14.0, 4.0, 19.0, 11.0, 2.5);
            open(&[(16.0, 11.0), (16.0, 20.0)]);
        }
        Icon::Mouse => {
            boxr(6.0, 3.0, 18.0, 21.0, 6.0);
            open(&[(12.0, 6.5), (12.0, 9.5)]);
        }
        Icon::Keyboard => {
            boxr(3.0, 7.0, 21.0, 17.0, 2.0);
            for x in [7.0, 11.0, 15.0] {
                dot(x, 10.5, 0.9);
            }
            open(&[(7.0, 14.0), (17.0, 14.0)]);
        }
        Icon::Trackpad => {
            boxr(4.0, 5.0, 20.0, 19.0, 2.5);
            open(&[(4.0, 15.0), (20.0, 15.0)]);
            open(&[(12.0, 15.0), (12.0, 19.0)]);
        }
        Icon::Gamepad => {
            boxr(3.0, 7.0, 21.0, 17.0, 5.0);
            open(&[(6.5, 12.0), (10.5, 12.0)]);
            open(&[(8.5, 10.0), (8.5, 14.0)]);
            dot(15.5, 11.0, 1.1);
            dot(17.5, 13.5, 1.1);
        }
        Icon::Phone => {
            boxr(7.0, 3.0, 17.0, 21.0, 2.5);
            open(&[(10.5, 18.0), (13.5, 18.0)]);
        }
        Icon::Tablet => {
            boxr(4.0, 3.0, 20.0, 21.0, 2.5);
            dot(12.0, 18.5, 0.9);
        }
        Icon::ScreenRec => {
            boxr(3.0, 5.0, 21.0, 17.0, 2.0);
            open(&[(9.0, 21.0), (15.0, 21.0)]);
            dot(12.0, 11.0, 2.4);
        }
        Icon::Access => {
            dot(12.0, 4.5, 1.6);
            open(&[(5.0, 8.5), (12.0, 10.0), (19.0, 8.5)]);
            open(&[(12.0, 10.0), (12.0, 14.0)]);
            open(&[(8.5, 21.0), (12.0, 14.0), (15.5, 21.0)]);
        }
        Icon::Script => {
            open(&[(7.0, 5.0), (3.0, 12.0), (7.0, 19.0)]);
            open(&[(17.0, 5.0), (21.0, 12.0), (17.0, 19.0)]);
            open(&[(14.0, 4.0), (10.0, 20.0)]);
        }
        Icon::Mic => {
            boxr(9.0, 3.0, 15.0, 14.0, 3.0);
            arc(12.0, 11.0, 6.0, 0.0, std::f32::consts::PI);
            open(&[(12.0, 17.0), (12.0, 21.0)]);
        }
        Icon::Camera => {
            boxr(3.0, 7.0, 15.0, 17.0, 2.0);
            closed(&[(15.0, 11.0), (21.0, 8.0), (21.0, 16.0), (15.0, 13.0)]);
        }
        Icon::Bell => {
            open(&[(5.0, 17.0), (7.0, 15.0), (7.0, 10.0)]);
            arc(12.0, 10.0, 5.0, std::f32::consts::PI, std::f32::consts::TAU);
            open(&[(17.0, 10.0), (17.0, 15.0), (19.0, 17.0), (5.0, 17.0)]);
            open(&[(10.0, 20.0), (14.0, 20.0)]);
        }
        Icon::Display => {
            boxr(3.0, 4.0, 21.0, 16.0, 1.5);
            open(&[(12.0, 16.0), (12.0, 20.0)]);
            open(&[(8.0, 20.0), (16.0, 20.0)]);
        }
        Icon::Sun => {
            p.circle_stroke(pt(12.0, 12.0), 4.0 * u, st);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (dx, dy) = (a.cos(), a.sin());
                open(&[(12.0 + dx * 7.0, 12.0 + dy * 7.0), (12.0 + dx * 9.5, 12.0 + dy * 9.5)]);
            }
        }
        Icon::Speaker | Icon::SpeakerOff => {
            closed(&[(3.0, 9.0), (7.0, 9.0), (12.0, 5.0), (12.0, 19.0), (7.0, 15.0), (3.0, 15.0)]);
            if icon == Icon::Speaker {
                arc(13.0, 12.0, 4.0, -0.9, 0.9);
                arc(13.0, 12.0, 7.5, -0.9, 0.9);
            } else {
                open(&[(15.0, 9.0), (21.0, 15.0)]);
                open(&[(21.0, 9.0), (15.0, 15.0)]);
            }
        }
    }
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", b)
    } else if v >= 100.0 {
        format!("{:.0} {}", v, UNITS[i])
    } else if v >= 10.0 {
        format!("{:.1} {}", v, UNITS[i])
    } else {
        format!("{:.2} {}", v, UNITS[i])
    }
}

pub fn human_rate(bps: f64) -> String {
    format!("{}/sec", human_bytes(bps.max(0.0) as u64))
}

pub fn human_duration(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorative_beat_lands_on_a_fixed_clock() {
        let p = 1.0 / 30.0;
        assert!((next_beat(0.0, 30.0) - p).abs() < 1e-9);
        assert!((next_beat(0.01, 30.0) - (p - 0.01)).abs() < 1e-9);
        // too close to the tick: skip to the one after
        assert!((next_beat(p - 0.001, 30.0) - (p + 0.001)).abs() < 1e-9);
    }

    /// One pass at `time`, with or without a pointer move; returns the
    /// decorative clock and whether a frame was asked for.
    fn ambient_pass(ctx: &egui::Context, time: f64, moved: bool) -> (f64, bool) {
        let mut input = egui::RawInput { time: Some(time), ..Default::default() };
        if moved {
            input.events.push(egui::Event::PointerMoved(Pos2::new(time as f32, 5.0)));
        }
        let mut clock = 0.0;
        let mut out = ctx.run_ui(input, |ui| {
            clock = ambient_clock(ui.ctx());
            ambient(ui.ctx());
        });
        out.textures_delta.clear();
        let asked = out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay < std::time::Duration::from_secs(1));
        (clock, asked)
    }

    #[test]
    fn decorative_loops_rest_without_input_and_wake_on_a_move() {
        let ctx = egui::Context::default();
        let (c0, asked) = ambient_pass(&ctx, 1.0, true);
        assert!(asked);
        let (c1, asked) = ambient_pass(&ctx, 1.05, false);
        assert!(asked && (c1 - c0 - 0.05).abs() < 1e-9);
        // frames every half second (the panel's heartbeat), no input
        let mut t = 1.05;
        let mut last = c1;
        while t < 1.0 + AMBIENT_REST + 2.0 {
            t += 0.5;
            let (c, asked) = ambient_pass(&ctx, t, false);
            last = c;
            if t - 1.0 >= AMBIENT_REST {
                assert!(!asked, "still animating at {t}");
            }
        }
        // resting: the clock holds still
        let (held, _) = ambient_pass(&ctx, t + 0.5, false);
        assert_eq!(held, last);
        // a move wakes the loops without jumping them ahead
        let (woke, asked) = ambient_pass(&ctx, t + 3.0, true);
        assert!(asked && woke - held <= 0.1 + 1e-9);
    }

    /// Runs `f` with a painter inside a real egui frame.
    fn with_painter(f: impl FnOnce(&Painter)) {
        let ctx = egui::Context::default();
        let mut f = Some(f);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                f(ui.painter());
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn text_fit_cuts_long_text_fast() {
        with_painter(|p| {
            let long = "word ".repeat(200_000);
            let t = std::time::Instant::now();
            let r = text_fit(p, Pos2::ZERO, Align2::LEFT_TOP, &long, FontId::proportional(13.0), Color32::WHITE, 200.0);
            assert!(r.width() <= 200.0 && r.width() > 150.0, "{r:?}");
            // used to lay out the text once per dropped character
            assert!(t.elapsed().as_millis() < 200, "{:?}", t.elapsed());
            let short = text_fit(p, Pos2::ZERO, Align2::LEFT_TOP, "hi", FontId::proportional(13.0), Color32::WHITE, 200.0);
            assert!(short.width() < 30.0);
        });
    }

    #[test]
    fn prefixes() {
        assert_eq!(prefix_chars("héllo", 2), "hé");
        assert_eq!(prefix_chars("hi", 5), "hi");
    }

    #[test]
    fn byte_sizes() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_500), "1.50 KB");
        assert_eq!(human_bytes(46_100_000), "46.1 MB");
        assert_eq!(human_bytes(2_000_000_000_000), "2.00 TB");
        assert_eq!(human_bytes(u64::MAX), format!("{:.0} TB", u64::MAX as f64 / 1e12));
    }

    #[test]
    fn middle_ellipsis_keeps_ends() {
        assert_eq!(middle_ellipsis("short", 20), "short");
        let s = middle_ellipsis("/Users/me/Library/Caches/com.example.app/Cache", 20);
        assert_eq!(s.chars().count(), 20);
        assert!(s.starts_with("/Users/m") && s.ends_with("app/Cache") && s.contains('…'));
        // multi-byte text is cut on characters
        assert_eq!(middle_ellipsis("ааааааааааббббббббб", 10).chars().count(), 10);
    }
}

/// How a [`text_input`] looks: the magnifier and clear button of a search
/// field, hidden typing, or text kept to the right (amounts).
#[derive(Clone, Copy, Default)]
pub struct FieldStyle {
    pub search: bool,
    pub secret: bool,
    pub right: bool,
}

/// Gap between the caret and the hint, so the caret never sits on its first letter.
const HINT_GAP: f32 = 3.0;

/// The one text field: a rounded fill, a ring while focused, text and hint
/// in body type on one baseline, vertically centred in `r` whatever its height.
pub fn text_input(ui: &mut Ui, r: Rect, id: egui::Id, text: &mut String, hint: &str, st: FieldStyle) -> egui::Response {
    let p = ui.painter().clone();
    let fid = ty::body();
    let edit_id = id.with("edit");
    let focused = ui.ctx().memory(|m| m.focused()) == Some(edit_id);
    p.rect_filled(r, R_SMALL, white(15));
    if focused {
        p.rect_stroke(r, R_SMALL, Stroke::new(1.0, white(46)), egui::StrokeKind::Inside);
    }
    let clear = st.search && !text.is_empty();
    let left = if st.search {
        icon(&p, Icon::Search, Rect::from_center_size(Pos2::new(r.left() + 15.0, r.center().y), Vec2::splat(13.0)), TEXT_FAINT());
        r.left() + 28.0
    } else {
        r.left() + 10.0
    };
    let right = if clear { r.right() - 26.0 } else { r.right() - 10.0 };
    // exactly one line tall, centred: the caret, the text and the hint share it
    let row_h = ui.ctx().fonts_mut(|f| f.row_height(&fid));
    // egui grows a TextEdit to interact_size.y downwards; size the row to match so it stays centred
    let row_h = row_h.max(ui.spacing().interact_size.y);
    let tr = Rect::from_min_max(Pos2::new(left, r.center().y - row_h / 2.0), Pos2::new(right.max(left + 1.0), r.center().y + row_h / 2.0));
    if text.is_empty() {
        let (at, align) = if st.right { (Pos2::new(tr.right() - HINT_GAP, r.center().y), Align2::RIGHT_CENTER) } else { (Pos2::new(tr.left() + HINT_GAP, r.center().y), Align2::LEFT_CENTER) };
        text_fit(&p, at, align, hint, fid.clone(), TEXT_FAINT(), tr.width() - HINT_GAP);
    }
    let align = if st.right { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
    let te = egui::TextEdit::singleline(text).id(edit_id).frame(egui::Frame::NONE).margin(Vec2::ZERO).font(fid).text_color(TEXT()).password(st.secret).align(align).desired_width(tr.width()).min_size(tr.size());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tr).layout(egui::Layout::left_to_right(egui::Align::Min)));
    child.visuals_mut().text_cursor.stroke = Stroke::new(1.5, TEXT());
    let resp = child.add(te);
    if clear {
        let cr = Rect::from_center_size(Pos2::new(r.right() - 14.0, r.center().y), Vec2::splat(16.0));
        let cresp = ui.interact(cr, id.with("clear"), egui::Sense::click());
        p.circle_filled(cr.center(), 7.0, if cresp.hovered() { white(70) } else { white(40) });
        let d = 2.5;
        let s = Stroke::new(1.3, Color32::from_black_alpha(200));
        p.line_segment([cr.center() + Vec2::new(-d, -d), cr.center() + Vec2::new(d, d)], s);
        p.line_segment([cr.center() + Vec2::new(-d, d), cr.center() + Vec2::new(d, -d)], s);
        if cresp.on_hover_text("Clear").clicked() {
            text.clear();
            resp.request_focus();
        }
    }
    if st.search && resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        text.clear();
    }
    resp
}

/// A search field: [`text_input`] with the magnifier and a clear button.
pub fn search_field(ui: &mut Ui, r: Rect, text: &mut String, hint: &str, focus: bool) -> egui::Response {
    let id = ui.scope_id().with(("search-field", hint));
    let resp = text_input(ui, r, id, text, hint, FieldStyle { search: true, ..Default::default() });
    if focus && !resp.has_focus() {
        resp.request_focus();
    }
    resp
}

/// A circle outline centred on `r`. egui 0.37 paints circle strokes outside
/// the radius, which put ring tracks a half-stroke off the arcs drawn on them.
pub fn stroke_circle(p: &Painter, c: Pos2, r: f32, stroke: impl Into<Stroke>) {
    let s: Stroke = stroke.into();
    p.circle_stroke(c, (r - s.width / 2.0).max(0.0), s);
}
