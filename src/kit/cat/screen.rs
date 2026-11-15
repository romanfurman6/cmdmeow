//! The desktop cat on screen: a clear strip along the bottom of the screen,
//! above every window, that clicks go through. The cat and the charger's
//! bowl are two layers; Core Animation plays a `desk::Script` on them, so
//! nothing is drawn per frame by the app.
//!
//! The sheets are decoded on the cat's thread (`load`), the window and the
//! animations live on the main thread (`Desk`).

use super::desk::{self, Script, Shot, CLIPS};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, MainThreadMarker};
use objc2_app_kit::{NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::ffi::c_void;
use std::path::PathBuf;

/// The feet stand this far above the bottom of the strip, in points.
const FLOOR: f64 = 6.0;
/// The bowl's picture: its size in pixels (2 a point) and where it stands.
const BOWL_PX: (f64, f64) = (180.0, 90.0);
const BOWL_FOOT: (f64, f64) = (0.5, 4.0 / 90.0);

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row_bytes: usize, space: *mut c_void, info: u32) -> *mut c_void;
    fn CGBitmapContextCreateImage(context: *mut c_void) -> *mut c_void;
    fn CGContextDrawImage(context: *mut c_void, rect: NSRect, image: *mut c_void);
    fn CGContextRelease(context: *mut c_void);
    fn CGImageGetWidth(image: *mut c_void) -> usize;
    fn CGImageGetHeight(image: *mut c_void) -> usize;
    fn CGImageCreateWithImageInRect(image: *mut c_void, rect: NSRect) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    static kCGColorSpaceSRGB: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageSourceCreateWithData(data: *const c_void, options: *const c_void) -> *mut c_void;
    fn CGImageSourceCreateImageAtIndex(source: *mut c_void, index: usize, options: *const c_void) -> *mut c_void;
}

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    fn CACurrentMediaTime() -> f64;
}

/// `sprites/<name>`: in the app (`Resources/cat`), or in the app's data
/// folder (`cat`), as the sounds are.
fn sprite_file(name: &str) -> Option<PathBuf> {
    let in_app = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/cat")));
    in_app.into_iter().chain([crate::kit::fs::data_path("cat")]).map(|dir| dir.join("sprites").join(name)).find(|p| p.is_file())
}

/// A picture file decoded once into memory (owned `CGImage`), so its
/// frames share the pixels and nothing is decoded again while it plays.
fn decode(name: &str) -> Option<*mut c_void> {
    let bytes = std::fs::read(sprite_file(name)?).ok()?;
    unsafe {
        let data = CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
        if data.is_null() {
            return None;
        }
        let source = CGImageSourceCreateWithData(data, std::ptr::null());
        CFRelease(data);
        if source.is_null() {
            return None;
        }
        let image = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null());
        CFRelease(source);
        if image.is_null() {
            return None;
        }
        let (w, h) = (CGImageGetWidth(image), CGImageGetHeight(image));
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        // 1: alpha last, premultiplied
        let context = CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, w * 4, space, 1);
        CGColorSpaceRelease(space);
        if context.is_null() {
            CGImageRelease(image);
            return None;
        }
        CGContextDrawImage(context, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)), image);
        CGImageRelease(image);
        let flat = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        (!flat.is_null()).then_some(flat)
    }
}

/// The frames of the sheets a script shows (`clip[i][f]`, owned
/// `CGImage`s, empty for sheets it does not show) and the bowl.
pub struct Frames {
    clip: Vec<Vec<*mut c_void>>,
    bowl: Option<*mut c_void>,
}

// CGImages are immutable and safe to hand from thread to thread.
unsafe impl Send for Frames {}

impl Drop for Frames {
    fn drop(&mut self) {
        for image in self.clip.iter().flatten().chain(self.bowl.iter()) {
            unsafe { CGImageRelease(*image) };
        }
    }
}

