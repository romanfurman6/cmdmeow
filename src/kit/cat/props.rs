//! The cat's things on screen: its bed, its bowl (in two halves, so its
//! head can go into it: the inside and the food under the cat, the front
//! wall over its chin) and the yarn ball. Flat colour with a dark outline,
//! drawn like the cat: Core Animation shape layers, built once and only
//! moved, shown or hidden each frame.

use super::live::Scene;
use super::toys::{self, BallView, BowlView, BOWL_H, BR};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_core_foundation::{CGAffineTransform, CGPoint};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::f64::consts::PI;
use std::ffi::c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPathCreateMutable() -> *mut c_void;
    fn CGPathMoveToPoint(path: *mut c_void, m: *const c_void, x: f64, y: f64);
    fn CGPathAddLineToPoint(path: *mut c_void, m: *const c_void, x: f64, y: f64);
    fn CGPathAddEllipseInRect(path: *mut c_void, m: *const c_void, rect: NSRect);
    fn CGPathAddRoundedRect(path: *mut c_void, m: *const c_void, rect: NSRect, w: f64, h: f64);
    fn CGPathCloseSubpath(path: *mut c_void);
    fn CGPathRelease(path: *mut c_void);
    fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> *mut c_void;
    fn CGColorRelease(color: *mut c_void);
}

/// The outline of everything, as on the cat.
const INK: u32 = 0x0a0f09;
const BED: u32 = 0x7f8fb4;
const BED_TOP: u32 = 0xa7b5d6;
const RIM: u32 = 0xd7dee5;
const INSIDE: u32 = 0x6f7a85;
const WALL: u32 = 0xb4bec8;
const SHINE: u32 = 0xe9eef2;
const SHADE: u32 = 0x939eaa;
const KIBBLE: [u32; 3] = [0x8e4f22, 0xa8622d, 0xb97437];
const KIBBLE_EDGE: u32 = 0x3e2210;
const KIBBLE_HOLE: u32 = 0x5a3214;
const CASE: u32 = 0x5d6772;
const CHARGE: u32 = 0xf4f6f8;
const LOW: u32 = 0xe04646;
const LOW_DIM: u32 = 0x7a2a24;
const CHARGING: u32 = 0xb6dc58;
const YARN: u32 = 0xef5d5d;
const STRAND: u32 = 0xb8323d;
/// The bowl's rim (top and bottom half-widths, its depth) and foot.
const RT: f64 = 42.0;
const RY: f64 = 10.0;
const RB: f64 = 35.0;

type Layer = Retained<AnyObject>;

/// A path in a layer's own points, y up.
struct Path(*mut c_void);

impl Drop for Path {
    fn drop(&mut self) {
        unsafe { CGPathRelease(self.0) };
    }
}

impl Path {
    fn new() -> Path {
        Path(unsafe { CGPathCreateMutable() })
    }

    fn ellipse(self, c: (f64, f64), r: (f64, f64)) -> Path {
        unsafe { CGPathAddEllipseInRect(self.0, std::ptr::null(), NSRect::new(NSPoint::new(c.0 - r.0, c.1 - r.1), NSSize::new(r.0 * 2.0, r.1 * 2.0))) };
        self
    }

    fn rect(self, at: (f64, f64), size: (f64, f64), round: f64) -> Path {
        let r = NSRect::new(NSPoint::new(at.0, at.1), NSSize::new(size.0, size.1));
        unsafe { CGPathAddRoundedRect(self.0, std::ptr::null(), r, round, round) };
        self
    }

    /// A line through the points; a run of points `None` breaks it.
    fn line(self, pts: &[Option<(f64, f64)>], close: bool) -> Path {
        let mut pen = false;
        for p in pts {
            match p {
                Some((x, y)) if pen => unsafe { CGPathAddLineToPoint(self.0, std::ptr::null(), *x, *y) },
                Some((x, y)) => unsafe {
                    CGPathMoveToPoint(self.0, std::ptr::null(), *x, *y);
                    pen = true;
                },
                None => pen = false,
            }
        }
        if close {
            unsafe { CGPathCloseSubpath(self.0) };
        }
        self
    }
}

/// Points on an ellipse (middle `c`, radii `r`, turned `rot`) from angle
/// `a0` to `a1`, y up.
fn arc(c: (f64, f64), r: (f64, f64), rot: f64, a0: f64, a1: f64, n: usize) -> Vec<(f64, f64)> {
    let (sn, cs) = rot.sin_cos();
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            let (x, y) = (r.0 * a.cos(), r.1 * a.sin());
            (c.0 + x * cs - y * sn, c.1 + x * sn + y * cs)
        })
        .collect()
}

unsafe fn color(rgb: u32, a: f64) -> *mut c_void {
    let c = |s: u32| f64::from((rgb >> s) & 0xff) / 255.0;
    CGColorCreateSRGB(c(16), c(8), c(0), a)
}

