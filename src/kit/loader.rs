//! The removal loader: a card over the whole window while something slow is
//! removed (Cleanup, Disk Map, Uninstaller, Leftovers, Startup, the
//! dictation model, Remove Cmd+Meow). One look everywhere: the mark fills with lime from the
//! bottom (ears last), a title ("Cleaning…"), "x of y", the path being
//! removed and Stop where stopping is safe. When it's done the ⌘ cut turns
//! into a ✓.
//!
//! It shows only if the work takes longer than [`DELAY`], and once shown it
//! stays at least [`MIN_SHOWN`], so quick removals never flash. Everything
//! under it is dimmed and takes no hover, tooltip or click. The card is a
//! floating surface: Whisper shadow, popover edge (ui-rules P1–P5).
//!
//! The mark's fill, wave and ⌘→✓ are Core Animation layers (`spin::mark`);
//! egui draws only the card and its text, when progress changes.

use crate::kit::glass::{self, *};
use egui::{Align2, Color32, CornerRadius, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

/// How long a job runs before the loader shows.
pub const DELAY: f64 = 0.4;
/// Once shown, the loader stays at least this long.
pub const MIN_SHOWN: f64 = 0.8;

const W: f32 = 340.0;
const PAD: f32 = sp::XL;
const MARK: f32 = 88.0;
const R: f32 = 14.0;
/// The app window's corner radius.
const WINDOW_R: u8 = 12;
const TITLE_H: f32 = 20.0;
const COUNT_H: f32 = 18.0;
const CUR_H: f32 = 16.0;
const BTN_H: f32 = sp::CONTROL_H;

/// What the counter line counts.
#[derive(Clone, Debug, PartialEq)]
pub enum Count {
    /// "4.20 GB of 8.90 GB"
    Bytes { done: u64, total: u64 },
    /// No counter: the mark sweeps instead of filling.
    Unknown,
}

impl Count {
    fn frac(&self) -> Option<f32> {
        match *self {
            Count::Bytes { done, total } if total > 0 => Some((done as f64 / total as f64).clamp(0.0, 1.0) as f32),
            Count::Bytes { .. } => Some(0.0),
            Count::Unknown => None,
        }
    }
    fn line(&self) -> String {
        match *self {
            Count::Bytes { done, total } => {
                format!("{} of {}", human_bytes(done.min(total)), human_bytes(total))
            }
            Count::Unknown => String::new(),
        }
    }
}

/// 2400 → "2,400".
pub fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Where a job is.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// "Cleaning…", the counter, the path being removed now.
    Working { title: String, count: Count, current: String, can_stop: bool },
    /// "Moved 8.90 GB to the Trash", "340 items", what couldn't be done.
    Finished { title: String, sub: String, problem: Option<String>, put_back: bool },
    /// Stopped by the person: what was done until then.
    Stopped { sub: String },
    /// Nothing was removed: the empty mark (no ✓), the reason in amber.
    Failed { title: String, problem: String },
}

/// What the person clicked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Click {
    Stop,
    /// Done: the caller clears its job; the loader forgets it too.
    Done,
    PutBack,
    /// The job ended before the loader had to show: the caller clears it
    /// and shows its result the quiet way (a toast, its page).
    Ended,
}

/// Timing of one job, kept per key between frames.
#[derive(Clone, Debug)]
struct Timing {
    started: f64,
    shown: Option<f64>,
    /// The last working state, drawn until [`MIN_SHOWN`] has passed.
    last: Option<Status>,
}

/// What to draw this frame.
#[derive(Debug, PartialEq)]
enum Plan {
    /// Not yet (or never): ask again in this many seconds.
    Wait(f64),
    Draw,
    /// Ended before it ever showed.
    Skip,
}

fn plan(t: &mut Timing, now: f64, working: bool) -> Plan {
    match t.shown {
        None if working => {
            let left = DELAY - (now - t.started);
            if left > 0.0 {
                Plan::Wait(left)
            } else {
                t.shown = Some(now);
                Plan::Draw
            }
        }
        None => Plan::Skip,
        Some(_) => Plan::Draw,
    }
}

