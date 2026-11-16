//! The live desktop cat on screen: a see-through window over the first
//! screen, on the desk under every app's window and just above the icons,
//! with one Core Animation layer per part of the cat, posed each frame
//! from `live`, and its things; and, at the desktop icons' own level, the pictures
//! of the icons it pushes while they move (Finder takes over once each one
//! comes to rest).

use super::grid::{self, Spot};
use super::live::{Live, Scene};
use super::props::Props;
use super::rig::Rig;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, AnyThread, MainThreadMarker};
use objc2_app_kit::{NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_core_foundation::{CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSError, NSPoint, NSRect, NSSize, NSString};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row_bytes: usize, space: *mut c_void, info: u32) -> *mut c_void;
    fn CGBitmapContextCreateImage(context: *mut c_void) -> *mut c_void;
    fn CGBitmapContextGetData(context: *mut c_void) -> *mut u8;
    fn CGContextDrawImage(context: *mut c_void, rect: NSRect, image: *mut c_void);
    fn CGContextRelease(context: *mut c_void);
    fn CGImageGetWidth(image: *mut c_void) -> usize;
    fn CGImageGetHeight(image: *mut c_void) -> usize;
    fn CGImageCreateWithImageInRect(image: *mut c_void, rect: NSRect) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGImageRetain(image: *mut c_void) -> *mut c_void;
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
    fn CGPathCreateMutable() -> *mut c_void;
    fn CGPathMoveToPoint(path: *mut c_void, m: *const c_void, x: f64, y: f64);
    fn CGPathAddQuadCurveToPoint(path: *mut c_void, m: *const c_void, cx: f64, cy: f64, x: f64, y: f64);
    fn CGPathCloseSubpath(path: *mut c_void);
    fn CGPathAddEllipseInRect(path: *mut c_void, m: *const c_void, rect: NSRect);
    fn CGPathRelease(path: *mut c_void);
    fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> *mut c_void;
    fn CGColorRelease(color: *mut c_void);
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

/// `kCGDesktopIconWindowLevelKey`
const DESKTOP_ICONS: i32 = 18;
/// Windows from here up are the Dock, menus and the like, not apps'
/// (`kCGDockWindowLevel`).
const DOCK_LEVEL: i64 = 20;
/// Owners of system windows that lie over the desk but are not apps.
const SYSTEM: [&str; 5] = ["Dock", "Window Server", "WindowManager", "Control Center", "Notification Center"];

/// An owned `CGImage`.
pub struct Image(pub *mut c_void);

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { CGImageRelease(self.0) }
    }
}

unsafe impl Send for Image {}

/// `cat/<rel>`: in the app (`Resources/cat`), or in the app's data folder.
pub fn asset(rel: &str) -> Option<PathBuf> {
    let in_app = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/cat")));
    in_app.into_iter().chain([crate::kit::fs::data_path("cat")]).map(|dir| dir.join(rel)).find(|p| p.is_file())
}

/// A picture file decoded once into memory, premultiplied sRGB.
fn decode(path: &std::path::Path) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
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
        let flat = flatten(image);
        CGImageRelease(image);
        flat
    }
}

/// The picture redrawn into plain premultiplied sRGB pixels (owned).
unsafe fn flatten(image: *mut c_void) -> Option<Image> {
    let (w, h) = (CGImageGetWidth(image), CGImageGetHeight(image));
    let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    // 1: alpha last, premultiplied
    let context = CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, w * 4, space, 1);
    CGColorSpaceRelease(space);
    if context.is_null() {
        return None;
    }
    CGContextDrawImage(context, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)), image);
    let flat = CGBitmapContextCreateImage(context);
    CGContextRelease(context);
    (!flat.is_null()).then_some(Image(flat))
}

/// The cat's brain and its parts' pictures by file.
pub type Loaded = (Live, HashMap<[u16; 2], Image>);

/// A picture of part of the screen, the point at its top left (first
/// screen points, down from its top) and its pixels per point.
pub type Shot = (Image, (f64, f64), f64);

