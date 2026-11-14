//! What Cmd+Meow needs from macOS (privacy permissions), with a live status
//! and a way to ask for each. Tools declare their needs; setup and Settings ›
//! Permissions show them.

use crate::kit::glass::Icon;

/// See [`Perm::take_answered`].
static ANSWERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn answered() {
    ANSWERED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// A macOS permission, or something a tool sets up itself (`Own`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Perm {
    Screen,
    Accessibility,
    FullDisk,
    /// Apple Events to System Events (the login items list).
    SystemEvents,
    Microphone,
    Camera,
    /// CoreBluetooth: AirPods case and bud batteries heard over the air.
    Bluetooth,
    Notifications,
    /// A tool's own setup step, e.g. Fan Control's privileged helper.
    Own { id: &'static str, name: &'static str, icon: Icon },
}

/// One thing a tool needs. `optional`: the tool works without it, only a
/// part of it doesn't (the camera in recordings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Need {
    pub perm: Perm,
    pub optional: bool,
}

impl Need {
    pub const fn must(perm: Perm) -> Need {
        Need { perm, optional: false }
    }
    pub const fn may(perm: Perm) -> Need {
        Need { perm, optional: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Allowed,
    /// macOS hasn't asked yet (or we can't tell): the button asks.
    NotAsked,
    /// Refused or turned off: the button opens System Settings.
    Denied,
}

impl Perm {
    /// Every system permission, in the order setup lists them.
    pub const SYSTEM: [Perm; 8] = [Perm::Screen, Perm::Accessibility, Perm::FullDisk, Perm::SystemEvents, Perm::Microphone, Perm::Camera, Perm::Bluetooth, Perm::Notifications];

    pub fn name(self) -> &'static str {
        match self {
            Perm::Screen => "Screen Recording",
            Perm::Accessibility => "Accessibility",
            Perm::FullDisk => "Full Disk Access",
            Perm::SystemEvents => "System Events",
            Perm::Microphone => "Microphone",
            Perm::Camera => "Camera",
            Perm::Bluetooth => "Bluetooth",
            Perm::Notifications => "Notifications",
            Perm::Own { name, .. } => name,
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Perm::Screen => Icon::ScreenRec,
            Perm::Accessibility => Icon::Access,
            Perm::FullDisk => Icon::Disk,
            Perm::SystemEvents => Icon::Script,
            Perm::Microphone => Icon::Mic,
            Perm::Camera => Icon::Camera,
            Perm::Bluetooth => Icon::Bluetooth,
            Perm::Notifications => Icon::Bell,
            Perm::Own { icon, .. } => icon,
        }
    }

    /// Why the app asks, in a few words (the permission rows).
    pub fn why(self) -> &'static str {
        match self {
            Perm::Screen => "Screenshots and recordings",
            Perm::Accessibility => "Shortcuts and typing dictation",
            Perm::FullDisk => "Finding files to clean up",
            Perm::SystemEvents => "Reading your login items",
            Perm::Microphone => "Dictation and recording voice",
            Perm::Camera => "Camera in recordings",
            Perm::Bluetooth => "AirPods and device batteries",
            Perm::Notifications => "Alerts from your tools",
            Perm::Own { .. } => "",
        }
    }

    /// What the button says before anything was asked.
    pub fn ask_label(self) -> &'static str {
        match self {
            Perm::FullDisk => "Open Settings",
            Perm::Own { .. } => "Install",
            _ => "Allow",
        }
    }

    /// The System Settings pane for this permission.
    pub fn pane(self) -> Option<&'static str> {
        Some(match self {
            Perm::Screen => "Privacy_ScreenCapture",
            Perm::Accessibility => "Privacy_Accessibility",
            Perm::FullDisk => "Privacy_AllFiles",
            Perm::SystemEvents => "Privacy_Automation",
            Perm::Microphone => "Privacy_Microphone",
            Perm::Camera => "Privacy_Camera",
            Perm::Bluetooth => "Privacy_Bluetooth",
            Perm::Notifications => return None,
            Perm::Own { .. } => return None,
        })
    }

    /// macOS answers it with a question over the app (Allow / Don't Allow)
    /// and never needs System Settings, unless it was refused before.
    pub fn answered_in_place(self) -> bool {
        matches!(self, Perm::SystemEvents | Perm::Microphone | Perm::Camera | Perm::Bluetooth | Perm::Notifications)
    }

    /// A macOS question shown over the app was just answered (set by its
    /// completion handler), so the app can take the front back at once
    /// instead of after the next status poll. Read once.
    pub fn take_answered() -> bool {
        ANSWERED.swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// The live status of a system permission (`Own` ones belong to their tool).
    pub fn status(self) -> Status {
        imp::status(self)
    }

    /// Ask: show macOS's question when it has one, else open System Settings
    /// on the right pane. Never blocks.
    pub fn ask(self) {
        let before = self.status();
        if before == Status::Denied || matches!(self, Perm::FullDisk) {
            open_settings(self);
            if self == Perm::FullDisk {
                reveal_app();
            }
            return;
        }
        imp::ask(self);
    }
}

