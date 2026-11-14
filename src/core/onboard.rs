//! First-run setup: pick tools, allow what they need from macOS (live
//! status), crash reports (official builds only), open at login, then hand
//! over to the menu bar panel. Also the permission list in Settings › General.

use crate::kit::sync::LockExt;
use super::shell::App;
use crate::kit::glass::*;
use crate::kit::perms::{self, Perm, Status};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::collections::HashMap;

pub const SETUP_W: f32 = 820.0;
/// Most steps setup can have (with the crash reports step).
const MAX_STEPS: usize = 7;
const TOP_H: f32 = 44.0;
const BOTTOM_H: f32 = 64.0;
/// The centred column of the AI agents and Download models steps.
const COL_W: f32 = 640.0;

/// What a setup step shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Welcome,
    Tools,
    Agents,
    /// 3a, right after Agents (no dot of its own): the picked agents whose
    /// sign-in is in the Keychain, each asked for on a click.
    SignIns,
    Models,
    Perms,
    Crash,
    Ready,
}

/// The steps in order. The crash reports question only exists in official
/// builds (screenshots show it too).
fn kinds(app: &App) -> Vec<Kind> {
    let mut v = vec![Kind::Welcome, Kind::Tools, Kind::Agents];
    if has_sign_ins(app) {
        v.push(Kind::SignIns);
    }
    // only when an enabled tool needs a download (Dictation's speech model)
    // or already has one; models fetched on first use don't count
    if has_models_step(app) {
        v.push(Kind::Models);
    }
    v.push(Kind::Perms);
    if app.demo || crate::kit::crash_report::available() {
        v.push(Kind::Crash);
    }
    v.push(Kind::Ready);
    v
}

/// The step a screenshot view (`setup-perms`…) shows.
pub fn view_step(app: &App, view: &str) -> usize {
    let k = match view {
        "setup-tools" | "setup-tools-all" | "setup-tools-preview" => Kind::Tools,
        "setup-ai" => Kind::Agents,
        "setup-signins" => Kind::SignIns,
        "setup-models" => Kind::Models,
        "setup-perms" => Kind::Perms,
        "setup-crash" => Kind::Crash,
        "setup-ready" => Kind::Ready,
        _ => Kind::Welcome,
    };
    kinds(app).iter().position(|x| *x == k).unwrap_or(0)
}

/// The setup window's state while it is open.
pub struct Setup {
    pub step: usize,
    /// Seconds the current step has been on screen (its animations count
    /// from here). It only moves while the window is in front and drawing,
    /// at most a frame's worth at a time: a slow first frame or a macOS
    /// prompt over the window no longer skips the animation.
    clock: f32,
    /// When the clock last moved (NaN before the step's first frame).
    last: f64,
    /// The step before this one: the cat on the step dots hops from there.
    from: usize,
    /// The Tools step shows every tool with a switch, not the goals.
    pub all_tools: bool,
    /// The tools again, with sample data, for the Tools step's panel: the
    /// live ones would read the person's folders (Capture's Recent list
    /// lists ~/Desktop, and macOS asked about it on a hover).
    samples: Option<Vec<Box<dyn super::tool::Tool>>>,
}

impl Setup {
    pub fn new(step: usize) -> Self {
        Setup { step, clock: 0.0, last: f64::NAN, from: step, all_tools: false, samples: None }
    }
}

/// Marker file: setup was finished or closed once.
pub fn done_marker() -> std::path::PathBuf {
    crate::kit::fs::data_path("setup-done")
}

fn step_file() -> std::path::PathBuf {
    crate::kit::fs::data_path("setup-step")
}

/// The step setup was on when the app last quit: some permissions make macOS
/// restart the app, and setup picks up where it was.
pub fn saved_step() -> usize {
    std::fs::read_to_string(step_file()).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0).min(MAX_STEPS - 1)
}

/// Setup was finished (closing the window no longer counts).
pub fn is_done() -> bool {
    // asked every frame (menu bar title): once setup is done it stays done,
    // so stop looking at the disk
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let done = done_marker().exists();
    if done {
        DONE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    done
}

pub fn mark_done() {
    let _ = std::fs::remove_file(step_file());
    let m = done_marker();
    if let Some(d) = m.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(m, b"");
}

/// Live permission state, polled about once a second while shown.
#[derive(Default)]
pub struct PermWatch {
    status: HashMap<Perm, Status>,
    polled: f64,
    /// When a permission's button was pressed (spinner until it flips).
    asked: HashMap<Perm, f64>,
    /// Last error of a tool's own setup step (shown on its row).
    own_err: HashMap<Perm, String>,
    /// When a permission turned Allowed while we watched (check pops).
    allowed_at: HashMap<Perm, f64>,
    /// Sample statuses for screenshots.
    pub demo: Option<HashMap<Perm, Status>>,
    /// Open at login, as last read (None until the first read lands).
    login: Option<bool>,
    /// A macOS question was just answered: bring setup back in front (macOS
    /// handed focus to whatever was behind, often System Settings left open
    /// by an earlier row).
    refocus: bool,
    /// macOS answers slowly enough to drop frames, so reads run off the ui thread.
    bg: std::sync::Arc<std::sync::Mutex<Bg>>,
}

#[derive(Default)]
struct Bg {
    running: bool,
    result: Option<(Vec<(Perm, Status)>, bool)>,
}

/// One row: a permission and the enabled tools that want it.
pub struct Row {
    pub perm: Perm,
    pub optional: bool,
    /// (tool index) of the enabled tools that need it.
    pub tools: Vec<usize>,
    /// The bar on a locked page: the row says what is missing ("Needs Full
    /// Disk Access") instead of naming the permission and why it's asked.
    pub locked: bool,
}

/// What the enabled tools need, required rows first, in a stable order.
pub fn rows(app: &App) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for i in 0..app.tools.len() {
        if !app.tool_on(i) {
            continue;
        }
        for n in app.tools[i].needs() {
            match out.iter_mut().find(|r| r.perm == n.perm) {
                Some(r) => {
                    r.optional &= n.optional;
                    r.tools.push(i);
                }
                None => out.push(Row { perm: n.perm, optional: n.optional, tools: vec![i], locked: false }),
            }
        }
    }
    let rank = |p: Perm| Perm::SYSTEM.iter().position(|s| *s == p).unwrap_or(Perm::SYSTEM.len());
    out.sort_by_key(|r| (r.optional, rank(r.perm)));
    out
}

impl PermWatch {
    /// Start a background read of `perms` and Open at login (at most once a second).
    fn kick(&mut self, perms: Vec<Perm>, now: f64) {
        if now - self.polled < 1.0 && self.polled > 0.0 {
            return;
        }
        {
            let mut b = self.bg.lock_ok();
            if b.running {
                return;
            }
            b.running = true;
        }
        self.polled = now;
        let bg = self.bg.clone();
        std::thread::spawn(move || {
            let v: Vec<(Perm, Status)> = perms.into_iter().map(|p| (p, p.status())).collect();
            let login = crate::kit::login::enabled();
            let mut b = bg.lock_ok();
            b.result = Some((v, login));
            b.running = false;
        });
    }

    fn apply(&mut self, perm: Perm, s: Status, now: f64) {
        let before = self.status.insert(perm, s);
        if s == Status::Allowed && before.is_some_and(|b| b != Status::Allowed) {
            self.allowed_at.insert(perm, now);
        }
        if perm.answered_in_place() && self.asked.contains_key(&perm) && s != Status::NotAsked && before != Some(s) {
            self.refocus = true;
        }
        // allowed, or refused a while after asking: the wait is over
        if s == Status::Allowed || (s == Status::Denied && self.asked.get(&perm).is_some_and(|t| now - t > 3.0)) {
            self.asked.remove(&perm);
        }
    }

    /// Refresh statuses for `rows`: system ones in the background, at most
    /// once a second; a tool's own ones here (they're cheap).
    pub fn poll(&mut self, app_tools: &[Box<dyn super::tool::Tool>], rows: &[Row], now: f64) {
        if let Some(d) = self.demo.clone() {
            for r in rows {
                self.apply(r.perm, d.get(&r.perm).copied().unwrap_or(Status::NotAsked), now);
            }
            return;
        }
        let landed = self.bg.lock_ok().result.take();
        if let Some((v, login)) = landed {
            self.login = Some(login);
            for (p, st) in v {
                self.apply(p, st, now);
            }
        }
        let due = now - self.polled >= 1.0 || self.polled == 0.0;
        if due {
            for r in rows {
                if let Perm::Own { id, .. } = r.perm {
                    if let Some(&i) = r.tools.first() {
                        let st = app_tools[i].own_status(id);
                        self.apply(r.perm, st, now);
                        // the tool says when its setup is over: stop the
                        // spinner then (cancelled or failed), not after a minute
                        if let Some((busy, err)) = app_tools[i].own_progress(id) {
                            if !busy {
                                self.asked.remove(&r.perm);
                            }
                            self.own_err.insert(r.perm, err);
                        }
                    }
                }
            }
        }
        let sys: Vec<Perm> = rows.iter().map(|r| r.perm).filter(|p| !matches!(p, Perm::Own { .. })).collect();
        self.kick(sys, now);
    }

    /// Open at login, read in the background (macOS is slow to answer).
    pub fn login_on(&mut self, demo: bool, now: f64) -> bool {
        if demo {
            return true;
        }
        if let Some((_, login)) = self.bg.lock_ok().result.as_ref() {
            self.login = Some(*login);
        }
        self.kick(Vec::new(), now);
        self.login.unwrap_or(false)
    }

    /// Turn Open at login on or off, and show it at once.
    pub fn set_login(&mut self, on: bool) {
        crate::kit::login::settle();
        match crate::kit::login::set(on) {
            Ok(()) => self.login = Some(on),
            Err(e) => eprintln!("[login] {e}"),
        }
    }

    pub fn status(&self, p: Perm) -> Status {
        self.status.get(&p).copied().unwrap_or(Status::NotAsked)
    }

    /// The status once a read has landed (None before that).
    pub fn known(&self, p: Perm) -> Option<Status> {
        self.status.get(&p).copied()
    }

    /// A macOS question was answered since the last call (see `refocus`).
    pub fn take_refocus(&mut self) -> bool {
        std::mem::take(&mut self.refocus)
    }

    /// Ask for a system permission (the spinner shows until it flips).
    pub fn ask(&mut self, p: Perm, now: f64) {
        self.asked.insert(p, now);
        self.polled = 0.0;
        if self.demo.is_none() {
            p.ask();
        }
    }

    pub fn waiting(&self, p: Perm, now: f64) -> bool {
        self.asked.get(&p).is_some_and(|t| now - t < 60.0) && self.status(p) != Status::Allowed
    }

    /// A row's tick is popping in: every frame for its moment.
    fn busy(&self, now: f64) -> bool {
        self.allowed_at.values().any(|t| now - t < 0.9)
    }

    /// Some row waits for its answer (its spinner turns): frames at the
    /// decorative pace, not every frame. An ask left unanswered stops
    /// counting with [`Self::waiting`], so it can't keep the app drawing.
    fn spinning(&self, now: f64) -> bool {
        self.asked.keys().any(|&p| self.waiting(p, now))
    }
}

/// Screen Recording was allowed after launch: capture works after a restart
/// (once: a restarted copy that still doesn't have it won't get it from another).
pub fn needs_restart(app: &App) -> bool {
    app.perm_watch.demo.is_none() && app.perm_watch.status(Perm::Screen) == Status::Allowed && !perms::screen_at_launch() && !perms::relaunched() && rows(app).iter().any(|r| r.perm == Perm::Screen)
}

