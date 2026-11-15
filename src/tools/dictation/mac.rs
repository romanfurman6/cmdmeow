//! macOS-only pieces of Dictation: where the text caret is (Accessibility),
//! the app in front, pasting into it, and keeping the pill above everything.
//! Elsewhere these do nothing, so the tool builds and its tests run on Linux.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::fit::Around;
use egui::{Pos2, Rect};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The app the words go into.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub pid: i32,
    pub name: String,
}

/// What was on the clipboard before we pasted: every item, every type.
#[derive(Default)]
pub struct Saved {
    items: Vec<Vec<(String, Vec<u8>)>>,
}

/// Where the typing happens, in global points (top-left origin): the
/// caret's rect and the focused text field's frame, each when the app
/// tells it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spot {
    pub caret: Option<Rect>,
    pub field: Option<Rect>,
}

impl Spot {
    /// A point to find the screen by.
    pub fn center(&self) -> Option<Pos2> {
        self.field.or(self.caret).map(|r| r.center())
    }
}

/// The caret and the focused field of `target`.
pub fn spot(target: &Target) -> Spot {
    let (caret, frame) = imp::spot(target.pid);
    // a caret outside its own field is made up (Chromium does this): drop
    // it, even when the field is too tall to sit above
    let frame = frame.filter(|f| f.is_finite() && !(f.min.x.abs() < 1.0 && f.min.y.abs() < 1.0) && f.width() >= 40.0);
    let caret = caret.filter(|c| frame.is_none_or(|f| f.expand(24.0).intersects(*c)));
    Spot { caret, field: frame.filter(|f| field_like(*f)) }
}

/// A frame that reads as a text field to sit above: not a whole document
/// or web page, not a speck.
pub fn field_like(r: Rect) -> bool {
    let at_origin = r.min.x.abs() < 1.0 && r.min.y.abs() < 1.0;
    r.is_finite() && !at_origin && r.width() >= 40.0 && r.height() >= 12.0 && r.height() <= 320.0
}

/// A rect that can't be a caret: empty at the origin, or absurdly big.
pub fn plausible_caret(r: Rect) -> bool {
    let at_origin = r.min.x.abs() < 1.0 && r.min.y.abs() < 1.0;
    r.is_finite() && !at_origin && r.height() > 2.0 && r.height() < 200.0 && r.width() < 2000.0
}

/// The text around the caret in `target`'s focused field; None when the
/// app doesn't say (or it's a password field).
pub fn around(target: &Target) -> Option<Around> {
    imp::around(target.pid)
}

/// When the app reads what we put on the clipboard.
#[derive(Clone, Copy, Debug, Default)]
pub struct Receipt {
    /// Clipboard change count after our write.
    pub count: isize,
    /// The last read, and the change count after we answered it.
    pub read: Option<(Instant, isize)>,
}

/// A paste in flight: what was on the clipboard before, and whether the
/// app has taken the text yet.
pub struct Pasted {
    pub saved: Saved,
    pub pressed: Instant,
    pub receipt: Arc<Mutex<Receipt>>,
    /// The object that hands the text over when it's read; the clipboard
    /// calls it, so it lives until the clipboard is put back.
    _owner: Option<Box<dyn std::any::Any>>,
}

impl Pasted {
    /// The change count the clipboard has while it still holds our text.
    pub fn count(&self) -> isize {
        let r = self.receipt.lock().map(|r| *r).unwrap_or_default();
        r.read.map(|(_, c)| c).unwrap_or(r.count)
    }

    /// Take the clipboard from before out (a newer paste keeps it).
    pub fn take_saved(&mut self) -> Saved {
        std::mem::take(&mut self.saved)
    }

    pub fn last_read(&self) -> Option<Instant> {
        self.receipt.lock().ok().and_then(|r| r.read.map(|(t, _)| t))
    }
}

/// Put back the clipboard from before `p` (unless someone copied since),
/// then let the text's owner go.
pub fn put_back(mut p: Pasted) {
    let count = p.count();
    restore(p.take_saved(), count);
    drop(p);
}

/// After the app reads the text, it may read again right away (Chromium
/// looks, then reads): wait this long after the last read.
pub const READ_QUIET: Duration = Duration::from_millis(200);
/// No read seen after ⌘V (or it came before, from a clipboard manager):
/// put the clipboard back after this long anyway.
pub const READ_GIVE_UP: Duration = Duration::from_secs(4);

