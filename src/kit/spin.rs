//! Things that move without redrawing the window: the fan glyph, the arc of
//! work going on, a progress bar easing to its value.
//!
//! egui redraws a whole window for any change, so blades turning at 30 fps
//! kept an open panel at ~12% CPU (each frame ~3.3 ms on an M1), and a
//! spinner drawn at a few frames a second stutters. On macOS they are
//! instead Core Animation layers over the egui surface, moved by the window
//! server: no frames of ours at all.
//!
//! A native layer always sits over everything egui draws, so it is only
//! used while it can't be told apart from egui drawing it: nothing egui
//! shows (a popover, a tooltip) overlaps it, it stays where it was last
//! frame (not scrolling or sliding), its painter isn't faded, and nothing
//! asked to [`suspend`] it (the widget editor). Otherwise egui draws the
//! same thing at the same angle.
//!
//! Widgets call [`blades`], [`arc`] or [`bar`] while drawing; the shell
//! calls [`flush`] at the end of every pass of a window that may show them.

use egui::{Color32, LayerId, Painter, Pos2, Rect, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;

pub use crate::kit::loader::MarkState;

/// What a request draws.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Look {
    /// The fan glyph (SP11): four rounded blades, the hub cut out.
    Blades,
    /// An open circle turning: work going on.
    Arc,
    /// A bar filled to `frac` on a faint track.
    Bar { frac: f32 },
    /// The removal loader's mark (kit::loader): the keycap in `track`, lime
    /// up to `frac` with a wave on top, the ⌘ or ✓ cut out in `bg`.
    Mark { frac: f32, state: MarkState, track: Color32, bg: Color32 },
}

struct Request {
    painter: Painter,
    key: String,
    /// The blades' or the arc's square (centre and radius), or the bar.
    rect: Rect,
    color: Color32,
    look: Look,
    /// Turns a second.
    tps: f64,
    /// Turning even while the app rests (the arc shows work going on;
    /// blades are decoration and rest with the other loops).
    always: bool,
}

/// Where a key's layer points: `angle0` at time `t0`, turning `tps` turns a second.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Turn {
    angle0: f64,
    t0: f64,
    tps: f64,
}

impl Turn {
    fn angle_at(&self, t: f64) -> f64 {
        (self.angle0 + (t - self.t0).max(0.0) * self.tps * std::f64::consts::TAU).rem_euclid(std::f64::consts::TAU)
    }
    /// Carry on from where the blades are at `t`, at a new speed.
    fn retimed(&self, t: f64, tps: f64) -> Turn {
        Turn { angle0: self.angle_at(t), t0: t, tps }
    }
}

thread_local! {
    static PENDING: RefCell<Vec<(egui::ViewportId, Request)>> = const { RefCell::new(Vec::new()) };
    static TURNS: RefCell<HashMap<String, Turn>> = RefCell::new(HashMap::new());
    static SUSPENDED: RefCell<Vec<egui::ViewportId>> = const { RefCell::new(Vec::new()) };
    static ALLOWED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// One turn every 2400 / rpm seconds: slow enough to read while still
/// following the real speed.
fn turns_per_sec(rpm: f32) -> f64 {
    rpm.max(0.0) as f64 / 2400.0
}

/// The arc: a 4.2 rad stroke turning 4 rad a second (as `glass::spinner`).
const ARC_SPAN: f32 = 4.2;
const ARC_TPS: f64 = 4.0 / std::f64::consts::TAU;
const ARC_WIDTH: f32 = 2.5;

fn push(p: &Painter, req: Request) {
    let id = p.ctx().viewport_id();
    PENDING.with(|q| q.borrow_mut().push((id, req)));
}

/// The fan glyph at `c`, reaching `r` from its hub, turning with the fan's
/// `rpm`. Its hub is cut out: whatever is under it shows through. Drawn
/// natively or by egui when the pass ends.
pub fn blades(p: &Painter, key: &str, c: Pos2, r: f32, rpm: f32, color: Color32) {
    // CMDMEOW_SPIN_RPM=<n>: turn at n rpm whatever the fans do (to check
    // the blades on a cool Mac)
    static TEST_RPM: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let rpm = TEST_RPM.get_or_init(|| std::env::var("CMDMEOW_SPIN_RPM").ok().and_then(|v| v.parse().ok())).unwrap_or(rpm);
    let rect = Rect::from_center_size(c, Vec2::splat(r * 2.0));
    push(p, Request { painter: p.clone(), key: key.to_string(), rect, color, look: Look::Blades, tps: turns_per_sec(rpm), always: false });
}

/// A turning arc of radius `r` at `c`: something is being worked on.
pub fn arc(p: &Painter, key: &str, c: Pos2, r: f32, color: Color32) {
    let rect = Rect::from_center_size(c, Vec2::splat(r * 2.0));
    push(p, Request { painter: p.clone(), key: key.to_string(), rect, color, look: Look::Arc, tps: ARC_TPS, always: true });
}

/// A work spinner keyed by where it's drawn from (`at`, the caller's line)
/// and how many that line drew before it this pass, for callers without a
/// name of their own (`glass::spinner`): a spinner that moves (a growing
/// Disk Map bubble, a scrolled row) keeps its one layer instead of getting
/// a new layer every frame.
pub fn arc_at(p: &Painter, c: Pos2, r: f32, color: Color32, at: &'static std::panic::Location<'static>) {
    let id = p.ctx().viewport_id();
    let site = egui::Id::NULL.with(("spin-arc", at.file(), at.line(), at.column(), p.layer_id()));
    let prefix = format!("arc-{}-", site.value());
    let n = PENDING.with(|q| q.borrow().iter().filter(|(v, r)| *v == id && r.key.starts_with(&prefix)).count());
    arc(p, &format!("{prefix}{n}"), c, r, color);
}

/// A progress bar filled to `frac` (as `glass::bar`); natively it eases
/// from one value to the next, so a few frames a second look smooth.
pub fn bar(p: &Painter, key: &str, rect: Rect, frac: f32, color: Color32) {
    push(p, Request { painter: p.clone(), key: key.to_string(), rect, color, look: Look::Bar { frac: frac.clamp(0.0, 1.0) }, tps: 0.0, always: false });
}

/// The removal loader's mark in `rect`: lime (`color`) filled to `frac`,
/// moving as `state` says; natively the fill eases, the wave runs and the
/// ⌘ turns into a ✓ without any egui frames.
#[allow(clippy::too_many_arguments)]
pub fn mark(p: &Painter, key: &str, rect: Rect, frac: f32, state: MarkState, color: Color32, track: Color32, bg: Color32) {
    push(p, Request { painter: p.clone(), key: key.to_string(), rect, color, look: Look::Mark { frac: frac.clamp(0.0, 1.0), state, track, bg }, tps: 0.0, always: false });
}

/// Draw blades with egui this pass (the widget editor: its widgets jiggle
/// and get dragged around).
pub fn suspend(ctx: &egui::Context) {
    let id = ctx.viewport_id();
    SUSPENDED.with(|s| s.borrow_mut().push(id));
}

/// Native layers on or off for everything (off for screenshots and demo
/// views, which egui renders itself).
pub fn set_allowed(on: bool) {
    ALLOWED.with(|a| a.set(on));
}

/// Native layers may be used now (off for screenshots, demo views and
/// `CMDMEOW_NATIVE_SPIN=0`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_allowed() -> bool {
    native_wanted()
}

fn native_wanted() -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ALLOWED.with(|a| a.get()) && *ENV.get_or_init(|| std::env::var("CMDMEOW_NATIVE_SPIN").map(|v| v != "0").unwrap_or(true))
}

/// The fan glyph at `angle`, drawn by egui: a white texture of the
/// glyph at this size, turned and tinted `color`.
pub fn draw(p: &Painter, c: Pos2, r: f32, angle: f32, color: Color32) {
    let ppp = p.ctx().pixels_per_point();
    // a texture near the drawn size so it stays sharp; sizes go up in
    // steps of 8 px so a sweep of sizes (the widget editor, a move between
    // displays) makes a few textures, not one per size
    let px = ((r * 2.0 * ppp).ceil() as u32 + 2).div_ceil(8).saturating_mul(8).clamp(8, 1024);
    let tex = glyph_texture(p.ctx(), px);
    // the glyph reaches px/2 - 1 in the texture: scale it to reach r
    let half = r * (px as f32 / 2.0) / (px as f32 / 2.0 - 1.0);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    let (sin, cos) = angle.sin_cos();
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (x, y) = ((u * 2.0 - 1.0) * half, (v * 2.0 - 1.0) * half);
        mesh.vertices.push(egui::epaint::Vertex { pos: c + Vec2::new(x * cos - y * sin, x * sin + y * cos), uv: Pos2::new(u, v), color });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
}

