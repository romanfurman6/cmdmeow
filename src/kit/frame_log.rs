//! Dev builds only: a line in the dev log, with the time to the
//! millisecond, for whatever could make the app look frozen while it isn't
//! (2026-11-16: clicks in the sidebar worked, but the window kept showing
//! the old page for 5–10 s). Each line starts with `[frame] HH:MM:SS.mmm`:
//! - a pass of the app longer than [`SLOW_PASS`], with the share of it
//!   spent painting (the app window is painted inside the root pass);
//! - from eframe: a paint longer than 100 ms, a window covered or
//!   uncovered (occluded), a window gaining or losing focus;
//! - the page shown in the app window changing;
//! - [`note`]: moments worth lining up with those (the Dock handoff).
//!
//! Release builds pay one bool check per pass.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A pass longer than this gets a line.
const SLOW_PASS: Duration = Duration::from_millis(250);

static ON: AtomicBool = AtomicBool::new(false);
static PASS: Mutex<Option<Instant>> = Mutex::new(None);
static PAGE: Mutex<Option<&'static str>> = Mutex::new(None);

/// Call once at launch: on in dev builds.
pub fn init() {
    if crate::kit::fs::is_dev() {
        ON.store(true, Ordering::Relaxed);
        eframe::set_trace(write);
    }
}

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn write(msg: &str) {
    eprintln!("{}", line(&chrono::Local::now().format("%H:%M:%S%.3f").to_string(), msg));
}

fn line(time: &str, msg: &str) -> String {
    format!("[frame] {time} {msg}")
}

/// A moment worth seeing next to the frames (dev builds only).
pub fn note(msg: impl FnOnce() -> String) {
    if on() {
        write(&msg());
    }
}

/// Call first thing in the root pass.
pub fn begin() {
    if on() {
        *PASS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        eframe::take_paint_micros();
    }
}

/// Call last thing in the root pass, with the page the app window shows
/// (None while it's closed).
pub fn end(page: Option<&'static str>) {
    if !on() {
        return;
    }
    {
        let mut last = PAGE.lock().unwrap_or_else(|e| e.into_inner());
        if *last != page {
            *last = page;
            write(&format!("page {}", page.unwrap_or("(window closed)")));
        }
    }
    let Some(t) = PASS.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let d = t.elapsed();
    if let Some(msg) = slow_pass(d, Duration::from_micros(eframe::take_paint_micros())) {
        write(&msg);
    }
}

/// The line for a pass that took `pass`, `paint` of it painting; None when
/// it was quick.
fn slow_pass(pass: Duration, paint: Duration) -> Option<String> {
    (pass >= SLOW_PASS).then(|| format!("slow pass {} ms (painting {} ms, app code {} ms)", pass.as_millis(), paint.as_millis(), pass.saturating_sub(paint).as_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_pass_says_nothing() {
        assert_eq!(slow_pass(Duration::from_millis(249), Duration::ZERO), None);
    }

    #[test]
    fn a_slow_pass_splits_painting_from_app_code() {
        assert_eq!(slow_pass(Duration::from_millis(900), Duration::from_millis(850)).as_deref(), Some("slow pass 900 ms (painting 850 ms, app code 50 ms)"));
    }

    #[test]
    fn lines_carry_their_tag_and_time() {
        assert_eq!(line("15:38:46.753", "page Cleanup"), "[frame] 15:38:46.753 page Cleanup");
    }
}
