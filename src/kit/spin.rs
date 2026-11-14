//! Fan blades and work spinners that turn without redrawing the window.
//!
//! egui redraws a whole window for any change, so blades turning at 30 fps
//! kept an open panel at ~12% CPU (each frame ~3.3 ms on an M1). On macOS
//! the blades are instead Core Animation layers over the egui surface,
//! turned by the window server: no frames of ours at all.
//!
//! A native layer always sits over everything egui draws, so it is only
//! used while it can't be told apart from egui drawing it: nothing egui
//! shows (a popover, a tooltip) overlaps it, it stays where it was last
//! frame (not scrolling or sliding), its painter isn't faded, and nothing
//! asked to [`suspend`] it (the widget editor). Otherwise egui draws the
//! same blades at the same angle, at the decorative pace.
//!
//! Widgets call [`blades`] (or `glass::spinner`, which calls [`arc`]) while
//! drawing; the shell calls [`flush`] at the end of every pass of a window
//! that may show them.

use egui::{Color32, LayerId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

struct Request {
    painter: Painter,
    key: String,
    center: Pos2,
    radius: f32,
    look: Look,
    color: Color32,
    hub: Color32,
}

/// What turns.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Look {
    /// Fan blades at the fan's speed; they rest with the decorative loops.
    Blades { rpm: f32 },
    /// A work spinner's open arc; it turns as long as it's drawn.
    Arc,
}

/// A work spinner's arc: how far it reaches, its line, its speed (as the
/// egui spinner always turned: 4 radians a second).
const ARC_SWEEP: f32 = 4.2;
const ARC_WIDTH: f32 = 2.5;
const ARC_TPS: f64 = 4.0 / std::f64::consts::TAU;

/// Where a key's blades point: `angle0` at time `t0`, turning `tps` turns a second.
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

/// Five blades around a hub at `c`, turning with the fan's `rpm`; `hub`
/// fills the middle. Drawn natively or by egui when the pass ends.
pub fn blades(p: &Painter, key: &str, c: Pos2, r: f32, rpm: f32, color: Color32, hub: Color32) {
    // CMDMEOW_SPIN_RPM=<n>: turn at n rpm whatever the fans do (to check
    // the blades on a cool Mac)
    static TEST_RPM: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let rpm = TEST_RPM.get_or_init(|| std::env::var("CMDMEOW_SPIN_RPM").ok().and_then(|v| v.parse().ok())).unwrap_or(rpm);
    let id = p.ctx().viewport_id();
    let req = Request { painter: p.clone(), key: key.to_string(), center: c, radius: r, look: Look::Blades { rpm }, color, hub };
    PENDING.with(|q| q.borrow_mut().push((id, req)));
}

/// A work spinner (an open arc of radius `r` around `c`), turning natively
/// where it can (no frames of ours, smooth even while the app is busy) and
/// drawn by egui at the animation pace where it can't.
pub fn arc(p: &Painter, c: Pos2, r: f32, color: Color32) {
    let ctx = p.ctx();
    let id = ctx.viewport_id();
    // one layer per spot: a spinner that moves gets a fresh one (egui draws
    // it meanwhile), the old one goes when it's no longer asked for
    let spot = egui::Id::NULL.with(("spin-arc", p.layer_id(), (c.x * 2.0).round() as i32, (c.y * 2.0).round() as i32, (r * 2.0).round() as i32));
    let req = Request { painter: p.clone(), key: format!("arc-{}", spot.value()), center: c, radius: r, look: Look::Arc, color, hub: Color32::TRANSPARENT };
    PENDING.with(|q| q.borrow_mut().push((id, req)));
}

/// The arc egui draws for a spinner pointing at `angle`.
pub fn draw_arc(p: &Painter, c: Pos2, r: f32, angle: f32, color: Color32) {
    p.add(egui::epaint::PathShape::line(crate::kit::glass::arc_points(c, r, angle, angle + ARC_SWEEP, 40), egui::epaint::PathStroke::new(ARC_WIDTH, color)));
}

/// Draw blades with egui this pass (the widget editor: its widgets jiggle
/// and get dragged around).
pub fn suspend(ctx: &egui::Context) {
    let id = ctx.viewport_id();
    SUSPENDED.with(|s| s.borrow_mut().push(id));
}

/// Native blades on or off for everything (off for screenshots and demo
/// views, which egui renders itself).
pub fn set_allowed(on: bool) {
    ALLOWED.with(|a| a.set(on));
}

fn native_wanted() -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ALLOWED.with(|a| a.get()) && *ENV.get_or_init(|| std::env::var("CMDMEOW_NATIVE_SPIN").map(|v| v != "0").unwrap_or(true))
}

