//! The cat's face: what it looks like in each mood and how every part
//! moves, as plain drawing. `pose` says where the ears, eyes, whiskers,
//! nose and mouth are at a moment; `icon` and `bezel` draw that.
//!
//! Coordinates are the mark's 1024 box (see `kit::brand`). Angles are in
//! degrees, clockwise as the screen shows them.

use crate::kit::brand;
use tiny_skia::{BlendMode, Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    /// The plain mark (⌘ on the keycap).
    Idle,
    /// Eyes that follow the pointer.
    Watch,
    Purr,
    Hiss,
    Hungry,
    Hello,
    Eat,
}

impl Mood {
    /// How far each ear leans out while the mood lasts.
    fn ear_tilt(self) -> f32 {
        match self {
            Mood::Idle | Mood::Watch | Mood::Hello => 0.0,
            Mood::Purr => 12.0,
            Mood::Hiss => 44.0,
            Mood::Hungry => 18.0,
            Mood::Eat => 6.0,
        }
    }

    /// How far the whiskers turn up (hiss: forward) while the mood lasts.
    fn whisker_tilt(self) -> f32 {
        match self {
            Mood::Hiss => -16.0,
            Mood::Purr | Mood::Eat => 8.0,
            _ => 0.0,
        }
    }
}

/// The whole cat's own movement (the big cat sways and hops; the menu bar
/// icon only sways).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub rot: f32,
    /// Shift in points of the 128 pt cat.
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
}

#[cfg(test)]
impl Body {
    const REST: Body = Body { rot: 0.0, dx: 0.0, dy: 0.0, scale: 1.0 };
}

/// Where everything is at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub mood: Mood,
    /// Left and right ear, turned around the inner end of their base.
    pub ear: [f32; 2],
    /// The eyes' shift toward the pointer.
    pub look: (f32, f32),
    /// The open eyes' height (1 open, 0.1 shut).
    pub blink: f32,
    /// The closed, happy eyes' height.
    pub squint: f32,
    /// Left and right whiskers.
    pub whisker: [f32; 2],
    pub nose: f32,
    /// The chewing mouth's height.
    pub jaw: f32,
    /// The shine in the big hungry eyes.
    pub spark: f32,
    /// The hissing pupils' width.
    pub slit: f32,
    pub body: Body,
    /// The menu bar icon's lean.
    pub sway: f32,
}

// ------------------------------------------------------------------ timing

/// CSS `cubic-bezier(x1, y1, x2, y2)` at `x`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let at = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

#[derive(Clone, Copy)]
enum Ease {
    Linear,
    InOut,
    Out,
}

impl Ease {
    fn at(self, x: f32) -> f32 {
        // the ends are exact, so a move starts and stops on its stops
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        match self {
            Ease::Linear => x,
            Ease::InOut => bezier(0.42, 0.0, 0.58, 1.0, x),
            Ease::Out => bezier(0.0, 0.0, 0.58, 1.0, x),
        }
    }
}

/// A value over one loop: (fraction of the loop, value) stops, first at 0
/// and last at 1, eased between neighbours.
type Stops = &'static [(f32, f32)];

fn stops_at(stops: Stops, ease: Ease, frac: f32) -> f32 {
    for w in stops.windows(2) {
        let ((f0, v0), (f1, v1)) = (w[0], w[1]);
        if frac <= f1 {
            if f1 <= f0 || v0 == v1 {
                return v1;
            }
            return v0 + (v1 - v0) * ease.at(((frac - f0) / (f1 - f0)).clamp(0.0, 1.0));
        }
    }
    stops.last().map(|s| s.1).unwrap_or(0.0)
}

/// One looping movement: its stops, how long a loop takes, when it starts
/// and how many loops it runs (0: for ever).
#[derive(Clone, Copy)]
struct Loop {
    stops: Stops,
    period: f32,
    delay: f32,
    ease: Ease,
    count: u32,
}

impl Loop {
    const fn new(stops: Stops, period: f32) -> Loop {
        Loop { stops, period, delay: 0.0, ease: Ease::InOut, count: 0 }
    }
    const fn after(mut self, delay: f32) -> Loop {
        self.delay = delay;
        self
    }
    const fn eased(mut self, ease: Ease) -> Loop {
        self.ease = ease;
        self
    }
    const fn times(mut self, n: u32) -> Loop {
        self.count = n;
        self
    }

    /// Its value `t` seconds into the mood, or `rest` before it starts
    /// and after it ends.
    fn at(&self, t: f32, rest: f32) -> f32 {
        let t = t - self.delay;
        if t < 0.0 || (self.count > 0 && t >= self.period * self.count as f32) {
            return rest;
        }
        stops_at(self.stops, self.ease, (t / self.period).fract())
    }

    /// Seconds from `t` until it next moves (0: moving now), or None when
    /// it never moves again. A loop moves between stops with different
    /// values.
    fn still_for(&self, t: f32) -> Option<f32> {
        let local = t - self.delay;
        if self.count > 0 && local >= self.period * self.count as f32 {
            return None;
        }
        if local < 0.0 {
            // waiting to start: then until its first moving stretch
            return Some(-local + self.first_move(0.0)? * self.period);
        }
        let frac = (local / self.period).fract();
        match self.first_move(frac) {
            Some(f) => Some((f - frac).max(0.0) * self.period),
            // nothing left in this loop: the next loop's first move
            None => {
                let last = self.count > 0 && (local / self.period) as u32 + 1 >= self.count;
                if last {
                    None
                } else {
                    Some((1.0 - frac + self.first_move(0.0)?) * self.period)
                }
            }
        }
    }

