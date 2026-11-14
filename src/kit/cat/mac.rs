//! The cat on a Mac: its face in the menu bar icon, the big cat in the
//! middle of the screen, its voice, and what the Mac feels (a hand on the
//! trackpad, the charger, the lid, the pointer).
//!
//! The cat's own thread decides and draws; everything that touches AppKit
//! runs on the main thread. The thread sleeps until something moves, so a
//! resting cat costs nothing.

use super::engine::{Cmd, Engine, Event};
use super::{face, Kind, Prefs};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, MainThreadMarker};
use objc2_app_kit::{NSBackingStoreType, NSColor, NSEvent, NSEventMask, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The menu bar icon: 18 pt at 2x.
const ICON_PX: u32 = 36;
const ICON_PT: f64 = 18.0;
/// Frames a second while only the icon moves, and while the big cat shows.
const ICON_FPS: f64 = 30.0;
const BIG_FPS: f64 = 60.0;
/// The big cat fades in and out over this long.
const FADE: f64 = 0.15;
/// The purr fades out in this many steps of this long.
const HUSH_STEPS: u8 = 6;
const HUSH_STEP: Duration = Duration::from_millis(60);
/// The trackpad taps this often while it purrs (a cat purrs at about 25 Hz).
const BUZZ_EVERY: Duration = Duration::from_millis(40);
/// This many fingers at once are a hand.
const HAND_FINGERS: i32 = 4;
/// Signs of a hand go to the cat this far apart, in milliseconds.
const HAND_SIGN_MS: u64 = 200;
/// A hand on the trackpad this soon after a key press is a typing hand.
const TYPING_SECS: f64 = 1.5;
/// The pointer is told to the cat this often at most, in milliseconds.
const POINTER_MS: u64 = 33;
/// The eyes turn fully when the pointer is this share of the screen away…
const LOOK_REACH: f64 = 0.4;
/// …by this much of the 1024 box, sideways and down.
const LOOK_MAX: (f32, f32) = (150.0, 70.0);
/// The cat watches the pointer over this share of the screen, from the top.
const NEAR_SHARE: f64 = 0.6;

static TX: Mutex<Option<Sender<Event>>> = Mutex::new(None);
/// Pixels a point on the screen the big cat shows on, as `f32` bits.
static BIG_SCALE: AtomicU32 = AtomicU32::new(0x4000_0000);
static LOG: OnceLock<bool> = OnceLock::new();

/// `CMDMEOW_CAT_LOG=1` prints what the cat feels and does.
fn log() -> bool {
    *LOG.get_or_init(|| std::env::var_os("CMDMEOW_CAT_LOG").is_some())
}

/// Milliseconds since the cat came to life.
fn uptime_ms() -> u64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn send(ev: Event) {
    // what the Mac feels follows the switches (main thread: Settings)
    if let Event::Prefs(p) = &ev {
        MAIN.with(|m| {
            if let Ok(mut m) = m.try_borrow_mut() {
                if let Some(m) = m.as_mut() {
                    m.feel(p);
                }
            }
        });
    }
    if let Some(tx) = &*TX.lock().unwrap_or_else(|e| e.into_inner()) {
        let _ = tx.send(ev);
    }
}

// ------------------------------------------------------------- main thread

/// Everything that lives on the main thread.
struct Main {
    /// The menu bar icon (`NSStatusItem`).
    item: Retained<AnyObject>,
    big: Option<Big>,
    /// Counts the big cat's comings and goings, so a late "take it off
    /// screen" knows it is out of date.
    big_turn: u32,
    /// The sound that plays (`NSSound`) and whether it is the looping purr.
    voice: Option<(Retained<AnyObject>, bool)>,
    /// The purr that is fading out.
    fading: Option<Retained<AnyObject>>,
    buzz_turn: u32,
    buzzing: bool,
    /// The pointer watchers (event monitors: other apps', and ours), while
    /// the eyes follow.
    pointer: Vec<Retained<AnyObject>>,
    /// The trackpads being listened to.
    pads: Vec<*mut c_void>,
}

struct Big {
    window: Retained<NSWindow>,
    /// The layer that shows the cat's picture.
    layer: Retained<AnyObject>,
}

thread_local! {
    static MAIN: RefCell<Option<Main>> = const { RefCell::new(None) };
}

/// Run `f` on the main thread with the cat's things.
fn on_main(f: impl FnOnce(&mut Main) + Send + 'static) {
    DispatchQueue::main().exec_async(move || with_main(f));
}

fn with_main(f: impl FnOnce(&mut Main)) {
    MAIN.with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            if let Some(m) = m.as_mut() {
                f(m);
            }
        }
    });
}