// ---------------------------------------------------------------- drawing

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// 0 → 1 with a small overshoot (a pop).
fn pop(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = 1.7;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

/// The mascot's colour: lime on dark, deep green on light (lime on the pale
/// light backdrop is too faint).
fn cat_color() -> Color32 {
    if is_light() { ACCENT() } else { BLUE() }
}

/// An ear flick starting at `at` (seconds): two quick outward tilts, in radians.
fn twitch(t: f32, at: f32) -> f32 {
    let u = (t - at) / 0.36;
    if !(0.0..1.0).contains(&u) {
        return 0.0;
    }
    (u * std::f32::consts::TAU).sin().abs() * 0.28 * (1.0 - 0.4 * u)
}

/// How long the cat's hop from one step dot to the next takes.
const HOP: f32 = 0.32;

/// The step dots along the top. The cat sits on the current step's dot and
/// hops there from the step before; a dot lights up when the cat lands.
fn step_dots(p: &egui::Painter, full: Rect, steps: usize, from: usize, step: usize, t: f32) {
    const D: f32 = 6.0;
    const GAP: f32 = 22.0;
    let total = steps as f32 * D + (steps as f32 - 1.0) * GAP;
    let x = |i: usize| full.center().x - total / 2.0 + D / 2.0 + i as f32 * (D + GAP);
    let y = full.top() + TOP_H / 2.0 + 9.0;
    let k = (t / HOP).clamp(0.0, 1.0);
    let moving = from != step;
    for i in 0..steps {
        let lit = i < step || (i == step && (!moving || k >= 1.0)) || (i <= from && i <= step);
        p.circle_filled(Pos2::new(x(i), y), D / 2.0, if lit { BLUE() } else { white(46) });
    }
    // the Welcome step has the big cat: back there, the small one hops
    // onto its dot and fades away
    let fade = if step == 0 { 1.0 - ((t - HOP) / 0.2).clamp(0.0, 1.0) } else { 1.0 };
    if fade <= 0.0 || (step == 0 && !moving) {
        return;
    }
    let m = k * k * (3.0 - 2.0 * k);
    let cx = egui::lerp(x(from)..=x(step), m);
    let jump = if moving { (k * std::f32::consts::PI).sin() * 8.0 } else { 0.0 };
    // a squash as it lands
    let l = ((t - HOP) / 0.2).clamp(0.0, 1.0);
    let sq = if moving && l > 0.0 && l < 1.0 { (l * std::f32::consts::PI).sin() * 0.18 } else { 0.0 };
    let size = 16.0;
    let (w, h) = (size * (1.0 + sq), size * (1.0 - sq));
    let bottom = y - D / 2.0 - 3.0 - jump;
    let r = Rect::from_min_max(Pos2::new(cx - w / 2.0, bottom - h), Pos2::new(cx + w / 2.0, bottom));
    crate::kit::brand::draw_mascot(p, r, cat_color().gamma_multiply(fade), (0.0, twitch(t, HOP + 0.15)));
}

/// The whole setup screen in `full` (the app window, or the root in screenshots).
/// Says whether the person finished it.
pub fn setup_ui(app: &mut App, ui: &mut Ui, full: Rect) -> Finish {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let p = ui.painter().clone();
    let Some(step) = app.setup.as_ref().map(|s| s.step) else { return Finish::No };
    let kinds = kinds(app);
    let steps = kinds.len();
    let step = step.min(steps - 1);
    let kind = kinds[step];
    // `|` not `||`: both flags are read once and must both be cleared
    let answered = Perm::take_answered();
    if (app.perm_watch.take_refocus() | answered) && !app.demo {
        app.perm_watch.polled = 0.0;
        crate::kit::native::activate();
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    // a step's animations run while it can be seen: not while the window
    // is hidden, covered or behind a macOS prompt, nor during its first
    // frames (macOS shows them late), else they play to nobody
    let seen = ctx.input(|i| i.viewport().occluded != Some(true) && i.viewport().minimized != Some(true) && i.viewport().focused != Some(false)) && (app.demo || super::shell::first_frames_shown());
    let mut t = 0.0;
    if let Some(s) = app.setup.as_mut() {
        let dt = if s.last.is_nan() { 0.0 } else { (now - s.last).clamp(0.0, 0.05) as f32 };
        s.last = now;
        if seen {
            s.clock += dt;
        }
        t = s.clock;
    }
    // screenshots show each step settled, not mid-animation
    // (CMDMEOW_SETUP_T=<s> renders a moment of the animation instead)
    let t = if app.demo { std::env::var("CMDMEOW_SETUP_T").ok().and_then(|v| v.parse().ok()).unwrap_or(t + 5.0) } else { t };
    // setup lives in the app window: a centered column when that is wider
    let win = full;
    // and as tall as the window it was made for: a bigger window gets it
    // in the middle, not pinned to the top with a gap above the buttons
    let full = Rect::from_center_size(win.center(), Vec2::new(win.width().min(SETUP_W + 80.0), win.height().min(crate::kit::mechanics::APP_HEIGHT)));
    // drag the window by its top strip
    let drag = Rect::from_min_size(win.min + Vec2::new(80.0, 0.0), Vec2::new(win.width() - 80.0, TOP_H));
    if ui.interact(drag, ui.scope_id().with("setup-drag"), Sense::click_and_drag()).drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let from = app.setup.as_ref().map(|s| s.from).unwrap_or(step).min(steps - 1);
    // Sign-ins is 3a: it shares the Agents dot
    let dot = |i: usize| kinds[..=i].iter().filter(|k| **k != Kind::SignIns).count() - 1;
    let dots = kinds.iter().filter(|k| **k != Kind::SignIns).count();
    step_dots(&p, full, dots, dot(from), dot(step), t);
    let body = Rect::from_min_max(Pos2::new(full.left() + 40.0, full.top() + TOP_H), Pos2::new(full.right() - 40.0, full.bottom() - BOTTOM_H));
    // the step slides in from the side it comes from
    let side = if from > step { -1.0 } else { 1.0 };
    let slide = (1.0 - ease(t * 2.2)) * 18.0 * side;
    // Back and Continue line up with the step's own column
    let col = match kind {
        Kind::Agents | Kind::SignIns | Kind::Models => Rect::from_center_size(body.center(), Vec2::new(body.width().min(COL_W), body.height())),
        _ => body,
    };
    let body = body.translate(Vec2::new(slide, 0.0));
    let mut next = false;
    let mut restart_now = false;
    match kind {
        Kind::Welcome => next = welcome(app, ui, body, t),
        Kind::Tools => tools(app, ui, body, t),
        Kind::Agents => agents(app, ui, body, t),
        Kind::SignIns => sign_ins(app, ui, body, t),
        Kind::Models => models_step(app, ui, body, t),
        Kind::Perms => permissions_step(app, ui, body, t, now),
        Kind::Crash => next = crash_step(app, ui, body, t),
        Kind::Ready => restart_now = ready(app, ui, body, t),
    }
    // bottom bar
    let bar = Rect::from_min_max(Pos2::new(col.left(), full.bottom() - BOTTOM_H), Pos2::new(col.right(), full.bottom()));
    let mut finish = if restart_now { Finish::Restart } else { Finish::No };
    if step > 0 {
        let r = Rect::from_min_size(Pos2::new(bar.left(), bar.center().y - 16.0), Vec2::new(70.0, 32.0));
        if ghost_button(ui, r, "Back").clicked() {
            go(app, step - 1);
        }
    }
    let label = match kind {
        // these steps have their own buttons
        Kind::Welcome | Kind::Crash => None,
        Kind::Tools | Kind::Agents | Kind::SignIns | Kind::Models | Kind::Perms => Some("Continue"),
        Kind::Ready if needs_restart(app) => Some("Restart and open"),
        Kind::Ready => Some("Open panel"),
    };
    if let Some(label) = label {
        let w = button_width(&p, label, BTN_PRIMARY).max(110.0);
        let r = Rect::from_min_size(Pos2::new(bar.right() - w, bar.center().y - 16.0), Vec2::new(w, 32.0));
        // nothing to set up with no tool on
        let can = kind != Kind::Tools || (0..app.tools.len()).any(|i| app.tool_on(i));
        if pill_button(ui, r, label, BTN_PRIMARY, can).clicked() && can {
            if kind == Kind::Ready {
                finish = if needs_restart(app) { Finish::Restart } else { Finish::Panel };
            } else {
                next = true;
            }
        }
        if kind == Kind::Perms && !all_required(app) {
            let r2 = Rect::from_min_size(Pos2::new(r.left() - 80.0, r.top()), Vec2::new(70.0, 32.0));
            if ghost_button(ui, r2, "Later").clicked() {
                next = true;
            }
        }
    }
    if next {
        go(app, step + 1);
    }
    // setup is an immediate window drawn in the (hidden) root's pass: ask
    // the root for the frames too, or after a click the window kept showing
    // the old step until it was resized (first launch, M1)
    let busy_for = if kind == Kind::Welcome { WELCOME_END } else { 2.5 };
    let animating = (seen && t < busy_for) || app.perm_watch.busy(now) || (kind == Kind::SignIns && crate::tools::claude::keychain::busy()) || next;
    if !seen && t < busy_for {
        // waiting to be seen: look again soon, without drawing flat out
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        ctx.request_repaint_after_for(std::time::Duration::from_millis(100), egui::ViewportId::ROOT);
    } else if animating {
        ctx.request_repaint();
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    } else if app.perm_watch.spinning(now) {
        let beat = std::time::Duration::from_millis(33);
        ctx.request_repaint_after(beat);
        ctx.request_repaint_after_for(beat, egui::ViewportId::ROOT);
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        ctx.request_repaint_after_for(std::time::Duration::from_millis(1000), egui::ViewportId::ROOT);
    }
    finish
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finish {
    No,
    Panel,
    Restart,
}

fn go(app: &mut App, step: usize) {
    let last = kinds(app).len() - 1;
    if let Some(s) = app.setup.as_mut() {
        s.from = s.step;
        s.step = step.min(last);
        s.clock = 0.0;
        s.last = f64::NAN;
        if !app.demo {
            let f = step_file();
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, s.step.to_string());
        }
    }
}

fn all_required(app: &App) -> bool {
    rows(app).iter().filter(|r| !r.optional).all(|r| app.perm_watch.status(r.perm) == Status::Allowed)
}

fn ghost_button(ui: &mut Ui, r: Rect, label: &str) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(("ghost", label)), Sense::click());
    let col = if resp.hovered() { TEXT() } else { TEXT_DIM() };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font(13.0), col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The app's mark: a dark squircle with the Cmd+Meow glyph, fading in with `sweep`.
fn app_mark(p: &egui::Painter, c: Pos2, size: f32, sweep: f32) {
    let r = Rect::from_center_size(c, Vec2::splat(size));
    p.rect_filled(r, size * 0.225, Color32::from_rgb(0x22, 0x22, 0x26));
    p.rect_stroke(r, size * 0.225, Stroke::new(1.0, white(36)), StrokeKind::Inside);
    let a = (0.35 + 0.65 * sweep.clamp(0.0, 1.0)) * 255.0;
    brand_glyph(p, c, size * 0.6, Color32::from_white_alpha(a as u8));
}

/// The Cmd+Meow glyph (as in the menu bar), tinted `col`.
fn brand_glyph(p: &egui::Painter, c: Pos2, size: f32, col: Color32) {
    let id = egui::Id::NULL.with("cmdmeow-brand-glyph");
    let tex = p.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let n = 128;
        let mut px = crate::kit::brand::glyph_rgba(n);
        for c in px.chunks_mut(4) {
            c[0] = 255;
            c[1] = 255;
            c[2] = 255;
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([n as usize, n as usize], &px);
        let t = p.ctx().load_texture("cmdmeow-brand-glyph", img, egui::TextureOptions::LINEAR);
        p.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    p.image(tex.id(), Rect::from_center_size(c, Vec2::splat(size)), uv, col);
}

fn welcome(app: &mut App, ui: &mut Ui, body: Rect, t: f32) -> bool {
    let p = ui.painter().clone();
    // the ring, the name and the button fill the step, as one block in its middle
    let ring = 150.0;
    let chip = 21.0;
    let title = 36.0;
    let button = Vec2::new(168.0, 46.0);
    let block = 2.0 * (ring + chip) + 40.0 + title + sp::S + 18.0 + 36.0 + button.y;
    let c = Pos2::new(body.center().x, body.center().y - block / 2.0 + chip + ring);
    // the cat peeks out ears first, flicks them, then hops up; the ring
    // draws itself round it and the tools land on it one by one
    crate::kit::glass::stroke_circle(&p, c, ring, Stroke::new(2.0, white(14)));
    let start = -std::f32::consts::FRAC_PI_2;
    // the ring and the tools come once the cat is up
    let v = t - WELCOME_WAIT - WELCOME_SHAKE;
    let k = ease((v - 1.15) / 0.9);
    if k > 0.0 {
        p.add(egui::Shape::line(arc_points(c, ring, start, start + std::f32::consts::TAU * k, 128), Stroke::new(2.5, BLUE())));
    }
    let n = app.tools.len().max(1);
    for (i, tool) in app.tools.iter().enumerate() {
        let a = start + i as f32 / n as f32 * std::f32::consts::TAU;
        let at = c + Vec2::new(a.cos(), a.sin()) * ring;
        let s = pop((v - 1.25 - 0.07 * i as f32) / 0.4);
        if s <= 0.0 {
            continue;
        }
        p.circle_filled(at, chip * s, BG());
        crate::kit::glass::stroke_circle(&p, at, chip * s, Stroke::new(1.0, white(36)));
        icon(&p, tool.badge().icon, Rect::from_center_size(at, Vec2::splat(19.0 * s)), TEXT_SOFT());
    }
    let size = 184.0;
    // peek() is laid out for a 96 pt cat
    let ((rise, sx, sy), ears) = welcome_cat(t);
    let rise = rise * size / 96.0;
    let floor = c.y + size / 2.0;
    let (w, h) = (size * sx, size * sy);
    let cat = Rect::from_min_max(Pos2::new(c.x - w / 2.0, floor - rise - h), Pos2::new(c.x + w / 2.0, floor - rise));
    // below the floor the cat is hidden: that's where it peeks out from
    let clip = Rect::from_min_max(Pos2::new(c.x - size * 0.7, c.y - size), Pos2::new(c.x + size * 0.7, floor));
    crate::kit::brand::draw_mascot(&p.with_clip_rect(clip.intersect(p.clip_rect())), cat, cat_color(), ears);
    let name = p.text(Pos2::new(c.x, c.y + ring + chip + 40.0), Align2::CENTER_TOP, "Cmd+Meow", bold(title), TEXT());
    let line = p.text(Pos2::new(c.x, name.bottom() + sp::S), Align2::CENTER_TOP, format!("{} tools. One menu bar.", app.tools.len()), font(16.0), TEXT_FAINT());
    let r = Rect::from_min_size(Pos2::new(c.x - button.x / 2.0, line.bottom() + 36.0), button);
    pill_button(ui, r, "Set up", BTN_PRIMARY, true).clicked()
}

/// Welcome waits this long after the window is on screen before anything moves.
const WELCOME_WAIT: f32 = 0.6;
/// How long only the ears show, shaking, before the cat jumps out.
const WELCOME_SHAKE: f32 = 0.45;
/// When the Welcome animation is over (the last tool has landed).
const WELCOME_END: f32 = WELCOME_WAIT + WELCOME_SHAKE + 2.6;

/// The Welcome cat at `t`: a still moment, then its ears come up over the
/// floor and shake, then it jumps out ([`peek`] with the ears' moment
/// stretched); and the ears' tilt (left, right).
fn welcome_cat(t: f32) -> ((f32, f32, f32), (f32, f32)) {
    let u = t - WELCOME_WAIT;
    // peek() holds still from 0.45 s to 0.85 s with just the ears up
    let pose = if u < 0.45 { peek(u) } else if u < 0.85 + WELCOME_SHAKE { peek(0.6) } else { peek(u - WELCOME_SHAKE) };
    let shake = |a: &[f32]| a.iter().map(|&at| twitch(u, at)).sum::<f32>();
    let after = WELCOME_SHAKE;
    let ears = (shake(&[0.42, 0.78, 1.12, 2.3 + after]), shake(&[0.6, 0.95, 1.55 + after]));
    (pose, ears)
}

/// The Welcome cat at `t`: how far it is lifted from where it rests (0 =
/// resting, negative = still below the floor) and its squash (width, height).
fn peek(t: f32) -> (f32, f32, f32) {
    let lerp = |a: f32, b: f32, x: f32| a + (b - a) * x.clamp(0.0, 1.0);
    match t {
        // hidden, then the ears come up over the floor
        t if t < 0.45 => (lerp(-100.0, -72.0, ease((t - 0.1) / 0.35)), 1.0, 1.0),
        // looking round
        t if t < 0.85 => (-72.0, 1.0, 1.0),
        // crouch
        t if t < 0.97 => {
            let x = ease((t - 0.85) / 0.12);
            (lerp(-72.0, -80.0, x), 1.0 + 0.04 * x, 1.0 - 0.08 * x)
        }
        // jump, stretched
        t if t < 1.2 => {
            let x = ease((t - 0.97) / 0.23);
            (lerp(-80.0, 14.0, x), lerp(1.04, 0.96, x), lerp(0.92, 1.05, x))
        }
        // fall back and land with a squash
        t if t < 1.36 => {
            let x = (t - 1.2) / 0.16;
            (lerp(14.0, 0.0, x * x), 1.0, 1.0)
        }
        t => {
            let s = pop((t - 1.36) / 0.3);
            let sq = if s < 1.0 { (1.0 - s) * 0.14 } else { 0.0 };
            (0.0, 1.0 + sq, 1.0 - sq)
        }
    }
}

/// Crash reports, asked once (official builds only). Returns true when the
/// person picked an answer: setup moves on.
fn crash_step(app: &mut App, ui: &mut Ui, body: Rect, t: f32) -> bool {
    let p = ui.painter().clone();
    let c = Pos2::new(body.center().x, body.top() + 96.0);
    let z = pop(t / 0.5).max(0.0);
    if z > 0.0 {
        let mark = Rect::from_center_size(c, Vec2::splat(80.0 * z));
        p.rect_filled(mark, 18.0 * z, CARD());
        p.rect_stroke(mark, 18.0 * z, Stroke::new(1.0, white(24)), StrokeKind::Inside);
        icon(&p, Icon::Shield, mark.shrink(20.0 * z), BLUE());
    }
    let q = p.text(Pos2::new(c.x, c.y + 62.0), Align2::CENTER_TOP, "Send crash reports?", bold(22.0), TEXT());
    p.text(Pos2::new(c.x, q.bottom() + sp::LINE_GAP), Align2::CENTER_TOP, "They help fix bugs faster. Your personal data stays on your Mac.", font(13.0), TEXT_FAINT());
    // what a report has, and what it never has
    let facts: [(bool, &str); 3] = [(true, "What went wrong and where in the app"), (true, "App and macOS version"), (false, "Files, names or IP address")];
    let w = 400.0;
    let mut y = c.y + 134.0;
    for (k, (sent, label)) in facts.iter().enumerate() {
        let a = ease((t - 0.15 - 0.08 * k as f32) / 0.4);
        let row = Rect::from_min_size(Pos2::new(c.x - w / 2.0, y - (1.0 - a) * 8.0), Vec2::new(w, 40.0));
        p.rect_filled(row, 10.0, CARD().gamma_multiply(a));
        let m = Pos2::new(row.left() + 22.0, row.center().y);
        if *sent {
            check(&p, m, 12.0, GREEN().gamma_multiply(a));
        } else {
            icon(&p, Icon::Close, Rect::from_center_size(m, Vec2::splat(12.0)), TEXT_MUTED().gamma_multiply(a));
        }
        let col = if *sent { TEXT() } else { TEXT_MUTED() };
        p.text(Pos2::new(row.left() + 44.0, row.center().y), Align2::LEFT_CENTER, *label, font(13.0), col.gamma_multiply(a));
        y += 48.0;
    }
    // the answer
    let yes = "Send reports";
    let bw = button_width(&p, yes, BTN_PRIMARY).max(140.0);
    let no_w = 96.0;
    let gap = 12.0;
    let left = c.x - (bw + gap + no_w) / 2.0;
    let by = y + 14.0;
    let mut picked = None;
    if ghost_button(ui, Rect::from_min_size(Pos2::new(left, by), Vec2::new(no_w, 36.0)), "Don't send").clicked() {
        picked = Some(false);
    }
    if pill_button(ui, Rect::from_min_size(Pos2::new(left + no_w + gap, by), Vec2::new(bw, 36.0)), yes, BTN_PRIMARY, true).clicked() {
        picked = Some(true);
    }
    match picked {
        Some(on) => {
            if !app.demo {
                crate::kit::crash_report::set_consent(on);
            }
            true
        }
        None => false,
    }
}

/// Section order for the tools grid.
fn sections(app: &App) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for want in ["Clean", "Create", "Monitor", "Utilities"] {
        if app.tools.iter().any(|t| t.section() == want) {
            out.push(want);
        }
    }
    for t in &app.tools {
        if !out.contains(&t.section()) {
            out.push(t.section());
        }
    }
    out
}

/// A ready-made set of tools on the Tools step: picking it turns them all on.
pub struct Goal {
    pub name: &'static str,
    pub tools: &'static [&'static str],
}

/// What people come for. Every tool but Fan Control is in one goal; Fan
/// Control (it needs an admin helper) is only in the full list.
pub const GOALS: [Goal; 6] = {
    use crate::tools::*;
    [
        Goal { name: "Clean my Mac", tools: &[cleanup::ID, lens::ID, uninstaller::ID] },
        Goal { name: "Screenshots & video", tools: &[capture::ID, capture::VIDEO_ID, capture::SHARING_ID] },
        Goal { name: "Clipboard", tools: &[clipboard::ID] },
        Goal { name: "Voice typing", tools: &[dictation::ID] },
        Goal { name: "Watch my Mac", tools: &[system::ID, startup::ID, displays::ID] },
        Goal { name: "For developers", tools: &[claude::ID, keep_awake::ID] },
    ]
};

/// Tools on in a fresh install: none. Setup's goals start unpicked and the
/// person picks what they came for (Continue waits for one).
pub const ON_AT_FIRST: [&str; 0] = [];

