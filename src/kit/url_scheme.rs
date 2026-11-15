//! Links into the app: `cmdmeow://<tool>/<action>` (the Dev build answers
//! `cmdmeow-dev://`). Raycast, Shortcuts, scripts or `open cmdmeow://awake/on`
//! reach a tool this way. A tool claims its host with [`route`]; a link that
//! arrives before its tool is up (the one that launched the app) waits for it.

use std::sync::Mutex;

type Handler = Box<dyn Fn(&str) -> bool + Send>;

static ROUTES: Mutex<Vec<(&'static str, Handler)>> = Mutex::new(Vec::new());
/// Links whose host no tool has claimed yet, oldest first.
static PENDING: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// Links waiting longer than this many are dropped.
const PENDING_CAP: usize = 8;

/// `(host, action)` of a link, both lowercase: `cmdmeow://Awake/On/` → ("awake", "on").
/// Query and fragment are ignored.
pub fn parse(url: &str) -> Option<(String, String)> {
    let (_, rest) = url.split_once("://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let host = parts.next()?.to_ascii_lowercase();
    let action = parts.collect::<Vec<_>>().join("/").to_ascii_lowercase();
    Some((host, action))
}

/// Let `f` answer links to `host`. It gets the action ("on", "off", …) and
/// says whether it knew it. Links that came early are handed over now.
pub fn route(host: &'static str, f: impl Fn(&str) -> bool + Send + 'static) {
    let early: Vec<String> = {
        let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        let (mine, rest): (Vec<_>, Vec<_>) = p.drain(..).partition(|(h, _)| h == host);
        *p = rest;
        mine.into_iter().map(|(_, a)| a).collect()
    };
    for a in &early {
        answer(host, a, &f);
    }
    ROUTES.lock().unwrap_or_else(|e| e.into_inner()).push((host, Box::new(f)));
}

/// Hand a link to its tool.
pub fn open(url: &str) {
    let Some((host, action)) = parse(url) else {
        eprintln!("[url] can't read {url}");
        return;
    };
    let routes = ROUTES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((h, f)) = routes.iter().find(|(h, _)| *h == host) {
        answer(h, &action, f);
        return;
    }
    drop(routes);
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if p.len() >= PENDING_CAP {
        p.remove(0);
    }
    p.push((host, action));
}

/// Links to `host` as commands for a tool to pick up in its `tick`: `read`
/// turns an action into a command (None = unknown action). Links arrive
/// outside a ui pass, while the tool is busy elsewhere, so they wait here.
pub struct Inbox<C> {
    queue: std::sync::Arc<Mutex<Vec<C>>>,
}

impl<C: Send + 'static> Inbox<C> {
    pub fn new(host: &'static str, ctx: Option<egui::Context>, read: fn(&str) -> Option<C>) -> Self {
        let queue = std::sync::Arc::new(Mutex::new(Vec::new()));
        let q = queue.clone();
        route(host, move |action| {
            let Some(c) = read(action) else { return false };
            q.lock().unwrap_or_else(|e| e.into_inner()).push(c);
            // wake the app even with every window hidden, as a hotkey does
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
            true
        });
        Inbox { queue }
    }

    /// Commands that came in since the last call, oldest first.
    pub fn take(&self) -> Vec<C> {
        std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn answer(host: &str, action: &str, f: &(impl Fn(&str) -> bool + ?Sized)) {
    if !f(action) {
        eprintln!("[url] {host} doesn't know \"{action}\"");
    }
}

/// What opening the app again while it runs means: show its window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const REOPEN_LINK: &str = "cmdmeow://open/window";

/// Start listening for links (and for the app being opened again). Call
/// before the app's run loop starts, so the link that launched it isn't missed.
pub fn install() {
    #[cfg(target_os = "macos")]
    mac::install();
}

/// Once the app has finished launching (call every frame; it runs once).
/// AppKit puts its own "opened again" handler in place while it launches,
/// over the one [`install`] set, so it's set again here.
pub fn after_launch() {
    #[cfg(target_os = "macos")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(mac::set_reopen);
    }
}

/// Another copy of this build is already running (`open -n`, a second
/// download): ask it to show its window. True when this copy should quit.
pub fn hand_over() -> bool {
    #[cfg(target_os = "macos")]
    return mac::hand_over();
    #[cfg(not(target_os = "macos"))]
    false
}

/// The distributed notification one copy sends the running one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn reopen_note() -> String {
    format!("{}.reopen", super::remove::bundle_id())
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, define_class, msg_send, sel, AnyThread};
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    use std::sync::atomic::{AtomicPtr, Ordering};

    /// 'GURL': the Apple event class and id of "open this URL".
    const GURL: u32 = u32::from_be_bytes(*b"GURL");
    /// '----': the event's direct parameter, the URL itself.
    const DIRECT: u32 = u32::from_be_bytes(*b"----");
    /// 'aevt' / 'rapp': the app was opened again while running (Finder,
    /// Spotlight, Dock). A menu bar app has no window to show for it, and its
    /// icon may be hidden (notch, full menu bar), so this opens the window.
    const CORE: u32 = u32::from_be_bytes(*b"aevt");
    const REOPEN: u32 = u32::from_be_bytes(*b"rapp");

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowUrlHandler"]
        struct UrlHandler;

        unsafe impl NSObjectProtocol for UrlHandler {}

        impl UrlHandler {
            #[unsafe(method(handleGetURL:withReply:))]
            fn handle(&self, event: &AnyObject, _reply: &AnyObject) {
                let url: Option<Retained<NSString>> = unsafe {
                    let param: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: DIRECT];
                    param.and_then(|p| msg_send![&*p, stringValue])
                };
                if let Some(u) = url {
                    super::open(&u.to_string());
                }
            }

            #[unsafe(method(handleReopen:withReply:))]
            fn reopen(&self, _event: &AnyObject, _reply: &AnyObject) {
                super::open(super::REOPEN_LINK);
            }

            /// Another copy of the app asked this one to show itself.
            #[unsafe(method(handleReopenNote:))]
            fn reopen_note(&self, _note: &AnyObject) {
                super::open(super::REOPEN_LINK);
            }
        }
    );

    /// The handler, kept for the life of the app (the event manager and the
    /// notification centre don't retain it).
    static HANDLER: AtomicPtr<UrlHandler> = AtomicPtr::new(std::ptr::null_mut());

    fn handler() -> Option<&'static UrlHandler> {
        // SAFETY: set once in `install` and never released
        unsafe { HANDLER.load(Ordering::Acquire).as_ref() }
    }

    pub fn install() {
        let made: Retained<UrlHandler> = unsafe { msg_send![super(UrlHandler::alloc().set_ivars(())), init] };
        HANDLER.store(Retained::into_raw(made), Ordering::Release);
        let Some(h) = handler() else { return };
        unsafe {
            let mgr: Retained<AnyObject> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
            let _: () = msg_send![&*mgr, setEventHandler: h, andSelector: sel!(handleGetURL:withReply:), forEventClass: GURL, andEventID: GURL];
            // delivered even while the app is in the background
            const DELIVER_IMMEDIATELY: usize = 4;
            let name = NSString::from_str(&super::reopen_note());
            let center: Retained<AnyObject> = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
            let _: () = msg_send![&*center, addObserver: h, selector: sel!(handleReopenNote:), name: &*name, object: Option::<&AnyObject>::None, suspensionBehavior: DELIVER_IMMEDIATELY];
        }
        set_reopen();
    }

    /// Answer "opened again" ourselves (see [`super::after_launch`]).
    pub fn set_reopen() {
        let Some(h) = handler() else { return };
        unsafe {
            let mgr: Retained<AnyObject> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
            let _: () = msg_send![&*mgr, setEventHandler: h, andSelector: sel!(handleReopen:withReply:), forEventClass: CORE, andEventID: REOPEN];
        }
    }

    pub fn hand_over() -> bool {
        use objc2_app_kit::NSRunningApplication;
        use objc2_foundation::NSBundle;
        let Some(id) = NSBundle::mainBundle().bundleIdentifier() else { return false };
        let me = std::process::id() as i32;
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        // two copies starting together each see the other: only the later
        // one (higher pid) hands over, so one always stays
        if !running.iter().any(|a| (0..me).contains(&a.processIdentifier())) {
            return false;
        }
        let name = NSString::from_str(&super::reopen_note());
        unsafe {
            let center: Retained<AnyObject> = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
            let _: () = msg_send![&*center, postNotificationName: &*name, object: Option::<&AnyObject>::None, userInfo: Option::<&AnyObject>::None, deliverImmediately: true];
        }
        eprintln!("[url] already running: asked it to show its window");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn reopen_note_is_per_build() {
        let n = reopen_note();
        assert!(n.starts_with(crate::kit::remove::bundle_id()) && n.ends_with(".reopen"));
    }

    #[test]
    fn parses_host_and_action() {
        assert_eq!(parse("cmdmeow://awake/on"), Some(("awake".into(), "on".into())));
        assert_eq!(parse("cmdmeow-dev://Awake/Toggle/?x=1#y"), Some(("awake".into(), "toggle".into())));
        assert_eq!(parse("cmdmeow://awake"), Some(("awake".into(), "".into())));
        assert_eq!(parse("cmdmeow://"), None);
        assert_eq!(parse("awake/on"), None);
    }

    #[test]
    fn early_links_wait_for_their_tool() {
        let hits = Arc::new(AtomicUsize::new(0));
        open("cmdmeow://test-early/on");
        open("cmdmeow://other-host/on");
        let h = hits.clone();
        route("test-early", move |a| {
            assert_eq!(a, "on");
            h.fetch_add(1, Ordering::SeqCst);
            true
        });
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        open("cmdmeow://test-early/on");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert!(PENDING.lock().unwrap().iter().any(|(h, _)| h == "other-host"));
    }

    #[test]
    fn inbox_keeps_known_actions_in_order() {
        open("cmdmeow://test-inbox/two");
        let inbox = Inbox::new("test-inbox", None, |a| match a {
            "one" => Some(1),
            "two" => Some(2),
            _ => None,
        });
        open("cmdmeow://test-inbox/one");
        open("cmdmeow://test-inbox/nope");
        open("cmdmeow://Test-Inbox/Two/");
        assert_eq!(inbox.take(), vec![2, 1, 2]);
        assert!(inbox.take().is_empty());
    }
}