fn later(after: Duration, f: impl FnOnce(&mut Main) + Send + 'static) {
    let when = DispatchTime::try_from(after).unwrap_or(DispatchTime::NOW);
    let _ = DispatchQueue::main().after(when, move || with_main(f));
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGDataProviderCreateWithCFData(data: *const c_void) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    #[allow(clippy::too_many_arguments)]
    fn CGImageCreate(w: usize, h: usize, bits: usize, pixel_bits: usize, row_bytes: usize, space: *mut c_void, info: u32, provider: *mut c_void, decode: *const f64, interpolate: bool, intent: i32) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
    static kCGColorSpaceSRGB: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, i: isize) -> *const c_void;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *const c_void, source: *const c_void, mode: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

/// A `CGImage` (owned) from premultiplied RGBA pixels, `px` square.
fn cg_image(rgba: &[u8], px: u32, srgb: bool) -> Option<*mut c_void> {
    if rgba.len() != (px * px * 4) as usize {
        return None;
    }
    unsafe {
        let data = CFDataCreate(std::ptr::null(), rgba.as_ptr(), rgba.len() as isize);
        if data.is_null() {
            return None;
        }
        let provider = CGDataProviderCreateWithCFData(data);
        CFRelease(data);
        let space = if srgb { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) } else { CGColorSpaceCreateDeviceRGB() };
        // 1: alpha last, premultiplied
        let image = CGImageCreate(px as usize, px as usize, 8, 32, px as usize * 4, space, 1, provider, std::ptr::null(), false, 0);
        CGColorSpaceRelease(space);
        CGDataProviderRelease(provider);
        (!image.is_null()).then_some(image)
    }
}

/// One drawn moment: the icon, and the big cat when it shows.
struct Frame {
    icon: Option<Vec<u8>>,
    big: Option<(Vec<u8>, u32)>,
}

impl Main {
    fn show(&mut self, f: Frame) {
        if let Some(px) = f.icon {
            self.set_icon(&px);
        }
        if let (Some((px, n)), Some(big)) = (f.big, &self.big) {
            if let Some(image) = cg_image(&px, n, true) {
                unsafe {
                    let _: () = msg_send![class!(CATransaction), begin];
                    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                    let _: () = msg_send![&*big.layer, setContents: image as *mut AnyObject];
                    let _: () = msg_send![class!(CATransaction), commit];
                    CGImageRelease(image);
                }
            }
        }
    }

    /// The menu bar icon's picture: a template, so macOS tints it.
    fn set_icon(&mut self, rgba: &[u8]) {
        let Some(image) = cg_image(rgba, ICON_PX, false) else { return };
        unsafe {
            let alloc: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let ns: Option<Retained<AnyObject>> = msg_send![alloc, initWithCGImage: image as *const c_void, size: NSSize::new(ICON_PT, ICON_PT)];
            CGImageRelease(image);
            let Some(ns) = ns else { return };
            let _: () = msg_send![&*ns, setTemplate: true];
            let button: Option<Retained<AnyObject>> = msg_send![&*self.item, button];
            if let Some(button) = button {
                let _: () = msg_send![&*button, setImage: &*ns];
            }
        }
    }