/// The ids to save as switched off on a fresh install.
pub fn off_at_first<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    ids.filter(|id| !ON_AT_FIRST.contains(id)).map(str::to_string).collect()
}

/// Indexes of a goal's tools that this build has.
fn goal_tools(app: &App, g: &Goal) -> Vec<usize> {
    g.tools.iter().filter_map(|id| app.tool_index(id)).collect()
}

/// 0→1 over `dur` seconds from when `id` was first drawn (again): things
/// that just appeared drop or pop in. Screenshots show them settled.
fn appear(ctx: &egui::Context, id: egui::Id, now: f64, dur: f32, demo: bool) -> f32 {
    if demo {
        return 1.0;
    }
    let (first, last) = ctx.data_mut(|d| d.get_temp::<(f64, f64)>(id)).unwrap_or((now, now));
    // not drawn for a moment: it was gone, so it appears afresh
    let first = if now - last > 0.2 { now } else { first };
    ctx.data_mut(|d| d.insert_temp(id, (first, now)));
    let k = ((now - first) as f32) / dur;
    if k < 1.0 {
        ctx.request_repaint();
    }
    ease(k)
}

const PANEL_W: f32 = 300.0;

/// The preview panel is not drawn smaller than this (text stays readable).
const MIN_PANEL_SCALE: f32 = 0.72;

fn tools(app: &mut App, ui: &mut Ui, body: Rect, t: f32) {
    let p = ui.painter().clone();
    let on = (0..app.tools.len()).filter(|&i| app.tool_on(i)).count();
    let left = Rect::from_min_max(body.min, Pos2::new(body.right() - PANEL_W - 28.0, body.bottom()));
    let head = body.top() + 6.0;
    let g = p.text(Pos2::new(left.left(), head), Align2::LEFT_TOP, "What do you need?", bold(22.0), TEXT());
    // the count rolls to its new number like an odometer
    let shown = ui.ctx().animate_value_with_time(ui.scope_id().with("setup-tools-on"), on as f32, 0.28);
    rolling_count(&p, Pos2::new(g.right() + 10.0, g.bottom() - 4.0), shown, app.tools.len());
    let all = app.setup.as_ref().is_some_and(|s| s.all_tools);
    let seg = Rect::from_min_size(Pos2::new(left.right() - 150.0, g.center().y - sp::CONTROL_H / 2.0), Vec2::new(150.0, sp::CONTROL_H));
    if let Some(k) = segmented(ui, seg, ui.scope_id().with("setup-tools-mode"), &["Goals", "All tools"], all as usize) {
        if let Some(s) = app.setup.as_mut() {
            s.all_tools = k == 1;
        }
    }
    let top = g.bottom() + 20.0;
    let area = Rect::from_min_max(Pos2::new(left.left(), top), left.max);
    if all {
        all_tools(app, ui, area, t);
    } else {
        goal_cards(app, ui, area, t);
    }
    mini_panel(app, ui, Rect::from_min_max(Pos2::new(body.right() - PANEL_W, body.top() + 6.0), body.max), t);
}

/// The goal cards: a click picks a goal (its tools on) or drops it.
fn goal_cards(app: &mut App, ui: &mut Ui, area: Rect, t: f32) {
    let p = ui.painter().clone();
    let gap = sp::M;
    let cw = (area.width() - gap) / 2.0;
    // three rows fill the step
    let ch = ((area.height() - sp::L - gap * 2.0) / 3.0).clamp(96.0, 116.0);
    // a card's loop plays only while the pointer is on it (all six at once
    // was too busy); otherwise it rests where it stopped, first at the pose
    // screenshots show
    let wall = ui.ctx().input(|i| i.time);
    let mut flip = None;
    for (k, goal) in GOALS.iter().enumerate() {
        let idx = goal_tools(app, goal);
        if idx.is_empty() {
            continue;
        }
        let picked = idx.iter().all(|&i| app.tool_on(i));
        let a = ease((t - 0.05 * k as f32) / 0.4);
        let (col, row) = ((k % 2) as f32, (k / 2) as f32);
        let r = Rect::from_min_size(Pos2::new(area.left() + col * (cw + gap), area.top() + row * (ch + gap) + (1.0 - a) * 8.0), Vec2::new(cw, ch));
        let id = ui.scope_id().with(("setup-goal", goal.name));
        let resp = ui.interact(r, id, Sense::click());
        let s = ui.ctx().animate_bool_with_time(id.with("on"), picked, 0.18);
        let hov = ui.ctx().animate_bool(id.with("h"), resp.hovered());
        let r = r.translate(Vec2::new(0.0, -hov));
        let now = if app.demo { 2.0 } else { loop_clock(ui.ctx(), id.with("loop"), wall, resp.hovered()) };
        let fill = lerp_color(lerp_color(CARD(), white(30), hov * 0.15), with_alpha(BLUE(), 0.10), s);
        p.rect_filled(r, 12.0, fill.gamma_multiply(a));
        p.rect_stroke(r, 12.0, Stroke::new(1.0, lerp_color(CARD_EDGE(), with_alpha(BLUE(), 0.55), s).gamma_multiply(a)), StrokeKind::Inside);
        // the goal's tools, as their chips; the check and name sit below,
        // so the space right of the chips is free for the goal's loop
        let inner = r.shrink(14.0);
        let jolt = ui.ctx().animate_bool_with_time(id.with("jolt"), picked, 0.32);
        let bump = 4.0 * jolt * (1.0 - jolt) * if picked { 0.12 } else { -0.08 };
        let ny = inner.bottom() - 8.0;
        let chips: Vec<(Rect, &'static str)> = idx
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                let c = Pos2::new(inner.left() + 15.0 + j as f32 * 36.0, inner.top() + 15.0);
                (Rect::from_center_size(c, Vec2::splat(30.0)), app.tools[i].id())
            })
            .collect();
        let last = chips.last().map_or(inner.left(), |c| c.0.right());
        let free = Rect::from_min_max(Pos2::new(last + 14.0, inner.top() - 2.0), Pos2::new(inner.right(), ny - 12.0));
        card_loop(&p.with_clip_rect(r), k, r, free, &chips, now, hov);
        for (&i, &(cr, tid)) in idx.iter().zip(&chips) {
            let b = app.tools[i].badge();
            let (off, sc) = chip_motion(tid, now);
            let cr = Rect::from_center_size(cr.center() + off, cr.size() * sc * (1.0 + bump));
            tool_chip(&p, cr, b.icon, b.color);
        }
        // check, then name, bottom left
        let cc = Pos2::new(inner.left() + 9.0, ny);
        p.circle_stroke(cc, 8.5, Stroke::new(1.5, lerp_color(white(40), BLUE(), s)));
        if s > 0.01 {
            let z = pop(s);
            p.circle_filled(cc, 9.0 * z, BLUE());
            check(&p, cc, 7.0 * z, ON_ACTION);
        }
        // what each tool asks macOS for is on the Permissions step, in words
        let px = inner.right();
        let nx = inner.left() + 26.0;
        text_fit(&p, Pos2::new(nx, ny), Align2::LEFT_CENTER, goal.name, bold(14.0), TEXT(), px - nx - 6.0);
        if resp.clicked() {
            flip = Some((idx, !picked));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if let Some((idx, o)) = flip {
        for i in idx {
            app.set_tool_on(i, o);
        }
    }
}

/// A goal card's loop time: it runs while `on` (the pointer is on the card)
/// at the decorative pace, and holds still otherwise.
/// `target`, reached over `dur` seconds from wherever it was, with frames
/// at 30 fps while it moves (not every frame the display can show).
fn glide(ctx: &egui::Context, id: egui::Id, target: f32, dur: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let (from, to, start) = ctx.data_mut(|d| d.get_temp::<(f32, f32, f64)>(id)).unwrap_or((target, target, now));
    let at = |t: f64| from + (to - from) * ease(((t - start) as f32 / dur).clamp(0.0, 1.0));
    let (from, to, start) = if (target - to).abs() > f32::EPSILON { (at(now), target, now) } else { (from, to, start) };
    ctx.data_mut(|d| d.insert_temp(id, (from, to, start)));
    let k = ((now - start) as f32 / dur).clamp(0.0, 1.0);
    if k < 1.0 {
        let beat = std::time::Duration::from_millis(33);
        ctx.request_repaint_after(beat);
        ctx.request_repaint_after_for(beat, egui::ViewportId::ROOT);
    }
    from + (to - from) * ease(k)
}

fn loop_clock(ctx: &egui::Context, id: egui::Id, wall: f64, on: bool) -> f32 {
    let (mut t, last) = ctx.data_mut(|d| d.get_temp::<(f32, f64)>(id)).unwrap_or((2.0, wall));
    if on {
        t += (wall - last).clamp(0.0, 0.1) as f32;
        // wraps at a multiple of every loop's period, so f32 stays exact
        t %= 32760.0;
        // ~30 fps while the pointer is on it, even when it holds still
        // (setup draws in the root's pass: ask the root too)
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        ctx.request_repaint_after_for(std::time::Duration::from_millis(33), egui::ViewportId::ROOT);
    }
    ctx.data_mut(|d| d.insert_temp(id, (t, wall)));
    t
}

/// A value through `(phase, value)` keys, eased between neighbours; holds
/// the last value past the last key.
fn keys(ph: f32, k: &[(f32, f32)]) -> f32 {
    for w in k.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        if ph <= b {
            let x = if b > a { ((ph - a) / (b - a)).clamp(0.0, 1.0) } else { 1.0 };
            return va + (vb - va) * x * x * (3.0 - 2.0 * x);
        }
    }
    k.last().map_or(0.0, |l| l.1)
}

/// `v` turned by `a` radians round the origin.
fn turn(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// How a goal card's chip moves with its loop: an offset and a scale.
fn chip_motion(tool: &str, now: f32) -> (Vec2, Vec2) {
    use crate::tools::*;
    match tool {
        // the bin dips as each piece of junk lands
        uninstaller::ID => {
            let ph = (now / 4.8).fract();
            let b = [0.19f32, 0.52, 0.85].iter().map(|&a| (1.0 - (ph - a).abs() / 0.025).max(0.0)).fold(0.0, f32::max);
            (Vec2::new(0.0, 2.0 * b), Vec2::new(1.0, 1.0 - 0.07 * b))
        }
        // the clipboard takes each clip in
        clipboard::ID => {
            let b = keys((now / 1.5).fract(), &[(0.0, 0.0), (0.25, 0.0), (0.32, 1.0), (0.45, 0.0)]);
            (Vec2::ZERO, Vec2::splat(1.0 + 0.12 * b))
        }
        // the cup takes a sip as the limit resets
        keep_awake::ID => {
            let y = keys((now / 5.0).fract(), &[(0.0, 0.0), (0.86, 0.0), (0.90, -3.0), (0.93, 0.5), (0.96, 0.0)]);
            (Vec2::new(0.0, y), Vec2::splat(1.0))
        }
        _ => (Vec2::ZERO, Vec2::splat(1.0)),
    }
}

/// Goal card `k`'s loop, `k` in [`GOALS`] order: wordless and always
/// running, in `free` (right of the chips) at `now` seconds.
/// `hov`: how much the pointer is on the card (0..1); what would change
/// the card itself (the capture loop dims it) waits for it.
fn card_loop(p: &egui::Painter, k: usize, card: Rect, free: Rect, chips: &[(Rect, &str)], now: f32, hov: f32) {
    use crate::tools::*;
    if free.width() < 40.0 || free.height() < 30.0 {
        return;
    }
    let chip = |id: &str| chips.iter().find(|c| c.1 == id).map(|c| c.0);
    match GOALS[k].tools.first().copied() {
        Some(cleanup::ID) => {
            if let Some(bin) = chip(uninstaller::ID).or(chips.last().map(|c| c.0)) {
                loop_clean(p, free, bin, now);
            }
        }
        Some(capture::ID) => loop_capture(p, card, free, now, hov),
        Some(clipboard::ID) => loop_clipboard(p, free, now),
        Some(dictation::ID) => {
            if let Some(mic) = chip(dictation::ID) {
                loop_voice(p, free, mic, now);
            }
        }
        Some(system::ID) => loop_watch(p, free, chips.first().map_or(free.center().y, |c| c.0.center().y), now),
        Some(claude::ID) => loop_dev(p, free, now),
        _ => {}
    }
}

/// Clean my Mac: junk arcs into the bin, the disk bar shrinks, then a check.
fn loop_clean(p: &egui::Painter, f: Rect, bin: Rect, now: f32) {
    use std::f32::consts::PI;
    let ph = (now / 4.8).fract();
    let w = f.width().min(64.0);
    let bar = Rect::from_min_size(Pos2::new(f.right() - w, f.bottom() - 5.0), Vec2::new(w, 5.0));
    p.rect_filled(bar, 2.5, white(20));
    let v = keys(ph, &[(0.0, 0.92), (0.17, 0.92), (0.21, 0.74), (0.50, 0.74), (0.54, 0.58), (0.83, 0.58), (0.87, 0.44), (0.94, 0.44), (1.0, 0.92)]);
    p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(w * v, 5.0)), 2.5, tint::CLEANUP());
    let s = keys(ph, &[(0.0, 0.0), (0.86, 0.0), (0.89, 1.3), (0.92, 1.0), (0.96, 1.0), (1.0, 0.0)]);
    if s > 0.01 {
        let c = Pos2::new(bar.left() - 12.0, bar.center().y);
        p.circle_filled(c, 6.0 * s, tint::CLEANUP());
        check(p, c, 6.5 * s, ON_ACTION);
    }
    let cols = [tint::CLEANUP(), tint::CAPTURE(), tint::HEALTH()];
    for (j, (at, dx, dy, spin)) in [(0.02f32, 0.0f32, 0.0f32, -1.0f32), (0.35, -12.0, 8.0, 1.2), (0.68, 4.0, -6.0, -0.9)].into_iter().enumerate() {
        let q = (ph - at) / 0.19;
        if !(0.0..1.0).contains(&q) {
            continue;
        }
        let m = keys(q, &[(0.0, 0.0), (0.9, 1.0)]);
        let a0 = Pos2::new(f.right() - 8.0 + dx, f.center().y - 6.0 + dy);
        let e = bin.center();
        let c = Pos2::new((a0.x + e.x) / 2.0, f.top() - 16.0);
        let u = 1.0 - m;
        let pos = Pos2::new(u * u * a0.x + 2.0 * u * m * c.x + m * m * e.x, u * u * a0.y + 2.0 * u * m * c.y + m * m * e.y);
        let sc = if q < 0.9 { 1.0 - 0.3 * m } else { 0.7 - 0.4 * (q - 0.9) / 0.1 };
        let al = (q / 0.12).min(1.0) * if q > 0.9 { 1.0 - (q - 0.9) / 0.1 } else { 1.0 };
        let ang = spin * 1.2 * PI * m;
        let at = |x: f32, y: f32| pos + turn(Vec2::new(x, y) * sc, ang);
        let col = cols[j];
        let fill = with_alpha(col, 0.25 * al);
        let st = Stroke::new(1.2, with_alpha(col, al));
        match j {
            // a document with a folded corner
            0 => {
                p.add(egui::Shape::convex_polygon(vec![at(-5.0, -6.0), at(1.0, -6.0), at(5.0, -2.0), at(5.0, 6.0), at(-5.0, 6.0)], fill, st));
                p.add(egui::Shape::line(vec![at(1.0, -6.0), at(1.0, -2.0), at(5.0, -2.0)], st));
            }
            // an app
            1 => {
                let pts = rounded_points(Rect::from_center_size(Pos2::ZERO, Vec2::splat(10.0)), 3.0).into_iter().map(|q| at(q.x, q.y)).collect();
                p.add(egui::Shape::convex_polygon(pts, fill, st));
            }
            // a folder
            _ => {
                p.add(egui::Shape::convex_polygon(vec![at(-6.0, -5.0), at(-1.5, -5.0), at(0.0, -3.0), at(-6.0, -3.0)], fill, st));
                p.add(egui::Shape::convex_polygon(vec![at(-6.0, -3.0), at(6.0, -3.0), at(6.0, 5.0), at(-6.0, 5.0)], fill, st));
            }
        }
    }
}