unsafe fn set_color(l: &AnyObject, fill: bool, rgb: Option<(u32, f64)>) {
    let c = rgb.map_or(std::ptr::null_mut(), |(rgb, a)| color(rgb, a));
    if fill {
        let _: () = msg_send![l, setFillColor: c];
    } else {
        let _: () = msg_send![l, setStrokeColor: c];
    }
    if !c.is_null() {
        CGColorRelease(c);
    }
}

/// A group to move a thing's layers about together.
unsafe fn group(parent: &AnyObject, z: f64) -> Layer {
    let l: Layer = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*l, setAnchorPoint: CGPoint { x: 0.0, y: 0.0 }];
    let _: () = msg_send![&*l, setPosition: CGPoint { x: 0.0, y: 0.0 }];
    let _: () = msg_send![&*l, setZPosition: z];
    let _: () = msg_send![parent, addSublayer: &*l];
    l
}

unsafe fn shape(parent: &AnyObject, path: &Path, fill: Option<(u32, f64)>, stroke: Option<(u32, f64)>) -> Layer {
    let l: Layer = msg_send![class!(CAShapeLayer), layer];
    let _: () = msg_send![&*l, setPath: path.0];
    set_color(&l, true, fill);
    if let Some((rgb, w)) = stroke {
        set_color(&l, false, Some((rgb, 1.0)));
        let _: () = msg_send![&*l, setLineWidth: w];
        let _: () = msg_send![&*l, setLineJoin: &*NSString::from_str("round")];
        let _: () = msg_send![&*l, setLineCap: &*NSString::from_str("round")];
    }
    let _: () = msg_send![parent, addSublayer: &*l];
    l
}

unsafe fn put(l: &AnyObject, x: f64, y: f64) {
    let _: () = msg_send![l, setPosition: CGPoint { x, y }];
}

unsafe fn hide(l: &AnyObject, hidden: bool) {
    let _: () = msg_send![l, setHidden: hidden];
}

/// The battery painted on the bowl, as last drawn.
#[derive(Clone, Copy, PartialEq)]
struct Charge {
    width: i32,
    state: u8,
}

pub struct Props {
    bed: Layer,
    back: Layer,
    front: Layer,
    kibble: Vec<Layer>,
    shown_kibble: usize,
    battery: Layer,
    charge: Layer,
    bolt: Layer,
    drawn: Option<Charge>,
    ball: Layer,
    ball_shadow: Layer,
    ball_body: Layer,
    ball_spin: Layer,
}

