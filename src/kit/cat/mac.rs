//! The cat on a Mac: its face in the menu bar icon, the desktop cat at the
//! bottom of the screen (`screen`), its voice, and what the Mac feels (a
//! hand on the trackpad, the charger, the lid, the pointer).
//!
//! The cat's own thread decides and draws; everything that touches AppKit
//! runs on the main thread. The thread sleeps until something moves, so a
//! resting cat costs nothing.

use super::engine::{Cmd, Engine, Event};
use super::screen::{self, Desk};
use super::desk::{Do, Rest};
use super::{desk, face, Kind, Prefs, Show};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, MainThreadMarker};
use objc2_app_kit::NSWindow;
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
/// Frames a second while the icon moves.
const ICON_FPS: f64 = 30.0;
/// The desktop cat fades in and out over this long.
const FADE: f64 = 0.15;
/// A cat that lives on screen does a small thing (a blink, a wash) every
/// so often, this many seconds apart at least and at most…
const LIFE_EVERY: (f64, f64) = (25.0, 70.0);
/// …and curls up asleep when nothing happened for this long.
const NAP_AFTER: Duration = Duration::from_secs(240);
/// A pointer that rests on the cat this long is played with; one that has
/// left it this long has gone.
const HOVER_IN: f64 = 0.3;
const HOVER_OUT: f64 = 0.4;
/// The pointer is looked for on the cat this often.
const HOVER_EVERY: f64 = 0.15;
/// The folder on the Desktop the tidy carries files into.
const TIDY_FOLDER: &str = "Tidy";
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
/// The cat looks at the pointer this often while it moves, and while it rests.
const LOOK_MOVING: f64 = 1.0 / 30.0;
const LOOK_RESTING: f64 = 0.1;
/// The pointer counts as moving this long after its last move, in seconds.
const MOVED_LATELY: f64 = 0.5;
/// Where the menu bar icon is, the main thread tells again this often.
const PLACE_EVERY: f64 = 2.0;
/// The eyes turn fully when the pointer is this share of the screen away…
const LOOK_REACH: f64 = 0.4;
/// …by this much of the 1024 box, sideways and down.
const LOOK_MAX: (f32, f32) = (150.0, 70.0);
/// The cat watches the pointer over this share of the screen, from the top.
const NEAR_SHARE: f64 = 0.6;

static TX: Mutex<Option<Sender<Event>>> = Mutex::new(None);
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
    over: Option<Over>,
    /// The desktop cat (made the first time it shows).
    desk: Option<Desk>,
    /// It lives on screen, in the corner.
    live: bool,
    /// What it is doing (None: resting, or not on screen), and how it
    /// rests once that is done.
    doing: Option<Do>,
    rest_next: Rest,
    /// Counts the acts asked for: one whose sheets come late is dropped.
    asked: u32,
    /// Since when nothing happened to the cat (it naps after a while).
    quiet: Instant,
    /// A tidy is on its way (asking Finder, or carrying files).
    tidying: bool,
    /// Where Finder showed each file of the tidy, so Put back can lay it there again.
    spots: Vec<(PathBuf, f64, f64)>,
    seed: u32,
    /// The sound that plays (`NSSound`) and whether it is the looping purr.
    voice: Option<(Retained<AnyObject>, bool)>,
    /// The purr that is fading out.
    fading: Option<Retained<AnyObject>>,
    buzz_turn: u32,
    buzzing: bool,
    /// The trackpads being listened to.
    pads: Vec<*mut c_void>,
    /// Whether the body is felt for slaps.
    slaps: bool,
}

/// The moving face, drawn over the menu bar button. Each new button image
/// costs about 4 ms on the main thread (AppKit redraws and masks it, and the
/// menu bar is told), so while the cat moves the button holds a blank image
/// and only this layer's picture changes; at rest the button shows the mark
/// itself again.
struct Over {
    layer: Retained<AnyObject>,
    /// The button's own image (the mark, a template), and an empty one the
    /// same size that keeps its place.
    mark: Retained<AnyObject>,
    blank: Retained<AnyObject>,
    /// The colour macOS draws the template in (sRGB, 0–1, not premultiplied)
    /// and the appearance it was read under.
    ink: [f32; 4],
    look: String,
    /// The menu bar's own colour space (owned): pictures in it go to the
    /// screen as they are, not converted every frame.
    space: *mut c_void,
    on: bool,
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
    fn CGColorSpaceRetain(space: *mut c_void) -> *mut c_void;
    fn CGDataProviderCreateWithCFData(data: *const c_void) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    #[allow(clippy::too_many_arguments)]
    fn CGImageCreate(w: usize, h: usize, bits: usize, pixel_bits: usize, row_bytes: usize, space: *mut c_void, info: u32, provider: *mut c_void, decode: *const f64, interpolate: bool, intent: i32) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
    fn CGEventCreate(source: *const c_void) -> *const c_void;
    fn CGEventGetLocation(event: *const c_void) -> NSPoint;
    static kCGColorSpaceSRGB: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, i: isize) -> *const c_void;
    fn CFBooleanGetValue(boolean: *const c_void) -> bool;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *const c_void, source: *const c_void, mode: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