/// The fan glyph (SP11; Flaticon Premium icon 17711600, source and
/// licence in assets/glyphs/SOURCES.md): outlines in a circle of
/// radius 1 around the hub, y down. Each is a start point, then cubic
/// segments as (control, control, end). The second outline is the hub,
/// cut out (even-odd).
const GLYPH: &[&[[f32; 2]]] = &[
    &[[0.6696, -0.4512], [0.5534, -0.3566], [0.4371, -0.2621], [0.3209, -0.1675], [0.2976, -0.2140], [0.2651, -0.2559], [0.2232, -0.2884], [0.2883, -0.4233], [0.3302, -0.5953], [0.3302, -0.6837], [0.3302, -0.8604], [0.1860, -0.9999], [0.0140, -0.9999], [-0.1628, -0.9999], [-0.5859, -0.9301], [-0.4464, -0.6790], [-0.3503, -0.5628], [-0.2542, -0.4465], [-0.1581, -0.3303], [-0.2046, -0.3070], [-0.2465, -0.2745], [-0.2790, -0.2326], [-0.4139, -0.2977], [-0.5859, -0.3396], [-0.6743, -0.3396], [-0.8510, -0.3396], [-0.9905, -0.1954], [-0.9905, -0.0233], [-0.9905, 0.1534], [-0.9208, 0.5766], [-0.6696, 0.4370], [-0.5534, 0.3409], [-0.4371, 0.2448], [-0.3209, 0.1487], [-0.2976, 0.1952], [-0.2651, 0.2371], [-0.2232, 0.2696], [-0.2883, 0.4045], [-0.3302, 0.5766], [-0.3302, 0.6649], [-0.3302, 0.8416], [-0.1860, 0.9811], [-0.0140, 0.9811], [0.1628, 0.9811], [0.5859, 0.9114], [0.4464, 0.6603], [0.3503, 0.5440], [0.2542, 0.4277], [0.1581, 0.3115], [0.2046, 0.2882], [0.2465, 0.2557], [0.2790, 0.2138], [0.4139, 0.2789], [0.5859, 0.3208], [0.6743, 0.3208], [0.8510, 0.3208], [0.9905, 0.1766], [0.9905, 0.0046], [0.9905, -0.1675], [0.9208, -0.5907], [0.6696, -0.4512]],
    &[[0.0000, 0.1720], [-0.0977, 0.1720], [-0.1721, 0.0929], [-0.1721, -0.0001], [-0.1721, -0.0977], [-0.0930, -0.1721], [0.0000, -0.1721], [0.0977, -0.1721], [0.1721, -0.0931], [0.1721, -0.0001], [0.1721, 0.0929], [0.0977, 0.1720], [0.0000, 0.1720]],
];

/// [`GLYPH`] as a tiny-skia path, scaled by `k` around `(c, c)`.
fn glyph_skia(c: f32, k: f32) -> Option<tiny_skia::Path> {
    let at = |q: [f32; 2]| (c + q[0] * k, c + q[1] * k);
    let mut pb = tiny_skia::PathBuilder::new();
    for o in GLYPH {
        let (x, y) = at(o[0]);
        pb.move_to(x, y);
        for seg in o[1..].chunks_exact(3) {
            let ((x1, y1), (x2, y2), (x, y)) = (at(seg[0]), at(seg[1]), at(seg[2]));
            pb.cubic_to(x1, y1, x2, y2, x, y);
        }
        pb.close();
    }
    pb.finish()
}

/// The glyph filling a `px` square (the glyph's reach 1 px in from the
/// edge), white with coverage as alpha, made once per size.
fn glyph_texture(ctx: &egui::Context, px: u32) -> egui::TextureHandle {
    let id = egui::Id::NULL.with(("cmdmeow-fan-glyph", px));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let mut rgba = vec![0u8; (px * px * 4) as usize];
    if let (Some(mut pm), Some(path)) = (tiny_skia::Pixmap::new(px, px), glyph_skia(px as f32 / 2.0, px as f32 / 2.0 - 1.0)) {
        let mut ink = tiny_skia::Paint::default();
        ink.set_color_rgba8(255, 255, 255, 255);
        ink.anti_alias = true;
        pm.fill_path(&path, &ink, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::identity(), None);
        rgba = pm.take();
        // premultiplied white → straight white with the coverage as alpha
        for c in rgba.chunks_exact_mut(4) {
            if c[3] > 0 {
                c[..3].fill(255);
            }
        }
    }
    let ci = egui::ColorImage::from_rgba_unmultiplied([px as usize, px as usize], &rgba);
    let t = ctx.load_texture("cmdmeow-fan-glyph", ci, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// What egui draws for `req` at `angle`.
fn draw_req(req: &Request, angle: f32) {
    let (p, c, r) = (&req.painter, req.rect.center(), req.rect.width() / 2.0);
    match req.look {
        Look::Blades => draw(p, c, r, angle, req.color),
        Look::Arc => {
            p.add(egui::epaint::PathShape::line(crate::kit::glass::arc_points(c, r, angle, angle + ARC_SPAN, 40), egui::epaint::PathStroke::new(ARC_WIDTH, req.color)));
        }
        Look::Bar { frac } => crate::kit::glass::bar(p, req.rect, frac, req.color),
        Look::Mark { frac, state, track, bg } => crate::kit::loader::draw_mark(p, req.rect, frac, state, req.color, track, bg),
    }
}

/// The bar's track (as `glass::bar` draws it).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn track() -> Color32 {
    crate::kit::glass::white(20)
}

/// How much of a round-capped line from h/2 to w - h/2 to stroke so it
/// covers what `glass::bar` fills at `frac`: max(w·frac, h) from the left.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn stroke_end(w: f32, h: f32, frac: f32) -> f64 {
    if frac <= 0.0 || w <= h {
        return if frac > 0.0 { 1.0 } else { 0.0 };
    }
    (((w * frac - h) / (w - h)).clamp(0.0, 1.0) as f64).max(1e-4)
}

/// Is any egui layer drawn over `layer` covering `rect`?
fn covered(ctx: &egui::Context, layer: LayerId, rect: Rect) -> bool {
    ctx.memory(|m| {
        let order: Vec<LayerId> = m.layer_ids().collect();
        let above: Vec<LayerId> = match order.iter().position(|l| *l == layer) {
            Some(i) => order[i + 1..].to_vec(),
            None => order.into_iter().filter(|l| l.order > layer.order).collect(),
        };
        above.iter().any(|l| m.areas().is_visible(l) && m.area_rect(l.id).is_some_and(|a| a.intersects(rect)))
    })
}

/// End of a window's pass: show this pass's requests natively where that
/// looks the same, draw the rest with egui, drop layers no longer drawn.
pub fn flush(ctx: &egui::Context) {
    let id = ctx.viewport_id();
    let reqs: Vec<Request> = PENDING.with(|q| {
        let mut q = q.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(v, _)| *v == id);
        *q = rest;
        mine.into_iter().map(|(_, r)| r).collect()
    });
    let suspended = SUSPENDED.with(|s| {
        let mut s = s.borrow_mut();
        let was = s.contains(&id);
        s.retain(|v| *v != id);
        was
    });
    // blades turn only while the decorative loops run (they rest when idle)
    let live = crate::kit::glass::ambient_live(ctx);
    let now = clock();
    let mut keep = Vec::new();
    let (mut egui_turning, mut egui_working) = (false, false);
    for req in &reqs {
        let tps = if req.always || live { req.tps } else { 0.0 };
        let turn = TURNS.with(|t| {
            let mut t = t.borrow_mut();
            let e = t.entry(req.key.clone()).or_insert(Turn { angle0: 0.0, t0: now, tps });
            if e.tps != tps {
                *e = e.retimed(now, tps);
            }
            *e
        });
        let rect = req.rect.expand(2.0);
        let clip = req.painter.clip_rect().intersect(ctx.content_rect());
        let can = native_wanted()
            && !suspended
            && req.painter.opacity() >= 0.999
            && clip.intersects(rect)
            && !covered(ctx, req.painter.layer_id(), rect);
        // a layer that can't show now stays (hidden) for when it can
        let shown = if can {
            match native::show(id, &req.key, clip, req.rect, req.color, req.look, turn) {
                Some(on) => {
                    keep.push(req.key.clone());
                    on
                }
                None => false,
            }
        } else {
            if native::hide(id, &req.key) {
                keep.push(req.key.clone());
            }
            false
        };
        if !shown {
            draw_req(req, turn.angle_at(now) as f32);
            if tps > 0.0 {
                if req.always {
                    egui_working = true;
                } else {
                    egui_turning = true;
                }
            }
        }
    }
    native::retain(id, &keep);
    if egui_working {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    } else if egui_turning {
        crate::kit::glass::ambient(ctx);
    }
}