    /// The big cat comes (in the middle of the screen in use) or goes.
    fn set_big(&mut self, on: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        self.big_turn = self.big_turn.wrapping_add(1);
        if on {
            if self.big.is_none() {
                self.big = big_window(mtm);
            }
            let Some(big) = &self.big else { return };
            if let Some(screen) = NSScreen::mainScreen(mtm) {
                let f = screen.frame();
                let side = face::BEZEL_PT as f64;
                big.window.setFrameOrigin(NSPoint::new(f.origin.x + (f.size.width - side) / 2.0, f.origin.y + (f.size.height - side) / 2.0));
                BIG_SCALE.store((screen.backingScaleFactor() as f32).to_bits(), Ordering::Relaxed);
            }
            big.window.orderFrontRegardless();
            fade(&big.window, 1.0);
        } else if let Some(big) = &self.big {
            fade(&big.window, 0.0);
            let turn = self.big_turn;
            later(Duration::from_secs_f64(FADE + 0.05), move |m| {
                if m.big_turn == turn {
                    if let Some(big) = &m.big {
                        big.window.orderOut(None);
                    }
                }
            });
        }
    }

    /// Play `kind`'s sound `n`, unless the Mac should be quiet now.
    fn play(&mut self, kind: Kind, n: u8) {
        self.stop_voice();
        if !crate::kit::sound::on() || crate::kit::activity::recording() || mic_in_use() {
            return;
        }
        let Some(path) = sound_file(kind, n) else {
            if log() {
                eprintln!("[cat] no sound file for {} {n}", kind.slug());
            }
            return;
        };
        let looped = kind == Kind::Purr;
        unsafe {
            let alloc: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSSound), alloc];
            let path = NSString::from_str(&path.to_string_lossy());
            let sound: Option<Retained<AnyObject>> = msg_send![alloc, initWithContentsOfFile: &*path, byReference: true];
            let Some(sound) = sound else { return };
            let _: () = msg_send![&*sound, setLoops: looped];
            let _: bool = msg_send![&*sound, play];
            self.voice = Some((sound, looped));
        }
    }

    fn stop_voice(&mut self) {
        if let Some((sound, _)) = self.voice.take() {
            let _: bool = unsafe { msg_send![&*sound, stop] };
        }
    }

    /// The purr fades out.
    fn hush(&mut self) {
        if let Some(old) = self.fading.take() {
            let _: bool = unsafe { msg_send![&*old, stop] };
        }
        match self.voice.take() {
            Some((sound, true)) => {
                self.fading = Some(sound);
                hush_step(HUSH_STEPS);
            }
            other => self.voice = other,
        }
    }

    /// The trackpad purrs under the hand: a light tap, again and again.
    fn buzz(&mut self, on: bool) {
        self.buzz_turn = self.buzz_turn.wrapping_add(1);
        self.buzzing = on;
        if on {
            buzz_step(self.buzz_turn);
        }
    }

    /// Listen to what the switches ask for, and to nothing else.
    fn feel(&mut self, p: &Prefs) {
        if p.watch == self.pointer.is_empty() {
            if self.pointer.is_empty() {
                self.pointer = watch_pointer();
                self.tell_pointer();
            } else {
                for monitor in std::mem::take(&mut self.pointer) {
                    unsafe { NSEvent::removeMonitor(&monitor) };
                }
            }
        }
        if p.purr != !self.pads.is_empty() {
            if p.purr {
                self.pads = pads_start();
            } else {
                pads_stop(std::mem::take(&mut self.pads));
            }
        }
    }

    /// Tell the cat where the pointer is from the menu bar icon.
    fn tell_pointer(&self) {
        let p = NSEvent::mouseLocation();
        let (icon, screen): (NSRect, NSRect) = unsafe {
            let button: Option<Retained<AnyObject>> = msg_send![&*self.item, button];
            let Some(button) = button else { return };
            let window: Option<Retained<AnyObject>> = msg_send![&*button, window];
            let Some(window) = window else { return };
            let screen: Option<Retained<AnyObject>> = msg_send![&*window, screen];
            let Some(screen) = screen else { return };
            (msg_send![&*window, frame], msg_send![&*screen, frame])
        };
        let (w, h) = (screen.size.width.max(1.0), screen.size.height.max(1.0));
        let top = screen.origin.y + h;
        let on_screen = p.x >= screen.origin.x && p.x <= screen.origin.x + w && p.y >= screen.origin.y && p.y <= top;
        let dx = (p.x - (icon.origin.x + icon.size.width / 2.0)) / w;
        let dy = ((icon.origin.y + icon.size.height / 2.0) - p.y) / h;
        let look = ((dx / LOOK_REACH).clamp(-1.0, 1.0) as f32 * LOOK_MAX.0, (dy / LOOK_REACH).clamp(0.0, 1.0) as f32 * LOOK_MAX.1);
        send(Event::Pointer { look, near: on_screen && top - p.y <= h * NEAR_SHARE });
    }
}