/// A `CGImage` (owned) from premultiplied RGBA pixels, `px` square.
fn cg_image(rgba: &[u8], px: u32, srgb: bool) -> Option<*mut c_void> {
    unsafe {
        let space = if srgb { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) } else { CGColorSpaceCreateDeviceRGB() };
        let image = cg_image_in(rgba, px, space);
        CGColorSpaceRelease(space);
        image
    }
}

/// The same in the colour space `space` (not taken over).
fn cg_image_in(rgba: &[u8], px: u32, space: *mut c_void) -> Option<*mut c_void> {
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
        // 1: alpha last, premultiplied
        let image = CGImageCreate(px as usize, px as usize, 8, 32, px as usize * 4, space, 1, provider, std::ptr::null(), false, 0);
        CGDataProviderRelease(provider);
        (!image.is_null()).then_some(image)
    }
}

/// One drawn moment of the icon (and whether it is the mark at rest).
struct Frame {
    icon: Option<(Vec<u8>, bool)>,
}

impl Main {
    fn show(&mut self, f: Frame) {
        if let Some((px, rest)) = f.icon {
            self.set_icon(&px, rest);
        }
    }

    /// The menu bar icon's picture (black, alpha = coverage): over the
    /// button while the cat moves, the button's own mark at rest.
    fn set_icon(&mut self, rgba: &[u8], rest: bool) {
        let button: Option<Retained<AnyObject>> = unsafe { msg_send![&*self.item, button] };
        let Some(button) = button else { return };
        if self.over.is_none() && !rest {
            self.over = Over::new(&button);
        }
        let Some(o) = self.over.as_mut() else {
            set_template(&button, rgba);
            return;
        };
        if rest {
            o.hide(&button);
            return;
        }
        // the menu bar turned light or dark: read the new colour off the mark
        let look = appearance(&button);
        if o.on && look != o.look {
            o.hide(&button);
        }
        let start = !o.on;
        if start && look != o.look {
            if let Some(ink) = read_ink(&button) {
                o.ink = ink;
            }
            if log() {
                eprintln!("[cat] icon ink {:?} under {look}", o.ink);
            }
            o.look = look;
        }
        if start {
            o.take_space(&button);
        }
        // the ink is a grey, the same numbers in any RGB space
        let Some(image) = cg_image_in(&tint(rgba, o.ink), ICON_PX, o.space) else { return };
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            if start {
                // where the button draws its image (the title beside it may have changed)
                let window: Option<Retained<AnyObject>> = msg_send![&*button, window];
                let scale: f64 = window.map_or(2.0, |w| msg_send![&*w, backingScaleFactor]);
                let _: () = msg_send![&*o.layer, setFrame: image_rect(&button)];
                let _: () = msg_send![&*o.layer, setContentsScale: scale];
                let _: () = msg_send![&*o.layer, setHidden: false];
            }
            let _: () = msg_send![&*o.layer, setContents: image as *mut AnyObject];
            let _: () = msg_send![class!(CATransaction), commit];
            CGImageRelease(image);
            if start {
                let _: () = msg_send![&*button, setImage: &*o.blank];
                o.on = true;
            }
        }
    }

    /// A reaction began: the desktop cat acts it out.
    fn react(&mut self, kind: Kind) {
        self.quiet = Instant::now();
        self.tidying = false;
        self.act(Do::React(kind));
    }

    /// Start `what`: its sheets are decoded off the main thread, then it plays.
    fn act(&mut self, what: Do) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(area) = screen::area(mtm) else { return };
        if self.desk.is_none() {
            self.desk = Desk::new(mtm);
        }
        let Some(d) = &self.desk else { return };
        let script = desk::script(what, area.size.width, d.rest, self.live);
        self.load(Some(what), script, Vec::new());
    }

    fn load(&mut self, what: Option<Do>, script: desk::Script, files: Vec<(screen::File, PathBuf)>) {
        self.asked = self.asked.wrapping_add(1);
        let ask = self.asked;
        self.doing = what;
        let _ = std::thread::Builder::new().name("cat sheets".into()).spawn(move || {
            let frames = screen::load(&script);
            if frames.is_none() && log() {
                eprintln!("[cat] sprites missing for {what:?}");
            }
            on_main(move |m| {
                if let Some(frames) = frames {
                    m.start(ask, what, script, frames, files);
                } else if m.asked == ask {
                    m.doing = None;
                    m.tidying = false;
                }
            });
        });
    }

    /// The sheets are in: play, unless something newer was asked for meanwhile.
    fn start(&mut self, ask: u32, what: Option<Do>, script: desk::Script, frames: screen::Frames, files: Vec<(screen::File, PathBuf)>) {
        if ask != self.asked {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(area) = screen::area(mtm) else { return };
        let Some(d) = &mut self.desk else { return };
        let (files, paths): (Vec<_>, Vec<_>) = files.into_iter().unzip();
        let ends = d.play(area, &script, frames, files);
        if d.window().alphaValue() < 1.0 {
            fade(d.window(), 1.0);
        }
        self.rest_next = script.rest;
        self.doing = what;
        let turn = d.turn;
        // each file goes into the box folder as it starts to fall
        for f in &script.fetch {
            let Some(path) = paths.get(f.file).cloned() else { continue };
            later(Duration::from_secs_f64(f.fall as f64 / desk::FPS), move |m| {
                if m.desk.as_ref().is_some_and(|d| d.turn == turn) {
                    let spot = m.spots.iter().find(|(p, ..)| *p == path).map(|(_, x, y)| (*x, *y));
                    put_away(&path, spot);
                }
            });
        }
        if let Some(secs) = ends {
            self.over_in(secs);
        }
    }

    /// The act is over in `secs`, unless another one starts first.
    fn over_in(&mut self, secs: f64) {
        let Some(turn) = self.desk.as_ref().map(|d| d.turn) else { return };
        later(Duration::from_secs_f64(secs), move |m| m.over(turn));
    }

    /// The act is over: back to rest in the corner, or off the screen.
    fn over(&mut self, turn: u32) {
        let Some(d) = &mut self.desk else { return };
        if d.turn != turn {
            return;
        }
        self.doing = None;
        self.tidying = false;
        let rest = if self.live { self.rest_next } else { Rest::Away };
        if rest != Rest::Away {
            let Some(area) = MainThreadMarker::new().and_then(screen::area) else { return };
            d.settle(area, rest, None);
            set_hot(d.hot());
            self.life_later();
            return;
        }
        set_hot(None);
        fade(d.window(), 0.0);
        let turn = d.turn;
        later(Duration::from_secs_f64(FADE + 0.05), move |m| {
            if let Some(d) = &mut m.desk {
                if d.turn == turn {
                    d.clear();
                }
            }
        });
    }

    /// The reaction is over: a held act (the purr) plays its way out.
    fn desk_end(&mut self) {
        if let Some(secs) = self.desk.as_mut().filter(|d| d.holding()).and_then(|d| d.end()) {
            self.over_in(secs);
        }
    }

    /// The cat moves in to its corner, or out of the screen.
    fn set_live(&mut self, on: bool) {
        if on == self.live {
            return;
        }
        self.live = on;
        if on {
            self.quiet = Instant::now();
            if self.doing.is_none() {
                self.act(Do::Blink);
            }
        } else if self.doing.is_none() {
            set_hot(None);
            if let Some(d) = &mut self.desk {
                d.clear();
            }
        }
    }

    /// A pointer came to rest on the cat (or left it).
    fn hover(&mut self, on: bool) {
        if on {
            if self.live && self.doing.is_none() {
                self.quiet = Instant::now();
                self.act(Do::Play);
            }
        } else if self.doing == Some(Do::Play) {
            self.desk_end();
        }
    }

    /// Something small, a while from now.
    fn life_later(&mut self) {
        let Some(turn) = self.desk.as_ref().map(|d| d.turn) else { return };
        let (lo, hi) = LIFE_EVERY;
        let secs = lo + (hi - lo) * self.random();
        later(Duration::from_secs_f64(secs), move |m| m.life(turn));
    }

    fn life(&mut self, turn: u32) {
        let Some(d) = &self.desk else { return };
        if !self.live || self.doing.is_some() || d.turn != turn || d.rest != Rest::Sit {
            return;
        }
        if self.quiet.elapsed() >= NAP_AFTER {
            self.act(Do::Nap);
        } else if self.random() < 0.35 {
            self.act(Do::Wash);
        } else {
            self.act(Do::Blink);
        }
    }

    /// 0–1, from a small xorshift.
    fn random(&mut self) -> f64 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x as f64 / u32::MAX as f64
    }

    /// The tidy shortcut: ask Finder where the files lie (off the main
    /// thread: the first time macOS asks the person), then fetch them.
    fn tidy(&mut self) {
        if self.tidying {
            return;
        }
        self.tidying = true;
        self.quiet = Instant::now();
        let _ = std::thread::Builder::new().name("cat tidy".into()).spawn(|| {
            let found = desktop_files();
            on_main(move |m| m.tidy_go(found));
        });
    }

    fn tidy_go(&mut self, found: Vec<(PathBuf, f64, f64)>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let (Some(area), Some(top)) = (screen::area(mtm), first_screen_height(mtm)) else {
            self.tidying = false;
            return;
        };
        // Finder counts down from the top of the main screen; the window, up from the bottom of its usable area
        let mut files = Vec::new();
        self.spots = found.clone();
        let _ = std::fs::remove_file(tidy_log());
        for (path, x, y) in found {
            let at = NSPoint::new(x - area.origin.x, top - y - area.origin.y);
            let on_screen = at.x > 0.0 && at.x < area.size.width && at.y > 0.0 && at.y < area.size.height;
            if on_screen {
                files.push((path, at));
            }
        }
        if files.is_empty() {
            if log() {
                eprintln!("[cat] nothing to tidy");
            }
            self.tidying = false;
            return;
        }
        if self.desk.is_none() {
            self.desk = Desk::new(mtm);
        }
        let Some(d) = &self.desk else { return };
        let xs: Vec<f64> = files.iter().map(|(_, at)| at.x).collect();
        let script = desk::tidy(area.size.width, d.rest, &xs);
        let mut carried = Vec::new();
        let mut fetch = Vec::new();
        for f in &script.fetch {
            let (path, at) = &files[f.file];
            let Some(icon) = screen::icon_for(path) else { continue };
            fetch.push(desk::Fetch { file: carried.len(), ..*f });
            carried.push((screen::File { icon, at: *at }, path.clone()));
        }
        let script = desk::Script { fetch, ..script };
        set_hot(None);
        self.load(None, script, carried);
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
        self.set_live(p.awake() && p.show == Show::Always);
        if p.purr != !self.pads.is_empty() {
            if p.purr {
                self.pads = pads_start();
            } else {
                pads_stop(std::mem::take(&mut self.pads));
            }
        }
        if p.hiss != self.slaps {
            self.slaps = p.hiss;
            if p.hiss {
                super::slap::start(|| LID_CLOSED.load(Ordering::SeqCst), || send(Event::Slap), log());
            } else {
                super::slap::stop();
            }
        }
    }

    /// Tell the cat's thread where the menu bar icon and its screen are.
    fn place(&self) {
        let found: Option<(NSRect, NSRect, NSRect)> = unsafe {
            let button: Option<Retained<AnyObject>> = msg_send![&*self.item, button];
            let window: Option<Retained<AnyObject>> = button.and_then(|b| msg_send![&*b, window]);
            let screen: Option<Retained<AnyObject>> = window.as_ref().and_then(|w| msg_send![&**w, screen]);
            let screens: Option<Retained<AnyObject>> = msg_send![class!(NSScreen), screens];
            let first: Option<Retained<AnyObject>> = screens.and_then(|s| msg_send![&*s, firstObject]);
            match (window, screen, first) {
                (Some(w), Some(s), Some(f)) => Some((msg_send![&*w, frame], msg_send![&*s, frame], msg_send![&*f, frame])),
                _ => None,
            }
        };
        // AppKit counts up from the bottom of the first screen; the pointer's
        // own position counts down from its top
        *PLACE.lock().unwrap_or_else(|e| e.into_inner()) = found.map(|(icon, screen, first)| {
            let top = first.size.height;
            Place {
                icon: (icon.origin.x + icon.size.width / 2.0, top - (icon.origin.y + icon.size.height / 2.0)),
                screen: NSRect::new(NSPoint::new(screen.origin.x, top - screen.origin.y - screen.size.height), screen.size),
            }
        });
    }
}