/// The skeleton and its pictures, read once (off the main thread).
pub fn load(seed: u32) -> Option<Loaded> {
    let json = std::fs::read_to_string(asset("rig/rig.json")?).ok()?;
    let rig = Rig::parse(&json).ok()?;
    let mut images = HashMap::new();
    let mut sizes = HashMap::new();
    for (key, f) in &rig.files {
        let Some(img) = asset(&format!("rig/parts/{}", f.img)).and_then(|p| decode(&p)) else { continue };
        let (w, h) = unsafe { (CGImageGetWidth(img.0) as f64, CGImageGetHeight(img.0) as f64) };
        sizes.insert(*key, (w, h));
        images.insert(*key, img);
    }
    Some((Live::new(rig, sizes, seed)?, images))
}

/// A part layer's picture-to-screen map, from the y-down map of `live`
/// (`m`, picture pixels down from its top) to AppKit's y-up layer space
/// of a window `h` points tall.
fn up(m: &[f64; 6], ih: f64, h: f64) -> CGAffineTransform {
    CGAffineTransform { a: m[0], b: -m[1], c: -m[2], d: m[3], tx: m[2] * ih + m[4], ty: h - m[3] * ih - m[5] }
}

/// One pushed icon: its picture, and the wallpaper over its old spot.
struct Proxy {
    icon: Retained<AnyObject>,
    patch: Option<Retained<AnyObject>>,
}

/// What Finder showed when the tidy began, and pictures of it.
pub struct Desktop {
    pub spots: Vec<Spot>,
    pub view: grid::View,
    pub pitch: (f64, f64),
    /// Finder's position minus the middle of the icon's picture
    pub off: (f64, f64),
    /// the icons alone (see-through around them) and the wallpaper alone,
    /// with the point of the screen at their top left and pixels a point
    pub icons: Option<Shot>,
    pub paper: Option<Shot>,
}

unsafe impl Send for Desktop {}

impl Desktop {
    /// The rectangle an icon and its label take, around its picture's middle
    /// (points, top left origin).
    pub fn cell(&self, mid: (f64, f64)) -> (f64, f64, f64, f64) {
        let s = self.view.icon;
        let label = self.view.text * 2.6 + 8.0;
        let w = (self.pitch.0 - 2.0).max(s + 8.0);
        (mid.0 - w / 2.0, mid.1 - s / 2.0 - 6.0, w, s + 12.0 + label)
    }
}

pub struct Stage {
    window: Retained<NSWindow>,
    desk: Retained<NSWindow>,
    /// on the desk window: the wallpaper patches, under the icons' pictures
    patches: Retained<AnyObject>,
    moved: Retained<AnyObject>,
    /// the cat: its shadow and its parts, between its things
    body: Retained<AnyObject>,
    shadow: Retained<AnyObject>,
    props: Props,
    parts: Vec<(Retained<AnyObject>, Option<[u16; 2]>)>,
    claws: Retained<AnyObject>,
    claws_on: Option<usize>,
    claws_k: f64,
    images: HashMap<[u16; 2], Image>,
    pub live: Live,
    /// the first screen, AppKit points
    frame: NSRect,
    proxies: HashMap<usize, Proxy>,
    pub desktop: Option<Desktop>,
    through: bool,
}

unsafe fn new_layer(root: &AnyObject) -> Retained<AnyObject> {
    let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*layer, setAnchorPoint: CGPoint { x: 0.0, y: 0.0 }];
    let _: () = msg_send![&*layer, setPosition: CGPoint { x: 0.0, y: 0.0 }];
    let _: () = msg_send![root, addSublayer: &*layer];
    layer
}

unsafe fn shape(root: &AnyObject, rgba: [f64; 4]) -> Retained<AnyObject> {
    let layer: Retained<AnyObject> = msg_send![class!(CAShapeLayer), layer];
    let color = CGColorCreateSRGB(rgba[0], rgba[1], rgba[2], rgba[3]);
    let _: () = msg_send![&*layer, setFillColor: color];
    CGColorRelease(color);
    let _: () = msg_send![root, addSublayer: &*layer];
    layer
}

