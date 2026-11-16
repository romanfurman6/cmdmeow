//! The desktop cat on screen: a clear strip along the bottom of the screen,
//! above every window, that clicks go through. The cat, the charger's bowl,
//! the tidy box and the files it carries are layers; Core Animation plays
//! a `desk::Script` on them, so nothing is drawn per frame by the app. At
//! rest the cat is one still picture.
//!
//! The sheets are decoded off the main thread (`load`), the window and the
//! animations live on the main thread (`Desk`).

use super::desk::{self, Rest, Script, Shot, CLIPS};
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
/// The tidy box (back and front halves, the same frame): size in pixels
/// and where it stands.
const BOX_PX: (f64, f64) = (215.0, 166.0);
const BOX_FOOT: (f64, f64) = (0.5, 2.0 / 96.0);
/// A file's icon is held this far down from its top (share of its height),
/// and its picture has this much empty space under it.
const BITE: f64 = 0.2;
const ICON_PAD: f64 = 0.06;
/// A file drops into the box over this many frames, shrinking to this.
const INTO_BOX: usize = 8;
const IN_BOX_SCALE: f64 = 0.55;

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
    fn CGImageRetain(image: *mut c_void) -> *mut c_void;
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
/// `CGImage`s, empty for sheets it does not show), the overlays of the
/// carrying clips, the bowl, the box and the picture the cat rests on.
pub struct Frames {
    clip: Vec<Vec<*mut c_void>>,
    over: Vec<Vec<*mut c_void>>,
    bowl: Option<*mut c_void>,
    tidy_box: Option<(*mut c_void, *mut c_void)>,
    rest: Option<*mut c_void>,
    /// A clear pixel: the overlay in frames that have none.
    clear: Option<*mut c_void>,
}

// CGImages are immutable and safe to hand from thread to thread.
unsafe impl Send for Frames {}

impl Drop for Frames {
    fn drop(&mut self) {
        let boxes = self.tidy_box.iter().flat_map(|(a, b)| [a, b]);
        for image in self.clip.iter().flatten().chain(self.over.iter().flatten()).chain(self.bowl.iter()).chain(boxes).chain(self.rest.iter()).chain(self.clear.iter()) {
            unsafe { CGImageRelease(*image) };
        }
    }
}

impl Frames {
    /// Takes the resting picture out (the rest is let go of with the frames).
    pub fn take_rest(&mut self) -> Option<Image> {
        self.rest.take().map(Image)
    }
}

/// One still picture (an owned `CGImage`).
pub struct Image(*mut c_void);

unsafe impl Send for Image {}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { CGImageRelease(self.0) };
    }
}

/// A copy of `part` in its own pixels, so the sheet it was cut from can go.
fn own(part: *mut c_void) -> Option<*mut c_void> {
    unsafe {
        let (w, h) = (CGImageGetWidth(part), CGImageGetHeight(part));
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        let context = CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, w * 4, space, 1);
        CGColorSpaceRelease(space);
        if context.is_null() {
            return None;
        }
        CGContextDrawImage(context, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)), part);
        let copy = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        (!copy.is_null()).then_some(copy)
    }
}