/// The menu bar icon's middle and its screen, counted down from the top of
/// the first screen, as the main thread last saw them.
#[derive(Clone, Copy)]
struct Place {
    icon: (f64, f64),
    screen: NSRect,
}

static PLACE: Mutex<Option<Place>> = Mutex::new(None);

impl Over {
    fn new(button: &AnyObject) -> Option<Over> {
        unsafe {
            let mark: Option<Retained<AnyObject>> = msg_send![button, image];
            let mark = mark?;
            let size: NSSize = msg_send![&*mark, size];
            let alloc: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let blank: Retained<AnyObject> = msg_send![alloc, initWithSize: size];
            let _: () = msg_send![button, setWantsLayer: true];
            let root: Option<Retained<AnyObject>> = msg_send![button, layer];
            let root = root?;
            let layer: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*layer, setHidden: true];
            let _: () = msg_send![&*root, addSublayer: &*layer];
            Some(Over { layer, mark, blank, ink: [1.0, 1.0, 1.0, 1.0], look: String::new(), space: CGColorSpaceCreateDeviceRGB(), on: false })
        }
    }

    /// The colour space of the window the button is in.
    fn take_space(&mut self, button: &AnyObject) {
        unsafe {
            let window: Option<Retained<AnyObject>> = msg_send![button, window];
            let space: Option<Retained<AnyObject>> = window.and_then(|w| msg_send![&*w, colorSpace]);
            let cg: *mut c_void = space.map_or(std::ptr::null_mut(), |s| msg_send![&*s, CGColorSpace]);
            if !cg.is_null() && cg != self.space {
                CGColorSpaceRelease(self.space);
                self.space = CGColorSpaceRetain(cg);
            }
        }
    }

    /// Back to the button's own mark.
    fn hide(&mut self, button: &AnyObject) {
        if !self.on {
            return;
        }
        unsafe {
            let _: () = msg_send![button, setImage: &*self.mark];
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*self.layer, setHidden: true];
            let _: () = msg_send![class!(CATransaction), commit];
        }
        self.on = false;
    }
}