/// A see-through window over `frame`. The cat's own catches clicks on the
/// cat, so it is a panel that does not bring the app forward.
fn make_window(mtm: MainThreadMarker, frame: NSRect, level: isize, panel: bool) -> Option<(Retained<NSWindow>, Retained<AnyObject>)> {
    let window = if panel {
        // NSWindowStyleMaskNonactivatingPanel
        let style = NSWindowStyleMask(NSWindowStyleMask::Borderless.0 | (1 << 7));
        unsafe {
            let alloc: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSPanel), alloc];
            let p: Option<Retained<AnyObject>> = msg_send![alloc, initWithContentRect: frame, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false];
            let p = p?;
            let _: () = msg_send![&*p, setBecomesKeyOnlyIfNeeded: true];
            let _: () = msg_send![&*p, setFloatingPanel: false];
            let _: () = msg_send![&*p, setHidesOnDeactivate: false];
            Retained::cast_unchecked::<NSWindow>(p)
        }
    } else {
        unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) }
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setIgnoresMouseEvents(true);
    window.setLevel(level);
    unsafe {
        let srgb: Retained<AnyObject> = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let _: () = msg_send![&*window, setColorSpace: &*srgb];
    }
    window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle);
    window.setFrame_display(frame, false);
    let view = window.contentView()?;
    view.setWantsLayer(true);
    let root: Retained<AnyObject> = unsafe { msg_send![&*view, layer] };
    Some((window, root))
}

/// The first screen (the one with the menu bar: Finder counts from its top
/// left) and its usable part, as (x0, y0, x1, y1) down from its top.
pub fn first_screen(mtm: MainThreadMarker) -> Option<(NSRect, (f64, f64, f64, f64))> {
    let s = NSScreen::screens(mtm).firstObject()?;
    let (f, v) = (s.frame(), s.visibleFrame());
    let top = f.origin.y + f.size.height;
    Some((f, (v.origin.x - f.origin.x, top - (v.origin.y + v.size.height), v.origin.x - f.origin.x + v.size.width, top - v.origin.y)))
}

impl Stage {
    pub fn new(mtm: MainThreadMarker, loaded: Loaded) -> Option<Stage> {
        let (live, images) = loaded;
        let (frame, ground) = first_screen(mtm)?;
        // on the desk, under every window: just above the icons (and the
        // pictures of the icons it pushes)
        let icon_level = unsafe { CGWindowLevelForKey(DESKTOP_ICONS) } as isize + 1;
        let (window, root) = make_window(mtm, frame, icon_level + 1, true)?;
        let (desk, droot) = make_window(mtm, frame, icon_level, false)?;
        let (patches, moved) = unsafe { (new_layer(&droot), new_layer(&droot)) };
        let (body, shadow, props, claws) = unsafe {
            let body = new_layer(&root);
            let _: () = msg_send![&*body, setZPosition: 3.0f64];
            let shadow = shape(&body, [0.0, 0.0, 0.0, 0.2]);
            let props = Props::new(&root);
            let claws = shape(&body, [0.957, 0.965, 0.937, 1.0]);
            let ink = CGColorCreateSRGB(0.04, 0.06, 0.035, 1.0);
            let _: () = msg_send![&*claws, setStrokeColor: ink];
            CGColorRelease(ink);
            let _: () = msg_send![&*claws, setHidden: true];
            (body, shadow, props, claws)
        };
        let mut live = live;
        live.set_area(frame.size.width, frame.size.height, ground);
        // the desk window lies under the cat's, both see-through
        desk.orderFrontRegardless();
        window.orderFrontRegardless();
        Some(Stage { window, desk, patches, moved, body, shadow, props, parts: Vec::new(), claws, claws_on: None, claws_k: 0.0, images, live, frame, proxies: HashMap::new(), desktop: None, through: true })
    }

    /// The first screen's size, points.
    pub fn size(&self) -> (f64, f64) {
        (self.frame.size.width, self.frame.size.height)
    }