/// The big cat's window: clear, above everything, on every Space, and
/// clicks go through it.
fn big_window(mtm: MainThreadMarker) -> Option<Big> {
    let side = face::BEZEL_PT as f64;
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side, side));
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), rect, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setIgnoresMouseEvents(true);
    window.setLevel(25);
    window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle);
    window.setAlphaValue(0.0);
    let view = window.contentView()?;
    view.setWantsLayer(true);
    let root = view.layer()?;
    let layer: Retained<AnyObject> = unsafe {
        let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
        let _: () = msg_send![&*layer, setFrame: rect];
        let _: () = msg_send![&*root, addSublayer: &*layer];
        layer
    };
    Some(Big { window, layer })
}

/// Fade a window to `alpha`.
fn fade(window: &NSWindow, alpha: f64) {
    unsafe {
        let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
        let context: Retained<AnyObject> = msg_send![class!(NSAnimationContext), currentContext];
        let _: () = msg_send![&*context, setDuration: FADE];
        let proxy: Retained<AnyObject> = msg_send![window, animator];
        let _: () = msg_send![&*proxy, setAlphaValue: alpha];
        let _: () = msg_send![class!(NSAnimationContext), endGrouping];
    }
}

fn hush_step(left: u8) {
    later(HUSH_STEP, move |m| {
        let Some(sound) = &m.fading else { return };
        if left <= 1 {
            let _: bool = unsafe { msg_send![&**sound, stop] };
            m.fading = None;
        } else {
            let volume = (left - 1) as f32 / HUSH_STEPS as f32;
            let _: () = unsafe { msg_send![&**sound, setVolume: volume] };
            hush_step(left - 1);
        }
    });
}

fn buzz_step(turn: u32) {
    later(BUZZ_EVERY, move |m| {
        if !m.buzzing || m.buzz_turn != turn {
            return;
        }
        unsafe {
            let performer: Retained<AnyObject> = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
            // pattern 0: generic; time 1: now
            let _: () = msg_send![&*performer, performFeedbackPattern: 0isize, performanceTime: 1usize];
        }
        buzz_step(turn);
    });
}

/// `purr-2.mp3` and the like: in the app (`Resources/cat`), or in the
/// app's data folder (`cat`).
fn sound_file(kind: Kind, n: u8) -> Option<PathBuf> {
    let (stem, _) = kind.sounds();
    let name = format!("{stem}-{n}.mp3");
    let in_app = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/cat")));
    in_app.into_iter().chain([crate::kit::fs::data_path("cat")]).map(|dir| dir.join(&name)).find(|p| p.is_file())
}

// ------------------------------------------------------------ the mic

#[repr(C)]
struct AudioAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(object: u32, address: *const AudioAddress, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
}

const fn four(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// Whether any app is listening to the microphone (a call, a recording,
/// dictation): the cat keeps quiet then.
fn mic_in_use() -> bool {
    let read = |object: u32, selector: &[u8; 4]| -> Option<u32> {
        let address = AudioAddress { selector: four(selector), scope: four(b"glob"), element: 0 };
        let (mut value, mut size) = (0u32, 4u32);
        let err = unsafe { AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut u32).cast()) };
        (err == 0).then_some(value)
    };
    // 1: the system object; 'dIn ': its default input device; 'gone': running somewhere
    read(1, b"dIn ").filter(|device| *device != 0).and_then(|device| read(device, b"gone")).is_some_and(|running| running != 0)
}