/// Screenshots: a crosshair drags a selection, the rest dims, handles, a
/// flash, and the shot shrinks to a thumbnail that slides away.
fn loop_capture(p: &egui::Painter, card: Rect, f: Rect, now: f32, hov: f32) {
    let ph = (now / 3.6).fract();
    if ph >= 0.9 {
        return;
    }
    let w = (f.width() - 6.0).min(110.0);
    let h = (w * 0.6).min(f.height() - 10.0);
    let a0 = Pos2::new(f.left() + 2.0, f.top() + 4.0);
    let full = Rect::from_min_size(a0, Vec2::new(w, h));
    let (sel, al) = if ph < 0.6 {
        let g = ease(ph / 0.34);
        (Rect::from_min_size(a0, Vec2::new(w * g, h * g)), 1.0)
    } else {
        let m = keys(ph, &[(0.6, 0.0), (0.8, 1.0)]);
        let to = Pos2::new(f.right() - w * 0.18 - 2.0, f.bottom() - h * 0.18);
        let mut c = full.center().lerp(to, m);
        let n = ((ph - 0.8) / 0.1).clamp(0.0, 1.0);
        c.x += 30.0 * ease(n);
        (Rect::from_center_size(c, full.size() * (1.0 - 0.64 * m)), 1.0 - n)
    };
    // the dimmed screen around the selection only while the pointer is on
    // the card: at rest it made this card look unlike the others (M1)
    let dim = keys(ph, &[(0.0, 0.0), (0.06, 0.28), (0.52, 0.28), (0.60, 0.0)]) * if is_light() { 0.45 } else { 1.0 } * hov;
    if dim > 0.003 && sel.width() > 0.5 {
        let shade = Color32::from_black_alpha((dim * 255.0) as u8);
        let top = CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 };
        let bottom = CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 };
        p.rect_filled(Rect::from_min_max(card.min, Pos2::new(card.right(), sel.top())), top, shade);
        p.rect_filled(Rect::from_min_max(Pos2::new(card.left(), sel.bottom()), card.max), bottom, shade);
        p.rect_filled(Rect::from_min_max(Pos2::new(card.left(), sel.top()), Pos2::new(sel.left(), sel.bottom())), 0.0, shade);
        p.rect_filled(Rect::from_min_max(Pos2::new(sel.right(), sel.top()), Pos2::new(card.right(), sel.bottom())), 0.0, shade);
    }
    let col = tint::CAPTURE();
    if ph >= 0.54 {
        p.rect_filled(sel, 2.0, with_alpha(col, 0.18 * al * hov));
    }
    if ph < 0.53 {
        let path = vec![sel.left_top(), sel.right_top(), sel.right_bottom(), sel.left_bottom(), sel.left_top()];
        p.extend(egui::Shape::dashed_line(&path, Stroke::new(1.0, col), 3.0, 2.0));
    } else {
        p.rect_stroke(sel, 2.0, Stroke::new(1.0, with_alpha(col, al)), StrokeKind::Inside);
    }
    let ink = if is_light() { TEXT() } else { Color32::WHITE };
    let ca = keys(ph, &[(0.0, 1.0), (0.40, 1.0), (0.46, 0.0)]);
    if ca > 0.0 {
        let c = sel.right_bottom();
        let st = Stroke::new(1.0, with_alpha(ink, ca));
        p.line_segment([c - Vec2::new(7.5, 0.0), c + Vec2::new(7.5, 0.0)], st);
        p.line_segment([c - Vec2::new(0.0, 7.5), c + Vec2::new(0.0, 7.5)], st);
    }
    let ha = keys(ph, &[(0.0, 0.0), (0.36, 0.0), (0.40, 1.0), (0.52, 1.0), (0.56, 0.0)]);
    if ha > 0.0 {
        for c in [sel.left_top(), sel.right_top(), sel.left_bottom(), sel.right_bottom()] {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(4.5)), 1.0, with_alpha(ink, ha));
        }
    }
    // the shutter's flash: the rest pose falls in it, so only under the pointer
    let fl = keys(ph, &[(0.0, 0.0), (0.52, 0.0), (0.54, 0.22), (0.64, 0.0)]) * hov;
    if fl > 0.0 {
        p.rect_filled(card, 12.0, Color32::from_white_alpha((fl * 255.0) as u8));
    }
}

/// Clipboard: clips of different kinds fly in and stack; the oldest drops off.
fn loop_clipboard(p: &egui::Painter, f: Rect, now: f32) {
    let ph = (now / 6.0).fract();
    let size = Vec2::new(56.0f32.min(f.width() * 0.5), 34.0);
    let base = Pos2::new(f.left() + f.width() * 0.45, f.top() + 4.0 + size.y / 2.0);
    let mut items: Vec<(f32, usize)> = (0..4).map(|i| ((ph - i as f32 * 0.25).rem_euclid(1.0), i)).collect();
    // oldest first, so the newest lands on top
    items.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (q, i) in items {
        if q >= 0.83 {
            continue;
        }
        let r0 = [-6.0f32, 4.0, -3.0, 5.0][i];
        let lv = |n: f32| (Vec2::new(-6.0 * n, 6.0 * n), r0 - 2.0 * n, 1.0 - 0.03 * n);
        let ((off, deg, sc), al) = if q < 0.08 {
            let m = ease(q / 0.08);
            ((Vec2::new(-110.0, 10.0) * (1.0 - m), -12.0 + (r0 + 12.0) * m, 0.6 + 0.4 * m), m)
        } else if q < 0.25 {
            (lv(0.0), 1.0)
        } else if q < 0.33 {
            (lv(ease((q - 0.25) / 0.08)), 1.0)
        } else if q < 0.5 {
            (lv(1.0), 1.0)
        } else if q < 0.58 {
            let m = ease((q - 0.5) / 0.08);
            (lv(1.0 + m), 1.0 - 0.15 * m)
        } else if q < 0.75 {
            (lv(2.0), 0.85)
        } else {
            let m = ease((q - 0.75) / 0.08);
            let (o, d, s) = lv(2.0);
            ((o + Vec2::new(-18.0, 28.0) * m, d - 8.0 * m, s - 0.04 * m), 0.85 * (1.0 - m))
        };
        clip_card(p, base + off, size * sc, deg.to_radians(), i, al);
    }
}

/// One clip in the clipboard loop: text, a colour, an image or a link.
fn clip_card(p: &egui::Painter, c: Pos2, size: Vec2, ang: f32, kind: usize, al: f32) {
    let at = |x: f32, y: f32| c + turn(Vec2::new(x, y), ang);
    let outline: Vec<Pos2> = rounded_points(Rect::from_center_size(Pos2::ZERO, size), 7.0).into_iter().map(|q| at(q.x, q.y)).collect();
    let shadow = outline.iter().map(|q| *q + Vec2::new(0.0, 3.0)).collect();
    p.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha((70.0 * al) as u8), Stroke::NONE));
    p.add(egui::Shape::convex_polygon(outline, with_alpha(POPOVER(), al), Stroke::new(1.0, with_alpha(tint::CAPTURE(), 0.45 * al))));
    let (l, t) = (-size.x / 2.0 + 7.0, -size.y / 2.0);
    let line = |x0: f32, x1: f32, y: f32, col: Color32| p.line_segment([at(x0, y), at(x1, y)], Stroke::new(3.0, with_alpha(col, al)));
    let ink = white(140);
    let span = size.x - 14.0;
    match kind {
        0 => {
            line(l, l + span * 0.9, t + 9.0, ink);
            line(l, l + span * 0.6, t + 16.0, ink);
            line(l, l + span * 0.75, t + 23.0, ink);
        }
        1 => {
            p.circle_filled(at(l + 5.0, 0.0), 5.0 * size.y / 34.0, with_alpha(tint::CLEANUP(), al));
            line(l + 14.0, l + span * 0.85, -2.5, ink);
            line(l + 14.0, l + span * 0.6, 3.5, white(70));
        }
        2 => {
            let (b, r) = (size.y / 2.0 - 6.0, size.x / 2.0 - 6.0);
            p.add(egui::Shape::convex_polygon(vec![at(-r, -b), at(r, -b), at(r, b), at(-r, b)], with_alpha(tint::HEALTH(), 0.14 * al), Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![at(-r + 1.0, b), at(-r * 0.25, -b * 0.3), at(r * 0.45, b)], with_alpha(tint::HEALTH(), 0.75 * al), Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![at(r * 0.05, b), at(r * 0.5, b * 0.1), at(r - 1.0, b)], with_alpha(tint::HEALTH(), 0.5 * al), Stroke::NONE));
            p.circle_filled(at(r - 5.0, -b + 4.0), 2.8, with_alpha(tint::CAPTURE(), al));
        }
        _ => {
            let st = Stroke::new(1.5, with_alpha(tint::CAPTURE(), al));
            p.circle_stroke(at(l + 3.5, 0.0), 3.5, st);
            p.circle_stroke(at(l + 8.5, 0.0), 3.5, st);
            line(l + 17.0, l + span * 0.9, 0.0, with_alpha(tint::HEALTH(), 0.85));
        }
    }
}

/// Voice typing: the voice wave runs from the mic and comes out as words.
fn loop_voice(p: &egui::Painter, f: Rect, mic: Rect, now: f32) {
    use std::f32::consts::TAU;
    const SAID: [&str; 3] = ["Ship it today", "Call me at 5", "Sounds good!"];
    let said = SAID[(now / 4.2) as usize % SAID.len()];
    let q = (now / 4.2).fract() * 4.2;
    let fnt = font(11.5);
    let widest = SAID.iter().map(|s| p.layout_no_wrap((*s).into(), fnt.clone(), TEXT()).size().x).fold(0.0, f32::max);
    let n = (((f.width() - 10.0 - widest) / 6.0).floor() as i32).clamp(5, 12) as usize;
    let cy = mic.center().y;
    // the wave is loud while words come, quiet in between
    let loud = keys(q / 4.2, &[(0.0, 0.35), (0.06, 1.0), (0.42, 1.0), (0.55, 0.35)]);
    for j in 0..n {
        let x = j as f32;
        let v = 0.5 + 0.25 * (TAU * (now / 0.9 + x * 0.17)).sin() + 0.25 * (TAU * (now / 1.3 - x * 0.29) + x).sin();
        let h = 30.0 * (0.12 + 0.88 * v * loud);
        let fade = (1.0 - (x - n as f32 * 0.6) / (n as f32 * 0.4 + 1.0)).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_center_size(Pos2::new(f.left() + 1.5 + 6.0 * x, cy), Vec2::new(3.0, h)), 1.5, with_alpha(tint::CAPTURE(), fade));
    }
    let x = f.left() + n as f32 * 6.0 + 8.0;
    let shown = (((q - 0.35) / 0.11).floor() as i32 + 1).clamp(0, said.len() as i32) as usize;
    let al = if q > 3.85 { 1.0 - (q - 3.85) / 0.35 } else { 1.0 };
    let g = p.text(Pos2::new(x, cy), Align2::LEFT_CENTER, &said[..shown], fnt, with_alpha(TEXT(), al));
    if (now / 0.8).fract() < 0.5 {
        let cx = if shown == 0 { x } else { g.right() + 1.5 };
        p.rect_filled(Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(1.5, 13.0)), 0.5, with_alpha(tint::CAPTURE(), al));
    }
    // the mic glows with the voice
    let v = 0.5 - 0.5 * (TAU * now / 0.9).cos();
    p.rect_stroke(mic.expand(1.0 + 2.0 * v), 10.0, Stroke::new(3.0, with_alpha(tint::CAPTURE(), 0.2 * v)), StrokeKind::Outside);
}

/// Watch my Mac: an endless heartbeat trace on a grid, a slow CPU line
/// behind it, a bright head at "now".
fn loop_watch(p: &egui::Painter, f: Rect, cy: f32, now: f32) {
    let mh = f.height().min(44.0);
    let top = (cy - mh / 2.0).clamp(f.top(), f.bottom() - mh);
    let m = Rect::from_min_size(Pos2::new(f.left(), top), Vec2::new(f.width(), mh));
    let p = p.with_clip_rect(m.intersect(p.clip_rect()));
    let col = tint::HEALTH();
    let grid = Stroke::new(1.0, with_alpha(col, 0.08));
    let mut x = m.left() + 6.0;
    while x < m.right() {
        p.line_segment([Pos2::new(x, m.top()), Pos2::new(x, m.bottom())], grid);
        x += 12.0;
    }
    let mut y = m.top() + 5.5;
    while y < m.bottom() {
        p.line_segment([Pos2::new(m.left(), y), Pos2::new(m.right(), y)], grid);
        y += 11.0;
    }
    let head = m.left() + m.width() * 0.88;
    let sy = mh / 44.0;
    let fade = |x: f32| ((x - m.left()) / (m.width() * 0.45)).clamp(0.0, 1.0);
    // CPU, slow and soft
    let mut prev: Option<Pos2> = None;
    let mut x = m.left();
    while x <= head {
        let u = (x - m.left() + now * 22.5) / 60.0 * std::f32::consts::TAU;
        let q = Pos2::new(x, m.top() + (31.0 + 3.0 * u.sin() + 1.5 * (u * 2.3).sin()) * sy);
        if let Some(a) = prev {
            p.line_segment([a, q], Stroke::new(1.5, with_alpha(tint::CLEANUP(), 0.45 * fade(x))));
        }
        prev = Some(q);
        x += 3.0;
    }
    // heartbeat, one beat every 50 px, scrolling left
    const BEAT: [(f32, f32); 8] = [(0.0, 22.0), (20.0, 22.0), (23.0, 18.0), (26.0, 22.0), (30.0, 22.0), (34.0, 8.0), (39.0, 36.0), (43.0, 22.0)];
    let off = (now / 1.5).fract() * 50.0;
    let mut pts = Vec::new();
    let mut x0 = m.left() - 50.0 - off;
    while x0 <= head + 50.0 {
        pts.extend(BEAT.iter().map(|&(x, y)| Pos2::new(x0 + x, m.top() + y * sy)));
        x0 += 50.0;
    }
    let mut tip = None;
    for w in pts.windows(2) {
        let (a, mut b) = (w[0], w[1]);
        if a.x >= head {
            break;
        }
        if b.x > head {
            b = a + (b - a) * ((head - a.x) / (b.x - a.x));
            tip = Some(b);
        }
        p.line_segment([a, b], Stroke::new(1.8, with_alpha(col, fade((a.x + b.x) / 2.0))));
    }
    if let Some(t) = tip {
        p.circle_filled(t, 5.0, with_alpha(col, 0.3));
        p.circle_filled(t, 2.5, if is_light() { col } else { Color32::WHITE });
    }
}

/// For developers: a usage gauge climbs green → amber → red, the limit
/// resets and the needle drops back.
fn loop_dev(p: &egui::Painter, f: Rect, now: f32) {
    use std::f32::consts::PI;
    let ph = (now / 5.0).fract();
    let v = keys(ph, &[(0.0, 0.0), (0.06, 0.0), (0.30, 0.42), (0.34, 0.38), (0.70, 0.84), (0.80, 0.84), (0.83, 0.81), (0.90, 0.0)]);
    let r = (f.width() / 2.0 - 9.0).min(f.height() - 14.0).min(24.0);
    let c = Pos2::new(f.center().x, f.top() + r + 8.0);
    let ang = |x: f32| PI + x * PI;
    let arc = |a: f32, b: f32, col: Color32, w: f32| {
        if b > a {
            let n = (((b - a) * 32.0).ceil() as usize).max(2);
            p.add(egui::Shape::line(arc_points(c, r, ang(a), ang(b), n), Stroke::new(w, col)));
        }
    };
    arc(0.0, 1.0, white(20), 5.0);
    arc(0.7, 1.0, with_alpha(RED(), 0.16), 5.0);
    arc(0.0, v.min(0.45), tint::CLEANUP(), 5.0);
    arc(0.45, v.min(0.7), AMBER(), 5.0);
    arc(0.7, v, RED(), 5.0);
    for i in 0..=20 {
        let x = i as f32 / 20.0;
        let major = i % 5 == 0;
        let d = Vec2::angled(ang(x));
        let (a, b, col) = if major { (r + 4.5, r + 7.0, white(100)) } else { (r + 4.5, r + 5.8, white(45)) };
        p.line_segment([c + d * a, c + d * b], Stroke::new(if major { 1.0 } else { 0.8 }, col));
    }
    let rs = keys(ph, &[(0.0, 0.0), (0.86, 0.0), (0.90, 1.0), (0.98, 1.35)]);
    let ra = keys(ph, &[(0.0, 0.0), (0.86, 0.0), (0.90, 0.9), (0.98, 0.0)]);
    if ra > 0.0 {
        p.circle_stroke(c, 9.0 * (0.6 + 0.4 * rs), Stroke::new(1.4, with_alpha(tint::CLEANUP(), ra)));
    }
    let d = Vec2::angled(ang(v));
    p.line_segment([c - d * 3.0, c + d * (r - 5.0)], Stroke::new(1.6, TEXT()));
    p.circle_filled(c, 3.0, TEXT());
    p.circle_filled(c, 1.1, BG());
}