    /// The screens changed: the windows follow the first one.
    pub fn refit(&mut self, mtm: MainThreadMarker) {
        let Some((frame, ground)) = first_screen(mtm) else { return };
        if frame.size.width != self.frame.size.width || frame.size.height != self.frame.size.height || frame.origin.x != self.frame.origin.x || frame.origin.y != self.frame.origin.y {
            self.frame = frame;
            self.window.setFrame_display(frame, false);
            self.desk.setFrame_display(frame, false);
        }
        self.live.set_area(frame.size.width, frame.size.height, ground);
    }

    /// Draw a scene (one Core Animation transaction, no implicit animation).
    pub fn draw(&mut self, scene: &Scene) {
        let h = self.frame.size.height;
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            // grow the pool of part layers as needed
            while self.parts.len() < scene.parts.len() {
                let l = new_layer(&self.body);
                self.parts.push((l, None));
            }
            let mut claws_at = None;
            for (i, p) in scene.parts.iter().enumerate() {
                let Some(img) = self.images.get(&p.file) else { continue };
                let (layer, shown) = &mut self.parts[i];
                let ih = CGImageGetHeight(img.0) as f64;
                if *shown != Some(p.file) {
                    let iw = CGImageGetWidth(img.0) as f64;
                    let _: () = msg_send![&**layer, setContents: img.0 as *mut AnyObject];
                    let _: () = msg_send![&**layer, setBounds: CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: iw, height: ih } }];
                    *shown = Some(p.file);
                }
                let _: () = msg_send![&**layer, setHidden: false];
                let _: () = msg_send![&**layer, setOpacity: p.alpha as f32];
                let _: () = msg_send![&**layer, setAffineTransform: up(&p.m, ih, h)];
                if p.claws > 0.02 {
                    claws_at = Some((i, ih, CGImageGetWidth(img.0) as f64, p.claws));
                }
            }
            for (layer, _) in self.parts.iter().skip(scene.parts.len()) {
                let _: () = msg_send![&**layer, setHidden: true];
            }
            self.draw_claws(claws_at);
            // the shadow under the feet; the bed, the bowl, the ball
            match scene.feet {
                Some((x, y)) => {
                    let path = CGPathCreateMutable();
                    CGPathAddEllipseInRect(path, std::ptr::null(), NSRect::new(NSPoint::new(x - 46.0, h - y - 5.0), NSSize::new(92.0, 14.0)));
                    let _: () = msg_send![&*self.shadow, setPath: path];
                    CGPathRelease(path);
                    let _: () = msg_send![&*self.shadow, setHidden: false];
                }
                None => {
                    let _: () = msg_send![&*self.shadow, setHidden: true];
                }
            }
            self.props.draw(scene, h);
            // the icons on the move
            for &(id, x, y, rot) in &scene.icons {
                if let Some(p) = self.proxies.get(&id) {
                    let _: () = msg_send![&*p.icon, setPosition: CGPoint { x, y: h - y }];
                    let (sn, cs) = (-rot * super::rig::D2R).sin_cos();
                    let _: () = msg_send![&*p.icon, setAffineTransform: CGAffineTransform { a: cs, b: sn, c: -sn, d: cs, tx: 0.0, ty: 0.0 }];
                }
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
        // the cat catches the pointer only where it or its things are
        let over = self.live.catches();
        if over == self.through {
            self.through = !over;
            self.window.setIgnoresMouseEvents(!over);
        }
    }

    /// An app's window lies over the cat's at `p` (first screen points,
    /// down from its top): a click there is the app's, not the cat's.
    pub fn covered(&self, p: (f64, f64)) -> bool {
        let id = self.window.windowNumber();
        if id <= 0 {
            return false;
        }
        let me = std::process::id() as i64;
        unsafe {
            // kCGWindowListOptionOnScreenAboveWindow | kCGWindowListExcludeDesktopElements
            let list = CGWindowListCopyWindowInfo((1 << 1) | (1 << 4), id as u32);
            let Some(list) = (list as *const AnyObject).as_ref() else { return false };
            let n: usize = msg_send![list, count];
            let text = |d: &AnyObject, key: &str| -> Option<String> {
                let v: Option<Retained<AnyObject>> = msg_send![d, objectForKey: &*NSString::from_str(key)];
                let v = v?;
                let is_text: bool = msg_send![&*v, isKindOfClass: class!(NSString)];
                is_text.then(|| Retained::cast_unchecked::<NSString>(v).to_string())
            };
            let number = |d: &AnyObject, key: &str| -> Option<f64> {
                let v: Option<Retained<AnyObject>> = msg_send![d, objectForKey: &*NSString::from_str(key)];
                let v = v?;
                let is_number: bool = msg_send![&*v, isKindOfClass: class!(NSNumber)];
                is_number.then(|| msg_send![&*v, doubleValue])
            };
            let mut over = false;
            for i in 0..n {
                let d: Retained<AnyObject> = msg_send![list, objectAtIndex: i];
                let layer = number(&d, "kCGWindowLayer").unwrap_or(0.0) as i64;
                if !(0..DOCK_LEVEL).contains(&layer) || number(&d, "kCGWindowAlpha").unwrap_or(1.0) < 0.05 || number(&d, "kCGWindowOwnerPID").unwrap_or(-1.0) as i64 == me {
                    continue;
                }
                // the system's own overlays (the Dock's "show desktop" one) are not apps
                if text(&d, "kCGWindowOwnerName").is_some_and(|o| SYSTEM.contains(&o.as_str())) {
                    continue;
                }
                let b: Option<Retained<AnyObject>> = msg_send![&*d, objectForKey: &*NSString::from_str("kCGWindowBounds")];
                let Some(b) = b else { continue };
                let (Some(x), Some(y), Some(w), Some(h)) = (number(&b, "X"), number(&b, "Y"), number(&b, "Width"), number(&b, "Height")) else { continue };
                if p.0 >= x && p.0 <= x + w && p.1 >= y && p.1 <= y + h {
                    over = true;
                    break;
                }
            }
            CFRelease(list as *const AnyObject as *const c_void);
            over
        }
    }

    /// Three hooked claws out of the paw's far edge, in the paw's own picture.
    unsafe fn draw_claws(&mut self, at: Option<(usize, f64, f64, f64)>) {
        let Some((i, ih, iw, k)) = at else {
            if self.claws_on.take().is_some() {
                let _: () = msg_send![&*self.claws, setHidden: true];
            }
            return;
        };
        if self.claws_on != Some(i) {
            let _: () = msg_send![&*self.parts[i].0, addSublayer: &*self.claws];
            self.claws_on = Some(i);
            self.claws_k = -1.0;
        }
        let _: () = msg_send![&*self.claws, setHidden: false];
        if (k - self.claws_k).abs() < 0.02 {
            return;
        }
        self.claws_k = k;
        // three hooked claws out of the toes, along the bottom of the paw
        // picture at its front end; the picture's y runs down, the layer's up
        let (w, h) = (iw, ih);
        let (len, b) = (h * 0.22 * k, w * 0.016);
        let p = |px: f64, py: f64| (px, h - py);
        let path = CGPathCreateMutable();
        for u in [0.36, 0.402, 0.448] {
            let (x, y) = (w * u, h * 0.7);
            let a = p(x - b, y);
            CGPathMoveToPoint(path, std::ptr::null(), a.0, a.1);
            let (c, e) = (p(x - b * 1.3, y + len * 0.75), p(x - len * 0.45, y + len));
            CGPathAddQuadCurveToPoint(path, std::ptr::null(), c.0, c.1, e.0, e.1);
            let (c, e) = (p(x + b * 0.4, y + len * 0.6), p(x + b, y));
            CGPathAddQuadCurveToPoint(path, std::ptr::null(), c.0, c.1, e.0, e.1);
            CGPathCloseSubpath(path);
        }
        let _: () = msg_send![&*self.claws, setPath: path];
        let _: () = msg_send![&*self.claws, setLineWidth: h * 0.03];
        let _: () = msg_send![&*self.claws, setLineJoin: &*NSString::from_str("round")];
        CGPathRelease(path);
    }

    /// The windows show (or hide, once the cat is away and nothing moves).
    pub fn show(&self, on: bool) {
        if on {
            self.desk.orderFrontRegardless();
            self.window.orderFrontRegardless();
        } else {
            self.window.orderOut(None);
            if self.proxies.is_empty() {
                self.desk.orderOut(None);
            }
        }
    }

    // ------------------------------------------------------------ icons

    /// Icon `id` (an index into `desktop.spots`) starts to move: its picture
    /// rides on the desk window, and the wallpaper covers its old spot.
    pub fn lift(&mut self, id: usize) {
        let h = self.frame.size.height;
        let Some(d) = &self.desktop else { return };
        let Some(spot) = d.spots.get(id) else { return };
        if self.proxies.contains_key(&id) {
            return;
        }
        let mid = (spot.x - d.off.0, spot.y - d.off.1);
        let cell = d.cell(mid);
        let crop = |shot: &Option<Shot>| -> Option<Image> {
            let (img, origin, scale) = shot.as_ref()?;
            let r = NSRect::new(NSPoint::new((cell.0 - origin.0) * scale, (cell.1 - origin.1) * scale), NSSize::new(cell.2 * scale, cell.3 * scale));
            let c = unsafe { CGImageCreateWithImageInRect(img.0, r) };
            (!c.is_null()).then_some(Image(c))
        };
        let (icon_img, paper_img) = (crop(&d.icons), crop(&d.paper));
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let bounds = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: cell.2, height: cell.3 } };
            let patch = paper_img.map(|img| {
                let l = new_layer(&self.patches);
                let _: () = msg_send![&*l, setBounds: bounds];
                let _: () = msg_send![&*l, setContents: img.0 as *mut AnyObject];
                let _: () = msg_send![&*l, setPosition: CGPoint { x: cell.0, y: h - cell.1 - cell.3 }];
                l
            });
            let icon = new_layer(&self.moved);
            let _: () = msg_send![&*icon, setBounds: bounds];
            if let Some(img) = &icon_img {
                let _: () = msg_send![&*icon, setContents: img.0 as *mut AnyObject];
            } else if let Some(img) = workspace_icon(&spot.name, d.view.icon) {
                // no picture of the screen: the file's own icon, without its label
                let _: () = msg_send![&*icon, setContents: img.0 as *mut AnyObject];
                let _: () = msg_send![&*icon, setContentsGravity: &*NSString::from_str("top")];
            }
            // turned and placed about the icon's middle
            let anchor = CGPoint { x: (mid.0 - cell.0) / cell.2, y: 1.0 - (mid.1 - cell.1) / cell.3 };
            let _: () = msg_send![&*icon, setAnchorPoint: anchor];
            let _: () = msg_send![&*icon, setPosition: CGPoint { x: mid.0, y: h - mid.1 }];
            let _: () = msg_send![class!(CATransaction), commit];
            self.proxies.insert(id, Proxy { icon, patch });
        }
        self.desk.orderFrontRegardless();
    }

    /// Finder shows every pushed icon at its new spot: the pictures go.
    pub fn drop_all(&mut self) {
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            for (_, p) in self.proxies.drain() {
                let _: () = msg_send![&*p.icon, removeFromSuperlayer];
                if let Some(patch) = p.patch {
                    let _: () = msg_send![&*patch, removeFromSuperlayer];
                }
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }

    pub fn moving(&self) -> bool {
        !self.proxies.is_empty()
    }
}

