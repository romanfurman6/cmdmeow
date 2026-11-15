//! The system's file-change history (FSEvents): which folders changed since
//! a moment, so an update of the Disk Map reads only those again.

use super::scan::Change;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The newest event id now; a map made from here on is current up to it.
pub fn current() -> Option<u64> {
    imp::current()
}

/// Folders that changed since event `id` under `roots`, waiting at most
/// `wait` for the history to be read. `None` when it's incomplete (events
/// were dropped, ids wrapped, a root moved) or took too long: the caller
/// then maps from scratch.
pub fn since(id: u64, roots: &[PathBuf], wait: Duration) -> Option<Vec<Change>> {
    let raw = imp::since(id, roots, wait)?;
    if raw.len() > super::scan::UPDATE_LIMIT {
        return None; // so much changed that mapping again is quicker
    }
    changes(raw)
}

const MUST_SCAN_SUBDIRS: u32 = 0x1;
const USER_DROPPED: u32 = 0x2;
const KERNEL_DROPPED: u32 = 0x4;
const IDS_WRAPPED: u32 = 0x8;
const HISTORY_DONE: u32 = 0x10;
const ROOT_CHANGED: u32 = 0x20;
const MOUNT: u32 = 0x40;
const UNMOUNT: u32 = 0x80;

/// The Data volume as macOS mounts it; what's on it is also reached
/// through firmlinks from "/" (/Users, /Applications…), which is where the
/// map has it.
const DATA: &str = "/System/Volumes/Data";

/// Raw (path, flags) events turned into changed folders on the map's paths,
/// one per folder, nested ones kept only under a folder read whole.
fn changes(raw: Vec<(String, u32)>) -> Option<Vec<Change>> {
    let mut out: std::collections::HashMap<PathBuf, bool> = Default::default();
    for (path, flags) in raw {
        if flags & (USER_DROPPED | KERNEL_DROPPED | IDS_WRAPPED | ROOT_CHANGED) != 0 {
            return None;
        }
        if flags & (HISTORY_DONE | MOUNT | UNMOUNT) != 0 {
            continue;
        }
        let p = map_path(&path);
        let deep = flags & MUST_SCAN_SUBDIRS != 0;
        if deep && p == Path::new("/") {
            return None;
        }
        let e = out.entry(p).or_insert(false);
        *e |= deep;
    }
    let deep: Vec<PathBuf> = out.iter().filter(|(_, d)| **d).map(|(p, _)| p.clone()).collect();
    let mut v: Vec<Change> = out
        .into_iter()
        // anything under a folder read whole is covered by it
        .filter(|(p, _)| !deep.iter().any(|q| p != q && p.starts_with(q)))
        .map(|(path, deep)| Change { path, deep })
        .collect();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    Some(v)
}

