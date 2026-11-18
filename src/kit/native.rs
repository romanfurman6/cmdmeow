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

    // ---- Liquid Glass under the panel's widget cards (macOS 26+) ----
    //
    // The panel window stacks, back to front: the frosted blur (apply_glass),
    // the glow (an image: light down the centre, darker sides), a view that
    // clips to the scrolling area with one NSGlassEffectView per widget card,
    // and egui's own view on top, which draws the cards' content only.

    use objc2_app_kit::{NSGlassEffectView, NSGlassEffectViewStyle};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    struct CardGlass {
        content: usize,
        glow: Retained<NSView>,
        glow_key: (u32, u32, bool),
        clip: Retained<NSView>,
        views: Vec<Retained<NSGlassEffectView>>,
        last: Vec<[f32; 4]>,
        last_clip: [f32; 4],
    }

    thread_local! {
        static CARDS: RefCell<Option<CardGlass>> = const { RefCell::new(None) };
    }

    /// True when this macOS has Liquid Glass (NSGlassEffectView, macOS 26).
    /// `CMDMEOW_LIQUID=0` turns it off.
    pub fn liquid_glass() -> bool {
        static S: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *S.get_or_init(|| std::env::var("CMDMEOW_LIQUID").as_deref() != Ok("0") && objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some())
    }

    fn ns_rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// Put glass under `cards` (egui points, top-left origin, in the window
    /// whose egui view is `content_view`), clipped to `clip`; `glow` is the
    /// panel background (width, height, RGBA rows top first) for `light`.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_card_glass(content_view: *mut std::ffi::c_void, clip: [f32; 4], cards: &[[f32; 4]], radius: f64, light: bool, glow: impl FnOnce(u32, u32) -> Vec<u8>) {
        if content_view.is_null() || MainThreadMarker::new().is_none() {
            return;
        }
        let mtm = MainThreadMarker::new().unwrap();
        // SAFETY: the pointer is the live NSView of eframe's root window
        let content: &NSView = unsafe { &*(content_view as *const NSView) };
        let Some(superview) = (unsafe { content.superview() }) else { return };
        let cf = content.frame();
        let (w, h) = (cf.size.width, cf.size.height);
        CARDS.with(|c| {
            let mut c = c.borrow_mut();
            if c.as_ref().map(|g| g.content) != Some(content_view as usize) {
                if let Some(old) = c.take() {
                    old.glow.removeFromSuperview();
                    old.clip.removeFromSuperview();
                }
                let glow = NSView::initWithFrame(mtm.alloc::<NSView>(), cf);
                glow.setWantsLayer(true);
                glow.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
                superview.addSubview_positioned_relativeTo(&glow, NSWindowOrderingMode::Below, Some(content));
                let clip = NSView::initWithFrame(mtm.alloc::<NSView>(), cf);
                clip.setWantsLayer(true);
                if let Some(l) = clip.layer() {
                    l.setMasksToBounds(true);
                }
                superview.addSubview_positioned_relativeTo(&clip, NSWindowOrderingMode::Below, Some(content));
                *c = Some(CardGlass { content: content_view as usize, glow, glow_key: (0, 0, false), clip, views: Vec::new(), last: Vec::new(), last_clip: [0.0; 4] });
            }
            let g = c.as_mut().unwrap();
            // the glow, rebuilt when the window's size or the theme changes
            let key = (w.round() as u32, h.round() as u32, light);
            if g.glow_key != key && key.0 > 0 && key.1 > 0 {
                g.glow_key = key;
                g.glow.setFrame(cf);
                if let Some(img) = glow_image(key.0, key.1, glow(key.0, key.1)) {
                    if let Some(l) = g.glow.layer() {
                        l.setCornerRadius(radius + 2.0);
                        l.setMasksToBounds(true);
                        unsafe { l.setContents(Some(&img)) };
                    }
                }
            }
            // egui's y runs down from the top; AppKit's runs up from the bottom
            if g.last_clip != clip {
                g.last_clip = clip;
                let [x, y, cw, ch] = clip.map(f64::from);
                g.clip.setFrame(ns_rect(cf.origin.x + x, cf.origin.y + h - y - ch, cw, ch));
            }
            if g.last.as_slice() != cards {
                g.last = cards.to_vec();
                let [cx, cy, _, ch] = clip.map(f64::from);
                for (i, r) in cards.iter().enumerate() {
                    if i == g.views.len() {
                        let v = NSGlassEffectView::initWithFrame(mtm.alloc::<NSGlassEffectView>(), ns_rect(0.0, 0.0, 10.0, 10.0));
                        v.setCornerRadius(radius);
                        v.setStyle(NSGlassEffectViewStyle::Regular);
                        g.clip.addSubview(&v);
                        g.views.push(v);
                    }
                    let [x, y, rw, rh] = r.map(f64::from);
                    let v = &g.views[i];
                    // in the clip view: x from its left, y up from its bottom
                    v.setFrame(ns_rect(x - cx, ch - (y - cy) - rh, rw, rh));
                    v.setHidden(false);
                }
                for v in g.views.iter().skip(cards.len()) {
                    v.setHidden(true);
                }
            }
            g.glow.setHidden(false);
            g.clip.setHidden(false);
        });
    }

    /// Hide the glass and the glow (the panel went away, or screenshots).
    pub fn hide_card_glass() {
        CARDS.with(|c| {
            if let Some(g) = c.borrow().as_ref() {
                g.glow.setHidden(true);
                g.clip.setHidden(true);
            }
        });
    }

    fn glow_image(w: u32, h: u32, rgba: Vec<u8>) -> Option<Retained<objc2_app_kit::NSImage>> {
        use objc2::AllocAnyThread;
        use objc2_app_kit::NSImage;
        use objc2_foundation::NSData;
        let img = image::RgbaImage::from_raw(w, h, rgba)?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
        let ns = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))?;
        ns.setSize(NSSize::new(w as f64, h as f64));
        Some(ns)
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
        let was = app.activationPolicy();
        if was == policy {
            return;
        }
        let _ = app.setActivationPolicy(policy);
        // a menu bar app that turns into a Dock app while already active
        // keeps the previous app's menus in the menu bar until it's
        // activated anew (deactivate alone didn't do it): hand activation
        // to the Dock for a moment, and the window that is opening takes it
        // back on its next frames
        if visible && app.isActive() {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            unsafe {
                let id = objc2_foundation::NSString::from_str("com.apple.dock");
                let docks: Retained<AnyObject> = msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: &*id];
                let dock: Option<Retained<AnyObject>> = msg_send![&*docks, firstObject];
                if let Some(dock) = dock {
                    let _: bool = msg_send![&*dock, activateWithOptions: 0usize];
                }
            }
        }
    }

    /// A Window menu (Minimize ⌘M, Zoom) beside the app menu winit makes,
    /// so ⌘M works in the app window.
    pub fn install_window_menu() {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send, sel};
        use objc2_foundation::NSString;
        let Some(_mtm) = MainThreadMarker::new() else { return };
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            // the first item of the main menu is the app menu (winit's):
            // without it a lone Window menu would take its place
            let main: Option<Retained<AnyObject>> = msg_send![&*app, mainMenu];
            let Some(main) = main else { return };
            // winit names the app menu's items after the binary ("Quit
            // cmdmeow"): use the app's own name
            let process: Retained<AnyObject> = msg_send![class!(NSProcessInfo), processInfo];
            let pname: Retained<NSString> = msg_send![&*process, processName];
            let me: Retained<AnyObject> = msg_send![class!(NSRunningApplication), currentApplication];
            let shown: Option<Retained<NSString>> = msg_send![&*me, localizedName];
            let (pname, shown) = (pname.to_string(), shown.map(|s| s.to_string()).unwrap_or_default());
            if !shown.is_empty() && shown != pname {
                let first: Option<Retained<AnyObject>> = msg_send![&*main, itemAtIndex: 0isize];
                let sub: Option<Retained<AnyObject>> = match &first {
                    Some(f) => msg_send![&**f, submenu],
                    None => None,
                };
                if let Some(sub) = sub {
                    let n: isize = msg_send![&*sub, numberOfItems];
                    for i in 0..n {
                        let item: Retained<AnyObject> = msg_send![&*sub, itemAtIndex: i];
                        let t: Retained<NSString> = msg_send![&*item, title];
                        let t = t.to_string();
                        if t.contains(&pname) {
                            let _: () = msg_send![&*item, setTitle: &*NSString::from_str(&t.replace(&pname, &shown))];
                        }
                    }
                }
            }
            let title = NSString::from_str("Window");
            let existing: Option<Retained<AnyObject>> = msg_send![&*main, itemWithTitle: &*title];
            if existing.is_some() {
                return;
            }
            let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
            let _: () = msg_send![&*menu, setTitle: &*title];
            for (name, action, key) in [("Close", sel!(performClose:), "w"), ("Minimize", sel!(performMiniaturize:), "m"), ("Zoom", sel!(performZoom:), "")] {
                let item: Retained<AnyObject> = msg_send![class!(NSMenuItem), new];
                let _: () = msg_send![&*item, setTitle: &*NSString::from_str(name)];
                let _: () = msg_send![&*item, setAction: action];
                let _: () = msg_send![&*item, setKeyEquivalent: &*NSString::from_str(key)];
                let _: () = msg_send![&*menu, addItem: &*item];
            }
            let holder: Retained<AnyObject> = msg_send![class!(NSMenuItem), new];
            let _: () = msg_send![&*holder, setTitle: &*title];
            let _: () = msg_send![&*holder, setSubmenu: &*menu];
            let _: () = msg_send![&*main, addItem: &*holder];
            let _: () = msg_send![&*app, setWindowsMenu: &*menu];
        }
    }

    /// Put the app window (the titled one) in front of other apps' windows,
    /// even when macOS didn't let the app activate (a launch it didn't
    /// see the user ask for).
    pub fn bring_app_window_front() {
        use objc2_app_kit::NSWindowStyleMask;
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        for w in app.windows().iter() {
            if w.isVisible() && w.styleMask().contains(NSWindowStyleMask::Titled) {
                w.orderFrontRegardless();
                w.makeKeyWindow();
            }
        }
    }

    /// The title of our window that has the keyboard, if any.
    pub fn key_window_title() -> Option<String> {
        let mtm = MainThreadMarker::new()?;
        NSApplication::sharedApplication(mtm).keyWindow().map(|w| w.title().to_string())
    }

    /// The process id of the app in front, when it isn't us.
    pub fn front_other_app() -> Option<i32> {
        let pid = NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier();
        (pid != std::process::id() as i32 && pid > 0).then_some(pid)
    }

    /// Hand the keyboard to the app with `pid`. macOS allows it while we are
    /// the active app; our windows stay where they are.
    pub fn give_focus_to(pid: i32) -> bool {
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else { return false };
        if other.isTerminated() {
            return false;
        }
        let me = NSApplication::sharedApplication(mtm);
        // macOS 14+: cooperative activation, the active app yields first
        let modern: bool = unsafe { objc2::msg_send![&*me, respondsToSelector: objc2::sel!(yieldActivationToApplication:)] };
        if modern {
            let _: () = unsafe { objc2::msg_send![&*me, yieldActivationToApplication: &*other] };
        }
        #[allow(deprecated)]
        other.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
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

    /// Apps in the Dock that are running now (not background helpers).
    pub fn open_apps() -> Vec<super::OpenApp> {
        use objc2_app_kit::NSApplicationActivationPolicy;
        let me = std::process::id() as i32;
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular && !a.isTerminated() && a.processIdentifier() != me)
            .map(|a| super::OpenApp {
                pid: a.processIdentifier(),
                id: a.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
                name: a.localizedName().map(|s| s.to_string()).unwrap_or_default(),
                bundle: a.bundleURL().and_then(|u| u.path()).map(|s| std::path::PathBuf::from(s.to_string())),
            })
            .collect()
    }

    /// Ask an app to quit, like ⌘Q: it may save, ask, or refuse. Never forced.
    pub fn quit_app(pid: i32) -> bool {
        use objc2_app_kit::NSRunningApplication;
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| a.terminate())
    }

    /// The app with this process id is still running.
    pub fn app_alive(pid: i32) -> bool {
        use objc2_app_kit::NSRunningApplication;
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| !a.isTerminated())
    }

    /// A standard macOS alert with `ok` and Cancel, brought to the front
    /// (the app may have no window up). True on `ok`. Main thread only;
    /// blocks until answered.
    pub fn confirm(title: &str, body: &str, ok: &str) -> bool {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        /// NSAlertFirstButtonReturn
        const FIRST: isize = 1000;
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![&*app, activateIgnoringOtherApps: true];
            let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
            let _: () = msg_send![&*alert, setMessageText: &*NSString::from_str(title)];
            let _: () = msg_send![&*alert, setInformativeText: &*NSString::from_str(body)];
            let _: Retained<AnyObject> = msg_send![&*alert, addButtonWithTitle: &*NSString::from_str(ok)];
            let _: Retained<AnyObject> = msg_send![&*alert, addButtonWithTitle: &*NSString::from_str("Cancel")];
            // over Finder even when the app isn't frontmost yet (just
            // launched by a double-click): floating, and kept on deactivate
            let w: Retained<AnyObject> = msg_send![&*alert, window];
            let _: () = msg_send![&*w, setHidesOnDeactivate: false];
            /// NSModalPanelWindowLevel
            const MODAL_LEVEL: isize = 8;
            let _: () = msg_send![&*w, setLevel: MODAL_LEVEL];
            // lay it out first, or it opens at the screen's corner
            let _: () = msg_send![&*alert, layout];
            let _: () = msg_send![&*w, center];
            let _: () = msg_send![&*w, orderFrontRegardless];
            let r: isize = msg_send![&*alert, runModal];
            r == FIRST
        }
    }

    /// The app is frontmost.
    pub fn app_active() -> bool {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            msg_send![&*app, isActive]
        }
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