/// The file's icon from NSWorkspace, `size` points (2 px a point).
fn workspace_icon(name: &str, size: f64) -> Option<Image> {
    let path = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Desktop").join(name))?;
    let px = size * 2.0;
    unsafe {
        let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let path = NSString::from_str(&path.to_string_lossy());
        let image: Option<Retained<AnyObject>> = msg_send![&*ws, iconForFile: &*path];
        let image = image?;
        let _: () = msg_send![&*image, setSize: NSSize::new(px, px)];
        let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px, px));
        let cg: *mut c_void = msg_send![&*image, CGImageForProposedRect: &mut rect as *mut NSRect, context: std::ptr::null::<AnyObject>(), hints: std::ptr::null::<AnyObject>()];
        (!cg.is_null()).then(|| Image(CGImageRetain(cg)))
    }
}

// ------------------------------------------------------------ Finder

/// Where Finder shows each desktop item, and its view settings (off the
/// main thread: the first time, macOS asks the person).
pub fn read_desktop() -> (Vec<Spot>, grid::View) {
    let script = r#"tell application "Finder"
set out to ""
repeat with f in (get every item of desktop)
try
set p to desktop position of f
set out to out & (name of f) & tab & ((item 1 of p) as text) & tab & ((item 2 of p) as text) & linefeed
end try
end repeat
return out
end tell"#;
    let spots = crate::kit::macos::osascript(script).map(|o| grid::parse(&o)).unwrap_or_default();
    let defaults = std::process::Command::new("/usr/bin/defaults").args(["read", "com.apple.finder", "DesktopViewSettings"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    (spots, grid::view(&defaults))
}

fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Finder places these items here (its own points), all at once.
pub fn place(items: &[(String, f64, f64)]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let mut script = String::from("tell application \"Finder\"\n");
    for (name, x, y) in items {
        script.push_str(&format!("try\nset desktop position of item \"{}\" of desktop to {{{}, {}}}\nend try\n", quote(name), x.round(), y.round()));
    }
    script.push_str("end tell");
    crate::kit::macos::osascript(&script).map(|_| ())
}

// ------------------------------------------------------------ pictures of the desktop

/// Two pictures on their way; `done` gets them once both are in.
struct Pending<F> {
    icons: Option<Option<Shot>>,
    paper: Option<Option<Shot>>,
    done: Option<F>,
}

/// Handed to Screen Capture Kit's own threads.
struct Pass<T>(T);
unsafe impl<T> Send for Pass<T> {}

impl<F: FnOnce(Option<Shot>, Option<Shot>)> Pending<F> {
    fn icons(this: &Mutex<Pending<F>>, shot: Option<Shot>) {
        let mut p = this.lock().unwrap_or_else(|e| e.into_inner());
        p.icons = Some(shot);
        p.finish();
    }

    fn paper(this: &Mutex<Pending<F>>, shot: Option<Shot>) {
        let mut p = this.lock().unwrap_or_else(|e| e.into_inner());
        p.paper = Some(shot);
        p.finish();
    }

    fn finish(&mut self) {
        if self.icons.is_some() && self.paper.is_some() {
            if let Some(done) = self.done.take() {
                done(self.icons.take().flatten(), self.paper.take().flatten());
            }
        }
    }
}

/// The icons alone and the wallpaper alone, as the screen shows them now
/// (macOS 14 and later, with screen recording allowed; else nothing).
/// `done` gets them on a background thread.
pub fn shoot(done: impl FnOnce(Option<Shot>, Option<Shot>) + Send + 'static) {
    use objc2_screen_capture_kit::{SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration};
    let has_shots = objc2::runtime::AnyClass::get(c"SCScreenshotManager").is_some();
    if !has_shots || !unsafe { CGPreflightScreenCaptureAccess() } {
        return done(None, None);
    }
    let pending = Arc::new(Mutex::new(Pending { icons: None, paper: None, done: Some(done) }));
    let block = RcBlock::new(move |content: *mut SCShareableContent, _err: *mut NSError| {
        let pending = pending.clone();
        let Some(content) = (unsafe { content.as_ref() }) else {
            Pending::icons(&pending, None);
            return Pending::paper(&pending, None);
        };
        unsafe {
            let main = CGMainDisplayID();
            let Some(display) = content.displays().iter().find(|d| d.displayID() == main) else {
                Pending::icons(&pending, None);
                return Pending::paper(&pending, None);
            };
            let df = display.frame();
            let icon_level = CGWindowLevelForKey(DESKTOP_ICONS) as isize;
            let windows = content.windows();
            let on_main = |f: CGRect| f.origin.x < df.origin.x + df.size.width && f.origin.x + f.size.width > df.origin.x && f.origin.y < df.origin.y + df.size.height && f.origin.y + f.size.height > df.origin.y;
            let finder = windows.iter().find(|w| w.windowLayer() == icon_level && on_main(w.frame()) && w.owningApplication().is_some_and(|a| a.bundleIdentifier().to_string() == "com.apple.finder"));
            let paper: Vec<_> = windows.iter().filter(|w| w.windowLayer() < icon_level && on_main(w.frame())).collect();
            let scale = 2.0;
            let conf = |w: f64, h: f64| {
                let c = SCStreamConfiguration::new();
                c.setWidth((w * scale) as usize);
                c.setHeight((h * scale) as usize);
                c.setShowsCursor(false);
                c
            };
            // the icons: Finder's desktop window on its own, see-through around them
            match &finder {
                Some(w) => {
                    let f = w.frame();
                    let filter = SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), w);
                    let c = conf(f.size.width, f.size.height);
                    c.setShouldBeOpaque(false);
                    let p = Pass(pending.clone());
                    let origin = (f.origin.x - df.origin.x, f.origin.y - df.origin.y);
                    let got = RcBlock::new(move |img: *mut objc2_core_graphics::CGImage, _e: *mut NSError| {
                        let shot = if img.is_null() { None } else { flatten(img as *mut c_void) }.map(|i| (i, origin, scale));
                        Pending::icons(&p.0, shot);
                    });
                    SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &c, Some(&got));
                }
                None => Pending::icons(&pending, None),
            }
            // the wallpaper: the windows under the icons, nothing else
            if paper.is_empty() {
                Pending::paper(&pending, None);
            } else {
                let list = NSArray::from_retained_slice(&paper);
                let filter = SCContentFilter::initWithDisplay_includingWindows(SCContentFilter::alloc(), &display, &list);
                let c = conf(df.size.width, df.size.height);
                let p = Pass(pending.clone());
                let got = RcBlock::new(move |img: *mut objc2_core_graphics::CGImage, _e: *mut NSError| {
                    let shot = if img.is_null() { None } else { flatten(img as *mut c_void) }.map(|i| (i, (0.0, 0.0), scale));
                    Pending::paper(&p.0, shot);
                });
                SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &c, Some(&got));
            }
        }
    });
    unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &block) };
}

