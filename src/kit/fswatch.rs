//! "Something changed in these folders": an FSEvents stream that fires a
//! [`Kick`], so a loop can sleep until a folder changes instead of reading
//! it again every few seconds. Recursive, coalesced by macOS (`latency`
//! seconds), and free while nothing happens. Elsewhere it does nothing and
//! callers keep their timers.

use super::activity::Kick;
use std::path::PathBuf;

/// Start watching `paths` for the rest of the app's life; every change
/// kicks `kick`. False when the stream couldn't be started.
pub fn watch(paths: &[PathBuf], latency: f64, kick: &'static Kick) -> bool {
    imp::watch(paths, latency, kick)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Kick;
    use objc2_foundation::{NSArray, NSString};
    use std::ffi::c_void;
    use std::path::PathBuf;

    #[repr(C)]
    struct Context {
        version: isize,
        info: *mut c_void,
        retain: *const c_void,
        release: *const c_void,
        copy_description: *const c_void,
    }

    type Callback = extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn FSEventStreamCreate(alloc: *const c_void, cb: Callback, ctx: *mut Context, paths: *const c_void, since: u64, latency: f64, flags: u32) -> *mut c_void;
        fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
        fn FSEventStreamStart(stream: *mut c_void) -> u8;
        fn FSEventStreamInvalidate(stream: *mut c_void);
        fn FSEventStreamRelease(stream: *mut c_void);
    }
    unsafe extern "C" {
        fn dispatch_queue_create(label: *const std::ffi::c_char, attr: *mut c_void) -> *mut c_void;
    }

    const SINCE_NOW: u64 = u64::MAX;
    /// kFSEventStreamCreateFlagWatchRoot: also tell when a watched folder
    /// itself is created, moved or removed.
    const WATCH_ROOT: u32 = 0x4;

    extern "C" fn changed(_s: *const c_void, info: *mut c_void, _n: usize, _paths: *mut c_void, _flags: *const u32, _ids: *const u64) {
        // `info` is the `&'static Kick` handed over in `watch`
        let kick = unsafe { &*(info as *const Kick) };
        kick.kick();
    }

    pub fn watch(paths: &[PathBuf], latency: f64, kick: &'static Kick) -> bool {
        if paths.is_empty() {
            return false;
        }
        let list: Vec<_> = paths.iter().map(|p| NSString::from_str(&p.to_string_lossy())).collect();
        let arr = NSArray::from_retained_slice(&list);
        let mut ctx = Context { version: 0, info: kick as *const Kick as *mut c_void, retain: std::ptr::null(), release: std::ptr::null(), copy_description: std::ptr::null() };
        unsafe {
            let stream = FSEventStreamCreate(std::ptr::null(), changed, &mut ctx, &*arr as *const NSArray<NSString> as *const c_void, SINCE_NOW, latency, WATCH_ROOT);
            if stream.is_null() {
                return false;
            }
            let queue = dispatch_queue_create(c"cmdmeow.fswatch".as_ptr(), std::ptr::null_mut());
            FSEventStreamSetDispatchQueue(stream, queue);
            if FSEventStreamStart(stream) == 0 {
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
                return false;
            }
        }
        // the stream and its queue live as long as the app
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Kick;
    use std::path::PathBuf;
    pub fn watch(_paths: &[PathBuf], _latency: f64, _kick: &'static Kick) -> bool {
        false
    }
}