/// Open System Settings on the permission's pane.
pub fn open_settings(p: Perm) {
    let url = match p {
        Perm::Notifications => "x-apple.systempreferences:com.apple.Notifications-Settings.extension".to_string(),
        _ => match p.pane() {
            Some(pane) => format!("x-apple.systempreferences:com.apple.preference.security?{pane}"),
            None => return,
        },
    };
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Show the app in Finder, so it can be dragged into a Settings list
/// (Full Disk Access has no macOS question; the app is added by hand).
pub fn reveal_app() {
    if let Some(app) = app_bundle() {
        let _ = std::process::Command::new("open").arg("-R").arg(app).spawn();
    }
}

/// The .app this binary runs from, if any.
pub fn app_bundle() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(|p| p.to_path_buf())
}

/// The name the app has in System Settings lists ("CmdMeow Dev").
pub fn app_name() -> String {
    app_bundle().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "CmdMeow".into())
}

/// Screen Recording as macOS reported it when the app started. macOS only
/// applies a new grant to a fresh process, so a grant made later needs a
/// restart before capture works.
pub fn screen_at_launch() -> bool {
    use std::sync::OnceLock;
    static AT: OnceLock<bool> = OnceLock::new();
    *AT.get_or_init(imp::screen_preflight)
}

/// The argument a restarted copy gets, so it knows a restart already happened.
const RELAUNCHED: &str = "--relaunched";

/// This copy was started by [`relaunch`]. When Screen Recording still isn't
/// on here, another restart won't help: the grant in System Settings is for
/// another copy of the app (or not on at all), so say that instead.
pub fn relaunched() -> bool {
    std::env::args().any(|a| a == RELAUNCHED)
}

/// Start the app again a second from now (to pick up Screen Recording);
/// the caller quits this copy.
pub fn relaunch() {
    relaunch_with("");
}

/// Passed to a copy started by [`relaunch_to_panel`].
const OPEN_PANEL: &str = "--open-panel";

/// [`relaunch`], and show the menu bar panel once started (setup's last
/// step, "Open panel", when it needs a restart).
pub fn relaunch_to_panel() {
    relaunch_with(OPEN_PANEL);
}

/// Started by [`relaunch_to_panel`]: the only launch that shows the panel.
pub fn launched_to_panel() -> bool {
    std::env::args().any(|a| a == OPEN_PANEL)
}

