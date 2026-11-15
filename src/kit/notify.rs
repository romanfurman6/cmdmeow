//! macOS notifications. Inside the app bundle they go through the system's
//! notification center; a bare binary without a bundle id falls back to
//! AppleScript's `display notification`. Sending never asks macOS for
//! permission: setup asks, or the switch that turns an alert on ([`flip`]).

use crate::kit::perms::{Perm, Status};

/// Notifications are allowed (an alert switch shows on only then).
pub fn allowed() -> bool {
    Perm::Notifications.status() == Status::Allowed
}

/// What an alert switch shows: the person's choice, if macOS lets it through.
pub fn shown(pref: bool) -> bool {
    pref && allowed()
}

/// Click on an alert switch: off when it shows on; otherwise on, asking
/// macOS (or opening its settings) when notifications aren't allowed yet.
pub fn flip(pref: &mut bool) {
    if shown(*pref) {
        *pref = false;
    } else {
        *pref = true;
        if !allowed() {
            Perm::Notifications.ask();
        }
    }
}

/// Post a notification with a title and one line of text.
pub fn send(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    mac::send(title, body);
    #[cfg(not(target_os = "macos"))]
    let _ = (title, body);
}

/// Post a notification with a button. A click on the button calls the
/// handler registered under `handler` (see [`on_action`]) with `payload`.
/// Where buttons can't be shown (no app bundle), it's a plain notification.
pub fn send_with(title: &str, body: &str, button: &str, handler: &'static str, payload: &str) {
    #[cfg(target_os = "macos")]
    mac::send_with(title, body, button, handler, payload);
    #[cfg(not(target_os = "macos"))]
    let _ = (title, body, button, handler, payload);
}

type Handler = Box<dyn Fn(String) + Send + Sync>;
static HANDLERS: std::sync::Mutex<Vec<(&'static str, Handler)>> = std::sync::Mutex::new(Vec::new());

/// Run `f` with the payload when a notification's button sent with this
/// `handler` name is clicked (on whatever thread macOS calls back on).
pub fn on_action(handler: &'static str, f: impl Fn(String) + Send + Sync + 'static) {
    if let Ok(mut g) = HANDLERS.lock() {
        g.retain(|(n, _)| *n != handler);
        g.push((handler, Box::new(f)));
    }
}

/// A button was clicked: hand the payload to its handler.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn dispatch(handler: &str, payload: String) {
    if let Ok(g) = HANDLERS.lock() {
        if let Some((_, f)) = g.iter().find(|(n, _)| *n == handler) {
            f(payload);
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_foundation::{NSBundle, NSString};
    use objc2_user_notifications::{UNMutableNotificationContent, UNNotificationRequest, UNUserNotificationCenter};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    pub fn send(title: &str, body: &str) {
        // the notification center throws without a bundle id (cargo run)
        let bundled = NSBundle::mainBundle().bundleIdentifier().is_some();
        if !bundled {
            osascript(title, body);
            return;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let id = NSString::from_str(&format!("cmdmeow.{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let req = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        center.addNotificationRequest_withCompletionHandler(&req, None);
    }

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSSet};
    use objc2_user_notifications::{UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions, UNNotificationResponse, UNUserNotificationCenterDelegate};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Mutex, OnceLock};

    /// The one button of every category.
    const ACTION: &str = "cmdmeow.button";

    /// Categories by id ("cmdmeow.<button title>") → button title.
    static CATEGORIES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
    /// Notification id → (handler, payload), for the button's click.
    static SENT: Mutex<Option<HashMap<String, (&'static str, String)>>> = Mutex::new(None);

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowNotifyDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &block2::DynBlock<dyn Fn()>) {
                let action = response.actionIdentifier().to_string();
                let id = response.notification().request().identifier().to_string();
                let hit = SENT.lock().ok().and_then(|mut g| g.as_mut().and_then(|m| m.remove(&id)));
                if action == ACTION {
                    if let Some((handler, payload)) = hit {
                        super::dispatch(handler, payload);
                    }
                }
                done.call(());
            }
        }
    );

    impl Delegate {
        fn new() -> Retained<Self> {
            let this = Self::alloc().set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    struct Keep(#[allow(dead_code)] Retained<Delegate>);
    unsafe impl Send for Keep {}
    unsafe impl Sync for Keep {}
    static DELEGATE: OnceLock<Keep> = OnceLock::new();

    /// Make sure a category with this button exists; returns its id.
    fn category(center: &UNUserNotificationCenter, button: &str) -> String {
        let id = format!("cmdmeow.{button}");
        let mut g = match CATEGORIES.lock() {
            Ok(g) => g,
            Err(_) => return id,
        };
        if g.contains_key(&id) {
            return id;
        }
        g.insert(id.clone(), button.to_string());
        // setting the categories replaces them all, so every one goes again
        let cats: Vec<Retained<UNNotificationCategory>> = g
            .iter()
            .map(|(cid, title)| {
                let act = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str(ACTION), &NSString::from_str(title), UNNotificationActionOptions::empty());
                UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(&NSString::from_str(cid), &NSArray::from_retained_slice(&[act]), &NSArray::new(), UNNotificationCategoryOptions::empty())
            })
            .collect();
        center.setNotificationCategories(&NSSet::from_retained_slice(&cats));
        id
    }

    pub fn send_with(title: &str, body: &str, button: &str, handler: &'static str, payload: &str) {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            osascript(title, body);
            return;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let d = DELEGATE.get_or_init(|| Keep(Delegate::new()));
        center.setDelegate(Some(ProtocolObject::from_ref(&*d.0)));
        let cat = category(&center, button);
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setCategoryIdentifier(&NSString::from_str(&cat));
        let id = format!("cmdmeow.{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut g) = SENT.lock() {
            let m = g.get_or_insert_with(HashMap::new);
            // a long run can't grow this without bound
            if m.len() > 200 {
                m.clear();
            }
            m.insert(id.clone(), (handler, payload.to_string()));
        }
        let req = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&id), &content, None);
        center.addNotificationRequest_withCompletionHandler(&req, None);
    }

    fn osascript(title: &str, body: &str) {
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"{}\"", q(body), q(title));
        let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
    }
}
