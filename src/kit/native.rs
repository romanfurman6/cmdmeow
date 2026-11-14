//! macOS bits: the real frosted-glass material behind our window, menu-bar app
//! behaviour, thermal state and the list of open apps.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy,
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode, NSWorkspace,
    };
    use objc2_foundation::NSProcessInfo;
    use std::cell::RefCell;

    thread_local! {
        static GLASS: RefCell<Vec<(isize, Retained<NSView>)>> = const { RefCell::new(Vec::new()) };
        static LIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// True when the system appearance is light. Our windows pin their own
    /// appearance, so this asks the app, which follows the system.
    pub fn system_light() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        !app.effectiveAppearance().name().to_string().contains("Dark")
    }

    fn appearance(light: bool) -> Option<Retained<NSAppearance>> {
        NSAppearance::appearanceNamed(if light { unsafe { NSAppearanceNameAqua } } else { unsafe { NSAppearanceNameDarkAqua } })
    }

    /// Give every glass window the light or dark material.
    pub fn set_appearance(light: bool) {
        LIGHT.with(|l| l.set(light));
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(ap) = appearance(light) else { return };
        let app = NSApplication::sharedApplication(mtm);
        for w in app.windows().iter() {
            let num = w.windowNumber();
            if GLASS.with(|g| g.borrow().iter().any(|(n, _)| *n == num)) {
                w.setAppearance(Some(&ap));
            }
        }
    }

    /// Put the system's frosted blur (NSVisualEffectView) behind every egui
    /// window, with rounded corners. Our backdrop sits on top, partly clear.
    pub fn apply_glass(corner_radius: f64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        for w in windows.iter() {
            let Some(content) = w.contentView() else { continue };
            let class = content.class().name().to_string_lossy().to_string();
            if !class.contains("Winit") || w.title().to_string().starts_with(super::PLAIN_WINDOW) {
                continue;
            }
            let num = w.windowNumber();
            let known = GLASS.with(|g| g.borrow().iter().any(|(n, _)| *n == num));
            if known {
                continue;
            }
            let Some(superview) = (unsafe { content.superview() }) else { continue };
            w.setOpaque(false);
            w.setBackgroundColor(Some(&NSColor::clearColor()));
            // glass in our own mode, so text stays readable on any wallpaper
            if let Some(ap) = appearance(LIGHT.with(|l| l.get())) {
                w.setAppearance(Some(&ap));
            }
            let frame = content.frame();
            // a real frosted blur, clipped to the window's rounded corners
            // (Liquid Glass refracts what's behind instead of blurring it,
            // which reads as a stretched picture under the panel)
            let e = NSVisualEffectView::initWithFrame(mtm.alloc::<NSVisualEffectView>(), frame);
            // CMDMEOW_BLUR_MATERIAL picks another material when tuning
            let material = match std::env::var("CMDMEOW_BLUR_MATERIAL").as_deref() {
                Ok("popover") => NSVisualEffectMaterial::Popover,
                Ok("hud") => NSVisualEffectMaterial::HUDWindow,
                Ok("sidebar") => NSVisualEffectMaterial::Sidebar,
                Ok("under") => NSVisualEffectMaterial::UnderWindowBackground,
                _ => NSVisualEffectMaterial::Menu,
            };
            e.setMaterial(material);
            e.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            e.setState(NSVisualEffectState::Active);
            // rounded through a stretchable mask image: rounding the view's
            // own layer turns the blur off
            if let Some(mask) = rounded_mask(corner_radius) {
                e.setMaskImage(Some(&mask));
            }
            let view: Retained<NSView> = Retained::into_super(e);
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            superview.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(&content));
            w.invalidateShadow();
            GLASS.with(|g| g.borrow_mut().push((num, view)));
        }
    }

    /// A white rounded square, 2x, that stretches between its corners: the
    /// mask that gives the blur the window's rounded corners.
    fn rounded_mask(radius: f64) -> Option<Retained<objc2_app_kit::NSImage>> {
        use objc2::AllocAnyThread;
        use objc2_app_kit::{NSImage, NSImageResizingMode};
        use objc2_foundation::{NSData, NSEdgeInsets, NSSize};
        let r = radius.max(1.0);
        let side_pt = (r * 2.0 + 2.0).ceil();
        let px = (side_pt * 2.0) as u32;
        let rp = r * 2.0;
        let mut img = image::RgbaImage::new(px, px);
        for (x, y, p) in img.enumerate_pixels_mut() {
            // distance outside the rounded rect, in pixels (antialiased edge)
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let cx = fx.clamp(rp, px as f64 - rp);
            let cy = fy.clamp(rp, px as f64 - rp);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - rp;
            let a = (0.5 - d).clamp(0.0, 1.0);
            *p = image::Rgba([255, 255, 255, (a * 255.0).round() as u8]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
        let data = NSData::with_bytes(&png);
        let ns = NSImage::initWithData(NSImage::alloc(), &data)?;
        ns.setSize(NSSize::new(side_pt, side_pt));
        ns.setCapInsets(NSEdgeInsets { top: r, left: r, bottom: r, right: r });
        ns.setResizingMode(NSImageResizingMode::Stretch);
        Some(ns)
    }

    /// Diagnostic: describe our windows (frames, view classes, glass layers).
    pub fn debug_windows() -> String {
        let Some(mtm) = MainThreadMarker::new() else { return String::new() };
        let app = NSApplication::sharedApplication(mtm);
        let mut out = String::new();
        for w in app.windows().iter() {
            let f = w.frame();
            let screen = w.screen().map(|s| { let sf = s.frame(); (sf.size.width, sf.size.height, s.backingScaleFactor()) });
            out.push_str(&format!("win #{} frame {:?} opaque {} visible {} scale {} screen {:?}\n", w.windowNumber(), (f.origin.x, f.origin.y, f.size.width, f.size.height), w.isOpaque(), w.isVisible(), w.backingScaleFactor(), screen));
            if let Some(c) = w.contentView() {
                let cf = c.frame();
                out.push_str(&format!("  content {} {:?}\n", c.class().name().to_string_lossy(), (cf.size.width, cf.size.height)));
                if let Some(sv) = unsafe { c.superview() } {
                    out.push_str(&format!("  super {}\n", sv.class().name().to_string_lossy()));
                    for v in sv.subviews().iter() {
                        let vf = v.frame();
                        out.push_str(&format!("    sub {} {:?} hidden {}\n", v.class().name().to_string_lossy(), (vf.origin.x, vf.origin.y, vf.size.width, vf.size.height), v.isHidden()));
                    }
                }
            }
        }
        out
    }

    /// Accessory = menu bar app with no Dock icon; Regular = normal app window.
    pub fn set_dock_icon(visible: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let policy = if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
        let _ = app.setActivationPolicy(policy);
    }

    pub fn activate() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        // macOS 14+ ignores activateIgnoringOtherApps more and more; its
        // cooperative `activate` is the one that brings the window forward
        let modern: bool = unsafe { objc2::msg_send![&*app, respondsToSelector: objc2::sel!(activate)] };
        if modern {
            let _: () = unsafe { objc2::msg_send![&*app, activate] };
        }
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }

    /// Render an SF Symbol (Apple's own icon set) to straight RGBA pixels, white
    /// with alpha, `px` pixels tall. None when the name doesn't exist.
    pub fn render_symbol(name: &str, px: f64, weight: f64) -> Option<(usize, usize, Vec<u8>)> {
        use objc2::AllocAnyThread;
        use objc2_app_kit::{NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSImageSymbolConfiguration};
        use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
        MainThreadMarker::new()?;
        let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
        let cfg = NSImageSymbolConfiguration::configurationWithPointSize_weight(px, weight);
        let img = img.imageWithSymbolConfiguration(&cfg)?;
        let size = img.size();
        if size.width <= 0.0 || size.height <= 0.0 {
            return None;
        }
        let w = (size.width.ceil() as isize) + 2;
        let h = (size.height.ceil() as isize) + 2;
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                w,
                h,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                w * 4,
                32,
            )
        }?;
        let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        let previous = NSGraphicsContext::currentContext();
        NSGraphicsContext::setCurrentContext(Some(&ctx));
        img.drawInRect_fromRect_operation_fraction(
            NSRect::new(NSPoint::new(1.0, 1.0), NSSize::new(size.width, size.height)),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            NSCompositingOperation::SourceOver,
            1.0,
        );
        ctx.flushGraphics();
        NSGraphicsContext::setCurrentContext(previous.as_deref());
        let data = rep.bitmapData();
        if data.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, (w * h * 4) as usize) };
        // keep only the coverage; colour is applied when drawing
        let mut out = vec![0u8; bytes.len()];
        for i in 0..(w * h) as usize {
            let a = bytes[i * 4 + 3];
            out[i * 4] = 255;
            out[i * 4 + 1] = 255;
            out[i * 4 + 2] = 255;
            out[i * 4 + 3] = a;
        }
        Some((w as usize, h as usize, out))
    }

    pub fn weight_semibold() -> f64 {
        unsafe { objc2_app_kit::NSFontWeightSemibold }
    }

    pub fn thermal_state() -> u8 {
        let s = NSProcessInfo::processInfo().thermalState();
        s.0.clamp(0, 3) as u8
    }

    pub fn running_apps() -> (Vec<String>, Vec<String>) {
        let ws = NSWorkspace::sharedWorkspace();
        let apps = ws.runningApplications();
        let mut ids = Vec::new();
        let mut names = Vec::new();
        for a in apps.iter() {
            if let Some(id) = a.bundleIdentifier() {
                ids.push(id.to_string());
            }
            if let Some(n) = a.localizedName() {
                names.push(n.to_string());
            }
        }
        (ids, names)
    }

    /// Move a file or folder to the Trash the way Finder does: it lands in the
    /// Trash of its own volume (so external drives work) and can be put back.
    pub fn trash_item(p: &std::path::Path) -> Result<(), String> {
        trash_item_at(p).map(|_| ())
    }

    /// Like `trash_item`, and says where in the Trash the item now is (so it
    /// can be put back).
    pub fn trash_item_at(p: &std::path::Path) -> Result<std::path::PathBuf, String> {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
        let mut out: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut out))
            .map_err(|e| e.localizedDescription().to_string())?;
        out.and_then(|u| u.path()).map(|s| std::path::PathBuf::from(s.to_string())).ok_or_else(|| "Moved, but the Trash didn't say where".to_string())
    }

    /// Call `f` (on the main thread) when the system switches between light
    /// and dark. Our windows pin their own appearance, so they get no event
    /// of their own for it.
    pub fn watch_theme(f: impl Fn() + 'static) {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        let block = block2::RcBlock::new(move |_note: std::ptr::NonNull<AnyObject>| f());
        let name = NSString::from_str("AppleInterfaceThemeChangedNotification");
        unsafe {
            let center: Retained<AnyObject> = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
            let queue: Retained<AnyObject> = msg_send![class!(NSOperationQueue), mainQueue];
            let token: Option<Retained<AnyObject>> = msg_send![&*center, addObserverForName: &*name, object: Option::<&AnyObject>::None, queue: &*queue, usingBlock: &*block];
            // observed for the app's whole life
            std::mem::forget(token);
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::*;

/// Windows whose title starts with this get no glass layer behind them: a
/// tool's floating windows that draw everything themselves (pinned images,
/// overlays).
pub const PLAIN_WINDOW: &str = "plain-";

#[cfg(not(target_os = "macos"))]
mod other {
    pub fn apply_glass(_r: f64) {}
    pub fn system_light() -> bool {
        false
    }
    pub fn set_appearance(_light: bool) {}
    pub fn watch_theme(_f: impl Fn() + 'static) {}
    pub fn debug_windows() -> String {
        String::new()
    }
    pub fn set_dock_icon(_v: bool) {}
    pub fn activate() {}
    pub fn thermal_state() -> u8 {
        0
    }
    pub fn render_symbol(_n: &str, _px: f64, _w: f64) -> Option<(usize, usize, Vec<u8>)> {
        None
    }
    pub fn weight_semibold() -> f64 {
        0.3
    }
    pub fn running_apps() -> (Vec<String>, Vec<String>) {
        (vec![], vec![])
    }
    /// Move into ~/.Trash. Refuses (instead of deleting) when the item is on
    /// another volume, so "Move to Trash" never turns into a permanent delete.
    pub fn trash_item(p: &std::path::Path) -> Result<(), String> {
        trash_item_at(p).map(|_| ())
    }
    pub fn trash_item_at(p: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let trash = dirs::home_dir().ok_or("No home folder")?.join(".Trash");
        std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let base = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dest = trash.join(&base);
        let mut k = 1;
        while dest.symlink_metadata().is_ok() {
            dest = trash.join(format!("{base} {k}"));
            k += 1;
        }
        std::fs::rename(p, &dest).map_err(|e| e.to_string())?;
        Ok(dest)
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::*;