    /// The start of the first moving stretch at or after `frac`.
    fn first_move(&self, frac: f32) -> Option<f32> {
        self.stops.windows(2).find(|w| w[0].1 != w[1].1 && w[1].0 > frac).map(|w| w[0].0.max(frac))
    }
}

// the movements, as the design canvas has them
const FLICK: Stops = &[(0.0, 0.0), (0.84, 0.0), (0.88, 14.0), (0.92, -4.0), (0.96, 6.0), (1.0, 0.0)];
/// The same flick in a loop four times as long: at rest every frame makes
/// the menu bar draw itself again, so the ears flick less often.
const FLICK_RARE: Stops = &[(0.0, 0.0), (0.96, 0.0), (0.97, 14.0), (0.98, -4.0), (0.99, 6.0), (1.0, 0.0)];
const DIP_EARLY: Stops = &[(0.0, 0.0), (0.25, 10.0), (0.5, 0.0), (1.0, 0.0)];
const DIP_LATE: Stops = &[(0.0, 0.0), (0.5, 0.0), (0.75, 10.0), (1.0, 0.0)];
const BOB: Stops = &[(0.0, 0.0), (0.5, 4.0), (1.0, 0.0)];
const TREMBLE: Stops = &[(0.0, -2.5), (0.5, 2.5), (1.0, -2.5)];
const WAVE: Stops = &[(0.0, -4.0), (0.5, 7.0), (1.0, -4.0)];
const TWITCH: Stops = &[(0.0, 0.0), (0.6, 0.0), (0.7, 9.0), (0.8, -3.0), (0.9, 5.0), (1.0, 0.0)];
const SNIFF: Stops = &[(0.0, 1.0), (0.7, 1.0), (0.77, 1.28), (0.85, 1.0), (0.92, 1.22), (1.0, 1.0)];
const WINK: Stops = &[(0.0, 1.0), (0.9, 1.0), (0.95, 0.1), (1.0, 1.0)];
const SLOW_BLINK: Stops = &[(0.0, 1.0), (0.25, 1.0), (0.45, 0.6), (0.6, 0.6), (0.85, 1.0), (1.0, 1.0)];
const SQUINT: Stops = &[(0.0, 1.0), (0.25, 0.62), (0.5, 1.0), (0.75, 0.62), (1.0, 1.0)];
const CHEW: Stops = &[(0.0, 1.0), (0.5, 0.2), (1.0, 1.0)];
const SPARK: Stops = &[(0.0, 1.0), (0.5, 1.45), (1.0, 1.0)];
const SLIT: Stops = &[(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)];
const SWAY: Stops = &[(0.0, 0.0), (0.25, -7.0), (0.5, 0.0), (0.75, 7.0), (1.0, 0.0)];
const RUB_ROT: Stops = &[(0.0, 0.0), (0.25, -6.0), (0.5, 0.0), (0.75, 6.0), (1.0, 0.0)];
const RUB_DX: Stops = &[(0.0, 0.0), (0.25, -3.0), (0.5, 0.0), (0.75, 3.0), (1.0, 0.0)];
const RUB_DY: Stops = &[(0.0, 0.0), (0.25, 1.0), (0.5, -2.0), (0.75, 1.0), (1.0, 0.0)];
const RUB_SCALE: Stops = &[(0.0, 1.0), (0.25, 1.04), (0.5, 1.02), (0.75, 1.04), (1.0, 1.0)];
const SHAKE: Stops = &[(0.0, 0.0), (0.2, -7.0), (0.4, 7.0), (0.6, -4.0), (0.8, 4.0), (1.0, 0.0)];
const HOP: Stops = &[(0.0, 0.0), (0.4, -10.0), (1.0, 0.0)];
const NOD: Stops = &[(0.0, 0.0), (0.5, 3.0), (1.0, 0.0)];

/// Every movement of a mood, by the part it moves.
#[derive(Clone, Copy, Default)]
struct Moves {
    ear: [Option<Loop>; 2],
    whisker: [Option<Loop>; 2],
    nose: Option<Loop>,
    blink: Option<Loop>,
    squint: Option<Loop>,
    jaw: Option<Loop>,
    spark: Option<Loop>,
    slit: Option<Loop>,
    sway: Option<Loop>,
    rot: Option<Loop>,
    dx: Option<Loop>,
    dy: Option<Loop>,
    scale: Option<Loop>,
}

impl Moves {
    fn all(&self) -> [Option<Loop>; 15] {
        [self.ear[0], self.ear[1], self.whisker[0], self.whisker[1], self.nose, self.blink, self.squint, self.jaw, self.spark, self.slit, self.sway, self.rot, self.dx, self.dy, self.scale]
    }
}