impl Props {
    /// The things' layers in `root`, around the cat's body at depth 3:
    /// the bed (1) and the bowl (2, its front 4) under and over it, the
    /// ball (5) always in front.
    pub unsafe fn new(root: &AnyObject) -> Props {
        // the bed: a round cushion with a raised rim
        let bed = group(root, 1.0);
        shape(&bed, &Path::new().ellipse((0.0, -2.0), (102.0, 14.0)), Some((0, 0.22)), None);
        shape(&bed, &Path::new().ellipse((0.0, 16.0), (100.0, 30.0)), Some((BED, 1.0)), Some((INK, 3.0)));
        shape(&bed, &Path::new().ellipse((0.0, 21.0), (80.0, 19.0)), Some((BED_TOP, 1.0)), Some((INK, 2.4)));
        shape(&bed, &Path::new().ellipse((-26.0, 27.0), (26.0, 5.0)), Some((0xffffff, 0.28)), None);
        hide(&bed, true);

        // the bowl's back half: its shadow, the far rim, the steel inside, the food
        let back = group(root, 2.0);
        shape(&back, &Path::new().ellipse((0.0, -1.0), (RB + 9.0, 7.0)), Some((0, 0.22)), None);
        shape(&back, &Path::new().ellipse((0.0, BOWL_H), (RT, RY)), Some((RIM, 1.0)), Some((INK, 2.6)));
        shape(&back, &Path::new().ellipse((0.0, BOWL_H - 1.0), (RT - 6.0, RY - 3.2)), Some((INSIDE, 1.0)), Some((INK, 1.6)));
        let kibble = toys::kibbles()
            .into_iter()
            .map(|(x, y, r, s)| {
                let tint = KIBBLE[if s < 2 { 0 } else if s < 5 { 1 } else { 2 }];
                let k = shape(&back, &Path::new().ellipse((x, BOWL_H - y), (r, r)), Some((tint, 1.0)), Some((KIBBLE_EDGE, 1.3)));
                shape(&k, &Path::new().ellipse((x, BOWL_H - y), (r * 0.32, r * 0.32)), Some((KIBBLE_HOLE, 1.0)), None);
                k
            })
            .collect::<Vec<_>>();
        hide(&back, true);

        // the front wall, a little flared to the foot, with the near half of the rim
        let front = group(root, 4.0);
        let near = arc((0.0, BOWL_H), (RT, -RY), 0.0, PI, 0.0, 28);
        let mut wall: Vec<Option<(f64, f64)>> = near.iter().copied().map(Some).collect();
        wall.extend(arc((0.0, 3.0), (RB, -5.0), 0.0, 0.0, PI, 24).into_iter().map(Some));
        shape(&front, &Path::new().line(&wall, true), Some((WALL, 1.0)), None);
        // a light and a dark band down the steel
        let top = |x: f64| BOWL_H - RY * (1.0 - (x / RT).powi(2)).max(0.0).sqrt();
        let foot = |x: f64| if x.abs() <= RB { 3.0 - 5.0 * (1.0 - (x / RB).powi(2)).max(0.0).sqrt() } else { 3.0 + (x.abs() - RB) / (RT - RB) * (BOWL_H - 3.0) };
        let band = |x0: f64, x1: f64| {
            let x1 = x1.min(RT - 0.5);
            let xs: Vec<f64> = (0..=8).map(|i| x0 + (x1 - x0) * f64::from(i) / 8.0).collect();
            let mut pts: Vec<Option<(f64, f64)>> = xs.iter().map(|&x| Some((x, top(x)))).collect();
            pts.extend(xs.iter().rev().map(|&x| Some((x, foot(x).min(top(x))))));
            Path::new().line(&pts, true)
        };
        shape(&front, &band(-RT * 0.62, -RT * 0.62 + 7.0), Some((SHINE, 1.0)), None);
        shape(&front, &band(RT * 0.55, RT * 0.55 + 14.0), Some((SHADE, 1.0)), None);
        shape(&front, &Path::new().line(&wall, true), None, Some((INK, 3.0)));
        // the steel of the rim sits just inside the outline: at the ends, where
        // the arc runs straight down, a stroke on the outline itself pokes
        // out past the ink on either side
        let steel: Vec<Option<(f64, f64)>> = arc((0.0, BOWL_H), (RT - 1.4, -(RY - 1.4)), 0.0, PI, 0.0, 28).into_iter().map(Some).collect();
        shape(&front, &Path::new().line(&steel, false), None, Some((RIM, 5.0)));
        let rim: Vec<Option<(f64, f64)>> = near.into_iter().map(Some).collect();
        shape(&front, &Path::new().line(&rim, false), None, Some((INK, 2.6)));
        let lip: Vec<Option<(f64, f64)>> = arc((0.0, BOWL_H - 2.6), (RT - 1.0, -(RY - 0.6)), 0.0, PI * 0.08, PI * 0.92, 22).into_iter().map(Some).collect();
        shape(&front, &Path::new().line(&lip, false), None, Some((INK, 1.6)));
        // the battery, painted on its side
        let battery = group(&front, 0.0);
        shape(&battery, &Path::new().rect((-12.0, 4.0), (22.0, 11.0), 2.5), Some((CASE, 1.0)), Some((INK, 2.0)));
        shape(&battery, &Path::new().rect((10.0, 7.5), (2.6, 4.0), 0.0), Some((INK, 1.0)), None);
        let charge = shape(&battery, &Path::new(), Some((CHARGE, 1.0)), None);
        let bolt_pts = [(0.5, 13.5), (-4.0, 9.0), (-1.0, 9.0), (-2.5, 5.5), (2.0, 10.0), (-1.0, 10.0)].map(Some);
        let bolt = shape(&battery, &Path::new().line(&bolt_pts, true), Some((INK, 1.0)), None);
        hide(&front, true);

        // the yarn ball: its shadow on the desk, and the ball above it
        let ball = group(root, 5.0);
        let ball_shadow = shape(&ball, &Path::new().ellipse((0.0, 0.0), (BR * 1.1, BR * 0.32)), Some((0, 1.0)), None);
        let ball_body = group(&ball, 0.0);
        let ball_spin = group(&ball_body, 0.0);
        shape(&ball_spin, &Path::new().ellipse((0.0, 0.0), (BR, BR)), Some((YARN, 1.0)), None);
        // a few wound strands, inside the ball
        let mut strands = Vec::new();
        for (a, r) in [(0.2f64, 9.0f64), (1.4, 12.0), (2.5, 7.0), (3.6, 11.0)] {
            let c = (a.cos() * 4.0, -(a.sin() * 4.0));
            strands.extend(arc(c, (r, r * 0.45), -a, 0.0, PI * 2.0, 48).into_iter().map(|p| (p.0.hypot(p.1) < BR - 0.4).then_some(p)));
            strands.push(None);
        }
        shape(&ball_spin, &Path::new().line(&strands, false), None, Some((STRAND, 1.7)));
        shape(&ball_spin, &Path::new().ellipse((0.0, 0.0), (BR, BR)), None, Some((INK, 2.2)));
        let shine: Vec<Option<(f64, f64)>> = arc((-4.0, 5.0), (3.5, 2.2), 0.6, 0.0, PI * 2.0, 20).into_iter().map(Some).collect();
        shape(&ball_body, &Path::new().line(&shine, true), Some((0xffffff, 0.45)), None);
        hide(&ball, true);

        Props { bed, back, front, shown_kibble: kibble.len(), kibble, battery, charge, bolt, drawn: None, ball, ball_shadow, ball_body, ball_spin }
    }