/// Cut a sheet into its frames (None when it does not have them all).
fn cut(sheet: *mut c_void, c: &desk::Clip) -> Option<Vec<*mut c_void>> {
    let mut out = Vec::with_capacity(c.n as usize);
    for f in 0..c.n {
        let (x, y, w, h) = c.rect(f);
        let part = unsafe { CGImageCreateWithImageInRect(sheet, NSRect::new(NSPoint::new(x as f64, y as f64), NSSize::new(w as f64, h as f64))) };
        if part.is_null() {
            for p in out {
                unsafe { CGImageRelease(p) };
            }
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// Decode what `script` needs (off the main thread: a few sheets take tens
/// of milliseconds). None when a sheet is missing.
pub fn load(script: &Script) -> Option<Frames> {
    let n = CLIPS.len();
    let mut frames = Frames { clip: (0..n).map(|_| Vec::new()).collect(), over: (0..n).map(|_| Vec::new()).collect(), bowl: None, tidy_box: None, rest: None, clear: None };
    let rest = script.rest.shot(1000.0);
    let mut wanted = script.clips();
    if let Some(r) = rest {
        if !wanted.contains(&r.clip) {
            wanted.push(r.clip);
        }
    }
    // each sheet on its own thread: the act starts (and its sound plays)
    // when the slowest is in, not after all of them in a row
    type Parts = Option<Vec<usize>>;
    let sheet = |name: String, i: usize| -> Parts {
        let c = &CLIPS[i];
        let sheet = decode(&name)?;
        let parts = cut(sheet, c);
        unsafe { CGImageRelease(sheet) };
        Some(parts?.into_iter().map(|p| p as usize).collect())
    };
    let done: Vec<(usize, Parts, Option<Parts>)> = std::thread::scope(|scope| {
        let jobs: Vec<_> = wanted
            .iter()
            .map(|&i| {
                let name = CLIPS[i].name;
                let main = scope.spawn(move || sheet(format!("{name}.webp"), i));
                let over = desk::has_overlay(i).then(|| scope.spawn(move || sheet(format!("{name}_o.webp"), i)));
                (i, main, over)
            })
            .collect();
        jobs.into_iter().map(|(i, main, over)| (i, main.join().ok().flatten(), over.map(|o| o.join().ok().flatten()))).collect()
    });
    let mut missing = false;
    for (i, main, over) in done {
        match main {
            Some(parts) => frames.clip[i] = parts.into_iter().map(|p| p as *mut c_void).collect(),
            None => missing = true,
        }
        match over {
            Some(Some(parts)) => frames.over[i] = parts.into_iter().map(|p| p as *mut c_void).collect(),
            Some(None) => missing = true,
            None => {}
        }
    }
    if missing {
        // `frames` lets go of what did come in
        return None;
    }
    if let Some(r) = rest {
        frames.rest = own(*frames.clip[r.clip].get(r.f as usize)?);
        if !script.clips().contains(&r.clip) {
            // only the one picture was wanted from that sheet
            for p in std::mem::take(&mut frames.clip[r.clip]) {
                unsafe { CGImageRelease(p) };
            }
        }
    }
    if script.bowl.is_some() {
        frames.bowl = decode("bowl.webp");
    }
    if script.tidy_box.is_some() {
        frames.tidy_box = Some((decode("box_back.webp")?, decode("box_front.webp")?));
    }
    if script.clips().iter().any(|i| desk::has_overlay(*i)) {
        frames.clear = clear_pixel();
    }
    Some(frames)
}

/// Seconds from `t` (since the act began) to where its loop next starts
/// again: the end of the intro, or of the round under way.
pub fn until_loop_ends(t: f64, intro: f64, hold: f64) -> f64 {
    if t < intro || hold <= 0.0 {
        return (intro - t).max(0.0);
    }
    let into = (t - intro) % hold;
    hold - into
}

/// A one-pixel clear picture.
fn clear_pixel() -> Option<*mut c_void> {
    unsafe {
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        let context = CGBitmapContextCreate(std::ptr::null_mut(), 1, 1, 8, 4, space, 1);
        CGColorSpaceRelease(space);
        if context.is_null() {
            return None;
        }
        let image = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        (!image.is_null()).then_some(image)
    }
}

/// A file the tidy carries: its icon and where its middle is on screen,
/// in the window's points (from the bottom left of the usable area).
pub struct File {
    pub icon: Image,
    pub at: NSPoint,
}

/// The icon Finder shows for the file at `path` (on the main thread).
pub fn icon_for(path: &std::path::Path) -> Option<Image> {
    let px = desk::ICON_PT * desk::PX_PER_PT;
    unsafe {
        let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let path = NSString::from_str(&path.to_string_lossy());
        let image: Option<Retained<AnyObject>> = msg_send![&*ws, iconForFile: &*path];
        let image = image?;
        let _: () = msg_send![&*image, setSize: NSSize::new(px, px)];
        let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px, px));
        let cg: *mut c_void = msg_send![&*image, CGImageForProposedRect: &mut rect as *mut NSRect, context: std::ptr::null::<AnyObject>(), hints: std::ptr::null::<AnyObject>()];
        // not ours until retained
        (!cg.is_null()).then(|| Image(CGImageRetain(cg)))
    }
}

/// The main screen's usable area (above the Dock, below the menu bar).
pub fn area(mtm: MainThreadMarker) -> Option<NSRect> {
    NSScreen::mainScreen(mtm).map(|s| s.visibleFrame())
}

/// The strip, its layers and what plays on them.
pub struct Desk {
    window: Retained<NSWindow>,
    cat: Retained<AnyObject>,
    /// The nose, mouth and whiskers over a carried file.
    mouth: Retained<AnyObject>,
    bowl: Retained<AnyObject>,
    box_back: Retained<AnyObject>,
    box_front: Retained<AnyObject>,
    /// The files of a tidy (between the box's halves).
    files: Vec<Retained<AnyObject>>,
    frames: Option<Frames>,
    /// The way out of a held act (the purr, the play), waiting for its end.
    outro: Vec<Shot>,
    /// When the act began (media time), its intro's and held loop's length:
    /// the way out starts where the loop starts again, never mid-move.
    began: f64,
    intro_secs: f64,
    hold_secs: f64,
    /// Counts the acts, so a late "it is over" knows it is out of date.
    pub turn: u32,
    /// How the cat rests now, and on which picture.
    pub rest: Rest,
    still: Option<Image>,
}

impl Desk {
    pub fn new(mtm: MainThreadMarker) -> Option<Desk> {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, strip_height()));
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
        // back to front: the bowl (the cat eats in front of it), the box's
        // back, the cat, the files (added later, under the box's front), the
        // box's front, the mouth over a carried file
        let [bowl, box_back, cat, box_front, mouth] = [(); 5].map(|_| unsafe { layer(&root) });
        Some(Desk { window, cat, mouth, bowl, box_back, box_front, files: Vec::new(), frames: None, outro: Vec::new(), began: 0.0, intro_secs: 0.0, hold_secs: 0.0, turn: 0, rest: Rest::Away, still: None })
    }

    pub fn window(&self) -> &NSWindow {
        &self.window
    }

    /// Where the cat is at rest, for a pointer to find it: AppKit points.
    pub fn hot(&self) -> Option<NSRect> {
        let shot = self.rest.shot(self.window.frame().size.width)?;
        let (w, h) = if self.rest == Rest::Sleep { (100.0, 70.0) } else { (80.0, 110.0) };
        let f = self.window.frame();
        Some(NSRect::new(NSPoint::new(f.origin.x + shot.x - w / 2.0, f.origin.y + FLOOR), NSSize::new(w, h)))
    }

    /// The window along the bottom of `area`: a strip, or all of it (a
    /// tidy, for the files to fall from where they lie).
    fn place(&self, area: NSRect, tall: bool) {
        let h = if tall { area.size.height } else { strip_height() };
        self.window.setFrame_display(NSRect::new(area.origin, NSSize::new(area.size.width, h)), false);
    }

    /// Sit (or sleep) still in the corner of `area`, on `still`.
    pub fn settle(&mut self, area: NSRect, rest: Rest, still: Option<Image>) {
        self.turn = self.turn.wrapping_add(1);
        if let Some(i) = still {
            self.still = Some(i);
        }
        self.rest = rest;
        self.place(area, false);
        let shot = rest.shot(area.size.width);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            for l in [&self.cat, &self.mouth, &self.bowl, &self.box_back, &self.box_front] {
                let _: () = msg_send![&**l, removeAllAnimations];
            }
            for l in [&self.mouth, &self.bowl, &self.box_back, &self.box_front] {
                let _: () = msg_send![&**l, setHidden: true];
            }
            for l in self.files.drain(..) {
                let _: () = msg_send![&*l, removeFromSuperlayer];
            }
            if let (Some(shot), Some(still)) = (shot, &self.still) {
                place_cat(&self.cat, &shot);
                let _: () = msg_send![&*self.cat, setContents: still.0 as *mut AnyObject];
                face(&self.cat, shot.dir);
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        // the sheets are let go of: at rest only the one picture is kept
        self.frames = None;
        self.outro.clear();
        self.window.orderFrontRegardless();
        self.window.setAlphaValue(1.0);
    }

    /// Play `script` with `frames` along the bottom of `area`; `files`: what
    /// a tidy carries, in the order of `script.fetch`. Returns how long it
    /// plays (None: it holds until `end`).
    pub fn play(&mut self, area: NSRect, script: &Script, mut frames: Frames, files: Vec<File>) -> Option<f64> {
        self.turn = self.turn.wrapping_add(1);
        if let Some(i) = frames.take_rest() {
            self.still = Some(i);
        }
        let tidy = script.tidy_box.is_some();
        let secs = Script::secs(&script.intro);
        self.place(area, tidy);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            for l in [&self.cat, &self.mouth, &self.bowl, &self.box_back, &self.box_front] {
                let _: () = msg_send![&**l, removeAllAnimations];
            }
            for l in self.files.drain(..) {
                let _: () = msg_send![&*l, removeFromSuperlayer];
            }
            face(&self.cat, -1);
            if let (Some(x), Some(image)) = (script.bowl, frames.bowl) {
                still_at(&self.bowl, image, BOWL_PX, BOWL_FOOT, x);
            } else {
                let _: () = msg_send![&*self.bowl, setHidden: true];
            }
            let now = CACurrentMediaTime();
            match (script.tidy_box, frames.tidy_box) {
                (Some(x), Some((back, front))) => {
                    still_at(&self.box_back, back, BOX_PX, BOX_FOOT, x);
                    still_at(&self.box_front, front, BOX_PX, BOX_FOOT, x);
                    for l in [&self.box_back, &self.box_front] {
                        let _: () = msg_send![&**l, setOpacity: 0.0f32];
                        let _: () = msg_send![&**l, addAnimation: &*come_and_go(now, secs), forKey: &*NSString::from_str("box")];
                    }
                }
                _ => {
                    let _: () = msg_send![&*self.box_back, setHidden: true];
                    let _: () = msg_send![&*self.box_front, setHidden: true];
                }
            }
            if let Some(first) = script.intro.first() {
                rest_at(&self.cat, first, &frames);
            }
            if let Some(a) = group(&script.intro, &frames, now, false, false) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("intro")];
            }
            if let Some(a) = group(&script.hold, &frames, now + secs, true, false) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("hold")];
            }
            if frames.clear.is_some() {
                let _: () = msg_send![&*self.mouth, setHidden: false];
                let _: () = msg_send![&*self.mouth, setContents: std::ptr::null::<AnyObject>()];
                if let Some(a) = group(&script.intro, &frames, now, false, true) {
                    let _: () = msg_send![&*self.mouth, addAnimation: &*a, forKey: &*NSString::from_str("intro")];
                }
            } else {
                let _: () = msg_send![&*self.mouth, setHidden: true];
            }
            if let Some(bx) = script.tidy_box {
                for (fetch, file) in script.fetch.iter().zip(files) {
                    let l = layer_below(&self.box_front);
                    if let Some(a) = carried(&l, fetch, file, &script.intro, bx, now) {
                        let _: () = msg_send![&*l, addAnimation: &*a, forKey: &*NSString::from_str("file")];
                    }
                    self.files.push(l);
                }
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.window.orderFrontRegardless();
        self.frames = Some(frames);
        let held = !script.hold.is_empty();
        self.outro = script.outro.clone();
        self.began = unsafe { CACurrentMediaTime() };
        self.intro_secs = secs;
        self.hold_secs = Script::secs(&script.hold);
        (!held).then_some(secs)
    }

    /// A held act is over: play its way out, from the end of the intro or
    /// of the loop under way (cutting in would make the cat jump). Returns
    /// (seconds until the way out starts, seconds until it is over); None:
    /// nothing was held.
    pub fn end(&mut self) -> Option<(f64, f64)> {
        if self.outro.is_empty() {
            return None;
        }
        let outro = std::mem::take(&mut self.outro);
        let frames = self.frames.as_ref()?;
        let now = unsafe { CACurrentMediaTime() };
        let wait = until_loop_ends(now - self.began, self.intro_secs, self.hold_secs);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            // added last, it covers the loop once it begins
            if let Some(a) = group(&outro, frames, now + wait, false, false) {
                let _: () = msg_send![&*self.cat, addAnimation: &*a, forKey: &*NSString::from_str("outro")];
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        Some((wait, wait + Script::secs(&outro)))
    }

    /// Something is held now (and `end` will play its way out).
    pub fn holding(&self) -> bool {
        !self.outro.is_empty()
    }

    /// Off the screen: let go of the pictures.
    pub fn clear(&mut self) {
        self.turn = self.turn.wrapping_add(1);
        self.window.orderOut(None);
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            for l in [&self.cat, &self.mouth, &self.bowl, &self.box_back, &self.box_front] {
                let _: () = msg_send![&**l, removeAllAnimations];
                let _: () = msg_send![&**l, setContents: std::ptr::null::<AnyObject>()];
            }
            for l in self.files.drain(..) {
                let _: () = msg_send![&*l, removeFromSuperlayer];
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.frames = None;
        self.outro.clear();
        self.rest = Rest::Away;
        self.still = None;
    }
}

/// How tall the strip is: the tallest clip and the floor under it.
fn strip_height() -> f64 {
    desk::tallest_pt() + FLOOR
}

/// A new layer at the top of `root`, at 2 pixels a point.
unsafe fn layer(root: &AnyObject) -> Retained<AnyObject> {
    let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*layer, setContentsScale: desk::PX_PER_PT];
    let _: () = msg_send![root, addSublayer: &*layer];
    layer
}

