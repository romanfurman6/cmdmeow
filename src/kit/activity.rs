//! Whether anything of the app is on screen, so background work can slow
//! down while it isn't, and stop asking for frames nobody would see.
//!
//! The shell reports visibility every frame ([`set_visible`]). Background
//! loops sleep with [`nap`], which ends early when a window comes on screen
//! (fresh numbers right away) or when their own [`Kick`] fires; they ask for
//! frames with [`repaint`], which does nothing while hidden: the shell's
//! hidden heartbeat picks their changes up (tray title, tool ticks).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

static VISIBLE: AtomicBool = AtomicBool::new(true);
/// Bumped whenever a sleeper should look up: shown, or some kick.
static GEN: Mutex<u64> = Mutex::new(0);
static WAKE: Condvar = Condvar::new();

fn bump() {
    let mut g = GEN.lock().unwrap_or_else(|e| e.into_inner());
    *g = g.wrapping_add(1);
    WAKE.notify_all();
}

/// Called by the shell every frame: is any of its windows (or a tool's own
/// window) on screen?
pub fn set_visible(v: bool) {
    let was = VISIBLE.swap(v, Ordering::Relaxed);
    if v && !was {
        bump();
    }
}

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Called by the recorder: is a recording counting down, live or saving?
/// Until it's over, the app counts as hidden (widgets and monitors slow to
/// their hidden pace) and heavy work (folder walks) waits.
pub fn set_recording(v: bool) {
    let was = RECORDING.swap(v, Ordering::Relaxed);
    if was && !v {
        bump();
    }
}

/// Whether a recording is under way.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// Run the current thread at background priority: macOS keeps it on the
/// efficiency cores and slows its disk reads, so it never competes with
/// the app or a recording. For watchers and folder walks.
pub fn low_priority() {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0);
    }
}

/// Whether anything of the app is on screen (and no recording is under way).
pub fn visible() -> bool {
    VISIBLE.load(Ordering::Relaxed) && !recording()
}

/// Ask for a frame if someone can see it.
#[track_caller]
pub fn repaint(ctx: &egui::Context) {
    if visible() {
        ctx.request_repaint();
    }
}

/// A wake-up call for one background loop ("settings changed, look now").
#[derive(Default)]
pub struct Kick(AtomicBool);

impl Kick {
    pub const fn new() -> Self {
        Kick(AtomicBool::new(false))
    }
    pub fn kick(&self) {
        self.0.store(true, Ordering::Relaxed);
        bump();
    }
    /// Take a pending kick.
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::Relaxed)
    }
}

/// Why [`nap`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    Timeout,
    /// A window came on screen.
    Shown,
    /// The loop's own kick (taken).
    Kicked,
}

/// Sleep up to `d`, ending early when a window comes on screen or `kick`
/// fires. No polling: the thread is parked until one of those happens.
pub fn nap(d: Duration, kick: Option<&Kick>) -> Wake {
    let until = Instant::now() + d;
    let mut g = GEN.lock().unwrap_or_else(|e| e.into_inner());
    let start = *g;
    let was_visible = visible();
    loop {
        if kick.is_some_and(Kick::take) {
            return Wake::Kicked;
        }
        if *g != start && !was_visible && visible() {
            return Wake::Shown;
        }
        let now = Instant::now();
        if now >= until {
            return Wake::Timeout;
        }
        g = WAKE.wait_timeout(g, until - now).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// "Every so often": due the first time, then once `every` has passed.
#[derive(Default, Clone, Copy)]
pub struct Every {
    last: Option<Instant>,
}

impl Every {
    /// Due now? If so, counts as done.
    pub fn due(&mut self, every: Duration) -> bool {
        let due = self.last.is_none_or(|t| t.elapsed() >= every);
        if due {
            self.last = Some(Instant::now());
        }
        due
    }
    /// Count it as done now.
    pub fn mark(&mut self) {
        self.last = Some(Instant::now());
    }
    /// Make the next [`Every::due`] true.
    pub fn reset(&mut self) {
        self.last = None;
    }
    /// Time since it last ran.
    pub fn age(&self) -> Option<Duration> {
        self.last.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_is_due_first_then_waits() {
        let mut e = Every::default();
        assert!(e.due(Duration::from_secs(60)));
        assert!(!e.due(Duration::from_secs(60)));
        assert!(e.due(Duration::ZERO));
        e.reset();
        assert!(e.due(Duration::from_secs(60)));
    }

    #[test]
    fn nap_ends_on_kick_and_times_out() {
        static K: Kick = Kick::new();
        let t = std::thread::spawn(|| nap(Duration::from_secs(20), Some(&K)));
        std::thread::sleep(Duration::from_millis(50));
        K.kick();
        assert_eq!(t.join().unwrap(), Wake::Kicked);
        let t0 = Instant::now();
        assert_eq!(nap(Duration::from_millis(30), Some(&K)), Wake::Timeout);
        assert!(t0.elapsed() >= Duration::from_millis(30));
    }
}