/// Whether to put the old clipboard back now. Only a read after ⌘V was
/// pressed is the app pasting; an earlier one is someone watching the
/// clipboard.
pub fn restore_due(pressed: Instant, last_read: Option<Instant>, now: Instant) -> bool {
    match last_read {
        Some(r) if r >= pressed => now >= r + READ_QUIET,
        _ => now >= pressed + READ_GIVE_UP,
    }
}

pub use imp::{accessibility, activate, float, front_app, mouse, paste, paste_tracked, restore, screen_at};

#[cfg(target_os = "macos")]
mod imp {
    use super::{plausible_caret, Around, Pasted, Receipt, Saved, Target};
    use egui::{Pos2, Rect, Vec2};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSEvent, NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting, NSRunningApplication, NSScreen, NSWindowCollectionBehavior, NSWorkspace};
    use objc2_foundation::{NSArray, NSData, NSObject, NSObjectProtocol, NSString};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    type CFTypeRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CgRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    const AX_VALUE_POINT: u32 = 1;
    const AX_VALUE_SIZE: u32 = 2;
    const AX_VALUE_RECT: u32 = 3;
    const AX_VALUE_RANGE: u32 = 4;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(e: CFTypeRef, attr: CFTypeRef, out: *mut CFTypeRef) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(e: CFTypeRef, attr: CFTypeRef, param: CFTypeRef, out: *mut CFTypeRef) -> i32;
        fn AXUIElementSetAttributeValue(e: CFTypeRef, attr: CFTypeRef, value: CFTypeRef) -> i32;
        fn AXUIElementSetMessagingTimeout(e: CFTypeRef, secs: f32) -> i32;
        fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
        fn AXValueGetValue(v: CFTypeRef, kind: u32, out: *mut c_void) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(x: CFTypeRef);
        fn CFGetTypeID(x: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(n: CFTypeRef, kind: isize, out: *mut c_void) -> bool;
        static kCFBooleanTrue: CFTypeRef;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceCreate(state: i32) -> CFTypeRef;
        fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CFTypeRef;
        fn CGEventSetFlags(e: CFTypeRef, flags: u64);
        fn CGEventPost(tap: u32, e: CFTypeRef);
    }

    /// An owned CoreFoundation object, released on drop.
    struct Cf(CFTypeRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn attr(e: CFTypeRef, name: &str) -> Option<Cf> {
        let key = NSString::from_str(name);
        let mut out: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(e, Retained::as_ptr(&key).cast(), &mut out) };
        (err == 0 && !out.is_null()).then_some(Cf(out))
    }

    fn param_attr(e: CFTypeRef, name: &str, param: CFTypeRef) -> Option<Cf> {
        let key = NSString::from_str(name);
        let mut out: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyParameterizedAttributeValue(e, Retained::as_ptr(&key).cast(), param, &mut out) };
        (err == 0 && !out.is_null()).then_some(Cf(out))
    }

    fn value<T: Default>(v: &Cf, kind: u32) -> Option<T> {
        let mut out = T::default();
        unsafe { AXValueGetValue(v.0, kind, &mut out as *mut T as *mut c_void) }.then_some(out)
    }

    fn rect_for(elem: CFTypeRef, r: CfRange) -> Option<Rect> {
        let range = Cf(unsafe { AXValueCreate(AX_VALUE_RANGE, &r as *const CfRange as *const c_void) });
        let v = param_attr(elem, "AXBoundsForRange", range.0)?;
        let b: CgRect = value(&v, AX_VALUE_RECT)?;
        Some(Rect::from_min_size(Pos2::new(b.x as f32, b.y as f32), Vec2::new(b.w as f32, b.h as f32)))
    }

    pub fn accessibility() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// The caret's rect and the focused element's frame.
    pub fn spot(pid: i32) -> (Option<Rect>, Option<Rect>) {
        if pid <= 0 || !accessibility() {
            return (None, None);
        }
        let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
        unsafe {
            AXUIElementSetMessagingTimeout(app.0, 0.25);
            // Chromium and Electron apps build their accessibility tree
            // only when asked to
            let key = NSString::from_str("AXManualAccessibility");
            AXUIElementSetAttributeValue(app.0, Retained::as_ptr(&key).cast(), kCFBooleanTrue);
        }
        let Some(focused) = attr(app.0, "AXFocusedUIElement") else { return (None, None) };
        (caret_in(&focused), frame(&focused))
    }

    fn frame(e: &Cf) -> Option<Rect> {
        let pos: CgPoint = attr(e.0, "AXPosition").and_then(|v| value(&v, AX_VALUE_POINT))?;
        let size: CgPoint = attr(e.0, "AXSize").and_then(|v| value(&v, AX_VALUE_SIZE))?;
        Some(Rect::from_min_size(Pos2::new(pos.x as f32, pos.y as f32), Vec2::new(size.x as f32, size.y as f32)))
    }

    fn caret_in(focused: &Cf) -> Option<Rect> {
        let sel = attr(focused.0, "AXSelectedTextRange").and_then(|v| value::<CfRange>(&v, AX_VALUE_RANGE))?;
        let tries = [sel, CfRange { location: sel.location, length: 1 }, CfRange { location: (sel.location - 1).max(0), length: 1 }];
        for (k, r) in tries.iter().enumerate() {
            if let Some(b) = rect_for(focused.0, *r) {
                if plausible_caret(b) {
                    // the character before the caret: its right edge
                    return Some(if k == 2 && sel.location > 0 { Rect::from_min_max(Pos2::new(b.right(), b.top()), b.max) } else { b });
                }
            }
        }
        None
    }

    fn string(v: &Cf) -> Option<String> {
        if unsafe { CFGetTypeID(v.0) != CFStringGetTypeID() } {
            return None;
        }
        Some(unsafe { &*(v.0 as *const NSString) }.to_string())
    }

    fn number(v: &Cf) -> Option<isize> {
        if unsafe { CFGetTypeID(v.0) != CFNumberGetTypeID() } {
            return None;
        }
        let mut out: isize = 0;
        // kCFNumberCFIndexType
        unsafe { CFNumberGetValue(v.0, 14, &mut out as *mut isize as *mut c_void) }.then_some(out)
    }

    fn text_in(elem: CFTypeRef, r: CfRange) -> Option<String> {
        if r.length <= 0 {
            return Some(String::new());
        }
        let range = Cf(unsafe { AXValueCreate(AX_VALUE_RANGE, &r as *const CfRange as *const c_void) });
        string(&param_attr(elem, "AXStringForRange", range.0)?)
    }

    pub fn around(pid: i32) -> Option<Around> {
        if pid <= 0 || !accessibility() {
            return None;
        }
        let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
        unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) };
        let focused = attr(app.0, "AXFocusedUIElement")?;
        if attr(focused.0, "AXSubrole").and_then(|v| string(&v)).as_deref() == Some("AXSecureTextField") {
            return None;
        }
        let sel: CfRange = attr(focused.0, "AXSelectedTextRange").and_then(|v| value(&v, AX_VALUE_RANGE))?;
        if sel.location < 0 || sel.length < 0 {
            return None;
        }
        // a few words each side is all the fitting needs
        let from = (sel.location - 64).max(0);
        let before = text_in(focused.0, CfRange { location: from, length: sel.location - from })?;
        let start = sel.location + sel.length;
        let after = match attr(focused.0, "AXNumberOfCharacters").and_then(|v| number(&v)) {
            Some(total) => text_in(focused.0, CfRange { location: start, length: (total - start).clamp(0, 16) }),
            None => text_in(focused.0, CfRange { location: start, length: 16 }).or_else(|| text_in(focused.0, CfRange { location: start, length: 1 })),
        }
        .unwrap_or_default();
        Some(Around { before, after })
    }

    pub fn front_app() -> Option<Target> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(Target { pid: app.processIdentifier(), name: app.localizedName().map(|n| n.to_string()).unwrap_or_default() })
    }

    /// Bring `t` to the front again (in case clicking our pill or panel
    /// took it away).
    pub fn activate(t: &super::Target) {
        if t.pid <= 0 || t.pid == std::process::id() as i32 {
            return;
        }
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(t.pid) {
            if !app.isActive() {
                #[allow(deprecated)]
                let _ = app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            }
        }
    }

    fn main_height(mtm: MainThreadMarker) -> f64 {
        NSScreen::screens(mtm).firstObject().map(|s| s.frame().size.height).unwrap_or(0.0)
    }

    pub fn mouse() -> Option<Pos2> {
        let mtm = MainThreadMarker::new()?;
        let p = NSEvent::mouseLocation();
        Some(Pos2::new(p.x as f32, (main_height(mtm) - p.y) as f32))
    }

    /// The visible part (no menu bar, no Dock) of the screen holding `p`,
    /// top-left origin.
    pub fn screen_at(p: Pos2) -> Option<Rect> {
        let mtm = MainThreadMarker::new()?;
        let top = main_height(mtm);
        let rects: Vec<(Rect, Rect)> = NSScreen::screens(mtm)
            .iter()
            .map(|s| {
                let flip = |f: objc2_foundation::NSRect| Rect::from_min_size(Pos2::new(f.origin.x as f32, (top - f.origin.y - f.size.height) as f32), Vec2::new(f.size.width as f32, f.size.height as f32));
                (flip(s.frame()), flip(s.visibleFrame()))
            })
            .collect();
        rects.iter().find(|(f, _)| f.contains(p)).or(rects.first()).map(|(_, v)| *v)
    }

    /// Keep our window `title` above other apps' windows, on every Space
    /// and over full-screen apps, and never key (typing stays where it was).
    pub fn float(title: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        for w in NSApplication::sharedApplication(mtm).windows().iter() {
            if w.title().to_string() != title {
                continue;
            }
            // NSStatusWindowLevel: above floating panels and the Dock
            if w.level() != 25 {
                w.setLevel(25);
            }
            let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle;
            if w.collectionBehavior() != behavior {
                w.setCollectionBehavior(behavior);
            }
            if !w.ignoresMouseEvents() {
                w.setIgnoresMouseEvents(true);
            }
        }
    }

    /// Put `text` on the clipboard and press ⌘V in the app in front. The
    /// write is marked transient, so clipboard managers (ours too) skip it.
    /// Returns what was there before (for [`restore`]) and the clipboard's
    /// change count after our write; None if nothing could be written.
    /// `press`: false only puts it on the clipboard.
    pub fn paste(text: &str, press: bool) -> Option<(Saved, isize)> {
        let pb = NSPasteboard::generalPasteboard();
        let saved = save(&pb);
        pb.clearContents();
        let transient = NSString::from_str("org.nspasteboard.TransientType");
        let ok = unsafe { pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
        if !ok {
            return None;
        }
        pb.setData_forType(Some(&NSData::new()), &transient);
        let count = pb.changeCount();
        if press {
            press_cmd_v();
        }
        Some((saved, count))
    }

    struct OwnerState {
        text: String,
        receipt: Arc<Mutex<Receipt>>,
    }

    define_class!(
        /// Hands the text to the clipboard the moment an app reads it, so
        /// we know the paste happened.
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowDictationPaste"]
        #[ivars = OwnerState]
        struct Owner;

        unsafe impl NSObjectProtocol for Owner {}

        impl Owner {
            #[unsafe(method(pasteboard:provideDataForType:))]
            fn provide(&self, pb: &NSPasteboard, ty: &NSString) {
                let st = self.ivars();
                if ty.to_string() == unsafe { NSPasteboardTypeString }.to_string() {
                    let _ = pb.setString_forType(&NSString::from_str(&st.text), ty);
                }
                let count = pb.changeCount();
                if let Ok(mut r) = st.receipt.lock() {
                    r.read = Some((Instant::now(), count));
                }
            }
        }
    );

    /// Like [`paste`] with ⌘V, but the text is only promised: the clipboard
    /// asks us for it when the app reads it, which tells the caller when
    /// it's safe to put the old clipboard back.
    pub fn paste_tracked(text: &str) -> Option<Pasted> {
        let pb = NSPasteboard::generalPasteboard();
        let saved = save(&pb);
        let receipt = Arc::new(Mutex::new(Receipt::default()));
        let owner: Retained<Owner> = unsafe { msg_send![super(Owner::alloc().set_ivars(OwnerState { text: text.to_string(), receipt: receipt.clone() })), init] };
        let transient = NSString::from_str("org.nspasteboard.TransientType");
        let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeString }, &*transient]);
        let o: &AnyObject = &owner;
        unsafe { pb.declareTypes_owner(&types, Some(o)) };
        if !pb.setData_forType(Some(&NSData::new()), &transient) {
            return None;
        }
        if let Ok(mut r) = receipt.lock() {
            r.count = pb.changeCount();
        }
        press_cmd_v();
        Some(Pasted { saved, pressed: Instant::now(), receipt, _owner: Some(Box::new(owner)) })
    }

    fn press_cmd_v() {
        const KEY_V: u16 = 9;
        const FLAG_CMD: u64 = 0x0010_0000;
        unsafe {
            // a private source: modifiers still held on the keyboard (the
            // shortcut's ⌥) don't mix into our ⌘V
            let src = Cf(CGEventSourceCreate(-1));
            for down in [true, false] {
                let e = Cf(CGEventCreateKeyboardEvent(src.0, KEY_V, down));
                if e.0.is_null() {
                    return;
                }
                CGEventSetFlags(e.0, FLAG_CMD);
                // kCGHIDEventTap
                CGEventPost(0, e.0);
            }
        }
    }

    fn save(pb: &NSPasteboard) -> Saved {
        let mut items = Vec::new();
        if let Some(list) = pb.pasteboardItems() {
            for it in list.iter() {
                let mut kinds = Vec::new();
                for ty in it.types().iter() {
                    if let Some(d) = it.dataForType(&ty) {
                        kinds.push((ty.to_string(), d.to_vec()));
                    }
                }
                if !kinds.is_empty() {
                    items.push(kinds);
                }
            }
        }
        Saved { items }
    }

    /// Put back what [`paste`] replaced, unless someone copied since
    /// (`count` no longer matches).
    pub fn restore(saved: Saved, count: isize) {
        let pb = NSPasteboard::generalPasteboard();
        if pb.changeCount() != count {
            return;
        }
        pb.clearContents();
        // it was empty before: leave it empty, not holding what was said
        if saved.items.is_empty() {
            return;
        }
        let transient = NSString::from_str("org.nspasteboard.TransientType");
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = saved
            .items
            .iter()
            .map(|kinds| {
                let it = NSPasteboardItem::new();
                for (ty, data) in kinds {
                    it.setData_forType(&NSData::with_bytes(data), &NSString::from_str(ty));
                }
                // it is already in clipboard history; don't add it twice
                it.setData_forType(&NSData::new(), &transient);
                ProtocolObject::from_retained(it)
            })
            .collect();
        pb.writeObjects(&NSArray::from_retained_slice(&items));
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::{Around, Pasted, Saved, Target};
    use egui::{Pos2, Rect};

    pub fn accessibility() -> bool {
        false
    }
    pub fn around(_pid: i32) -> Option<Around> {
        None
    }
    pub fn paste_tracked(_text: &str) -> Option<Pasted> {
        None
    }
    pub fn spot(_pid: i32) -> (Option<Rect>, Option<Rect>) {
        (None, None)
    }
    pub fn front_app() -> Option<Target> {
        None
    }
    pub fn activate(_t: &Target) {}
    pub fn mouse() -> Option<Pos2> {
        None
    }
    pub fn screen_at(_p: Pos2) -> Option<Rect> {
        None
    }
    pub fn float(_title: &str) {}
    pub fn paste(_text: &str, _press: bool) -> Option<(Saved, isize)> {
        None
    }
    pub fn restore(_saved: Saved, _count: isize) {}
}

