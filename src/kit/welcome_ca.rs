//! The Welcome step's animation on Core Animation (R-anim, M1): the cat
//! peeking out and hopping up with its ears flicking, the ring drawing
//! itself round it and the tools popping onto the ring.
//!
//! onboard.rs samples the animation (one frame per 1/60 s, see [`Frames`])
//! and says where it sits ([`Scene`]); here it becomes layers over the
//! setup window with keyframe animations that the window server plays. No
//! egui frames of ours while it runs, and the cat is drawn from its
//! vector paths, sharp at any size.
//!
//! The layers sit over everything egui draws, so like `kit::spin` they show
//! only while they hold still: a pass where the scene moved (the step
//! sliding in, a live resize) hides them and egui draws that frame. The
//! timeline keeps running meanwhile ([`elapsed`]).

use crate::kit::glass::Icon;
use egui::{Color32, Pos2};

/// Frames per second of [`Frames`].
pub const FPS: f64 = 60.0;

/// The Welcome animation, one sample per 1/[`FPS`] s from 0 to its end.
#[derive(Clone, Debug, Default)]
pub struct Frames {
    /// The cat: how far it is lifted (points; negative = below the floor),
    /// its width and height squash.
    pub cat: Vec<(f32, f32, f32)>,
    /// The ears' outward tilt (left, right), radians.
    pub ears: Vec<(f32, f32)>,
    /// How much of the ring is drawn, 0 to 1.
    pub arc: Vec<f32>,
    /// Each tool chip's scale (0 = not there yet).
    pub chips: Vec<Vec<f32>>,
}

/// Where the Welcome animation sits and how it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The ring's centre, in window points.
    pub c: Pos2,
    pub ring: f32,
    pub chip: f32,
    /// The cat's side; it rests with its bottom at `c.y + size / 2`.
    pub size: f32,
    pub icons: Vec<Icon>,
    pub cat: Color32,
    pub arc: Color32,
    pub chip_fill: Color32,
    pub chip_edge: Color32,
    pub icon: Color32,
}

/// Show the scene at time `t` of its animation (seconds). True when the
/// layers are on screen this pass, so egui draws none of it; false while
/// they can't show (egui draws the frame). `frames` is asked once per
/// new set of layers.
pub fn show(ctx: &egui::Context, scene: &Scene, t: f32, frames: impl FnOnce() -> Frames) -> bool {
    if !crate::kit::spin::native_allowed() {
        return false;
    }
    native::show(ctx.viewport_id(), scene, t, frames)
}

/// Seconds since the running animation started, while its layers are up.
pub fn elapsed(ctx: &egui::Context) -> Option<f32> {
    native::elapsed(ctx.viewport_id())
}

/// Take the layers away (the step is hidden or covered: it waits; shown
/// again it starts from where it was).
pub fn remove(ctx: &egui::Context) {
    native::remove(ctx.viewport_id());
}

/// End of a pass of the setup window: layers [`show`] wasn't called for
/// this pass (another step) go.
pub fn end_pass(ctx: &egui::Context) {
    native::end_pass(ctx.viewport_id());
}