/// Every tool with a switch, by section in two columns. Returns the tool
/// under the pointer.
fn all_tools(app: &mut App, ui: &mut Ui, area: Rect, t: f32) {
    let p = ui.painter().clone();
    let secs = sections(app);
    let gap = 18.0;
    let cw = (area.width() - gap) / 2.0;
    let mut ys = [area.top(); 2];
    let mut flip = None;
    let mut k = 0;
    for (si, sec) in secs.iter().enumerate() {
        let c = si % 2;
        let x = area.left() + c as f32 * (cw + gap);
        group_header(&p, Pos2::new(x + 8.0, ys[c]), sec);
        ys[c] += 20.0;
        for i in 0..app.tools.len() {
            if app.tools[i].section() != *sec {
                continue;
            }
            let a = ease((t - 0.03 * k as f32) / 0.3);
            k += 1;
            let r = Rect::from_min_size(Pos2::new(x, ys[c]), Vec2::new(cw, 36.0));
            ys[c] += 36.0;
            let id = ui.scope_id().with(("setup-tool", app.tools[i].id()));
            let resp = ui.interact(r, id, Sense::click());
            let o = app.tool_on(i);
            if ui.rect_contains_pointer(r) {
                p.rect_filled(r, 9.0, CARD());
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let b = app.tools[i].badge();
            let chip = Rect::from_center_size(Pos2::new(r.left() + 20.0, r.center().y), Vec2::splat(24.0));
            tool_chip(&p, chip, b.icon, if o { b.color } else { tint::NEUTRAL() });
            let sw = Rect::from_min_size(Pos2::new(r.right() - 38.0, r.center().y - 9.0), Vec2::new(30.0, 18.0));
            let tx = chip.right() + 10.0;
            text_fit(&p, Pos2::new(tx, r.center().y), Align2::LEFT_CENTER, app.tools[i].name(), font(13.0), lerp_color(TEXT_DIM(), TEXT(), a * o as u8 as f32).gamma_multiply(a.max(0.3)), sw.left() - tx - 60.0);
            if switch(ui, sw, id.with("sw"), o, true).clicked() || resp.clicked() {
                flip = Some((i, !o));
            }
        }
        ys[c] += sp::S;
    }
    if let Some((i, o)) = flip {
        app.set_tool_on(i, o);
    }
}

/// The menu bar panel as it will be: the picked tools' widgets, laid out
/// as the panel lays them out, filled with sample data. A widget drops in
/// when its goal is picked and the others make room.
fn mini_panel(app: &mut App, ui: &mut Ui, area: Rect, t: f32) {
    use crate::core::panel;
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let a = ease((t - 0.15) / 0.4);
    // the menu bar, our icon lit
    let mb = Rect::from_min_size(area.min, Vec2::new(area.width(), 24.0));
    p.rect_filled(mb, 7.0, pick(Color32::from_black_alpha(100), Color32::from_white_alpha(170)).gamma_multiply(a));
    let cat = Rect::from_min_size(Pos2::new(mb.right() - 30.0, mb.top() + 3.0), Vec2::new(22.0, 18.0));
    p.rect_filled(cat, 5.0, with_alpha(BLUE(), 0.16 * a));
    brand_glyph(&p, cat.center(), 14.0, BLUE().gamma_multiply(a));
    let mut sx = cat.left() - 12.0;
    for name in ["battery.75percent", "wifi"] {
        let r = Rect::from_min_size(Pos2::new(sx - 14.0, mb.center().y - 7.0), Vec2::splat(14.0));
        symbol(&p, name, r, TEXT_DIM().gamma_multiply(a));
        sx -= 24.0;
    }
    // the panel's own grid, at its own width, scaled to the column; when
    // it's taller than the step, scaled down to fit
    let native_w = panel::inner_width();
    let room = Rect::from_min_max(Pos2::new(area.left(), mb.bottom() + 8.0), area.max);
    let pad = panel::PAD;
    // each widget at its fullest size up to Medium, so it shows what it
    // holds; when that gets too small to read (many goals), the sizes the
    // panel itself gives them
    let (placed, _) = panel::place(app, native_w);
    let lay = |fullest: bool| {
        let ws: Vec<(usize, &'static str, crate::core::tool::Size)> = placed
            .iter()
            .filter_map(|w| {
                let owner = w.owner?;
                let spec = app.tools[owner].widgets().into_iter().find(|s| s.id == w.id)?;
                let size = if fullest { crate::core::tool::UP_TO_MEDIUM.iter().rev().copied().find(|z| spec.sizes.contains(z)).unwrap_or(w.size) } else { w.size };
                Some((owner, w.id, size))
            })
            .collect();
        let sizes: Vec<(u8, f32)> = ws.iter().map(|w| (w.2.cols(), panel::size_px(w.2, native_w).y)).collect();
        let (rects, total) = crate::core::layout::flow(&sizes, native_w, panel::GAP);
        (ws, rects, total)
    };
    let fit = |total: f32| (room.width() / (native_w + 2.0 * pad)).min(room.height() / (total + 2.0 * pad)).min(1.0);
    let (mut placed, mut rects, mut total) = lay(true);
    if fit(total) < MIN_PANEL_SCALE {
        (placed, rects, total) = lay(false);
    }
    let total = ctx.animate_value_with_time(ui.scope_id().with("setup-panel-h"), total, 0.25);
    // never smaller than readable: past that, the panel runs off the bottom
    let scale = fit(total).max(MIN_PANEL_SCALE.min(room.width() / (native_w + 2.0 * pad)));
    let empty_h = 120.0;
    let h = if placed.is_empty() { empty_h } else { ((total + 2.0 * pad) * scale).min(room.height()) };
    let panel = Rect::from_min_size(room.min, Vec2::new(room.width(), h));
    let panel = panel.translate(Vec2::new(0.0, (1.0 - a) * -10.0));
    p.add(egui::epaint::Shadow { offset: [0, 14], blur: 36, spread: 0, color: Color32::from_black_alpha((70.0 * a) as u8) }.as_shape(panel, CornerRadius::same(12)));
    p.rect_filled(panel, 12.0, BG().gamma_multiply(a));
    p.rect_stroke(panel, 12.0, Stroke::new(1.0, CARD_EDGE().gamma_multiply(a)), StrokeKind::Inside);
    if placed.is_empty() {
        brand_glyph(&p, panel.center(), 40.0, white(24).gamma_multiply(a));
        return;
    }
    let Some(mut samples) = app.setup.as_mut().map(|s| s.samples.take().unwrap_or_else(|| crate::tools::all(&crate::core::tool::ToolEnv { ctx: ctx.clone(), demo: true }))) else { return };
    let mut cx = app.cx();
    // centred in the column when the height is what limits it
    let x0 = panel.center().x - (native_w + 2.0 * pad) * scale / 2.0;
    let origin = Pos2::new(x0 + pad * scale, panel.top() + pad * scale);
    let id = ui.scope_id().with("setup-panel-widgets");
    let demo = app.demo;
    let layer = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::ZERO)
        .constrain(false)
        .interactable(false)
        .show(&ctx, |ui| {
            ui.multiply_opacity(a);
            ui.set_clip_rect(Rect::from_min_size(Pos2::new(-pad, -pad), Vec2::new(native_w + 2.0 * pad, h / scale)));
            for (&(owner, wid_s, size), rect) in placed.iter().zip(&rects) {
                let tid = app.tools[owner].id();
                let Some(tool) = samples.iter_mut().find(|t| t.id() == tid) else { continue };
                let wid = id.with((tid, wid_s));
                // it moves to its new place, and drops in when it's new
                let x = ctx.animate_value_with_time(wid.with("x"), rect.left(), 0.25);
                let y = ctx.animate_value_with_time(wid.with("y"), rect.top(), 0.25);
                let d = appear(&ctx, wid.with("in"), now, 0.36, demo);
                let r = Rect::from_min_size(Pos2::new(x, y - (1.0 - d) * 14.0), rect.size());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt((tid, wid_s, "sample")));
                child.multiply_opacity(d);
                // a sample that fails stays blank; the live tool isn't stopped for it
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tool.widget_ui(wid_s, size, &mut child, r, &mut cx)));
            }
        })
        .response
        .layer_id;
    ctx.set_transform_layer(layer, egui::emath::TSTransform::new(origin.to_vec2(), scale));
    // cut off at the bottom: it fades out there, as a list that goes on
    if (total + 2.0 * pad) * scale > room.height() + 0.5 {
        let fade = Rect::from_min_max(Pos2::new(panel.left() + 1.0, panel.bottom() - 40.0), Pos2::new(panel.right() - 1.0, panel.bottom() - 1.0));
        let top_c = BG().gamma_multiply(0.0);
        let bot_c = BG().gamma_multiply(a);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(fade.left_top(), top_c);
        mesh.colored_vertex(fade.right_top(), top_c);
        mesh.colored_vertex(fade.right_bottom(), bot_c);
        mesh.colored_vertex(fade.left_bottom(), bot_c);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, id.with("fade"))).add(mesh);
    }
    // what a sample widget asks for (open a page…) goes nowhere
    let _ = cx.take_actions();
    if let Some(s) = app.setup.as_mut() {
        s.samples = Some(samples);
    }
}

/// The AI agents the person uses: one main (Cleanup checks with it), and
/// the ones AI Usage tracks. Saved in the AI Usage settings, changed later in
/// Settings › AI Usage.
/// "`n`/`total`" where `n` rolls between whole numbers as `shown` moves:
/// the old number slides up out of view and the new one comes up from below.
fn rolling_count(p: &egui::Painter, left_bottom: Pos2, shown: f32, total: usize) {
    let f = mono(13.0);
    let lo = shown.floor();
    let frac = shown - lo;
    let lo_s = format!("{}", lo as i64);
    let hi_s = format!("{}", lo as i64 + 1);
    let w = p.layout_no_wrap(if frac > 0.0 { hi_s.clone() } else { lo_s.clone() }, f.clone(), TEXT_FAINT()).size().x.max(p.layout_no_wrap(lo_s.clone(), f.clone(), TEXT_FAINT()).size().x);
    let line = 16.0;
    let cell = Rect::from_min_max(Pos2::new(left_bottom.x, left_bottom.y - line), Pos2::new(left_bottom.x + w, left_bottom.y + 1.0));
    let clip = p.with_clip_rect(cell.intersect(p.clip_rect()));
    clip.text(left_bottom - Vec2::new(0.0, frac * line), Align2::LEFT_BOTTOM, &lo_s, f.clone(), TEXT_FAINT());
    if frac > 0.0 {
        clip.text(left_bottom + Vec2::new(0.0, (1.0 - frac) * line), Align2::LEFT_BOTTOM, &hi_s, f.clone(), TEXT_FAINT());
    }
    p.text(Pos2::new(cell.right(), left_bottom.y), Align2::LEFT_BOTTOM, format!("/{total}"), f, TEXT_FAINT());
}

/// Step 3a exists: AI Usage is on and a picked agent (Main or Track) keeps
/// its sign-in in the Keychain. Only Claude Code does; Codex and Gemini
/// keep theirs in files, read without asking.
fn has_sign_ins(app: &App) -> bool {
    let usage_on = (0..app.tools.len()).any(|i| app.tools[i].id() == crate::tools::claude::ID && app.tool_on(i));
    usage_on && crate::tools::claude::keychain::wanted(app.demo)
}

/// Step 3a: Keychain sign-ins of the picked agents, so AI Usage shows
/// limits from their accounts. Nothing is read before Allow is pressed;
/// then macOS asks (Allow / Always Allow / Deny). Laid out like AI agents.
fn sign_ins(app: &mut App, ui: &mut Ui, body: Rect, t: f32) {
    use crate::kit::perms::Status;
    use crate::tools::claude::keychain;
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    keychain::refocus(&ctx);
    let w = body.width().min(640.0);
    let x = body.center().x - w / 2.0;
    let head = body.top() + 6.0;
    p.text(Pos2::new(x, head), Align2::LEFT_TOP, "Sign-ins", bold(22.0), TEXT());
    // the section, as on AI agents: what it's for
    let y = head + 22.0 + sp::XL;
    let cr = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(42.0));
    tool_chip(&p, cr, Icon::Lock, TEXT_SOFT());
    p.text(Pos2::new(cr.right() + sp::M, cr.top() + 2.0), Align2::LEFT_TOP, "Keychain", bold(15.0), TEXT());
    p.text(Pos2::new(cr.right() + sp::M, cr.bottom() - 2.0), Align2::LEFT_BOTTOM, "Limits from your account in AI Usage", ty::body(), TEXT_DIM());
    // one tile per picked agent that keeps it there: today only Claude Code
    let a = ease(t / 0.4);
    let r = Rect::from_min_size(Pos2::new(x, cr.bottom() + sp::M + (1.0 - a) * 8.0), Vec2::new(w, 84.0));
    let st = if app.demo { Status::NotAsked } else { keychain::status() };
    let ok = st == Status::Allowed;
    let s = ctx.animate_bool_with_time(egui::Id::new("signin-tile"), ok, 0.18);
    p.rect_filled(r, 12.0, lerp_color(CARD(), with_alpha(GREEN(), 0.10), s).gamma_multiply(a));
    p.rect_stroke(r, 12.0, Stroke::new(1.0, lerp_color(CARD_EDGE(), with_alpha(GREEN(), 0.55), s).gamma_multiply(a)), StrokeKind::Inside);
    agent_mark(&p, Rect::from_min_size(Pos2::new(r.left() + 12.0, r.top() + 12.0), Vec2::splat(28.0)), "claude", false);
    p.text(Pos2::new(r.left() + 12.0, r.bottom() - 12.0), Align2::LEFT_BOTTOM, "Claude Code", ty::title(), TEXT());
    let right = r.right() - sp::L;
    let busy = !app.demo && keychain::busy();
    let err = if app.demo { String::new() } else { keychain::error() };
    if ok {
        let since = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("signin-ok"))).unwrap_or(now - 10.0);
        check_drawn(&p, Pos2::new(right - 13.0, r.center().y), 12.0, GREEN(), ease(((now - since) / 0.35) as f32));
    } else if busy {
        let c = Pos2::new(right - 8.0, r.center().y);
        spinner(&p, c, 6.0, now, BLUE());
        p.text(Pos2::new(c.x - 16.0, c.y), Align2::RIGHT_CENTER, "Choose Always Allow", font(12.0), TEXT_FAINT());
    } else {
        let label = if st == Status::Denied || !err.is_empty() { "Try again" } else { "Allow" };
        let bw = button_width(&p, label, BTN_SECONDARY);
        let b = Rect::from_min_size(Pos2::new(right - bw, r.center().y - 13.0), Vec2::new(bw, 26.0));
        if pill_button(ui, b, label, BTN_SECONDARY, true).clicked() && !app.demo {
            keychain::connect(Some(ctx.clone()));
        }
        if !err.is_empty() {
            p.text(Pos2::new(b.left() - sp::M, r.center().y), Align2::RIGHT_CENTER, err, font(12.0), RED());
        } else if st == Status::Denied {
            p.text(Pos2::new(b.left() - sp::M, r.center().y), Align2::RIGHT_CENTER, "Denied", font(12.0), AMBER());
        }
    }
    // the check pops once when it turns allowed while shown
    let id = egui::Id::new("signin-ok");
    let was = ctx.data(|d| d.get_temp::<bool>(id.with("was"))).unwrap_or(ok);
    if ok && !was {
        ctx.data_mut(|d| d.insert_temp(id, now));
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id.with("was"), ok));
}

/// An AI agent's mark, in `r`: drawn here, no image files. Claude a
/// sunburst, Codex a prompt, Gemini a four-point star, OpenCode braces.
fn agent_mark(p: &egui::Painter, r: Rect, id: &str, dim: bool) {
    let k = if dim { 0.45 } else { 1.0 };
    let c = r.center();
    let s = r.width();
    match id {
        "claude" => {
            let col = Color32::from_rgb(0xD9, 0x77, 0x57).gamma_multiply(k);
            p.rect_filled(r, s * 0.27, with_alpha(col, 0.14));
            for i in 0..12 {
                let a = i as f32 / 12.0 * std::f32::consts::TAU;
                let len = if i % 2 == 0 { 0.34 } else { 0.26 };
                let d = Vec2::new(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.07, c + d * s * len], Stroke::new(s * 0.075, col));
            }
        }
        "codex" => {
            p.rect_filled(r, s * 0.27, pick(Color32::from_gray(236), Color32::from_gray(24)).gamma_multiply(k));
            p.text(c, Align2::CENTER_CENTER, ">_", mono(s * 0.42), pick(Color32::from_gray(20), Color32::WHITE).gamma_multiply(k));
        }
        "gemini" => {
            let col = Color32::from_rgb(0x4C, 0x8D, 0xF6).gamma_multiply(k);
            p.rect_filled(r, s * 0.27, with_alpha(col, 0.14));
            let q = s * 0.32;
            let w = s * 0.07;
            let pts = vec![c + Vec2::new(0.0, -q), c + Vec2::new(w, -w), c + Vec2::new(q, 0.0), c + Vec2::new(w, w), c + Vec2::new(0.0, q), c + Vec2::new(-w, w), c + Vec2::new(-q, 0.0), c + Vec2::new(-w, -w)];
            p.add(egui::Shape::convex_polygon(vec![pts[0], pts[1], pts[7]], col, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![pts[2], pts[3], pts[1]], col, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![pts[4], pts[5], pts[3]], col, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![pts[6], pts[7], pts[5]], col, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![pts[1], pts[3], pts[5], pts[7]], col, Stroke::NONE));
        }
        _ => {
            p.rect_filled(r, s * 0.27, white(18).gamma_multiply(k));
            p.text(c, Align2::CENTER_CENTER, "{ }", mono(s * 0.38), TEXT().gamma_multiply(k));
        }
    }
}