/// End of the main pass: requests made in a window whose pass already
/// ended without a [`flush`] can't be drawn any more; forget them.
pub fn drop_unflushed() {
    PENDING.with(|q| q.borrow_mut().clear());
    SUSPENDED.with(|s| s.borrow_mut().clear());
}

/// The open cover (M5): a window's backdrop with the loader mark rising
/// in it, under egui's surface. While the window's first frame isn't on
/// screen yet the surface is empty and this shows instead of a grey
/// window; the first frame, drawn on a solid backdrop, covers it. The
/// backdrop is there at once; the mark fades in 150 ms after, so a window
/// that draws in time never shows it. Core Animation runs it, so it keeps moving while a paint holds the
/// main thread. Returns false when the window has no view yet.
pub fn cover(id: egui::ViewportId, size: Vec2, radius: f32) -> bool {
    use crate::kit::glass;
    let look = native::CoverLook {
        stops: glass::backdrop_stops(glass::theme()),
        sky: glass::theme() == glass::Theme::Miami,
        glow: glass::backdrop_glow(size, glass::WINDOW_BEAM),
        edge: glass::backdrop_edge(),
        track: glass::white(26),
        lime: glass::ACTION(),
        radius,
        mark: COVER_MARK,
    };
    // CMDMEOW_OPEN_COVER=0: no cover (to compare against)
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let on = *ON.get_or_init(|| std::env::var_os("CMDMEOW_OPEN_COVER").is_none_or(|v| v != "0"));
    on && native_wanted() && native::cover(id, &look)
}

/// The loader mark's side in the open cover.
const COVER_MARK: f32 = 56.0;

/// Drop window `id`'s open cover, if it has one.
pub fn uncover(id: egui::ViewportId) {
    native::uncover(id);
}

/// Seconds on Core Animation's clock (so native and egui drawing agree).
fn clock() -> f64 {
    native::media_time()
}

#[cfg(target_os = "macos")]
pub(crate) mod native {
    use super::{Look, MarkState, Turn};
    use egui::{Color32, Pos2, Rect};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use std::ffi::c_void;

    #[link(name = "QuartzCore", kind = "framework")]
    extern "C" {
        fn CACurrentMediaTime() -> f64;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPathCreateMutable() -> *mut c_void;
        fn CGPathMoveToPoint(path: *mut c_void, m: *const c_void, x: f64, y: f64);
        fn CGPathAddLineToPoint(path: *mut c_void, m: *const c_void, x: f64, y: f64);
        fn CGPathAddCurveToPoint(path: *mut c_void, m: *const c_void, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64);
        fn CGPathAddQuadCurveToPoint(path: *mut c_void, m: *const c_void, cx: f64, cy: f64, x: f64, y: f64);
        fn CGPathAddArc(path: *mut c_void, m: *const c_void, x: f64, y: f64, radius: f64, start: f64, end: f64, clockwise: bool);
        fn CGPathCloseSubpath(path: *mut c_void);
        fn CGPathAddEllipseInRect(path: *mut c_void, m: *const c_void, rect: NSRect);
        fn CGPathRelease(path: *mut c_void);
        fn CGPathCreateCopyByStrokingPath(path: *const c_void, m: *const c_void, width: f64, cap: i32, join: i32, miter: f64) -> *mut c_void;
        fn CGPathCreateCopyBySubtractingPath(path: *const c_void, cut: *const c_void, even_odd: bool) -> *mut c_void;
    }

    pub fn media_time() -> f64 {
        unsafe { CACurrentMediaTime() }
    }

    /// One key's layers in one window: blades and hub, the arc, or the
    /// bar's track and fill.
    struct Layers {
        viewport: egui::ViewportId,
        key: String,
        /// The view's layer they hang from (a new window means new layers).
        root: *const AnyObject,
        container: Retained<AnyObject>,
        a: Retained<AnyObject>,
        b: Retained<AnyObject>,
        /// What was set last, to touch only what changed.
        frame: Option<(Rect, Rect)>,
        shape: Option<(u8, Rect)>,
        colors: Option<(Color32, Option<Color32>, Color32, *const AnyObject)>,
        turn: Option<Turn>,
        frac: Option<f64>,
        hidden: bool,
        /// The mark's other layers: level, wave, mask, ⌘ cut, ✓ cut.
        extra: Vec<Retained<AnyObject>>,
        /// The mark's fill and state as last set.
        mark: Option<(f32, MarkState)>,
    }

    thread_local! {
        static LAYERS: RefCell<Vec<Layers>> = const { RefCell::new(Vec::new()) };
    }

    fn ns_rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// `c` as egui shows it. egui's surface is untagged, so its bytes land on
    /// the display as they are: tag them with the window's own colour space
    /// (Display P3 when unknown), not sRGB, or the native layers look paler.
    pub(crate) fn ns_color(c: Color32, space: Option<&AnyObject>) -> Retained<AnyObject> {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let comps = [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, a as f64 / 255.0];
        unsafe {
            match space {
                Some(s) => msg_send![class!(NSColor), colorWithColorSpace: s, components: comps.as_ptr(), count: 4isize],
                None => msg_send![class!(NSColor), colorWithDisplayP3Red: comps[0], green: comps[1], blue: comps[2], alpha: comps[3]],
            }
        }
    }

    /// Set `layer`'s fill or stroke colour to `c` (None: no fill).
    pub(crate) fn set_color(layer: &AnyObject, stroke: bool, c: Option<Color32>, space: Option<&AnyObject>) {
        // the NSColor owns the CGColor: keep it alive across the set
        let ns = c.map(|c| ns_color(c, space));
        unsafe {
            let cg: *const c_void = match &ns {
                Some(ns) => msg_send![&**ns, CGColor],
                None => std::ptr::null(),
            };
            if stroke {
                let _: () = msg_send![layer, setStrokeColor: cg];
            } else {
                let _: () = msg_send![layer, setFillColor: cg];
            }
        }
    }

    pub(crate) fn set_path(layer: &AnyObject, path: *mut c_void) {
        unsafe {
            let _: () = msg_send![layer, setPath: path as *const c_void];
            CGPathRelease(path);
        }
    }

    /// The fan glyph in a square of side 2r, centre (r, r), y down (fill it even-odd).
    pub(crate) fn blades_path(r: f32) -> *mut c_void {
        let at = |q: [f32; 2]| ((r + q[0] * r) as f64, (r + q[1] * r) as f64);
        unsafe {
            let path = CGPathCreateMutable();
            for o in super::GLYPH {
                let (x, y) = at(o[0]);
                CGPathMoveToPoint(path, std::ptr::null(), x, y);
                for seg in o[1..].chunks_exact(3) {
                    let ((x1, y1), (x2, y2), (x, y)) = (at(seg[0]), at(seg[1]), at(seg[2]));
                    CGPathAddCurveToPoint(path, std::ptr::null(), x1, y1, x2, y2, x, y);
                }
                CGPathCloseSubpath(path);
            }
            path
        }
    }