fn moves(mood: Mood) -> Moves {
    let l = Loop::new;
    match mood {
        Mood::Idle => Moves { ear: [Some(l(FLICK_RARE, 36.0).after(3.0)), Some(l(FLICK_RARE, 52.0).after(7.0))], ..Moves::default() },
        Mood::Watch => Moves {
            ear: [Some(l(FLICK, 5.3).after(1.0)), Some(l(FLICK, 7.1).after(3.0))],
            nose: Some(l(SNIFF, 4.7).after(2.0)),
            blink: Some(l(WINK, 4.0)),
            ..Moves::default()
        },
        Mood::Purr => Moves {
            ear: [Some(l(DIP_EARLY, 1.6)), Some(l(DIP_LATE, 1.6))],
            whisker: [Some(l(WAVE, 0.8)), Some(l(WAVE, 0.8).after(0.15))],
            squint: Some(l(SQUINT, 1.6)),
            sway: Some(l(SWAY, 1.6)),
            rot: Some(l(RUB_ROT, 1.6)),
            dx: Some(l(RUB_DX, 1.6)),
            dy: Some(l(RUB_DY, 1.6)),
            scale: Some(l(RUB_SCALE, 1.6)),
            ..Moves::default()
        },
        Mood::Hiss => Moves {
            ear: [Some(l(TREMBLE, 0.1).eased(Ease::Linear)), Some(l(TREMBLE, 0.1).eased(Ease::Linear).after(0.05))],
            whisker: [Some(l(TREMBLE, 0.09).eased(Ease::Linear)), Some(l(TREMBLE, 0.09).eased(Ease::Linear).after(0.04))],
            slit: Some(l(SLIT, 0.25)),
            dx: Some(l(SHAKE, 0.3).eased(Ease::Out).times(1)),
            ..Moves::default()
        },
        Mood::Hungry => Moves {
            ear: [Some(l(DIP_EARLY, 1.0)), Some(l(DIP_LATE, 1.0))],
            whisker: [Some(l(WAVE, 1.0)), Some(l(WAVE, 1.0))],
            nose: Some(l(SNIFF, 1.0)),
            spark: Some(l(SPARK, 0.7)),
            dy: Some(l(HOP, 0.5).times(2)),
            ..Moves::default()
        },
        Mood::Hello => Moves {
            ear: [Some(l(FLICK, 1.8)), Some(l(FLICK, 2.3))],
            whisker: [Some(l(TWITCH, 0.9)), Some(l(TWITCH, 0.9).after(0.2))],
            nose: Some(l(SNIFF, 1.8)),
            blink: Some(l(SLOW_BLINK, 1.7).times(2)),
            dy: Some(l(HOP, 0.9).times(4)),
            ..Moves::default()
        },
        Mood::Eat => Moves {
            ear: [Some(l(BOB, 0.3)), Some(l(BOB, 0.3))],
            whisker: [Some(l(WAVE, 0.3)), Some(l(WAVE, 0.3))],
            jaw: Some(l(CHEW, 0.3)),
            dy: Some(l(NOD, 0.3)),
            ..Moves::default()
        },
    }
}

/// The cat `t` seconds into `mood`, its eyes shifted by `look`.
pub fn pose(mood: Mood, t: f32, look: (f32, f32)) -> Pose {
    let m = moves(mood);
    let v = |l: Option<Loop>, rest: f32| l.map_or(rest, |l| l.at(t, rest));
    let look = if mood == Mood::Watch { look } else { (0.0, 0.0) };
    // watching, both ears lean the way the eyes look
    let lean = if mood == Mood::Watch { (look.0 / 25.0).round() } else { 0.0 };
    let tilt = mood.ear_tilt();
    let wt = mood.whisker_tilt();
    Pose {
        mood,
        // an ear leans out by turning away from the head: the left one
        // anticlockwise, the right one clockwise
        ear: [lean - tilt - v(m.ear[0], 0.0), lean + tilt + v(m.ear[1], 0.0)],
        look,
        blink: v(m.blink, 1.0),
        squint: v(m.squint, 1.0),
        // whiskers turn up: the left ones clockwise, the right ones anticlockwise
        whisker: [wt + v(m.whisker[0], 0.0), -wt - v(m.whisker[1], 0.0)],
        nose: v(m.nose, 1.0),
        jaw: v(m.jaw, 1.0),
        spark: v(m.spark, 1.0),
        slit: v(m.slit, 1.0),
        body: Body { rot: v(m.rot, 0.0), dx: v(m.dx, 0.0), dy: v(m.dy, 0.0), scale: v(m.scale, 1.0) },
        sway: v(m.sway, 0.0),
    }
}

/// Seconds from `t` until anything of `mood` next moves: 0 while it moves,
/// None when it has come to rest for good.
pub fn still_for(mood: Mood, t: f32) -> Option<f32> {
    moves(mood).all().into_iter().flatten().filter_map(|l| l.still_for(t)).reduce(f32::min)
}

// ----------------------------------------------------------------- drawing

fn path(build: impl FnOnce(&mut PathBuilder)) -> Option<Path> {
    let mut pb = PathBuilder::new();
    build(&mut pb);
    pb.finish()
}

fn oval(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    PathBuilder::from_oval(tiny_skia::Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry)?)
}

/// The closed, happy eyes: two arcs.
fn arcs() -> Option<Path> {
    path(|p| {
        p.move_to(290.0, 574.0);
        p.quad_to(366.0, 476.0, 442.0, 574.0);
        p.move_to(582.0, 574.0);
        p.quad_to(658.0, 476.0, 734.0, 574.0);
    })
}