fn agents(app: &mut App, ui: &mut Ui, body: Rect, t: f32) {
    use crate::kit::ai::{self, settings};
    use crate::tools::*;
    let p = ui.painter().clone();
    let set = settings::get();
    // a column in the middle of the step
    let w = body.width().min(COL_W);
    let x = body.center().x - w / 2.0;
    let head = body.top() + 6.0;
    p.text(Pos2::new(x, head), Align2::LEFT_TOP, "AI agents", bold(22.0), TEXT());
    let n = ai::PROVIDERS.len();
    let found: Vec<bool> = (0..n).map(|k| app.demo || ai::make(k).data_dir().is_some_and(|d| d.exists())).collect();
    let tile_h = 84.0;
    let gap = sp::S;
    let tile_w = (w - gap * (n as f32 - 1.0)) / n as f32;
    // a section: its tool, its name, what it's for, and where it shows (the
    // tools' own chips); then one tile per agent
    let section = |ui: &mut Ui, y: f32, chip_icon: Icon, chip_col: Color32, name: &str, what: &str, tools: &[&str]| -> f32 {
        let cr = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(42.0));
        tool_chip(&p, cr, chip_icon, chip_col);
        p.text(Pos2::new(cr.right() + sp::M, cr.top() + 2.0), Align2::LEFT_TOP, name, bold(15.0), TEXT());
        p.text(Pos2::new(cr.right() + sp::M, cr.bottom() - 2.0), Align2::LEFT_BOTTOM, what, ty::body(), TEXT_DIM());
        let mut tx = x + w;
        for id in tools.iter().rev() {
            if let Some(i) = app.tool_index(id) {
                let b = app.tools[i].badge();
                let r = Rect::from_min_size(Pos2::new(tx - 26.0, cr.center().y - 13.0), Vec2::splat(26.0));
                tool_chip(&p, r, b.icon, b.color);
                ui.interact(r, ui.scope_id().with(("setup-ai-tool", name, *id)), Sense::hover()).on_hover_text(app.tools[i].name());
                tx -= 26.0 + 6.0;
            }
        }
        cr.bottom() + sp::M
    };
    // a ping round the main agent's mark when it changes
    let now = ui.ctx().input(|i| i.time);
    let ping_id = ui.scope_id().with("setup-main-ping");
    let (was, since) = ui.ctx().data(|d| d.get_temp::<(String, f64)>(ping_id)).unwrap_or_else(|| (set.main.clone(), f64::NEG_INFINITY));
    let since = if was != set.main { now } else { since };
    ui.ctx().data_mut(|d| d.insert_temp(ping_id, (set.main.clone(), since)));
    let ping = ((now - since) / 0.55) as f32;
    type Change = Box<dyn FnOnce(&mut settings::Settings)>;
    let mut change: Option<Change> = None;
    // one tile: the agent's mark and name, found or not; `on` lights it
    let tile = |ui: &mut Ui, r: Rect, k: usize, on: bool, a: f32| -> egui::Response {
        let m = &ai::PROVIDERS[k];
        let resp = ui.interact(r, ui.scope_id().with(("setup-ai-tile", r.top() as i32, m.id)), if found[k] { Sense::click() } else { Sense::hover() });
        let hov = ui.ctx().animate_bool(resp.id.with("h"), resp.hovered() && found[k]);
        let s = ui.ctx().animate_bool_with_time(resp.id.with("on"), on, 0.18);
        let fill = lerp_color(lerp_color(CARD(), white(30), hov * 0.15), with_alpha(BLUE(), 0.10), s);
        p.rect_filled(r, 12.0, fill.gamma_multiply(a));
        p.rect_stroke(r, 12.0, Stroke::new(1.0, lerp_color(CARD_EDGE(), with_alpha(BLUE(), 0.55), s).gamma_multiply(a)), StrokeKind::Inside);
        let mr = Rect::from_min_size(Pos2::new(r.left() + 12.0, r.top() + 12.0), Vec2::splat(28.0));
        agent_mark(&p, mr, m.id, !found[k]);
        p.text(Pos2::new(r.left() + 12.0, r.bottom() - 12.0), Align2::LEFT_BOTTOM, m.name, ty::title(), if found[k] { TEXT() } else { TEXT_FAINT() });
        if !found[k] {
            p.text(Pos2::new(r.right() - 12.0, r.bottom() - 12.0), Align2::RIGHT_BOTTOM, "Not found", ty::caption(), TEXT_FAINT());
        }
        if resp.hovered() && found[k] {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    };
    // AI Check: one agent
    let mut y = head + 22.0 + sp::XL;
    let cleanup_chip = app.tool_index(cleanup::ID).map(|i| app.tools[i].badge().color).unwrap_or(BLUE());
    y = section(ui, y, Icon::Sparkle, cleanup_chip, "AI Check", "Checks what's safe to remove", &[]);
    for k in 0..n {
        let m = &ai::PROVIDERS[k];
        let a = ease((t - 0.05 * k as f32) / 0.4);
        let r = Rect::from_min_size(Pos2::new(x + k as f32 * (tile_w + gap), y + (1.0 - a) * 8.0), Vec2::new(tile_w, tile_h));
        let main = set.main == m.id;
        let resp = tile(ui, r, k, main, a);
        let rr = Rect::from_center_size(Pos2::new(r.right() - 20.0, r.top() + 20.0), Vec2::splat(16.0));
        if main && (0.0..1.0).contains(&ping) {
            let e = ease(ping);
            crate::kit::glass::stroke_circle(&p, rr.center(), 8.0 + 13.0 * e, Stroke::new(1.5, with_alpha(BLUE(), 0.7 * (1.0 - e))));
            ui.ctx().request_repaint();
        }
        let dot = if found[k] { radio(ui, rr, ui.scope_id().with(("setup-main", m.id)), main, true).clicked() } else { false };
        if (resp.clicked() || dot) && !main && found[k] {
            let id = m.id.to_string();
            change = Some(Box::new(move |s| {
                s.off.retain(|x| *x != id);
                s.provider = id.clone();
                s.main = id;
            }));
        }
    }
    // AI Usage: every agent it follows; only when that tool was picked
    // (ticking agents for a tool left off did nothing, M1)
    let usage_on = app.demo || app.tool_index(claude::ID).is_some_and(|i| app.tool_on(i));
    if usage_on {
        y += tile_h + sp::XL + sp::M;
        let usage = app.tool_index(claude::ID).map(|i| app.tools[i].badge()).map(|b| (b.icon, b.color)).unwrap_or((Icon::Sparkle, BLUE()));
        y = section(ui, y, usage.0, usage.1, "AI Usage", "Limits and tokens in your panel", &[]);
        for k in 0..n {
            let m = &ai::PROVIDERS[k];
            let a = ease((t - 0.2 - 0.05 * k as f32) / 0.4);
            let r = Rect::from_min_size(Pos2::new(x + k as f32 * (tile_w + gap), y + (1.0 - a) * 8.0), Vec2::new(tile_w, tile_h));
            let main = set.main == m.id;
            let on = set.is_on(m.id) || main;
            let resp = tile(ui, r, k, on && found[k], a);
            let cb = Rect::from_center_size(Pos2::new(r.right() - 20.0, r.top() + 20.0), Vec2::splat(16.0));
            // the main agent is always followed; one not on this Mac has
            // nothing to follow, so no box (a ticked one read as "on", M1)
            if !found[k] {
                continue;
            }
            let box_ = checkbox(ui, cb, ui.scope_id().with(("setup-track", m.id)), on, true);
            let box_ = if main { box_.on_hover_text("The AI Check agent is always followed") } else { box_ };
            if (box_.clicked() || resp.clicked()) && !main {
                let id = m.id.to_string();
                let was_on = set.is_on(m.id);
                change = Some(Box::new(move |s| {
                    s.off.retain(|x| *x != id);
                    if was_on {
                        s.off.push(id);
                    }
                }));
            }
        }
    }
    if let Some(f) = change {
        settings::update(!app.demo, f);
    }
}

/// One model on the Download models step and the tools it's for: (tool
/// index, what that tool uses it for). Several tools can share one file.
struct ModelRow {
    tools: Vec<(usize, &'static str)>,
    model: crate::core::tool::SetupModel,
}

/// Every enabled tool's models, one row per model (shared ones merged; the
/// tool that is downloading it, or needs it now, comes first).
fn models(app: &App) -> Vec<ModelRow> {
    use crate::core::tool::ModelState;
    let mut rows: Vec<ModelRow> = Vec::new();
    for i in (0..app.tools.len()).filter(|&i| app.tool_on(i)) {
        for m in app.tools[i].setup_models() {
            match rows.iter_mut().find(|r| r.model.name == m.name) {
                Some(r) if r.model.state == ModelState::OnFirstUse && m.state != ModelState::OnFirstUse => {
                    r.tools.insert(0, (i, m.what));
                    r.model = m;
                }
                Some(r) => r.tools.push((i, m.what)),
                None => rows.push(ModelRow { tools: vec![(i, m.what)], model: m }),
            }
        }
    }
    rows
}

/// The step is there when a model needs a download now or is already here.
fn has_models_step(app: &App) -> bool {
    models(app).iter().any(|r| r.model.state != crate::core::tool::ModelState::OnFirstUse)
}

fn models_step(app: &mut App, ui: &mut Ui, body: Rect, t: f32) {
    use crate::core::tool::ModelState;
    let p = ui.painter().clone();
    // a column in the middle of the step (left-aligned it left the right half empty)
    let w = body.width().min(COL_W);
    let x = body.center().x - w / 2.0;
    let head = body.top() + 6.0;
    let g = p.text(Pos2::new(x, head), Align2::LEFT_TOP, "Download models", bold(22.0), TEXT());
    let sub = text_wrap(&p, Pos2::new(x, g.bottom() + sp::LINE_GAP), Align2::LEFT_TOP, "They run on this Mac. Each one says which tool uses it. The download keeps going if you continue.", ty::body(), TEXT_DIM(), w, 3);
    let list = models(app);
    let mut y = sub.bottom() + sp::XL;
    let mut start: Option<(usize, &'static str)> = None;
    for (k, row) in list.iter().enumerate() {
        let (m, i) = (&row.model, row.tools[0].0);
        let a = ease((t - 0.05 * k as f32) / 0.4);
        // two lines (the model, then the tools it's for), a third for the bar or an error
        let third = matches!(m.state, ModelState::Downloading { .. } | ModelState::Unpacking | ModelState::Failed(_));
        let row_h = if third { 78.0 } else { 58.0 };
        let r = Rect::from_min_size(Pos2::new(x, y + (1.0 - a) * 8.0), Vec2::new(w, row_h));
        y += row_h + sp::S;
        glass_card(&p, r, R_CARD, 0.0, false);
        let ib = Rect::from_min_size(Pos2::new(r.left() + 14.0, r.top() + 14.0), Vec2::splat(30.0));
        p.rect_filled(ib, 8.0, white(16));
        icon(&p, app.tools[i].badge().icon, ib.shrink(7.0), if m.state == ModelState::Ready { GREEN() } else { TEXT_SOFT() });
        let tx = ib.right() + 12.0;
        let right = r.right() - 14.0;
        let line1 = r.top() + 19.0;
        let line2 = r.top() + 39.0;
        // a two-line row's status or button sits on its middle; with a
        // third line (the bar) it stays on the first
        let side = if third { line1 } else { r.center().y };
        let title = p.text(Pos2::new(tx, line1), Align2::LEFT_CENTER, m.name, bold(13.0), TEXT());
        p.text(Pos2::new(title.right() + 8.0, line1), Align2::LEFT_CENTER, human_bytes(m.bytes), font(12.0), TEXT_FAINT());
        // which tools use it, and for what
        let uses = row.tools.iter().map(|(ti, what)| format!("{}: {}", app.tools[*ti].name(), what.to_lowercase())).collect::<Vec<_>>().join(" · ");
        let status_w = 150.0;
        text_fit(&p, Pos2::new(tx, line2), Align2::LEFT_CENTER, &uses, font(12.0), TEXT_DIM(), right - tx - status_w);
        let bar = Rect::from_min_max(Pos2::new(tx, r.bottom() - 14.0), Pos2::new(right, r.bottom() - 8.0));
        let mut button: Option<&str> = None;
        match &m.state {
            ModelState::Ready => {
                let t = p.text(Pos2::new(right, side), Align2::RIGHT_CENTER, "On this Mac", font(12.0), GREEN());
                check(&p, Pos2::new(t.left() - 12.0, side), 12.0, GREEN());
            }
            ModelState::OnFirstUse => {
                p.text(Pos2::new(right, side), Align2::RIGHT_CENTER, "Downloads on first use", font(12.0), TEXT_FAINT());
            }
            ModelState::Downloading { done, total } => {
                let total = if *total > 0 { *total } else { m.bytes };
                let f = if total > 0 { (*done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                p.rect_filled(bar, 3.0, white(20));
                // the bar fills smoothly between progress reports, at the
                // decorative pace (egui's own easing asked for every frame)
                let f = glide(ui.ctx(), ui.scope_id().with(("setup-model", m.id)), f, 0.25);
                p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, bar.height())), 3.0, BLUE());
                p.text(Pos2::new(right, side), Align2::RIGHT_CENTER, format!("{} of {} · {:.0}%", human_bytes(*done), human_bytes(total), f * 100.0), font(12.0), TEXT_DIM());
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
            ModelState::Unpacking => {
                p.rect_filled(bar, 3.0, BLUE());
                p.text(Pos2::new(right, side), Align2::RIGHT_CENTER, "Unpacking", font(12.0), TEXT_DIM());
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            ModelState::Failed(e) => {
                text_fit(&p, Pos2::new(tx, bar.center().y), Align2::LEFT_CENTER, e, font(12.0), RED(), right - tx);
                button = Some("Try again");
            }
            ModelState::Missing => button = Some("Download"),
        }
        if let Some(label) = button {
            let bw = button_width(&p, label, BTN_SECONDARY);
            let b = Rect::from_min_size(Pos2::new(right - bw, side - 13.0), Vec2::new(bw, 26.0));
            if pill_button(ui, b, label, BTN_SECONDARY, true).clicked() {
                start = Some((i, m.id));
            }
        }
    }
    if let Some((i, id)) = start {
        if !app.demo {
            app.tools[i].download_model(id);
        }
    }
}

fn check(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let pts = vec![c + Vec2::new(-0.45, 0.0) * s, c + Vec2::new(-0.12, 0.33) * s, c + Vec2::new(0.45, -0.32) * s];
    p.add(egui::Shape::line(pts, Stroke::new((s * 0.22).max(1.2), col)));
}

/// [`check`] drawn up to `k` (0 → nothing, 1 → whole), short stroke first.
fn check_drawn(p: &egui::Painter, c: Pos2, s: f32, col: Color32, k: f32) {
    if k <= 0.0 {
        return;
    }
    let pts = [c + Vec2::new(-0.45, 0.0) * s, c + Vec2::new(-0.12, 0.33) * s, c + Vec2::new(0.45, -0.32) * s];
    let (l0, l1) = (pts[0].distance(pts[1]), pts[1].distance(pts[2]));
    let mut d = (l0 + l1) * k.min(1.0);
    let mut out = vec![pts[0]];
    for (a, b, l) in [(pts[0], pts[1], l0), (pts[1], pts[2], l1)] {
        if d >= l {
            out.push(b);
            d -= l;
        } else {
            out.push(a + (b - a) * (d / l));
            break;
        }
    }
    p.add(egui::Shape::line(out, Stroke::new((s * 0.22).max(1.2), col)));
}

fn permissions_step(app: &mut App, ui: &mut Ui, body: Rect, t: f32, now: f64) {
    let p = ui.painter().clone();
    let list = rows(app);
    {
        let (tools, watch) = (&app.tools, &mut app.perm_watch);
        watch.poll(tools, &list, now);
    }
    let req: Vec<&Row> = list.iter().filter(|r| !r.optional).collect();
    let done = req.iter().filter(|r| app.perm_watch.status(r.perm) == Status::Allowed).count();
    // progress ring
    let c = Pos2::new(body.left() + 22.0, body.top() + 22.0);
    crate::kit::glass::stroke_circle(&p, c, 18.0, Stroke::new(4.0, white(20)));
    let frac = if req.is_empty() { 1.0 } else { done as f32 / req.len() as f32 };
    let shown = ui.ctx().animate_value_with_time(ui.scope_id().with("perm-ring"), frac, 0.6);
    let full = done == req.len();
    if shown > 0.005 {
        let a0 = -std::f32::consts::FRAC_PI_2;
        p.add(egui::Shape::line(arc_points(c, 18.0, a0, a0 + std::f32::consts::TAU * shown, 48), Stroke::new(4.0, if full { GREEN() } else { BLUE() })));
    }
    // all required ones allowed: the count turns into a check and the ring
    // grows soft cat ears (it was full already when the step opened: no show)
    let full_id = ui.scope_id().with("perm-full");
    let full_at = if full {
        let at = ui.ctx().data(|d| d.get_temp::<f64>(full_id)).unwrap_or(if t < 0.1 { now - 10.0 } else { now });
        ui.ctx().data_mut(|d| d.insert_temp(full_id, at));
        Some(at)
    } else {
        ui.ctx().data_mut(|d| d.remove::<f64>(full_id));
        None
    };
    match full_at {
        Some(at) => {
            let since = (now - at) as f32;
            check(&p, c, 14.0 * pop(since / 0.35).max(0.0), GREEN());
            let z = pop((since - 0.25) / 0.4).max(0.0);
            if z > 0.0 {
                ring_ears(&p, c, z, GREEN());
            }
            if since < 0.8 {
                ui.ctx().request_repaint();
            }
        }
        // the ring's arc is the progress; no bare "1/2" inside it
        None => {}
    }
    p.text(Pos2::new(c.x + 36.0, c.y), Align2::LEFT_CENTER, "Permissions", bold(22.0), TEXT());
    let area = Rect::from_min_max(Pos2::new(body.left(), body.top() + 56.0), body.max);
    // rows at their smallest; past that the list scrolls (it never runs
    // under the Back / Continue bar)
    let head = if list.iter().any(|r| r.optional) { 26.0 } else { 0.0 };
    let need = list.len() as f32 * (38.0 + 6.0) + head;
    if need <= area.height() {
        perm_list(app, ui, area, &list, t, now);
    } else {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        child.set_clip_rect(area);
        egui::ScrollArea::vertical().id_salt("setup-perms").auto_shrink([false, false]).show(&mut child, |ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), need), Sense::hover());
            perm_list(app, ui, r, &list, t, now);
        });
    }
}