    /// The arc from angle 0, in a square of side 2r, y down (the way
    /// angles grow in egui).
    fn arc_path(r: f32) -> *mut c_void {
        unsafe {
            let path = CGPathCreateMutable();
            CGPathAddArc(path, std::ptr::null(), r as f64, r as f64, r as f64, 0.0, super::ARC_SPAN as f64, false);
            path
        }
    }

    /// A line through the middle of a `w`×`h` bar, half a height in from
    /// each end: stroked `h` wide with round caps, it is the bar.
    fn bar_path(w: f32, h: f32) -> *mut c_void {
        let y = (h / 2.0) as f64;
        unsafe {
            let path = CGPathCreateMutable();
            CGPathMoveToPoint(path, std::ptr::null(), y, y);
            CGPathAddLineToPoint(path, std::ptr::null(), (w as f64 - y).max(y + 0.01), y);
            path
        }
    }

    pub(crate) fn new_layer(class_name: &str, scale: f64) -> Retained<AnyObject> {
        let l: Retained<AnyObject> = unsafe {
            match class_name {
                "shape" => msg_send![class!(CAShapeLayer), layer],
                _ => msg_send![class!(CALayer), layer],
            }
        };
        unsafe {
            let _: () = msg_send![&*l, setContentsScale: scale];
        }
        l
    }

    /// The view's root layer for viewport `id`, its window's scale and colour space.
    pub(crate) fn root_of(id: egui::ViewportId) -> Option<(Retained<AnyObject>, f64, Option<Retained<AnyObject>>)> {
        let view = eframe::viewport_ns_view(id)? as *const AnyObject;
        unsafe {
            let view: &AnyObject = view.as_ref()?;
            let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
            let window: Option<Retained<AnyObject>> = msg_send![view, window];
            let scale: f64 = match &window {
                Some(w) => msg_send![&**w, backingScaleFactor],
                None => 2.0,
            };
            let space: Option<Retained<AnyObject>> = match &window {
                Some(w) => msg_send![&**w, colorSpace],
                None => None,
            };
            Some((layer?, scale, space))
        }
    }

    /// Hide `key`'s layer in viewport `id`, if it has one (and say so).
    pub fn hide(id: egui::ViewportId, key: &str) -> bool {
        LAYERS.with(|all| {
            let mut all = all.borrow_mut();
            let Some(b) = all.iter_mut().find(|b| b.viewport == id && b.key == key) else { return false };
            if !b.hidden {
                unsafe {
                    let _: () = msg_send![class!(CATransaction), begin];
                    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                    let _: () = msg_send![&*b.container, setHidden: true];
                    let _: () = msg_send![class!(CATransaction), commit];
                }
                b.hidden = true;
            }
            // shown again only once it held still for a pass
            b.frame = None;
            b.frac = None;
            true
        })
    }

    fn kind(look: Look) -> u8 {
        match look {
            Look::Blades => 0,
            Look::Arc => 1,
            Look::Bar { .. } => 2,
            Look::Mark { .. } => 3,
        }
    }

    /// Show `key`'s layers in viewport `id` (`clip` and `rect` in egui
    /// points): Some(true) when they're on screen, Some(false) while a fresh
    /// or moved layer waits a pass, None if the window can't be reached.
    pub fn show(id: egui::ViewportId, key: &str, clip: Rect, rect: Rect, color: Color32, look: Look, turn: Turn) -> Option<bool> {
        let (root, scale, space) = root_of(id)?;
        let space_ptr: *const AnyObject = space.as_deref().map_or(std::ptr::null(), |s| s as *const AnyObject);
        let root_ptr: *const AnyObject = &*root;
        LAYERS.with(|all| {
            let mut all = all.borrow_mut();
            // layers of a window that's gone
            all.retain(|b| b.viewport != id || b.root == root_ptr);
            let i = match all.iter().position(|b| b.viewport == id && b.key == key) {
                Some(i) => i,
                None => {
                    let container = new_layer("plain", scale);
                    let a = new_layer("shape", scale);
                    let b = new_layer("shape", scale);
                    unsafe {
                        let _: () = msg_send![&*container, setName: &*NSString::from_str(&format!("cmdmeow-spin-{key}"))];
                        let _: () = msg_send![&*container, setMasksToBounds: true];
                        let _: () = msg_send![&*container, addSublayer: &*a];
                        let _: () = msg_send![&*container, addSublayer: &*b];
                        let _: () = msg_send![&*root, addSublayer: &*container];
                    }
                    all.push(Layers {
                        viewport: id,
                        key: key.to_string(),
                        root: root_ptr,
                        container,
                        a,
                        b,
                        frame: None,
                        shape: None,
                        colors: None,
                        turn: None,
                        frac: None,
                        hidden: false,
                        extra: Vec::new(),
                        mark: None,
                    });
                    all.len() - 1
                }
            };
            let l = &mut all[i];
            // a fresh layer or one that moved shows next pass, once it holds
            // still (scrolling, sliding): egui draws it this pass
            let frame = (clip, rect);
            let moved = l.frame != Some(frame);
            let k = kind(look);
            let size = Rect::from_min_size(Pos2::ZERO, rect.size());
            let (second, edge) = match look {
                Look::Blades => (None, Color32::TRANSPARENT),
                Look::Arc => (None, Color32::TRANSPARENT),
                Look::Bar { .. } => (None, super::track()),
                Look::Mark { track, bg, .. } => (Some(bg), track),
            };
            let settled = match look {
                Look::Bar { frac } => l.frac == Some(super::stroke_end(rect.width(), rect.height(), frac)),
                Look::Mark { frac, state, .. } => l.mark == Some((frac, state)),
                _ => l.turn == Some(turn),
            };
            // nothing to change: no transaction at all (this runs every pass,
            // and a commit costs more than the rest of a quiet pass)
            if !moved && !l.hidden && settled && l.shape == Some((k, size)) && l.colors == Some((color, second, edge, space_ptr)) {
                return Some(true);
            }
            unsafe {
                let _: () = msg_send![class!(CATransaction), begin];
                let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                if moved {
                    let flipped: bool = msg_send![&*root, contentsAreFlipped];
                    let bounds: NSRect = msg_send![&*root, bounds];
                    // the container's own space runs y-down like egui's
                    let y = if flipped { clip.min.y as f64 } else { bounds.size.height - clip.max.y as f64 };
                    let _: () = msg_send![&*l.container, setGeometryFlipped: !flipped];
                    // stay put against the top right when the window grows or
                    // shrinks (the panel widens to the left for a sheet and
                    // grows down) until the next pass places it again
                    const MIN_X: u32 = 1 << 0;
                    const MIN_Y: u32 = 1 << 3;
                    const MAX_Y: u32 = 1 << 5;
                    let _: () = msg_send![&*l.container, setAutoresizingMask: MIN_X | if flipped { MAX_Y } else { MIN_Y }];
                    let _: () = msg_send![&*l.container, setFrame: ns_rect(clip.min.x as f64, y, clip.width() as f64, clip.height() as f64)];
                    // bounds + position, never frame: frame on a turned layer
                    // is its rotated bounding box and drifts off the dial
                    let mid = NSPoint::new((rect.center().x - clip.min.x) as f64, (rect.center().y - clip.min.y) as f64);
                    for s in [&l.a, &l.b] {
                        let _: () = msg_send![&**s, setBounds: ns_rect(0.0, 0.0, rect.width() as f64, rect.height() as f64)];
                        let _: () = msg_send![&**s, setPosition: mid];
                    }
                    if let Look::Mark { .. } = look {
                        mark_layers(l, scale);
                        place_mark(l, rect, mid);
                    }
                    l.frame = Some(frame);
                }
                if l.shape != Some((k, size)) {
                    let (w, h, r) = (rect.width(), rect.height(), rect.width() / 2.0);
                    match look {
                        Look::Blades => {
                            set_path(&l.a, blades_path(r));
                            let _: () = msg_send![&*l.a, setFillRule: &*NSString::from_str("even-odd")];
                            let _: () = msg_send![&*l.b, setHidden: true];
                        }
                        Look::Arc => {
                            set_path(&l.a, arc_path(r));
                            let _: () = msg_send![&*l.a, setLineWidth: super::ARC_WIDTH as f64];
                            let _: () = msg_send![&*l.a, setLineCap: &*NSString::from_str("round")];
                            let _: () = msg_send![&*l.b, setHidden: true];
                        }
                        Look::Mark { .. } => {
                            mark_layers(l, scale);
                            mark_paths(l, w);
                            let _: () = msg_send![&*l.b, setHidden: false];
                            l.mark = None;
                        }
                        Look::Bar { .. } => {
                            for s in [&l.a, &l.b] {
                                set_path(s, bar_path(w, h));
                                let _: () = msg_send![&**s, setLineWidth: h as f64];
                                let _: () = msg_send![&**s, setLineCap: &*NSString::from_str("round")];
                            }
                            let _: () = msg_send![&*l.b, setHidden: false];
                            l.frac = None;
                        }
                    }
                    l.shape = Some((k, size));
                    l.colors = None;
                }
                // a window moved to another screen gets new colours too
                if l.colors != Some((color, second, edge, space_ptr)) {
                    let sp = space.as_deref();
                    match look {
                        Look::Blades => {
                            set_color(&l.a, false, Some(color), sp);
                            set_color(&l.a, true, None, sp);
                        }
                        Look::Arc => {
                            set_color(&l.a, false, None, sp);
                            set_color(&l.a, true, Some(color), sp);
                        }
                        Look::Mark { bg, track, .. } => {
                            set_color(&l.a, false, Some(track), sp);
                            set_color(&l.b, false, None, sp);
                            // extra: [level, wave, mask, ⌘, ✓]
                            set_color(&l.extra[1], false, Some(color), sp);
                            set_color(&l.extra[2], false, Some(Color32::BLACK), sp);
                            for c in [&l.extra[3], &l.extra[4]] {
                                set_color(c, false, None, sp);
                                set_color(c, true, Some(bg), sp);
                            }
                        }
                        Look::Bar { .. } => {
                            set_color(&l.a, false, None, sp);
                            set_color(&l.a, true, Some(edge), sp);
                            set_color(&l.b, false, None, sp);
                            set_color(&l.b, true, Some(color), sp);
                        }
                    }
                    l.colors = Some((color, second, edge, space_ptr));
                }
                match look {
                    Look::Mark { frac, state, .. } => {
                        if l.mark != Some((frac, state)) {
                            set_mark(l, rect.width(), frac, state, l.mark.filter(|_| !moved));
                            l.mark = Some((frac, state));
                        }
                    }
                    Look::Bar { frac } => {
                        let end = super::stroke_end(rect.width(), rect.height(), frac);
                        if l.frac != Some(end) {
                            // a new value eases in; the first one is just set
                            let ease = l.frac.is_some() && !moved;
                            if ease {
                                let _: () = msg_send![class!(CATransaction), begin];
                                let _: () = msg_send![class!(CATransaction), setDisableActions: false];
                                let _: () = msg_send![class!(CATransaction), setAnimationDuration: 0.3f64];
                            }
                            let _: () = msg_send![&*l.b, setStrokeEnd: end];
                            if ease {
                                let _: () = msg_send![class!(CATransaction), commit];
                            }
                            l.frac = Some(end);
                        }
                    }
                    _ => {
                        if l.turn != Some(turn) {
                            set_turn(&l.a, turn);
                            l.turn = Some(turn);
                        }
                    }
                }
                let hide = moved;
                if hide != l.hidden {
                    let _: () = msg_send![&*l.container, setHidden: hide];
                    l.hidden = hide;
                }
                let _: () = msg_send![class!(CATransaction), commit];
            }
            Some(!moved)
        })
    }

