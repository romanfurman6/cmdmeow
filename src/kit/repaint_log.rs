//! `CMDMEOW_REPAINT_LOG=1`: every few seconds, print how many frames the
//! main window drew, how long the app's own code took per frame, and which
//! lines asked for those frames. Finds what keeps an idle app redrawing.
//! Off (and free) unless the variable is set.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EVERY: Duration = Duration::from_secs(3);

struct Log {
    since: Instant,
    frames: u32,
    busy: Duration,
    started: Option<Instant>,
    causes: HashMap<String, u32>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CMDMEOW_REPAINT_LOG").is_some())
}

static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Call first thing in `main`: launch time is counted from here.
pub fn mark_start() {
    STARTED.get_or_init(Instant::now);
}

/// Call first thing in the root pass.
pub fn begin(ctx: &egui::Context) {
    if !on() {
        return;
    }
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| {
        if let Some(t) = STARTED.get() {
            eprintln!("[timing] first pass {} ms after launch", t.elapsed().as_millis());
        }
    });
    let causes = ctx.repaint_causes();
    let mut g = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let log = g.get_or_insert_with(|| Log { since: Instant::now(), frames: 0, busy: Duration::ZERO, started: None, causes: HashMap::new() });
    log.frames += 1;
    log.started = Some(Instant::now());
    for c in causes {
        // one count per line per frame: how many frames this line asked for
        let key = format!("{}:{}", c.file, c.line);
        *log.causes.entry(key).or_default() += 1;
    }
}

/// Call last thing in the root pass.
pub fn end() {
    if !on() {
        return;
    }
    let mut g = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let Some(log) = g.as_mut() else { return };
    if let Some(s) = log.started.take() {
        log.busy += s.elapsed();
    }
    let span = log.since.elapsed();
    if span < EVERY {
        return;
    }
    let fps = log.frames as f64 / span.as_secs_f64();
    let per = if log.frames > 0 { log.busy.as_secs_f64() * 1000.0 / log.frames as f64 } else { 0.0 };
    let mut top: Vec<_> = log.causes.drain().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    let list: Vec<String> = top.iter().take(8).map(|(k, n)| format!("{k}×{n}")).collect();
    // GPU frames over all windows: painted, and skipped as unchanged
    let (painted, same) = eframe::take_paint_counts();
    let secs = span.as_secs_f64();
    eprintln!(
        "[repaint] {fps:.1} fps, app code {per:.2} ms/frame, painted {:.1}/s, unchanged {:.1}/s | {}",
        painted as f64 / secs,
        same as f64 / secs,
        list.join("  ")
    );
    log.since = Instant::now();
    log.frames = 0;
    log.busy = Duration::ZERO;
}