#[cfg(target_os = "macos")]
mod native {
    use super::{Frames, Scene, FPS};
    use crate::kit::brand;
    use crate::kit::glass::Icon;
    use crate::kit::spin::native::{blades_path, new_layer, root_of, set_color, set_path};
    use egui::Pos2;
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
        fn CGPathAddArc(path: *mut c_void, m: *const c_void, x: f64, y: f64, radius: f64, start: f64, end: f64, clockwise: bool);
        fn CGPathAddEllipseInRect(path: *mut c_void, m: *const c_void, rect: NSRect);
        fn CGPathCloseSubpath(path: *mut c_void);
        fn CGPathRelease(path: *mut c_void);
        fn CGPathCreateCopyByStrokingPath(path: *const c_void, m: *const c_void, width: f64, cap: i32, join: i32, miter: f64) -> *mut c_void;
        fn CGPathCreateCopyBySubtractingPath(path: *const c_void, cut: *const c_void, even_odd: bool) -> *mut c_void;
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(s: *mut c_void);
        fn CGDataProviderCreateWithCFData(data: *const c_void) -> *mut c_void;
        fn CGDataProviderRelease(p: *mut c_void);
        fn CGImageCreate(w: usize, h: usize, bpc: usize, bpp: usize, row: usize, space: *const c_void, info: u32, provider: *mut c_void, decode: *const f64, interpolate: bool, intent: i32) -> *mut c_void;
        fn CGImageRelease(i: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: isize) -> *const c_void;
        fn CFRelease(p: *const c_void);
    }

    struct Up {
        viewport: egui::ViewportId,
        /// The view's layer they hang from (a new window means new layers).
        root: *const AnyObject,
        holder: Retained<AnyObject>,
        /// Everything, placed with its origin at the ring's centre.
        stage: Retained<AnyObject>,
        scene: Scene,
        /// When the animation's t = 0 was, in media time.
        begin: f64,
        hidden: bool,
        /// [`show`] was called this pass.
        touched: bool,
    }

    thread_local! {
        static UP: RefCell<Option<Up>> = const { RefCell::new(None) };
    }

    fn key(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    fn num(v: f64) -> Retained<NSNumber> {
        NSNumber::new_f64(v)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    pub fn elapsed(id: egui::ViewportId) -> Option<f32> {
        // layers of a window that's gone (closed and opened again) don't count
        let root = root_of(id).map(|(r, _, _)| &*r as *const AnyObject)?;
        UP.with(|u| u.borrow().as_ref().filter(|u| u.viewport == id && u.root == root).map(|u| (unsafe { CACurrentMediaTime() } - u.begin) as f32))
    }

    pub fn remove(id: egui::ViewportId) {
        UP.with(|u| {
            let mut u = u.borrow_mut();
            if u.as_ref().is_some_and(|x| x.viewport == id) {
                if let Some(x) = u.take() {
                    unsafe {
                        let _: () = msg_send![class!(CATransaction), begin];
                        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                        let _: () = msg_send![&*x.holder, removeFromSuperlayer];
                        let _: () = msg_send![class!(CATransaction), commit];
                    }
                }
            }
        });
    }

    pub fn end_pass(id: egui::ViewportId) {
        let stale = UP.with(|u| {
            let mut u = u.borrow_mut();
            match u.as_mut() {
                Some(x) if x.viewport == id => !std::mem::replace(&mut x.touched, false),
                _ => false,
            }
        });
        if stale {
            remove(id);
        }
    }

    pub fn show(id: egui::ViewportId, scene: &Scene, t: f32, frames: impl FnOnce() -> Frames) -> bool {
        let Some((root, scale, space)) = root_of(id) else { return false };
        let root_ptr: *const AnyObject = &*root;
        // new layers for another window, look or set of tools
        let fresh = UP.with(|u| {
            let u = u.borrow();
            match u.as_ref() {
                Some(x) => x.viewport != id || x.root != root_ptr || !same_look(&x.scene, scene),
                None => true,
            }
        });
        if fresh {
            remove(id);
            let begin = unsafe { CACurrentMediaTime() } - t as f64;
            let Some(up) = (unsafe { build(id, &root, root_ptr, scale, space.as_deref(), scene, &frames(), begin) }) else { return false };
            UP.with(|u| *u.borrow_mut() = Some(up));
            // shown from the next pass, once it holds still
            return false;
        }
        UP.with(|u| {
            let mut u = u.borrow_mut();
            let Some(x) = u.as_mut() else { return false };
            x.touched = true;
            let moved = x.scene.c != scene.c;
            unsafe {
                let _: () = msg_send![class!(CATransaction), begin];
                let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                if moved {
                    let _: () = msg_send![&*x.stage, setPosition: stage_point(&root, scene.c)];
                    x.scene.c = scene.c;
                }
                if moved != x.hidden {
                    let _: () = msg_send![&*x.holder, setHidden: moved];
                    x.hidden = moved;
                }
                let _: () = msg_send![class!(CATransaction), commit];
            }
            !x.hidden
        })
    }

    /// Everything but where it sits.
    fn same_look(a: &Scene, b: &Scene) -> bool {
        a.ring == b.ring && a.chip == b.chip && a.size == b.size && a.icons == b.icons && a.cat == b.cat && a.arc == b.arc && a.chip_fill == b.chip_fill && a.chip_edge == b.chip_edge && a.icon == b.icon
    }

    /// The ring's centre in the holder (whose space runs y-down like egui's).
    fn stage_point(_root: &AnyObject, c: Pos2) -> NSPoint {
        NSPoint::new(c.x as f64, c.y as f64)
    }

    /// A keyframe animation of `path` through `values`, one per 1/FPS s
    /// from `begin`; the layer's own value is the last one.
    unsafe fn keyframes(layer: &AnyObject, path: &str, values: &AnyObject, n: usize, begin: f64) {
        let anim: Retained<AnyObject> = msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: &*key(path)];
        let _: () = msg_send![&*anim, setValues: values];
        let times: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: n];
        for i in 0..n {
            let v = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            let _: () = msg_send![&*times, addObject: &*num(v)];
        }
        let _: () = msg_send![&*anim, setKeyTimes: &*times];
        let _: () = msg_send![&*anim, setDuration: ((n.max(2) - 1) as f64 / FPS)];
        let _: () = msg_send![&*anim, setBeginTime: begin];
        let _: () = msg_send![&*anim, setFillMode: &*key("backwards")];
        let _: () = msg_send![layer, addAnimation: &*anim, forKey: &*key(&format!("cmdmeow-{path}"))];
    }

    /// A keyframe animation of a number.
    unsafe fn number_track(layer: &AnyObject, path: &str, v: &[f64], begin: f64) {
        let Some(&last) = v.last() else { return };
        let arr: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: v.len()];
        for x in v {
            let _: () = msg_send![&*arr, addObject: &*num(*x)];
        }
        let _: () = msg_send![layer, setValue: &*num(last), forKeyPath: &*key(path)];
        keyframes(layer, path, &arr, v.len(), begin);
    }

    /// Point (x, y) of the 1024 box in a cat of side `s` (with the mark's nudge).
    fn box_pt(s: f64, x: f32, y: f32) -> (f64, f64) {
        (((x + brand::NUDGE.0) / 1024.0) as f64 * s, ((y + brand::NUDGE.1) / 1024.0) as f64 * s)
    }

    unsafe fn poly_path(s: f64, pts: &[(f32, f32)]) -> *mut c_void {
        let path = CGPathCreateMutable();
        for (i, &(x, y)) in pts.iter().enumerate() {
            let (x, y) = box_pt(s, x, y);
            if i == 0 {
                CGPathMoveToPoint(path, std::ptr::null(), x, y);
            } else {
                CGPathAddLineToPoint(path, std::ptr::null(), x, y);
            }
        }
        CGPathCloseSubpath(path);
        path
    }

    /// The keycap without ears, the ⌘ cut out of it, in a cat of side `s`.
    unsafe fn body_path(s: f64) -> *mut c_void {
        let body = CGPathCreateMutable();
        for seg in brand::BODY {
            match *seg {
                brand::Seg::M(x, y) => {
                    let (x, y) = box_pt(s, x, y);
                    CGPathMoveToPoint(body, std::ptr::null(), x, y);
                }
                brand::Seg::L(x, y) => {
                    let (x, y) = box_pt(s, x, y);
                    CGPathAddLineToPoint(body, std::ptr::null(), x, y);
                }
                brand::Seg::C(a, b, c, d, x, y) => {
                    let (a, b) = box_pt(s, a, b);
                    let (c, d) = box_pt(s, c, d);
                    let (x, y) = box_pt(s, x, y);
                    CGPathAddCurveToPoint(body, std::ptr::null(), a, b, c, d, x, y);
                }
            }
        }
        CGPathCloseSubpath(body);
        let cmd = CGPathCreateMutable();
        for (cx, cy, r, a0, a1) in brand::CMD_LOOPS {
            let (x, y) = box_pt(s, cx + r * a0.cos(), cy + r * a0.sin());
            CGPathMoveToPoint(cmd, std::ptr::null(), x, y);
            let (cx, cy) = box_pt(s, cx, cy);
            // y down: angles grow clockwise on screen, as in the 1024 box
            CGPathAddArc(cmd, std::ptr::null(), cx, cy, r as f64 / 1024.0 * s, a0 as f64, a1 as f64, a1 < a0);
        }
        for (x0, y0, x1, y1) in brand::CMD_BARS {
            let (x0, y0) = box_pt(s, x0, y0);
            let (x1, y1) = box_pt(s, x1, y1);
            CGPathMoveToPoint(cmd, std::ptr::null(), x0, y0);
            CGPathAddLineToPoint(cmd, std::ptr::null(), x1, y1);
        }
        // butt caps, miter joins: as tiny-skia strokes it
        let cut = CGPathCreateCopyByStrokingPath(cmd, std::ptr::null(), brand::CMD_WIDTH as f64 / 1024.0 * s, 0, 0, 10.0);
        CGPathRelease(cmd);
        if cut.is_null() {
            return body;
        }
        let out = CGPathCreateCopyBySubtractingPath(body, cut, false);
        CGPathRelease(cut);
        if out.is_null() {
            return body;
        }
        CGPathRelease(body);
        out
    }

    /// A white-on-clear picture of `rgba` (straight alpha) as a CGImage.
    unsafe fn image(w: usize, h: usize, rgba: &[u8]) -> *mut c_void {
        let space = CGColorSpaceCreateDeviceRGB();
        let data = CFDataCreate(std::ptr::null(), rgba.as_ptr(), rgba.len() as isize);
        let provider = CGDataProviderCreateWithCFData(data);
        // RGBA, alpha last, not premultiplied
        const ALPHA_LAST: u32 = 3;
        let img = CGImageCreate(w, h, 8, 32, w * 4, space, ALPHA_LAST, provider, std::ptr::null(), true, 0);
        CGDataProviderRelease(provider);
        CFRelease(data);
        CGColorSpaceRelease(space);
        img
    }

    /// A layer holding `child` that scales about its bottom middle.
    unsafe fn squash_layer(scale: f64, s: f64, at: NSPoint) -> Retained<AnyObject> {
        let l = new_layer("plain", scale);
        let _: () = msg_send![&*l, setBounds: rect(0.0, 0.0, s, s)];
        let _: () = msg_send![&*l, setAnchorPoint: NSPoint::new(0.5, 1.0)];
        let _: () = msg_send![&*l, setPosition: at];
        l
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn build(id: egui::ViewportId, root: &AnyObject, root_ptr: *const AnyObject, scale: f64, space: Option<&AnyObject>, scene: &Scene, f: &Frames, begin: f64) -> Option<Up> {
        let n = f.cat.len();
        if n < 2 || f.ears.len() != n || f.arc.len() != n {
            return None;
        }
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        let bounds: NSRect = msg_send![root, bounds];
        let flipped: bool = msg_send![root, contentsAreFlipped];
        const W_SIZABLE: u32 = 1 << 1;
        const H_SIZABLE: u32 = 1 << 4;
        let holder = new_layer("plain", scale);
        let _: () = msg_send![&*holder, setName: &*key("cmdmeow-welcome")];
        // y down like egui
        let _: () = msg_send![&*holder, setGeometryFlipped: !flipped];
        let _: () = msg_send![&*holder, setFrame: rect(0.0, 0.0, bounds.size.width, bounds.size.height)];
        let _: () = msg_send![&*holder, setAutoresizingMask: W_SIZABLE | H_SIZABLE];
        let _: () = msg_send![&*holder, setHidden: true];
        let stage = new_layer("plain", scale);
        let _: () = msg_send![&*stage, setBounds: rect(0.0, 0.0, 0.0, 0.0)];
        let _: () = msg_send![&*stage, setPosition: stage_point(root, scene.c)];
        let _: () = msg_send![&*holder, addSublayer: &*stage];

        // the ring drawing itself from the top, clockwise
        let r = scene.ring as f64;
        let arc = new_layer("shape", scale);
        let path = CGPathCreateMutable();
        let top = -std::f64::consts::FRAC_PI_2;
        CGPathAddArc(path, std::ptr::null(), 0.0, 0.0, r, top, top + std::f64::consts::TAU, false);
        set_path(&arc, path);
        let _: () = msg_send![&*arc, setLineWidth: 2.5f64];
        set_color(&arc, false, None, space);
        set_color(&arc, true, Some(scene.arc), space);
        number_track(&arc, "strokeEnd", &f.arc.iter().map(|&v| v as f64).collect::<Vec<_>>(), begin);
        let _: () = msg_send![&*stage, addSublayer: &*arc];

        // the tools on the ring
        let k = scene.icons.len().max(1);
        let chip = scene.chip as f64;
        for (i, icon) in scene.icons.iter().enumerate() {
            let a = top + i as f64 / k as f64 * std::f64::consts::TAU;
            let l = new_layer("plain", scale);
            let side = chip * 2.0 + 2.0;
            let _: () = msg_send![&*l, setBounds: rect(0.0, 0.0, side, side)];
            let _: () = msg_send![&*l, setPosition: NSPoint::new(a.cos() * r, a.sin() * r)];
            let disc = new_layer("shape", scale);
            let _: () = msg_send![&*disc, setFrame: rect(0.0, 0.0, side, side)];
            let p = CGPathCreateMutable();
            CGPathAddEllipseInRect(p, std::ptr::null(), rect(1.0, 1.0, chip * 2.0, chip * 2.0));
            set_path(&disc, p);
            let _: () = msg_send![&*disc, setLineWidth: 1.0f64];
            set_color(&disc, false, Some(scene.chip_fill), space);
            set_color(&disc, true, Some(scene.chip_edge), space);
            let _: () = msg_send![&*l, addSublayer: &*disc];
            if let Some(g) = icon_layer(*icon, scale, space, scene, side) {
                let _: () = msg_send![&*l, addSublayer: &*g];
            }
            if let Some(v) = f.chips.get(i) {
                number_track(&l, "transform.scale", &v.iter().map(|&s| s.max(0.0001) as f64).collect::<Vec<_>>(), begin);
            }
            let _: () = msg_send![&*stage, addSublayer: &*l];
        }

        // the cat: hidden below the floor (its resting bottom), it peeks out from there
        let s = scene.size as f64;
        let clip = new_layer("plain", scale);
        let _: () = msg_send![&*clip, setFrame: rect(-0.7 * s, -s, 1.4 * s, 1.5 * s)];
        let _: () = msg_send![&*clip, setMasksToBounds: true];
        let lift = squash_layer(scale, s, NSPoint::new(0.7 * s, 1.5 * s));
        let wide = squash_layer(scale, s, NSPoint::new(s / 2.0, s));
        let tall = squash_layer(scale, s, NSPoint::new(s / 2.0, s));
        number_track(&lift, "transform.translation.y", &f.cat.iter().map(|c| -c.0 as f64).collect::<Vec<_>>(), begin);
        number_track(&wide, "transform.scale.x", &f.cat.iter().map(|c| c.1 as f64).collect::<Vec<_>>(), begin);
        number_track(&tall, "transform.scale.y", &f.cat.iter().map(|c| c.2 as f64).collect::<Vec<_>>(), begin);
        for right in [false, true] {
            let ear = new_layer("shape", scale);
            let _: () = msg_send![&*ear, setFrame: rect(0.0, 0.0, s, s)];
            set_color(&ear, false, Some(scene.cat), space);
            let paths: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: n];
            let mut last = std::ptr::null_mut();
            for e in &f.ears {
                let p = poly_path(s, &brand::bent_ear(right, if right { e.1 } else { e.0 }));
                let _: () = msg_send![&*paths, addObject: p as *const AnyObject];
                if !last.is_null() {
                    CGPathRelease(last);
                }
                last = p;
            }
            // the last frame is the ear at rest
            set_path(&ear, last);
            keyframes(&ear, "path", &paths, n, begin);
            let _: () = msg_send![&*tall, addSublayer: &*ear];
        }
        let body = new_layer("shape", scale);
        let _: () = msg_send![&*body, setFrame: rect(0.0, 0.0, s, s)];
        set_path(&body, body_path(s));
        set_color(&body, false, Some(scene.cat), space);
        let _: () = msg_send![&*tall, addSublayer: &*body];
        let _: () = msg_send![&*wide, addSublayer: &*tall];
        let _: () = msg_send![&*lift, addSublayer: &*wide];
        let _: () = msg_send![&*clip, addSublayer: &*lift];
        let _: () = msg_send![&*stage, addSublayer: &*clip];

        let _: () = msg_send![root, addSublayer: &*holder];
        let _: () = msg_send![class!(CATransaction), commit];
        Some(Up { viewport: id, root: root_ptr, holder, stage, scene: scene.clone(), begin, hidden: true, touched: true })
    }

    /// A tool's icon, 19 pt, centred in a chip of side `side`: its SF
    /// Symbol, or the fan glyph.
    unsafe fn icon_layer(icon: Icon, scale: f64, space: Option<&AnyObject>, scene: &Scene, side: f64) -> Option<Retained<AnyObject>> {
        const SIZE: f64 = 19.0;
        let mid = NSPoint::new(side / 2.0, side / 2.0);
        if icon == Icon::Fan {
            let r = (SIZE / 2.0) as f32;
            let l = new_layer("shape", scale);
            let _: () = msg_send![&*l, setBounds: rect(0.0, 0.0, SIZE, SIZE)];
            let _: () = msg_send![&*l, setPosition: mid];
            set_path(&l, blades_path(r));
            let _: () = msg_send![&*l, setFillRule: &*key("even-odd")];
            set_color(&l, false, Some(scene.icon), space);
            return Some(l);
        }
        let name = crate::kit::glass::symbol_name(icon)?;
        let px = (SIZE * scale * 0.92).round().max(8.0);
        let (w, h, rgba) = crate::kit::native::render_symbol(name, px, crate::kit::native::weight_semibold())?;
        let (pw, ph) = (w as f64 / scale, h as f64 / scale);
        let k = (SIZE / pw).min(SIZE / ph).min(1.15);
        let img = image(w, h, &rgba);
        if img.is_null() {
            return None;
        }
        // the icon colour, shown through the symbol's shape
        let l = new_layer("plain", scale);
        let _: () = msg_send![&*l, setBounds: rect(0.0, 0.0, pw * k, ph * k)];
        let _: () = msg_send![&*l, setPosition: mid];
        let c = crate::kit::spin::native::ns_color(scene.icon, space);
        let cg: *const c_void = msg_send![&*c, CGColor];
        let _: () = msg_send![&*l, setBackgroundColor: cg];
        let mask = new_layer("plain", scale);
        let _: () = msg_send![&*mask, setFrame: rect(0.0, 0.0, pw * k, ph * k)];
        let _: () = msg_send![&*mask, setContents: img as *const AnyObject];
        let _: () = msg_send![&*mask, setContentsGravity: &*key("resize")];
        CGImageRelease(img);
        let _: () = msg_send![&*l, setMask: &*mask];
        Some(l)
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{Frames, Scene};
    pub fn show(_id: egui::ViewportId, _scene: &Scene, _t: f32, _frames: impl FnOnce() -> Frames) -> bool {
        false
    }
    pub fn elapsed(_id: egui::ViewportId) -> Option<f32> {
        None
    }
    pub fn remove(_id: egui::ViewportId) {}
    pub fn end_pass(_id: egui::ViewportId) {}
}