/// Top-left of a `size` pill: just above the text field's left edge
/// (below it when there's no room above); in a big field (a document),
/// just under the caret; else under the mouse. Kept inside `screen`.
pub fn pill_pos(spot: Spot, mouse: Option<Pos2>, screen: Option<Rect>, size: egui::Vec2) -> Pos2 {
    const GAP: f32 = 6.0;
    let top = screen.map_or(f32::MIN, |s| s.top() + 4.0);
    let bottom = screen.map_or(f32::MAX, |s| s.bottom() - 4.0);
    let want = match (spot.field, spot.caret, mouse) {
        (Some(f), _, _) => {
            let above = f.top() - GAP - size.y;
            let y = if above >= top { above } else if f.bottom() + GAP + size.y <= bottom { f.bottom() + GAP } else { above };
            Pos2::new(f.left(), y)
        }
        (None, Some(c), _) => {
            let below = c.bottom() + GAP;
            Pos2::new(c.left() - 14.0, if below + size.y > bottom { c.top() - GAP - size.y } else { below })
        }
        (None, None, Some(m)) => {
            let below = m.y + 22.0;
            Pos2::new(m.x - size.x / 2.0, if below + size.y > bottom { bottom - size.y } else { below })
        }
        (None, None, None) => Pos2::new(600.0, 400.0),
    };
    let Some(s) = screen else { return want };
    Pos2::new(
        want.x.clamp(s.left() + 4.0, (s.right() - 4.0 - size.x).max(s.left() + 4.0)),
        want.y.clamp(s.top() + 4.0, (s.bottom() - 4.0 - size.y).max(s.top() + 4.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Vec2;

    #[test]
    fn carets_that_cant_be_real_are_refused() {
        assert!(plausible_caret(Rect::from_min_size(Pos2::new(300.0, 200.0), Vec2::new(1.0, 17.0))));
        assert!(!plausible_caret(Rect::from_min_size(Pos2::ZERO, Vec2::new(1.0, 17.0))));
        assert!(!plausible_caret(Rect::from_min_size(Pos2::new(300.0, 200.0), Vec2::new(0.0, 0.0))));
        assert!(!plausible_caret(Rect::from_min_size(Pos2::new(300.0, 200.0), Vec2::new(900.0, 700.0))));
    }

    #[test]
    fn the_clipboard_comes_back_after_the_app_reads_it() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        // read 50 ms after ⌘V: back 200 ms after that
        assert!(!restore_due(t0, Some(ms(50)), ms(200)));
        assert!(restore_due(t0, Some(ms(50)), ms(250)));
        // a slow app reading after 2 s still gets the new text
        assert!(!restore_due(t0, None, ms(2000)));
        assert!(!restore_due(t0, Some(ms(2000)), ms(2100)));
        // a read before ⌘V (a clipboard watcher) doesn't count
        assert!(!restore_due(ms(100), Some(t0), ms(500)));
        // never read: give up after a while
        assert!(restore_due(t0, None, ms(4000)));
    }

    #[test]
    fn the_pill_sits_above_the_field_at_its_left_edge() {
        let screen = Some(Rect::from_min_size(Pos2::new(0.0, 25.0), Vec2::new(1440.0, 850.0)));
        let size = Vec2::new(140.0, 36.0);
        let field = Rect::from_min_size(Pos2::new(400.0, 600.0), Vec2::new(700.0, 80.0));
        let caret = Rect::from_min_size(Pos2::new(900.0, 650.0), Vec2::new(1.0, 18.0));
        let spot = Spot { caret: Some(caret), field: Some(field) };
        assert_eq!(pill_pos(spot, None, screen, size), Pos2::new(400.0, 600.0 - 6.0 - 36.0));
        // a field at the top of the screen: under it instead
        let high = Spot { caret: None, field: Some(Rect::from_min_size(Pos2::new(400.0, 40.0), Vec2::new(300.0, 24.0))) };
        assert_eq!(pill_pos(high, None, screen, size), Pos2::new(400.0, 40.0 + 24.0 + 6.0));
        // at the right edge: pulled in
        let right = Spot { caret: None, field: Some(Rect::from_min_size(Pos2::new(1400.0, 300.0), Vec2::new(40.0, 24.0))) };
        assert_eq!(pill_pos(right, None, screen, size).x, 1440.0 - 4.0 - 140.0);
    }

    #[test]
    fn without_a_field_the_pill_follows_the_caret_then_the_mouse() {
        let screen = Some(Rect::from_min_size(Pos2::new(0.0, 25.0), Vec2::new(1440.0, 850.0)));
        let size = Vec2::new(140.0, 36.0);
        let caret = Some(Rect::from_min_size(Pos2::new(500.0, 300.0), Vec2::new(1.0, 18.0)));
        assert_eq!(pill_pos(Spot { caret, field: None }, None, screen, size), Pos2::new(486.0, 324.0));
        // at the bottom: above the caret
        let low = Some(Rect::from_min_size(Pos2::new(500.0, 850.0), Vec2::new(1.0, 18.0)));
        assert_eq!(pill_pos(Spot { caret: low, field: None }, None, screen, size).y, 850.0 - 6.0 - 36.0);
        // nothing: under the mouse
        assert_eq!(pill_pos(Spot::default(), Some(Pos2::new(700.0, 400.0)), screen, size), Pos2::new(630.0, 422.0));
    }

    #[test]
    fn documents_and_web_pages_are_not_fields() {
        assert!(field_like(Rect::from_min_size(Pos2::new(400.0, 600.0), Vec2::new(700.0, 80.0))));
        assert!(!field_like(Rect::from_min_size(Pos2::new(0.0, 25.0), Vec2::new(1440.0, 800.0))));
        assert!(!field_like(Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 24.0))));
        assert!(!field_like(Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(10.0, 24.0))));
    }
}