/// A template picture as the button's image (when no layer can be had).
fn set_template(button: &AnyObject, rgba: &[u8]) {
    let Some(image) = cg_image(rgba, ICON_PX, false) else { return };
    unsafe {
        let alloc: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let ns: Option<Retained<AnyObject>> = msg_send![alloc, initWithCGImage: image as *const c_void, size: NSSize::new(ICON_PT, ICON_PT)];
        CGImageRelease(image);
        let Some(ns) = ns else { return };
        let _: () = msg_send![&*ns, setTemplate: true];
        let _: () = msg_send![button, setImage: &*ns];
    }
}

/// Where the button draws its image, in its own coordinates.
fn image_rect(button: &AnyObject) -> NSRect {
    unsafe {
        let bounds: NSRect = msg_send![button, bounds];
        let cell: Option<Retained<AnyObject>> = msg_send![button, cell];
        match cell {
            Some(cell) => msg_send![&*cell, imageRectForBounds: bounds],
            None => bounds,
        }
    }
}

/// The name of the appearance the button draws under.
fn appearance(button: &AnyObject) -> String {
    unsafe {
        let a: Option<Retained<AnyObject>> = msg_send![button, effectiveAppearance];
        let name: Option<Retained<NSString>> = a.and_then(|a| msg_send![&*a, name]);
        name.map(|n| n.to_string()).unwrap_or_default()
    }
}