// ------------------------------------------------------- power, wake, lock

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSNotificationCreateRunLoopSource(callback: extern "C" fn(*mut c_void), context: *mut c_void) -> *mut c_void;
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSCopyPowerSourcesList(blob: *const c_void) -> *const c_void;
    fn IOPSGetPowerSourceDescription(blob: *const c_void, source: *const c_void) -> *const c_void;
}

/// The built-in battery's charge in percent and whether the charger is in.
fn power() -> Option<(u8, bool)> {
    unsafe {
        let blob = IOPSCopyPowerSourcesInfo();
        if blob.is_null() {
            return None;
        }
        let list = IOPSCopyPowerSourcesList(blob);
        let mut found = None;
        if !list.is_null() {
            for i in 0..CFArrayGetCount(list) {
                // a dictionary, read as the NSDictionary it also is
                let d = IOPSGetPowerSourceDescription(blob, CFArrayGetValueAtIndex(list, i)) as *const AnyObject;
                let Some(d) = d.as_ref() else { continue };
                let text = |key: &str| -> Option<String> {
                    let v: Option<Retained<AnyObject>> = msg_send![d, objectForKey: &*NSString::from_str(key)];
                    let v = v?;
                    let is_text: bool = msg_send![&*v, isKindOfClass: class!(NSString)];
                    is_text.then(|| Retained::cast_unchecked::<NSString>(v).to_string())
                };
                let number = |key: &str| -> Option<i32> {
                    let v: Option<Retained<AnyObject>> = msg_send![d, objectForKey: &*NSString::from_str(key)];
                    let v = v?;
                    let is_number: bool = msg_send![&*v, isKindOfClass: class!(NSNumber)];
                    is_number.then(|| msg_send![&*v, intValue])
                };
                if text("Type").as_deref() != Some("InternalBattery") {
                    continue;
                }
                if let Some(now) = number("Current Capacity") {
                    let max = number("Max Capacity").unwrap_or(100).max(1);
                    let plugged = text("Power Source State").as_deref() == Some("AC Power");
                    found = Some(((now * 100 / max).clamp(0, 100) as u8, plugged));
                }
                break;
            }
            CFRelease(list);
        }
        CFRelease(blob);
        found
    }
}

extern "C" fn power_changed(_context: *mut c_void) {
    if let Some((pct, plugged)) = power() {
        if log() {
            eprintln!("[cat] power {pct}% plugged {plugged}");
        }
        send(Event::Power { pct, plugged });
    }
}

/// Call `f` on the main thread when a notification called `name` comes,
/// from other apps (`distributed`) or from the workspace.
fn observe(name: &str, distributed: bool, f: impl Fn() + 'static) {
    let block = block2::RcBlock::new(move |_note: std::ptr::NonNull<AnyObject>| f());
    let name = NSString::from_str(name);
    unsafe {
        let center: Retained<AnyObject> = if distributed {
            msg_send![class!(NSDistributedNotificationCenter), defaultCenter]
        } else {
            let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            msg_send![&*workspace, notificationCenter]
        };
        let queue: Retained<AnyObject> = msg_send![class!(NSOperationQueue), mainQueue];
        let token: Option<Retained<AnyObject>> = msg_send![&*center, addObserverForName: &*name, object: Option::<&AnyObject>::None, queue: &*queue, usingBlock: &*block];
        // observed for the app's whole life
        std::mem::forget(token);
    }
}

// ------------------------------------------------------------- the pointer

/// Tell the cat when the pointer moves (listen-only monitors: no
/// permission needed). The global one hears other apps; while this app is
/// in front the moves come to it instead, so a local one hears those.
fn watch_pointer() -> Vec<Retained<AnyObject>> {
    static LAST: AtomicU64 = AtomicU64::new(0);
    fn moved() {
        let now = uptime_ms();
        if now.saturating_sub(LAST.load(Ordering::Relaxed)) < POINTER_MS {
            return;
        }
        LAST.store(now, Ordering::Relaxed);
        with_main(|m| m.tell_pointer());
    }
    let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDragged;
    let global = block2::RcBlock::new(|_event: std::ptr::NonNull<NSEvent>| moved());
    let local = block2::RcBlock::new(|event: std::ptr::NonNull<NSEvent>| {
        moved();
        event.as_ptr()
    });
    let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global);
    // the event goes on to the app unchanged
    let local = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) };
    global.into_iter().chain(local).collect()
}