/// Two soft cat ears on top of the permissions ring (radius 18 round `c`),
/// grown to `z` (0 → none, 1 → full; a bit over 1 overshoots). Wide bases
/// and a rounded tip, so they read as ears, not horns.
fn ring_ears(p: &egui::Painter, c: Pos2, z: f32, col: Color32) {
    for side in [-1.0f32, 1.0] {
        let pt = |deg: f32, r: f32| {
            let a = deg.to_radians();
            c + Vec2::new(a.cos() * side, a.sin()) * r
        };
        // base sunk into the ring, tip out and up
        let (b0, b1, tip) = (pt(-150.0, 17.0), pt(-98.0, 17.0), pt(-123.0, 30.0));
        let mid = b0 + (b1 - b0) * 0.5;
        let tip = mid + (tip - mid) * z;
        // the tip's corner rounded off: a curve between points `round` from it
        let round = 8.0 * z.min(1.0);
        let (t1, t2) = (tip + (b0 - tip).normalized() * round, tip + (b1 - tip).normalized() * round);
        let mut pts = vec![b0];
        pts.extend((0..=8).map(|i| {
            let u = i as f32 / 8.0;
            let v = 1.0 - u;
            Pos2::new(v * v * t1.x + 2.0 * u * v * tip.x + u * u * t2.x, v * v * t1.y + 2.0 * u * v * tip.y + u * u * t2.y)
        }));
        pts.push(b1);
        // same winding on both sides
        if side > 0.0 {
            pts.reverse();
        }
        p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    }
}

/// The permission rows (setup and Settings › General). Returns the height used.
pub fn perm_list(app: &mut App, ui: &mut Ui, area: Rect, list: &[Row], t: f32, now: f64) -> f32 {
    let p = ui.painter().clone();
    let mut y = area.top();
    let mut opt_head = false;
    // rows shrink a little when the list wouldn't fit (8 permissions + Optional)
    let n = list.len().max(1) as f32;
    let head = if list.iter().any(|r| r.optional) { 26.0 } else { 0.0 };
    let row_h = ((area.height() - head - 6.0 * n) / n).clamp(38.0, 46.0);
    let mut ask: Option<Perm> = None;
    for (k, r) in list.iter().enumerate() {
        if r.optional && !opt_head {
            opt_head = true;
            // a section title like the others (26 in all: 4 above, the title, 7 below)
            y += sp::XS;
            p.text(Pos2::new(area.left() + 4.0, y), Align2::LEFT_TOP, "Optional", ty::section(), white(150));
            y += 22.0;
        }
        let a = ease((t - 0.05 * k as f32) / 0.4);
        let rr = Rect::from_min_size(Pos2::new(area.left(), y - (1.0 - a) * 10.0), Vec2::new(area.width(), row_h));
        let st = app.perm_watch.status(r.perm);
        let waiting = app.perm_watch.waiting(r.perm, now);
        let ok = st == Status::Allowed;
        p.rect_filled(rr, 10.0, CARD().gamma_multiply(a));
        // just allowed: the row flashes green once and its check draws itself
        let since = app.perm_watch.allowed_at.get(&r.perm).map(|t0| (now - t0) as f32).unwrap_or(10.0);
        if since < 0.8 {
            p.rect_filled(rr, 10.0, with_alpha(GREEN(), 0.2 * (1.0 - ease(since / 0.8))));
        }
        let ib = Rect::from_min_size(Pos2::new(rr.left() + 12.0, rr.center().y - 15.0), Vec2::splat(30.0));
        let tint = match st {
            Status::Allowed => GREEN(),
            Status::Denied => AMBER(),
            Status::NotAsked => TEXT_SOFT(),
        };
        p.rect_filled(ib, 8.0, white(16));
        icon(&p, r.perm.icon(), ib.shrink(7.0), tint);
        let name = if r.locked && !ok { format!("Needs {}", r.perm.name()) } else { r.perm.name().to_string() };
        p.text(Pos2::new(ib.right() + 12.0, rr.center().y), Align2::LEFT_CENTER, name, bold(13.0), TEXT().gamma_multiply(a));
        // status on the right
        let right = rr.right() - 12.0;
        // why it's asked, in one column (the status keeps its room); the
        // tools that want it are named on hover, not drawn as a row of icons
        let why_x = ib.right() + 190.0;
        if !r.tools.is_empty() {
            let names: Vec<&str> = r.tools.iter().map(|&i| app.tools[i].name()).collect();
            let hr = Rect::from_min_max(Pos2::new(ib.left(), rr.top()), Pos2::new(right - 210.0, rr.bottom()));
            ui.interact(hr, ui.scope_id().with(("perm-used-by", k)), Sense::hover()).on_hover_text(format!("Used by {}", names.join(", ")));
        }
        let why_w = right - 210.0 - why_x;
        if why_w > 60.0 && !r.locked && !r.perm.why().is_empty() {
            text_fit(&p, Pos2::new(why_x, rr.center().y), Align2::LEFT_CENTER, r.perm.why(), font(12.0), TEXT_FAINT().gamma_multiply(a), why_w);
        }
        if ok {
            let c = Pos2::new(right - 13.0, rr.center().y);
            check_drawn(&p, c, 12.0, GREEN(), ease(since / 0.35));
        } else if waiting {
            let c = Pos2::new(right - 8.0, rr.center().y);
            spinner(&p, c, 6.0, now, BLUE());
            // a tool's own step (an admin prompt, a download) isn't in System Settings
            let hint = match r.perm {
                Perm::FullDisk => "Switch it on in System Settings",
                Perm::Own { .. } => "",
                _ => "System Settings",
            };
            // macOS asks right here; Settings isn't involved
            let hint = if r.perm.answered_in_place() { "" } else { hint };
            p.text(Pos2::new(c.x - 16.0, c.y), Align2::RIGHT_CENTER, hint, font(12.0), TEXT_FAINT());
        } else {
            let err = app.perm_watch.own_err.get(&r.perm).filter(|e| !e.is_empty());
            let (label, kind) = if err.is_some() {
                ("Try again", BTN_SECONDARY)
            } else if st == Status::Denied {
                ("Open Settings", BTN_SECONDARY)
            } else {
                (r.perm.ask_label(), BTN_SECONDARY)
            };
            let w = button_width(&p, label, kind);
            let b = Rect::from_min_size(Pos2::new(right - w, rr.center().y - 13.0), Vec2::new(w, 26.0));
            if let Some(e) = err {
                text_fit(&p, Pos2::new(b.left() - 10.0, rr.center().y), Align2::RIGHT_CENTER, e, font(12.0), RED(), 260.0);
            }
            if pill_button(ui, b, label, kind, true).clicked() {
                ask = Some(r.perm);
            }
        }
        y += row_h + 6.0;
    }
    if let Some(perm) = ask {
        match perm {
            Perm::Own { id, .. } => {
                app.perm_watch.asked.insert(perm, now);
                app.perm_watch.polled = 0.0;
                if app.perm_watch.demo.is_none() {
                    if let Some(&i) = list.iter().find(|r| r.perm == perm).and_then(|r| r.tools.first()) {
                        app.tools[i].ask_own(id);
                    }
                }
            }
            _ => app.perm_watch.ask(perm, now),
        }
    }
    y - area.top()
}

/// A shortcut's caps: modifiers are one char each; the key is the rest.
fn caps(keys: &str) -> Vec<String> {
    let mods: String = keys.chars().take_while(|c| "⌃⌥⇧⌘".contains(*c)).collect();
    let key: String = keys.chars().skip(mods.chars().count()).collect();
    let mut v: Vec<String> = mods.chars().map(|c| c.to_string()).collect();
    v.push(key);
    v
}

fn cap_w(p: &egui::Painter, c: &str) -> f32 {
    p.layout_no_wrap(c.to_string(), font(13.0), TEXT()).size().x.max(10.0) + 12.0
}

fn keycaps(p: &egui::Painter, right: Pos2, keys: &str) {
    let mut x = right.x;
    let caps = caps(keys);
    for c in caps.iter().rev() {
        let f: FontId = font(13.0);
        let g = p.layout_no_wrap(c.clone(), f, TEXT());
        let w = cap_w(p, c);
        let r = Rect::from_min_size(Pos2::new(x - w, right.y - 11.0), Vec2::new(w, 22.0));
        p.rect_filled(r, 5.0, white(26));
        p.rect_filled(Rect::from_min_max(Pos2::new(r.left() + 1.0, r.bottom() - 1.5), Pos2::new(r.right() - 1.0, r.bottom())), CornerRadius::same(2), Color32::from_black_alpha(90));
        // center the glyph's ink, not the font's line box (symbols sat low);
        // the cap's bottom edge takes 1.5 pt, so aim a hair above the middle
        let ink = g.mesh_bounds;
        let target = r.center() - Vec2::new(0.0, 0.75);
        let pos = Pos2::new(r.center().x - g.size().x / 2.0, target.y - ink.center().y);
        p.galley(pos, g, TEXT());
        x = r.left() - 3.0;
    }
}

/// The Ready step's cat, standing on `feet`: it pops in, crouches and
/// bursts the chosen tools out round it, which arc away and fall.
fn celebrate(app: &App, p: &egui::Painter, feet: Pos2, t: f32) {
    use std::f32::consts::PI;
    let size = 56.0;
    let burst = 0.5;
    let z = pop(t / 0.35).max(0.0);
    // crouch before the burst, stretch on it, then settle
    let sq = if t < burst { 0.12 * ease((t - 0.3) / (burst - 0.3)) } else { -0.08 * (1.0 - ease((t - burst) / 0.3)) };
    let (w, h) = (size * z * (1.0 + sq), size * z * (1.0 - sq));
    let cat = Rect::from_min_max(Pos2::new(feet.x - w / 2.0, feet.y - h), Pos2::new(feet.x + w / 2.0, feet.y));
    let c = Pos2::new(feet.x, feet.y - size / 2.0);
    // a ring pushed out by the burst
    let u = (t - burst) / 0.45;
    if (0.0..1.0).contains(&u) {
        crate::kit::glass::stroke_circle(p, c, 24.0 + 52.0 * ease(u), Stroke::new(2.0, with_alpha(BLUE(), 0.6 * (1.0 - u))));
    }
    let perk = if t > burst { 0.35 * (1.0 - ease((t - burst) / 0.5)) } else { 0.0 };
    if z > 0.0 {
        crate::kit::brand::draw_mascot(p, cat, cat_color(), (perk + twitch(t, 1.6), perk));
    }
    // the chosen tools fly out in a fan over the top and drop
    let on: Vec<usize> = (0..app.tools.len()).filter(|&i| app.tool_on(i)).collect();
    let n = on.len().max(1) as f32;
    let fly = (t - burst) / 1.0;
    if !(0.0..1.0).contains(&fly) {
        return;
    }
    for (k, &i) in on.iter().enumerate() {
        // from straight left over the top to straight right, a little uneven
        let a = PI + (k as f32 + 0.5) / n * PI + ((k * 7 % 5) as f32 - 2.0) * 0.06;
        let dist = 120.0 + ((k * 37) % 5) as f32 * 18.0;
        let out = ease((fly * 1.8).min(1.0));
        let at = c + Vec2::new(a.cos(), a.sin()) * dist * out + Vec2::new(0.0, 140.0 * fly * fly);
        let s = pop(fly / 0.18).max(0.0);
        let fade = 1.0 - ((fly - 0.55) / 0.45).clamp(0.0, 1.0);
        let lime = k % 2 == 0;
        let r = 12.0 * s;
        p.circle_filled(at, r, (if lime { BLUE() } else { POPOVER() }).gamma_multiply(fade));
        if !lime {
            crate::kit::glass::stroke_circle(p, at, r, Stroke::new(1.0, white(36).gamma_multiply(fade)));
        }
        icon(p, app.tools[i].badge().icon, Rect::from_center_size(at, Vec2::splat(12.0 * s)), (if lime { ON_ACTION } else { TEXT_SOFT() }).gamma_multiply(fade));
    }
}

