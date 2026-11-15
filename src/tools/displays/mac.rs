//! The macOS side of Displays: listing displays, reaching a monitor over
//! DDC (Apple silicon's AV service), macOS's own brightness for Apple panels,
//! darkening the picture through the gamma table, where the sound goes, and
//! the brightness and volume keys. Elsewhere it finds no displays, so the
//! tool builds and its tests run on Linux.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::engine::{Audio, Bus, Found, Key};
use egui::Pos2;

/// The system-defined event that carries the media keys.
const NX_SYSDEFINED: u32 = 14;
/// Its subtype for the keyboard's special keys.
const AUX_KEYS: i16 = 8;

/// A media key from an NSSystemDefined event's subtype and data1:
/// (key, pressed, repeat). Key up returns pressed = false.
pub fn decode(subtype: i16, data1: i64) -> Option<(Key, bool, bool)> {
    if subtype != AUX_KEYS {
        return None;
    }
    let d = data1 as u32;
    let code = (d >> 16) & 0xFFFF;
    let state = (d >> 8) & 0xFF;
    let repeat = d & 1 == 1;
    let key = match code {
        0 => Key::VolumeUp,
        1 => Key::VolumeDown,
        2 => Key::BrightnessUp,
        3 => Key::BrightnessDown,
        7 => Key::Mute,
        _ => return None,
    };
    match state {
        0x0A => Some((key, true, repeat)),
        0x0B => Some((key, false, repeat)),
        _ => None,
    }
}

/// ⌥⇧ held: a fine step.
pub fn fine(flags: u64) -> bool {
    const SHIFT: u64 = 0x0002_0000;
    const ALT: u64 = 0x0008_0000;
    flags & SHIFT != 0 && flags & ALT != 0
}

/// Decides a key press: true takes the key away from macOS.
pub type Handler = Box<dyn Fn(Key, bool, Pos2) -> bool + Send + Sync>;

pub use imp::{audio, float, start_keys, MacBus};

#[cfg(target_os = "macos")]
mod imp {
    use super::super::ddc::{self, Reading, REPLY_LEN};
    use super::*;
    use egui::{Rect, Vec2};
    use std::collections::HashMap;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::time::Duration;

    type CfRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CgRect {
        origin: CgPoint,
        w: f64,
        h: f64,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: CfRef);
        fn CFGetTypeID(cf: CfRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFStringCreateWithCString(alloc: CfRef, s: *const c_char, encoding: u32) -> CfRef;
        fn CFStringGetCString(s: CfRef, buf: *mut c_char, len: isize, encoding: u32) -> u8;
        fn CFDictionaryGetValue(d: CfRef, key: CfRef) -> CfRef;
        fn CFDictionaryGetCount(d: CfRef) -> isize;
        fn CFDictionaryGetKeysAndValues(d: CfRef, keys: *mut CfRef, values: *mut CfRef);
        fn CFMachPortCreateRunLoopSource(alloc: CfRef, port: CfRef, order: isize) -> CfRef;
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopAddSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: CfRef;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetOnlineDisplayList(max: u32, ids: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayIsBuiltin(id: u32) -> u32;
        fn CGDisplayVendorNumber(id: u32) -> u32;
        fn CGDisplayModelNumber(id: u32) -> u32;
        fn CGDisplaySerialNumber(id: u32) -> u32;
        fn CGDisplayBounds(id: u32) -> CgRect;
        fn CGGetDisplayTransferByTable(id: u32, cap: u32, r: *mut f32, g: *mut f32, b: *mut f32, count: *mut u32) -> i32;
        fn CGSetDisplayTransferByTable(id: u32, n: u32, r: *const f32, g: *const f32, b: *const f32) -> i32;
        fn CGDisplayRestoreColorSyncSettings();
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, info: *mut c_void) -> CfRef;
        fn CGEventTapEnable(tap: CfRef, on: bool);
        fn CGEventGetFlags(e: CfRef) -> u64;
        fn CGEventGetLocation(e: CfRef) -> CgPoint;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegistryGetRootEntry(main_port: u32) -> u32;
        fn IORegistryEntryCreateIterator(entry: u32, plane: *const c_char, options: u32, iter: *mut u32) -> i32;
        fn IOIteratorNext(iter: u32) -> u32;
        fn IOObjectConformsTo(obj: u32, class: *const c_char) -> u32;
        fn IOObjectRelease(obj: u32) -> i32;
        fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
        fn IORegistryEntryGetName(entry: u32, name: *mut c_char) -> i32;
        fn IORegistryEntryCreateCFProperty(entry: u32, key: CfRef, alloc: CfRef, options: u32) -> CfRef;
        fn IORegistryEntryGetParentEntry(entry: u32, plane: *const c_char, parent: *mut u32) -> i32;
        fn IORegistryEntryCopyFromPath(main_port: u32, path: CfRef) -> u32;
    }

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyData(obj: u32, addr: *const AudioAddr, qsize: u32, qdata: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    }

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    const UTF8: u32 = 0x0800_0100;
    const ITERATE_RECURSIVELY: u32 = 1;
    const SERVICE_PLANE: &CStr = c"IOService";
    const APPLE_VENDOR: u32 = 0x610;
    const GAMMA_LEN: usize = 256;

