//! The theme switch animation (onboarding's Pick a theme step): the window
//! as it looked in the old theme is laid over the new one, blurs, and a
//! soft round wave from the clicked card cuts it away (H, "ripple through
//! blur").
//!
//! The old look is egui's own last frame (a viewport screenshot); Core
//! Animation then runs the whole thing on the window server (R-anim): no
//! frames of ours while it plays, and the new theme underneath is drawn
//! once.
//!
//! Flow: [`begin`] asks for the screenshot and remembers the theme to
//! switch to; [`poll`] (every frame of the setup window) sees it arrive,
//! puts the layer up and only then switches the theme, so the new theme
//! never shows before the cover is in place. Without native layers (other
//! systems, screenshots, Reduce motion) the theme just switches.

use crate::kit::glass::{set_theme_choice, ThemeChoice};
use std::sync::Mutex;

/// How long the wave takes to clear the window.
pub const DURATION: f64 = 0.8;
/// How soft the wave's edge is, as a share of its full reach.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FEATHER: f64 = 0.2;
/// The old theme's blur at its strongest, in points.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const BLUR: f64 = 10.0;

struct Pending {
    viewport: egui::ViewportId,
    /// Where the wave starts, in points from the window's top left.
    from: egui::Pos2,
    choice: ThemeChoice,
    shot: Option<std::sync::Arc<egui::ColorImage>>,
    /// When the screenshot was asked for (it may never come).
    asked: f64,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// Switch to `choice` with the wave starting at `from`. Falls back to an
/// instant switch where the animation can't run.
pub fn begin(ctx: &egui::Context, from: egui::Pos2, choice: ThemeChoice) {
    if !native::available() {
        set_theme_choice(choice);
        ctx.request_repaint();
        return;
    }
    let now = ctx.input(|i| i.time);
    let viewport = ctx.viewport_id();
    // a second click while the first is still waiting for its picture:
    // the newest choice wins, the picture already asked for still serves
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(old) = p.as_mut() {
        old.choice = choice;
        old.from = from;
        return;
    }
    *p = Some(Pending { viewport, from, choice, shot: None, asked: now });
    drop(p);
    let c = ctx.clone();
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
        if let Some(p) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            p.shot = Some(img);
        }
        c.request_repaint();
    })));
    ctx.request_repaint();
}

/// Every frame of the window that called [`begin`]: once the picture is
/// in, cover the window with it, switch the theme and let the wave run.
pub fn poll(ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let mut lock = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = lock.as_ref() else {
        drop(lock);
        native::tidy(now);
        return;
    };
    if p.viewport != ctx.viewport_id() {
        return;
    }
    let Some(p) = lock.take_if(|p| p.shot.is_some() || now - p.asked > 0.5) else {
        // still waiting for the picture
        ctx.request_repaint();
        return;
    };
    drop(lock);
    if let Some(shot) = &p.shot {
        native::start(p.viewport, shot, p.from, now);
    }
    set_theme_choice(p.choice);
    ctx.request_repaint();
    // one more frame once the wave is done, to take the layer away
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(DURATION + 0.05));
}

/// True while a switch is waiting for its picture (the step shows the
/// new card as picked meanwhile).
pub fn pending_choice() -> Option<ThemeChoice> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| p.choice)
}