fn key_id(key: &str) -> egui::Id {
    egui::Id::NULL.with(("cmdmeow-loader", key))
}

/// Forget a job's timing (a new run of the same job starts over).
pub fn reset(ctx: &egui::Context, key: &str) {
    ctx.data_mut(|d| d.remove::<Timing>(key_id(key)));
}

/// Draw the loader for job `key` over the whole window, centred on `page`
/// (the tool's page, right of the sidebar). Call it every frame while the
/// job exists, finished ones included, until it returns [`Click::Done`].
pub fn show(ui: &mut Ui, page: Rect, key: &str, status: &Status) -> Option<Click> {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let id = key_id(key);
    let mut t = ctx.data(|d| d.get_temp::<Timing>(id)).unwrap_or(Timing { started: now, shown: None, last: None });
    let working = matches!(status, Status::Working { .. });
    if working {
        t.last = Some(status.clone());
    }
    let p = plan(&mut t, now, working);
    let shown_for = t.shown.map(|s| now - s).unwrap_or(0.0);
    match p {
        Plan::Wait(left) => {
            ctx.data_mut(|d| d.insert_temp(id, t));
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.01)));
            return None;
        }
        Plan::Skip => {
            reset(&ctx, key);
            return Some(Click::Ended);
        }
        Plan::Draw => {}
    }
    // a quick finish keeps the working look until it was seen long enough
    let early = !working && shown_for < MIN_SHOWN;
    let drawn = match (&t.last, early) {
        (Some(last), true) => {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(MIN_SHOWN - shown_for + 0.01));
            finish_count(last)
        }
        _ => status.clone(),
    };
    ctx.data_mut(|d| d.insert_temp(id, t));
    let click = card(&ctx, page, key, &drawn);
    if click == Some(Click::Done) || click == Some(Click::PutBack) {
        reset(&ctx, key);
    }
    let _ = ui;
    click
}