/// The colour the button draws its template mark in: the most opaque pixel
/// of a picture of the button. White or black by the appearance when there
/// is none.
fn read_ink(button: &AnyObject) -> Option<[f32; 4]> {
    let fallback = if appearance(button).contains("Dark") { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
    unsafe {
        let bounds: NSRect = msg_send![button, bounds];
        let rep: Option<Retained<AnyObject>> = msg_send![button, bitmapImageRepForCachingDisplayInRect: bounds];
        let Some(rep) = rep else { return Some(fallback) };
        let _: () = msg_send![button, cacheDisplayInRect: bounds, toBitmapImageRep: &*rep];
        let (w, h): (isize, isize) = (msg_send![&*rep, pixelsWide], msg_send![&*rep, pixelsHigh]);
        let flipped: bool = msg_send![button, isFlipped];
        let rect = image_rect(button);
        let k = w as f64 / bounds.size.width.max(1.0);
        let top = if flipped { rect.origin.y } else { bounds.size.height - rect.origin.y - rect.size.height };
        let (x0, y0) = ((rect.origin.x * k) as isize, (top * k) as isize);
        let (x1, y1) = (((rect.origin.x + rect.size.width) * k) as isize, ((top + rect.size.height) * k) as isize);
        let srgb: Retained<AnyObject> = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let mut best = [0.0f32; 4];
        for y in y0.max(0)..y1.min(h) {
            for x in x0.max(0)..x1.min(w) {
                let c: Option<Retained<AnyObject>> = msg_send![&*rep, colorAtX: x, y: y];
                let c: Option<Retained<AnyObject>> = c.and_then(|c| msg_send![&*c, colorUsingColorSpace: &*srgb]);
                let Some(c) = c else { continue };
                let a: f64 = msg_send![&*c, alphaComponent];
                if a as f32 > best[3] {
                    let (r, g, b): (f64, f64, f64) = (msg_send![&*c, redComponent], msg_send![&*c, greenComponent], msg_send![&*c, blueComponent]);
                    best = [r as f32, g as f32, b as f32, a as f32];
                }
            }
        }
        Some(if best[3] > 0.5 { best } else { fallback })
    }
}

/// Black coverage (premultiplied) recoloured in `ink`.
fn tint(rgba: &[u8], ink: [f32; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let a = p[3] as f32 * ink[3];
        out.extend([(ink[0] * a) as u8, (ink[1] * a) as u8, (ink[2] * a) as u8, a as u8]);
    }
    out
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
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: *const c_void, alloc: *const c_void, options: u32) -> *const c_void;
    fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
    fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> *const c_void;
    fn IOServiceAddInterestNotification(port: *mut c_void, service: u32, kind: *const c_char, callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void), context: *mut c_void, notification: *mut u32) -> i32;
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

// ------------------------------------------------------------------ the lid

/// The power manager's word that the lid moved
/// (`kIOPMMessageClamshellStateChange`); its argument's first bit says closed.
const LID_MOVED: u32 = 0xE003_4100;
/// The power manager (`IOPMrootDomain`), kept for the app's whole life.
static POWER_ROOT: AtomicU32 = AtomicU32::new(0);
static LID_CLOSED: AtomicBool = AtomicBool::new(false);

/// Whether the lid is closed, as the power manager has it. `None` on a Mac
/// without a lid.
fn lid_closed() -> Option<bool> {
    let root = POWER_ROOT.load(Ordering::SeqCst);
    if root == 0 {
        return None;
    }
    unsafe {
        let key = NSString::from_str("AppleClamshellState");
        let v = IORegistryEntryCreateCFProperty(root, &*key as *const NSString as *const c_void, std::ptr::null(), 0);
        if v.is_null() {
            return None;
        }
        let closed = CFBooleanGetValue(v);
        CFRelease(v);
        Some(closed)
    }
}

/// The lid is `closed` now: opening it is news. A Mac on a charger or with
/// a monitor stays awake under a closed lid, so no wake tells of it.
fn lid_is(closed: bool) {
    let was = LID_CLOSED.swap(closed, Ordering::SeqCst);
    if was != closed {
        if log() {
            eprintln!("[cat] lid {}", if closed { "closed" } else { "open" });
        }
        if !closed {
            send(Event::LidOpen);
        }
    }
}