/// A new layer just under `above`.
unsafe fn layer_below(above: &AnyObject) -> Retained<AnyObject> {
    let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*layer, setContentsScale: desk::PX_PER_PT];
    let root: Option<Retained<AnyObject>> = msg_send![above, superlayer];
    if let Some(root) = root {
        let _: () = msg_send![&*root, insertSublayer: &*layer, below: above];
    }
    layer
}

/// A picture `px` pixels big standing with its `foot` at `x` on the floor.
unsafe fn still_at(layer: &AnyObject, image: *mut c_void, px: (f64, f64), foot: (f64, f64), x: f64) {
    let size = NSSize::new(px.0 / desk::PX_PER_PT, px.1 / desk::PX_PER_PT);
    let _: () = msg_send![layer, setBounds: NSRect::new(NSPoint::new(0.0, 0.0), size)];
    let _: () = msg_send![layer, setAnchorPoint: NSPoint::new(foot.0, foot.1)];
    let _: () = msg_send![layer, setPosition: NSPoint::new(x, FLOOR)];
    let _: () = msg_send![layer, setContents: image as *mut AnyObject];
    let _: () = msg_send![layer, setHidden: false];
}

/// The cat's size, feet and place for `shot` (not its picture).
unsafe fn place_cat(layer: &AnyObject, shot: &Shot) {
    let c = &CLIPS[shot.clip];
    let (w, h) = c.size_pt();
    let (ax, ay) = c.anchor();
    let _: () = msg_send![layer, setBounds: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h))];
    let _: () = msg_send![layer, setAnchorPoint: NSPoint::new(ax, ay)];
    let _: () = msg_send![layer, setPosition: NSPoint::new(shot.x, FLOOR)];
}