/// Test hook: `CMDMEOW_SLOW_REMOVE=<ms>` waits that long before each item a
/// loader job removes, so the loader can be checked on a few test files.
pub fn test_pause() {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let ms = *MS.get_or_init(|| std::env::var("CMDMEOW_SLOW_REMOVE").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// A job with no counter, run on a worker thread: "Removing x…" until the
/// closure returns the finished state.
pub struct Task {
    key: &'static str,
    working: Status,
    out: std::sync::Arc<std::sync::Mutex<Option<Status>>>,
}

impl Task {
    pub fn spawn(ctx: &egui::Context, key: &'static str, title: String, current: String, job: impl FnOnce() -> Status + Send + 'static) -> Self {
        reset(ctx, key);
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let (o, c) = (out.clone(), ctx.clone());
        std::thread::spawn(move || {
            test_pause();
            let s = job();
            *o.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
            c.request_repaint();
        });
        Task { key, working: Status::Working { title, count: Count::Unknown, current, can_stop: false }, out }
    }

    /// Draw it; `Some` once the person closed it (or it ended unseen).
    pub fn show(&self, ui: &mut Ui, page: Rect) -> Option<Click> {
        let done = self.out.lock().unwrap_or_else(|e| e.into_inner()).clone();
        show(ui, page, self.key, done.as_ref().unwrap_or(&self.working))
    }
}

/// A working state with its counter run to the end.
fn finish_count(s: &Status) -> Status {
    match s.clone() {
        Status::Working { title, count, current, can_stop } => {
            let count = match count {
                Count::Bytes { total, .. } => Count::Bytes { done: total, total },
                Count::Unknown => Count::Unknown,
            };
            let _ = can_stop;
            Status::Working { title, count, current, can_stop: false }
        }
        other => other,
    }
}

fn track() -> Color32 {
    white(26)
}

/// The card's height for `s`.
fn height(s: &Status) -> f32 {
    let buttons = match s {
        Status::Working { can_stop, .. } => *can_stop,
        _ => true,
    };
    PAD + MARK + sp::L + TITLE_H + sp::LINE_GAP + COUNT_H + sp::LINE_GAP + CUR_H + if buttons { sp::XL + BTN_H } else { 0.0 } + PAD
}

fn card(ctx: &egui::Context, page: Rect, key: &str, s: &Status) -> Option<Click> {
    let full = ctx.content_rect();
    let mut click = None;
    // the page goes still under it: no hover, tip, click or focus (P3)
    crate::kit::modal::shield(ctx);
    egui::Area::new(key_id(key).with("area")).order(Order::Foreground).fixed_pos(full.min).constrain(false).movable(false).show(ctx, |ui| {
        ui.set_min_size(full.size());
        // the scrim takes every click and hover, so nothing under it reacts
        let _ = ui.interact(full, key_id(key).with("scrim"), Sense::click_and_drag());
        let p = ui.painter().clone();
        p.rect_filled(full, CornerRadius::same(WINDOW_R), SCRIM());
        let center = if page.width() > W + 2.0 * sp::L { page.center() } else { full.center() };
        let m = Rect::from_center_size(center, Vec2::new(W.min(full.width() - 2.0 * sp::L), height(s)));
        let _ = ui.interact(m, key_id(key).with("card"), Sense::click());
        // a floating surface: the Whisper shadow and the popover edge (P1, P2)
        float_shadow(&p, m, R, 1.0);
        p.rect_filled(m, R, POPOVER());
        p.rect_stroke(m, R, Stroke::new(1.0, POPOVER_EDGE()), StrokeKind::Inside);
        let mark = Rect::from_center_size(Pos2::new(m.center().x, m.top() + PAD + MARK / 2.0), Vec2::splat(MARK));
        let (frac, state) = match s {
            Status::Working { count, .. } => match count.frac() {
                Some(f) => (f, MarkState::Filling),
                None => (0.0, MarkState::Sweeping),
            },
            Status::Stopped { .. } => (stopped_frac(ctx, key), MarkState::Filling),
            Status::Finished { .. } => (1.0, MarkState::Done),
            Status::Failed { .. } => (0.0, MarkState::Filling),
        };
        if !matches!(s, Status::Stopped { .. } | Status::Failed { .. }) {
            remember_frac(ctx, key, frac, state);
        }
        crate::kit::spin::mark(&p, &format!("loader-{key}"), mark, frac, state, ACTION(), track(), POPOVER());
        let w = m.width() - 2.0 * PAD;
        let mut y = mark.bottom() + sp::L;
        let line = |y: f32, text: &str, font: egui::FontId, col: Color32| {
            if !text.is_empty() {
                text_fit(&p, Pos2::new(m.center().x, y), Align2::CENTER_TOP, text, font, col, w);
            }
        };
        let (title, sub, third, third_amber) = match s {
            Status::Working { title, count, current, .. } => (title.clone(), count.line(), short(current), false),
            Status::Finished { title, sub, problem, .. } => (title.clone(), sub.clone(), problem.clone().unwrap_or_default(), problem.is_some()),
            Status::Stopped { sub } => ("Stopped".to_string(), sub.clone(), String::new(), false),
            Status::Failed { title, problem } => (title.clone(), String::new(), problem.clone(), true),
        };
        line(y, &title, ty::lead(), TEXT());
        y += TITLE_H + sp::LINE_GAP;
        line(y + 1.0, &sub, ty::body(), TEXT_DIM());
        y += COUNT_H + sp::LINE_GAP;
        if third_amber {
            line(y, &third, ty::label(), AMBER());
        } else {
            line(y, &glass::middle_ellipsis(&third, 46), mono(11.0), TEXT_FAINT());
        }
        y += CUR_H + sp::XL;
        // buttons, centred
        let buttons: Vec<(&str, u8, Click)> = match s {
            Status::Working { can_stop: true, .. } => {
                vec![("Stop", BTN_SECONDARY, Click::Stop)]
            }
            Status::Finished { put_back: true, .. } => vec![("Put back", BTN_SECONDARY, Click::PutBack), ("Done", BTN_PRIMARY, Click::Done)],
            Status::Finished { .. } | Status::Stopped { .. } | Status::Failed { .. } => {
                vec![("Done", BTN_PRIMARY, Click::Done)]
            }
            _ => vec![],
        };
        let widths: Vec<f32> = buttons.iter().map(|(l, k, _)| button_width(&p, l, *k).max(72.0)).collect();
        let total = widths.iter().sum::<f32>() + sp::S * (widths.len().max(1) - 1) as f32;
        let mut x = m.center().x - total / 2.0;
        for ((label, kind, c), bw) in buttons.iter().zip(&widths) {
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(*bw, BTN_H));
            if pill_button(ui, r, label, *kind, true).clicked() {
                click = Some(*c);
            }
            x += bw + sp::S;
        }
        // Return closes a finished card
        if matches!(s, Status::Finished { .. } | Status::Stopped { .. } | Status::Failed { .. }) && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            click = Some(Click::Done);
        }
    });
    click
}

