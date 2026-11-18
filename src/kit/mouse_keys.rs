//! Extra mouse buttons (middle, side, and any further ones) as system-wide
//! shortcuts. The shortcut library only knows keys, so these come from an
//! event tap of our own on a thread of its own: a press that matches a bound
//! shortcut is queued for [`super::hotkeys::Keys`] and wakes the app. With
//! Accessibility the press is taken, so the app under the pointer doesn't
//! also go Back; without it the tap only listens and the press goes through.

use super::hotkeys::Shortcut;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Releases kept for owners that never read them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const RELEASES_KEPT: usize = 32;
/// A recorder counts as open this long after it last asked.
const RECORDING_FOR: Duration = Duration::from_millis(500);

const CMD: u8 = 1;
const SHIFT: u8 = 2;
const ALT: u8 = 4;
const CTRL: u8 = 8;

struct Bound {
    id: u32,
    button: u8,
    mods: u8,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct State {
    bound: Vec<Bound>,
    /// Ids pressed / let go, oldest first.
    pressed: Vec<u32>,
    released: Vec<u32>,
    /// Buttons whose press we took: their release is ours too.
    taken: Vec<(u8, u32)>,
    /// While a recorder is open: the button it should take.
    recording_until: Option<Instant>,
    heard: Option<Shortcut>,
}

static STATE: Mutex<State> = Mutex::new(State { bound: Vec::new(), pressed: Vec::new(), released: Vec::new(), taken: Vec::new(), recording_until: None, heard: None });
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn state() -> MutexGuard<'static, State> {
    match STATE.lock() {
        Ok(s) => s,
        Err(p) => p.into_inner(),
    }
}

fn mods_of(s: &Shortcut) -> u8 {
    (if s.cmd { CMD } else { 0 }) | (if s.shift { SHIFT } else { 0 }) | (if s.alt { ALT } else { 0 }) | (if s.ctrl { CTRL } else { 0 })
}

/// Watch for `s` (a mouse-button shortcut). Returns its id, or why not
/// (read after the shortcut: "Mouse 4 is taken").
pub fn register(s: &Shortcut) -> Result<u32, &'static str> {
    let Some(button) = s.mouse_button() else { return Err("can't be used as a shortcut") };
    if !imp::SUPPORTED {
        return Err("can't be used on this system");
    }
    let mods = mods_of(s);
    let id = {
        let mut st = state();
        if st.bound.iter().any(|b| b.button == button && b.mods == mods) {
            return Err("is taken");
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        st.bound.push(Bound { id, button, mods });
        id
    };
    imp::start();
    Ok(id)
}

/// Stop watching for one shortcut.
pub fn unregister(id: u32) {
    let mut st = state();
    st.bound.retain(|b| b.id != id);
    st.pressed.retain(|i| *i != id);
    st.released.retain(|i| *i != id);
}

/// A shortcut recorder is open (call every frame it is): presses go to it,
/// not to bound shortcuts.
pub fn recording() {
    {
        let mut st = state();
        let now = Instant::now();
        // a new recording starts clean, whatever an old one left behind
        if st.recording_until.is_none_or(|t| t < now) {
            st.heard = None;
        }
        st.recording_until = Some(now + RECORDING_FOR);
    }
    imp::start();
}

/// The button the open recorder heard, if any.
pub fn take_heard() -> Option<Shortcut> {
    state().heard.take()
}

/// The pressed ids `mine` knows, as its commands, oldest first.
pub fn take_pressed<C>(mine: impl Fn(u32) -> Option<C>) -> Vec<C> {
    drain(&mut state().pressed, mine)
}

/// The let-go ids `mine` knows, as its commands, oldest first.
pub fn take_released<C>(mine: impl Fn(u32) -> Option<C>) -> Vec<C> {
    drain(&mut state().released, mine)
}

fn drain<C>(list: &mut Vec<u32>, mine: impl Fn(u32) -> Option<C>) -> Vec<C> {
    let mut out = Vec::new();
    list.retain(|id| match mine(*id) {
        Some(c) => {
            out.push(c);
            false
        }
        None => true,
    });
    out
}