extern "C" fn lid_moved(_context: *mut c_void, _service: u32, message: u32, argument: *mut c_void) {
    if message == LID_MOVED {
        lid_is(argument as usize & 1 == 1);
    }
}

/// Hear of the lid from the power manager, on the main thread.
fn watch_lid() {
    unsafe {
        let root = IOServiceGetMatchingService(0, IOServiceMatching(c"IOPMrootDomain".as_ptr()));
        if root == 0 {
            return;
        }
        POWER_ROOT.store(root, Ordering::SeqCst);
        let Some(closed) = lid_closed() else { return };
        LID_CLOSED.store(closed, Ordering::SeqCst);
        let port = IONotificationPortCreate(0);
        if port.is_null() {
            return;
        }
        let mut notification = 0u32;
        if IOServiceAddInterestNotification(port, root, c"IOGeneralInterest".as_ptr(), lid_moved, std::ptr::null_mut(), &mut notification) == 0 {
            CFRunLoopAddSource(CFRunLoopGetMain(), IONotificationPortGetRunLoopSource(port), kCFRunLoopCommonModes);
        }
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

/// Where the pointer is, as the eyes in the menu bar see it. Asked for by
/// the cat's own thread: an event monitor would wake the main thread for
/// every move of the pointer (over a hundred a second), and no permission
/// is needed to ask.
fn pointer() -> Option<Event> {
    let place = (*PLACE.lock().unwrap_or_else(|e| e.into_inner()))?;
    let p = unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let p = CGEventGetLocation(event);
        CFRelease(event);
        p
    };
    let s = place.screen;
    let (w, h) = (s.size.width.max(1.0), s.size.height.max(1.0));
    let on_screen = p.x >= s.origin.x && p.x <= s.origin.x + w && p.y >= s.origin.y && p.y <= s.origin.y + h;
    let dx = (p.x - place.icon.0) / w;
    let dy = (p.y - place.icon.1) / h;
    let look = ((dx / LOOK_REACH).clamp(-1.0, 1.0) as f32 * LOOK_MAX.0, (dy / LOOK_REACH).clamp(0.0, 1.0) as f32 * LOOK_MAX.1);
    Some(Event::Pointer { look, near: on_screen && p.y - s.origin.y <= h * NEAR_SHARE })
}

/// Seconds since the pointer last moved.
fn pointer_still() -> f64 {
    // 0: all event sources; 5: mouse moved; 6: dragged
    unsafe { CGEventSourceSecondsSinceLastEventType(0, 5).min(CGEventSourceSecondsSinceLastEventType(0, 6)) }
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

/// The app quits: nothing the cat switched on may stay on.
pub fn quit() {
    super::slap::quit();
}

/// Where a cat that lives on screen rests, for the pointer to find it:
/// points down from the top of the first screen. None: not resting there.
static HOT: Mutex<Option<NSRect>> = Mutex::new(None);

/// Tell the cat's thread where the resting cat is (AppKit points).
fn set_hot(rect: Option<NSRect>) {
    let top = MainThreadMarker::new().and_then(first_screen_height);
    let flipped = rect.zip(top).map(|(r, top)| NSRect::new(NSPoint::new(r.origin.x, top - r.origin.y - r.size.height), r.size));
    *HOT.lock().unwrap_or_else(|e| e.into_inner()) = flipped;
    // the cat's thread starts looking now
    send(Event::Nudge);
}

fn first_screen_height(mtm: MainThreadMarker) -> Option<f64> {
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    screens.firstObject().map(|s| s.frame().size.height)
}

/// The pointer, in points down from the top of the first screen.
fn pointer_at() -> Option<(f64, f64)> {
    unsafe {
        let e = CGEventCreate(std::ptr::null());
        if e.is_null() {
            return None;
        }
        let p = CGEventGetLocation(e);
        CFRelease(e);
        Some((p.x, p.y))
    }
}

/// The tidy shortcut was pressed (main thread).
pub fn tidy() {
    with_main(|m| m.tidy());
}

/// Put back the files of the last tidy (main thread).
pub fn put_back() {
    let _ = std::thread::Builder::new().name("cat put back".into()).spawn(put_back_now);
}

/// Each tidied file, one per line: where it was, where it went, Finder's x and y.
fn tidy_log() -> PathBuf {
    crate::kit::fs::data_path("cat-tidy")
}

fn put_back_now() {
    let Ok(text) = std::fs::read_to_string(tidy_log()) else { return };
    let mut spots = Vec::new();
    for l in text.lines() {
        let mut it = l.split('\t');
        let (Some(from), Some(to)) = (it.next().map(PathBuf::from), it.next().map(PathBuf::from)) else { continue };
        if !to.exists() || from.exists() || std::fs::rename(&to, &from).is_err() {
            continue;
        }
        let x: Option<f64> = it.next().and_then(|v| v.parse().ok());
        let y: Option<f64> = it.next().and_then(|v| v.parse().ok());
        if let (Some(x), Some(y)) = (x, y) {
            spots.push((from, x, y));
        }
    }
    let _ = std::fs::remove_file(tidy_log());
    if spots.is_empty() {
        return;
    }
    // Finder needs a moment to see the files again before it can place them
    std::thread::sleep(Duration::from_millis(600));
    let mut script = String::from("tell application \"Finder\"\n");
    for (path, x, y) in &spots {
        let p = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        script.push_str(&format!("try\nset desktop position of (POSIX file \"{p}\" as alias) to {{{}, {}}}\nend try\n", x.round(), y.round()));
    }
    script.push_str("end tell");
    if let Err(e) = crate::kit::macos::osascript(&script) {
        if log() {
            eprintln!("[cat] Finder could not place the files back: {e}");
        }
    }
}

/// A file is in use when it changed in the last minute or is still downloading.
fn busy(path: &std::path::Path, meta: &std::fs::Metadata) -> bool {
    let fresh = meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(60));
    let partial = path.extension().is_some_and(|e| matches!(e.to_string_lossy().as_ref(), "download" | "crdownload" | "part" | "partial"));
    fresh || partial
}

/// The files lying on the Desktop and where Finder shows them (points
/// down from the top of the main screen). Folders, hidden files and files
/// in use stay where they are.
fn desktop_files() -> Vec<(PathBuf, f64, f64)> {
    let Some(desktop) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Desktop")) else { return Vec::new() };
    let script = r#"tell application "Finder"
set out to ""
repeat with f in (every item of desktop)
try
set p to desktop position of f
set out to out & (POSIX path of (f as alias)) & tab & (item 1 of p) & tab & (item 2 of p) & linefeed
end try
end repeat
return out
end tell"#;
    let out = match crate::kit::macos::osascript(script) {
        Ok(o) => o,
        Err(e) => {
            if log() {
                eprintln!("[cat] Finder said no: {e}");
            }
            return Vec::new();
        }
    };
    out.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let path = PathBuf::from(it.next()?);
            let x: f64 = it.next()?.trim().parse().ok()?;
            let y: f64 = it.next()?.trim().parse().ok()?;
            let plain = path.parent() == Some(desktop.as_path()) && path.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.'));
            let file = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !busy(&path, &m));
            (plain && file).then_some((path, x, y))
        })
        .collect()
}