/// The pictures face left: a cat facing right is mirrored.
unsafe fn face(layer: &AnyObject, dir: i8) {
    let k: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: -(dir as f64)];
    let _: () = msg_send![layer, setValue: &*k, forKeyPath: &*NSString::from_str("transform.scale.x")];
}

/// The box fades in at the start of a tidy and out at its end.
unsafe fn come_and_go(begin: f64, secs: f64) -> Retained<AnyObject> {
    let fade = (0.25 / secs.max(0.5)).min(0.2);
    let a: Retained<AnyObject> = msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
    let values: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: 4usize];
    let times: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: 4usize];
    for (v, t) in [(0.0, 0.0), (1.0, fade), (1.0, 1.0 - fade), (0.0, 1.0)] {
        let v: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: v];
        let t: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: t];
        let _: () = msg_send![&*values, addObject: &*v];
        let _: () = msg_send![&*times, addObject: &*t];
    }
    let _: () = msg_send![&*a, setValues: &*values];
    let _: () = msg_send![&*a, setKeyTimes: &*times];
    let _: () = msg_send![&*a, setDuration: secs];
    let _: () = msg_send![&*a, setBeginTime: begin];
    let _: () = msg_send![&*a, setFillMode: &*NSString::from_str("forwards")];
    let _: () = msg_send![&*a, setRemovedOnCompletion: false];
    a
}

