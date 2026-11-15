//! Where System Settings is on screen and whether a macOS prompt sits on it,
//! so the permission hint can stand beside Settings and step aside while
//! macOS asks for a password or shows its own question.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use egui::{Pos2, Rect, Vec2};

/// What the hint needs to know, read from the on-screen window list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scene {
    /// System Settings' main window, global points, top-left origin.
    pub settings: Option<Rect>,
    /// The usable part of the display Settings is on (no menu bar or Dock).
    pub screen: Option<Rect>,
    /// A macOS prompt is up: a password / Touch ID sheet, a "would like to
    /// record" question, or any small window on top of Settings.
    pub prompt: bool,
}

/// One on-screen window, front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub pid: i32,
    pub owner: String,
    pub layer: i32,
    pub frame: Rect,
}

/// Processes that show macOS's own permission and password prompts.
const PROMPT_OWNERS: [&str; 8] = [
    "SecurityAgent",
    "coreautha",
    "LocalAuthenticationRemoteService",
    "LocalAuthenticationUIService",
    "UserNotificationCenter",
    "universalAccessAuthWarn",
    "CoreServicesUIAgent",
    "tccd",
];

/// Window layers at or above this are the Dock, menu bar and system overlays.
const DOCK_LAYER: i32 = 20;

/// Read the scene from `list` (front to back). `settings_pid` is System
/// Settings' process, `me` ours (our windows never count), `apps` the
/// processes of ordinary Dock apps: their windows (the Finder window Full
/// Disk Access points to) are never prompts.
pub fn read(list: &[Win], settings_pid: Option<i32>, me: i32, apps: &[i32]) -> Scene {
    let others: Vec<(usize, &Win)> = list.iter().enumerate().filter(|(_, w)| w.pid != me && w.frame.width() >= 40.0 && w.frame.height() >= 40.0).collect();
    let main = settings_pid.and_then(|pid| others.iter().filter(|(_, w)| w.pid == pid && w.layer == 0).max_by(|a, b| area(a.1.frame).total_cmp(&area(b.1.frame))).copied());
    let settings = main.map(|(_, w)| w.frame);
    let named = others.iter().any(|(_, w)| PROMPT_OWNERS.contains(&w.owner.as_str()));
    // a dialog in front of Settings: smaller, its middle on Settings (a sheet
    // of Settings itself counts too, e.g. "Quit & Reopen")
    let on_top = main.is_some_and(|(at, s)| {
        others.iter().any(|&(i, w)| i < at && (w.pid == s.pid || !apps.contains(&w.pid)) && (0..DOCK_LAYER).contains(&w.layer) && w.frame.width() < s.frame.width() && w.frame.height() < s.frame.height() && s.frame.contains(w.frame.center()))
    });
    Scene { settings, screen: None, prompt: named || on_top }
}

fn area(r: Rect) -> f32 {
    r.width() * r.height()
}

/// Space between the hint and Settings or the screen edge.
const GAP: f32 = 16.0;

/// Top-left of a card of `size` next to `settings` within `screen`: on its
/// right, else its left, else under it, else over it; with no room at all,
/// in the screen's bottom-right corner.
pub fn place(settings: Rect, screen: Rect, size: Vec2) -> Pos2 {
    let clamp_y = |y: f32| y.clamp(screen.top() + GAP, (screen.bottom() - GAP - size.y).max(screen.top() + GAP));
    let clamp_x = |x: f32| x.clamp(screen.left() + GAP, (screen.right() - GAP - size.x).max(screen.left() + GAP));
    // level with the top of Settings' list, where the switch is
    let y = clamp_y(settings.top() + 72.0);
    if settings.right() + GAP + size.x <= screen.right() - GAP {
        return Pos2::new(settings.right() + GAP, y);
    }
    if settings.left() - GAP - size.x >= screen.left() + GAP {
        return Pos2::new(settings.left() - GAP - size.x, y);
    }
    let x = clamp_x(settings.center().x - size.x / 2.0);
    if settings.bottom() + GAP + size.y <= screen.bottom() - GAP {
        return Pos2::new(x, settings.bottom() + GAP);
    }
    if settings.top() - GAP - size.y >= screen.top() + GAP {
        return Pos2::new(x, settings.top() - GAP - size.y);
    }
    Pos2::new(screen.right() - GAP - size.x, screen.bottom() - GAP - size.y)
}

