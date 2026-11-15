//! Crash and error reports to Sentry: only in official release builds, and
//! only after the person said yes (setup step or Settings › General).
//!
//! The DSN is baked in at compile time from `CMDMEOW_SENTRY_DSN`, which only
//! `.github/workflows/release.yml` sets (from a repository secret). Every other
//! build (forks, `cargo run`, `scripts/dev-install.sh`, PR CI) has no DSN, so
//! the SDK is never started, the question is never asked and nothing leaves
//! the Mac. Dev bundles stay off even with a DSN.

use super::sync::LockExt;
use std::sync::Mutex;

/// The running client; dropping it flushes and stops reporting.
static CLIENT: Mutex<Option<sentry::ClientInitGuard>> = Mutex::new(None);

/// The DSN this binary was built with, if any.
fn dsn() -> Option<&'static str> {
    option_env!("CMDMEOW_SENTRY_DSN").map(str::trim).filter(|d| !d.is_empty())
}

/// This is an official build that can send crash reports (so ask about it).
pub fn available() -> bool {
    dsn().is_some() && !super::fs::is_dev()
}

fn consent_file() -> std::path::PathBuf {
    super::fs::data_path("crash-reports")
}

/// What the person chose: Some(true) send, Some(false) don't, None not asked yet.
pub fn consent() -> Option<bool> {
    match std::fs::read_to_string(consent_file()).ok()?.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// Saves the choice and starts or stops reporting right away.
pub fn set_consent(on: bool) {
    let f = consent_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, if on { "on" } else { "off" });
    if on {
        start();
    } else {
        // drop outside the lock: closing flushes, which can take a moment
        let old = CLIENT.lock_ok().take();
        drop(old);
    }
}

/// Starts Sentry when this is an official build and the person agreed.
/// Hold the returned value in `main`: when it drops (normal exit or a panic
/// unwinding out of main) queued reports are sent first.
pub fn start_if_allowed() -> Flush {
    if consent() == Some(true) {
        start();
    }
    Flush
}

/// Whether reports are being sent right now.
pub fn running() -> bool {
    CLIENT.lock_ok().is_some()
}

fn start() {
    let Some(dsn) = dsn() else { return };
    if super::fs::is_dev() {
        return;
    }
    let mut slot = CLIENT.lock_ok();
    if slot.is_some() {
        return;
    }
    let release = match option_env!("CMDMEOW_BUILD") {
        Some(b) if !b.is_empty() => format!("cmdmeow@{}+{b}", env!("CARGO_PKG_VERSION")),
        _ => format!("cmdmeow@{}", env!("CARGO_PKG_VERSION")),
    };
    // crashes and errors only (no tracing, sessions or logs: those features
    // are off in Cargo.toml), and no IP address or other personal data
    let options = sentry::ClientOptions::new()
        .dsn(dsn)
        .release(release)
        .environment("production")
        .send_default_pii(false)
        // not the Mac's hostname, which often holds the owner's name
        .server_name("mac")
        .before_send(|mut event| {
            scrub(&mut event);
            Some(event)
        });
    let guard = sentry::init(options);
    if guard.is_enabled() {
        *slot = Some(guard);
        flush_after_panics();
    }
}

/// Sends queued reports when dropped.
pub struct Flush;

impl Drop for Flush {
    fn drop(&mut self) {
        if let Some(c) = sentry::Hub::current().client() {
            c.flush(Some(std::time::Duration::from_secs(2)));
        }
    }
}

/// Swaps the home folder for `~` everywhere a path can show up in a report
/// (error texts, breadcrumbs, extra data, stack frame paths), so none of
/// them carries the user name.
fn scrub(event: &mut sentry::protocol::Event<'static>) {
    let Some(home) = dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).filter(|h| h.len() > 1) else { return };
    scrub_with(event, &home);
}

fn scrub_with(event: &mut sentry::protocol::Event<'static>, home: &str) {
    use sentry::protocol::{Stacktrace, Value};
    let fix = |s: &mut String| {
        if s.contains(home) {
            *s = s.replace(home, "~");
        }
    };
    let fix_opt = |s: &mut Option<String>| {
        if let Some(s) = s.as_mut() {
            fix(s);
        }
    };
    fn fix_value(v: &mut Value, fix: &dyn Fn(&mut String)) {
        match v {
            Value::String(s) => fix(s),
            Value::Array(a) => a.iter_mut().for_each(|v| fix_value(v, fix)),
            Value::Object(o) => o.values_mut().for_each(|v| fix_value(v, fix)),
            _ => {}
        }
    }
    let fix_stack = |st: &mut Option<Stacktrace>| {
        for f in st.iter_mut().flat_map(|st| st.frames.iter_mut()) {
            fix_opt(&mut f.filename);
            fix_opt(&mut f.abs_path);
        }
    };
    fix_opt(&mut event.message);
    if let Some(l) = event.logentry.as_mut() {
        fix(&mut l.message);
    }
    for e in event.exception.values.iter_mut() {
        fix_opt(&mut e.value);
        fix_stack(&mut e.stacktrace);
    }
    fix_stack(&mut event.stacktrace);
    for t in event.threads.values.iter_mut() {
        fix_stack(&mut t.stacktrace);
    }
    for b in event.breadcrumbs.values.iter_mut() {
        fix_opt(&mut b.message);
        b.data.values_mut().for_each(|v| fix_value(v, &fix));
    }
    event.extra.values_mut().for_each(|v| fix_value(v, &fix));
}

/// After Sentry's own panic hook has queued the report, wait (briefly) for it
/// to go out: the process may exit or abort right after a panic, before the
/// sending thread gets to it.
fn flush_after_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            prev(info);
            if let Some(c) = sentry::Hub::current().client() {
                c.flush(Some(std::time::Duration::from_secs(2)));
            }
        }));
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_builds_never_report() {
        // CI and local builds never set CMDMEOW_SENTRY_DSN: nothing to ask, nothing starts
        if option_env!("CMDMEOW_SENTRY_DSN").is_none() {
            assert!(!super::available());
            super::start();
            assert!(!super::running());
        }
    }

    #[test]
    fn home_folder_is_scrubbed() {
        use sentry::protocol::{Breadcrumb, Event, Exception, Frame, Stacktrace, Value};
        let home = "/Users/someone";
        let frame = Frame { abs_path: Some(format!("{home}/src/a.rs")), filename: Some(format!("{home}/src/a.rs")), ..Default::default() };
        let mut ev = Event { message: Some(format!("no file {home}/Desktop/a.png")), ..Default::default() };
        ev.exception.values.push(Exception { value: Some(format!("open {home}/x")), stacktrace: Some(Stacktrace { frames: vec![frame], ..Default::default() }), ..Default::default() });
        let mut b = Breadcrumb { message: Some(format!("read {home}/y")), ..Default::default() };
        b.data.insert("path".into(), Value::String(format!("{home}/z")));
        ev.breadcrumbs.values.push(b);
        ev.extra.insert("files".into(), Value::Array(vec![Value::String(format!("{home}/w"))]));
        super::scrub_with(&mut ev, home);
        let all = serde_json::to_string(&ev).unwrap();
        assert!(!all.contains(home), "{all}");
        assert_eq!(ev.message.as_deref(), Some("no file ~/Desktop/a.png"));
        assert_eq!(ev.exception.values[0].stacktrace.as_ref().unwrap().frames[0].abs_path.as_deref(), Some("~/src/a.rs"));
        assert_eq!(ev.breadcrumbs.values[0].message.as_deref(), Some("read ~/y"));
    }
}