/// One file of a tidy: it falls from where it lies, waits on the floor,
/// rides in the mouth, and drops into the box at `bx`. Hidden before and
/// after (its own opacity stays 0).
unsafe fn carried(layer: &AnyObject, fetch: &desk::Fetch, file: File, shots: &[Shot], bx: f64, begin: f64) -> Option<Retained<AnyObject>> {
    let s = desk::ICON_PT;
    let _: () = msg_send![layer, setBounds: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(s, s))];
    let _: () = msg_send![layer, setAnchorPoint: NSPoint::new(0.5, 1.0 - BITE)];
    let _: () = msg_send![layer, setOpacity: 0.0f32];
    let _: () = msg_send![layer, setContents: file.icon.0 as *mut AnyObject];
    // where the bite point is when the icon lies on the floor
    let ground = FLOOR + s * (1.0 - BITE - ICON_PAD);
    let start = NSPoint::new(file.at.x, file.at.y + s * (0.5 - BITE));
    let end = fetch.drop + INTO_BOX;
    let n = end.checked_sub(fetch.fall)?;
    let array = || -> Retained<AnyObject> { msg_send![class!(NSMutableArray), arrayWithCapacity: n + 1] };
    let (position, spin, scale, times) = (array(), array(), array(), array());
    let mut last = (start, 0.0);
    for i in fetch.fall..end {
        let (at, lean, k) = if i < fetch.fall + desk::FALL_FRAMES {
            // falls, faster and faster
            let t = (i + 1 - fetch.fall) as f64 / desk::FALL_FRAMES as f64;
            (NSPoint::new(start.x + (fetch.x - start.x) * t, start.y + (ground - start.y) * t * t), 0.0, 1.0)
        } else if i < fetch.pick {
            (NSPoint::new(fetch.x, ground), 0.0, 1.0)
        } else if i <= fetch.drop {
            let shot = shots.get(i)?;
            match desk::mouth(shot.clip, shot.f) {
                Some((mx, my, lean)) => (NSPoint::new(shot.x + shot.dir as f64 * mx, FLOOR + my.max(ground - FLOOR)), lean * shot.dir as f64, 1.0),
                None => (last.0, last.1, 1.0),
            }
        } else {
            // into the box
            let t = (i - fetch.drop) as f64 / INTO_BOX as f64;
            let from = last.0;
            let to = NSPoint::new(bx, FLOOR + 30.0);
            (NSPoint::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t), last.1 * (1.0 - t), 1.0 - (1.0 - IN_BOX_SCALE) * t)
        };
        if i <= fetch.drop {
            last = (at, lean);
        }
        let p: Retained<AnyObject> = msg_send![class!(NSValue), valueWithPoint: at];
        let r: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: lean.to_radians()];
        let k: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: k];
        let _: () = msg_send![&*position, addObject: &*p];
        let _: () = msg_send![&*spin, addObject: &*r];
        let _: () = msg_send![&*scale, addObject: &*k];
    }
    for i in 0..=n {
        let t: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: i as f64 / n as f64];
        let _: () = msg_send![&*times, addObject: &*t];
    }
    let shown: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
    let one: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithDouble: 1.0f64];
    let _: () = msg_send![&*shown, setFromValue: &*one];
    let _: () = msg_send![&*shown, setToValue: &*one];
    let secs = n as f64 / desk::FPS;
    let _: () = msg_send![&*shown, setDuration: secs];
    let parts: Retained<AnyObject> = msg_send![class!(NSMutableArray), arrayWithCapacity: 4usize];
    let _: () = msg_send![&*parts, addObject: &*shown];
    for (path, values) in [("position", &position), ("transform.rotation.z", &spin), ("transform.scale", &scale)] {
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
    let _: () = msg_send![&*g, setBeginTime: begin + fetch.fall as f64 / desk::FPS];
    // before it falls and once it is in the box, the layer's own opacity (0) shows
    let _: () = msg_send![&*g, setFillMode: &*NSString::from_str("removed")];
    let _: () = msg_send![&*g, setRemovedOnCompletion: false];
    Some(g)
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
/// `over`: the overlays of the carrying clips instead of the cat.
unsafe fn group(shots: &[Shot], frames: &Frames, begin: f64, repeat: bool, over: bool) -> Option<Retained<AnyObject>> {
    if shots.is_empty() {
        return None;
    }
    let n = shots.len();
    let array = || -> Retained<AnyObject> { msg_send![class!(NSMutableArray), arrayWithCapacity: n + 1] };
    let (contents, bounds, anchor, position, flip, times) = (array(), array(), array(), array(), array(), array());
    for s in shots {
        let c = &CLIPS[s.clip];
        let image = if !over {
            *frames.clip[s.clip].get(s.f as usize)?
        } else if let Some(o) = frames.over[s.clip].get(s.f as usize) {
            *o
        } else {
            frames.clear?
        };
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