/// Decode what `script` needs (on the cat's thread: a few sheets take tens
/// of milliseconds). None when a sheet is missing.
pub fn load(script: &Script) -> Option<Frames> {
    let mut frames = Frames { clip: (0..CLIPS.len()).map(|_| Vec::new()).collect(), bowl: None };
    for i in script.clips() {
        let c = &CLIPS[i];
        let sheet = decode(&format!("{}.webp", c.name))?;
        for f in 0..c.n {
            let (x, y, w, h) = c.rect(f);
            let part = unsafe { CGImageCreateWithImageInRect(sheet, NSRect::new(NSPoint::new(x as f64, y as f64), NSSize::new(w as f64, h as f64))) };
            if !part.is_null() {
                frames.clip[i].push(part);
            }
        }
        unsafe { CGImageRelease(sheet) };
        if frames.clip[i].len() != c.n as usize {
            return None;
        }
    }
    if script.bowl.is_some() {
        frames.bowl = decode("bowl.webp");
    }
    Some(frames)
}

/// The strip, its two layers and what plays on them.
pub struct Desk {
    window: Retained<NSWindow>,
    cat: Retained<AnyObject>,
    bowl: Retained<AnyObject>,
    frames: Option<Frames>,
    /// The way out of a held act (the purr), waiting for its end.
    outro: Vec<Shot>,
    /// Counts the acts, so a late "it is over" knows it is out of date.
    pub turn: u32,
}

impl Desk {
    pub fn new(mtm: MainThreadMarker) -> Option<Desk> {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, desk::tallest_pt() + FLOOR));
        let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), rect, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setLevel(25);
        unsafe {
            let srgb: Retained<AnyObject> = msg_send![class!(NSColorSpace), sRGBColorSpace];
            let _: () = msg_send![&*window, setColorSpace: &*srgb];
        }
        window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle);
        window.setAlphaValue(0.0);
        let view = window.contentView()?;
        view.setWantsLayer(true);
        let root = view.layer()?;
        // the bowl first: the cat eats in front of it
        let [bowl, cat] = [(); 2].map(|_| unsafe {
            let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*layer, setContentsScale: desk::PX_PER_PT];
            let _: () = msg_send![&*root, addSublayer: &*layer];
            layer
        });
        Some(Desk { window, cat, bowl, frames: None, outro: Vec::new(), turn: 0 })
    }

    pub fn window(&self) -> &NSWindow {
        &self.window
    }

    /// Play `script` with `frames` along the bottom of the screen in use.
    /// Returns how long it plays before it should go (None: it holds until
    /// `end`).
    pub fn play(&mut self, mtm: MainThreadMarker, kind: super::Kind, frames: Frames) -> Option<f64> {
        self.turn = self.turn.wrapping_add(1);
        let screen = NSScreen::mainScreen(mtm)?;
        let area = screen.visibleFrame();
        let strip = NSRect::new(area.origin, NSSize::new(area.size.width, desk::tallest_pt() + FLOOR));
        self.window.setFrame_display(strip, false);
        let script = desk::script(kind, area.size.width);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*self.cat, removeAllAnimations];
            if let (Some(x), Some(image)) = (script.bowl, frames.bowl) {
                let size = NSSize::new(BOWL_PX.0 / desk::PX_PER_PT, BOWL_PX.1 / desk::PX_PER_PT);
                let _: () = msg_send![&*self.bowl, setBounds: NSRect::new(NSPoint::new(0.0, 0.0), size)];
                let _: () = msg_send![&*self.bowl, setAnchorPoint: NSPoint::new(BOWL_FOOT.0, BOWL_FOOT.1)];
                let _: () = msg_send![&*self.bowl, setPosition: NSPoint::new(x, FLOOR)];
                let _: () = msg_send![&*self.bowl, setContents: image as *mut AnyObject];
                let _: () = msg_send![&*self.bowl, setHidden: false];
            } else {
                let _: () = msg_send![&*self.bowl, setHidden: true];
            }
            let now = CACurrentMediaTime();
            if let Some(first) = script.intro.first() {
                rest_at(&self.cat, first, &frames);
            }
            if let Some(a) = group(&script.intro, &frames, now, false) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("intro")];
            }
            if let Some(a) = group(&script.hold, &frames, now + Script::secs(&script.intro), true) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("hold")];
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.window.orderFrontRegardless();
        self.frames = Some(frames);
        let held = !script.hold.is_empty();
        self.outro = script.outro;
        (!held).then(|| Script::secs(&script.intro))
    }

    /// A held act is over: play its way out. Returns how long that takes
    /// (None: nothing was held).
    pub fn end(&mut self) -> Option<f64> {
        if self.outro.is_empty() {
            return None;
        }
        let outro = std::mem::take(&mut self.outro);
        let frames = self.frames.as_ref()?;
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*self.cat, removeAllAnimations];
            if let Some(a) = group(&outro, frames, CACurrentMediaTime(), false) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("outro")];
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        Some(Script::secs(&outro))
    }

    /// Off the screen: let go of the pictures.
    pub fn clear(&mut self) {
        self.window.orderOut(None);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*self.cat, removeAllAnimations];
            let _: () = msg_send![&*self.cat, setContents: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*self.bowl, setContents: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.frames = None;
        self.outro.clear();
    }
}

