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
    /// With Settings › General "Show in Dock" off it stays Accessory.
    pub fn set_dock_icon(visible: bool) {
        super::DOCK_WANTED.store(visible, std::sync::atomic::Ordering::Relaxed);
        let visible = visible && super::show_in_dock();
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

    /// Where the menu bar icon is, in points with a top-left origin on the
    /// main display (the space window positions use). Asks the icon's own
    /// window, so it is right on any display, at any scale, in any
    /// arrangement. None while the icon isn't in a menu bar yet.
    pub fn menu_icon_place(item: *mut std::ffi::c_void) -> Option<super::MenuIconPlace> {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::{NSScreen, NSWindow};
        let mtm = MainThreadMarker::new()?;
        let item = unsafe { (item as *const AnyObject).as_ref()? };
        let button: Option<Retained<AnyObject>> = unsafe { msg_send![item, button] };
        let button = button?;
        let window: Option<Retained<NSWindow>> = unsafe { msg_send![&*button, window] };
        let window = window?;
        let screen = window.screen()?;
        let top = NSScreen::screens(mtm).firstObject()?.frame().size.height;
        let flip = |f: objc2_foundation::NSRect| {
            egui::Rect::from_min_size(egui::pos2(f.origin.x as f32, (top - (f.origin.y + f.size.height)) as f32), egui::vec2(f.size.width as f32, f.size.height as f32))
        };
        let icon = flip(window.frame());
        let (frame, visible) = (flip(screen.frame()), flip(screen.visibleFrame()));
        // right after launch the icon sits off the menu bar (bottom-left)
        if icon.width() < 1.0 || icon.height() < 1.0 || icon.max.y - frame.min.y > 60.0 {
            return None;
        }
        // below the menu bar; the panel floats over the Dock, as before
        let visible = egui::Rect::from_min_max(egui::pos2(frame.min.x, visible.min.y), frame.max);
        Some(super::MenuIconPlace { icon, visible })
    }

    /// The menu bar panel shows on whatever Space is on screen, a full-screen
    /// app's too. Without this macOS kept it on the Space it last showed on
    /// (a full-screen Space gone since), so a click on the icon showed nothing.
    pub fn panel_follows_spaces(id: egui::ViewportId) {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        let Some(_mtm) = MainThreadMarker::new() else { return };
        let Some(view) = eframe::viewport_ns_view(id) else { return };
        let Some(view) = (unsafe { (view as *const AnyObject).as_ref() }) else { return };
        let window: Option<Retained<NSWindow>> = unsafe { msg_send![view, window] };
        let Some(window) = window else { return };
        // on every Space, not "move to the active one": after full-screen
        // sweeps macOS can keep a dead full-screen tile Space as the
        // "active" one while Desktop 1 is on screen, and a window moved
        // there never shows (seen on the M1)
        let want = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle;
        if window.collectionBehavior() != want {
            window.setCollectionBehavior(want);
            // a window already ordered in takes the new Spaces on its next
            // order-in
            if window.isVisible() {
                window.orderFrontRegardless();
            }
        }
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

    /// Call `f` on every left press on the menu bar icon, seen as the app
    /// takes the event in, before any view. tray-icon's own view only
    /// reports a click on the release, and after full-screen sweeps on
    /// macOS 26 the releases stopped reaching it (the presses still came):
    /// a click on the icon did nothing. False when it can't watch.
    pub fn watch_menu_icon_presses(item: *mut std::ffi::c_void, f: impl Fn() + 'static) -> bool {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let Some(_mtm) = MainThreadMarker::new() else { return false };
        let Some(item) = (unsafe { Retained::retain(item as *mut AnyObject) }) else { return false };
        // a release whose press never came by (seen once in ten on the
        // M1) counts as the click
        let pressed = std::cell::Cell::new(false);
        let block = block2::RcBlock::new(move |e: std::ptr::NonNull<AnyObject>| -> *mut AnyObject {
            unsafe {
                use objc2_app_kit::NSEventType;
                let kind = (*(e.as_ptr() as *const objc2_app_kit::NSEvent)).r#type();
                let window: Option<Retained<AnyObject>> = msg_send![e.as_ref(), window];
                let button: Option<Retained<AnyObject>> = msg_send![&*item, button];
                let icon_window: Option<Retained<AnyObject>> = button.and_then(|b| msg_send![&*b, window]);
                let on_icon = matches!((&window, &icon_window), (Some(w), Some(iw)) if Retained::as_ptr(w) == Retained::as_ptr(iw));
                if kind == NSEventType::LeftMouseDown {
                    pressed.set(on_icon);
                    if on_icon {
                        f();
                        // taken here: the icon's button would start its own
                        // mouse tracking, which ate the release and held the
                        // app's run loop (the panel opened late, or a second
                        // click merged into the first)
                        return std::ptr::null_mut();
                    }
                } else if kind == NSEventType::LeftMouseUp {
                    if on_icon && !pressed.get() {
                        f();
                    }
                    pressed.set(false);
                }
            }
            e.as_ptr()
        });
        unsafe {
            // NSEventMaskLeftMouseDown | NSEventMaskLeftMouseUp
            let mask: u64 = (1 << 1) | (1 << 2);
            let token: Option<Retained<AnyObject>> = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: mask, handler: &*block];
            let ok = token.is_some();
            // watched for the app's whole life
            std::mem::forget(token);
            std::mem::forget(block);
            ok
        }
    }

    /// Show the menu bar icon pressed (while the panel is open), as the
    /// system's own menu bar items do.
    pub fn set_menu_icon_lit(item: *mut std::ffi::c_void, on: bool) {
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};
        let Some(_mtm) = MainThreadMarker::new() else { return };
        let Some(item) = (unsafe { (item as *const AnyObject).as_ref() }) else { return };
        unsafe {
            let button: Option<Retained<AnyObject>> = msg_send![item, button];
            if let Some(b) = button {
                let _: () = msg_send![&*b, highlight: Bool::new(on)];
            }
        }
    }

    /// Call `f` on every press that goes to another app (a click outside
    /// our windows). The panel hides on it: it can't count on losing focus,
    /// since macOS may not make the app active when the panel opens.
    pub fn watch_clicks_elsewhere(f: impl Fn() + 'static) {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let block = block2::RcBlock::new(move |_e: std::ptr::NonNull<AnyObject>| f());
        unsafe {
            // NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown | NSEventMaskOtherMouseDown
            let mask: u64 = (1 << 1) | (1 << 3) | (1 << 25);
            let token: Option<Retained<AnyObject>> = msg_send![class!(NSEvent), addGlobalMonitorForEventsMatchingMask: mask, handler: &*block];
            // watched for the app's whole life
            std::mem::forget(token);
        }
        std::mem::forget(block);
    }

    /// Debug (CMDMEOW_TRAY_LOG=1): log every left press/release our app
    /// receives, with the window and the view under it, to see whether a
    /// click on the menu bar icon reaches the app at all.
    pub fn log_left_clicks() {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let block = block2::RcBlock::new(move |e: std::ptr::NonNull<AnyObject>| -> *mut AnyObject {
            unsafe {
                let ev = e.as_ref();
                let kind = (*(e.as_ptr() as *const objc2_app_kit::NSEvent)).r#type().0;
                let window: Option<Retained<AnyObject>> = msg_send![ev, window];
                let loc: objc2_foundation::NSPoint = msg_send![ev, locationInWindow];
                let (num, wclass, hit) = match &window {
                    Some(w) => {
                        let num: isize = msg_send![&**w, windowNumber];
                        let content: Option<Retained<AnyObject>> = msg_send![&**w, contentView];
                        let sup: Option<Retained<AnyObject>> = content.as_ref().and_then(|c| msg_send![&**c, superview]);
                        let hit: Option<Retained<AnyObject>> = sup.as_ref().or(content.as_ref()).and_then(|v| msg_send![&**v, hitTest: loc]);
                        (num, (**w).class().name().to_string_lossy().into_owned(), hit.map(|h| (*h).class().name().to_string_lossy().into_owned()).unwrap_or_default())
                    }
                    None => (-1, String::new(), String::new()),
                };
                eprintln!("[tray-log] mouse type {kind} window #{num} {wclass} at ({:.1},{:.1}) hit {hit}", loc.x, loc.y);
            }
            e.as_ptr()
        });
        unsafe {
            // NSEventMaskLeftMouseDown | NSEventMaskLeftMouseUp
            let mask: u64 = (1 << 1) | (1 << 2);
            let token: Option<Retained<AnyObject>> = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: mask, handler: &*block];
            std::mem::forget(token);
        }
        std::mem::forget(block);
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