#[cfg(target_os = "macos")]
mod native {
    use super::{BLUR, DURATION, FEATHER};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, Encode};
    use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
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

    thread_local! {
        /// The cover up now, and when it is done.
        static UP: RefCell<Option<(Retained<AnyObject>, f64, Retained<AnyObject>)>> = const { RefCell::new(None) };
        /// The mask's clear and solid colours, made once.
        static WHITES: RefCell<Option<(Retained<AnyObject>, Retained<AnyObject>)>> = const { RefCell::new(None) };
    }

    /// Off for screenshots (they draw with egui alone) and with Reduce motion on.
    pub fn available() -> bool {
        if !crate::kit::spin::native_allowed() {
            return false;
        }
        unsafe {
            let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let reduce: bool = msg_send![&*ws, accessibilityDisplayShouldReduceMotion];
            !reduce
        }
    }

    fn remove() {
        UP.with(|u| {
            if let Some((l, _, view)) = u.borrow_mut().take() {
                unsafe {
                    let _: () = msg_send![class!(CATransaction), begin];
                    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                    let _: () = msg_send![&*l, removeFromSuperlayer];
                    let _: () = msg_send![class!(CATransaction), commit];
                    // Core Image filters were only on for the blur
                    let _: () = msg_send![&*view, setLayerUsesCoreImageFilters: false];
                }
            }
        });
    }

    /// Take the cover away once its wave is over.
    pub fn tidy(now: f64) {
        let done = UP.with(|u| u.borrow().as_ref().is_some_and(|(_, end, _)| now >= *end));
        if done {
            remove();
        }
    }

    /// `img` (the window in pixels) over viewport `id`, cut away by a wave from `from`.
    pub fn start(id: egui::ViewportId, img: &egui::ColorImage, from: egui::Pos2, now: f64) {
        // a switch while the last wave still runs: that one is over now
        remove();
        let Some(view) = eframe::viewport_ns_view(id) else { return };
        let view = view as *const AnyObject;
        let key = |k: &str| NSString::from_str(k);
        let num = |v: f64| NSNumber::new_f64(v);
        unsafe {
            let Some(view) = view.as_ref() else { return };
            let root: Option<Retained<AnyObject>> = msg_send![view, layer];
            let Some(root) = root else { return };
            let window: Option<Retained<AnyObject>> = msg_send![view, window];
            let scale: f64 = match &window {
                Some(w) => msg_send![&**w, backingScaleFactor],
                None => 2.0,
            };
            // egui's surface is untagged: tag the picture with the
            // window's own colour space so it matches what was on screen
            let ns_space: Option<Retained<AnyObject>> = match &window {
                Some(w) => msg_send![&**w, colorSpace],
                None => None,
            };
            let cg_space: *const c_void = match &ns_space {
                Some(s) => msg_send![&**s, CGColorSpace],
                None => std::ptr::null(),
            };
            let own_space = if cg_space.is_null() { CGColorSpaceCreateDeviceRGB() } else { std::ptr::null_mut() };
            let space = if cg_space.is_null() { own_space as *const c_void } else { cg_space };

            let [w, h] = img.size;
            if w == 0 || h == 0 {
                return;
            }
            let bytes: &[u8] = bytemuck_bytes(&img.pixels);
            let data = CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
            let provider = CGDataProviderCreateWithCFData(data);
            // RGBA, premultiplied (egui's Color32)
            const PREMULTIPLIED_LAST: u32 = 1;
            let image = CGImageCreate(w, h, 8, 32, w * 4, space, PREMULTIPLIED_LAST, provider, std::ptr::null(), false, 0);
            CGDataProviderRelease(provider);
            CFRelease(data);
            if !own_space.is_null() {
                CGColorSpaceRelease(own_space);
            }
            if image.is_null() {
                return;
            }

            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let bounds: NSRect = msg_send![&*root, bounds];
            let (bw, bh) = (bounds.size.width.max(1.0), bounds.size.height.max(1.0));
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(bw, bh));
            const W_SIZABLE: u32 = 1 << 1;
            const H_SIZABLE: u32 = 1 << 4;

            // y down like egui (as spin's window cover), over everything
            let root_flipped: bool = msg_send![&*root, contentsAreFlipped];
            let holder: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*holder, setName: &*key("cmdmeow-theme-fade")];
            let _: () = msg_send![&*holder, setGeometryFlipped: !root_flipped];
            let _: () = msg_send![&*holder, setFrame: frame];
            let _: () = msg_send![&*holder, setAutoresizingMask: W_SIZABLE | H_SIZABLE];
            let _: () = msg_send![&*holder, setZPosition: 1000.0f64];

            let cover: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*cover, setContentsScale: scale];
            let _: () = msg_send![&*cover, setFrame: frame];
            let _: () = msg_send![&*cover, setContents: image as *const AnyObject];
            let _: () = msg_send![&*cover, setContentsGravity: &*key("resize")];
            // CMDMEOW_FADE_FLIP=1 turns the picture over (should a macOS
            // version draw it upside down)
            if std::env::var_os("CMDMEOW_FADE_FLIP").is_some_and(|v| v == "1") {
                let _: () = msg_send![&*cover, setAffineTransform: CGAffine { a: 1.0, b: 0.0, c: 0.0, d: -1.0, tx: 0.0, ty: 0.0 }];
            }
            let _: () = msg_send![&*holder, addSublayer: &*cover];
            let _: () = msg_send![&*root, addSublayer: &*holder];
            CGImageRelease(image);

            // far enough to clear the farthest corner
            let (cx, cy) = (from.x as f64, from.y as f64);
            let reach = [(0.0, 0.0), (bw, 0.0), (0.0, bh), (bw, bh)].iter().map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()).fold(1.0, f64::max);

            // the mask: a radial gradient, clear inside the wave, solid
            // outside (round, so which way is up doesn't matter)
            let mask: Retained<AnyObject> = msg_send![class!(CAGradientLayer), layer];
            let _: () = msg_send![&*mask, setContentsScale: scale];
            let _: () = msg_send![&*mask, setType: &*key("radial")];
            let (clear, solid) = whites();
            let colors = NSArray::from_retained_slice(&[Retained::retain(clear as *mut AnyObject).unwrap(), Retained::retain(solid as *mut AnyObject).unwrap()]);
            let _: () = msg_send![&*mask, setColors: &*colors];
            let _: () = msg_send![&*mask, setStartPoint: NSPoint::new(0.5, 0.5)];
            let _: () = msg_send![&*mask, setEndPoint: NSPoint::new(1.0, 1.0)];
            let _: () = msg_send![&*mask, setFrame: NSRect::new(NSPoint::new(cx - reach, cy - reach), NSSize::new(reach * 2.0, reach * 2.0))];
            let start = NSArray::from_retained_slice(&[num(-FEATHER), num(0.0)]);
            let end = NSArray::from_retained_slice(&[num(1.0), num(1.0 + FEATHER)]);
            // the model ends cleared: the cover is see-through once the wave is done
            let _: () = msg_send![&*mask, setLocations: &*end];
            let _: () = msg_send![&*cover, setMask: &*mask];

            let ease: Retained<AnyObject> = msg_send![class!(CAMediaTimingFunction), functionWithName: &*key("easeInEaseOut")];
            let wave: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key("locations")];
            let _: () = msg_send![&*wave, setFromValue: &*start];
            let _: () = msg_send![&*wave, setToValue: &*end];
            let _: () = msg_send![&*wave, setDuration: DURATION];
            let _: () = msg_send![&*wave, setTimingFunction: &*ease];
            let _: () = msg_send![&*mask, addAnimation: &*wave, forKey: &*key("cmdmeow-wave")];

            // the old theme blurs as the wave passes (needs Core Image
            // filters on the view; without them the wave runs unblurred)
            let _: () = msg_send![view, setLayerUsesCoreImageFilters: true];
            let filter: Option<Retained<AnyObject>> = msg_send![class!(CIFilter), filterWithName: &*key("CIGaussianBlur")];
            if let Some(f) = filter {
                let _: () = msg_send![&*f, setDefaults];
                let _: () = msg_send![&*f, setName: &*key("blur")];
                let _: () = msg_send![&*f, setValue: &*num(BLUR), forKey: &*key("inputRadius")];
                let filters = NSArray::from_retained_slice(&[f]);
                let _: () = msg_send![&*cover, setFilters: &*filters];
                let ease_out: Retained<AnyObject> = msg_send![class!(CAMediaTimingFunction), functionWithName: &*key("easeOut")];
                let blur: Retained<AnyObject> = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*key("filters.blur.inputRadius")];
                let _: () = msg_send![&*blur, setFromValue: &*num(0.0)];
                let _: () = msg_send![&*blur, setToValue: &*num(BLUR)];
                let _: () = msg_send![&*blur, setDuration: DURATION * 0.6];
                let _: () = msg_send![&*blur, setTimingFunction: &*ease_out];
                let _: () = msg_send![&*cover, addAnimation: &*blur, forKey: &*key("cmdmeow-blur")];
            }
            let _: () = msg_send![class!(CATransaction), commit];
            let _: () = msg_send![class!(CATransaction), flush];
            let view = Retained::retain(view as *const AnyObject as *mut AnyObject);
            if let Some(view) = view {
                UP.with(|u| *u.borrow_mut() = Some((holder, now + DURATION, view)));
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGAffine {
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        tx: f64,
        ty: f64,
    }

    unsafe impl objc2::Encode for CGAffine {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGAffineTransform", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
    }

    /// The mask's colours as CGColors: white, clear and solid (the mask
    /// only reads alpha). The NSColors that own them are kept for good.
    fn whites() -> (*const c_void, *const c_void) {
        WHITES.with(|w| {
            let mut w = w.borrow_mut();
            let (clear, solid) = w.get_or_insert_with(|| unsafe {
                let c: Retained<AnyObject> = msg_send![class!(NSColor), colorWithWhite: 1.0f64, alpha: 0.0f64];
                let s: Retained<AnyObject> = msg_send![class!(NSColor), colorWithWhite: 1.0f64, alpha: 1.0f64];
                (c, s)
            });
            unsafe {
                let a: *const c_void = msg_send![&**clear, CGColor];
                let b: *const c_void = msg_send![&**solid, CGColor];
                (a, b)
            }
        })
    }

    fn bytemuck_bytes(px: &[egui::Color32]) -> &[u8] {
        // Color32 is four bytes, r g b a
        unsafe { std::slice::from_raw_parts(px.as_ptr() as *const u8, std::mem::size_of_val(px)) }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    pub fn available() -> bool {
        false
    }
    pub fn tidy(_now: f64) {}
    pub fn start(_id: egui::ViewportId, _img: &egui::ColorImage, _from: egui::Pos2, _now: f64) {}
}