    /// Where the things are now, on a window `h` points tall.
    pub unsafe fn draw(&mut self, scene: &Scene, h: f64) {
        match scene.bed {
            Some((x, y)) => {
                put(&self.bed, x, h - y);
                hide(&self.bed, false);
            }
            None => hide(&self.bed, true),
        }
        match scene.bowl {
            Some(b) => self.bowl(&b, h),
            None => {
                hide(&self.back, true);
                hide(&self.front, true);
            }
        }
        match scene.ball {
            Some(b) => self.ball(&b, h),
            None => hide(&self.ball, true),
        }
    }

    unsafe fn bowl(&mut self, b: &BowlView, h: f64) {
        put(&self.back, b.x, h - b.y);
        put(&self.front, b.x, h - b.y);
        // its head in the bowl: the front wall over its chin; else all of it under the cat
        let _: () = msg_send![&*self.front, setZPosition: if b.front { 4.0 } else { 2.5 }];
        hide(&self.back, false);
        hide(&self.front, false);
        // the food in it is the charge
        let n = (self.kibble.len() as f64 * b.level.clamp(0.0, 1.0)).round() as usize;
        if n != self.shown_kibble {
            for (i, k) in self.kibble.iter().enumerate() {
                hide(k, i >= n);
            }
            self.shown_kibble = n;
        }
        hide(&self.battery, !b.battery);
        let state = if b.charging { 2 } else if b.low { 1 } else { 0 };
        let now = Charge { width: ((17.6 * b.level.clamp(0.0, 1.0)).max(2.0) * 2.0).round() as i32, state };
        if self.drawn == Some(now) {
            return;
        }
        let path = Path::new().rect((-9.8, 6.2), (f64::from(now.width) / 2.0, 6.6), 0.0);
        let _: () = msg_send![&*self.charge, setPath: path.0];
        if self.drawn.map(|d| d.state) != Some(state) {
            let _: () = msg_send![&*self.charge, removeAllAnimations];
            set_color(&self.charge, true, Some((if state == 2 { CHARGING } else if state == 1 { LOW } else { CHARGE }, 1.0)));
            hide(&self.bolt, state != 2);
            if state == 1 {
                // low: it blinks red
                let key = NSString::from_str("fillColor");
                let anim: Layer = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key];
                let (from, to) = (color(LOW, 1.0), color(LOW_DIM, 1.0));
                let _: () = msg_send![&*anim, setFromValue: from as *mut AnyObject];
                let _: () = msg_send![&*anim, setToValue: to as *mut AnyObject];
                CGColorRelease(from);
                CGColorRelease(to);
                let _: () = msg_send![&*anim, setDuration: 0.94f64];
                let _: () = msg_send![&*anim, setAutoreverses: true];
                let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
                let _: () = msg_send![&*self.charge, addAnimation: &*anim, forKey: &*key];
            }
        }
        self.drawn = Some(now);
    }

    unsafe fn ball(&mut self, b: &BallView, h: f64) {
        hide(&self.ball, false);
        // the shadow shrinks and fades as it flies up
        let k = (1.0 - b.z / 260.0).max(0.35);
        put(&self.ball_shadow, b.x, h - (b.y - 2.0));
        let _: () = msg_send![&*self.ball_shadow, setAffineTransform: CGAffineTransform { a: k, b: 0.0, c: 0.0, d: k, tx: 0.0, ty: 0.0 }];
        let _: () = msg_send![&*self.ball_shadow, setOpacity: (0.22 * k * b.alpha) as f32];
        put(&self.ball_body, b.x, h - (b.y - BR - b.z));
        let s = b.scale;
        let _: () = msg_send![&*self.ball_body, setAffineTransform: CGAffineTransform { a: s, b: 0.0, c: 0.0, d: s, tx: 0.0, ty: 0.0 }];
        let _: () = msg_send![&*self.ball_body, setOpacity: b.alpha as f32];
        let (sn, cs) = (-b.rot).sin_cos();
        let _: () = msg_send![&*self.ball_spin, setAffineTransform: CGAffineTransform { a: cs, b: sn, c: -sn, d: cs, tx: 0.0, ty: 0.0 }];
    }
}
