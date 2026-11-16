//! The desktop cat, alive: what it wants and does, frame by frame, on the
//! skeleton of `rig`. It looks at the pointer, swipes at it with its claws
//! out, gets bored, washes, lies down, is scared by a slap from whatever it
//! was doing, purrs under a held pointer, races about after three clicks,
//! and tidies the desktop by pushing the real icons with its nose and paw
//! into Finder's grid.
//!
//! Pure: points down from the top left of the screen, milliseconds. The
//! Mac side feeds it the pointer and what the Mac felt, and draws the
//! `Scene` it returns (one layer per part).

use super::rig::{self, Local, Rig, St, World, D2R};
use std::collections::HashMap;

/// Points per pack unit: the cat is about 150 pt tall.
pub const S: f64 = 0.1;
/// The face parts are drawn this much smaller, around the eyes and nose.
const FS: f64 = 0.92;
/// The pack's moves play this much faster (they are slow for a desktop);
/// the walk faster still, so its stride matches the ground it covers.
const RATE: f64 = 1.3;
const WALK_RATE: f64 = 1.9;
/// Points a second the walk covers (its stride × its pace).
const WALK: f64 = 1.15 * WALK_RATE * S * 1000.0;
/// Sliding friction of a pushed icon, points/s².
const FR: f64 = 820.0;

/// An icon on the desktop the cat can move: its id (the Mac side's), the
/// middle of its picture, and how it slides.
#[derive(Debug, Clone)]
pub struct Icon {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    vx: f64,
    vy: f64,
    pub rot: f64,
    vr: f64,
    /// at the cat's nose
    nose: bool,
    slot: Option<(f64, f64)>,
    /// in its slot
    home: bool,
    /// moved since the Mac last placed it
    lifted: bool,
}

/// What the Mac side should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// This icon starts to move: show its picture in place of Finder's.
    Lift(usize),
    /// It came to rest here: Finder places it, then its picture goes.
    Rest { id: usize, x: f64, y: f64 },
    /// The tidy is over.
    Tidied,
    /// A paw or a nose met an icon (soft: a pat).
    Tap { soft: bool },
    /// A held pointer on the cat: the purr starts or stops.
    Purr(bool),
    /// It races off (three clicks), or comes to tidy.
    Mew,
}

/// One part to draw: its picture, where (an affine map from the picture's
/// pixels to screen points, y down), how opaque, and claws out on the paw.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub file: [u16; 2],
    pub m: [f64; 6],
    pub alpha: f64,
    pub claws: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub parts: Vec<Part>,
    /// The icons being moved: id, middle, turn in degrees.
    pub icons: Vec<(usize, f64, f64, f64)>,
    /// The food bowl's middle bottom, while it eats.
    pub bowl: Option<(f64, f64)>,
    /// The feet's shadow (middle).
    pub feet: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// off the screen
    Away,
    Enter,
    Leave,
    Sit,
    Wash,
    Lie,
    SitDown,
    Scare,
    Pet,
    Tidy,
    WalkHome,
    Zoom,
    Eat,
    Beg,
    Hello,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Crouch,
    Strike,
    Hold,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Pick,
    Plan,
    Approach,
    Aim,
    Push,
    Swat,
    Watch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    Nose,
    Paw,
}

struct Ids {
    head: usize,
    neck: usize,
    body: usize,
    tail: usize,
    up: usize,
    low: usize,
    paw: usize,
    ear_top: Option<usize>,
    ear_back: Option<usize>,
    mouth: Option<usize>,
    tail_tip: Option<usize>,
    pupils: Vec<usize>,
    paw_part: Option<usize>,
}

#[derive(Default)]
struct Swipe {
    phase: Option<Phase>,
    t0: f64,
    dur: [f64; 4],
    w: f64,
    claws: f64,
    again: bool,
    /// an icon it aims at (else the pointer), the way to send it and how far
    at: Option<usize>,
    dir: Option<(f64, f64)>,
    power: f64,
    hit: bool,
    soft: bool,
}

impl Swipe {
    fn dur(&self, p: Phase) -> f64 {
        self.dur[p as usize]
    }
}

struct Mind {
    interest: f64,
    bored: f64,
    quiet: f64,
    next_idle: f64,
    glance: Option<(f64, f64, f64)>,
    ear: f64,
    ear_side: f64,
    swish: f64,
    yawn: f64,
    last_swipe: f64,
}

struct Tidy {
    icon: Option<usize>,
    step: Option<Step>,
    t0: f64,
    face: f64,
    tries: u32,
    how: How,
    resume: bool,
}

struct Clip {
    name: &'static str,
    t0: f64,
}

pub struct Live {
    rig: Rig,
    ids: Ids,
    /// picture sizes in pixels, by file
    sizes: HashMap<[u16; 2], (f64, f64)>,
    open: Option<[u16; 2]>,
    closed: Option<[u16; 2]>,
    puffed: [u16; 2],
    /// the screen's size, and the part of it the cat walks on (x0, y0, x1, y1)
    w: f64,
    h: f64,
    ground: (f64, f64, f64, f64),
    /// it lives on screen (else it comes for a reaction and goes)
    lives: bool,
    cur: Clip,
    prev: Option<Clip>,
    blend_t0: f64,
    blend_ms: f64,
    x: f64,
    y: f64,
    dir: f64,
    flip: f64,
    mode: Mode,
    mode_t0: f64,
    until: f64,
    mind: Mind,
    scare: f64,
    scare_t0: f64,
    petting: bool,
    zoom: (u32, f64),
    sw: Swipe,
    look: (f64, f64, f64),
    nudge_t0: f64,
    /// nose and shoulder ahead of and above the feet, measured from the pose
    rel_nose: (f64, f64),
    rel_sh: (f64, f64),
    shoulder: Option<(f64, f64)>,
    reach: f64,
    nose: (f64, f64),
    last_paw: Option<(f64, f64)>,
    bounds: Option<(f64, f64, f64, f64)>,
    tidy: Tidy,
    pub icons: Vec<Icon>,
    /// half the width and height of an icon with its label
    cell: (f64, f64),
    ptr: Ptr,
    clicks: Vec<f64>,
    seed: u32,
    last: f64,
    out: Vec<Out>,
}

#[derive(Default)]
struct Ptr {
    x: f64,
    y: f64,
    on: bool,
    down: bool,
    down_at: f64,
    down_on_cat: bool,
    v: f64,
    moved: f64,
    stim: f64,
    last: Option<(f64, f64, f64)>,
}

const ANIMS: [&str; 12] = ["idle", "walk", "run", "sitting", "sit_tranistion_to", "sit_transition_from", "clean", "sleeping", "sleep_transition_to", "sleep_transition_from", "beg", "scratch"];

fn anim(name: &str) -> &'static str {
    ANIMS.iter().find(|a| **a == name).copied().unwrap_or("idle")
}

fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Overshoots, then settles.
fn back(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let c = 1.9;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

fn wrap(a: f64) -> f64 {
    a.sin().atan2(a.cos())
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

impl Live {
    /// `sizes`: each picture's size in pixels as it was decoded.
    pub fn new(rig: Rig, sizes: HashMap<[u16; 2], (f64, f64)>, seed: u32) -> Option<Live> {
        let id = |n: &str| rig.id(n);
        let ids = Ids {
            head: id("bone_011")?,
            neck: id("bone_009")?,
            body: id("bone_007")?,
            tail: id("bone_008")?,
            up: id("bone_020")?,
            low: id("bone_021")?,
            paw: id("bone_022")?,
            ear_top: id("ear_top"),
            ear_back: id("ear_back"),
            mouth: id("mouth"),
            tail_tip: id("tail_05"),
            pupils: ["blue", "blue_000"].iter().filter_map(|n| id(n)).collect(),
            paw_part: id("paw_01_001"),
        };
        let file = |end: &str| rig.files.iter().find(|(_, f)| f.name.ends_with(end)).map(|(k, _)| *k);
        let (open, closed) = (file("/eyes_open.png"), file("/eyes_sleeping.png"));
        Some(Live {
            ids,
            sizes,
            open,
            closed,
            puffed: [10, 8],
            rig,
            w: 0.0,
            h: 0.0,
            ground: (0.0, 0.0, 0.0, 0.0),
            lives: true,
            cur: Clip { name: "sitting", t0: 0.0 },
            prev: None,
            blend_t0: 0.0,
            blend_ms: 160.0,
            x: f64::NAN,
            y: f64::NAN,
            dir: -1.0,
            flip: 1.0,
            mode: Mode::Away,
            mode_t0: 0.0,
            until: 0.0,
            mind: Mind { interest: 0.0, bored: 0.0, quiet: 0.0, next_idle: 2500.0, glance: None, ear: -1e9, ear_side: 1.0, swish: -1e9, yawn: -1e9, last_swipe: -1e9 },
            scare: 0.0,
            scare_t0: -1e9,
            petting: false,
            zoom: (0, 0.0),
            sw: Swipe::default(),
            look: (0.0, 0.0, 0.0),
            nudge_t0: -1e9,
            rel_nose: (56.0, -112.0),
            rel_sh: (16.0, -55.0),
            shoulder: None,
            reach: 44.0,
            nose: (0.0, 0.0),
            last_paw: None,
            bounds: None,
            tidy: Tidy { icon: None, step: None, t0: 0.0, face: -1.0, tries: 0, how: How::Paw, resume: false },
            icons: Vec::new(),
            cell: (47.0, 37.0),
            ptr: Ptr { x: -1e4, y: -1e4, moved: -1e9, ..Ptr::default() },
            clicks: Vec::new(),
            seed: seed | 1,
            last: 0.0,
            out: Vec::new(),
        })
    }

    // ---------------------------------------------------------------- input

    /// The screen's size and where the cat may walk (the usable area, above the Dock).
    pub fn set_area(&mut self, w: f64, h: f64, ground: (f64, f64, f64, f64)) {
        self.w = w;
        self.h = h;
        self.ground = ground;
    }

    /// Where it rests: the bottom right corner of the usable area.
    fn home(&self) -> (f64, f64) {
        (self.ground.2 - 70.0, self.ground.3 - 14.0)
    }

    fn clamp_x(&self, x: f64) -> f64 {
        x.clamp(self.ground.0 + 50.0, (self.ground.2 - 50.0).max(self.ground.0 + 50.0))
    }

    fn clamp_y(&self, y: f64) -> f64 {
        y.clamp(self.ground.1 + 95.0, (self.ground.3 - 8.0).max(self.ground.1 + 95.0))
    }

    /// It lives on screen in its corner, or only comes for a reaction.
    pub fn set_lives(&mut self, on: bool, now: f64) {
        self.lives = on;
        if on && self.mode == Mode::Away {
            self.enter(now);
        } else if !on && matches!(self.mode, Mode::Sit | Mode::Lie | Mode::Wash) {
            self.set_mode(Mode::Leave, now);
        }
    }

    pub fn shown(&self) -> bool {
        self.mode != Mode::Away
    }

    /// The pointer (None: on another screen), and whether its button is down.
    pub fn pointer(&mut self, at: Option<(f64, f64)>, down: bool, now: f64) {
        match at {
            Some((x, y)) => {
                if let Some((lx, ly, lt)) = self.ptr.last {
                    if now > lt {
                        let v = (x - lx).hypot(y - ly) / ((now - lt) / 1000.0);
                        self.ptr.v = self.ptr.v * 0.6 + v * 0.4;
                        if v > 60.0 {
                            self.ptr.moved = now;
                        }
                        let d = (x - self.x).hypot(y - (self.y - 60.0));
                        if d.is_finite() {
                            self.ptr.stim += (v / 900.0).min(1.0) * (1.0 - d / 650.0).max(0.0) * 0.25 * ((now - lt) / 16.7).min(3.0);
                        }
                    }
                }
                self.ptr.last = Some((x, y, now));
                self.ptr.x = x;
                self.ptr.y = y;
                self.ptr.on = true;
            }
            None => {
                self.ptr.on = false;
                self.ptr.last = None;
            }
        }
        if down && !self.ptr.down {
            self.ptr.down_at = now;
            self.ptr.down_on_cat = self.over_cat();
            if self.ptr.down_on_cat {
                self.clicks.push(now);
                self.clicks.retain(|t| now - t <= 1300.0);
            }
        }
        self.ptr.down = down;
    }

    /// The pointer is over the cat (it catches clicks there).
    pub fn over_cat(&self) -> bool {
        self.ptr.on && self.bounds.is_some_and(|(x0, y0, x1, y1)| self.ptr.x >= x0 && self.ptr.x <= x1 && self.ptr.y >= y0 && self.ptr.y <= y1)
    }

    /// A slap: scared at once, from anything (a tidy goes on afterwards).
    pub fn slap(&mut self, now: f64) {
        if self.mode == Mode::Away {
            self.enter(now);
        }
        if self.petting {
            self.petting = false;
            self.out.push(Out::Purr(false));
        }
        let tidying = self.mode == Mode::Tidy;
        self.stop_all();
        self.tidy.resume = tidying;
        self.scare_t0 = now;
        self.set_mode(Mode::Scare, now);
        self.go("idle", 90.0, now);
        self.mind.interest = 0.0;
        self.mind.bored = 1.0;
    }

    /// A hand on the trackpad (or a held pointer): it curls up and purrs.
    pub fn pet(&mut self, on: bool, now: f64) {
        if on {
            if matches!(self.mode, Mode::Scare | Mode::Zoom | Mode::Tidy | Mode::Leave) {
                return;
            }
            if self.mode == Mode::Away {
                self.place_home();
            }
            self.stop_all();
            self.set_mode(Mode::Pet, now);
            self.go("sleeping", 380.0, now);
        } else if self.mode == Mode::Pet {
            self.set_mode(Mode::Sit, now);
            self.go("sitting", 380.0, now);
            self.mind.bored = 0.6;
            self.mind.quiet = now;
        }
    }

    /// The lid opened: it gets up, ears up, and looks around.
    pub fn hello(&mut self, now: f64) {
        if self.mode == Mode::Away {
            return self.enter(now);
        }
        if matches!(self.mode, Mode::Tidy | Mode::Scare | Mode::Zoom) {
            return;
        }
        self.stop_all();
        self.set_mode(Mode::Hello, now);
        self.go("idle", 300.0, now);
        self.mind.ear = now;
        self.mind.interest = 0.8;
    }

    /// On the charger: it eats from its bowl.
    pub fn eat(&mut self, now: f64) {
        if matches!(self.mode, Mode::Tidy | Mode::Scare | Mode::Zoom) {
            return;
        }
        if self.mode == Mode::Away {
            self.place_home();
        }
        self.stop_all();
        self.set_mode(Mode::Eat, now);
        self.go("idle", 300.0, now);
    }

    /// The battery is low: it begs.
    pub fn beg(&mut self, now: f64) {
        if matches!(self.mode, Mode::Tidy | Mode::Scare | Mode::Zoom) {
            return;
        }
        if self.mode == Mode::Away {
            self.place_home();
        }
        self.stop_all();
        self.set_mode(Mode::Beg, now);
        self.go("beg", 300.0, now);
    }

    /// Tidy these icons into these slots (the middle of each picture).
    /// `cell`: half the width and height an icon and its label take.
    pub fn tidy(&mut self, icons: Vec<(usize, f64, f64)>, slots: &[(f64, f64)], cell: (f64, f64), now: f64) {
        if self.mode == Mode::Away {
            self.place_home();
        }
        if self.petting {
            self.petting = false;
            self.out.push(Out::Purr(false));
        }
        self.stop_all();
        self.cell = cell;
        self.icons = icons.into_iter().map(|(id, x, y)| Icon { id, x, y, vx: 0.0, vy: 0.0, rot: 0.0, vr: 0.0, nose: false, slot: None, home: false, lifted: false }).collect();
        self.assign(slots);
        self.set_mode(Mode::Tidy, now);
        self.tidy.step = Some(Step::Pick);
        self.out.push(Out::Mew);
    }

    pub fn tidying(&self) -> bool {
        self.mode == Mode::Tidy || self.icons.iter().any(|i| i.lifted)
    }

    /// What happened since it was last asked.
    pub fn take(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// How often it wants drawing now (0: it is still, or away).
    pub fn fps(&self) -> f64 {
        match self.mode {
            Mode::Away => {
                if self.icons.iter().any(|i| i.lifted) {
                    60.0
                } else {
                    0.0
                }
            }
            Mode::Lie | Mode::Pet => 20.0,
            Mode::Sit if self.sw.phase.is_none() && self.mind.interest < 0.2 => 30.0,
            _ => 60.0,
        }
    }

    // ---------------------------------------------------------------- the mind

    fn random(&mut self) -> f64 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x as f64 / u32::MAX as f64
    }

    fn rnd(&mut self, a: f64, b: f64) -> f64 {
        a + self.random() * (b - a)
    }

    fn set_mode(&mut self, m: Mode, now: f64) {
        self.mode = m;
        self.mode_t0 = now;
    }

    fn go(&mut self, name: &str, ms: f64, now: f64) {
        let name = anim(name);
        if self.cur.name == name {
            return;
        }
        self.prev = Some(Clip { name: self.cur.name, t0: self.cur.t0 });
        self.cur = Clip { name, t0: now };
        self.blend_t0 = now;
        self.blend_ms = ms;
    }

    fn rate(name: &str) -> f64 {
        if name == "walk" {
            WALK_RATE
        } else {
            RATE
        }
    }

    fn anim_done(&self, now: f64) -> bool {
        self.rig.anims.get(self.cur.name).is_some_and(|a| !a.looped && (now - self.cur.t0) * Self::rate(self.cur.name) >= a.len)
    }

    fn place_home(&mut self) {
        let (x, y) = self.home();
        self.x = x;
        self.y = y;
        self.dir = -1.0;
        self.flip = 1.0;
    }

    /// In from the right edge, to its corner.
    fn enter(&mut self, now: f64) {
        let (_, y) = self.home();
        self.x = self.ground.2 + 90.0;
        self.y = y;
        self.dir = -1.0;
        self.flip = 1.0;
        self.set_mode(Mode::Enter, now);
        self.go("walk", 10.0, now);
    }

    fn stop_all(&mut self) {
        self.sw.phase = None;
        self.sw.at = None;
        self.tidy.icon = None;
        for i in &mut self.icons {
            i.nose = false;
        }
    }

    fn sit_down(&mut self, ms: f64, now: f64) {
        self.set_mode(Mode::SitDown, now);
        self.go("sit_tranistion_to", ms, now);
    }

    fn swipe(&mut self, now: f64, quick: bool, at: Option<usize>, dir: Option<(f64, f64)>, power: f64, soft: bool) {
        self.sw.phase = Some(if quick { Phase::Strike } else { Phase::Crouch });
        self.sw.t0 = now;
        self.sw.hit = false;
        self.sw.at = at;
        self.sw.dir = dir;
        self.sw.power = power;
        self.sw.soft = soft;
        let k = if soft { 1.5 } else { 1.0 };
        let crouch = if soft { self.rnd(120.0, 200.0) } else { self.rnd(200.0, 380.0) };
        let strike = self.rnd(80.0, 115.0) * k;
        let hold = self.rnd(40.0, 90.0);
        let back = self.rnd(190.0, 260.0);
        self.sw.dur = [crouch, strike, hold, back];
        self.sw.again = !quick && at.is_none() && self.random() < 0.3;
        self.mind.last_swipe = now;
    }

    fn idle(&mut self, now: f64) {
        let long = now - self.mind.quiet > 18000.0;
        let list = [(0, 34.0), (1, 20.0), (2, 14.0), (3, 12.0), (4, 10.0), (5, if long { 14.0 } else { 0.0 })];
        let mut t = self.random() * list.iter().map(|x| x.1).sum::<f64>();
        let mut what = 0;
        for (v, w) in list {
            t -= w;
            if t <= 0.0 {
                what = v;
                break;
            }
        }
        match what {
            0 => {
                let (gx, gy) = (self.rnd(self.w * 0.05, self.w * 0.7), self.rnd(self.h * 0.1, self.h * 0.8));
                let until = now + self.rnd(700.0, 1600.0);
                self.mind.glance = Some((gx, gy, until));
            }
            1 => {
                self.mind.ear = now;
                self.mind.ear_side = if self.random() < 0.5 { 1.0 } else { -1.0 };
            }
            2 => self.mind.swish = now,
            3 => {
                self.set_mode(Mode::Wash, now);
                self.go("clean", 300.0, now);
            }
            4 => self.mind.yawn = now,
            _ => {
                self.set_mode(Mode::Lie, now);
                self.go("sleeping", 600.0, now);
            }
        }
        self.mind.next_idle = now + self.rnd(1800.0, 4500.0);
    }

    /// One step towards `to`; true once there.
    fn walk_to(&mut self, to: (f64, f64), dt: f64, run: bool, now: f64) -> bool {
        let (dx, dy) = (to.0 - self.x, to.1 - self.y);
        let d = dx.hypot(dy);
        if d < 4.0 {
            self.x = to.0;
            self.y = to.1;
            return true;
        }
        let v = if run { WALK * 2.3 } else { WALK } * dt / 1000.0;
        if dx.abs() > 6.0 {
            self.dir = dx.signum();
        }
        self.go(if run { "run" } else { "walk" }, 160.0, now);
        // a turn first, then the walk (no moonwalking)
        if (self.flip - self.want_flip()).abs() > 1.2 {
            return false;
        }
        let k = (v / d).min(1.0);
        self.x += dx * k;
        self.y += dy * k;
        false
    }

    fn want_flip(&self) -> f64 {
        if self.dir < 0.0 {
            1.0
        } else {
            -1.0
        }
    }

    fn turned(&self, by: f64) -> bool {
        (self.flip - self.want_flip()).abs() <= by
    }

    // ---------------------------------------------------------------- icons

    /// The slots in the order of the picture: the top icons take the top slots.
    fn assign(&mut self, slots: &[(f64, f64)]) {
        let mut order: Vec<usize> = (0..self.icons.len()).collect();
        order.sort_by(|&a, &b| {
            let (p, q) = (&self.icons[a], &self.icons[b]);
            (p.y + p.x * 0.05).total_cmp(&(q.y + q.x * 0.05))
        });
        let mut free: Vec<(f64, f64)> = slots.to_vec();
        for i in order {
            if free.is_empty() {
                break;
            }
            let at = (self.icons[i].x, self.icons[i].y);
            let best = (0..free.len()).min_by(|&a, &b| (dist(free[a], at) + a as f64 * 40.0).total_cmp(&(dist(free[b], at) + b as f64 * 40.0))).unwrap_or(0);
            let slot = free.remove(best);
            let ic = &mut self.icons[i];
            ic.slot = Some(slot);
            ic.home = dist(slot, at) < 4.0;
        }
    }

    fn speed(i: &Icon) -> f64 {
        i.vx.hypot(i.vy)
    }

    fn lift(&mut self, k: usize) {
        if !self.icons[k].lifted {
            self.icons[k].lifted = true;
            self.out.push(Out::Lift(self.icons[k].id));
        }
    }

    fn physics(&mut self, dt: f64) {
        let s = dt / 1000.0;
        let (gx0, gy0, gx1, gy1) = self.ground;
        let (bw, bh) = self.cell;
        for k in 0..self.icons.len() {
            let i = &mut self.icons[k];
            if i.nose {
                continue;
            }
            i.x += i.vx * s;
            i.y += i.vy * s;
            i.rot += i.vr * s;
            let v = Self::speed(i);
            if v > 0.0 {
                let f = (v - FR * s).max(0.0) / v;
                i.vx *= f;
                i.vy *= f;
            }
            i.vr *= (1.0 - 6.0 * s).max(0.0);
            i.rot *= (1.0 - if v < 30.0 { 8.0 } else { 2.0 } * s).max(0.0);
            // the edges of the screen
            if i.x < gx0 + bw * 0.7 {
                i.x = gx0 + bw * 0.7;
                i.vx = i.vx.abs() * 0.4;
            }
            if i.x > gx1 - bw * 0.7 {
                i.x = gx1 - bw * 0.7;
                i.vx = -i.vx.abs() * 0.4;
            }
            if i.y < gy0 + 28.0 {
                i.y = gy0 + 28.0;
                i.vy = i.vy.abs() * 0.4;
            }
            if i.y > gy1 - bh * 1.25 {
                i.y = gy1 - bh * 1.25;
                i.vy = -i.vy.abs() * 0.4;
            }
            // a slow icon near its slot settles into it, like Finder's grid snap
            if let Some(slot) = i.slot {
                if !i.home && dist((i.x, i.y), slot) < 12.0 && v < 40.0 {
                    i.vx = 0.0;
                    i.vy = 0.0;
                    i.home = true;
                }
                if i.home {
                    let k = (s * 14.0).min(1.0);
                    i.x += (slot.0 - i.x) * k;
                    i.y += (slot.1 - i.y) * k;
                }
            }
        }
        // icons push each other: an icon with its label is a box
        let n = self.icons.len();
        let mut lifts = Vec::new();
        let mut taps = 0.0f64;
        for a in 0..n {
            for b in a + 1..n {
                let (p, q) = (&self.icons[a], &self.icons[b]);
                let (dx, dy) = (q.x - p.x, q.y - p.y);
                let (ox, oy) = (bw * 2.0 - dx.abs(), bh * 2.0 - dy.abs());
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                let along_x = ox / bw < oy / bh;
                let (nx, ny, over) = if along_x { (if dx < 0.0 { -1.0 } else { 1.0 }, 0.0, ox) } else { (0.0, if dy < 0.0 { -1.0 } else { 1.0 }, oy) };
                let fa = if p.nose { 0.0 } else if q.nose { 1.0 } else { 0.5 };
                let fb = 1.0 - fa;
                let vn = (q.vx - p.vx) * nx + (q.vy - p.vy) * ny;
                {
                    let p = &mut self.icons[a];
                    p.x -= nx * over * fa;
                    p.y -= ny * over * fa;
                }
                {
                    let q = &mut self.icons[b];
                    q.x += nx * over * fb;
                    q.y += ny * over * fb;
                }
                if fa > 0.0 {
                    lifts.push(a);
                    self.icons[a].home = false;
                }
                if fb > 0.0 {
                    lifts.push(b);
                    self.icons[b].home = false;
                }
                if vn < 0.0 {
                    let j = -(1.0 + 0.55) * vn;
                    if fa > 0.0 {
                        let p = &mut self.icons[a];
                        p.vx -= nx * j * fa;
                        p.vy -= ny * j * fa;
                        p.vr -= vn * 0.02;
                    }
                    if fb > 0.0 {
                        let q = &mut self.icons[b];
                        q.vx += nx * j * fb;
                        q.vy += ny * j * fb;
                        q.vr += vn * 0.02;
                    }
                    taps = taps.max(-vn);
                }
            }
        }
        for k in lifts {
            self.lift(k);
        }
        if taps > 120.0 {
            self.out.push(Out::Tap { soft: true });
        }
        // what came to rest goes back to Finder
        for k in 0..self.icons.len() {
            let i = &self.icons[k];
            let settled = i.slot.is_none_or(|s| !i.home || dist((i.x, i.y), s) < 0.6);
            if i.lifted && !i.nose && Self::speed(i) < 0.5 && settled && self.sw.at != Some(k) && self.tidy.icon.is_none_or(|t| t != k || !matches!(self.tidy.step, Some(Step::Push))) {
                let (id, x, y) = (i.id, i.slot.filter(|_| i.home).map_or(i.x, |s| s.0), i.slot.filter(|_| i.home).map_or(i.y, |s| s.1));
                let i = &mut self.icons[k];
                i.lifted = false;
                i.rot = 0.0;
                self.out.push(Out::Rest { id, x, y });
            }
        }
    }

    fn nose_stand(&self, k: usize, face: f64) -> (f64, f64) {
        let i = &self.icons[k];
        (self.clamp_x(i.x - face * (self.rel_nose.0 + 36.0)), self.clamp_y(i.y - self.rel_nose.1 - 20.0))
    }

    fn paw_stand(&self, k: usize, face: f64) -> (f64, f64) {
        let i = &self.icons[k];
        (self.clamp_x(i.x - face * (self.rel_sh.0 + self.reach * 0.62)), self.clamp_y(i.y - self.rel_sh.1 - self.reach * 0.38))
    }

    fn tidy_think(&mut self, now: f64, dt: f64) {
        let Some(step) = self.tidy.step else { return };
        let ic = self.tidy.icon;
        match step {
            Step::Pick => {
                let left: Vec<usize> = (0..self.icons.len()).filter(|&k| !self.icons[k].home && self.icons[k].slot.is_some()).collect();
                if left.is_empty() {
                    self.tidy.step = None;
                    self.set_mode(Mode::WalkHome, now);
                    self.mind.bored = 0.3;
                    self.out.push(Out::Tidied);
                    return;
                }
                if left.iter().any(|&k| Self::speed(&self.icons[k]) > 30.0) {
                    self.go("idle", 200.0, now);
                    return;
                }
                let me = (self.x, self.y);
                let k = *left.iter().min_by(|&&a, &&b| dist((self.icons[a].x, self.icons[a].y), me).total_cmp(&dist((self.icons[b].x, self.icons[b].y), me))).unwrap_or(&left[0]);
                self.tidy.icon = Some(k);
                self.tidy.tries = 0;
                self.tidy.step = Some(Step::Plan);
            }
            Step::Plan => {
                let Some(k) = ic.filter(|&k| !self.icons[k].home) else {
                    self.tidy.step = Some(Step::Pick);
                    return;
                };
                // trade slots instead of shoving a neighbour out: whoever sits on a slot keeps it
                let slot = self.icons[k].slot.unwrap_or((self.icons[k].x, self.icons[k].y));
                let at = (self.icons[k].x, self.icons[k].y);
                let near = (0..self.icons.len()).find(|&o| o != k && !self.icons[o].home && self.icons[o].slot.is_some_and(|s| dist(at, s) < 44.0));
                let sitter = (0..self.icons.len()).find(|&o| o != k && !self.icons[o].home && self.icons[o].slot.is_some() && dist((self.icons[o].x, self.icons[o].y), slot) < 44.0);
                if let Some(o) = near.or(sitter) {
                    let t = self.icons[o].slot;
                    self.icons[o].slot = self.icons[k].slot;
                    self.icons[k].slot = t;
                    for q in [o, k] {
                        let i = &mut self.icons[q];
                        if i.slot.is_some_and(|s| dist((i.x, i.y), s) < 12.0) {
                            i.home = true;
                            // it is where it belongs; Finder places it there
                            if !i.lifted {
                                i.lifted = true;
                                self.out.push(Out::Lift(i.id));
                            }
                        }
                    }
                    if self.icons[k].home {
                        self.tidy.step = Some(Step::Pick);
                        return;
                    }
                }
                let i = &self.icons[k];
                let Some(slot) = i.slot else { return };
                let (dx, dy) = (slot.0 - i.x, slot.1 - i.y);
                if dx.hypot(dy) < 12.0 {
                    self.tidy.step = Some(Step::Pick);
                    return;
                }
                self.tidy.face = if dx.abs() > 20.0 { dx.signum() } else { (i.x - self.x).signum() };
                if self.tidy.face == 0.0 {
                    self.tidy.face = self.dir;
                }
                self.tidy.how = if dx.abs() > 170.0 && dx.abs() > dy.abs() * 1.2 && self.tidy.tries < 4 { How::Nose } else { How::Paw };
                self.tidy.step = Some(Step::Approach);
            }
            Step::Approach => {
                let Some(k) = ic else { return };
                let to = if self.tidy.how == How::Nose { self.nose_stand(k, self.tidy.face) } else { self.paw_stand(k, self.tidy.face) };
                let run = dist(to, (self.x, self.y)) > 150.0;
                if self.walk_to(to, dt, run, now) {
                    self.dir = self.tidy.face;
                    self.go("idle", 160.0, now);
                    self.tidy.step = Some(Step::Aim);
                    self.tidy.t0 = now;
                }
            }
            Step::Aim => {
                let Some(k) = ic else { return };
                if now - self.tidy.t0 < 160.0 || !self.turned(0.2) {
                    return;
                }
                if Self::speed(&self.icons[k]) > 20.0 {
                    self.tidy.step = Some(Step::Plan);
                    return;
                }
                if self.tidy.how == How::Nose {
                    self.lift(k);
                    let i = &mut self.icons[k];
                    i.nose = true;
                    i.home = false;
                    self.tidy.step = Some(Step::Push);
                    self.tidy.t0 = now;
                    self.out.push(Out::Tap { soft: true });
                } else {
                    let i = &self.icons[k];
                    let Some(slot) = i.slot else { return };
                    let (dx, dy) = (slot.0 - i.x, slot.1 - i.y);
                    let d = dx.hypot(dy).max(0.001);
                    let near = d < 70.0;
                    let power = d * if near { 1.0 } else { self.rnd(0.8, 1.05) };
                    self.swipe(now, false, Some(k), Some((dx / d, dy / d)), power, near);
                    self.tidy.step = Some(Step::Swat);
                    self.tidy.tries += 1;
                }
            }
            Step::Push => {
                let Some(k) = ic else { return };
                let i = &self.icons[k];
                let Some(slot) = i.slot else { return };
                let (dx, dy) = (slot.0 - i.x, slot.1 - i.y);
                let d = dx.hypot(dy);
                if d < 46.0 || dx * self.tidy.face < 10.0 {
                    let i = &mut self.icons[k];
                    i.nose = false;
                    let v = (2.0 * FR * (d - 4.0).max(0.0)).sqrt();
                    i.vx = dx / d.max(0.001) * v;
                    i.vy = dy / d.max(0.001) * v;
                    self.nudge_t0 = now;
                    self.out.push(Out::Tap { soft: true });
                    self.go("idle", 160.0, now);
                    self.tidy.step = Some(Step::Watch);
                    self.tidy.t0 = now;
                    self.tidy.tries += 1;
                    return;
                }
                self.go("walk", 160.0, now);
                let v = WALK * dt / 1000.0;
                self.x = self.clamp_x(self.x + dx / d * v);
                self.y = self.clamp_y(self.y + dy / d * v);
            }
            Step::Swat => {
                if self.sw.phase.is_none() {
                    self.tidy.step = Some(Step::Watch);
                    self.tidy.t0 = now;
                }
            }
            Step::Watch => {
                let Some(k) = ic else { return };
                if now - self.tidy.t0 > 250.0 && Self::speed(&self.icons[k]) < 8.0 {
                    if self.tidy.tries > 7 {
                        // it never got there: Finder gets it there
                        let i = &mut self.icons[k];
                        if let Some(s) = i.slot {
                            i.x = s.0;
                            i.y = s.1;
                            i.home = true;
                        }
                    }
                    self.tidy.step = Some(if self.icons[k].home { Step::Pick } else { Step::Plan });
                }
            }
        }
    }

    /// The icon at the nose rides just ahead of it, bobbing with the step.
    fn nose_hold(&mut self, now: f64) {
        let (nx, ny, face) = (self.nose.0, self.nose.1, self.tidy.face);
        for i in &mut self.icons {
            if i.nose {
                let (tx, ty) = (nx + face * 36.0, ny + 20.0 + (now / 95.0).sin() * 1.5);
                i.x += (tx - i.x) * 0.35;
                i.y += (ty - i.y) * 0.35;
                i.vx = 0.0;
                i.vy = 0.0;
                i.rot += (-4.0 * face - i.rot) * 0.1;
            }
        }
    }

    fn hit_icon(&mut self, k: usize, swing: (f64, f64)) {
        self.sw.hit = true;
        let (dx, dy, v) = match self.sw.dir.filter(|_| self.sw.at == Some(k)) {
            Some((dx, dy)) => (dx, dy, (2.0 * FR * self.sw.power.max(0.0)).sqrt()),
            None => {
                let d = swing.0.hypot(swing.1).max(0.001);
                (swing.0 / d, swing.1 / d, 340.0)
            }
        };
        let spin = self.rnd(20.0, 60.0);
        self.lift(k);
        let i = &mut self.icons[k];
        i.home = false;
        i.vx = dx * v;
        i.vy = dy * v;
        i.vr = dx * spin;
        self.out.push(Out::Tap { soft: self.sw.soft });
    }

    fn think(&mut self, now: f64, dt: f64) {
        let o = self.home();
        if !self.x.is_finite() {
            self.place_home();
        }
        self.mind.interest = (self.mind.interest + self.ptr.stim - dt / 1000.0 * 0.22).clamp(0.0, 1.0);
        self.ptr.stim = 0.0;
        self.mind.bored = (self.mind.bored - dt / 1000.0 * 0.09).max(0.0);
        if self.mind.interest > 0.3 {
            self.mind.quiet = now;
        }
        if self.clicks.len() >= 3 && !matches!(self.mode, Mode::Zoom | Mode::Tidy | Mode::Away) {
            self.clicks.clear();
            if self.petting {
                self.petting = false;
                self.out.push(Out::Purr(false));
            }
            self.stop_all();
            self.set_mode(Mode::Zoom, now);
            self.go("run", 140.0, now);
            self.zoom = (0, now + 6500.0);
            self.out.push(Out::Mew);
        }
        // a pointer held on the cat pets it
        if self.ptr.down && self.ptr.down_on_cat && now - self.ptr.down_at > 350.0 && !self.petting && matches!(self.mode, Mode::Sit | Mode::Lie | Mode::Wash) {
            self.petting = true;
            self.pet(true, now);
            self.out.push(Out::Purr(true));
        }
        if !self.ptr.down && self.petting && self.ptr.down_on_cat {
            self.petting = false;
            self.ptr.down_on_cat = false;
            self.pet(false, now);
            self.out.push(Out::Purr(false));
        }
        let reachy = self.ptr.on && self.shoulder.is_some_and(|s| dist((self.ptr.x, self.ptr.y), s) < self.reach * 1.45);
        match self.mode {
            Mode::Away => {}
            Mode::Enter => {
                if self.walk_to(o, dt, false, now) {
                    self.dir = -1.0;
                    self.sit_down(180.0, now);
                }
            }
            Mode::Leave => {
                let to = (self.ground.2 + 90.0, self.y);
                if self.walk_to(to, dt, false, now) {
                    self.set_mode(Mode::Away, now);
                }
            }
            Mode::Sit => {
                let keen = self.mind.interest > 0.5 && self.mind.bored < 0.75 && now - self.ptr.moved < 600.0;
                let gap = self.rnd(300.0, 700.0);
                if self.sw.phase.is_none() && reachy && keen && now - self.mind.last_swipe > gap {
                    self.swipe(now, false, None, None, 0.0, false);
                }
                if self.sw.phase.is_none() && self.mind.bored >= 0.75 && self.mind.interest > 0.2 {
                    // had enough: looks away, and maybe washes or lies down
                    self.mind.interest = 0.0;
                    let g = (self.rnd(self.w * 0.05, self.w * 0.4), self.rnd(self.h * 0.1, self.h * 0.6), now + self.rnd(1200.0, 2200.0));
                    self.mind.glance = Some(g);
                    let r = self.random();
                    if r < 0.35 {
                        self.set_mode(Mode::Wash, now);
                        self.go("clean", 300.0, now);
                    } else if r < 0.5 {
                        self.set_mode(Mode::Lie, now);
                        self.go("sleeping", 600.0, now);
                    }
                }
                if self.sw.phase.is_none() && self.mind.interest < 0.3 && now > self.mind.next_idle {
                    self.idle(now);
                }
                if self.sw.phase.is_none() && now - self.mind.quiet > 9000.0 && dist((self.x, self.y), o) > 30.0 {
                    self.set_mode(Mode::WalkHome, now);
                }
                if !self.lives && self.sw.phase.is_none() && now - self.mode_t0 > 2500.0 && self.mind.interest < 0.3 {
                    self.set_mode(Mode::Leave, now);
                }
            }
            Mode::Wash => {
                if (self.mind.interest > 0.7 && self.mind.bored < 0.5) || now - self.mode_t0 > 2600.0 {
                    self.set_mode(Mode::Sit, now);
                    self.go("sitting", 260.0, now);
                }
            }
            Mode::Lie => {
                if self.until == 0.0 {
                    self.until = self.rnd(14000.0, 26000.0);
                }
                if self.mind.interest > 0.75 || now - self.mode_t0 > self.until {
                    self.until = 0.0;
                    self.set_mode(Mode::Sit, now);
                    self.go("sitting", 500.0, now);
                    self.mind.quiet = now;
                }
            }
            Mode::SitDown => {
                if self.anim_done(now) {
                    self.set_mode(Mode::Sit, now);
                    self.go("sitting", 120.0, now);
                }
            }
            Mode::Scare => {
                if now - self.scare_t0 > 1250.0 {
                    if std::mem::take(&mut self.tidy.resume) {
                        self.set_mode(Mode::Tidy, now);
                        self.tidy.step = Some(Step::Pick);
                    } else {
                        self.sit_down(220.0, now);
                    }
                }
            }
            Mode::Pet => {}
            Mode::Hello => {
                if now - self.mode_t0 > 2200.0 {
                    self.sit_down(220.0, now);
                }
            }
            Mode::Eat => {
                if now - self.mode_t0 > 4200.0 {
                    self.sit_down(220.0, now);
                }
            }
            Mode::Beg => {
                if now - self.mode_t0 > 2600.0 {
                    self.set_mode(Mode::Sit, now);
                    self.go("sitting", 300.0, now);
                }
            }
            Mode::Tidy => self.tidy_think(now, dt),
            Mode::WalkHome => {
                if now < self.mode_t0 {
                    self.go("idle", 200.0, now);
                } else if self.walk_to(o, dt, false, now) {
                    self.dir = -1.0;
                    self.sit_down(180.0, now);
                }
            }
            Mode::Zoom => {
                let tired = now > self.zoom.1 || self.zoom.0 >= 6;
                let goal = if tired {
                    o
                } else if self.ptr.on {
                    (self.clamp_x(self.ptr.x), self.clamp_y(self.ptr.y + 50.0))
                } else {
                    let y = self.rnd(self.h * 0.45, self.h * 0.85);
                    (if self.zoom.0 % 2 == 1 { self.w * 0.15 } else { self.w * 0.85 }, self.clamp_y(y))
                };
                if self.sw.phase.is_none() {
                    if dist(goal, (self.x, self.y)) < 18.0 {
                        if tired {
                            self.dir = -1.0;
                            self.sit_down(180.0, now);
                            self.mind.bored = 0.8;
                        } else {
                            self.swipe(now, true, None, None, 0.0, false);
                            self.zoom.0 += 1;
                            self.go("idle", 120.0, now);
                        }
                    } else {
                        let d0 = self.dir;
                        self.walk_to(goal, dt, !tired, now);
                        if self.dir != d0 {
                            self.zoom.0 += 1;
                        }
                    }
                }
            }
        }
        // the swipe's clock
        if let Some(p) = self.sw.phase {
            if now - self.sw.t0 > self.sw.dur(p) {
                if p == Phase::Strike && !self.sw.hit {
                    if let Some(k) = self.sw.at {
                        // a near miss still lands
                        self.hit_icon(k, (0.0, 0.0));
                    }
                }
                self.sw.t0 = now;
                self.sw.phase = match p {
                    Phase::Crouch => Some(Phase::Strike),
                    Phase::Strike => Some(Phase::Hold),
                    Phase::Hold => Some(Phase::Back),
                    Phase::Back => None,
                };
                if self.sw.phase.is_none() {
                    if self.sw.at.is_none() {
                        self.mind.bored += self.rnd(0.22, 0.4);
                    }
                    self.sw.at = None;
                    if self.sw.again && reachy {
                        self.sw.again = false;
                        self.swipe(now, true, None, None, 0.0, false);
                    }
                }
            }
        }
        if self.mind.glance.is_some_and(|g| now > g.2) {
            self.mind.glance = None;
        }
        let want = self.want_flip();
        self.flip += (want - self.flip).signum() * (want - self.flip).abs().min(dt / 70.0);
    }

    // ---------------------------------------------------------------- the pose

    /// Live one frame: think, move the icons, pose the body.
    pub fn tick(&mut self, now: f64) -> Scene {
        let dt = if self.last == 0.0 { 16.0 } else { (now - self.last).clamp(0.0, 50.0) };
        self.last = now;
        self.think(now, dt);
        self.physics(dt);
        let mut scene = if self.mode == Mode::Away { Scene::default() } else { self.pose(now, dt) };
        self.nose_hold(now);
        scene.icons = self.icons.iter().filter(|i| i.lifted).map(|i| (i.id, i.x, i.y, i.rot)).collect();
        if self.mode == Mode::Eat {
            scene.bowl = Some((self.x - self.dir * 70.0, self.y - 2.0));
        }
        scene
    }

    fn to_world(&self, p: (f64, f64), o: (f64, f64)) -> (f64, f64) {
        ((p.0 - o.0) / S, (o.1 - p.1) / S)
    }

    fn pose(&mut self, now: f64, dt: f64) -> Scene {
        let o = (self.x, self.y);
        let fl = if self.flip < 0.0 { -1.0 } else { 1.0 };
        let rig = &self.rig;
        let local = |c: &Clip| rig.local(c.name, (now - c.t0) * Self::rate(c.name));
        let Some(mut l) = local(&self.cur) else { return Scene::default() };
        let bw = ease((now - self.blend_t0) / self.blend_ms);
        if bw < 1.0 {
            if let Some(p) = self.prev.as_ref().and_then(local) {
                l = rig::blend(&p, &l, bw);
            }
        }
        let sk = if self.mode == Mode::Scare { ((now - self.scare_t0) / 110.0).min(1.0) * (1.0 - ease((now - self.scare_t0 - 900.0) / 350.0)) } else { 0.0 };
        self.scare += (sk - self.scare) * (dt / 35.0).min(1.0);
        let scare = self.scare;
        let tr = ((now - self.scare_t0) / 26.0).sin() * scare;
        let ptr = self.to_world((self.ptr.x, self.ptr.y), o);
        let p = (ptr.0 * fl, ptr.1);
        let w0 = rig::world(&l, &|_, s| s);
        let head = w0.bone(self.ids.head);
        // where it looks: the icon it works on, a glance, the pointer when it cares, or ahead
        let focus = match self.mode {
            Mode::Tidy => self.tidy.icon,
            _ => self.sw.at,
        }
        .and_then(|k| self.icons.get(k))
        .map(|i| (i.x, i.y));
        let mut tgt = None;
        if let Some(f) = focus {
            let g = self.to_world(f, o);
            tgt = Some((g.0 * fl, g.1));
        } else if let Some((gx, gy, _)) = self.mind.glance {
            let g = self.to_world((gx, gy), o);
            tgt = Some((g.0 * fl, g.1));
        } else if self.ptr.on && ((self.mind.interest > 0.22 && !matches!(self.mode, Mode::Lie | Mode::Pet)) || self.sw.phase.is_some() || self.mode == Mode::Hello) {
            tgt = Some(p);
        }
        let (mut la, mut lx, mut ly) = (0.0, 0.0, 0.0);
        if let (Some(t), Some(head)) = (tgt, head) {
            let (vx, vy) = (t.0 - head.x, t.1 - head.y);
            let d = vx.hypot(vy).max(1.0);
            let elev = vy.atan2(vx.abs() + 300.0);
            la = (-elev / D2R * 0.85).clamp(-26.0, 26.0);
            if self.mind.interest < 0.45 && self.mind.glance.is_none() && self.sw.phase.is_none() && focus.is_none() && self.mode != Mode::Hello {
                la *= 0.3;
            }
            lx = vx / d * 10.0;
            ly = vy / d * 7.0;
        }
        let pushing = self.mode == Mode::Tidy && self.tidy.step == Some(Step::Push);
        let eating = self.mode == Mode::Eat;
        let nt = (now - self.nudge_t0) / 260.0;
        let flick = if (0.0..1.0).contains(&nt) { (nt * std::f64::consts::PI).sin() } else { 0.0 };
        if pushing {
            la = 14.0;
        }
        if eating {
            la = 22.0;
        }
        let (ke, kh) = ((dt / 55.0).min(1.0), (dt / 160.0).min(1.0));
        self.look.1 += (lx - self.look.1) * ke;
        self.look.2 += (ly - self.look.2) * ke;
        self.look.0 += (la - self.look.0) * kh;
        let st = now - self.sw.t0;
        let crouch = match self.sw.phase {
            Some(Phase::Crouch) => ease(st / self.sw.dur(Phase::Crouch)),
            Some(Phase::Strike) => 1.0 - ease(st / self.sw.dur(Phase::Strike)),
            _ => 0.0,
        };
        let breath = (now / 950.0).sin() * 1.4;
        let moving = matches!(self.cur.name, "walk" | "run");
        let tail_base = if moving { (now / 400.0).sin() * 4.0 } else { ((now / 720.0).sin() * 3.0 + (now / 310.0).sin() * 1.5) * (1.0 + self.mind.interest * 1.8) };
        let swish_t = (now - self.mind.swish) / 900.0;
        let swish = if (0.0..1.0).contains(&swish_t) { (swish_t * std::f64::consts::PI * 3.0).sin() * 14.0 * (1.0 - swish_t) } else { 0.0 };
        let tail = tail_base + swish + if crouch > 0.0 { (now / 60.0).sin() * 6.0 * crouch } else { 0.0 } + if pushing { 10.0 } else { 0.0 };
        let chew = if eating { ((now - self.mode_t0) / 160.0).sin().abs() } else { 0.0 };
        let look_a = self.look.0;
        let ids = (self.ids.head, self.ids.neck, self.ids.body, self.ids.tail);
        let fix0 = move |n: usize, s: St| -> St {
            if n == ids.0 {
                St { a: s.a + look_a + 4.0 * scare + 1.5 * tr - 5.0 * crouch + 16.0 * flick + 4.0 * chew, ..s }
            } else if n == ids.1 {
                St { a: s.a - 6.0 * scare + 3.0 * crouch + if pushing { 6.0 } else { 0.0 } + if eating { 10.0 } else { 0.0 } + 6.0 * flick, y: s.y + breath * 0.5, ..s }
            } else if n == ids.2 {
                St { a: s.a - 5.0 * scare - 2.0 * crouch, x: s.x + 24.0 * scare + 2.0 * tr + 7.0 * crouch, y: s.y + 10.0 * scare + breath - 4.0 * crouch, ..s }
            } else if n == ids.3 {
                St { a: s.a + tail, ..s }
            } else {
                s
            }
        };
        let mut wd = rig::world(&l, &fix0);
        let (Some(s0), Some(e), Some(wr)) = (wd.bone(self.ids.up), wd.bone(self.ids.low), wd.bone(self.ids.paw)) else { return Scene::default() };
        let (l1, l2) = ((e.x - s0.x).hypot(e.y - s0.y), (wr.x - e.x).hypot(wr.y - e.y));
        self.shoulder = Some((o.0 + s0.x * S * self.flip, o.1 - s0.y * S));
        self.reach = (l1 + l2) * S;
        if !moving && self.sw.phase.is_none() {
            self.rel_sh = (s0.x.abs() * S, -s0.y * S);
        }
        // the paw: wound up by the chest, then along an arc through the target, then back
        let target = match self.sw.at.and_then(|k| self.icons.get(k)) {
            Some(i) => Some((i.x, i.y + 4.0)),
            None if self.ptr.on => Some((self.ptr.x, self.ptr.y)),
            None => None,
        };
        let (mut tt, mut w, mut claws, mut paw_tilt) = (None, 0.0, 0.0, 0.0);
        if let (Some(ph), Some(target)) = (self.sw.phase, target) {
            let aim = self.to_world(target, o);
            let aim = (aim.0 * fl, aim.1);
            let (ux, uy) = match self.sw.dir {
                Some((dx, dy)) => (dx * fl, -dy),
                None => {
                    let (dx0, dy0) = (aim.0 - s0.x, aim.1 - s0.y);
                    let d0 = dx0.hypot(dy0).max(1.0);
                    (dx0 / d0, dy0 / d0)
                }
            };
            let directed = self.sw.dir.is_some();
            let wind = if directed { (aim.0 - ux * 70.0, aim.1 - uy * 70.0 + 60.0) } else { (s0.x + ux * (l1 + l2) * 0.35 + 30.0, s0.y + (l1 + l2) * 0.55) };
            let reach_past = if self.sw.soft { 20.0 } else { 45.0 };
            let past = (aim.0 + ux * reach_past, aim.1 + uy * reach_past - if directed { 0.0 } else { 30.0 });
            let soft = self.sw.soft;
            match ph {
                Phase::Crouch => {
                    tt = Some(wind);
                    w = ease(st / self.sw.dur(Phase::Crouch)) * 0.9;
                    paw_tilt = 30.0 * w;
                }
                Phase::Strike => {
                    let k = back(st / self.sw.dur(Phase::Strike));
                    let lift = if directed { 30.0 } else { 60.0 };
                    let mid = ((wind.0 + past.0) / 2.0 - uy * lift, (wind.1 + past.1) / 2.0 + ux.abs() * lift + if directed { 0.0 } else { 40.0 });
                    let q = k.min(1.0);
                    let a1 = (wind.0 + (mid.0 - wind.0) * q, wind.1 + (mid.1 - wind.1) * q);
                    let a2 = (mid.0 + (past.0 - mid.0) * q, mid.1 + (past.1 - mid.1) * q);
                    tt = Some((a1.0 + (a2.0 - a1.0) * k, a1.1 + (a2.1 - a1.1) * k));
                    w = 1.0;
                    claws = if soft { 0.0 } else { 1.0 };
                    paw_tilt = 30.0 - 70.0 * k.min(1.0);
                }
                Phase::Hold => {
                    tt = Some(past);
                    w = 1.0;
                    claws = if soft { 0.0 } else { 1.0 };
                    paw_tilt = -40.0;
                }
                Phase::Back => {
                    let b = self.sw.dur(Phase::Back);
                    tt = Some(past);
                    w = 1.0 - ease_out(st / b);
                    claws = if soft { 0.0 } else { 1.0 - ease(st / (b * 0.5)) };
                    paw_tilt = -40.0 * w;
                }
            }
        }
        self.sw.w += (if tt.is_some() { w } else { 0.0 } - self.sw.w) * (dt / 16.0).min(1.0);
        self.sw.claws = claws;
        if let Some(t) = tt.filter(|_| self.sw.w > 0.01) {
            wd = self.reach_for(&l, &fix0, t, (s0, e, wr), (l1, l2), paw_tilt);
        }
        // the paw meets an icon during the strike
        if let Some(pw) = wd.bone(self.ids.paw) {
            let paw = (o.0 + pw.x * S * self.flip, o.1 - pw.y * S);
            if self.sw.phase == Some(Phase::Strike) && !self.sw.hit {
                if let Some(last) = self.last_paw {
                    let swing = (paw.0 - last.0, paw.1 - last.1);
                    let r = self.cell.0.min(self.cell.1) * 0.85;
                    let hit = match self.sw.at {
                        Some(k) if self.icons.get(k).is_some_and(|i| dist((i.x, i.y), paw) < r * 1.25) => Some(k),
                        _ => (0..self.icons.len()).find(|&k| !self.icons[k].nose && dist((self.icons[k].x, self.icons[k].y), paw) < r),
                    };
                    if let Some(k) = hit {
                        self.hit_icon(k, swing);
                    }
                }
            }
            self.last_paw = Some(paw);
        }
        // ears: a flick now and then, up and forward when it hunts
        let et = (now - self.mind.ear) / 220.0;
        let ear_flick = if (0.0..1.0).contains(&et) { (et * std::f64::consts::PI).sin() * 18.0 } else { 0.0 };
        for ob in &mut wd.objs {
            if Some(ob.name) == self.ids.ear_top {
                ob.st.a += ear_flick * self.mind.ear_side - 6.0 * crouch;
            }
            if Some(ob.name) == self.ids.ear_back {
                ob.st.a += ear_flick * 0.5 * self.mind.ear_side;
            }
            if eating && Some(ob.name) == self.ids.mouth {
                ob.st.al = 1.0;
                ob.st.sy *= 0.3 + 0.5 * chew;
                ob.st.sx *= 0.6;
            }
        }
        // a yawn: eyes shut, mouth wide
        let yt = (now - self.mind.yawn) / 1400.0;
        if (0.0..1.0).contains(&yt) && self.mode == Mode::Sit {
            let k = (yt * std::f64::consts::PI).sin();
            for ob in &mut wd.objs {
                if Some(ob.name) == self.ids.mouth {
                    ob.st.al = 1.0;
                    ob.st.sy *= 0.3 + 0.9 * k;
                    ob.st.sx *= 0.5 + 0.3 * k;
                }
            }
            if k > 0.35 {
                self.set_eyes(&mut wd, self.closed);
            }
        }
        if matches!(self.mode, Mode::Lie | Mode::Pet) {
            self.set_eyes(&mut wd, self.closed);
        }
        // the pupils open wide when it hunts
        let hunting = matches!(self.mode, Mode::Sit | Mode::Zoom | Mode::Tidy);
        let dil = 1.0 + 0.28 * (self.mind.interest * if hunting { 1.0 } else { 0.0 }).max(crouch).max(if focus.is_some() { 0.6 } else { 0.0 });
        if dil > 1.01 {
            for k in 0..wd.objs.len() {
                if self.ids.pupils.contains(&wd.objs[k].name) {
                    let s = wd.objs[k].st;
                    wd.objs[k].st = self.grow(s, dil, None);
                }
            }
        }
        if scare > 0.01 {
            self.scared(&mut wd, scare, ((now - self.scare_t0) / 90.0).sin());
        }
        self.draw(&wd, o)
    }

    /// Two-bone reach of the front leg towards `t` (pack units, flipped).
    #[allow(clippy::too_many_arguments)]
    fn reach_for(&self, l: &Local, fix0: &dyn Fn(usize, St) -> St, t: (f64, f64), (s0, e, wr): (St, St, St), (l1, l2): (f64, f64), paw_tilt: f64) -> World {
        let (dx, dy) = (t.0 - s0.x, t.1 - s0.y);
        let dd = dx.hypot(dy).max(1.0);
        let d = ((l1 - l2).abs() * 1.05).max(((l1 + l2) * 0.98).min(dd));
        let tt = (s0.x + dx / dd * d, s0.y + dy / dd * d);
        let base = (tt.1 - s0.y).atan2(tt.0 - s0.x);
        let a = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0).acos();
        let side = ((wr.x - s0.x) * (e.y - s0.y) - (wr.y - s0.y) * (e.x - s0.x)).signum();
        let side = if side == 0.0 { 1.0 } else { side };
        let pick = |s: f64| {
            let ep = (s0.x + l1 * (base + s * a).cos(), s0.y + l1 * (base + s * a).sin());
            let c = ((tt.0 - s0.x) * (ep.1 - s0.y) - (tt.1 - s0.y) * (ep.0 - s0.x)).signum();
            (ep, c)
        };
        let (c1, cs) = pick(1.0);
        let ep = if cs == side { c1 } else { pick(-1.0).0 };
        let w = self.sw.w;
        let du = wrap((ep.1 - s0.y).atan2(ep.0 - s0.x) - (e.y - s0.y).atan2(e.x - s0.x)) / D2R * w;
        let (up, low, paw) = (self.ids.up, self.ids.low, self.ids.paw);
        let fix_u = |n: usize, s: St| if n == up { St { a: s.a + du, ..s } } else { fix0(n, s) };
        let w2 = rig::world(l, &fix_u);
        let (Some(e2), Some(r2)) = (w2.bone(low), w2.bone(paw)) else { return w2 };
        let dl = wrap((tt.1 - e2.y).atan2(tt.0 - e2.x) - (r2.y - e2.y).atan2(r2.x - e2.x)) / D2R * w;
        let fix = |n: usize, s: St| {
            if n == low {
                St { a: s.a + dl, ..s }
            } else if n == paw {
                St { a: s.a + paw_tilt * w, ..s }
            } else {
                fix_u(n, s)
            }
        };
        rig::world(l, &fix)
    }

    fn file_name(&self, f: Option<[u16; 2]>) -> &str {
        f.and_then(|f| self.rig.files.get(&f)).map_or("", |f| f.name.as_str())
    }

    fn set_eyes(&self, wd: &mut World, to: Option<[u16; 2]>) {
        let Some(to) = to else { return };
        for k in 0..wd.objs.len() {
            if self.file_name(wd.objs[k].st.f).contains("eyes") {
                wd.objs[k].st.f = Some(to);
                return;
            }
        }
    }

    /// The middle of a part's picture, in its parent's space.
    fn centre(&self, o: &St) -> Option<(f64, f64)> {
        let f = self.rig.files.get(&o.f?)?;
        let (px, py) = (o.px.unwrap_or(f.px), o.py.unwrap_or(f.py));
        let (sn, cs) = (o.a * D2R).sin_cos();
        let (lx, ly) = ((f.w / 2.0 - px * f.w) * o.sx, ((1.0 - py) * f.h - f.h / 2.0) * o.sy);
        Some((o.x + lx * cs - ly * sn, o.y + lx * sn + ly * cs))
    }

    /// Scaled by `k` around its middle (or moved so its middle lands on `to`).
    fn grow(&self, o: St, k: f64, to: Option<(f64, f64)>) -> St {
        let Some(c) = self.centre(&o) else { return o };
        let mut g = St { sx: o.sx * k, sy: o.sy * k, ..o };
        let Some(c2) = self.centre(&g) else { return o };
        let t = to.unwrap_or(c);
        g.x += t.0 - c2.0;
        g.y += t.1 - c2.1;
        g
    }

    /// The scare on the parts: ears flat, wide eyes, mouth open in a hiss, the tail puffed.
    fn scared(&self, wd: &mut World, k: f64, wob: f64) {
        let mut eyes = None;
        for i in 0..wd.objs.len() {
            let n = wd.objs[i].name;
            let s = &mut wd.objs[i].st;
            if Some(n) == self.ids.ear_top {
                s.a += -35.0 * k;
                s.sy *= 1.0 - 0.25 * k;
            }
            if Some(n) == self.ids.ear_back {
                s.sy *= 1.0 - 0.35 * k;
            }
            if Some(n) == self.ids.mouth && k > 0.05 {
                s.al = 1.0;
                s.sy *= 0.35 + 0.3 * k;
                s.sx *= 0.55 + 0.2 * k;
            }
            if Some(n) == self.ids.tail_tip {
                if k >= 0.5 && self.rig.files.contains_key(&self.puffed) {
                    s.f = Some(self.puffed);
                }
                s.a += 18.0 * k + wob * 3.0;
                s.sx *= 1.0 + 0.25 * k;
                s.sy *= 1.0 + 0.08 * k;
            }
            if eyes.is_none() && self.file_name(s.f).contains("eyes") {
                eyes = Some(i);
            }
        }
        let Some(e) = eyes else { return };
        if k > 0.3 {
            if let Some(open) = self.open {
                wd.objs[e].st.f = Some(open);
            }
        }
        let ek = 1.0 + 0.12 * k;
        let es = self.grow(wd.objs[e].st, ek, None);
        wd.objs[e].st = es;
        let Some(ec) = self.centre(&es) else { return };
        for i in 0..wd.objs.len() {
            if !self.ids.pupils.contains(&wd.objs[i].name) {
                continue;
            }
            let s = wd.objs[i].st;
            let Some(pc) = self.centre(&s) else { continue };
            wd.objs[i].st = self.grow(s, 1.0 + 0.18 * k, Some((ec.0 + (pc.0 - ec.0) * ek, ec.1 + (pc.1 - ec.1) * ek)));
        }
    }

    /// The parts as pictures on the screen.
    fn draw(&mut self, wd: &World, org: (f64, f64)) -> Scene {
        let fl = self.flip;
        let face_of = |name: &str| ["eyes", "pupils/", "/nose.png", "/mouth.png", "/tongue.png", "whisker"].iter().any(|k| name.contains(k));
        let apply = |m: &[f64; 6], u: f64, v: f64| (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]);
        let mut list: Vec<(Part, bool)> = Vec::with_capacity(wd.objs.len());
        let (mut eyes, mut nose) = (None, None);
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for ob in &wd.objs {
            let Some(key) = ob.st.f else { continue };
            let Some(f0) = self.rig.files.get(&key) else { continue };
            let key = if f0.name.starts_with("pupils/") { self.rig.black } else { key };
            let Some(f) = self.rig.files.get(&key) else { continue };
            let Some(&(iw, ih)) = self.sizes.get(&key) else { continue };
            let mut o = ob.st;
            if self.ids.pupils.contains(&ob.name) {
                o.x += self.look.1;
                o.y += self.look.2;
            }
            let sc = iw / f.w;
            let (px, py) = (o.px.unwrap_or(f.px), o.py.unwrap_or(f.py));
            let (sn, cs) = (o.a * D2R).sin_cos();
            let (a_, b_) = (o.sx / sc, -o.sy / sc);
            let (lx0, ly0) = (-px * f.w * o.sx, (1.0 - py) * f.h * o.sy);
            let mut m = [S * cs * a_, -S * sn * a_, -S * sn * b_, -S * cs * b_, org.0 + S * (cs * lx0 - sn * ly0 + o.x), org.1 - S * (sn * lx0 + cs * ly0 + o.y)];
            if fl != 1.0 {
                m = [m[0] * fl, m[1], m[2] * fl, m[3], org.0 + fl * (m[4] - org.0), m[5]];
            }
            let c = apply(&m, iw / 2.0, ih / 2.0);
            if f.name.contains("eyes") {
                eyes = Some(c);
            }
            if f.name.ends_with("/nose.png") {
                nose = Some(c);
            }
            for (u, v) in [(0.0, 0.0), (iw, ih), (iw, 0.0), (0.0, ih)] {
                let q = apply(&m, u, v);
                x0 = x0.min(q.0);
                x1 = x1.max(q.0);
                y0 = y0.min(q.1);
                y1 = y1.max(q.1);
            }
            let claws = if Some(ob.name) == self.ids.paw_part { self.sw.claws } else { 0.0 };
            list.push((Part { file: key, m, alpha: o.al, claws }, face_of(&f.name)));
        }
        if let Some(n) = nose {
            self.nose = n;
            let ahead = -fl.signum();
            if fl.abs() > 0.95 && self.mode != Mode::Tidy {
                self.rel_nose = ((n.0 - org.0) * ahead, n.1 - org.1);
            }
        }
        self.bounds = (x1 > x0).then_some((x0 + 10.0, y0 + 6.0, x1 - 10.0, y1));
        let c = eyes.zip(nose).map(|(e, n)| ((e.0 + n.0) / 2.0, (e.1 + n.1) / 2.0));
        let parts = list
            .into_iter()
            .map(|(mut p, face)| {
                if let (true, Some(c)) = (face, c) {
                    let m = p.m;
                    p.m = [m[0] * FS, m[1] * FS, m[2] * FS, m[3] * FS, m[4] * FS + c.0 * (1.0 - FS), m[5] * FS + c.1 * (1.0 - FS)];
                }
                p
            })
            .collect();
        Scene { parts, icons: Vec::new(), bowl: None, feet: Some(org) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> Live {
        let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/cat/rig/rig.json")).expect("rig.json");
        let rig = Rig::parse(&json).expect("rig");
        // the pictures are 0.3 px per pack unit
        let sizes = rig.files.iter().map(|(k, f)| (*k, ((f.w * 0.3).round().max(1.0), (f.h * 0.3).round().max(1.0)))).collect();
        let mut l = Live::new(rig, sizes, 7).expect("live");
        l.set_area(1512.0, 982.0, (0.0, 25.0, 1512.0, 900.0));
        l
    }

    /// Runs it for `ms` at 60 fps from `t`, returns the end time.
    fn run(l: &mut Live, t: f64, ms: f64) -> f64 {
        let mut t = t;
        let end = t + ms;
        while t < end {
            t += 16.7;
            let _ = l.tick(t);
        }
        t
    }

    #[test]
    fn it_walks_in_to_its_corner_and_sits() {
        let mut l = live();
        l.set_lives(true, 0.0);
        assert!(l.shown());
        let t = run(&mut l, 0.0, 6000.0);
        assert_eq!(l.mode, Mode::Sit, "{:?}", l.mode);
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        let s = l.tick(t + 16.0);
        assert!(s.parts.len() > 20, "{} parts", s.parts.len());
        let (x0, y0, x1, y1) = l.bounds.unwrap();
        assert!(x1 - x0 > 60.0 && y1 - y0 > 90.0, "{:?}", l.bounds);
    }

    #[test]
    fn a_slap_scares_it_out_of_anything() {
        let mut l = live();
        l.set_lives(true, 0.0);
        let mut t = run(&mut l, 0.0, 6000.0);
        l.pet(true, t);
        assert_eq!(l.mode, Mode::Pet);
        t = run(&mut l, t, 1000.0);
        l.slap(t);
        assert_eq!(l.mode, Mode::Scare);
        t = run(&mut l, t, 300.0);
        assert!(l.scare > 0.5, "scare {}", l.scare);
        run(&mut l, t, 2500.0);
        assert!(matches!(l.mode, Mode::Sit | Mode::SitDown), "{:?}", l.mode);
    }

    #[test]
    fn three_clicks_on_it_and_it_races_about_then_comes_back() {
        let mut l = live();
        l.set_lives(true, 0.0);
        let mut t = run(&mut l, 0.0, 6000.0);
        let (x0, y0, x1, y1) = l.bounds.unwrap();
        let at = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        for _ in 0..3 {
            l.pointer(Some(at), true, t);
            t = run(&mut l, t, 50.0);
            l.pointer(Some(at), false, t);
            t = run(&mut l, t, 100.0);
        }
        assert_eq!(l.mode, Mode::Zoom);
        l.pointer(None, false, t);
        t = run(&mut l, t, 12000.0);
        assert!(matches!(l.mode, Mode::Sit | Mode::SitDown), "{:?}", l.mode);
        let _ = t;
    }

    #[test]
    fn the_tidy_lines_every_icon_up_in_its_slot() {
        let mut l = live();
        l.set_lives(true, 0.0);
        let mut t = run(&mut l, 0.0, 6000.0);
        let icons = vec![(1, 300.0, 200.0), (2, 600.0, 520.0), (3, 900.0, 140.0), (4, 420.0, 640.0), (5, 1380.0, 60.0), (6, 700.0, 300.0)];
        let slots: Vec<(f64, f64)> = (0..6).map(|r| (1440.0, 60.0 + r as f64 * 92.0)).collect();
        l.tidy(icons, &slots, (47.0, 37.0), t);
        let mut rests = HashMap::new();
        let mut lifted = std::collections::HashSet::new();
        let mut tidied = false;
        for _ in 0..(180 * 60) {
            t += 16.7;
            let _ = l.tick(t);
            for o in l.take() {
                match o {
                    Out::Lift(id) => {
                        lifted.insert(id);
                    }
                    Out::Rest { id, x, y } => {
                        rests.insert(id, (x, y));
                    }
                    Out::Tidied => tidied = true,
                    _ => {}
                }
            }
            if tidied && !l.tidying() {
                break;
            }
        }
        assert!(tidied, "never finished; step {:?}", l.tidy.step);
        // every icon ended in a slot, one icon a slot, and Finder was told where
        for i in &l.icons {
            let s = i.slot.expect("slot");
            assert!(dist((i.x, i.y), s) < 1.0, "icon {} at {:.0},{:.0}, slot {:?}", i.id, i.x, i.y, s);
            if lifted.contains(&i.id) {
                let r = rests[&i.id];
                assert!(dist(r, s) < 1.0, "icon {} rests at {r:?}, slot {s:?}", i.id);
            }
        }
        let mut seen: Vec<(i64, i64)> = l.icons.iter().map(|i| (i.x.round() as i64, i.y.round() as i64)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn a_slap_mid_tidy_scares_it_and_then_it_carries_on() {
        let mut l = live();
        l.set_lives(true, 0.0);
        let mut t = run(&mut l, 0.0, 6000.0);
        l.tidy(vec![(1, 300.0, 200.0), (2, 600.0, 520.0)], &[(1440.0, 60.0), (1440.0, 152.0)], (47.0, 37.0), t);
        t = run(&mut l, t, 2000.0);
        l.slap(t);
        assert_eq!(l.mode, Mode::Scare);
        run(&mut l, t, 1500.0);
        assert_eq!(l.mode, Mode::Tidy);
    }

    #[test]
    fn a_still_cat_asks_for_few_frames_and_none_when_away() {
        let mut l = live();
        assert_eq!(l.fps(), 0.0);
        l.set_lives(true, 0.0);
        assert_eq!(l.fps(), 60.0);
        run(&mut l, 0.0, 6000.0);
        assert!(l.fps() <= 30.0);
    }
}