/// The layer's own values at `shot`, so nothing jumps between animations.
unsafe fn rest_at(layer: &AnyObject, shot: &Shot, frames: &Frames) {
    let c = &CLIPS[shot.clip];
    let (w, h) = c.size_pt();
    let (ax, ay) = c.anchor();
    let _: () = msg_send![layer, setBounds: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h))];
    let _: () = msg_send![layer, setAnchorPoint: NSPoint::new(ax, ay)];
    let _: () = msg_send![layer, setPosition: NSPoint::new(shot.x, FLOOR)];
    if let Some(image) = frames.clip[shot.clip].get(shot.f as usize) {
        let _: () = msg_send![layer, setContents: *image as *mut AnyObject];
    }
}

/// One animation for `shots`, a frame each 1/24 s from `begin` (media
/// time): the picture, its size, where its feet are, where it stands and
/// which way it faces all step together. `repeat`: over and over.
unsafe fn group(shots: &[Shot], frames: &Frames, begin: f64, repeat: bool) -> Option<Retained<AnyObject>> {
    if shots.is_empty() {
        return None;
    }
    let n = shots.len();
    let array = || -> Retained<AnyObject> { msg_send![class!(NSMutableArray), arrayWithCapacity: n + 1] };
    let (contents, bounds, anchor, position, flip, times) = (array(), array(), array(), array(), array(), array());
    for s in shots {
        let c = &CLIPS[s.clip];
        let image = *frames.clip[s.clip].get(s.f as usize)?;
        let (w, h) = c.size_pt();
        let (ax, ay) = c.anchor();
        let rect: Retained<AnyObject> = msg_send![class!(NSValue), valueWithRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h))];
        let foot: Retained<AnyObject> = msg_send![class!(NSValue), valueWithPoint: NSPoint::new(ax, ay)];
        let at: Retained<AnyObject> = msg_send![class!(NSValue), valueWithPoint: NSPoint::new(s.x, FLOOR)];
        // the pictures face left
        let face: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: -(s.dir as f64)];
        let _: () = msg_send![&*contents, addObject: image as *mut AnyObject];
        let _: () = msg_send![&*bounds, addObject: &*rect];
        let _: () = msg_send![&*anchor, addObject: &*foot];
        let _: () = msg_send![&*position, addObject: &*at];
        let _: () = msg_send![&*flip, addObject: &*face];
    }
    // discrete steps: one more key time than values, from 0 to 1
    for i in 0..=n {
        let t: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: i as f64 / n as f64];
        let _: () = msg_send![&*times, addObject: &*t];
    }
    let secs = Script::secs(shots);
    let parts: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: 5usize];
    for (path, values) in [("contents", &contents), ("bounds", &bounds), ("anchorPoint", &anchor), ("position", &position), ("transform.scale.x", &flip)] {
        let a: Retained<AnyObject> = msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: &*NSString::from_str(path)];
        let _: () = msg_send![&*a, setValues: &**values];
        let _: () = msg_send![&*a, setKeyTimes: &*times];
        let _: () = msg_send![&*a, setCalculationMode: &*NSString::from_str("discrete")];
        let _: () = msg_send![&*a, setDuration: secs];
        let _: () = msg_send![&*parts, addObject: &*a];
    }
    let g: Retained<AnyObject> = msg_send![class!(CAAnimationGroup), animation];
    let _: () = msg_send![&*g, setAnimations: &*parts];
    let _: () = msg_send![&*g, setDuration: secs];
    let _: () = msg_send![&*g, setBeginTime: begin];
    let _: () = msg_send![&*g, setFillMode: &*NSString::from_str("forwards")];
    let _: () = msg_send![&*g, setRemovedOnCompletion: false];
    if repeat {
        let _: () = msg_send![&*g, setRepeatCount: f32::INFINITY];
    }
    Some(g)
}