/// The menu bar icon and the part of its display below the menu bar, in
/// points, top-left origin on the main display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuIconPlace {
    pub icon: egui::Rect,
    pub visible: egui::Rect,
}

/// Settings › General "Show in Dock": off keeps the app a menu bar app
/// (no Dock icon, no ⌘Tab) even while its window is open. On by default.
pub const SHOW_IN_DOCK_KEY: &str = "app.show_in_dock";
static SHOW_IN_DOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// What the app last asked for (window open or not), so a switch flip
/// applies at once.
static DOCK_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn show_in_dock() -> bool {
    SHOW_IN_DOCK.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_show_in_dock(on: bool) {
    SHOW_IN_DOCK.store(on, std::sync::atomic::Ordering::Relaxed);
    let wanted = DOCK_WANTED.load(std::sync::atomic::Ordering::Relaxed);
    set_dock_icon(wanted);
    // leaving the Dock can drop the app's activation: keep its window in front
    if wanted && !on {
        activate();
    }
}

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
    pub fn install_window_menu() {}
    pub fn bring_app_window_front() {}
    pub fn activate() {}
    pub fn menu_icon_place(_item: *mut std::ffi::c_void) -> Option<super::MenuIconPlace> {
        None
    }
    pub fn panel_follows_spaces(_id: egui::ViewportId) {}
    pub fn log_left_clicks() {}
    pub fn watch_clicks_elsewhere(_f: impl Fn() + 'static) {}
    pub fn set_menu_icon_lit(_item: *mut std::ffi::c_void, _on: bool) {}
    pub fn watch_menu_icon_presses(_item: *mut std::ffi::c_void, _f: impl Fn() + 'static) -> bool {
        false
    }
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