/// The scene now, read at most four times a second.
pub fn scene() -> Scene {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Scene)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, s)) = *c {
        if at.elapsed() < Duration::from_millis(250) {
            return s;
        }
    }
    let s = imp::scene();
    *c = Some((Instant::now(), s));
    s
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Scene, Win};
    use egui::{Pos2, Rect, Vec2};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSScreen, NSWorkspace};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut c_void;
    }

    fn num(d: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<f64> {
        d.objectForKey(&NSString::from_str(key)).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    }

    fn text(d: &NSDictionary<NSString, AnyObject>, key: &str) -> String {
        d.objectForKey(&NSString::from_str(key)).and_then(|o| o.downcast::<NSString>().ok()).map(|s| s.to_string()).unwrap_or_default()
    }

    /// Every on-screen window, front to back. Owner names, layers and
    /// bounds need no Screen Recording access (titles would).
    fn windows() -> Vec<Win> {
        let raw = unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) }; // on screen, no desktop
        if raw.is_null() {
            return Vec::new();
        }
        let list: Retained<NSArray<NSDictionary<NSString, AnyObject>>> = unsafe { Retained::from_raw(raw.cast()) }.expect("window list");
        let mut out = Vec::new();
        for d in list.iter() {
            let Some(b) = d.objectForKey(&NSString::from_str("kCGWindowBounds")).and_then(|o| o.downcast::<NSDictionary>().ok()) else { continue };
            let b: Retained<NSDictionary<NSString, AnyObject>> = unsafe { Retained::cast_unchecked(b) };
            let (x, y, w, h) = (num(&b, "X").unwrap_or(0.0), num(&b, "Y").unwrap_or(0.0), num(&b, "Width").unwrap_or(0.0), num(&b, "Height").unwrap_or(0.0));
            out.push(Win {
                pid: num(&d, "kCGWindowOwnerPID").unwrap_or(-1.0) as i32,
                owner: text(&d, "kCGWindowOwnerName"),
                layer: num(&d, "kCGWindowLayer").unwrap_or(0.0) as i32,
                frame: Rect::from_min_size(Pos2::new(x as f32, y as f32), Vec2::new(w as f32, h as f32)),
            });
        }
        out
    }

    fn settings_pid() -> Option<i32> {
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.systempreferences"));
        apps.iter().find(|a| !a.isTerminated()).map(|a| a.processIdentifier())
    }

    /// Processes of ordinary apps (the ones with a Dock icon).
    fn app_pids() -> Vec<i32> {
        NSWorkspace::sharedWorkspace().runningApplications().iter().filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular).map(|a| a.processIdentifier()).collect()
    }

    /// The visible frame of the display holding `p`, top-left origin.
    fn screen_at(p: Pos2) -> Option<Rect> {
        let mtm = MainThreadMarker::new()?;
        let screens = NSScreen::screens(mtm);
        let top = screens.firstObject()?.frame().size.height;
        let flip = |f: objc2_foundation::NSRect| Rect::from_min_size(Pos2::new(f.origin.x as f32, (top - (f.origin.y + f.size.height)) as f32), Vec2::new(f.size.width as f32, f.size.height as f32));
        let s = screens.iter().find(|s| flip(s.frame()).contains(p)).or_else(|| screens.firstObject())?;
        Some(flip(s.visibleFrame()))
    }

    pub fn scene() -> Scene {
        let mut s = super::read(&windows(), settings_pid(), std::process::id() as i32, &app_pids());
        s.screen = s.settings.and_then(|r| screen_at(r.center()));
        s
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn scene() -> super::Scene {
        super::Scene::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(pid: i32, owner: &str, layer: i32, x: f32, y: f32, w: f32, h: f32) -> Win {
        Win { pid, owner: owner.into(), layer, frame: Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)) }
    }

    const ME: i32 = 1;
    const SET: i32 = 2;

    fn settings() -> Win {
        win(SET, "Системні параметри", 0, 200.0, 150.0, 715.0, 650.0)
    }

    #[test]
    fn finds_the_settings_window_and_no_prompt() {
        let list = vec![win(0, "Window Server", 25, 0.0, 0.0, 1512.0, 24.0), win(ME, "CmdMeow Dev", 3, 600.0, 400.0, 360.0, 132.0), settings(), win(9, "Finder", 0, 0.0, 0.0, 800.0, 600.0)];
        let s = read(&list, Some(SET), ME, &[]);
        assert_eq!(s.settings, Some(settings().frame));
        assert!(!s.prompt);
    }

    #[test]
    fn a_password_sheet_over_settings_is_a_prompt() {
        // the photo: macOS's "Privacy & Security" password sheet on Settings
        let list = vec![win(ME, "CmdMeow Dev", 3, 450.0, 820.0, 360.0, 132.0), win(77, "SomeAuthAgent", 0, 480.0, 330.0, 345.0, 430.0), settings()];
        assert!(read(&list, Some(SET), ME, &[]).prompt);
    }

    #[test]
    fn known_prompt_processes_count_anywhere() {
        let list = vec![win(77, "universalAccessAuthWarn", 8, 1000.0, 100.0, 260.0, 300.0), settings()];
        assert!(read(&list, Some(SET), ME, &[]).prompt);
        assert!(read(&list, None, ME, &[]).prompt);
    }

    #[test]
    fn our_own_windows_and_windows_behind_settings_dont_count() {
        let list = vec![win(ME, "CmdMeow Dev", 3, 400.0, 300.0, 360.0, 132.0), settings(), win(9, "Notes", 0, 300.0, 300.0, 200.0, 200.0)];
        assert!(!read(&list, Some(SET), ME, &[]).prompt);
    }

    #[test]
    fn a_big_app_window_in_front_is_not_a_prompt() {
        let list = vec![win(9, "Safari", 0, 100.0, 100.0, 1200.0, 800.0), settings()];
        assert!(!read(&list, Some(SET), ME, &[]).prompt);
    }

    #[test]
    fn a_sheet_of_settings_itself_is_a_prompt() {
        let list = vec![win(SET, "Системні параметри", 0, 400.0, 300.0, 300.0, 180.0), settings()];
        let s = read(&list, Some(SET), ME, &[]);
        assert_eq!(s.settings, Some(settings().frame));
        assert!(s.prompt);
    }

    #[test]
    fn a_finder_window_over_settings_is_not_a_prompt() {
        // Full Disk Access opens Finder to drag the app in from: the card must stay
        let list = vec![win(9, "Finder", 0, 300.0, 250.0, 600.0, 400.0), settings()];
        assert!(!read(&list, Some(SET), ME, &[9, SET]).prompt);
        // the same window from a background agent is a prompt
        assert!(read(&list, Some(SET), ME, &[SET]).prompt);
    }

    const CARD: Vec2 = Vec2::new(360.0, 132.0);

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 25.0), Vec2::new(1512.0, 900.0))
    }

    #[test]
    fn stands_right_of_settings_when_there_is_room() {
        let s = settings().frame;
        let p = place(s, screen(), CARD);
        assert_eq!(p.x, s.right() + GAP);
        assert!(!Rect::from_min_size(p, CARD).intersects(s));
    }

    #[test]
    fn goes_left_when_settings_hugs_the_right_edge() {
        let s = Rect::from_min_size(Pos2::new(780.0, 100.0), Vec2::new(715.0, 650.0));
        let p = place(s, screen(), CARD);
        assert_eq!(p.x, s.left() - GAP - CARD.x);
        assert!(!Rect::from_min_size(p, CARD).intersects(s));
    }

    #[test]
    fn goes_under_a_wide_settings_window() {
        let s = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(1300.0, 600.0));
        let p = place(s, screen(), CARD);
        assert_eq!(p.y, s.bottom() + GAP);
        assert!(!Rect::from_min_size(p, CARD).intersects(s));
    }

    #[test]
    fn with_no_room_it_takes_the_corner_and_stays_on_screen() {
        let p = place(screen(), screen(), CARD);
        assert!(screen().contains_rect(Rect::from_min_size(p, CARD)));
    }
}