/// Whether Finder's `desktop position` is the middle of the icon's picture
/// or its top left corner: whichever square holds more of the icons'
/// pixels in the picture of them. Returns position minus middle.
pub fn finder_offset(spots: &[Spot], icon: f64, shot: Option<&Shot>) -> (f64, f64) {
    let Some((img, origin, scale)) = shot else { return (0.0, 0.0) };
    let (w, h) = unsafe { (CGImageGetWidth(img.0), CGImageGetHeight(img.0)) };
    let alpha = unsafe {
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        let ctx = CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, w * 4, space, 1);
        CGColorSpaceRelease(space);
        if ctx.is_null() {
            return (0.0, 0.0);
        }
        CGContextDrawImage(ctx, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)), img.0);
        let data = CGBitmapContextGetData(ctx);
        let px: Vec<u8> = if data.is_null() { Vec::new() } else { (0..w * h).map(|i| *data.add(i * 4 + 3)).collect() };
        CGContextRelease(ctx);
        px
    };
    if alpha.is_empty() {
        return (0.0, 0.0);
    }
    // the bitmap's rows run from the top
    let sum = |x0: f64, y0: f64| -> u64 {
        let (px0, py0) = (((x0 - origin.0) * scale).max(0.0) as usize, ((y0 - origin.1) * scale).max(0.0) as usize);
        let n = (icon * scale) as usize;
        let mut s = 0u64;
        for y in (py0..(py0 + n).min(h)).step_by(2) {
            for x in (px0..(px0 + n).min(w)).step_by(2) {
                s += alpha[y * w + x] as u64;
            }
        }
        s
    };
    let (mut mid, mut corner) = (0u64, 0u64);
    for s in spots.iter().take(12) {
        mid += sum(s.x - icon / 2.0, s.y - icon / 2.0);
        corner += sum(s.x, s.y);
    }
    if corner > mid * 2 {
        (-icon / 2.0, -icon / 2.0)
    } else {
        (0.0, 0.0)
    }
}