/// Returns true when "Restart" next to Screen Recording was clicked.
fn ready(app: &mut App, ui: &mut Ui, body: Rect, t: f32) -> bool {
    let mut restart = false;
    let p = ui.painter().clone();
    let left = Rect::from_min_max(body.min, Pos2::new(body.right() - 340.0, body.bottom()));
    let mut y = body.center().y - 120.0;
    p.text(Pos2::new(left.left(), y), Align2::LEFT_TOP, "Ready", bold(22.0), TEXT());
    celebrate(app, &p, Pos2::new(left.left() + 28.0, y - 14.0), t);
    y += 46.0;
    // open at login
    let row = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(left.width(), 52.0));
    p.rect_filled(row, 10.0, CARD());
    icon(&p, Icon::Power, Rect::from_min_size(Pos2::new(row.left() + 14.0, row.center().y - 9.0), Vec2::splat(18.0)), TEXT_SOFT());
    p.text(Pos2::new(row.left() + 44.0, row.center().y), Align2::LEFT_CENTER, "Open at login", bold(13.0), TEXT());
    let now = ui.ctx().input(|i| i.time);
    // on by default; registered when setup finishes
    let on = crate::kit::login::default_pending() || app.perm_watch.login_on(app.demo, now);
    let sw = Rect::from_min_size(Pos2::new(row.right() - 50.0, row.center().y - 11.0), Vec2::new(38.0, 22.0));
    if switch(ui, sw, ui.scope_id().with("setup-login"), on, true).clicked() && !app.demo {
        app.perm_watch.set_login(!on);
    }
    y += 62.0;
    // shortcuts of the enabled tools
    let keys: Vec<(Icon, &'static str, String)> = (0..app.tools.len()).filter(|&i| app.tool_on(i)).flat_map(|i| {
        let ic = app.tools[i].badge().icon;
        app.tools[i].setup_shortcuts().into_iter().map(move |(l, k)| (if l.starts_with("Record") { Icon::ScreenRec } else { ic }, l, k))
    }).collect();
    for (k, (ic, label, keys)) in keys.iter().enumerate() {
        let a = ease((t - 0.08 * (k + 1) as f32) / 0.4);
        let row = Rect::from_min_size(Pos2::new(left.left(), y - (1.0 - a) * 8.0), Vec2::new(left.width(), 44.0));
        p.rect_filled(row, 10.0, CARD().gamma_multiply(a));
        icon(&p, *ic, Rect::from_min_size(Pos2::new(row.left() + 14.0, row.center().y - 9.0), Vec2::splat(18.0)), TEXT_SOFT().gamma_multiply(a));
        p.text(Pos2::new(row.left() + 44.0, row.center().y), Align2::LEFT_CENTER, *label, font(13.0), TEXT().gamma_multiply(a));
        keycaps(&p, Pos2::new(row.right() - 12.0, row.center().y), keys);
        y += 52.0;
    }
    if needs_restart(app) {
        let row = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(left.width(), 44.0));
        p.rect_filled(row, 10.0, CARD());
        icon(&p, Icon::ScreenRec, Rect::from_min_size(Pos2::new(row.left() + 14.0, row.center().y - 9.0), Vec2::splat(18.0)), AMBER());
        p.text(Pos2::new(row.left() + 44.0, row.center().y), Align2::LEFT_CENTER, "Screen Recording needs a restart", font(13.0), TEXT());
        let bw = button_width(&p, "Restart", BTN_PRIMARY);
        let br = Rect::from_min_size(Pos2::new(row.right() - 10.0 - bw, row.center().y - 13.0), Vec2::new(bw, 26.0));
        if pill_button(ui, br, "Restart", BTN_PRIMARY, true).clicked() {
            restart = true;
        }
    }
    // a little panel builds itself from the chosen tools, under the menu bar icon
    let pw = 300.0;
    let px = body.right() - pw;
    let on_tools: Vec<usize> = (0..app.tools.len()).filter(|&i| app.tool_on(i)).collect();
    let cell = (pw - 20.0 - 3.0 * 8.0) / 4.0;
    let spans: Vec<usize> = on_tools.iter().map(|&i| if matches!(app.tools[i].id(), "cleanup" | "system") { 2 } else { 1 }).collect();
    let mut rows_n = 0;
    {
        let mut col = 0;
        for s in &spans {
            if col + s > 4 {
                rows_n += 1;
                col = 0;
            }
            col += s;
        }
        if col > 0 {
            rows_n += 1;
        }
    }
    let ph = 20.0 + rows_n as f32 * 58.0 + (rows_n.max(1) as f32 - 1.0) * 8.0;
    let top = body.center().y - ph / 2.0 + 24.0;
    // a strip of the menu bar with the app icon lit, the panel hanging under it
    let bar = Rect::from_min_size(Pos2::new(px, top - 40.0), Vec2::new(pw, 24.0));
    p.rect_filled(bar, 6.0, white(18));
    let mut bx = bar.right() - 12.0;
    let clock = p.text(Pos2::new(bx, bar.center().y), Align2::RIGHT_CENTER, "Tue 17:58", font(11.0), TEXT_FAINT());
    bx = clock.left() - 14.0;
    for ic in [Icon::Wifi, Icon::Battery(80)] {
        icon(&p, ic, Rect::from_center_size(Pos2::new(bx - 7.0, bar.center().y), Vec2::splat(13.0)), TEXT_FAINT());
        bx -= 24.0;
    }
    let me = Rect::from_center_size(Pos2::new(bx - 8.0, bar.center().y), Vec2::new(26.0, 20.0));
    let glow = ((t * 3.0).sin() * 0.5 + 0.5) * 0.25 + 0.2;
    p.rect_filled(me, 5.0, with_alpha(BLUE(), glow));
    brand_glyph(&p, me.center(), 15.0, Color32::WHITE);
    p.text(Pos2::new(bar.left() + 10.0, bar.center().y), Align2::LEFT_CENTER, "Click this icon", font(11.0), TEXT_FAINT());
    // a little caret from the icon to the panel
    let c = Pos2::new(me.center().x, top);
    p.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-6.0, 0.5), c + Vec2::new(0.0, -6.0), c + Vec2::new(6.0, 0.5)], BG(), Stroke::NONE));
    let panel = Rect::from_min_size(Pos2::new(px, top), Vec2::new(pw, ph));
    p.rect_filled(panel, 12.0, BG());
    p.rect_stroke(panel, 12.0, Stroke::new(1.0, white(36)), StrokeKind::Inside);
    let (mut col, mut row) = (0usize, 0usize);
    for (k, (&i, &s)) in on_tools.iter().zip(&spans).enumerate() {
        if col + s > 4 {
            col = 0;
            row += 1;
        }
        let a = pop((t - 0.3 - 0.09 * k as f32) / 0.45);
        let r = Rect::from_min_size(Pos2::new(panel.left() + 10.0 + col as f32 * (cell + 8.0), panel.top() + 10.0 + row as f32 * 66.0 - (1.0 - a.min(1.0)) * 14.0), Vec2::new(cell * s as f32 + 8.0 * (s as f32 - 1.0), 58.0));
        col += s;
        if a <= 0.0 {
            continue;
        }
        let blue = false;
        p.rect_filled(r, 9.0, (if blue { BLUE() } else { SIDEBAR() }).gamma_multiply(a.min(1.0)));
        let fg = if blue { ON_ACTION } else { TEXT_SOFT() };
        // the whole name (R10): on two lines when one is too narrow, the icon moves up
        let name = app.tools[i].name();
        let two = p.layout_no_wrap(name.to_string(), font(11.0), fg).size().x > r.width() - 8.0;
        let icon_up = if two { 14.0 } else { 8.0 };
        icon(&p, app.tools[i].badge().icon, Rect::from_center_size(r.center() - Vec2::new(0.0, icon_up), Vec2::splat(18.0 * a)), fg);
        text_wrap(&p, Pos2::new(r.center().x, r.bottom() - if two { 5.0 } else { 9.0 }), Align2::CENTER_BOTTOM, name, font(11.0), fg.gamma_multiply(0.85 * a.min(1.0)), r.width() - 8.0, 2);
    }
    restart
}

// ---------------------------------------------------------------- System Settings helper

const HELP_W: f32 = 360.0;
const HELP_H: f32 = 132.0;
/// The permissions switched on by hand in a System Settings list.
const HELPED: [Perm; 3] = [Perm::FullDisk, Perm::Accessibility, Perm::Screen];

/// While one of [`HELPED`] is being set up, a small card stands beside
/// System Settings showing what to do: find the app in the list and switch
/// it on. It floats so Settings can't cover it, so it keeps off Settings
/// itself and steps aside while macOS shows a prompt (the password sheet,
/// "would like to record this screen") instead of hiding it.
pub fn perm_helper(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let Some(perm) = HELPED.into_iter().find(|&p| app.perm_watch.waiting(p, now) && (app.demo || app.perm_watch.status(p) != Status::Allowed)) else {
        return;
    };
    let size = Vec2::new(HELP_W, HELP_H);
    let scene = crate::kit::settings_window::scene();
    let at = match (scene.settings, scene.screen) {
        _ if scene.prompt => None,
        (Some(set), Some(screen)) => Some(crate::kit::settings_window::place(set, screen, size)),
        // a demo has no Settings window to stand by
        _ if app.demo => {
            let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1512.0, 982.0));
            Some(((screen - size) / 2.0).to_pos2())
        }
        // Settings isn't up yet (or is closed): nothing to point at
        _ => None,
    };
    let id = egui::ViewportId::from_hash_of("cmdmeow-fda-help");
    let Some(at) = at else {
        // not drawn this pass: the window closes, and opens again in place
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        return;
    };
    // Settings was moved: follow it
    let was = ctx.input(|i| i.raw.viewports.get(&id).and_then(|v| v.outer_rect));
    if was.is_some_and(|r| (r.min - at).length() > 1.0) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(at));
    }
    let builder = egui::ViewportBuilder::default()
        .with_title(perm.name())
        .with_inner_size(size)
        .with_position(at)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_has_shadow(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop);
    let mut close = false;
    crate::kit::viewport::show(ctx, id, builder, |ui, _class| {
        close = help_card(ui, ui.max_rect(), now, perm);
    });
    if close {
        app.perm_watch.asked.remove(&perm);
    }
}

/// The helper's card. Returns true when its close button was clicked.
pub fn help_card(ui: &mut Ui, full: Rect, now: f64, perm: Perm) -> bool {
    let p = ui.painter().clone();
    p.rect_filled(full, 12.0, BG());
    p.rect_stroke(full, 12.0, Stroke::new(1.0, white(40)), StrokeKind::Inside);
    let name = perms::app_name();
    // a copy of the Settings row, its switch flipping on and off
    let row = Rect::from_min_size(full.min + Vec2::new(14.0, 14.0), Vec2::new(full.width() - 28.0, 40.0));
    p.rect_filled(row, 8.0, white(14));
    app_mark(&p, Pos2::new(row.left() + 20.0, row.center().y), 22.0, 0.75);
    p.text(Pos2::new(row.left() + 40.0, row.center().y), Align2::LEFT_CENTER, &name, font(13.0), TEXT());
    let cycle = (now % 2.4) as f32 / 2.4;
    let on = ease(((cycle - 0.25) / 0.25).clamp(0.0, 1.0)) * (1.0 - ease(((cycle - 0.85) / 0.15).clamp(0.0, 1.0)));
    let sw = Rect::from_min_size(Pos2::new(row.right() - 50.0, row.center().y - 11.0), Vec2::new(38.0, 22.0));
    p.rect_filled(sw, 11.0, lerp_color(white(46), BLUE(), on));
    p.circle_filled(Pos2::new(sw.left() + 11.0 + 16.0 * on, sw.center().y), 9.0, Color32::WHITE);
    // a pointer that clicks the switch
    let tip = sw.center() + Vec2::new(4.0, 6.0) + Vec2::new(14.0, 12.0) * (1.0 - ease((cycle / 0.25).clamp(0.0, 1.0)));
    let arrow = vec![tip, tip + Vec2::new(0.0, 13.0), tip + Vec2::new(3.5, 9.5), tip + Vec2::new(8.5, 10.0)];
    p.add(egui::Shape::convex_polygon(arrow, Color32::WHITE, Stroke::new(1.0, Color32::BLACK)));
    let y = row.bottom() + 14.0;
    p.text(Pos2::new(full.left() + 16.0, y), Align2::LEFT_TOP, format!("Switch on {name} in the list"), bold(13.0), TEXT());
    let more = match perm {
        Perm::FullDisk => "Not there? Drag it in from the Finder window.",
        _ => "macOS may ask for your password next.",
    };
    p.text(Pos2::new(full.left() + 16.0, y + 20.0), Align2::LEFT_TOP, more, font(12.0), TEXT_FAINT());
    let x = Rect::from_min_size(Pos2::new(full.right() - 26.0, full.bottom() - 26.0), Vec2::splat(18.0));
    let r = ui.interact(x, ui.scope_id().with("fda-help-close"), Sense::click());
    icon(&p, Icon::Close, x.shrink(3.0), if r.hovered() { TEXT() } else { TEXT_MUTED() });
    // it can stay up for minutes, and each of its frames redraws setup too:
    // move at the decorative pace, and sleep while the switch holds still
    let wait = help_wait(cycle).map_or(std::time::Duration::from_secs_f32(1.0 / 30.0), std::time::Duration::from_secs_f32);
    ui.ctx().request_repaint_after(wait);
    ui.ctx().request_repaint_after_for(wait, egui::ViewportId::ROOT);
    r.clicked()
}

/// Seconds the help card's loop holds still from `cycle` (0..1 of its
/// 2.4 s), or None while the pointer or the switch moves.
fn help_wait(cycle: f32) -> Option<f32> {
    // the pointer comes in (0..0.25), the switch flips on (..0.5), holds,
    // and flips back off (0.85..1)
    (0.5..0.85).contains(&cycle).then(|| (0.85 - cycle) * 2.4)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_cat_rises_smoothly_and_comes_to_rest() {
        let (rise, _, _) = peek(0.0);
        assert!(rise <= -96.0, "hidden below the floor at first: {rise}");
        let (rise, _, _) = peek(0.6);
        assert!((-80.0..-60.0).contains(&rise), "only the ears show while it looks round: {rise}");
        assert_eq!(peek(2.0), (0.0, 1.0, 1.0), "at rest, unsquashed");
        let mut last = peek(0.0).0;
        for i in 1..300 {
            let r = peek(i as f32 * 0.01).0;
            assert!((r - last).abs() < 15.0, "a jump of {} at {:.2}s", r - last, i as f32 * 0.01);
            last = r;
        }
    }

    #[test]
    fn goals_cover_every_tool_but_fan_once() {
        let env = crate::core::tool::ToolEnv { ctx: egui::Context::default(), demo: true };
        let ids: Vec<&str> = crate::tools::all(&env).iter().map(|t| t.id()).collect();
        let mut seen: Vec<&str> = Vec::new();
        for g in &GOALS {
            for id in g.tools {
                assert!(ids.contains(id), "{} lists unknown tool {id}", g.name);
                assert!(!seen.contains(id), "{id} is in two goals");
                seen.push(id);
            }
        }
        let left: Vec<&&str> = ids.iter().filter(|id| !seen.contains(id)).collect();
        assert_eq!(left, vec![&crate::tools::fan::ID]);
    }

    #[test]
    fn fresh_install_starts_with_every_tool_off() {
        let env = crate::core::tool::ToolEnv { ctx: egui::Context::default(), demo: true };
        let all = crate::tools::all(&env);
        let off = off_at_first(all.iter().map(|t| t.id()));
        assert_eq!(off.len(), all.len(), "the goals start unpicked");
    }

    #[test]
    fn an_answered_macos_question_brings_setup_back() {
        let mut w = PermWatch::default();
        w.apply(Perm::Camera, Status::NotAsked, 0.0);
        w.asked.insert(Perm::Camera, 1.0);
        w.apply(Perm::Camera, Status::NotAsked, 1.5);
        assert!(!w.take_refocus(), "still asking");
        w.apply(Perm::Camera, Status::Allowed, 2.0);
        assert!(w.take_refocus());
        assert!(!w.take_refocus(), "once");
        // Full Disk Access is switched on in System Settings: leave it in front
        w.asked.insert(Perm::FullDisk, 3.0);
        w.apply(Perm::FullDisk, Status::Allowed, 4.0);
        assert!(!w.take_refocus());
    }

    #[test]
    fn an_unanswered_ask_stops_drawing_after_a_while() {
        let mut w = PermWatch::default();
        w.apply(Perm::Accessibility, Status::NotAsked, 0.0);
        w.asked.insert(Perm::Accessibility, 1.0);
        assert!(!w.busy(2.0), "a wait is not every frame");
        assert!(w.spinning(2.0));
        assert!(!w.spinning(62.0), "given up on");
        assert!(!w.busy(62.0));
    }

    #[test]
    fn a_goal_loop_runs_only_under_the_pointer() {
        let ctx = egui::Context::default();
        let id = egui::Id::NULL.with("loop");
        assert_eq!(loop_clock(&ctx, id, 10.0, false), 2.0, "rests at the screenshot pose");
        assert_eq!(loop_clock(&ctx, id, 11.0, false), 2.0, "holds while not hovered");
        let t = loop_clock(&ctx, id, 11.05, true);
        assert!((t - 2.05).abs() < 1e-4, "{t}");
        // a long gap (window hidden) moves it a frame's worth at most
        let t2 = loop_clock(&ctx, id, 20.0, true);
        assert!((t2 - t - 0.1).abs() < 1e-4, "{t2}");
        assert_eq!(loop_clock(&ctx, id, 30.0, false), t2, "stops where it was");
    }

    #[test]
    fn keycaps_split_modifiers_from_the_key() {
        assert_eq!(caps("⌘⇧2"), vec!["⌘", "⇧", "2"]);
        assert_eq!(caps("⌥Space"), vec!["⌥", "Space"]);
    }

    #[test]
    fn the_help_card_sleeps_while_its_switch_holds() {
        assert_eq!(help_wait(0.1), None, "pointer moving");
        assert_eq!(help_wait(0.4), None, "switch flipping");
        assert!((help_wait(0.5).unwrap() - 0.84).abs() < 1e-3);
        assert_eq!(help_wait(0.9), None, "switch flipping back");
    }

    #[test]
    fn welcome_waits_then_ears_shake_then_the_cat_jumps() {
        // still, nothing showing, while the window settles on screen
        assert!(welcome_cat(0.0).0 .0 <= -96.0 && welcome_cat(WELCOME_WAIT - 0.01).0 .0 <= -96.0);
        assert_eq!(welcome_cat(0.3).1, (0.0, 0.0));
        // just the ears, shaking
        let mid = WELCOME_WAIT + 0.9;
        assert!((-80.0..-60.0).contains(&welcome_cat(mid).0 .0));
        let most = (0..60).map(|i| welcome_cat(WELCOME_WAIT + 0.4 + i as f32 * 0.01).1 .0).fold(0.0f32, f32::max);
        assert!(most > 0.1, "{most}");
        // then up, and at rest by the end
        assert_eq!(welcome_cat(WELCOME_END).0, (0.0, 1.0, 1.0));
        let mut last = welcome_cat(0.0).0 .0;
        for i in 1..400 {
            let r = welcome_cat(i as f32 * 0.01).0 .0;
            assert!((r - last).abs() < 15.0, "a jump of {} at {:.2}s", r - last, i as f32 * 0.01);
            last = r;
        }
    }

    #[test]
    fn ear_flick_is_short_and_small() {
        assert_eq!(twitch(0.9, 1.0), 0.0);
        assert_eq!(twitch(1.4, 1.0), 0.0);
        let most = (0..36).map(|i| twitch(1.0 + i as f32 * 0.01, 1.0)).fold(0.0f32, f32::max);
        assert!(most > 0.1 && most <= 0.3, "{most}");
    }
}