    // ------------------------------------------------------------ CF helpers

    /// An owned CF object, released on drop.
    struct Cf(CfRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }
    // the AV service objects are only ever used from the worker thread
    unsafe impl Send for Cf {}

    fn cfstr(s: &str) -> Cf {
        let c = CString::new(s).unwrap_or_default();
        Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
    }

    fn string_of(cf: CfRef) -> Option<String> {
        if cf.is_null() || unsafe { CFGetTypeID(cf) != CFStringGetTypeID() } {
            return None;
        }
        let mut buf = [0 as c_char; 512];
        if unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) } == 0 {
            return None;
        }
        Some(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
    }

    fn dict_get(d: CfRef, key: &str) -> CfRef {
        if d.is_null() || unsafe { CFGetTypeID(d) != CFDictionaryGetTypeID() } {
            return std::ptr::null();
        }
        let k = cfstr(key);
        unsafe { CFDictionaryGetValue(d, k.0) }
    }

    /// A localized-names dictionary: English if there, else any.
    fn localized(d: CfRef) -> Option<String> {
        if d.is_null() || unsafe { CFGetTypeID(d) != CFDictionaryGetTypeID() } {
            return None;
        }
        if let Some(s) = string_of(dict_get(d, "en_US")) {
            return Some(s);
        }
        let n = unsafe { CFDictionaryGetCount(d) }.max(0) as usize;
        let mut keys = vec![std::ptr::null(); n];
        let mut values = vec![std::ptr::null(); n];
        unsafe { CFDictionaryGetKeysAndValues(d, keys.as_mut_ptr(), values.as_mut_ptr()) };
        values.into_iter().find_map(string_of)
    }

    // ------------------------------------------------------------ private symbols

    type InfoFn = unsafe extern "C" fn(u32) -> CfRef;
    type BrightnessFn = unsafe extern "C" fn(u32, *mut f32) -> i32;
    type AvCreateFn = unsafe extern "C" fn(CfRef, u32) -> CfRef;
    type AvReadFn = unsafe extern "C" fn(CfRef, u32, u32, *mut u8, u32) -> i32;
    type AvWriteFn = unsafe extern "C" fn(CfRef, u32, u32, *const u8, u32) -> i32;

    struct Private {
        info: Option<InfoFn>,
        ds_get: Option<BrightnessFn>,
        av_create: Option<AvCreateFn>,
        av_read: Option<AvReadFn>,
        av_write: Option<AvWriteFn>,
    }

    /// Looked up at run time, so a macOS without one of them still runs
    /// (then that way of reaching a display is simply missing).
    fn private() -> &'static Private {
        static P: std::sync::OnceLock<Private> = std::sync::OnceLock::new();
        P.get_or_init(|| unsafe {
            let open = |p: &CStr| dlopen(p.as_ptr(), 1);
            let core_display = open(c"/System/Library/Frameworks/CoreDisplay.framework/CoreDisplay");
            let display_services = open(c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices");
            let iokit = open(c"/System/Library/Frameworks/IOKit.framework/IOKit");
            let sym = |h: *mut c_void, name: &CStr| if h.is_null() { std::ptr::null_mut() } else { dlsym(h, name.as_ptr()) };
            macro_rules! f {
                ($h:expr, $name:expr, $t:ty) => {{
                    let p = sym($h, $name);
                    if p.is_null() {
                        None
                    } else {
                        Some(std::mem::transmute::<*mut c_void, $t>(p))
                    }
                }};
            }
            Private {
                info: f!(core_display, c"CoreDisplay_DisplayCreateInfoDictionary", InfoFn),
                ds_get: f!(display_services, c"DisplayServicesGetBrightness", BrightnessFn),
                av_create: f!(iokit, c"IOAVServiceCreateWithService", AvCreateFn),
                av_read: f!(iokit, c"IOAVServiceReadI2C", AvReadFn),
                av_write: f!(iokit, c"IOAVServiceWriteI2C", AvWriteFn),
            }
        })
    }

    fn native_brightness(id: u32) -> Option<f32> {
        let get = private().ds_get?;
        let mut v: f32 = -1.0;
        let ok = unsafe { get(id, &mut v) } == 0;
        (ok && (0.0..=1.0).contains(&v)).then_some(v)
    }

    // ------------------------------------------------------------ the registry walk

    fn entry_name(e: u32) -> String {
        let mut buf = [0 as c_char; 128];
        unsafe { IORegistryEntryGetName(e, buf.as_mut_ptr()) };
        unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
    }

    fn entry_string(e: u32, key: &str) -> Option<String> {
        let k = cfstr(key);
        let v = Cf(unsafe { IORegistryEntryCreateCFProperty(e, k.0, std::ptr::null(), 0) });
        string_of(v.0)
    }

    fn behind_bridge(proxy: u32) -> bool {
        let mut parent = 0u32;
        if unsafe { IORegistryEntryGetParentEntry(proxy, SERVICE_PLANE.as_ptr(), &mut parent) } != 0 {
            return false;
        }
        let bridge = entry_string(parent, "EPICProviderClass").is_some_and(|c| c == "AppleDCPMCDP29XX");
        unsafe { IOObjectRelease(parent) };
        bridge
    }

    /// Every framebuffer's DDC line: framebuffer registry id → (AV service, chip).
    fn ddc_lines() -> HashMap<u64, (Cf, u8)> {
        let mut out = HashMap::new();
        let Some(create) = private().av_create else { return out };
        let mut iter = 0u32;
        let root = unsafe { IORegistryGetRootEntry(0) };
        if unsafe { IORegistryEntryCreateIterator(root, SERVICE_PLANE.as_ptr(), ITERATE_RECURSIVELY, &mut iter) } != 0 {
            return out;
        }
        let mut entries = Vec::new();
        let mut proxies: Vec<u32> = Vec::new();
        loop {
            let e = unsafe { IOIteratorNext(iter) };
            if e == 0 {
                break;
            }
            if unsafe { IOObjectConformsTo(e, c"IOMobileFramebuffer".as_ptr()) } != 0 {
                let mut id = 0u64;
                if unsafe { IORegistryEntryGetRegistryEntryID(e, &mut id) } == 0 {
                    entries.push(ddc::RegEntry::Framebuffer(id));
                    proxies.push(0);
                }
                unsafe { IOObjectRelease(e) };
                continue;
            }
            if entry_name(e) == "DCPAVServiceProxy" {
                let external = entry_string(e, "Location").is_some_and(|l| l == "External");
                entries.push(ddc::RegEntry::Proxy { external, mcdp: external && behind_bridge(e) });
                proxies.push(e);
                continue;
            }
            unsafe { IOObjectRelease(e) };
        }
        unsafe { IOObjectRelease(iter) };
        for (fb, at, chip) in ddc::link_proxies(&entries) {
            let svc = Cf(unsafe { create(std::ptr::null(), proxies[at]) });
            if !svc.0.is_null() {
                out.insert(fb, (svc, chip));
            }
        }
        for p in proxies.into_iter().filter(|&p| p != 0) {
            unsafe { IOObjectRelease(p) };
        }
        out
    }

    /// The registry id of a display's framebuffer, from its info dictionary.
    fn framebuffer_of(info: CfRef) -> Option<u64> {
        let path = dict_get(info, "IODisplayLocation");
        string_of(path)?;
        let e = unsafe { IORegistryEntryCopyFromPath(0, path) };
        if e == 0 {
            return None;
        }
        let mut id = 0u64;
        let ok = unsafe { IORegistryEntryGetRegistryEntryID(e, &mut id) } == 0;
        unsafe { IOObjectRelease(e) };
        ok.then_some(id)
    }

    // ------------------------------------------------------------ the bus

    struct Gamma {
        r: Vec<f32>,
        g: Vec<f32>,
        b: Vec<f32>,
    }

    #[derive(Default)]
    pub struct MacBus {
        /// The online display ids the lines below were found for.
        ids: Vec<u32>,
        lines: HashMap<u32, (Cf, u8)>,
        /// The picture's own gamma table, taken before we first darkened it.
        gamma: HashMap<u32, Gamma>,
        /// Displays we darkened, and how much light they keep.
        dimmed: HashMap<u32, f32>,
        /// More looks for lines still missing (a monitor's line can show up
        /// a few seconds after the display does).
        walks_left: u8,
    }

    impl MacBus {
        fn line(&self, id: u32) -> Option<(CfRef, u8)> {
            self.lines.get(&id).map(|(c, chip)| (c.0, *chip))
        }

        fn set_gamma(&mut self, id: u32, scale: f32) -> bool {
            if !self.gamma.contains_key(&id) {
                let (mut r, mut g, mut b) = (vec![0f32; GAMMA_LEN], vec![0f32; GAMMA_LEN], vec![0f32; GAMMA_LEN]);
                let mut n = 0u32;
                let err = unsafe { CGGetDisplayTransferByTable(id, GAMMA_LEN as u32, r.as_mut_ptr(), g.as_mut_ptr(), b.as_mut_ptr(), &mut n) };
                if err != 0 || n == 0 {
                    return false;
                }
                let n = n as usize;
                r.truncate(n);
                g.truncate(n);
                b.truncate(n);
                self.gamma.insert(id, Gamma { r, g, b });
            }
            let t = &self.gamma[&id];
            let scaled = |v: &[f32]| v.iter().map(|x| x * scale).collect::<Vec<f32>>();
            let (r, g, b) = (scaled(&t.r), scaled(&t.g), scaled(&t.b));
            unsafe { CGSetDisplayTransferByTable(id, r.len() as u32, r.as_ptr(), g.as_ptr(), b.as_ptr()) == 0 }
        }
    }

    impl Bus for MacBus {
        fn list(&mut self) -> Vec<Found> {
            let mut ids = [0u32; 16];
            let mut n = 0u32;
            if unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) } != 0 {
                return Vec::new();
            }
            let mut ids: Vec<u32> = ids[..(n as usize).min(16)].to_vec();
            ids.sort_unstable();
            let changed = ids != self.ids;
            let infos: Vec<(u32, Cf)> = ids.iter().map(|&id| (id, Cf(private().info.map(|f| unsafe { f(id) }).unwrap_or(std::ptr::null())))).collect();
            if changed {
                self.walks_left = 5;
            }
            let missing = ids.iter().any(|&id| !self.lines.contains_key(&id) && unsafe { CGDisplayIsBuiltin(id) } == 0);
            if changed || (missing && self.walks_left > 0) {
                self.walks_left = self.walks_left.saturating_sub(1);
                // the lines belong to framebuffers, which follow the cables
                let mut by_fb = ddc_lines();
                self.lines.clear();
                for (id, info) in &infos {
                    if let Some(line) = framebuffer_of(info.0).and_then(|fb| by_fb.remove(&fb)) {
                        self.lines.insert(*id, line);
                    }
                }
            }
            if changed {
                self.gamma.retain(|id, _| ids.contains(id));
                self.dimmed.retain(|id, _| ids.contains(id));
                // macOS puts every gamma table back when displays change
                for (id, scale) in self.dimmed.clone() {
                    self.set_gamma(id, scale);
                }
                self.ids = ids.clone();
            }
            let mut keys: Vec<String> = Vec::new();
            infos
                .iter()
                .map(|(id, info)| {
                    let id = *id;
                    let builtin = unsafe { CGDisplayIsBuiltin(id) } != 0;
                    let vendor = unsafe { CGDisplayVendorNumber(id) };
                    let mut key = format!("{:x}-{:x}-{:x}", vendor, unsafe { CGDisplayModelNumber(id) }, unsafe { CGDisplaySerialNumber(id) });
                    // two identical monitors without serials
                    let twins = keys.iter().filter(|k| k.split('#').next() == Some(key.as_str())).count();
                    if twins > 0 {
                        key = format!("{key}#{twins}");
                    }
                    keys.push(key.clone());
                    let b = unsafe { CGDisplayBounds(id) };
                    let frame = Rect::from_min_size(Pos2::new(b.origin.x as f32, b.origin.y as f32), Vec2::new(b.w as f32, b.h as f32));
                    let name = localized(dict_get(info.0, "DisplayProductName")).unwrap_or_else(|| if builtin { "Built-in Display".into() } else { "Display".into() });
                    let native = builtin || (vendor == APPLE_VENDOR && native_brightness(id).is_some());
                    Found { id, key, name, builtin, native, ddc: self.lines.contains_key(&id), frame }
                })
                .collect()
        }

        fn ddc_read(&mut self, id: u32, code: u8) -> Option<Reading> {
            let (svc, chip) = self.line(id)?;
            let p = private();
            let (read, write) = (p.av_read?, p.av_write?);
            let req = ddc::get_request(code);
            for _ in 0..4 {
                std::thread::sleep(Duration::from_millis(10));
                if unsafe { write(svc, chip as u32, ddc::HOST as u32, req.as_ptr(), req.len() as u32) } != 0 {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                std::thread::sleep(Duration::from_millis(50));
                let mut buf = [0u8; REPLY_LEN + 1];
                if unsafe { read(svc, chip as u32, 0, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                    if let Ok(r) = ddc::parse_reply(code, &buf) {
                        return Some(r);
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            None
        }

        fn ddc_write(&mut self, id: u32, code: u8, value: u16) -> bool {
            let Some((svc, chip)) = self.line(id) else { return false };
            let Some(write) = private().av_write else { return false };
            let req = ddc::set_request(code, value);
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(10));
                if unsafe { write(svc, chip as u32, ddc::HOST as u32, req.as_ptr(), req.len() as u32) } == 0 {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        }

        fn native_get(&mut self, id: u32) -> Option<f32> {
            native_brightness(id)
        }

        fn dim(&mut self, id: u32, scale: f32) -> bool {
            if scale >= 0.999 {
                if self.dimmed.remove(&id).is_none() {
                    return true;
                }
                // back to the profile's own tables, then darken the others again
                unsafe { CGDisplayRestoreColorSyncSettings() };
                for (other, s) in self.dimmed.clone() {
                    self.set_gamma(other, s);
                }
                return true;
            }
            self.dimmed.insert(id, scale);
            self.set_gamma(id, scale)
        }
    }

    // ------------------------------------------------------------ sound

    #[repr(C)]
    struct AudioAddr {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn four(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    fn prop<T: Default>(obj: u32, selector: u32) -> Option<T> {
        let addr = AudioAddr { selector, scope: four(b"glob"), element: 0 };
        let mut v = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        let err = unsafe { AudioObjectGetPropertyData(obj, &addr, 0, std::ptr::null(), &mut size, &mut v as *mut T as *mut c_void) };
        (err == 0).then_some(v)
    }

    /// Where the sound goes: a monitor over HDMI or DisplayPort (by name),
    /// or anything else.
    pub fn audio() -> Audio {
        const SYSTEM: u32 = 1;
        let Some(dev) = prop::<u32>(SYSTEM, four(b"dOut")).filter(|&d| d != 0) else { return Audio::Elsewhere };
        let transport = prop::<u32>(dev, four(b"tran")).unwrap_or(0);
        if transport != four(b"hdmi") && transport != four(b"dprt") {
            return Audio::Elsewhere;
        }
        let name = prop::<usize>(dev, four(b"lnam")).map(|p| Cf(p as CfRef)).and_then(|cf| string_of(cf.0)).unwrap_or_default();
        Audio::Monitor(name)
    }

    // ------------------------------------------------------------ the keys

    type TapCallback = unsafe extern "C" fn(proxy: CfRef, kind: u32, event: CfRef, info: *mut c_void) -> CfRef;

    struct Tap {
        handler: Handler,
        port: std::sync::atomic::AtomicPtr<c_void>,
        /// Keys whose press we took: their release is ours too.
        taken: std::sync::Mutex<Vec<Key>>,
    }

    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    unsafe extern "C" fn on_event(_proxy: CfRef, kind: u32, event: CfRef, info: *mut c_void) -> CfRef {
        let tap = unsafe { &*(info as *const Tap) };
        if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER {
            let port = tap.port.load(std::sync::atomic::Ordering::Relaxed);
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
            return event;
        }
        if kind != NX_SYSDEFINED || event.is_null() {
            return event;
        }
        let parsed = objc2::rc::autoreleasepool(|_| {
            let cg = unsafe { &*(event as *const objc2_core_graphics::CGEvent) };
            let ns = objc2_app_kit::NSEvent::eventWithCGEvent(cg)?;
            decode(ns.subtype().0, ns.data1() as i64)
        });
        let Some((key, down, _repeat)) = parsed else { return event };
        let mut taken = match tap.taken.lock() {
            Ok(t) => t,
            Err(p) => p.into_inner(),
        };
        if !down {
            if let Some(i) = taken.iter().position(|k| *k == key) {
                taken.remove(i);
                return std::ptr::null();
            }
            return event;
        }
        let flags = unsafe { CGEventGetFlags(event) };
        let p = unsafe { CGEventGetLocation(event) };
        // never let a panic cross into CoreGraphics
        let take = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (tap.handler)(key, fine(flags), Pos2::new(p.x as f32, p.y as f32)))).unwrap_or(false);
        if take {
            if !taken.contains(&key) {
                taken.push(key);
            }
            return std::ptr::null();
        }
        event
    }

    /// Watch the media keys on a thread of their own. Taking keys needs
    /// Accessibility; until it is granted, try again now and then.
    pub fn start_keys(handler: Handler) {
        let tap: &'static Tap = Box::leak(Box::new(Tap { handler, port: Default::default(), taken: Default::default() }));
        std::thread::Builder::new()
            .name("display-keys".into())
            .spawn(move || loop {
                // never create the tap before Accessibility is granted: the
                // attempt itself makes macOS show its alert (setup asks instead)
                if crate::kit::perms::Perm::Accessibility.status() != crate::kit::perms::Status::Allowed {
                    std::thread::sleep(Duration::from_secs(10));
                    continue;
                }
                let port = unsafe { CGEventTapCreate(1, 0, 0, 1u64 << NX_SYSDEFINED, on_event, tap as *const Tap as *mut c_void) };
                if port.is_null() {
                    std::thread::sleep(Duration::from_secs(10));
                    continue;
                }
                tap.port.store(port as *mut c_void, std::sync::atomic::Ordering::Relaxed);
                unsafe {
                    let src = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                    CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
                    CGEventTapEnable(port, true);
                    CFRunLoopRun();
                }
                return;
            })
            .ok();
    }

    /// Keep our window `title` above everything, on every Space, and out of
    /// the mouse's way.
    pub fn float(title: &str) {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSWindowCollectionBehavior};
        let Some(mtm) = MainThreadMarker::new() else { return };
        for w in NSApplication::sharedApplication(mtm).windows().iter() {
            if w.title().to_string() != title {
                continue;
            }
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
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::super::ddc::Reading;
    use super::*;

    #[derive(Default)]
    pub struct MacBus;

    impl Bus for MacBus {
        fn list(&mut self) -> Vec<Found> {
            Vec::new()
        }
        fn ddc_read(&mut self, _id: u32, _code: u8) -> Option<Reading> {
            None
        }
        fn ddc_write(&mut self, _id: u32, _code: u8, _value: u16) -> bool {
            false
        }
        fn native_get(&mut self, _id: u32) -> Option<f32> {
            None
        }
        fn dim(&mut self, _id: u32, _scale: f32) -> bool {
            false
        }
    }

    pub fn audio() -> Audio {
        Audio::Elsewhere
    }
    pub fn start_keys(_handler: Handler) {}
    pub fn float(_title: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data1(code: u32, state: u32, repeat: bool) -> i64 {
        ((code << 16) | (state << 8) | repeat as u32) as i64
    }

    #[test]
    fn decodes_media_keys() {
        assert_eq!(decode(8, data1(2, 0x0A, false)), Some((Key::BrightnessUp, true, false)));
        assert_eq!(decode(8, data1(3, 0x0B, false)), Some((Key::BrightnessDown, false, false)));
        assert_eq!(decode(8, data1(0, 0x0A, true)), Some((Key::VolumeUp, true, true)));
        assert_eq!(decode(8, data1(1, 0x0A, false)).map(|k| k.0), Some(Key::VolumeDown));
        assert_eq!(decode(8, data1(7, 0x0A, false)).map(|k| k.0), Some(Key::Mute));
        // play/pause and keyboard backlight are not ours
        assert_eq!(decode(8, data1(16, 0x0A, false)), None);
        assert_eq!(decode(8, data1(21, 0x0A, false)), None);
        // other subtypes and states
        assert_eq!(decode(7, data1(2, 0x0A, false)), None);
        assert_eq!(decode(8, data1(2, 0x00, false)), None);
        // the sign bit of a 64-bit data1 doesn't matter
        assert_eq!(decode(8, data1(2, 0x0A, false) | (-1i64 << 32)), Some((Key::BrightnessUp, true, false)));
    }

    #[test]
    fn fine_steps_need_option_and_shift() {
        assert!(fine(0x0002_0000 | 0x0008_0000));
        assert!(!fine(0x0008_0000));
        assert!(!fine(0x0002_0000));
        assert!(!fine(0));
    }
}