/// A running app the person can see (in the Dock).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenApp {
    pub pid: i32,
    pub id: String,
    pub name: String,
    pub bundle: Option<std::path::PathBuf>,
}

/// Windows whose title starts with this get no glass layer behind them: a
/// tool's floating windows that draw everything themselves (pinned images,
/// overlays).
pub const PLAIN_WINDOW: &str = "plain-";

#[cfg(not(target_os = "macos"))]
mod other {
    pub fn apply_glass(_r: f64) {}
    pub fn liquid_glass() -> bool {
        false
    }
    pub fn sync_card_glass(_v: *mut std::ffi::c_void, _clip: [f32; 4], _cards: &[[f32; 4]], _radius: f64, _light: bool, _glow: impl FnOnce(u32, u32) -> Vec<u8>) {}
    pub fn hide_card_glass() {}
    pub fn system_light() -> bool {
        false
    }
    pub fn set_appearance(_light: bool) {}
    pub fn watch_theme(_f: impl Fn() + 'static) {}
    pub fn debug_windows() -> String {
        String::new()
    }
    pub fn set_dock_icon(_v: bool) {}
    pub fn install_window_menu() {}
    pub fn bring_app_window_front() {}
    pub fn activate() {}
    pub fn key_window_title() -> Option<String> {
        None
    }
    pub fn front_other_app() -> Option<i32> {
        None
    }
    pub fn give_focus_to(_pid: i32) -> bool {
        false
    }
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
    pub fn open_apps() -> Vec<super::OpenApp> {
        vec![]
    }
    pub fn quit_app(_pid: i32) -> bool {
        false
    }
    pub fn app_alive(_pid: i32) -> bool {
        false
    }
    pub fn app_active() -> bool {
        true
    }
    /// No alert off macOS: never confirmed.
    pub fn confirm(_title: &str, _body: &str, _ok: &str) -> bool {
        false
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