fn map_path(raw: &str) -> PathBuf {
    let t = raw.trim_end_matches('/');
    let t = if t.is_empty() { "/" } else { t };
    match Path::new(t).strip_prefix(DATA) {
        Ok(rest) => Path::new("/").join(rest),
        Err(_) => PathBuf::from(t),
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_foundation::{NSArray, NSString};
    use std::ffi::{c_char, c_void, CStr};
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

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
        fn FSEventsGetCurrentEventId() -> u64;
        fn FSEventStreamCreate(alloc: *const c_void, cb: Callback, ctx: *mut Context, paths: *const c_void, since: u64, latency: f64, flags: u32) -> *mut c_void;
        fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
        fn FSEventStreamStart(stream: *mut c_void) -> u8;
        fn FSEventStreamStop(stream: *mut c_void);
        fn FSEventStreamInvalidate(stream: *mut c_void);
        fn FSEventStreamRelease(stream: *mut c_void);
    }
    unsafe extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
        fn dispatch_release(obj: *mut c_void);
    }

    /// kFSEventStreamCreateFlagNoDefer: deliver at once, no latency wait.
    const NO_DEFER: u32 = 0x2;

    #[derive(Default)]
    struct Got {
        events: Mutex<(Vec<(String, u32)>, bool)>,
        done: Condvar,
    }

    extern "C" fn got(_s: *const c_void, info: *mut c_void, n: usize, paths: *mut c_void, flags: *const u32, _ids: *const u64) {
        // `info` is the `Got` kept alive in `since` (one reference is leaked)
        let g = unsafe { &*(info as *const Got) };
        let paths = paths as *const *const c_char;
        let mut e = g.events.lock().unwrap_or_else(|x| x.into_inner());
        for i in 0..n {
            let (p, f) = unsafe { (CStr::from_ptr(*paths.add(i)).to_string_lossy().into_owned(), *flags.add(i)) };
            if f & super::HISTORY_DONE != 0 {
                e.1 = true;
            }
            if e.0.len() <= super::super::scan::UPDATE_LIMIT {
                e.0.push((p, f));
            }
        }
        g.done.notify_all();
    }

    pub fn current() -> Option<u64> {
        Some(unsafe { FSEventsGetCurrentEventId() })
    }

    pub fn since(id: u64, roots: &[PathBuf], wait: Duration) -> Option<Vec<(String, u32)>> {
        let g = Arc::new(Got::default());
        // the stream's queue may still hold a callback after it's released,
        // so it keeps its own reference for good (a few bytes per update)
        let info = Arc::into_raw(g.clone()) as *mut c_void;
        let list: Vec<_> = roots.iter().map(|p| NSString::from_str(&p.to_string_lossy())).collect();
        let arr = NSArray::from_retained_slice(&list);
        let mut ctx = Context { version: 0, info, retain: std::ptr::null(), release: std::ptr::null(), copy_description: std::ptr::null() };
        unsafe {
            let stream = FSEventStreamCreate(std::ptr::null(), got, &mut ctx, &*arr as *const NSArray<NSString> as *const c_void, id, 0.0, NO_DEFER);
            if stream.is_null() {
                return None;
            }
            let queue = dispatch_queue_create(c"cmdmeow.diskmap.history".as_ptr(), std::ptr::null_mut());
            FSEventStreamSetDispatchQueue(stream, queue);
            if FSEventStreamStart(stream) == 0 {
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
                dispatch_release(queue);
                return None;
            }
            let deadline = std::time::Instant::now() + wait;
            let mut e = g.events.lock().unwrap_or_else(|x| x.into_inner());
            while !e.1 {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                e = g.done.wait_timeout(e, left).unwrap_or_else(|x| x.into_inner()).0;
            }
            let done = e.1;
            let events = std::mem::take(&mut e.0);
            drop(e);
            FSEventStreamStop(stream);
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
            dispatch_release(queue);
            done.then_some(events)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::PathBuf;
    use std::time::Duration;
    pub fn current() -> Option<u64> {
        None
    }
    pub fn since(_id: u64, _roots: &[PathBuf], _wait: Duration) -> Option<Vec<(String, u32)>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(p: &str, deep: bool) -> Change {
        Change { path: PathBuf::from(p), deep }
    }

    #[test]
    fn data_volume_paths_map_onto_the_firmlinks() {
        assert_eq!(map_path("/System/Volumes/Data/Users/me/Downloads/"), PathBuf::from("/Users/me/Downloads"));
        assert_eq!(map_path("/Users/me/"), PathBuf::from("/Users/me"));
        assert_eq!(map_path("/"), PathBuf::from("/"));
    }

    #[test]
    fn events_become_folders() {
        let raw = vec![
            ("/Users/me/a/".to_string(), 0),
            ("/System/Volumes/Data/Users/me/a/".to_string(), 0),
            ("/Users/me/b/".to_string(), MUST_SCAN_SUBDIRS),
            ("/Users/me/b/c/".to_string(), 0),
            ("/".to_string(), HISTORY_DONE),
        ];
        assert_eq!(changes(raw).unwrap(), vec![ch("/Users/me/a", false), ch("/Users/me/b", true)]);
    }

    #[test]
    fn lost_history_means_a_new_map() {
        assert!(changes(vec![("/Users/".into(), KERNEL_DROPPED)]).is_none());
        assert!(changes(vec![("/".into(), MUST_SCAN_SUBDIRS)]).is_none());
        assert!(changes(vec![("/x/".into(), IDS_WRAPPED)]).is_none());
    }
}