/// The shape egui draws for blades at `angle`.
pub fn draw(p: &Painter, c: Pos2, r: f32, angle: f32, color: Color32, hub: Color32) {
    for pts in blade_outlines(c, r, angle) {
        p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
    p.circle_filled(c, r * 0.24, hub);
    crate::kit::glass::stroke_circle(p, c, r * 0.24, Stroke::new(1.0, crate::kit::glass::white(46)));
}

/// Each blade: an ellipse along it, leaned back a little like a real blade.
fn blade_outlines(c: Pos2, r: f32, angle: f32) -> Vec<Vec<Pos2>> {
    (0..5)
        .map(|k| {
            let a = angle + k as f32 * TAU / 5.0;
            let center = c + Vec2::angled(a) * r * 0.52;
            let lean = a + 0.35;
            let (ax, ay) = (Vec2::angled(lean) * r * 0.46, Vec2::angled(lean + PI / 2.0) * r * 0.19);
            (0..16)
                .map(|i| {
                    let u = i as f32 / 16.0 * TAU;
                    center + ax * u.cos() + ay * u.sin()
                })
                .collect()
        })
        .collect()
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

/// End of a window's pass: show this pass's blades natively where that
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
    // turning only while the decorative loops run (they rest when idle)
    let live = crate::kit::glass::ambient_live(ctx);
    let now = clock();
    let mut keep = Vec::new();
    let mut egui_turning = false;
    let mut egui_working = false;
    for req in &reqs {
        let tps = match req.look {
            Look::Blades { rpm } if live => turns_per_sec(rpm),
            Look::Blades { .. } => 0.0,
            Look::Arc => ARC_TPS,
        };
        let turn = TURNS.with(|t| {
            let mut t = t.borrow_mut();
            let e = t.entry(req.key.clone()).or_insert(Turn { angle0: 0.0, t0: now, tps });
            if e.tps != tps {
                *e = e.retimed(now, tps);
            }
            *e
        });
        let edge = if req.look == Look::Arc { ARC_WIDTH } else { 1.0 };
        let rect = Rect::from_center_size(req.center, Vec2::splat((req.radius + edge) * 2.0));
        let clip = req.painter.clip_rect().intersect(ctx.content_rect());
        let can = native_wanted()
            && !suspended
            && req.painter.opacity() >= 0.999
            && clip.intersects(rect)
            && !covered(ctx, req.painter.layer_id(), rect);
        // a layer that can't show now stays (hidden) for when it can
        let shown = if can {
            match native::show(id, &req.key, req.look == Look::Arc, clip, req.center, req.radius, req.color, req.hub, turn) {
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
            let angle = turn.angle_at(now) as f32;
            match req.look {
                Look::Blades { .. } => {
                    draw(&req.painter, req.center, req.radius, angle, req.color, req.hub);
                    egui_turning |= tps > 0.0;
                }
                Look::Arc => {
                    draw_arc(&req.painter, req.center, req.radius, angle, req.color);
                    egui_working = true;
                }
            }
        }
    }
    native::retain(id, &keep);
    if egui_working {
        crate::kit::glass::animate(ctx);
    } else if egui_turning {
        crate::kit::glass::ambient(ctx);
    }
}

/// End of the main pass: blades asked for in a window whose pass already
/// ended without a [`flush`] can't be drawn any more; forget them.
pub fn drop_unflushed() {
    PENDING.with(|q| q.borrow_mut().clear());
    SUSPENDED.with(|s| s.borrow_mut().clear());
}

/// Seconds on Core Animation's clock (so native and egui blades agree).
fn clock() -> f64 {
    native::media_time()
}

#[cfg(target_os = "macos")]
mod native {
    use super::Turn;
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
        fn CGPathCloseSubpath(path: *mut c_void);
        fn CGPathAddEllipseInRect(path: *mut c_void, m: *const c_void, rect: NSRect);
        fn CGPathRelease(path: *mut c_void);
    }

    pub fn media_time() -> f64 {
        unsafe { CACurrentMediaTime() }
    }

    /// One key's layers in one window.
    struct Blades {
        viewport: egui::ViewportId,
        key: String,
        /// The view's layer they hang from (a new window means new layers).
        root: *const AnyObject,
        container: Retained<AnyObject>,
        blades: Retained<AnyObject>,
        hub: Retained<AnyObject>,
        /// A spinner's arc (stroked) rather than blades (filled).
        arc: bool,
        /// What was set last, to touch only what changed.
        frame: Option<(Rect, Pos2, f32)>,
        colors: Option<(Color32, Color32, Color32, *const AnyObject)>,
        turn: Option<Turn>,
        hidden: bool,
    }

    thread_local! {
        static LAYERS: RefCell<Vec<Blades>> = const { RefCell::new(Vec::new()) };
    }

    fn ns_rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// `c` as egui shows it. egui's surface is untagged, so its bytes land on
    /// the display as they are: tag them with the window's own colour space
    /// (Display P3 when unknown), not sRGB, or the native blades look paler.
    fn ns_color(c: Color32, space: Option<&AnyObject>) -> Retained<AnyObject> {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let comps = [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, a as f64 / 255.0];
        unsafe {
            match space {
                Some(s) => msg_send![class!(NSColor), colorWithColorSpace: s, components: comps.as_ptr(), count: 4isize],
                None => msg_send![class!(NSColor), colorWithDisplayP3Red: comps[0], green: comps[1], blue: comps[2], alpha: comps[3]],
            }
        }
    }

    /// Set `layer`'s colour `sel` (fill or stroke) to `c`.
    fn set_color(layer: &AnyObject, stroke: bool, c: Color32, space: Option<&AnyObject>) {
        // the NSColor owns the CGColor: keep it alive across the set
        let ns = ns_color(c, space);
        unsafe {
            let cg: *const c_void = msg_send![&*ns, CGColor];
            if stroke {
                let _: () = msg_send![layer, setStrokeColor: cg];
            } else {
                let _: () = msg_send![layer, setFillColor: cg];
            }
        }
    }

    fn set_path(layer: &AnyObject, path: *mut c_void) {
        unsafe {
            let _: () = msg_send![layer, setPath: path as *const c_void];
            CGPathRelease(path);
        }
    }

    /// The blades' outline in a square of side 2r, centre (r, r), y down.
    fn blades_path(r: f32) -> *mut c_void {
        let c = Pos2::new(r, r);
        unsafe {
            let path = CGPathCreateMutable();
            for pts in super::blade_outlines(c, r, 0.0) {
                CGPathMoveToPoint(path, std::ptr::null(), pts[0].x as f64, pts[0].y as f64);
                for q in &pts[1..] {
                    CGPathAddLineToPoint(path, std::ptr::null(), q.x as f64, q.y as f64);
                }
                CGPathCloseSubpath(path);
            }
            path
        }
    }

    /// A spinner's arc in a square of side 2r, centre (r, r), y down.
    fn arc_path(r: f32) -> *mut c_void {
        let pts = crate::kit::glass::arc_points(Pos2::new(r, r), r, 0.0, super::ARC_SWEEP, 40);
        unsafe {
            let path = CGPathCreateMutable();
            CGPathMoveToPoint(path, std::ptr::null(), pts[0].x as f64, pts[0].y as f64);
            for q in &pts[1..] {
                CGPathAddLineToPoint(path, std::ptr::null(), q.x as f64, q.y as f64);
            }
            path
        }
    }

    fn hub_path(r: f32) -> *mut c_void {
        // as glass::stroke_circle: a 1 pt line just inside the hub's edge
        let hr = (r * 0.24) as f64 - 0.5;
        unsafe {
            let path = CGPathCreateMutable();
            CGPathAddEllipseInRect(path, std::ptr::null(), ns_rect(r as f64 - hr, r as f64 - hr, hr * 2.0, hr * 2.0));
            path
        }
    }

    fn new_layer(class_name: &str, scale: f64) -> Retained<AnyObject> {
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
    fn root_of(id: egui::ViewportId) -> Option<(Retained<AnyObject>, f64, Option<Retained<AnyObject>>)> {
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
            true
        })
    }

    /// Show `key`'s blades in viewport `id` (`clip` and `c` in egui points):
    /// Some(true) when they're on screen, Some(false) while a fresh or moved
    /// layer waits a pass, None if the window can't be reached.
    #[allow(clippy::too_many_arguments)]
    pub fn show(id: egui::ViewportId, key: &str, arc: bool, clip: Rect, c: Pos2, r: f32, color: Color32, hub: Color32, turn: Turn) -> Option<bool> {
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
                    let blades = new_layer("shape", scale);
                    let hub_l = new_layer("shape", scale);
                    unsafe {
                        let _: () = msg_send![&*container, setName: &*NSString::from_str(&format!("cmdmeow-spin-{key}"))];
                        let _: () = msg_send![&*container, setMasksToBounds: true];
                        let _: () = msg_send![&*container, addSublayer: &*blades];
                        let _: () = msg_send![&*container, addSublayer: &*hub_l];
                        let _: () = msg_send![&*hub_l, setLineWidth: 1.0f64];
                        if arc {
                            let none: *const c_void = std::ptr::null();
                            let _: () = msg_send![&*blades, setFillColor: none];
                            let _: () = msg_send![&*blades, setLineWidth: super::ARC_WIDTH as f64];
                            let _: () = msg_send![&*blades, setLineJoin: &*NSString::from_str("round")];
                            let _: () = msg_send![&*hub_l, setHidden: true];
                        }
                        let _: () = msg_send![&*root, addSublayer: &*container];
                    }
                    all.push(Blades { viewport: id, key: key.to_string(), root: root_ptr, container, blades, hub: hub_l, arc, frame: None, colors: None, turn: None, hidden: false });
                    all.len() - 1
                }
            };
            let b = &mut all[i];
            // a fresh layer or one that moved shows next pass, once it holds
            // still (scrolling, sliding): egui draws it this pass
            let frame = (clip, c, r);
            let moved = b.frame != Some(frame);
            let fresh_path = b.frame.is_none_or(|f| f.2 != r) || b.turn.is_none();
            let edge = crate::kit::glass::white(46);
            // nothing to change: no transaction at all (this runs every pass,
            // and a commit costs more than the rest of a quiet pass)
            if !moved && b.colors == Some((color, hub, edge, space_ptr)) && b.turn == Some(turn) && !b.hidden {
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
                    let _: () = msg_send![&*b.container, setGeometryFlipped: !flipped];
                    // stay put against the top right when the window grows or
                    // shrinks (the panel widens to the left for a sheet and
                    // grows down) until the next pass places it again
                    const MIN_X: u32 = 1 << 0;
                    const MIN_Y: u32 = 1 << 3;
                    const MAX_Y: u32 = 1 << 5;
                    let _: () = msg_send![&*b.container, setAutoresizingMask: MIN_X | if flipped { MAX_Y } else { MIN_Y }];
                    let _: () = msg_send![&*b.container, setFrame: ns_rect(clip.min.x as f64, y, clip.width() as f64, clip.height() as f64)];
                    // bounds + position, never frame: frame on a turned layer
                    // is its rotated bounding box and drifts off the dial
                    let side = (r * 2.0) as f64;
                    let mid = NSPoint::new((c.x - clip.min.x) as f64, (c.y - clip.min.y) as f64);
                    for l in [&b.blades, &b.hub] {
                        let _: () = msg_send![&**l, setBounds: ns_rect(0.0, 0.0, side, side)];
                        let _: () = msg_send![&**l, setPosition: mid];
                    }
                    if fresh_path {
                        if b.arc {
                            set_path(&b.blades, arc_path(r));
                        } else {
                            set_path(&b.blades, blades_path(r));
                            set_path(&b.hub, hub_path(r));
                        }
                    }
                    b.frame = Some(frame);
                }
                // a window moved to another screen gets new colours too
                if b.colors != Some((color, hub, edge, space_ptr)) {
                    let sp = space.as_deref();
                    if b.arc {
                        set_color(&b.blades, true, color, sp);
                    } else {
                        set_color(&b.blades, false, color, sp);
                        set_color(&b.hub, false, hub, sp);
                        set_color(&b.hub, true, edge, sp);
                    }
                    b.colors = Some((color, hub, edge, space_ptr));
                }
                if b.turn != Some(turn) {
                    set_turn(&b.blades, turn);
                    b.turn = Some(turn);
                }
                let hide = moved;
                if hide != b.hidden {
                    let _: () = msg_send![&*b.container, setHidden: hide];
                    b.hidden = hide;
                }
                let _: () = msg_send![class!(CATransaction), commit];
            }
            Some(!moved)
        })
    }

    /// Point the blades along `turn`: a still angle, plus a repeating full
    /// turn that started at `turn.t0` while they turn.
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
    use super::Turn;
    use egui::{Color32, Pos2, Rect};

    pub fn media_time() -> f64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
    }
    #[allow(clippy::too_many_arguments)]
    pub fn show(_id: egui::ViewportId, _key: &str, _arc: bool, _clip: Rect, _c: Pos2, _r: f32, _color: Color32, _hub: Color32, _turn: Turn) -> Option<bool> {
        None
    }
    pub fn hide(_id: egui::ViewportId, _key: &str) -> bool {
        false
    }
    pub fn retain(_id: egui::ViewportId, _keep: &[String]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(next <= std::time::Duration::from_millis(20), "and asks for the next frame: {next:?}");
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