/// The path being removed, with the home folder as "~".
fn short(p: &str) -> String {
    let home = crate::kit::fs::home().display().to_string();
    if !home.is_empty() && p.starts_with(&home) {
        format!("~{}", &p[home.len()..])
    } else {
        p.to_string()
    }
}

/// The fill a stopped job keeps (where it was when Stop was pressed).
fn stopped_frac(ctx: &egui::Context, key: &str) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(key_id(key).with("frac"))).unwrap_or(0.0)
}

fn remember_frac(ctx: &egui::Context, key: &str, frac: f32, state: MarkState) {
    if state == MarkState::Filling {
        ctx.data_mut(|d| d.insert_temp(key_id(key).with("frac"), frac));
    }
}

/// How the mark moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkState {
    /// Lime rises to the fraction done, a wave running on top.
    Filling,
    /// No counter: lime rises and starts over.
    Sweeping,
    /// Full, the ⌘ cut turned into a ✓.
    Done,
}

/// Where the lime's top edge is, in the mark's 1024 box, for `frac` done
/// (0 hides it just under the keycap; 1.06 covers the ear tips and the wave).
pub fn level(frac: f32) -> f32 {
    1030.0 - frac * 1040.0
}

/// Wave height and length in the 1024 box.
pub const WAVE_A: f32 = 26.0;
pub const WAVE_P: f32 = 256.0;