    /// The mark's own layers, made once: the fill holder (`b`) gets a mask
    /// of the keycap and a level layer holding the wave; the ⌘ and ✓ cuts
    /// sit on top. `a` is the keycap's track.
    unsafe fn mark_layers(l: &mut Layers, scale: f64) {
        if !l.extra.is_empty() {
            return;
        }
        let level = new_layer("plain", scale);
        let wave = new_layer("shape", scale);
        let mask = new_layer("shape", scale);
        let cmd = new_layer("shape", scale);
        let chk = new_layer("shape", scale);
        let zero = NSPoint::new(0.0, 0.0);
        let _: () = msg_send![&*level, setAnchorPoint: zero];
        let _: () = msg_send![&*wave, setAnchorPoint: zero];
        let _: () = msg_send![&*level, addSublayer: &*wave];
        let _: () = msg_send![&*l.b, addSublayer: &*level];
        let _: () = msg_send![&*l.b, setMask: &*mask];
        let _: () = msg_send![&*l.container, addSublayer: &*cmd];
        let _: () = msg_send![&*l.container, addSublayer: &*chk];
        let _: () = msg_send![&*chk, setOpacity: 0.0f32];
        l.extra = vec![level, wave, mask, cmd, chk];
    }

    /// Size and place the mark's extra layers like `a` and `b`.
    unsafe fn place_mark(l: &Layers, rect: Rect, mid: NSPoint) {
        let (w, h) = (rect.width() as f64, rect.height() as f64);
        let b = ns_rect(0.0, 0.0, w, h);
        for s in [&l.extra[2], &l.extra[3], &l.extra[4]] {
            let _: () = msg_send![&**s, setBounds: b];
        }
        let _: () = msg_send![&*l.extra[2], setPosition: NSPoint::new(w / 2.0, h / 2.0)];
        for s in [&l.extra[3], &l.extra[4]] {
            let _: () = msg_send![&**s, setPosition: mid];
        }
        let _: () = msg_send![&*l.extra[0], setBounds: b];
        let _: () = msg_send![&*l.extra[1], setBounds: b];
        let _: () = msg_send![&*l.extra[1], setPosition: NSPoint::new(0.0, 0.0)];
    }

    /// The keycap outline in a `w` square (y down).
    fn outline_path(w: f32) -> *mut c_void {
        use crate::kit::brand::{Seg, NUDGE, OUTLINE};
        let s = w as f64 / 1024.0;
        let x = |v: f32| (v + NUDGE.0) as f64 * s;
        let y = |v: f32| (v + NUDGE.1) as f64 * s;
        unsafe {
            let path = CGPathCreateMutable();
            for seg in OUTLINE {
                match *seg {
                    Seg::M(a, b) => CGPathMoveToPoint(path, std::ptr::null(), x(a), y(b)),
                    Seg::L(a, b) => CGPathAddLineToPoint(path, std::ptr::null(), x(a), y(b)),
                    Seg::C(a, b, c, d, e, f) => CGPathAddCurveToPoint(path, std::ptr::null(), x(a), y(b), x(c), y(d), x(e), y(f)),
                }
            }
            CGPathCloseSubpath(path);
            path
        }
    }

    /// The ⌘'s centerline in a `w` square.
    fn command_path(w: f32) -> *mut c_void {
        use crate::kit::brand::{CMD_BARS, CMD_LOOPS, NUDGE};
        let s = w as f64 / 1024.0;
        let x = |v: f32| (v + NUDGE.0) as f64 * s;
        let y = |v: f32| (v + NUDGE.1) as f64 * s;
        unsafe {
            let path = CGPathCreateMutable();
            for (cx, cy, r, a0, a1) in CMD_LOOPS {
                CGPathMoveToPoint(path, std::ptr::null(), x(cx + r * a0.cos()), y(cy + r * a0.sin()));
                // in this y-down space "not clockwise" is the way angles grow
                CGPathAddArc(path, std::ptr::null(), x(cx), y(cy), r as f64 * s, a0 as f64, a1 as f64, a1 < a0);
            }
            for (x0, y0, x1, y1) in CMD_BARS {
                CGPathMoveToPoint(path, std::ptr::null(), x(x0), y(y0));
                CGPathAddLineToPoint(path, std::ptr::null(), x(x1), y(y1));
            }
            path
        }
    }

    /// The ✓ in a `w` square.
    fn check_path(w: f32) -> *mut c_void {
        use crate::kit::brand::{CHECK, NUDGE};
        let s = w as f64 / 1024.0;
        unsafe {
            let path = CGPathCreateMutable();
            for (i, (a, b)) in CHECK.iter().enumerate() {
                let (px, py) = ((a + NUDGE.0) as f64 * s, (b + NUDGE.1) as f64 * s);
                if i == 0 {
                    CGPathMoveToPoint(path, std::ptr::null(), px, py);
                } else {
                    CGPathAddLineToPoint(path, std::ptr::null(), px, py);
                }
            }
            path
        }
    }

