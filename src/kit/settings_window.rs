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
    /// The Finder window in front (Full Disk Access reveals the app there
    /// to drag in): the hint keeps off it too.
    pub finder: Option<Rect>,
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
#[cfg_attr(not(test), allow(dead_code))]
pub fn read(list: &[Win], settings_pid: Option<i32>, me: i32, apps: &[i32]) -> Scene {
    read_with_finder(list, settings_pid, None, me, apps, &[])
}

/// [`read`], also finding the front Finder window of `finder_pid`.
/// `before`: prompt windows already up when the ask started (another app's
/// alert left open, M1); they never count.
pub fn read_with_finder(list: &[Win], settings_pid: Option<i32>, finder_pid: Option<i32>, me: i32, apps: &[i32], before: &[Win]) -> Scene {
    let others: Vec<(usize, &Win)> = list.iter().enumerate().filter(|(_, w)| w.pid != me && w.frame.width() >= 40.0 && w.frame.height() >= 40.0).collect();
    let main = settings_pid.and_then(|pid| others.iter().filter(|(_, w)| w.pid == pid && w.layer == 0).max_by(|a, b| area(a.1.frame).total_cmp(&area(b.1.frame))).copied());
    let settings = main.map(|(_, w)| w.frame);
    let named = others.iter().any(|(_, w)| is_prompt(w) && !before.contains(w));
    // a dialog in front of Settings: smaller, its middle on Settings (a sheet
    // of Settings itself counts too, e.g. "Quit & Reopen")
    let on_top = main.is_some_and(|(at, s)| {
        others.iter().any(|&(i, w)| i < at && !before.contains(w) && (w.pid == s.pid || !apps.contains(&w.pid)) && (0..DOCK_LAYER).contains(&w.layer) && w.frame.width() < s.frame.width() && w.frame.height() < s.frame.height() && s.frame.contains(w.frame.center()))
    });
    // the front one: the list runs front to back; the desktop's own
    // full-screen Finder window is left out by its size
    let finder = finder_pid.and_then(|pid| others.iter().find(|(_, w)| w.pid == pid && w.layer == 0 && w.frame.width() >= 200.0 && w.frame.height() >= 150.0)).map(|(_, w)| w.frame);
    Scene { settings, finder, screen: None, prompt: named || on_top }
}

fn is_prompt(w: &Win) -> bool {
    PROMPT_OWNERS.contains(&w.owner.as_str())
}

/// The prompt windows in `list`: the ones up before an ask.
pub fn prompts(list: &[Win]) -> Vec<Win> {
    list.iter().filter(|w| is_prompt(w)).cloned().collect()
}

fn area(r: Rect) -> f32 {
    r.width() * r.height()
}

/// Space between the hint and Settings or the screen edge.
const GAP: f32 = 16.0;

/// Top-left of a card of `size` next to `settings` within `screen`: on its
/// right, else its left, else under it, else over it; with no room at all,
/// in the screen's bottom-right corner.
#[cfg_attr(not(test), allow(dead_code))]
pub fn place(settings: Rect, screen: Rect, size: Vec2) -> Pos2 {
    place_clear(settings, None, screen, size)
}

/// [`place`], keeping off `finder` as well: the spots beside Settings come
/// first, then the ones beside Finder, and the first that covers neither
/// window wins. With none clear, the screen's bottom-right corner.
pub fn place_clear(settings: Rect, finder: Option<Rect>, screen: Rect, size: Vec2) -> Pos2 {
    let clamp_y = |y: f32| y.clamp(screen.top() + GAP, (screen.bottom() - GAP - size.y).max(screen.top() + GAP));
    let clamp_x = |x: f32| x.clamp(screen.left() + GAP, (screen.right() - GAP - size.x).max(screen.left() + GAP));
    let inner = screen.shrink(GAP - 0.5);
    let clear = |p: Pos2| {
        let r = Rect::from_min_size(p, size);
        inner.contains_rect(r) && !r.intersects(settings.expand(GAP - 0.5)) && !finder.is_some_and(|f| r.intersects(f.expand(GAP - 0.5)))
    };
    let around = |w: Rect| {
        // level with the top of Settings' list, where the switch is
        let y = clamp_y(w.top() + 72.0);
        let x = clamp_x(w.center().x - size.x / 2.0);
        [
            Pos2::new(w.right() + GAP, y),
            Pos2::new(w.left() - GAP - size.x, y),
            Pos2::new(x, w.bottom() + GAP),
            Pos2::new(x, w.top() - GAP - size.y),
            // under or over it, by its left or right edge
            Pos2::new(clamp_x(w.left()), w.bottom() + GAP),
            Pos2::new(clamp_x(w.right() - size.x), w.bottom() + GAP),
        ]
    };
    let mut spots = around(settings).to_vec();
    if let Some(f) = finder {
        spots.extend(around(f));
    }
    spots.into_iter().find(|&p| clear(p)).unwrap_or(Pos2::new(screen.right() - GAP - size.x, screen.bottom() - GAP - size.y))
}