// ------------------------------------------------------------ the trackpad

/// The trackpad's own library (private): it says how many fingers lie on
/// it, which no public call does.
struct Pads {
    create_list: unsafe extern "C" fn() -> *const c_void,
    register: unsafe extern "C" fn(*mut c_void, PadCallback),
    unregister: unsafe extern "C" fn(*mut c_void, PadCallback),
    start: unsafe extern "C" fn(*mut c_void, i32),
    stop: unsafe extern "C" fn(*mut c_void),
}

type PadCallback = extern "C" fn(device: *mut c_void, touches: *const c_void, count: i32, time: f64, frame: i32) -> i32;

fn pads() -> Option<&'static Pads> {
    static PADS: OnceLock<Option<Pads>> = OnceLock::new();
    extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    PADS.get_or_init(|| unsafe {
        let lib = dlopen(c"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport".as_ptr(), 1);
        if lib.is_null() {
            return None;
        }
        let sym = |name: &CStr| {
            let p = dlsym(lib, name.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(Pads {
            create_list: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> *const c_void>(sym(c"MTDeviceCreateList")?),
            register: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void, PadCallback)>(sym(c"MTRegisterContactFrameCallback")?),
            unregister: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void, PadCallback)>(sym(c"MTUnregisterContactFrameCallback")?),
            start: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void, i32)>(sym(c"MTDeviceStart")?),
            stop: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(sym(c"MTDeviceStop")?),
        })
    })
    .as_ref()
}

/// Every frame of touches, on the trackpad's own thread: a hand is
/// several fingers at once, and not right after typing (palms rest on the
/// trackpad then).
extern "C" fn pad_touched(_device: *mut c_void, _touches: *const c_void, count: i32, _time: f64, _frame: i32) -> i32 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    static LOGGED: AtomicU64 = AtomicU64::new(0);
    let now = uptime_ms();
    if log() && count > 0 && now.saturating_sub(LOGGED.swap(now, Ordering::Relaxed)) >= 500 {
        eprintln!("[cat] trackpad: {count} fingers");
    }
    if count < HAND_FINGERS {
        return 0;
    }
    let last = LAST.load(Ordering::Relaxed);
    if now.saturating_sub(last) < HAND_SIGN_MS || LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return 0;
    }
    // 1: the hardware's own event state; 10: key down
    let typed = unsafe { CGEventSourceSecondsSinceLastEventType(1, 10) };
    if typed >= TYPING_SECS {
        send(Event::Hand);
    }
    0
}

/// Start listening to every trackpad; the ones that started.
fn pads_start() -> Vec<*mut c_void> {
    let Some(lib) = pads() else {
        if log() {
            eprintln!("[cat] no trackpad library: no purring under the hand");
        }
        return Vec::new();
    };
    let mut out = Vec::new();
    unsafe {
        let list = (lib.create_list)();
        if list.is_null() {
            return out;
        }
        for i in 0..CFArrayGetCount(list) {
            let device = CFArrayGetValueAtIndex(list, i) as *mut c_void;
            if device.is_null() {
                continue;
            }
            (lib.register)(device, pad_touched);
            (lib.start)(device, 0);
            out.push(device);
        }
        // the list is kept: it owns the devices being listened to
    }
    if log() {
        eprintln!("[cat] listening to {} trackpad(s)", out.len());
    }
    out
}

fn pads_stop(devices: Vec<*mut c_void>) {
    let Some(lib) = pads() else { return };
    for device in devices {
        unsafe {
            (lib.unregister)(device, pad_touched);
            (lib.stop)(device);
        }
    }
}

// ---------------------------------------------------------------- starting