/// Move `path` into the tidy folder beside it (a name already there gets a
/// number), and note it so Put back can undo it.
fn put_away(path: &std::path::Path, spot: Option<(f64, f64)>) {
    let Some(dir) = path.parent().map(|p| p.join(TIDY_FOLDER)) else { return };
    let Some(name) = path.file_name() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut to = dir.join(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut k = 2;
    while to.exists() && k < 1000 {
        to = dir.join(format!("{stem} {k}{ext}"));
        k += 1;
    }
    if let Err(e) = std::fs::rename(path, &to) {
        if log() {
            eprintln!("[cat] could not tidy {}: {e}", path.display());
        }
        return;
    }
    let (x, y) = spot.map(|(x, y)| (x.to_string(), y.to_string())).unwrap_or_default();
    let line = format!("{}\t{}\t{x}\t{y}\n", path.display(), to.display());
    use std::io::Write;
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(tidy_log()).and_then(|mut f| f.write_all(line.as_bytes()));
}

pub fn start(prefs: Prefs, status_item: *mut c_void) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if MainThreadMarker::new().is_none() || status_item.is_null() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(item) = (unsafe { Retained::retain(status_item as *mut AnyObject) }) else { return };
    let (tx, rx) = mpsc::channel();
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
    let main = Main { item, over: None, desk: None, live: false, doing: None, rest_next: Rest::Away, asked: 0, quiet: Instant::now(), tidying: false, spots: Vec::new(), seed: seed | 1, voice: None, fading: None, buzz_turn: 0, buzzing: false, pads: Vec::new(), slaps: false };
    if !prefs.hiss {
        super::slap::tidy();
    }
    MAIN.with(|m| *m.borrow_mut() = Some(main));
    let first = prefs.clone();
    with_main(move |m| m.feel(&first));

    // the charger and the battery: the first reading is the starting point
    unsafe {
        let source = IOPSNotificationCreateRunLoopSource(power_changed, std::ptr::null_mut());
        if !source.is_null() {
            CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        }
    }
    power_changed(std::ptr::null_mut());
    watch_lid();
    observe("NSWorkspaceDidWakeNotification", false, || {
        send(Event::Wake);
        super::slap::woke();
        // a lid opened while the Mac slept
        if let Some(closed) = lid_closed() {
            lid_is(closed);
        }
    });
    // the screens come back when the lid opens, asleep or not
    observe("NSWorkspaceScreensDidWakeNotification", false, || {
        if let Some(closed) = lid_closed() {
            lid_is(closed);
        }
    });
    observe("com.apple.screenIsLocked", true, || send(Event::Locked(true)));
    observe("com.apple.screenIsUnlocked", true, || send(Event::Locked(false)));

    let watch = prefs.watch;
    let _ = std::thread::Builder::new().name("cat".into()).spawn(move || run(rx, Engine::new(prefs, seed), watch));
}