fn relaunch_with(extra: &str) {
    if let Some(app) = app_bundle() {
        let cmd = format!("sleep 1; /usr/bin/open -n {} --args {RELAUNCHED} {extra}", sh_quote(&app.to_string_lossy()));
        let _ = std::process::Command::new("/bin/sh").args(["-c", &cmd]).spawn();
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Perm, Status};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU8, Ordering};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut c_void;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    #[repr(C)]
    struct AEDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn AECreateDesc(type_code: u32, data: *const c_void, size: isize, result: *mut AEDesc) -> i16;
        fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
        fn AEDeterminePermissionToAutomateTarget(target: *const AEDesc, class: u32, id: u32, ask: u8) -> i32;
    }

    const fn four(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    pub fn screen_preflight() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    /// Screen Recording, live: the preflight answer is fixed for the life of
    /// the process, but other apps' window titles show up the moment access
    /// is granted.
    fn screen_live() -> bool {
        if screen_preflight() {
            return true;
        }
        let raw = unsafe { CGWindowListCopyWindowInfo(1, 0) }; // on screen only
        if raw.is_null() {
            return false;
        }
        let list: Retained<NSArray<NSDictionary<NSString, AnyObject>>> = unsafe { Retained::from_raw(raw.cast()) }.expect("window list");
        let me = std::process::id() as i32;
        let (k_pid, k_name, k_owner, k_layer) = (NSString::from_str("kCGWindowOwnerPID"), NSString::from_str("kCGWindowName"), NSString::from_str("kCGWindowOwnerName"), NSString::from_str("kCGWindowLayer"));
        list.iter().any(|w| {
            let pid = w.objectForKey(&k_pid).and_then(|n| n.downcast::<NSNumber>().ok()).map(|n| n.intValue()).unwrap_or(me);
            let owner = w.objectForKey(&k_owner).and_then(|n| n.downcast::<NSString>().ok()).map(|s| s.to_string()).unwrap_or_default();
            // ordinary app windows only: menu bar items and system overlays
            // (other layers) can carry a name without Screen Recording
            let layer = w.objectForKey(&k_layer).and_then(|n| n.downcast::<NSNumber>().ok()).map(|n| n.intValue()).unwrap_or(-1);
            pid != me && layer == 0 && owner != "Window Server" && owner != "Dock" && w.objectForKey(&k_name).is_some()
        })
    }

    /// Reading the TCC database itself is only allowed with Full Disk Access.
    fn full_disk() -> bool {
        let Some(home) = dirs::home_dir() else { return false };
        std::fs::File::open(home.join("Library/Application Support/com.apple.TCC/TCC.db")).is_ok() || std::fs::read_dir(home.join("Library/Safari")).is_ok()
    }

    /// 0 allowed, -1743 refused, -1744 would ask, -600 System Events isn't running.
    fn system_events(ask: bool) -> i32 {
        let id = b"com.apple.systemevents";
        let mut desc = AEDesc { descriptor_type: 0, data_handle: std::ptr::null_mut() };
        unsafe {
            if AECreateDesc(four(b"bund"), id.as_ptr().cast(), id.len() as isize, &mut desc) != 0 {
                return -1;
            }
            let r = AEDeterminePermissionToAutomateTarget(&desc, four(b"****"), four(b"****"), ask as u8);
            AEDisposeDesc(&mut desc);
            r
        }
    }

    fn media(p: Perm) -> Status {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo};
        let t = if p == Perm::Microphone { unsafe { AVMediaTypeAudio } } else { unsafe { AVMediaTypeVideo } };
        let Some(t) = t else { return Status::NotAsked };
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(t) } {
            AVAuthorizationStatus::Authorized => Status::Allowed,
            AVAuthorizationStatus::NotDetermined => Status::NotAsked,
            _ => Status::Denied,
        }
    }

    // 0 unknown, 1 not asked, 2 denied, 3 allowed; refreshed in the background
    static NOTIF: AtomicU8 = AtomicU8::new(0);
    // when the last refresh was asked for (ms since the epoch): at most once a second
    static NOTIF_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn bundled() -> bool {
        objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    /// The bundle explains Bluetooth use (asking without it gets the app killed).
    fn bluetooth_text() -> bool {
        let key = NSString::from_str("NSBluetoothAlwaysUsageDescription");
        let v: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![&*objc2_foundation::NSBundle::mainBundle(), objectForInfoDictionaryKey: &*key] };
        v.is_some()
    }

    /// CBManager.authorization: 0 not asked, 1 restricted, 2 denied, 3 allowed.
    fn bluetooth() -> Status {
        if !bluetooth_text() {
            return Status::NotAsked;
        }
        let a: isize = unsafe { objc2::msg_send![objc2::class!(CBManager), authorization] };
        match a {
            3 => Status::Allowed,
            1 | 2 => Status::Denied,
            _ => Status::NotAsked,
        }
    }

    fn notifications() -> Status {
        if !bundled() {
            return Status::Allowed; // falls back to AppleScript, which needs nothing
        }
        use objc2_user_notifications::{UNAuthorizationStatus, UNNotificationSettings, UNUserNotificationCenter};
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        if now.saturating_sub(NOTIF_AT.load(Ordering::Relaxed)) < 1000 && NOTIF.load(Ordering::Relaxed) != 0 {
            return cached();
        }
        NOTIF_AT.store(now, Ordering::Relaxed);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let done = block2::RcBlock::new(|s: std::ptr::NonNull<UNNotificationSettings>| {
            let st = unsafe { s.as_ref().authorizationStatus() };
            let v = if st == UNAuthorizationStatus::NotDetermined {
                1
            } else if st == UNAuthorizationStatus::Denied {
                2
            } else {
                3
            };
            NOTIF.store(v, Ordering::Relaxed);
        });
        center.getNotificationSettingsWithCompletionHandler(&done);
        cached()
    }

    fn cached() -> Status {
        match NOTIF.load(Ordering::Relaxed) {
            2 => Status::Denied,
            3 => Status::Allowed,
            _ => Status::NotAsked,
        }
    }

    pub fn status(p: Perm) -> Status {
        let yes = |b: bool| if b { Status::Allowed } else { Status::NotAsked };
        match p {
            Perm::Screen => yes(screen_live()),
            Perm::Accessibility => yes(unsafe { AXIsProcessTrusted() }),
            Perm::FullDisk => yes(full_disk()),
            Perm::SystemEvents => match system_events(false) {
                0 => Status::Allowed,
                -1743 => Status::Denied,
                _ => Status::NotAsked,
            },
            Perm::Microphone | Perm::Camera => media(p),
            Perm::Bluetooth => bluetooth(),
            Perm::Notifications => notifications(),
            Perm::Own { .. } => Status::NotAsked,
        }
    }

    pub fn ask(p: Perm) {
        match p {
            Perm::Screen => {
                // macOS shows its question only once; after that, Settings
                if !unsafe { CGRequestScreenCaptureAccess() } {
                    std::thread::spawn(|| {
                        // the first time, macOS shows its own alert, whose
                        // button opens Settings; opening Settings as well
                        // covered that alert (M1). Only when no alert came
                        // up within 2 s, open Settings ourselves.
                        for _ in 0..10 {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            if screen_live() || crate::kit::settings_window::scene().prompt {
                                return;
                            }
                        }
                        super::open_settings(Perm::Screen);
                    });
                }
            }
            Perm::Accessibility => {
                let key = NSString::from_str("AXTrustedCheckOptionPrompt");
                let yes = NSNumber::new_bool(true);
                let opts: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_retained_objects(&[&*key], &[Retained::into_super(Retained::into_super(yes)).into()]);
                unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&opts).cast()) };
            }
            Perm::FullDisk => {
                super::open_settings(p);
                super::reveal_app();
            }
            Perm::SystemEvents => {
                std::thread::spawn(|| {
                    // the question needs System Events running
                    let _ = std::process::Command::new("open").args(["-g", "-j", "-b", "com.apple.systemevents"]).status();
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    // a "Don't Allow" just shows on the row (its button then
                    // opens Settings); opening Settings here took over the screen
                    let _ = system_events(true);
                    super::answered();
                });
            }
            Perm::Microphone | Perm::Camera => {
                use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo};
                let t = if p == Perm::Microphone { unsafe { AVMediaTypeAudio } } else { unsafe { AVMediaTypeVideo } };
                if let Some(t) = t {
                    let done = block2::RcBlock::new(|_ok: objc2::runtime::Bool| super::answered());
                    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(t, &done) };
                }
            }
            Perm::Bluetooth => {
                // a central manager asks the first time; kept for the app's life
                if !bluetooth_text() {
                    return;
                }
                static ASKED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
                ASKED.get_or_init(|| {
                    let m: *mut AnyObject = unsafe {
                        let alloc: *mut AnyObject = objc2::msg_send![objc2::class!(CBCentralManager), alloc];
                        objc2::msg_send![alloc, initWithDelegate: std::ptr::null_mut::<AnyObject>(), queue: std::ptr::null_mut::<AnyObject>()]
                    };
                    m as usize
                });
            }
            Perm::Notifications => {
                if !bundled() {
                    return;
                }
                use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
                let center = UNUserNotificationCenter::currentNotificationCenter();
                let done = block2::RcBlock::new(|_granted: objc2::runtime::Bool, _err: *mut objc2_foundation::NSError| super::answered());
                center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &done);
            }
            Perm::Own { .. } => {}
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::{Perm, Status};

    pub fn screen_preflight() -> bool {
        true
    }
    pub fn status(_p: Perm) -> Status {
        Status::NotAsked
    }
    pub fn ask(_p: Perm) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_permission_has_a_pane_or_its_own_page() {
        for p in Perm::SYSTEM {
            assert!(!p.name().is_empty());
            assert!(p.pane().is_some() || p == Perm::Notifications, "{p:?}");
        }
    }

    #[test]
    fn an_answer_is_read_once() {
        answered();
        assert!(Perm::take_answered());
        assert!(!Perm::take_answered());
    }
}