    /// The fill's top: a wave from -w to 2w along y = 0, filled down to 2w.
    /// Sliding it left by one wave length repeats seamlessly.
    fn wave_path(w: f32) -> *mut c_void {
        use crate::kit::loader::{WAVE_A, WAVE_P};
        let s = w as f64 / 1024.0;
        let (half, a) = (WAVE_P as f64 / 2.0 * s, WAVE_A as f64 * s);
        let (x0, x1) = (-(w as f64), 2.0 * w as f64);
        unsafe {
            let path = CGPathCreateMutable();
            CGPathMoveToPoint(path, std::ptr::null(), x0, 0.0);
            let mut x = x0;
            let mut up = true;
            while x < x1 {
                CGPathAddQuadCurveToPoint(path, std::ptr::null(), x + half / 2.0, if up { -a * 2.0 } else { a * 2.0 }, x + half, 0.0);
                x += half;
                up = !up;
            }
            CGPathAddLineToPoint(path, std::ptr::null(), x, 2.0 * w as f64);
            CGPathAddLineToPoint(path, std::ptr::null(), x0, 2.0 * w as f64);
            CGPathCloseSubpath(path);
            path
        }
    }

    /// Paths of the mark's layers for a `w` square.
    unsafe fn mark_paths(l: &Layers, w: f32) {
        set_path(&l.a, outline_path(w));
        set_path(&l.extra[2], outline_path(w));
        set_path(&l.extra[1], wave_path(w));
        let s = w as f64 / 1024.0;
        set_path(&l.extra[3], command_path(w));
        let _: () = msg_send![&*l.extra[3], setLineWidth: crate::kit::brand::CMD_WIDTH as f64 * s];
        let _: () = msg_send![&*l.extra[3], setLineCap: &*NSString::from_str("butt")];
        set_path(&l.extra[4], check_path(w));
        let _: () = msg_send![&*l.extra[4], setLineWidth: crate::kit::brand::CHECK_WIDTH as f64 * s];
        let _: () = msg_send![&*l.extra[4], setLineCap: &*NSString::from_str("round")];
        let _: () = msg_send![&*l.extra[4], setLineJoin: &*NSString::from_str("round")];
        // the wave runs for as long as the layer lives
        let x = NSString::from_str("position.x");
        let anim: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*x];
        let _: () = msg_send![&*anim, setFromValue: &*NSNumber::new_f64(0.0)];
        let _: () = msg_send![&*anim, setToValue: &*NSNumber::new_f64(-(crate::kit::loader::WAVE_P as f64) * s)];
        let _: () = msg_send![&*anim, setDuration: 1.6f64];
        let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
        let _: () = msg_send![&*l.extra[1], addAnimation: &*anim, forKey: &*NSString::from_str("cmdmeow-wave")];
    }

    /// Move the mark to `frac`/`state` from `was` (None: just set it).
    unsafe fn set_mark(l: &Layers, w: f32, frac: f32, state: MarkState, was: Option<(f32, MarkState)>) {
        let s = w as f64 / 1024.0;
        let y = |f: f32| (crate::kit::loader::level(f) + crate::kit::brand::NUDGE.1) as f64 * s;
        let level = &l.extra[0];
        let sweep = NSString::from_str("cmdmeow-sweep");
        let _: () = msg_send![&**level, removeAnimationForKey: &*sweep];
        let target = match state {
            MarkState::Filling => frac,
            MarkState::Sweeping => 0.0,
            MarkState::Done => 1.06,
        };
        let ease = was.is_some();
        if ease {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: false];
            let _: () = msg_send![class!(CATransaction), setAnimationDuration: 0.6f64];
        }
        let _: () = msg_send![&**level, setPosition: NSPoint::new(0.0, y(target))];
        if ease {
            let _: () = msg_send![class!(CATransaction), commit];
        }
        if state == MarkState::Sweeping {
            let key = NSString::from_str("position.y");
            let anim: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key];
            let _: () = msg_send![&*anim, setFromValue: &*NSNumber::new_f64(y(0.0))];
            let _: () = msg_send![&*anim, setToValue: &*NSNumber::new_f64(y(1.06))];
            let _: () = msg_send![&*anim, setDuration: 1.4f64];
            let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
            let tf: Retained<AnyObject> = msg_send![class!(CAMediaTimingFunction), functionWithName: &*NSString::from_str("easeInEaseOut")];
            let _: () = msg_send![&*anim, setTimingFunction: &*tf];
            let _: () = msg_send![&**level, addAnimation: &*anim, forKey: &*sweep];
        }
        // ⌘ → ✓, and one hop, when it turns done
        let done = state == MarkState::Done;
        let was_done = was.is_some_and(|(_, st)| st == MarkState::Done);
        if done != was_done || was.is_none() {
            if ease {
                let _: () = msg_send![class!(CATransaction), begin];
                let _: () = msg_send![class!(CATransaction), setDisableActions: false];
                let _: () = msg_send![class!(CATransaction), setAnimationDuration: 0.25f64];
            }
            let _: () = msg_send![&*l.extra[3], setOpacity: if done { 0.0f32 } else { 1.0f32 }];
            let _: () = msg_send![&*l.extra[4], setOpacity: if done { 1.0f32 } else { 0.0f32 }];
            if ease {
                let _: () = msg_send![class!(CATransaction), commit];
            }
            if done && ease {
                for (path, vals) in [("transform.scale.x", [1.0, 1.08, 0.97, 1.0]), ("transform.scale.y", [1.0, 0.94, 1.04, 1.0])] {
                    let nums: Vec<Retained<NSNumber>> = vals.iter().map(|v| NSNumber::new_f64(*v)).collect();
                    let arr = objc2_foundation::NSArray::from_retained_slice(&nums);
                    for layer in [&l.a, &l.b, &l.extra[3], &l.extra[4]] {
                        let anim: Retained<AnyObject> = msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: &*NSString::from_str(path)];
                        let _: () = msg_send![&*anim, setValues: &*arr];
                        let _: () = msg_send![&*anim, setDuration: 0.46f64];
                        let _: () = msg_send![&**layer, addAnimation: &*anim, forKey: &*NSString::from_str(path)];
                    }
                }
            }
        }
    }

    /// Point the layer along `turn`: a still angle, plus a repeating full
    /// turn that started at `turn.t0` while it turns.
    unsafe fn set_turn(layer: &AnyObject, turn: Turn) {
        let key = NSString::from_str("transform.rotation.z");
        let anim_key = NSString::from_str("cmdmeow-turn");
        let _: () = msg_send![layer, removeAnimationForKey: &*anim_key];
        let _: () = msg_send![layer, setValue: &*NSNumber::new_f64(turn.angle0), forKeyPath: &*key];
        if turn.tps <= 0.0 {
            return;
        }
        let anim: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key];
        let _: () = msg_send![&*anim, setFromValue: &*NSNumber::new_f64(turn.angle0)];
        let _: () = msg_send![&*anim, setToValue: &*NSNumber::new_f64(turn.angle0 + std::f64::consts::TAU)];
        let _: () = msg_send![&*anim, setDuration: 1.0 / turn.tps];
        let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
        let begin: f64 = msg_send![layer, convertTime: turn.t0, fromLayer: Option::<&AnyObject>::None];
        let _: () = msg_send![&*anim, setBeginTime: begin];
        let _: () = msg_send![layer, addAnimation: &*anim, forKey: &*anim_key];
    }

    /// What the open cover looks like (see `super::cover`).
    pub struct CoverLook {
        pub stops: &'static [(f32, Color32)],
        /// Miami (TH6): the stops run top to bottom, not out from the glow.
        pub sky: bool,
        pub glow: (f32, f32),
        pub edge: Color32,
        pub track: Color32,
        pub lime: Color32,
        pub radius: f32,
        pub mark: f32,
    }

    thread_local! {
        static COVERS: RefCell<Vec<(egui::ViewportId, Retained<AnyObject>)>> = const { RefCell::new(Vec::new()) };
    }

    /// The keycap with the ⌘ cut out, in a `w` square: the mark's shape.
    fn cut_mark_path(w: f32) -> *mut c_void {
        let s = w as f64 / 1024.0;
        unsafe {
            let outline = outline_path(w);
            let cmd = command_path(w);
            // butt caps, miter joins: as the ⌘ is stroked everywhere else
            let stroke = CGPathCreateCopyByStrokingPath(cmd, std::ptr::null(), crate::kit::brand::CMD_WIDTH as f64 * s, 0, 0, 10.0);
            let cut = CGPathCreateCopyBySubtractingPath(outline, stroke, false);
            CGPathRelease(outline);
            CGPathRelease(cmd);
            CGPathRelease(stroke);
            cut
        }
    }

    pub fn cover(id: egui::ViewportId, look: &CoverLook) -> bool {
        let Some((root, scale, space)) = root_of(id) else { return false };
        uncover(id);
        let sp = space.as_deref();
        // the NSColors own their CGColors: keep them until the commit
        let mut keep: Vec<Retained<AnyObject>> = Vec::new();
        let mut cg = |c: Color32| -> *const c_void {
            let ns = ns_color(c, sp);
            let p: *const c_void = unsafe { msg_send![&*ns, CGColor] };
            keep.push(ns);
            p
        };
        let num = |v: f64| NSNumber::new_f64(v);
        let key = |k: &str| NSString::from_str(k);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let flipped: bool = msg_send![&*root, contentsAreFlipped];
            let bounds: NSRect = msg_send![&*root, bounds];
            let (w, h) = (bounds.size.width.max(1.0), bounds.size.height.max(1.0));
            const W_SIZABLE: u32 = 1 << 1;
            const H_SIZABLE: u32 = 1 << 4;
            const MARGINS: u32 = 1 | (1 << 2) | (1 << 3) | (1 << 5);

            // the window: rounded, its 1 pt edge on top, y down like egui
            let c = new_layer("plain", scale);
            let _: () = msg_send![&*c, setName: &*key("cmdmeow-open-cover")];
            let _: () = msg_send![&*c, setGeometryFlipped: !flipped];
            let _: () = msg_send![&*c, setFrame: ns_rect(0.0, 0.0, w, h)];
            let _: () = msg_send![&*c, setAutoresizingMask: W_SIZABLE | H_SIZABLE];
            let _: () = msg_send![&*c, setCornerRadius: look.radius as f64];
            let _: () = msg_send![&*c, setMasksToBounds: true];
            let _: () = msg_send![&*c, setBorderWidth: 1.0f64];
            let _: () = msg_send![&*c, setBorderColor: cg(look.edge)];

            // the backdrop (S1): a radial glow from the top middle, or
            // Miami's sky from top to bottom (TH6)
            let g: Retained<AnyObject> = msg_send![class!(CAGradientLayer), layer];
            let _: () = msg_send![&*g, setContentsScale: scale];
            let _: () = msg_send![&*g, setType: &*key(if look.sky { "axial" } else { "radial" })];
            let colors: Retained<AnyObject> = msg_send![class!(NSMutableArray), array];
            let stops: Retained<AnyObject> = msg_send![class!(NSMutableArray), array];
            for (t, col) in look.stops {
                let _: () = msg_send![&*colors, addObject: cg(*col) as *const AnyObject];
                let _: () = msg_send![&*stops, addObject: &*num(*t as f64)];
            }
            let _: () = msg_send![&*g, setColors: &*colors];
            let _: () = msg_send![&*g, setLocations: &*stops];
            let _: () = msg_send![&*g, setStartPoint: NSPoint::new(0.5, 0.0)];
            let end = if look.sky { NSPoint::new(0.5, 1.0) } else { NSPoint::new(0.5 + look.glow.0 as f64 / w, look.glow.1 as f64 / h) };
            let _: () = msg_send![&*g, setEndPoint: end];
            let _: () = msg_send![&*g, setFrame: ns_rect(0.0, 0.0, w, h)];
            let _: () = msg_send![&*g, setAutoresizingMask: W_SIZABLE | H_SIZABLE];
            let _: () = msg_send![&*c, addSublayer: &*g];

            // the mark (P6), centred: the keycap in the track colour, lime
            // rising inside it, the ⌘ cut out of both
            let m = look.mark;
            let side = m as f64;
            let s = side / 1024.0;
            let box_ = ns_rect(0.0, 0.0, side, side);
            let zero = NSPoint::new(0.0, 0.0);
            let mark = new_layer("plain", scale);
            let _: () = msg_send![&*mark, setBounds: box_];
            let _: () = msg_send![&*mark, setPosition: NSPoint::new((w / 2.0).round(), (h / 2.0).round())];
            let _: () = msg_send![&*mark, setAutoresizingMask: MARGINS];
            let track = new_layer("plain", scale);
            let _: () = msg_send![&*track, setFrame: box_];
            let _: () = msg_send![&*track, setBackgroundColor: cg(look.track)];
            let level = new_layer("plain", scale);
            let wave = new_layer("shape", scale);
            for l in [&level, &wave] {
                let _: () = msg_send![&**l, setAnchorPoint: zero];
                let _: () = msg_send![&**l, setBounds: box_];
            }
            let y = |v: f32| (v + crate::kit::brand::NUDGE.1) as f64 * s;
            let (low, high) = (y(crate::kit::loader::level(0.0)), y(-20.0));
            let _: () = msg_send![&*level, setPosition: NSPoint::new(0.0, low)];
            let _: () = msg_send![&*wave, setPosition: zero];
            set_path(&wave, wave_path(m));
            set_color(&wave, false, Some(look.lime), sp);
            let _: () = msg_send![&*level, addSublayer: &*wave];
            let _: () = msg_send![&*mark, addSublayer: &*track];
            let _: () = msg_send![&*mark, addSublayer: &*level];
            let cut = new_layer("shape", scale);
            let _: () = msg_send![&*cut, setFrame: box_];
            set_path(&cut, cut_mark_path(m));
            set_color(&cut, false, Some(Color32::BLACK), sp);
            let _: () = msg_send![&*mark, setMask: &*cut];
            let _: () = msg_send![&*c, addSublayer: &*mark];

            // the wave runs; the lime rises over 80% of 1.6 s, holds, again
            let anim: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key("position.x")];
            let _: () = msg_send![&*anim, setFromValue: &*num(0.0)];
            let _: () = msg_send![&*anim, setToValue: &*num(-(crate::kit::loader::WAVE_P as f64) * s)];
            let _: () = msg_send![&*anim, setDuration: 1.6f64];
            let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
            let _: () = msg_send![&*wave, addAnimation: &*anim, forKey: &*key("cmdmeow-wave")];
            let rise: Retained<AnyObject> = msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: &*key("position.y")];
            let vals = objc2_foundation::NSArray::from_retained_slice(&[num(low), num(high), num(high)]);
            let times = objc2_foundation::NSArray::from_retained_slice(&[num(0.0), num(0.8), num(1.0)]);
            let ease: Retained<AnyObject> = msg_send![class!(CAMediaTimingFunction), functionWithName: &*key("easeInEaseOut")];
            let linear: Retained<AnyObject> = msg_send![class!(CAMediaTimingFunction), functionWithName: &*key("linear")];
            let fns = objc2_foundation::NSArray::from_retained_slice(&[ease, linear]);
            let _: () = msg_send![&*rise, setValues: &*vals];
            let _: () = msg_send![&*rise, setKeyTimes: &*times];
            let _: () = msg_send![&*rise, setTimingFunctions: &*fns];
            let _: () = msg_send![&*rise, setDuration: 1.6f64];
            let _: () = msg_send![&*rise, setRepeatCount: f32::INFINITY];
            let _: () = msg_send![&*level, addAnimation: &*rise, forKey: &*key("cmdmeow-rise")];

            // the backdrop is there at once (what egui's first frame draws
            // anyway, instead of the window's empty body); the mark only
            // after 150 ms, in over 120 ms
            let fade: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key("opacity")];
            let _: () = msg_send![&*fade, setFromValue: &*num(0.0)];
            let _: () = msg_send![&*fade, setToValue: &*num(1.0)];
            let _: () = msg_send![&*fade, setBeginTime: CACurrentMediaTime() + 0.15];
            let _: () = msg_send![&*fade, setDuration: 0.12f64];
            let _: () = msg_send![&*fade, setFillMode: &*key("backwards")];
            let _: () = msg_send![&*mark, addAnimation: &*fade, forKey: &*key("cmdmeow-fade")];

            // under the Metal layer egui draws into
            let _: () = msg_send![&*root, insertSublayer: &*c, atIndex: 0u32];
            let _: () = msg_send![class!(CATransaction), commit];
            // to the window server now: the paint that follows may hold the
            // main thread for a second, and this has to be on screen by then
            let _: () = msg_send![class!(CATransaction), flush];
            COVERS.with(|all| all.borrow_mut().push((id, c)));
        }
        drop(keep);
        true
    }

    pub fn uncover(id: egui::ViewportId) {
        COVERS.with(|all| {
            all.borrow_mut().retain(|(v, c)| {
                if *v != id {
                    return true;
                }
                unsafe {
                    let _: () = msg_send![class!(CATransaction), begin];
                    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                    let _: () = msg_send![&**c, removeFromSuperlayer];
                    let _: () = msg_send![class!(CATransaction), commit];
                }
                false
            })
        });
    }

    /// Drop viewport `id`'s layers whose key wasn't shown natively this pass.
    pub fn retain(id: egui::ViewportId, keep: &[String]) {
        LAYERS.with(|all| {
            let mut all = all.borrow_mut();
            all.retain(|b| {
                let stay = b.viewport != id || keep.contains(&b.key);
                if !stay {
                    unsafe {
                        let _: () = msg_send![&*b.container, removeFromSuperlayer];
                    }
                }
                stay
            });
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{Look, Turn};
    use egui::{Color32, Rect};

    pub fn media_time() -> f64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
    }
    pub fn show(_id: egui::ViewportId, _key: &str, _clip: Rect, _rect: Rect, _color: Color32, _look: Look, _turn: Turn) -> Option<bool> {
        None
    }
    pub fn hide(_id: egui::ViewportId, _key: &str) -> bool {
        false
    }
    pub fn retain(_id: egui::ViewportId, _keep: &[String]) {}

    #[allow(dead_code)]
    pub struct CoverLook {
        pub stops: &'static [(f32, Color32)],
        /// Miami (TH6): the stops run top to bottom, not out from the glow.
        pub sky: bool,
        pub glow: (f32, f32),
        pub edge: Color32,
        pub track: Color32,
        pub lime: Color32,
        pub radius: f32,
        pub mark: f32,
    }
    pub fn cover(_id: egui::ViewportId, _look: &CoverLook) -> bool {
        false
    }
    pub fn uncover(_id: egui::ViewportId) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Shape;

    #[test]
    fn the_fan_glyph_has_blades_and_an_open_hub() {
        let px = 64u32;
        let path = glyph_skia(32.0, 31.0).expect("the glyph builds");
        let mut pm = tiny_skia::Pixmap::new(px, px).unwrap();
        let mut ink = tiny_skia::Paint::default();
        ink.set_color_rgba8(255, 255, 255, 255);
        pm.fill_path(&path, &ink, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::identity(), None);
        let alpha = |x: u32, y: u32| pm.pixel(x, y).unwrap().alpha();
        assert_eq!(alpha(32, 32), 0, "the hub is cut out");
        assert_eq!(alpha(32, 6), 255, "a blade reaches up");
        assert_eq!(alpha(32, 58), 255, "and down");
        assert_eq!(alpha(4, 4), 0, "nothing in the corners");
        // every point on the outline (not the control points) stays inside
        // the circle the glyph is sized to
        for o in GLYPH {
            for q in o.iter().step_by(3) {
                assert!((q[0] * q[0] + q[1] * q[1]).sqrt() <= 1.001, "{q:?}");
            }
        }
    }

    #[test]
    fn blades_without_native_layers_are_drawn_by_egui() {
        set_allowed(false);
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            blades(ui.painter(), "fan-test", Pos2::new(60.0, 60.0), 38.0, 1400.0, Color32::WHITE);
            flush(ui.ctx());
        });
        out.textures_delta.clear();
        set_allowed(true);
        assert!(out.shapes.iter().any(|s| matches!(s.shape, Shape::Mesh(_))), "the glyph is a textured mesh");
    }

    #[test]
    fn a_new_speed_carries_on_from_the_same_angle() {
        let t = Turn { angle0: 1.0, t0: 10.0, tps: 0.5 };
        let at = t.angle_at(12.0);
        let r = t.retimed(12.0, 0.25);
        assert!((r.angle_at(12.0) - at).abs() < 1e-9);
        assert!((r.angle_at(14.0) - (at + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)).abs() < 1e-9);
        // resting holds the angle
        let still = r.retimed(13.0, 0.0);
        assert_eq!(still.angle_at(13.0), still.angle_at(99.0));
    }

    #[test]
    fn speed_follows_rpm() {
        assert_eq!(turns_per_sec(2400.0), 1.0);
        assert_eq!(turns_per_sec(0.0), 0.0);
        assert_eq!(turns_per_sec(-5.0), 0.0);
    }

    #[test]
    fn the_native_bar_covers_what_egui_fills() {
        // a 100×6 bar: egui fills max(100·f, 6) from the left; the stroke
        // covers 6 + end·94
        let (w, h) = (100.0, 6.0);
        for f in [0.1f32, 0.25, 0.5, 0.9, 1.0] {
            let covered = h + stroke_end(w, h, f) as f32 * (w - h);
            assert!((covered - (w * f).max(h)).abs() < 0.01, "{f}");
        }
        assert_eq!(stroke_end(w, h, 0.0), 0.0, "empty is empty");
        assert!(stroke_end(w, h, 0.01) > 0.0, "a sliver still shows its dot");
    }

    #[test]
    fn a_spinner_without_native_layers_is_drawn_by_egui_and_keeps_turning() {
        set_allowed(false);
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            crate::kit::glass::spinner(ui.painter(), Pos2::new(50.0, 50.0), 8.0, 0.0, Color32::WHITE);
            flush(ui.ctx());
        });
        out.textures_delta.clear();
        set_allowed(true);
        assert!(out.shapes.iter().any(|s| matches!(s.shape, Shape::Path(_))), "the arc is drawn");
        let next = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap();
        assert!(next <= std::time::Duration::from_millis(40), "and asks for the next frame: {next:?}");
    }

    #[test]
    fn spinners_from_one_line_get_one_key_each_and_keep_it() {
        let ctx = egui::Context::default();
        let keys = |ctx: &egui::Context| {
            let mut keys = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                for i in 0..2 {
                    crate::kit::glass::spinner(ui.painter(), Pos2::new(20.0 + 30.0 * i as f32, 20.0), 6.0, 0.0, Color32::WHITE);
                }
                keys = PENDING.with(|q| q.borrow().iter().map(|(_, r)| r.key.clone()).collect());
                drop_unflushed();
            });
            out.textures_delta.clear();
            keys
        };
        let first = keys(&ctx);
        assert_eq!(first.len(), 2);
        assert_ne!(first[0], first[1], "two spinners, two layers");
        assert_eq!(keys(&ctx), first, "the next pass reuses them");
    }

    #[test]
    fn a_popup_over_the_blades_hands_them_to_egui() {
        let ctx = egui::Context::default();
        let blades = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::splat(40.0));
        let pass = |tip_at: Option<Pos2>| -> LayerId {
            let mut layer = None;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                layer = Some(ui.layer_id());
                if let Some(at) = tip_at {
                    egui::Area::new(egui::Id::NULL.with("tip")).order(egui::Order::Tooltip).fixed_pos(at).show(ui.ctx(), |ui| {
                        ui.set_min_size(Vec2::splat(30.0));
                    });
                }
            });
            out.textures_delta.clear();
            layer.unwrap()
        };
        // an area is placed after its first pass: run each case twice
        pass(None);
        let layer = pass(None);
        assert!(!covered(&ctx, layer, blades));
        pass(Some(Pos2::new(200.0, 200.0)));
        pass(Some(Pos2::new(200.0, 200.0)));
        assert!(!covered(&ctx, layer, blades), "a popup elsewhere leaves them native");
        pass(Some(Pos2::new(30.0, 30.0)));
        pass(Some(Pos2::new(30.0, 30.0)));
        assert!(covered(&ctx, layer, blades), "a popup over them hands them to egui");
    }
}