/// Button `button` (3 and up) went down with `mods` held. Returns whether
/// it is ours to take.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn down(button: u8, mods: u8) -> bool {
    let take = {
        let mut st = state();
        if st.recording_until.is_some_and(|t| t > Instant::now()) {
            st.heard = Shortcut::mouse(button, mods & CMD != 0, mods & SHIFT != 0, mods & ALT != 0, mods & CTRL != 0);
            false
        } else if let Some(id) = st.bound.iter().find(|b| b.button == button && b.mods == mods).map(|b| b.id) {
            st.pressed.push(id);
            if !st.taken.iter().any(|(b, _)| *b == button) {
                st.taken.push((button, id));
            }
            true
        } else {
            return false;
        }
    };
    super::hotkeys::wake();
    take
}

/// Button `button` came up. Returns whether its press was ours.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn up(button: u8) -> bool {
    {
        let mut st = state();
        let Some(i) = st.taken.iter().position(|(b, _)| *b == button) else { return false };
        let (_, id) = st.taken.remove(i);
        if st.bound.iter().any(|b| b.id == id) {
            st.released.push(id);
            if st.released.len() > RELEASES_KEPT {
                let extra = st.released.len() - RELEASES_KEPT;
                st.released.drain(..extra);
            }
        }
    }
    super::hotkeys::wake();
    true
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::kit::perms::{Perm, Status};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::Once;
    use std::time::Duration;

    pub const SUPPORTED: bool = true;

    type CfRef = *const c_void;
    type TapCallback = unsafe extern "C" fn(proxy: CfRef, kind: u32, event: CfRef, info: *mut c_void) -> CfRef;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: CfRef);
        fn CFMachPortCreateRunLoopSource(alloc: CfRef, port: CfRef, order: isize) -> CfRef;
        fn CFMachPortInvalidate(port: CfRef);
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopAddSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRemoveSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRunInMode(mode: CfRef, seconds: f64, return_after_source: u8) -> i32;
        static kCFRunLoopCommonModes: CfRef;
        static kCFRunLoopDefaultMode: CfRef;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, info: *mut c_void) -> CfRef;
        fn CGEventTapEnable(tap: CfRef, on: bool);
        fn CGEventGetFlags(e: CfRef) -> u64;
        fn CGEventGetIntegerValueField(e: CfRef, field: u32) -> i64;
    }

    const SESSION_TAP: u32 = 1;
    const HEAD_INSERT: u32 = 0;
    const TAP_DEFAULT: u32 = 0;
    const TAP_LISTEN_ONLY: u32 = 1;
    const OTHER_MOUSE_DOWN: u32 = 25;
    const OTHER_MOUSE_UP: u32 = 26;
    const BUTTON_NUMBER: u32 = 3;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    static PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    fn mods(flags: u64) -> u8 {
        let has = |m: u64| flags & m != 0;
        (if has(0x10_0000) { super::CMD } else { 0 }) | (if has(0x2_0000) { super::SHIFT } else { 0 }) | (if has(0x8_0000) { super::ALT } else { 0 }) | (if has(0x4_0000) { super::CTRL } else { 0 })
    }

    unsafe extern "C" fn on_event(_proxy: CfRef, kind: u32, event: CfRef, info: *mut c_void) -> CfRef {
        if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER {
            let port = PORT.load(Ordering::Relaxed);
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
            return event;
        }
        if event.is_null() || (kind != OTHER_MOUSE_DOWN && kind != OTHER_MOUSE_UP) {
            return event;
        }
        // macOS counts from 0: 2 is the middle button, our "Mouse 3"
        let n = unsafe { CGEventGetIntegerValueField(event, BUTTON_NUMBER) } + 1;
        let Ok(button) = u8::try_from(n) else { return event };
        if button < 3 {
            return event;
        }
        let flags = unsafe { CGEventGetFlags(event) };
        // never let a panic cross into CoreGraphics
        let take = std::panic::catch_unwind(|| if kind == OTHER_MOUSE_DOWN { super::down(button, mods(flags)) } else { super::up(button) }).unwrap_or(false);
        // `info` is non-null when the tap may take events (Accessibility)
        if take && !info.is_null() {
            return std::ptr::null();
        }
        event
    }

    /// Start the tap's thread (once). It taps actively once Accessibility is
    /// allowed and only listens until then, checking again now and then.
    pub fn start() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::thread::Builder::new()
                .name("mouse-keys".into())
                .spawn(|| loop {
                    let active = Perm::Accessibility.status() == Status::Allowed;
                    let mask = (1u64 << OTHER_MOUSE_DOWN) | (1u64 << OTHER_MOUSE_UP);
                    let info = if active { 1usize as *mut c_void } else { std::ptr::null_mut() };
                    let port = unsafe { CGEventTapCreate(SESSION_TAP, HEAD_INSERT, if active { TAP_DEFAULT } else { TAP_LISTEN_ONLY }, mask, on_event, info) };
                    if port.is_null() {
                        std::thread::sleep(Duration::from_secs(10));
                        continue;
                    }
                    PORT.store(port as *mut c_void, Ordering::Relaxed);
                    unsafe {
                        let rl = CFRunLoopGetCurrent();
                        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
                        CGEventTapEnable(port, true);
                        loop {
                            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 10.0, 0);
                            if !active && Perm::Accessibility.status() == Status::Allowed {
                                break;
                            }
                        }
                        // Accessibility came: swap the listening tap for one that takes
                        PORT.store(std::ptr::null_mut(), Ordering::Relaxed);
                        CGEventTapEnable(port, false);
                        CFRunLoopRemoveSource(rl, src, kCFRunLoopCommonModes);
                        CFMachPortInvalidate(port);
                        CFRelease(src);
                        CFRelease(port);
                    }
                })
                .ok();
        });
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn reads_the_modifier_flags() {
            assert_eq!(super::mods(0), 0);
            assert_eq!(super::mods(0x10_0000 | 0x8_0000), super::super::CMD | super::super::ALT);
            // caps lock and the like don't count
            assert_eq!(super::mods(0x1_0000 | 0x80_0000), 0);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub const SUPPORTED: bool = false;
    pub fn start() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    // one test: the state is shared by the whole process
    #[test]
    fn presses_reach_their_shortcut_and_recorders_win() {
        let back = Shortcut::mouse(4, false, false, false, false).unwrap();
        let alt_mid = Shortcut::mouse(3, false, false, true, false).unwrap();
        let ids = [(back.clone(), 4u8, 0u8), (alt_mid.clone(), 3, ALT)].map(|(s, b, m)| {
            assert_eq!(mods_of(&s), m);
            let id = state().bound.len() as u32 + 1000;
            state().bound.push(Bound { id, button: b, mods: m });
            id
        });
        // the side button, plain: taken, pressed, then let go
        assert!(down(4, 0));
        assert!(up(4));
        assert_eq!(take_pressed(|id| (id == ids[0]).then_some("back")), vec!["back"]);
        assert_eq!(take_released(|id| (id == ids[0]).then_some("back")), vec!["back"]);
        // with a modifier it's another shortcut; middle alone is no one's
        assert!(!down(4, CMD));
        assert!(!up(4));
        assert!(!down(3, 0));
        assert!(down(3, ALT));
        assert_eq!(take_pressed(|id| (id == ids[1]).then_some(1)), vec![1]);
        assert!(up(3));
        // a recorder open: the press goes to it, nothing fires
        recording();
        assert!(!down(4, 0));
        assert_eq!(take_heard(), Some(back.clone()));
        assert!(take_pressed(|id| Some(id)).is_empty());
        // a stale press doesn't leak into the next recording
        state().heard = Some(alt_mid);
        state().recording_until = Some(Instant::now() - Duration::from_secs(1));
        recording();
        assert_eq!(take_heard(), None);
        state().recording_until = None;
        for id in ids {
            unregister(id);
        }
        assert!(state().bound.iter().all(|b| !ids.contains(&b.id)));
    }
}