/// The cat's thread: take in what the Mac felt, decide, draw what changed,
/// sleep until something next moves.
fn run(rx: Receiver<Event>, mut cat: Engine, mut watch: bool) {
    let t0 = Instant::now();
    let clock = || t0.elapsed().as_secs_f64();
    let mut drawn = cat.pose(0.0);
    // the mark as the button shows it at rest
    let rest = face::icon(&face::pose(face::Mood::Idle, 0.0, (0.0, 0.0)), ICON_PX);
    let mut shown = rest.clone();
    let mut drawn_big = false;
    let mut last_draw = f64::MIN;
    // when the pointer and the icon's place are next looked at, and what
    // the pointer last said
    let (mut next_look, mut next_place) = (0.0, 0.0);
    let mut seen = None;
    // the pointer on the cat: (on it, since when), and what the main thread was told
    let mut next_hover = 0.0;
    let mut hover = (false, 0.0);
    let mut hover_told = false;
    loop {
        let now = clock();
        let frame = 1.0 / ICON_FPS;
        let due = (last_draw + frame - now).max(0.0);
        // a face that differs from the one on screen is drawn at the next frame
        let stale = cat.pose(now) != drawn || cat.big() != drawn_big;
        let wait = match cat.sleep(now) {
            _ if stale => Some(due),
            // moving, but not enough to see yet: look again a frame on
            Some(s) if s <= 0.0 => Some(if due > 0.0 { due } else { frame }),
            other => other,
        };
        let wait = if watch { Some(wait.map_or(next_look - now, |w| w.min(next_look - now)).max(0.0)) } else { wait };
        let hot = HOT.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let wait = if hot { Some(wait.map_or(next_hover - now, |w| w.min(next_hover - now)).max(0.0)) } else { wait };
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
            if log() && !matches!(ev, Event::Pointer { .. } | Event::Hand | Event::Nudge) {
                eprintln!("[cat] {ev:?}");
            }
            if let Event::Prefs(p) = &ev {
                if p.watch && !watch {
                    (next_look, next_place, seen) = (now, now, None);
                }
                watch = p.watch;
            }
            cmds.extend(cat.handle(now, ev));
        }
        if watch && now >= next_look {
            next_look = now + if pointer_still() < MOVED_LATELY { LOOK_MOVING } else { LOOK_RESTING };
            if now >= next_place {
                next_place = now + PLACE_EVERY;
                on_main(|m| m.place());
            }
            if let Some(ev @ Event::Pointer { look, near }) = pointer() {
                if seen != Some((look, near)) {
                    seen = Some((look, near));
                    cmds.extend(cat.handle(now, ev));
                }
            }
        }
        cmds.extend(cat.tick(now));
        // a reaction began (each one starts with its sound): the desktop cat
        // acts it out when it is to show
        let began = cmds.iter().rev().find_map(|c| if let Cmd::Sound { kind, .. } = c { Some(*kind) } else { None });
        for cmd in cmds {
            if log() {
                eprintln!("[cat] → {cmd:?}");
            }
            match cmd {
                Cmd::Sound { kind, n } => on_main(move |m| m.play(kind, n)),
                Cmd::Hush => on_main(|m| m.hush()),
                Cmd::Buzz(on) => on_main(move |m| m.buzz(on)),
                Cmd::Big(true) => {}
                Cmd::Big(false) => on_main(|m| m.desk_end()),
                Cmd::Keep(p) => {
                    super::keep(p);
                }
            }
        }
        if let Some(kind) = began.filter(|_| cat.big()) {
            on_main(move |m| m.react(kind));
        }
        // a pointer resting on a cat that lives on screen
        if let Some(rect) = *HOT.lock().unwrap_or_else(|e| e.into_inner()) {
            if now >= next_hover {
                next_hover = now + HOVER_EVERY;
                let on = pointer_at().is_some_and(|(x, y)| x >= rect.origin.x && x <= rect.origin.x + rect.size.width && y >= rect.origin.y && y <= rect.origin.y + rect.size.height);
                if on != hover.0 {
                    hover = (on, now);
                }
                let wait = if on { HOVER_IN } else { HOVER_OUT };
                if hover.0 != hover_told && now - hover.1 >= wait {
                    hover_told = hover.0;
                    let on = hover.0;
                    on_main(move |m| m.hover(on));
                }
            }
        } else if hover_told {
            hover_told = false;
            hover = (false, now);
        }
        let (pose, big) = (cat.pose(now), cat.big());
        let changed = pose != drawn || big != drawn_big;
        if changed && now - last_draw >= frame - 0.002 {
            // a move too small to change a pixel is not sent
            let icon = (pose != drawn).then(|| face::icon(&pose, ICON_PX)).filter(|px| *px != shown);
            if let Some(px) = &icon {
                shown.clone_from(px);
            }
            let icon = icon.map(|px| {
                let at_rest = px == rest;
                (px, at_rest)
            });
            if icon.is_some() {
                on_main(move |m| m.show(Frame { icon }));
            }
            drawn = pose;
            drawn_big = big;
            last_draw = now;
        }
    }
}
