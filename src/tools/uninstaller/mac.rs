//! macOS pieces of the Uninstaller: running apps, quitting them, the drag
//! pasteboard (to notice an app being dragged anywhere) and the mouse
//! button. Shell, AppleScript and app-icon helpers live in `kit::macos`. Elsewhere these
//! are inert so the tool builds and its tests run on Linux.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::PathBuf;

/// A `file://` URL as a path (percent-decoded).
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    let s = s.trim_end_matches('/');
    (!s.is_empty()).then(|| PathBuf::from(s))
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::{NSPasteboard, NSPasteboardNameDrag, NSPasteboardTypeFileURL, NSRunningApplication};
    use objc2_foundation::NSString;
    use std::path::PathBuf;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }

    pub fn is_running(id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
        apps.iter().any(|a| !a.isTerminated())
    }

    /// Ask every running copy to quit, the way the Dock's Quit does (the app
    /// can still ask to save or refuse). Returns whether any was running.
    pub fn ask_to_quit(id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
        let mut any = false;
        for a in apps.iter() {
            if !a.isTerminated() {
                any = true;
                a.terminate();
            }
        }
        any
    }

    /// Whether the left mouse button is down right now.
    pub fn mouse_down() -> bool {
        // kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft
        unsafe { CGEventSourceButtonState(0, 0) }
    }

    pub fn drag_change_count() -> isize {
        unsafe { NSPasteboard::pasteboardWithName(NSPasteboardNameDrag) }.changeCount()
    }

    /// A global mouse monitor; removed when dropped.
    pub struct DragHook(objc2::rc::Retained<objc2::runtime::AnyObject>);

    impl Drop for DragHook {
        fn drop(&mut self) {
            unsafe { objc2_app_kit::NSEvent::removeMonitor(&self.0) };
        }
    }

    /// Calls `f` on every mouse drag and button release in other apps (a
    /// global event monitor: listen-only, no permission needed), so nothing
    /// has to poll while no drag is going on. Must run on the main thread;
    /// None when it couldn't be set up. Dropping the hook removes it.
    pub fn on_mouse_drag(f: impl Fn() + 'static) -> Option<DragHook> {
        use objc2_app_kit::{NSEvent, NSEventMask};
        objc2::MainThreadMarker::new()?;
        let block = block2::RcBlock::new(move |_e: std::ptr::NonNull<NSEvent>| f());
        let mask = NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp;
        NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &block).map(DragHook)
    }

    /// Files on the drag pasteboard (what is being, or was last, dragged).
    pub fn drag_files() -> Vec<PathBuf> {
        let pb = unsafe { NSPasteboard::pasteboardWithName(NSPasteboardNameDrag) };
        let mut out = Vec::new();
        if let Some(items) = pb.pasteboardItems() {
            for it in items.iter() {
                if let Some(u) = unsafe { it.stringForType(NSPasteboardTypeFileURL) } {
                    if let Some(p) = super::file_url_path(&crate::kit::macos::file_path_url(&u.to_string())) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::PathBuf;
    pub fn is_running(_id: &str) -> bool {
        false
    }
    pub fn ask_to_quit(_id: &str) -> bool {
        false
    }
    pub fn mouse_down() -> bool {
        false
    }
    pub fn drag_change_count() -> isize {
        0
    }
    pub struct DragHook;
    pub fn on_mouse_drag(_f: impl Fn() + 'static) -> Option<DragHook> {
        None
    }
    pub fn drag_files() -> Vec<PathBuf> {
        Vec::new()
    }
}

pub use imp::*;
pub use crate::kit::macos::{app_icon, reveal, run_admin, sh_quote, uid};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls() {
        assert_eq!(file_url_path("file:///Applications/Zoom%20Pro.app/"), Some(PathBuf::from("/Applications/Zoom Pro.app")));
    }
}