pub fn start(prefs: Prefs, status_item: *mut c_void) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if MainThreadMarker::new().is_none() || status_item.is_null() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(item) = (unsafe { Retained::retain(status_item as *mut AnyObject) }) else { return };
    let (tx, rx) = mpsc::channel();
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let mut main = Main { item, big: None, big_turn: 0, voice: None, fading: None, buzz_turn: 0, buzzing: false, pointer: Vec::new(), pads: Vec::new() };
    main.feel(&prefs);
    MAIN.with(|m| *m.borrow_mut() = Some(main));

    // the charger and the battery: the first reading is the starting point
    unsafe {
        let source = IOPSNotificationCreateRunLoopSource(power_changed, std::ptr::null_mut());
        if !source.is_null() {
            CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        }
    }
    power_changed(std::ptr::null_mut());
    observe("NSWorkspaceDidWakeNotification", false, || send(Event::Wake));
    observe("com.apple.screenIsLocked", true, || send(Event::Locked(true)));
    observe("com.apple.screenIsUnlocked", true, || send(Event::Locked(false)));

    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
    let _ = std::thread::Builder::new().name("cat".into()).spawn(move || run(rx, Engine::new(prefs, seed)));
}

/// The cat's thread: take in what the Mac felt, decide, draw what changed,
/// sleep until something next moves.
fn run(rx: Receiver<Event>, mut cat: Engine) {
    let t0 = Instant::now();
    let clock = || t0.elapsed().as_secs_f64();
    let mut drawn = cat.pose(0.0);
    let mut drawn_big = false;
    let mut last_draw = f64::MIN;
    loop {
        let now = clock();
        let frame = 1.0 / if cat.big() { BIG_FPS } else { ICON_FPS };
        let due = (last_draw + frame - now).max(0.0);
        // a face that differs from the one on screen is drawn at the next frame
        let stale = cat.pose(now) != drawn || cat.big() != drawn_big;
        let wait = match cat.sleep(now) {
            _ if stale => Some(due),
            // moving, but not enough to see yet: look again a frame on
            Some(s) if s <= 0.0 => Some(if due > 0.0 { due } else { frame }),
            other => other,
        };
        let news = match wait {
            None => match rx.recv() {
                Ok(ev) => Some(ev),
                Err(_) => return,
            },
            Some(s) => match rx.recv_timeout(Duration::from_secs_f64(s)) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        let now = clock();
        let mut cmds = Vec::new();
        for ev in news.into_iter().chain(rx.try_iter()) {
            if log() && !matches!(ev, Event::Pointer { .. } | Event::Hand) {
                eprintln!("[cat] {ev:?}");
            }
            cmds.extend(cat.handle(now, ev));
        }
        cmds.extend(cat.tick(now));
        let big_comes = cmds.contains(&Cmd::Big(true));
        for cmd in cmds {
            if log() {
                eprintln!("[cat] → {cmd:?}");
            }
            match cmd {
                Cmd::Sound { kind, n } => on_main(move |m| m.play(kind, n)),
                Cmd::Hush => on_main(|m| m.hush()),
                Cmd::Buzz(on) => on_main(move |m| m.buzz(on)),
                // it comes with its first picture, below
                Cmd::Big(true) => {}
                Cmd::Big(false) => on_main(|m| m.set_big(false)),
                Cmd::Keep(p) => {
                    super::keep(p);
                }
            }
        }
        let (pose, big) = (cat.pose(now), cat.big());
        let changed = pose != drawn || big != drawn_big;
        if changed && (big_comes || now - last_draw >= frame - 0.002) {
            let icon = (pose != drawn || !big).then(|| face::icon(&pose, ICON_PX));
            let picture = big.then(|| {
                let scale = f32::from_bits(BIG_SCALE.load(Ordering::Relaxed)).clamp(1.0, 3.0);
                (face::bezel(&pose, scale), face::bezel_px(scale))
            });
            on_main(move |m| {
                if big_comes {
                    m.set_big(true);
                }
                m.show(Frame { icon, big: picture });
            });
            drawn = pose;
            drawn_big = big;
            last_draw = now;
        }
    }
}