/// Prompt windows on screen when the current ask started.
static BEFORE: std::sync::Mutex<Vec<Win>> = std::sync::Mutex::new(Vec::new());

/// An ask starts: macOS prompts already on screen (another app's alert
/// left open) belong to something else, and must not hide the hint for
/// as long as they stay up.
pub fn begin() {
    *BEFORE.lock().unwrap_or_else(|e| e.into_inner()) = imp::prompts_now();
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

    fn pid_of(bundle: &str) -> Option<i32> {
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle));
        apps.iter().find(|a| !a.isTerminated()).map(|a| a.processIdentifier())
    }

    fn settings_pid() -> Option<i32> {
        pid_of("com.apple.systempreferences")
    }

    pub fn prompts_now() -> Vec<Win> {
        super::prompts(&windows())
    }

    /// Whether System Settings has its window on screen yet.
    pub fn settings_up() -> bool {
        let Some(pid) = settings_pid() else { return false };
        windows().iter().any(|w| w.pid == pid && w.layer == 0 && w.frame.width() >= 200.0)
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
        let before = super::BEFORE.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut s = super::read_with_finder(&windows(), settings_pid(), pid_of("com.apple.finder"), std::process::id() as i32, &app_pids(), &before);
        s.screen = s.settings.and_then(|r| screen_at(r.center()));
        s
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn scene() -> super::Scene {
        super::Scene::default()
    }

    pub fn settings_up() -> bool {
        false
    }

    pub fn prompts_now() -> Vec<super::Win> {
        Vec::new()
    }
}

/// Whether System Settings has its window on screen yet.
pub fn settings_up() -> bool {
    imp::settings_up()
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
    fn a_prompt_up_before_the_ask_doesnt_count() {
        // M1: another app's alert (local network) left open hid the hint
        // for the whole setup
        let alert = win(55, "UserNotificationCenter", 0, 1100.0, 40.0, 260.0, 300.0);
        let list = vec![alert.clone(), settings()];
        assert!(!read_with_finder(&list, Some(SET), None, ME, &[SET], &prompts(&list)).prompt);
        // nor when it sits in the middle of Settings
        let over = win(55, "UserNotificationCenter", 0, 420.0, 350.0, 260.0, 200.0);
        let list = vec![over, settings()];
        assert!(!read_with_finder(&list, Some(SET), None, ME, &[SET], &prompts(&list)).prompt);
        // a new one that comes up while asking still does
        let ours = win(56, "UserNotificationCenter", 0, 600.0, 300.0, 260.0, 300.0);
        let list = vec![ours, alert.clone(), settings()];
        assert!(read_with_finder(&list, Some(SET), None, ME, &[SET], &[alert]).prompt);
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

    #[test]
    fn finds_the_front_finder_window() {
        let list = vec![win(9, "Finder", 0, 900.0, 380.0, 880.0, 420.0), settings(), win(9, "Finder", 0, 0.0, 0.0, 1512.0, 982.0)];
        let s = read_with_finder(&list, Some(SET), Some(9), ME, &[9, SET], &[]);
        assert_eq!(s.finder, Some(list[0].frame));
        assert!(!s.prompt);
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
    fn keeps_off_the_finder_window_too() {
        // the photo: Settings on the left, Finder's Applications window on
        // the right of it, where the card used to stand
        let set = Rect::from_min_size(Pos2::new(80.0, 50.0), Vec2::new(715.0, 650.0));
        let finder = Rect::from_min_size(Pos2::new(800.0, 120.0), Vec2::new(700.0, 400.0));
        let p = place_clear(set, Some(finder), screen(), CARD);
        let card = Rect::from_min_size(p, CARD);
        assert!(!card.intersects(set));
        assert!(!card.intersects(finder));
        assert!(screen().contains_rect(card));
    }

    #[test]
    fn with_no_room_it_takes_the_corner_and_stays_on_screen() {
        let p = place(screen(), screen(), CARD);
        assert!(screen().contains_rect(Rect::from_min_size(p, CARD)));
    }
}