fn nose() -> Option<Path> {
    path(|p| {
        p.move_to(474.0, 668.0);
        p.line_to(550.0, 668.0);
        p.line_to(512.0, 712.0);
        p.close();
    })
}

/// The cat mouth: a short stem under the nose and a curl to each side.
fn mouth() -> Option<Path> {
    path(|p| {
        p.move_to(512.0, 716.0);
        p.line_to(512.0, 730.0);
        p.quad_to(512.0, 778.0, 464.0, 778.0);
        p.quad_to(424.0, 778.0, 416.0, 740.0);
        p.move_to(512.0, 730.0);
        p.quad_to(512.0, 778.0, 560.0, 778.0);
        p.quad_to(600.0, 778.0, 608.0, 740.0);
    })
}

fn stem() -> Option<Path> {
    path(|p| {
        p.move_to(512.0, 716.0);
        p.line_to(512.0, 734.0);
    })
}

fn hiss_eyes() -> Option<Path> {
    path(|p| {
        p.move_to(284.0, 470.0);
        p.line_to(452.0, 552.0);
        p.line_to(452.0, 622.0);
        p.quad_to(366.0, 652.0, 284.0, 600.0);
        p.close();
        p.move_to(740.0, 470.0);
        p.line_to(572.0, 552.0);
        p.line_to(572.0, 622.0);
        p.quad_to(658.0, 652.0, 740.0, 600.0);
        p.close();
    })
}

fn hiss_mouth() -> Option<Path> {
    path(|p| {
        p.move_to(408.0, 752.0);
        p.quad_to(512.0, 716.0, 616.0, 752.0);
        p.line_to(584.0, 880.0);
        p.quad_to(512.0, 904.0, 440.0, 880.0);
        p.close();
    })
}

fn fangs() -> Option<Path> {
    path(|p| {
        p.move_to(440.0, 746.0);
        p.line_to(480.0, 738.0);
        p.line_to(462.0, 812.0);
        p.close();
        p.move_to(544.0, 738.0);
        p.line_to(584.0, 746.0);
        p.line_to(562.0, 812.0);
        p.close();
    })
}

fn hungry_mouth() -> Option<Path> {
    path(|p| {
        p.move_to(470.0, 752.0);
        p.quad_to(512.0, 728.0, 554.0, 752.0);
        p.quad_to(560.0, 830.0, 512.0, 836.0);
        p.quad_to(464.0, 830.0, 470.0, 752.0);
        p.close();
    })
}

/// The open jaw under the mouth, while eating. Its top is at y 744.
fn jaw() -> Option<Path> {
    path(|p| {
        p.move_to(428.0, 752.0);
        p.quad_to(468.0, 792.0, 512.0, 744.0);
        p.quad_to(556.0, 792.0, 596.0, 752.0);
        p.quad_to(594.0, 850.0, 512.0, 858.0);
        p.quad_to(430.0, 850.0, 428.0, 752.0);
        p.close();
    })
}

fn whiskers(right: bool) -> Option<Path> {
    path(|p| {
        if right {
            for (y0, y1, x0, x1) in [(640.0, 606.0, 984.0, 1094.0), (704.0, 704.0, 994.0, 1108.0), (768.0, 802.0, 984.0, 1094.0)] {
                p.move_to(x0, y0);
                p.line_to(x1, y1);
            }
        } else {
            for (y0, y1, x0, x1) in [(640.0, 606.0, 40.0, -70.0), (704.0, 704.0, 30.0, -84.0), (768.0, 802.0, 40.0, -70.0)] {
                p.move_to(x0, y0);
                p.line_to(x1, y1);
            }
        }
    })
}

/// Where each ear turns: the inner end of its base (turning around the
/// middle of the base opens a gap that reads as horns).
const EAR_PIVOT: [(f32, f32); 2] = [(330.0, 200.0), (692.0, 200.0)];
const WHISKER_PIVOT: [(f32, f32); 2] = [(90.0, 700.0), (934.0, 700.0)];

/// Scaling by (sx, sy) around (cx, cy).
fn scale_at(sx: f32, sy: f32, cx: f32, cy: f32) -> Transform {
    Transform::from_translate(cx, cy).pre_scale(sx, sy).pre_translate(-cx, -cy)
}

struct Inks {
    /// The cat.
    ink: Paint<'static>,
    /// What is cut out of it: clear (the icon) or the colour behind it.
    cut: Paint<'static>,
}

fn solid(c: Color, mode: BlendMode) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p.blend_mode = mode;
    p
}

/// The cat's head (keycap, ears) and, on the big cat, its whiskers.
fn draw_head(pm: &mut Pixmap, t: Transform, pose: &Pose, inks: &Inks, whisk: bool) {
    for (i, right) in [false, true].into_iter().enumerate() {
        if let Some(p) = brand::ear(right) {
            let (px, py) = EAR_PIVOT[i];
            pm.fill_path(&p, &inks.ink, FillRule::Winding, t.pre_concat(Transform::from_rotate_at(pose.ear[i], px, py)), None);
        }
    }
    if let Some(p) = brand::body() {
        pm.fill_path(&p, &inks.ink, FillRule::Winding, t, None);
    }
    if whisk && pose.mood != Mood::Idle {
        let stroke = Stroke { width: 16.0, line_cap: LineCap::Round, ..Stroke::default() };
        for (i, right) in [false, true].into_iter().enumerate() {
            if let Some(p) = whiskers(right) {
                let (px, py) = WHISKER_PIVOT[i];
                pm.stroke_path(&p, &inks.ink, &stroke, t.pre_concat(Transform::from_rotate_at(pose.whisker[i], px, py)), None);
            }
        }
    }
}