/// The mark as egui draws it, when no native layer can (screenshots, other
/// windows over it): the keycap in `track`, lime up to the level with a
/// still wave, then the ⌘ (or, `done` 0..1, the ✓) cut out in `bg`.
pub fn draw_mark(p: &egui::Painter, rect: Rect, frac: f32, state: MarkState, lime: Color32, track: Color32, bg: Color32) {
    let tex = crate::kit::brand::keycap(p.ctx());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    p.image(tex.id(), rect, uv, track);
    let done = p.ctx().animate_bool_with_time(egui::Id::NULL.with(("loader-mark-done", rect.min.x as i32, rect.min.y as i32)), state == MarkState::Done, 0.25);
    let f = match state {
        MarkState::Done => 1.06,
        MarkState::Sweeping => 0.5,
        _ => frac,
    };
    // lime: the keycap texture as a mesh whose top edge is the wave
    let top_box = level(f) + crate::kit::brand::NUDGE.1;
    let (h, w) = (rect.height(), rect.width());
    let mut mesh = egui::Mesh::with_texture(tex.id());
    let n = 32;
    for i in 0..=n {
        let u = i as f32 / n as f32;
        let x_box = u * 1024.0;
        let wave = (x_box / WAVE_P * std::f32::consts::TAU).sin() * WAVE_A;
        let yt = ((top_box + wave) / 1024.0).clamp(0.0, 1.0);
        let x = rect.left() + u * w;
        mesh.vertices.push(egui::epaint::Vertex { pos: Pos2::new(x, rect.top() + yt * h), uv: Pos2::new(u, yt), color: lime });
        mesh.vertices.push(egui::epaint::Vertex { pos: Pos2::new(x, rect.bottom()), uv: Pos2::new(u, 1.0), color: lime });
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(mesh);
    // the cuts, in the card's colour
    let s = w / 1024.0;
    let at = |x: f32, y: f32| crate::kit::brand::at(rect, x, y);
    if done < 1.0 {
        let col = with_alpha(bg, 1.0 - done);
        let st = Stroke::new(crate::kit::brand::CMD_WIDTH * s, col);
        for (cx, cy, r, a0, a1) in crate::kit::brand::CMD_LOOPS {
            let pts: Vec<Pos2> = glass::arc_points(Pos2::new(cx, cy), r, a0, a1, 40).into_iter().map(|q| at(q.x, q.y)).collect();
            p.add(egui::Shape::line(pts, st));
        }
        for (x0, y0, x1, y1) in crate::kit::brand::CMD_BARS {
            p.line_segment([at(x0, y0), at(x1, y1)], st);
        }
    }
    if done > 0.0 {
        let col = with_alpha(bg, done);
        let wd = crate::kit::brand::CHECK_WIDTH * s;
        let pts: Vec<Pos2> = crate::kit::brand::CHECK.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(egui::Shape::line(pts.clone(), Stroke::new(wd, col)));
        for q in pts {
            p.circle_filled(q, wd / 2.0, col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_x_of_y() {
        assert_eq!(Count::Bytes { done: 4_200_000_000, total: 8_900_000_000 }.line(), format!("{} of {}", human_bytes(4_200_000_000), human_bytes(8_900_000_000)));
        assert_eq!(Count::Unknown.line(), "");
        assert_eq!(Count::Bytes { done: 5, total: 0 }.frac(), Some(0.0));
        assert_eq!(Count::Bytes { done: 3, total: 2 }.frac(), Some(1.0));
        assert_eq!(Count::Unknown.frac(), None);
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(12), "12");
    }

    #[test]
    fn shows_only_after_the_delay() {
        let mut t = Timing { started: 10.0, shown: None, last: None };
        assert!(matches!(plan(&mut t, 10.2, true), Plan::Wait(l) if (l - 0.2).abs() < 1e-9));
        // ended before it showed: never drawn
        assert_eq!(plan(&mut t.clone(), 10.3, false), Plan::Skip);
        assert_eq!(plan(&mut t, 10.5, true), Plan::Draw);
        assert_eq!(t.shown, Some(10.5));
        assert_eq!(plan(&mut t, 10.6, false), Plan::Draw, "once shown it stays until Done");
    }

    #[test]
    fn a_quick_finish_shows_the_counter_full() {
        let w = Status::Working { title: "Cleaning…".into(), count: Count::Bytes { done: 1, total: 9 }, current: "/x".into(), can_stop: true };
        match finish_count(&w) {
            Status::Working { count, can_stop, .. } => {
                assert_eq!(count, Count::Bytes { done: 9, total: 9 });
                assert!(!can_stop);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn level_covers_the_mark_from_empty_to_ears() {
        // empty: the wave crest stays under the keycap's bottom (992)
        assert!(level(0.0) - WAVE_A > 992.0);
        // full: above the ear tips (44) by more than the wave
        assert!(level(1.06) + WAVE_A < 44.0);
    }

    #[test]
    fn cards_with_buttons_are_taller() {
        let w = |stop| Status::Working { title: "x".into(), count: Count::Unknown, current: String::new(), can_stop: stop };
        assert_eq!(height(&w(true)) - height(&w(false)), sp::XL + BTN_H);
    }
}