/// The eyes of `pose`, cut out of the head.
fn draw_eyes(pm: &mut Pixmap, t: Transform, pose: &Pose, inks: &Inks) {
    let fill = |pm: &mut Pixmap, p: Option<Path>, paint: &Paint, t: Transform| {
        if let Some(p) = p {
            pm.fill_path(&p, paint, FillRule::Winding, t, None);
        }
    };
    match pose.mood {
        Mood::Idle => {
            if let Some(p) = brand::command() {
                pm.stroke_path(&p, &inks.cut, &Stroke { width: 59.0, line_cap: LineCap::Butt, ..Stroke::default() }, t, None);
            }
        }
        Mood::Watch | Mood::Hello => {
            let t = t.pre_translate(pose.look.0, pose.look.1).pre_concat(scale_at(1.0, pose.blink, 512.0, 540.0));
            fill(pm, oval(366.0, 540.0, 62.0, 86.0), &inks.cut, t);
            fill(pm, oval(658.0, 540.0, 62.0, 86.0), &inks.cut, t);
        }
        Mood::Purr | Mood::Eat => {
            if let Some(p) = arcs() {
                let stroke = Stroke { width: 46.0, line_cap: LineCap::Round, ..Stroke::default() };
                pm.stroke_path(&p, &inks.cut, &stroke, t.pre_concat(scale_at(1.0, pose.squint, 512.0, 549.5)), None);
            }
        }
        Mood::Hiss => {
            fill(pm, hiss_eyes(), &inks.cut, t);
            fill(pm, oval(392.0, 574.0, 13.0 * pose.slit, 46.0), &inks.ink, t);
            fill(pm, oval(632.0, 574.0, 13.0 * pose.slit, 46.0), &inks.ink, t);
        }
        Mood::Hungry => {
            for cx in [366.0, 658.0] {
                fill(pm, PathBuilder::from_circle(cx, 536.0, 96.0), &inks.cut, t);
                fill(pm, PathBuilder::from_circle(cx, 518.0, 62.0), &inks.ink, t);
                fill(pm, PathBuilder::from_circle(cx - 22.0, 494.0, 20.0 * pose.spark), &inks.cut, t);
            }
        }
    }
}

/// Nose and mouth of `pose` (the big cat only: at 18 pt they are noise).
fn draw_muzzle(pm: &mut Pixmap, t: Transform, pose: &Pose, inks: &Inks) {
    if pose.mood == Mood::Idle {
        return;
    }
    let line = Stroke { width: 26.0, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    if let Some(p) = nose() {
        let t = t.pre_concat(scale_at(pose.nose, pose.nose, 512.0, 690.0));
        pm.fill_path(&p, &inks.cut, FillRule::Winding, t, None);
        pm.stroke_path(&p, &inks.cut, &Stroke { width: 14.0, line_join: LineJoin::Round, ..Stroke::default() }, t, None);
    }
    let fill = |pm: &mut Pixmap, p: Option<Path>, paint: &Paint, t: Transform| {
        if let Some(p) = p {
            pm.fill_path(&p, paint, FillRule::Winding, t, None);
        }
    };
    match pose.mood {
        Mood::Idle => {}
        Mood::Watch | Mood::Hello | Mood::Purr | Mood::Eat => {
            if pose.mood == Mood::Eat {
                fill(pm, jaw(), &inks.cut, t.pre_concat(scale_at(1.0, pose.jaw, 512.0, 744.0)));
            }
            if let Some(p) = mouth() {
                pm.stroke_path(&p, &inks.cut, &line, t, None);
            }
        }
        Mood::Hiss => {
            if let Some(p) = stem() {
                pm.stroke_path(&p, &inks.cut, &line, t, None);
            }
            fill(pm, hiss_mouth(), &inks.cut, t);
            fill(pm, fangs(), &inks.ink, t);
        }
        Mood::Hungry => {
            if let Some(p) = stem() {
                pm.stroke_path(&p, &inks.cut, &line, t, None);
            }
            fill(pm, hungry_mouth(), &inks.cut, t);
        }
    }
}

/// The mark's own small offset in its box (as `brand::glyph_rgba`).
fn mark(px: f32) -> Transform {
    let s = px / 1024.0;
    Transform::from_scale(s, s).pre_translate(512.0 - 511.5, 512.0 - 518.0)
}

/// The menu bar icon for `pose` in a `px` square: black, alpha = coverage,
/// premultiplied (a template image, macOS tints it). Eyes only.
pub fn icon(pose: &Pose, px: u32) -> Vec<u8> {
    let Some(mut pm) = Pixmap::new(px, px) else { return vec![0; (px * px * 4) as usize] };
    let inks = Inks { ink: solid(Color::BLACK, BlendMode::SourceOver), cut: solid(Color::BLACK, BlendMode::Clear) };
    // the sway turns the cat around a point near its feet
    let t = mark(px as f32).pre_concat(Transform::from_rotate_at(pose.sway, 512.0, 921.6));
    draw_head(&mut pm, t, pose, &inks, false);
    draw_eyes(&mut pm, t, pose, &inks);
    pm.take()
}

/// Side of the big cat's card, in points.
pub const BEZEL_PT: f32 = 200.0;
/// Side of the cat on it.
const BEZEL_CAT_PT: f32 = 128.0;
const BEZEL_RADIUS_PT: f32 = 28.0;

fn rgb(hex: u32, a: f32) -> Color {
    Color::from_rgba(((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, a).unwrap_or(Color::BLACK)
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    // a quarter circle as one cubic
    let k = r * 0.5523;
    path(|p| {
        p.move_to(x + r, y);
        p.line_to(x + w - r, y);
        p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
        p.line_to(x + w, y + h - r);
        p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
        p.line_to(x + r, y + h);
        p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
        p.line_to(x, y + r);
        p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        p.close();
    })
}

/// The big cat's card colour.
const BEZEL_BACK: u32 = 0x1A2818;

/// The big cat on its card for `pose`: `scale` pixels a point, premultiplied
/// RGBA, `bezel_px(scale)` square. The screen gets it in two layers (`card`
/// and `bezel_cat`); the tests look at it whole.
#[cfg(test)]
pub fn bezel(pose: &Pose, scale: f32) -> Vec<u8> {
    let n = bezel_px(scale);
    let Some(mut pm) = Pixmap::new(n, n) else { return vec![0; (n * n * 4) as usize] };
    draw_card(&mut pm, scale);
    draw_bezel_cat(&mut pm, pose, scale);
    pm.take()
}

/// The card alone, as `bezel` draws it under the cat.
pub fn card(scale: f32) -> Vec<u8> {
    let n = bezel_px(scale);
    let Some(mut pm) = Pixmap::new(n, n) else { return vec![0; (n * n * 4) as usize] };
    draw_card(&mut pm, scale);
    pm.take()
}

/// The cat alone, clear around it: laid over `card` it is `bezel`, so a
/// moving cat only redraws itself.
pub fn bezel_cat(pose: &Pose, scale: f32) -> Vec<u8> {
    let n = bezel_px(scale);
    let Some(mut pm) = Pixmap::new(n, n) else { return vec![0; (n * n * 4) as usize] };
    draw_bezel_cat(&mut pm, pose, scale);
    pm.take()
}

fn draw_card(pm: &mut Pixmap, scale: f32) {
    let card = Transform::from_scale(scale, scale);
    if let Some(p) = round_rect(0.0, 0.0, BEZEL_PT, BEZEL_PT, BEZEL_RADIUS_PT) {
        pm.fill_path(&p, &solid(rgb(BEZEL_BACK, 1.0), BlendMode::SourceOver), FillRule::Winding, card, None);
    }
    if let Some(p) = round_rect(0.5, 0.5, BEZEL_PT - 1.0, BEZEL_PT - 1.0, BEZEL_RADIUS_PT - 0.5) {
        pm.stroke_path(&p, &solid(rgb(0xFFFFFF, 0.08), BlendMode::SourceOver), &Stroke { width: 1.0, ..Stroke::default() }, card, None);
    }
}

fn draw_bezel_cat(pm: &mut Pixmap, pose: &Pose, scale: f32) {
    let card = Transform::from_scale(scale, scale);
    // the cut-outs are painted in the card's colour, so they look the same
    // over the card whether it is under them now or later
    let back = rgb(BEZEL_BACK, 1.0);
    let inks = Inks { ink: solid(rgb(0xC8F05A, 1.0), BlendMode::SourceOver), cut: solid(back, BlendMode::SourceOver) };
    // the cat's box in the middle of the card, then its own movement
    // around a point near its feet: turn, shift, grow
    let unit = BEZEL_CAT_PT / 1024.0;
    let edge = (BEZEL_PT - BEZEL_CAT_PT) / 2.0;
    let b = pose.body;
    let t = card
        .pre_translate(edge, edge)
        .pre_scale(unit, unit)
        .pre_concat(Transform::from_rotate_at(b.rot, 512.0, 870.4))
        .pre_translate(b.dx / unit, b.dy / unit)
        .pre_concat(scale_at(b.scale, b.scale, 512.0, 870.4));
    draw_head(pm, t, pose, &inks, true);
    draw_eyes(pm, t, pose, &inks);
    draw_muzzle(pm, t, pose, &inks);
}

pub fn bezel_px(scale: f32) -> u32 {
    (BEZEL_PT * scale).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &[u8], n: u32, x: f32, y: f32) -> u8 {
        let (x, y) = ((x / 1024.0 * n as f32) as u32, (y / 1024.0 * n as f32) as u32);
        px[((y * n + x) * 4 + 3) as usize]
    }

    #[test]
    fn easing_matches_css() {
        assert!((Ease::InOut.at(0.5) - 0.5).abs() < 1e-3);
        assert!(Ease::InOut.at(0.25) < 0.25 && Ease::InOut.at(0.75) > 0.75);
        assert!(Ease::Out.at(0.5) > 0.5);
        assert_eq!(Ease::Linear.at(0.3), 0.3);
    }

    #[test]
    fn a_mood_starts_from_its_rest_pose() {
        let p = pose(Mood::Purr, 0.0, (0.0, 0.0));
        assert_eq!(p.ear, [-12.0, 12.0]);
        assert_eq!(p.body, Body::REST);
        let p = pose(Mood::Hiss, 0.0, (40.0, 0.0));
        assert_eq!(p.look, (0.0, 0.0), "only a watching cat looks around");
        assert!((p.ear[0] + 44.0 - 2.5).abs() < 1e-3, "flat, and trembling");
    }

    #[test]
    fn the_purr_leans_left_then_right() {
        let left = pose(Mood::Purr, 0.4, (0.0, 0.0));
        assert!((left.body.rot + 6.0).abs() < 1e-3 && (left.sway + 7.0).abs() < 1e-3);
        assert!((left.squint - 0.62).abs() < 1e-3);
        assert!((left.ear[0] + 22.0).abs() < 1e-3, "the left ear dips with the lean: {}", left.ear[0]);
        let right = pose(Mood::Purr, 1.2, (0.0, 0.0));
        assert!((right.body.rot - 6.0).abs() < 1e-3);
        assert!((right.ear[1] - 22.0).abs() < 1e-3);
    }

    #[test]
    fn watching_eyes_follow_and_ears_lean() {
        let p = pose(Mood::Watch, 0.2, (-150.0, 70.0));
        assert_eq!(p.look, (-150.0, 70.0));
        assert_eq!(p.ear, [-6.0, -6.0]);
        // the quick blink near the end of each 4 s
        assert!((pose(Mood::Watch, 3.8, (0.0, 0.0)).blink - 0.1).abs() < 1e-3);
    }

    #[test]
    fn one_shot_moves_end() {
        // the hiss shakes once, the hop of the greeting runs four times
        assert_eq!(pose(Mood::Hiss, 0.31, (0.0, 0.0)).body.dx, 0.0);
        assert!(pose(Mood::Hello, 0.36, (0.0, 0.0)).body.dy < -9.9);
        assert_eq!(pose(Mood::Hello, 3.7, (0.0, 0.0)).body.dy, 0.0);
        assert_eq!(pose(Mood::Hello, 3.5, (0.0, 0.0)).blink, 1.0);
    }

    #[test]
    fn the_resting_cat_says_when_it_next_moves() {
        // left ear: first flick 3 s + 96% of 36 s in
        let first = still_for(Mood::Idle, 0.0).unwrap();
        assert!((first - (3.0 + 0.96 * 36.0)).abs() < 1e-3, "{first}");
        assert_eq!(still_for(Mood::Idle, 3.0 + 0.98 * 36.0), Some(0.0), "mid flick");
        // right after the left ear's flick: the right ear's first one
        let next = still_for(Mood::Idle, 39.01).unwrap();
        assert!((39.01 + next - (7.0 + 0.96 * 52.0)).abs() < 1e-2, "{next}");
        assert_eq!(still_for(Mood::Purr, 0.7), Some(0.0));
        // every stretch between two stops with different values counts as moving
        for i in 0..1200 {
            let t = i as f32 * 0.1;
            let moving = pose(Mood::Idle, t, (0.0, 0.0)) != pose(Mood::Idle, t + 0.02, (0.0, 0.0));
            if moving {
                assert!(still_for(Mood::Idle, t + 0.01).unwrap() <= 0.011, "moving at {t} but told to rest");
            }
        }
    }

    #[test]
    fn the_resting_icon_is_the_mark() {
        let n = 144;
        let px = icon(&pose(Mood::Idle, 0.0, (0.0, 0.0)), n);
        let mark = brand::glyph_rgba(n);
        let worst = px.chunks_exact(4).zip(mark.chunks_exact(4)).map(|(a, b)| (a[3] as i32 - b[3] as i32).abs()).max().unwrap();
        assert!(worst <= 40, "differs from the mark by {worst}/255 somewhere");
    }

    #[test]
    fn icon_faces_have_eyes_and_no_mouth() {
        let n = 256;
        let watch = icon(&pose(Mood::Watch, 0.0, (0.0, 0.0)), n);
        assert_eq!(alpha(&watch, n, 366.0, 534.0), 0, "left eye");
        assert_eq!(alpha(&watch, n, 658.0, 534.0), 0, "right eye");
        assert_eq!(alpha(&watch, n, 512.0, 534.0), 255, "between the eyes");
        assert_eq!(alpha(&watch, n, 512.0, 690.0), 255, "no nose at this size");
        assert_eq!(alpha(&watch, n, 512.0, 780.0), 255, "no mouth at this size");
        // looking left moves the eyes
        let left = icon(&pose(Mood::Watch, 0.0, (-150.0, 0.0)), n);
        assert_eq!(alpha(&left, n, 216.0, 534.0), 0);
        assert_eq!(alpha(&left, n, 420.0, 534.0), 255);
        let hungry = icon(&pose(Mood::Hungry, 0.0, (0.0, 0.0)), n);
        assert_eq!(alpha(&hungry, n, 366.0, 610.0), 0, "big eye");
        assert_eq!(alpha(&hungry, n, 372.0, 524.0), 255, "pupil");
    }

    #[test]
    fn the_big_cat_is_lime_on_its_card_with_a_face() {
        let px = bezel(&pose(Mood::Hello, 0.0, (0.0, 0.0)), 2.0);
        let n = bezel_px(2.0);
        assert_eq!(px.len(), (n * n * 4) as usize);
        let at = |x: f32, y: f32| {
            // a point of the cat's 1024 box on the card
            let (cx, cy) = (36.0 + (x + 0.5) / 1024.0 * 128.0, 36.0 + (y - 6.0) / 1024.0 * 128.0);
            let i = (((cy * 2.0) as u32 * n + (cx * 2.0) as u32) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(150.0, 900.0), [0xC8, 0xF0, 0x5A, 255], "lime body");
        assert_eq!(at(366.0, 540.0), [0x1A, 0x28, 0x18, 255], "eye shows the card");
        assert_eq!(at(512.0, 690.0), [0x1A, 0x28, 0x18, 255], "nose");
        assert_eq!(at(-20.0, 704.0), [0xC8, 0xF0, 0x5A, 255], "whisker outside the head");
        assert_eq!(px[3], 0, "the card's corner is round");
        assert_eq!(&px[(((n / 2) * n + 6) * 4) as usize..][..4], &[0x1A, 0x28, 0x18, 255], "card");
    }

    /// Writes every mood over time as PNG sheets into `$CAT_PREVIEW_DIR`
    /// (to look at them next to the design).
    #[test]
    #[ignore]
    fn write_previews() {
        let dir = std::path::PathBuf::from(std::env::var("CAT_PREVIEW_DIR").unwrap_or_else(|_| "cat-previews".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let moods = [(Mood::Idle, 12.0), (Mood::Watch, 6.0), (Mood::Purr, 1.6), (Mood::Hiss, 0.5), (Mood::Hungry, 1.0), (Mood::Hello, 2.4), (Mood::Eat, 0.3)];
        let cols = 8u32;
        let n = bezel_px(2.0);
        let mut big = image::RgbaImage::from_pixel(n * cols, n * moods.len() as u32, image::Rgba([40, 44, 40, 255]));
        let m = 144u32;
        let mut small = image::RgbaImage::from_pixel(m * cols, m * moods.len() as u32, image::Rgba([27, 32, 29, 255]));
        let over = |img: &mut image::RgbaImage, px: &[u8], n: u32, x0: u32, y0: u32, white: bool| {
            for (i, c) in px.chunks_exact(4).enumerate() {
                let (x, y) = (x0 + i as u32 % n, y0 + i as u32 / n);
                let a = c[3] as f32 / 255.0;
                let d = img.get_pixel_mut(x, y);
                for k in 0..3 {
                    // premultiplied source; the icon (black) is shown white, as on a dark menu bar
                    let s = if white { c[3] as f32 } else { c[k] as f32 };
                    d[k] = (s + d[k] as f32 * (1.0 - a)).round().min(255.0) as u8;
                }
            }
        };
        for (row, (mood, span)) in moods.into_iter().enumerate() {
            for col in 0..cols {
                let t = span * col as f32 / cols as f32;
                let look = if mood == Mood::Watch { (-150.0 + 300.0 * col as f32 / (cols - 1) as f32, 10.0 * col as f32) } else { (0.0, 0.0) };
                let p = pose(mood, if mood == Mood::Idle { 37.56 + 0.12 * col as f32 } else { t }, look);
                over(&mut big, &bezel(&p, 2.0), n, col * n, row as u32 * n, false);
                over(&mut small, &icon(&p, m), m, col * m, row as u32 * m, true);
            }
        }
        big.save(dir.join("bezel.png")).unwrap();
        small.save(dir.join("icon.png")).unwrap();
        // the icon at its real size, on a dark and a light bar
        let mut bar = image::RgbaImage::from_pixel(36 * 8, 72, image::Rgba([27, 32, 29, 255]));
        for (i, (mood, _)) in moods.into_iter().enumerate() {
            let px = icon(&pose(mood, 0.0, (0.0, 0.0)), 36);
            over(&mut bar, &px, 36, i as u32 * 36, 0, true);
            for y in 36..72 {
                for x in i as u32 * 36..(i as u32 + 1) * 36 {
                    bar.put_pixel(x, y, image::Rgba([236, 236, 236, 255]));
                }
            }
            over(&mut bar, &px, 36, i as u32 * 36, 36, false);
        }
        bar.save(dir.join("bar.png")).unwrap();
    }

    #[test]
    fn the_cat_over_its_card_is_the_whole_picture() {
        for mood in [Mood::Hello, Mood::Purr, Mood::Hiss, Mood::Hungry, Mood::Eat] {
            let p = pose(mood, 0.7, (0.0, 0.0));
            let (whole, card, cat) = (bezel(&p, 2.0), card(2.0), bezel_cat(&p, 2.0));
            for i in (0..whole.len()).step_by(4) {
                // premultiplied: cat + card × (1 − cat alpha)
                let a = cat[i + 3] as u32;
                for c in 0..4 {
                    let laid = cat[i + c] as u32 + (card[i + c] as u32 * (255 - a) + 127) / 255;
                    assert!(laid.abs_diff(whole[i + c] as u32) <= 2, "{mood:?} byte {}", i + c);
                }
            }
        }
    }
}
