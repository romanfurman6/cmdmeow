//! The desktop cat, alive: what it wants and does, frame by frame, on the
//! skeleton of `rig`. It sits on its bed, looks about, washes, walks about
//! the desk, and sleeps until something wakes it. A held pointer makes it
//! purr. A slap sends it off through the edge by its bed; it peeks back
//! and comes home. Three clicks bring a yarn ball people drag and throw
//! and it chases, pins and bats (three clicks on the ball put it away).
//! On the charger it eats from its bowl, and on a low battery it begs by
//! it (the food in the bowl is the charge). It falls asleep when the lid
//! closes and wakes a moment after it opens. People can move its bed and
//! bowl along the bottom of the screen. It can also tidy the desktop: its
//! paw rakes a heap of the real icons together, it takes the heap in its
//! mouth, carries it to Finder's grid and drops each icon into its slot.
//!
//! Pure: points down from the top left of the screen, milliseconds. The
//! Mac side feeds it the pointer and what the Mac felt, and draws the
//! `Scene` it returns (one layer per part, and its things).

use super::rig::{self, Local, Rig, St, World, D2R};
use super::toys::{self, Ball, BallView, BowlView, BOWL_H};
use super::Kind;
use std::collections::HashMap;
use std::f64::consts::PI;

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
/// A turn: a small hop, and the body comes round at its top.
const TURN_MS: f64 = 300.0;
/// A step back covers this many points a second: the walk played backwards.
const BACK: f64 = 80.0;
/// It begs below this charge (0–1) and stops once it is back up here.
const LOW: f64 = 0.10;
const FED: f64 = 0.15;
/// Each eating sound lasts this long; a meal is two or three of them.
const CRUNCH_MS: f64 = 2640.0;
/// It sleeps on this long after the lid opens.
const WAKE_AFTER: f64 = 3000.0;
/// Its head in the bowl: head and neck down, shoulders lower, a little
/// chew; it stands this much further back than its nose so the head is
/// over the middle of the bowl.
const EAT_LOOK: f64 = 27.0;
const EAT_NECK: f64 = 30.0;
const EAT_BODY: f64 = 6.0;
const EAT_DROP: f64 = -4.0;
const EAT_BACK: f64 = 34.0;
const EAT_CHEW: f64 = 1.5;
/// What people let go of above the bottom falls this fast (points/s²).
const DROP: f64 = 1900.0;


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
    /// on the heap the paw raked together, or in the mouth
    hold: Hold,
    /// when it hops onto the heap or into the mouth, for how long, and from where
    hop: f64,
    hop_ms: f64,
    from: Option<(f64, f64, f64)>,
    tapped: bool,
    /// its place on the heap
    j: usize,
    /// on its way to its slot
    fly: Option<Fly>,
    /// how much of its label shows (none on the heap or in the mouth)
    pub label: f64,
    slot: Option<(f64, f64)>,
    /// in its slot
    home: bool,
    /// moved since the Mac last placed it
    lifted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    No,
    Heap,
    Mouth,
}

/// A flight from the mouth to the slot: from, start, length.
#[derive(Debug, Clone, Copy)]
struct Fly {
    x0: f64,
    y0: f64,
    r0: f64,
    t0: f64,
    ms: f64,
}

impl Icon {
    fn new(id: usize, x: f64, y: f64) -> Icon {
        Icon { id, x, y, vx: 0.0, vy: 0.0, rot: 0.0, vr: 0.0, hold: Hold::No, hop: 0.0, hop_ms: 0.0, from: None, tapped: false, j: 0, fly: None, label: 1.0, slot: None, home: false, lifted: false }
    }

    fn held(&self) -> bool {
        self.hold != Hold::No
    }
}

/// Where each icon of the heap lies, ahead of the paws: x, y, turn (degrees).
const HEAP: [(f64, f64, f64); 8] = [(0.0, 0.0, -4.0), (12.0, -7.0, 7.0), (3.0, -14.0, -9.0), (19.0, 3.0, 5.0), (8.0, 7.0, -6.0), (22.0, -11.0, 10.0), (-3.0, -22.0, 4.0), (15.0, -21.0, -8.0)];
/// In the mouth each icon hangs from its top edge, fanned out (degrees).
const FAN: [f64; 8] = [-4.0, 14.0, -16.0, 26.0, -27.0, 6.0, 36.0, -9.0];

/// What the Mac side should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// This icon starts to move: show its picture in place of Finder's.
    Lift(usize),
    /// It came to rest here: Finder places it, then its picture goes.
    Rest { id: usize, x: f64, y: f64 },
    /// The tidy is over.
    Tidied,
    /// Something met something: a paw, the mouth, an icon, the ball, the
    /// bed or the bowl (soft: quieter).
    Tap { hit: Hit, soft: bool },
    /// A held pointer on the cat: the purr starts or stops.
    Purr(bool),
    /// It comes to tidy.
    Mew,
    /// A meow: the ball comes.
    Chirp,
    /// One of the kind's sounds, now: it eats, begs, or says hello once up.
    Voice(Kind),
    /// The bed or the bowl came to rest somewhere new: where they stand
    /// (from the left).
    Moved { bed: f64, bowl: f64 },
}

/// What made a sound on the desk; each has its own (none of the Mac's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A paw on an icon, or two icons knocking.
    Pat,
    /// One swipe rakes a heap together.
    Rake,
    /// The heap or one icon goes into the mouth.
    Grab,
    /// An icon lands in its slot.
    Land,
    /// The ball, kicked or bouncing.
    Ball,
    /// The bed drops to the floor.
    Bed,
    /// The bowl drops to the floor.
    Bowl,
}

impl Hit {
    /// Its file stem in the cat's assets (`pat.mp3` …).
    pub fn file(self) -> &'static str {
        match self {
            Hit::Pat => "pat",
            Hit::Rake => "rake",
            Hit::Grab => "grab",
            Hit::Land => "land",
            Hit::Ball => "ball",
            Hit::Bed => "bed",
            Hit::Bowl => "bowl",
        }
    }
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

/// An icon on the move: its middle, its turn in degrees, how much of its
/// label shows, and whether it is in front of the cat (in its mouth).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moving {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub label: f64,
    pub front: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub parts: Vec<Part>,
    /// The icons being moved.
    pub icons: Vec<Moving>,
    /// The feet's shadow (middle).
    pub feet: Option<(f64, f64)>,
    /// The bed's middle bottom.
    pub bed: Option<(f64, f64)>,
    pub bowl: Option<BowlView>,
    pub ball: Option<BallView>,
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
    /// getting up from lying (then `wake_next`)
    WakeUp,
    Scare,
    /// off through the edge by its bed, out of sight a while, a peek back
    Flee,
    Gone,
    Peek,
    Pet,
    Tidy,
    WalkHome,
    /// after the yarn ball
    Ball,
    /// a walk to somewhere else on the desk
    Roam,
    ToBowl,
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
    /// the next heap: the nearest icons, 5 to 8
    Pick,
    /// to the near side of them, facing the grid
    Goto,
    /// one sweep of the paw rakes them onto a heap
    Rake,
    /// head down, the heap into the mouth
    Grab,
    /// the ones the paw missed, picked up one by one
    Collect,
    /// to the grid, the heap in the mouth
    Carry,
    /// head down: each icon flies to its slot
    Deal,
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
    /// a sweep that rakes the heap's icons together
    rake: bool,
}

impl Swipe {
    fn dur(&self, p: Phase) -> f64 {
        self.dur[p as usize]
    }
}

/// What it does to the ball now: a pounce (the jump), a paw slammed down
/// (the scratch), or the ball pinned under its front paws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Jump,
    Slam,
    Hold,
}

#[derive(Default)]
struct Hunt {
    act: Option<Act>,
    t0: f64,
    from: (f64, f64),
    to: (f64, f64),
    hit: bool,
    rest: f64,
    last_jump: f64,
}

/// Where the paw comes down in the scratch (at 0.47 of it) and where the
/// front paws land in the jump (from 0.8), ahead of and above the feet.
const SLAM: (f64, f64) = (106.0, 10.0);
const LAND: (f64, f64) = (28.0, 6.0);

/// The edge it runs off through when scared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Left,
    Right,
    Bottom,
}

/// What a hand holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Ball,
    Bed,
    Bowl,
}

/// What it wants from its bowl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    Eat,
    Beg,
}

/// Begging by the bowl: up on its hind legs with a meow, down, a sit and a
/// stare, up again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BegStep {
    Stand,
    Up,
    Down,
    Sitting,
    Rise,
}

struct Feed {
    step: BegStep,
    t: f64,
    wait: f64,
    until: f64,
    /// the next eating sound
    voice: f64,
    /// the last meow
    meow: f64,
    /// how far its head is down in the bowl (0–1)
    k: f64,
}

/// The bed or the bowl: where people put it (None: by the corner) and how
/// fast it falls after they let go above the bottom.
#[derive(Default, Clone, Copy)]
struct Thing {
    at: Option<(f64, f64)>,
    vy: f64,
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
    next_roam: f64,
}

struct Tidy {
    step: Option<Step>,
    t0: f64,
    face: f64,
    resume: bool,
    /// this heap's icons, and those of them it holds, in the order they came
    batch: Vec<usize>,
    pile: Vec<usize>,
    /// the one it goes to pick up next, since when
    next: Option<usize>,
    since: f64,
    /// when the head last went down, and until when it waits
    dip: f64,
    wait: f64,
    /// where the raked heap lies
    spot: Option<(f64, f64)>,
}

struct Clip {
    name: &'static str,
    t0: f64,
    /// played backwards (the walk, for a step back)
    rev: bool,
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
    /// the turn under way: when, from and to which flip; and the hop of it (0–1)
    turn: (f64, f64, f64),
    hop: f64,
    mode: Mode,
    mode_t0: f64,
    /// what it does once it is up
    wake_next: Mode,
    /// a hello is due once it is up
    wake_hello: bool,
    mind: Mind,
    scare: f64,
    scare_t0: f64,
    petting: bool,
    hunt: Hunt,
    ball: Ball,
    ball_clicks: Vec<f64>,
    held: Option<Held>,
    /// where the hand holds what it holds, from its middle
    grab: (f64, f64),
    bed: Thing,
    bowl: Thing,
    /// the charge (0–1) and the charger; None on a Mac without a battery
    battery: Option<(f64, bool)>,
    want_eat: bool,
    hungry: bool,
    feed: Feed,
    edge: Edge,
    /// the Mac sleeps (the lid is closed): so does the cat
    doze: bool,
    /// when it gets up after the lid opened
    wake_at: f64,
    roam_to: (f64, f64),
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
    /// Finder's grid for this tidy, in order
    slots: Vec<(f64, f64)>,
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
    vx: f64,
    vy: f64,
    moved: f64,
    stim: f64,
    last: Option<(f64, f64, f64)>,
    /// where the button went down, and whether the hand moved since (a drag, not a click)
    press: (f64, f64),
    dragged: bool,
}

const ANIMS: [&str; 13] = ["idle", "walk", "run", "sitting", "sit_tranistion_to", "sit_transition_from", "clean", "sleeping", "sleep_transition_to", "sleep_transition_from", "beg", "scratch", "jump"];

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
            cur: Clip { name: "sitting", t0: 0.0, rev: false },
            prev: None,
            blend_t0: 0.0,
            blend_ms: 160.0,
            x: f64::NAN,
            y: f64::NAN,
            dir: -1.0,
            flip: 1.0,
            turn: (-1e9, 1.0, 1.0),
            hop: 0.0,
            mode: Mode::Away,
            mode_t0: 0.0,
            wake_next: Mode::Sit,
            wake_hello: false,
            mind: Mind { interest: 0.0, bored: 0.0, quiet: 0.0, next_idle: 2500.0, glance: None, ear: -1e9, ear_side: 1.0, swish: -1e9, yawn: -1e9, last_swipe: -1e9, next_roam: 20000.0 },
            scare: 0.0,
            scare_t0: -1e9,
            petting: false,
            hunt: Hunt { last_jump: -1e9, ..Hunt::default() },
            ball: Ball::default(),
            ball_clicks: Vec::new(),
            held: None,
            grab: (0.0, 0.0),
            bed: Thing::default(),
            bowl: Thing::default(),
            battery: None,
            want_eat: false,
            hungry: false,
            feed: Feed { step: BegStep::Stand, t: 0.0, wait: 0.0, until: 0.0, voice: 0.0, meow: -1e9, k: 0.0 },
            edge: Edge::Right,
            doze: false,
            wake_at: 0.0,
            roam_to: (0.0, 0.0),
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
            tidy: Tidy { step: None, t0: 0.0, face: -1.0, resume: false, batch: Vec::new(), pile: Vec::new(), next: None, since: 0.0, dip: -1e9, wait: 0.0, spot: None },
            slots: Vec::new(),
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
        if ground == self.ground && w == self.w && h == self.h {
            return;
        }
        let (was_home, me) = (self.home(), (self.x, self.y));
        self.w = w;
        self.h = h;
        self.ground = ground;
        // the bed and the bowl stand on the new bottom at once (no fall)
        for (t, held) in [(&mut self.bed, self.held == Some(Held::Bed)), (&mut self.bowl, self.held == Some(Held::Bowl))] {
            if let (Some((x, _)), false) = (t.at, held) {
                t.at = Some((x, ground.3));
                t.vy = 0.0;
            }
        }
        // the screen changed size: what was at home is at home again, and
        // nothing stays off the desk
        if !me.0.is_finite() || matches!(self.mode, Mode::Away | Mode::Enter | Mode::Leave | Mode::Flee | Mode::Gone | Mode::Peek) {
            return;
        }
        if dist(me, was_home) < 30.0 {
            (self.x, self.y) = self.home();
        } else {
            (self.x, self.y) = (self.clamp_x(me.0), self.clamp_y(me.1));
        }
        self.roam_to = (self.clamp_x(self.roam_to.0), self.clamp_y(self.roam_to.1));
    }

    /// Where people last left the bed and the bowl (from the left; they
    /// stand on the bottom).
    pub fn set_places(&mut self, bed: Option<f64>, bowl: Option<f64>) {
        let floor = self.ground.3;
        self.bed.at = bed.map(|x| (x, floor));
        self.bowl.at = bowl.map(|x| (x, floor));
    }

    /// The bed's middle bottom: on the bottom, where people put it, or by
    /// the right corner; up in a hand while held, and as it falls.
    fn bed_at(&self) -> (f64, f64) {
        let g = self.ground;
        let floor = g.3 - 18.0;
        match self.bed.at {
            Some((x, y)) => (x.clamp(g.0 + 104.0, (g.2 - 104.0).max(g.0 + 104.0)), y.clamp(g.1 + 60.0, floor)),
            None => (g.2 - 108.0, floor),
        }
    }

    /// Where it sits on its bed.
    fn home(&self) -> (f64, f64) {
        let b = self.bed_at();
        (b.0 + 22.0, b.1 - 8.0)
    }

    fn on_bed(&self, p: (f64, f64)) -> bool {
        let b = self.bed_at();
        (p.0 - b.0).abs() < 102.0 && p.1 > b.1 - 46.0 && p.1 < b.1 + 16.0
    }

    /// The bowl's middle bottom: on the bottom, where people put it, or
    /// beside the bed.
    fn bowl_at(&self) -> (f64, f64) {
        let g = self.ground;
        let floor = g.3 - 20.0;
        let (x, y) = self.bowl.at.unwrap_or((self.bed_at().0 - 190.0, floor));
        (x.clamp(g.0 + 50.0, (g.2 - 50.0).max(g.0 + 50.0)), y.clamp(g.1 + 60.0, floor))
    }

    fn on_bowl(&self, p: (f64, f64)) -> bool {
        let b = self.bowl_at();
        (p.0 - b.0).abs() < 46.0 && p.1 > b.1 - 44.0 && p.1 < b.1 + 8.0
    }

    /// Where it stands to eat, and which way it faces: its head over the
    /// middle of the bowl, from the side it is on.
    fn bowl_stand(&self) -> (f64, f64, f64) {
        let (b, g) = (self.bowl_at(), self.ground);
        let off = self.rel_nose.0 + EAT_BACK;
        let mut side = if self.x > b.0 { 1.0 } else { -1.0 };
        if b.0 + side * off > g.2 - 40.0 || b.0 + side * off < g.0 + 40.0 {
            side = -side;
        }
        (b.0 + side * off, b.1 - 6.0, -side)
    }

    /// Where the ball may lie.
    fn ball_area(&self) -> (f64, f64, f64, f64) {
        let g = self.ground;
        (g.0 + toys::BR + 6.0, g.1 + 110.0, g.2 - toys::BR - 6.0, g.3 - 8.0)
    }

    fn clamp_x(&self, x: f64) -> f64 {
        x.clamp(self.ground.0 + 50.0, (self.ground.2 - 50.0).max(self.ground.0 + 50.0))
    }

    fn clamp_y(&self, y: f64) -> f64 {
        y.clamp(self.ground.1 + 95.0, (self.ground.3 - 8.0).max(self.ground.1 + 95.0))
    }

    /// It lives on screen on its bed, or only comes for a reaction.
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
                        let s = (now - lt) / 1000.0;
                        let (vx, vy) = ((x - lx) / s, (y - ly) / s);
                        let v = vx.hypot(vy);
                        self.ptr.v = self.ptr.v * 0.6 + v * 0.4;
                        self.ptr.vx = self.ptr.vx * 0.5 + vx * 0.5;
                        self.ptr.vy = self.ptr.vy * 0.5 + vy * 0.5;
                        if v > 60.0 {
                            self.ptr.moved = now;
                        }
                        // any move it can see draws it in; a quick one near it most
                        let d = (x - self.x).hypot(y - (self.y - 60.0));
                        if d.is_finite() && v > 15.0 {
                            self.ptr.stim += (0.35 + 0.65 * (v / 700.0).min(1.0)) * (1.0 - d / 900.0).max(0.0) * 0.3 * ((now - lt) / 16.7).min(3.0);
                        }
                    }
                }
                self.ptr.last = Some((x, y, now));
                self.ptr.x = x;
                self.ptr.y = y;
                self.ptr.on = true;
                // what a hand holds goes with it (the ball once it is dragged, not on a click)
                let (gx, gy) = self.grab;
                if self.held.is_some() && !self.ptr.dragged && dist((x, y), self.ptr.press) > 5.0 {
                    self.ptr.dragged = true;
                }
                match self.held {
                    Some(Held::Ball) if self.ptr.dragged => {
                        self.ball.z = 40.0;
                        self.ball.x = x - gx;
                        self.ball.y = y - gy + toys::BR + 40.0;
                        (self.ball.vx, self.ball.vy, self.ball.vz) = (0.0, 0.0, 0.0);
                    }
                    Some(Held::Bed) => self.bed.at = Some((x - gx, y - gy)),
                    Some(Held::Bowl) => self.bowl.at = Some((x - gx, y - gy)),
                    _ => {}
                }
            }
            None => {
                self.ptr.on = false;
                self.ptr.last = None;
            }
        }
        if down && !self.ptr.down {
            self.press(now);
        }
        if !down && self.ptr.down {
            self.release(now);
        }
        self.ptr.down = down;
    }

    /// The button went down: on the ball, the bowl, the bed or the cat.
    fn press(&mut self, now: f64) {
        self.ptr.down_at = now;
        self.ptr.down_on_cat = self.over_cat();
        self.ptr.press = (self.ptr.x, self.ptr.y);
        self.ptr.dragged = false;
        if !self.ptr.on || self.mode == Mode::Away {
            return;
        }
        // someone clicks: the Mac is awake, whatever it said
        if self.doze && (self.ptr.down_on_cat || self.ball.under(self.ptr.press) || self.on_bed(self.ptr.press) || self.on_bowl(self.ptr.press)) {
            self.doze = false;
        }
        if self.doze {
            return;
        }
        let p = (self.ptr.x, self.ptr.y);
        if self.ball.under(p) {
            // three clicks on the ball put it away
            self.ball_clicks.push(now);
            self.ball_clicks.retain(|t| now - t <= 1300.0);
            if self.ball_clicks.len() >= 3 {
                self.ball_clicks.clear();
                return self.put_ball_away(now);
            }
            let c = self.ball.top();
            self.held = Some(Held::Ball);
            self.grab = (p.0 - c.0, p.1 - c.1);
            self.ball.held = true;
            self.ball.touched = now;
            return;
        }
        // the bowl's and the bed's rims below the cat's feet are theirs, not the cat's
        let b = self.bowl_at();
        if self.on_bowl(p) && (!self.ptr.down_on_cat || p.1 > b.1 - 18.0) {
            self.ptr.down_on_cat = false;
            self.held = Some(Held::Bowl);
            self.grab = (p.0 - b.0, p.1 - b.1);
            return;
        }
        let b = self.bed_at();
        if self.on_bed(p) && (!self.ptr.down_on_cat || p.1 > self.home().1 - 2.0) {
            self.ptr.down_on_cat = false;
            self.held = Some(Held::Bed);
            self.grab = (p.0 - b.0, p.1 - b.1);
            return;
        }
        if self.ptr.down_on_cat {
            self.clicks.push(now);
            self.clicks.retain(|t| now - t <= 1300.0);
        }
    }

    /// The button came up: what was held is let go (a ball thrown), and a
    /// click on a sleeping cat wakes it.
    fn release(&mut self, now: f64) {
        if self.mode == Mode::Lie && self.ptr.down_on_cat && now - self.ptr.down_at < 350.0 && !self.doze {
            self.get_up(Mode::Sit, now);
            self.mind.quiet = now;
        }
        match self.held.take() {
            Some(Held::Ball) => {
                // a throw keeps the hand's speed, and a flick up lofts it; a click leaves it be
                self.ball.held = false;
                if self.ptr.dragged && now - self.ptr.moved < 90.0 {
                    let k = (1500.0 / self.ptr.vx.hypot(self.ptr.vy).max(1.0)).min(1.0);
                    self.ball.vx = self.ptr.vx * k;
                    self.ball.vy = self.ptr.vy * k;
                    self.ball.vz = (120.0 + (-self.ptr.vy * k).max(0.0) * 0.3).min(520.0);
                }
                self.throw_ball(now);
            }
            Some(Held::Bowl) => {
                self.bowl.at = Some(self.bowl_at());
                self.bowl.vy = 0.0;
                if matches!(self.mode, Mode::Eat | Mode::Beg | Mode::ToBowl) {
                    self.set_mode(Mode::ToBowl, now);
                    self.go("idle", 160.0, now);
                }
                self.moved();
            }
            Some(Held::Bed) => {
                // the bed moved: it goes to it
                self.bed.at = Some(self.bed_at());
                self.bed.vy = 0.0;
                if self.mode == Mode::Lie {
                    self.get_up(Mode::Sit, now);
                } else if matches!(self.mode, Mode::Sit | Mode::SitDown | Mode::Wash | Mode::Roam) {
                    self.set_mode(Mode::WalkHome, now + 400.0);
                }
                self.mind.quiet = now;
                self.moved();
            }
            None => {}
        }
    }

    fn moved(&mut self) {
        let (bed, bowl) = (self.bed_at().0, self.bowl_at().0);
        self.out.push(Out::Moved { bed, bowl });
    }

    /// The pointer is over the cat.
    pub fn over_cat(&self) -> bool {
        self.ptr.on && self.bounds.is_some_and(|(x0, y0, x1, y1)| self.ptr.x >= x0 && self.ptr.x <= x1 && self.ptr.y >= y0 && self.ptr.y <= y1)
    }

    /// The pointer is over the cat or one of its things, or holds one: its
    /// window catches clicks there.
    pub fn catches(&self) -> bool {
        if self.held.is_some() {
            return true;
        }
        if !self.ptr.on || self.mode == Mode::Away {
            return false;
        }
        let p = (self.ptr.x, self.ptr.y);
        self.over_cat() || self.ball.under(p) || self.on_bed(p) || self.on_bowl(p)
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
        // a second slap while it is still scared keeps the tidy to come
        let tidying = self.mode == Mode::Tidy || self.tidy.resume;
        self.stop_all();
        self.tidy.resume = tidying;
        self.hunt.act = None;
        self.wake_at = 0.0;
        self.scare_t0 = now;
        self.set_mode(Mode::Scare, now);
        self.go("idle", 90.0, now);
        self.mind.interest = 0.0;
        self.mind.bored = 1.0;
    }

    /// A hand on the trackpad (or a held pointer): it curls up and purrs.
    pub fn pet(&mut self, on: bool, now: f64) {
        if on {
            if self.doze || matches!(self.mode, Mode::Scare | Mode::Ball | Mode::Tidy | Mode::Leave | Mode::Flee | Mode::Gone | Mode::Peek) {
                return;
            }
            if self.mode == Mode::Away {
                self.place_home();
            }
            self.stop_all();
            self.lie_down(Mode::Pet, now);
        } else if self.mode == Mode::Pet {
            self.get_up(Mode::Sit, now);
            self.mind.bored = 0.6;
            self.mind.quiet = now;
        }
    }

    /// The Mac went to sleep (the lid closed): it is asleep at once. Awake
    /// again: it sleeps on a moment, then gets up.
    pub fn doze(&mut self, on: bool, now: f64) {
        if on == self.doze {
            return;
        }
        self.doze = on;
        if !on {
            if self.mode == Mode::Lie {
                self.wake_at = now + WAKE_AFTER;
            }
            return;
        }
        if self.petting {
            self.petting = false;
            self.out.push(Out::Purr(false));
        }
        self.held = None;
        self.ball.held = false;
        self.stop_all();
        self.hunt.act = None;
        self.wake_at = 0.0;
        self.wake_hello = false;
        if matches!(self.mode, Mode::Away | Mode::Tidy) {
            return;
        }
        if matches!(self.mode, Mode::Flee | Mode::Gone | Mode::Peek | Mode::Enter | Mode::Leave) {
            self.place_home();
        }
        self.set_mode(Mode::Lie, now);
        self.go("sleeping", 60.0, now);
    }

    /// The lid opened (or the Mac woke): a hello, once it is up.
    pub fn hello(&mut self, now: f64) {
        if self.mode == Mode::Away {
            self.out.push(Out::Voice(Kind::Hello));
            return self.enter(now);
        }
        if self.doze || self.wake_at > 0.0 || self.mode == Mode::WakeUp {
            self.wake_hello = true;
            return;
        }
        if self.mode == Mode::Lie {
            self.wake_hello = true;
            return self.get_up(Mode::Sit, now);
        }
        self.out.push(Out::Voice(Kind::Hello));
        self.mind.ear = now;
        if matches!(self.mode, Mode::Sit | Mode::Wash | Mode::Roam | Mode::WalkHome | Mode::SitDown) {
            self.stop_all();
            self.set_mode(Mode::Hello, now);
            self.go("idle", 300.0, now);
            self.mind.interest = 0.8;
        }
    }

    /// On the charger: it goes to its bowl and eats.
    pub fn eat(&mut self, now: f64) {
        self.want_eat = true;
        if self.mode == Mode::Away {
            self.enter(now);
        }
    }

    /// The battery is low: it sits by its bowl and begs until it is fed.
    pub fn beg(&mut self, now: f64) {
        self.hungry = true;
        if self.mode == Mode::Away {
            self.enter(now);
        }
    }

    /// The battery's charge (percent) and whether the charger is in.
    pub fn power(&mut self, pct: u8, plugged: bool) {
        let level = f64::from(pct.min(100)) / 100.0;
        self.battery = Some((level, plugged));
        if !plugged {
            self.want_eat = false;
        }
        if plugged || level >= FED {
            self.hungry = false;
        }
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
        self.icons = icons.into_iter().map(|(id, x, y)| Icon::new(id, x, y)).collect();
        self.slots = slots.to_vec();
        // what already sits on a slot stays there
        for &s in slots {
            if let Some(i) = self.icons.iter_mut().find(|i| i.slot.is_none() && dist((i.x, i.y), s) < 16.0) {
                i.slot = Some(s);
                i.home = true;
            }
        }
        self.set_mode(Mode::Tidy, now);
        self.tidy.step = Some(Step::Pick);
        self.out.push(Out::Mew);
    }

    /// A tidy is on: under way, waiting out a scare, or an icon still moving.
    pub fn tidying(&self) -> bool {
        self.mode == Mode::Tidy || self.tidy.resume || self.icons.iter().any(|i| i.lifted)
    }

    /// The cat is switched off: it and its things go at once.
    pub fn vanish(&mut self, now: f64) {
        self.lives = false;
        self.end_tidy(now);
        if self.petting {
            self.petting = false;
            self.out.push(Out::Purr(false));
        }
        self.held = None;
        self.hunt.act = None;
        self.set_mode(Mode::Away, now);
    }

    /// Icon `id` is moving (pushed, flying, held), not resting.
    pub fn lifted(&self, id: usize) -> bool {
        self.icons.iter().any(|i| i.id == id && i.lifted)
    }

    /// The tidy is switched off: it stops where it is. The icons are let go
    /// (Finder shows each where it last came to rest) and the cat goes home.
    pub fn end_tidy(&mut self, now: f64) {
        self.stop_all();
        self.tidy.step = None;
        self.tidy.resume = false;
        self.tidy.batch.clear();
        self.tidy.spot = None;
        self.icons.clear();
        self.slots.clear();
        if self.mode == Mode::Tidy {
            if self.lives {
                self.set_mode(Mode::WalkHome, now);
            } else {
                self.set_mode(Mode::Leave, now);
            }
        }
    }

    /// What happened since it was last asked.
    pub fn take(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// How often it wants drawing now (0: it is still, or away).
    pub fn fps(&self) -> f64 {
        let falling = |t: &Thing, floor: f64| t.vy != 0.0 || t.at.is_some_and(|p| p.1 < floor);
        let busy = self.held.is_some() || (self.ball.on && !self.ball.held && (self.ball.speed() > 1.0 || self.ball.z > 0.0 || self.ball.gone > 0.0)) || falling(&self.bed, self.ground.3 - 18.0) || falling(&self.bowl, self.ground.3 - 20.0);
        match self.mode {
            Mode::Away => {
                if self.icons.iter().any(|i| i.lifted) {
                    60.0
                } else {
                    0.0
                }
            }
            _ if busy => 60.0,
            Mode::Lie | Mode::Pet if self.doze => 4.0,
            Mode::Lie | Mode::Pet if self.cur.name == "sleeping" => 20.0,
            Mode::Gone => 10.0,
            Mode::Sit if self.sw.phase.is_none() && self.mind.interest < 0.2 && !self.ball.on => 30.0,
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
        self.play(name, ms, now, false);
    }

    /// The move played backwards (the walk: a step back).
    fn go_back(&mut self, name: &str, ms: f64, now: f64) {
        self.play(name, ms, now, true);
    }

    fn play(&mut self, name: &str, ms: f64, now: f64, rev: bool) {
        let name = anim(name);
        if self.cur.name == name && self.cur.rev == rev {
            return;
        }
        self.prev = Some(Clip { name: self.cur.name, t0: self.cur.t0, rev: self.cur.rev });
        self.cur = Clip { name, t0: now, rev };
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

    /// How far into its move a clip is, pack time (backwards at the pace of a step back).
    fn clip_t(c: &Clip, now: f64) -> f64 {
        (now - c.t0) * Self::rate(c.name) * if c.rev { -BACK / WALK } else { 1.0 }
    }

    fn anim_done(&self, now: f64) -> bool {
        !self.cur.rev && self.rig.anims.get(self.cur.name).is_some_and(|a| !a.looped && (now - self.cur.t0) * Self::rate(self.cur.name) >= a.len)
    }

    /// Faces this way at once (no turn).
    fn set_flip(&mut self, f: f64) {
        self.flip = f;
        self.dir = if f > 0.0 { -1.0 } else { 1.0 };
        self.turn = (-1e9, f, f);
        self.hop = 0.0;
    }

    fn place_home(&mut self) {
        let (x, y) = self.home();
        self.x = x;
        self.y = y;
        self.set_flip(1.0);
    }

    /// In from the right edge, to its bed.
    fn enter(&mut self, now: f64) {
        let (_, y) = self.home();
        self.x = self.ground.2 + 90.0;
        self.y = y;
        self.set_flip(1.0);
        self.set_mode(Mode::Enter, now);
        self.go("walk", 10.0, now);
    }

    fn stop_all(&mut self) {
        self.sw.phase = None;
        self.sw.at = None;
        self.sw.rake = false;
        // the heap drops where it is
        self.tidy.next = None;
        self.tidy.pile.clear();
        for i in &mut self.icons {
            i.hold = Hold::No;
        }
    }

    fn sit_down(&mut self, ms: f64, now: f64) {
        self.set_mode(Mode::SitDown, now);
        self.go("sit_tranistion_to", ms, now);
    }

    /// Down through the pack's own move (a blend straight from sitting to
    /// lying slides the parts about).
    fn lie_down(&mut self, m: Mode, now: f64) {
        let lying = matches!(self.cur.name, "sleeping" | "sleep_transition_to");
        self.set_mode(m, now);
        if !lying {
            self.go("sleep_transition_to", 200.0, now);
        }
    }

    /// Up through the pack's own move, then `next`.
    fn get_up(&mut self, next: Mode, now: f64) {
        self.set_mode(Mode::WakeUp, now);
        self.wake_next = next;
        self.wake_at = 0.0;
        // just up: no walk off at once
        self.mind.next_roam = self.mind.next_roam.max(now + 8000.0);
        self.go("sleep_transition_from", 150.0, now);
    }


    fn swipe(&mut self, now: f64, quick: bool, at: Option<usize>, dir: Option<(f64, f64)>, power: f64, soft: bool) {
        self.sw.phase = Some(if quick { Phase::Strike } else { Phase::Crouch });
        self.sw.t0 = now;
        self.sw.hit = false;
        self.sw.at = at;
        self.sw.dir = dir;
        self.sw.power = power;
        self.sw.soft = soft;
        self.sw.rake = false;
        let k = if soft { 1.5 } else { 1.0 };
        let crouch = if soft { self.rnd(120.0, 200.0) } else { self.rnd(200.0, 380.0) };
        let strike = self.rnd(80.0, 115.0) * k;
        let hold = self.rnd(40.0, 90.0);
        let back = self.rnd(190.0, 260.0);
        self.sw.dur = [crouch, strike, hold, back];
        self.sw.again = !quick && at.is_none() && self.random() < 0.3;
        self.mind.last_swipe = now;
    }

    /// Something small to do while it sits: a glance, an ear, the tail, a
    /// wash, a yawn, or (after a long quiet) lie down.
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
            _ => self.lie_down(Mode::Lie, now),
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
        if dx.abs() > if d < 40.0 { 6.0 } else { 24.0 } {
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

    // ---------------------------------------------------------------- icons

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
        let mut snapped = Vec::new();
        for k in 0..self.icons.len() {
            let i = &mut self.icons[k];
            if i.held() || i.fly.is_some() {
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
                let d = dist((i.x, i.y), slot);
                if i.home && d > 0.6 {
                    let e = (s * 14.0).min(1.0);
                    i.x += (slot.0 - i.x) * e;
                    i.y += (slot.1 - i.y) * e;
                    // Finder hears where it ends up
                    snapped.push(k);
                } else if i.home && d > 0.0 {
                    // the last of the glide (Finder is told the slot itself)
                    i.x = slot.0;
                    i.y = slot.1;
                }
            }
        }
        for k in snapped {
            self.lift(k);
        }
        // icons push each other: an icon with its label is a box
        let n = self.icons.len();
        let mut lifts = Vec::new();
        let mut taps = 0.0f64;
        for a in 0..n {
            for b in a + 1..n {
                let (p, q) = (&self.icons[a], &self.icons[b]);
                // a flying or held icon passes over what is tidy already
                // and what is tidy stays put: a loose icon is pushed off it
                if p.fly.is_some() || q.fly.is_some() || (p.held() && q.held()) || (p.home && q.home) || (p.held() && q.home) || (q.held() && p.home) {
                    continue;
                }
                let (dx, dy) = (q.x - p.x, q.y - p.y);
                let (ox, oy) = (bw * 2.0 - dx.abs(), bh * 2.0 - dy.abs());
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                let along_x = ox / bw < oy / bh;
                let (nx, ny, over) = if along_x { (if dx < 0.0 { -1.0 } else { 1.0 }, 0.0, ox) } else { (0.0, if dy < 0.0 { -1.0 } else { 1.0 }, oy) };
                let fa = if p.held() || p.home { 0.0 } else if q.held() || q.home { 1.0 } else { 0.5 };
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
        for &k in &lifts {
            self.lift(k);
        }
        if taps > 120.0 {
            self.out.push(Out::Tap { hit: Hit::Pat, soft: true });
        }
        // what came to rest goes back to Finder
        for k in 0..self.icons.len() {
            let i = &self.icons[k];
            let settled = i.slot.is_none_or(|s| !i.home || dist((i.x, i.y), s) < 0.6);
            // still pushed by another (one carried over it): not yet
            let pushed = lifts.contains(&k);
            if i.lifted && !i.held() && i.fly.is_none() && Self::speed(i) < 0.5 && settled && !pushed && self.sw.at != Some(k) {
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

    /// The icons still to tidy.
    fn loose(&self) -> Vec<usize> {
        (0..self.icons.len()).filter(|&k| {
            let i = &self.icons[k];
            !i.home && !i.held() && i.fly.is_none()
        }).collect()
    }

    /// The next heap: the icon nearest the nose and the ones nearest to it,
    /// 5 to 8 of them, with the next free slots (the top ones take the top
    /// slots). An icon sitting on one of those slots goes on this heap too.
    fn pick_batch(&mut self) -> bool {
        let left = self.loose();
        if left.is_empty() {
            return false;
        }
        // a heap dropped half way (a slap) gives its slots back
        for &k in &left {
            self.icons[k].slot = None;
        }
        let runs = left.len().div_ceil(7);
        let n = left.len().div_ceil(runs);
        let taken: Vec<(f64, f64)> = self.icons.iter().filter_map(|i| i.slot).collect();
        let free: Vec<(f64, f64)> = self.slots.iter().copied().filter(|s| !taken.iter().any(|t| dist(*t, *s) < 0.5)).take(n).collect();
        let at = |k: usize| (self.icons[k].x, self.icons[k].y);
        let nose = self.nose;
        let seed = *left.iter().min_by(|&&a, &&b| dist(at(a), nose).total_cmp(&dist(at(b), nose))).unwrap_or(&left[0]);
        let by = |k: usize| {
            let (p, q) = (at(k), at(seed));
            (p.0 - q.0).hypot((p.1 - q.1) * 1.3)
        };
        let mut order = left.clone();
        order.sort_by(|&a, &b| by(a).total_cmp(&by(b)));
        let mut batch: Vec<usize> = order.into_iter().take(n).collect();
        for s in &free {
            if let Some(&k) = left.iter().find(|&&k| !batch.contains(&k) && dist(at(k), *s) < 50.0) {
                batch.pop();
                batch.insert(0, k);
            }
        }
        let mut by_top = batch.clone();
        by_top.sort_by(|&a, &b| self.icons[a].y.total_cmp(&self.icons[b].y).then(self.icons[a].x.total_cmp(&self.icons[b].x)));
        for (k, s) in by_top.into_iter().zip(free.iter().copied()) {
            self.icons[k].slot = Some(s);
        }
        batch.retain(|&k| self.icons[k].slot.is_some());
        // no room left in the grid: the rest stay where they are
        if free.is_empty() {
            for &k in &left {
                self.icons[k].home = true;
            }
        }
        self.tidy.batch = batch;
        self.tidy.pile.clear();
        self.tidy.next = None;
        !self.tidy.batch.is_empty()
    }

    /// Icon `k` hops onto the heap or into the mouth, `delay` ms from now.
    fn attach(&mut self, k: usize, now: f64, delay: f64, to: Hold) {
        self.lift(k);
        if !self.tidy.pile.contains(&k) {
            self.icons[k].j = self.tidy.pile.len();
            self.tidy.pile.push(k);
        }
        let nose = self.nose;
        let i = &mut self.icons[k];
        i.hold = to;
        i.home = false;
        i.vx = 0.0;
        i.vy = 0.0;
        i.vr = 0.0;
        i.hop = now + delay;
        i.from = None;
        i.tapped = false;
        i.hop_ms = if to == Hold::Mouth { 150.0 } else { 170.0 + (dist((i.x, i.y), nose) * 0.55).min(260.0) };
    }

    /// The paw's sweep: the heap's icons in reach hop together in front of
    /// the paws, one a beat after another.
    fn rake_in(&mut self, now: f64) {
        self.sw.hit = true;
        self.tidy.spot = Some((self.nose.0 + self.tidy.face * 40.0, self.nose.1 + 34.0));
        let nose = self.nose;
        let mut reach: Vec<usize> = self.tidy.batch.iter().copied().filter(|&k| {
            let i = &self.icons[k];
            !i.held() && !i.home && dist((i.x, i.y), nose) < 330.0
        }).collect();
        reach.sort_by(|&a, &b| dist((self.icons[a].x, self.icons[a].y), nose).total_cmp(&dist((self.icons[b].x, self.icons[b].y), nose)));
        for (n, k) in reach.into_iter().enumerate() {
            self.attach(k, now, n as f64 * 60.0, Hold::Heap);
        }
        self.out.push(Out::Tap { hit: Hit::Rake, soft: false });
    }

    fn middle(&self, ks: &[usize]) -> Option<(f64, f64)> {
        (!ks.is_empty()).then(|| {
            let n = ks.len() as f64;
            (ks.iter().map(|&k| self.icons[k].x).sum::<f64>() / n, ks.iter().map(|&k| self.icons[k].y).sum::<f64>() / n)
        })
    }

    fn tidy_think(&mut self, now: f64, dt: f64) {
        let Some(step) = self.tidy.step else { return };
        match step {
            Step::Pick => {
                // let things stop first
                if self.loose().iter().any(|&k| Self::speed(&self.icons[k]) > 30.0) {
                    self.go("idle", 200.0, now);
                    return;
                }
                if !self.pick_batch() {
                    if self.icons.iter().any(|i| i.held() || i.fly.is_some()) {
                        return;
                    }
                    self.tidy.step = None;
                    self.set_mode(Mode::WalkHome, now);
                    self.mind.bored = 0.3;
                    self.out.push(Out::Tidied);
                    return;
                }
                let m = self.middle(&self.tidy.batch).unwrap_or((self.x, self.y));
                let col = self.tidy.batch.iter().filter_map(|&k| self.icons[k].slot).map(|s| s.0).fold(f64::INFINITY, f64::min);
                self.tidy.face = if col > m.0 { 1.0 } else { -1.0 };
                self.tidy.step = Some(Step::Goto);
                self.tidy.since = now;
            }
            Step::Goto => {
                let rest: Vec<usize> = self.tidy.batch.iter().copied().filter(|&k| !self.icons[k].held() && !self.icons[k].home).collect();
                let Some(m) = self.middle(&rest) else {
                    self.tidy.step = Some(Step::Collect);
                    return;
                };
                let face = self.tidy.face;
                let to = (self.clamp_x(m.0 - face * (self.rel_nose.0 + 70.0)), self.clamp_y(m.1 - self.rel_nose.1 - 20.0));
                let run = dist(to, (self.x, self.y)) > 140.0;
                if self.walk_to(to, dt, run, now) || now - self.tidy.since > 5000.0 {
                    self.dir = face;
                    self.go("idle", 120.0, now);
                    // aimed at the middle of the bunch
                    self.swipe(now, false, None, None, 0.0, true);
                    self.sw.rake = true;
                    self.sw.again = false;
                    self.tidy.step = Some(Step::Rake);
                }
            }
            Step::Rake => {
                let landed = self.tidy.pile.iter().all(|&k| now > self.icons[k].hop + self.icons[k].hop_ms + 120.0);
                if self.sw.phase.is_none() && landed {
                    self.tidy.step = Some(Step::Grab);
                    self.tidy.t0 = now;
                    self.tidy.dip = now;
                }
            }
            Step::Grab => {
                if now - self.tidy.t0 > 150.0 && self.tidy.pile.iter().any(|&k| self.icons[k].hold == Hold::Heap) {
                    for k in self.tidy.pile.clone() {
                        if self.icons[k].hold == Hold::Heap {
                            let delay = self.icons[k].j as f64 * 12.0;
                            self.attach(k, now, delay, Hold::Mouth);
                        }
                    }
                    self.out.push(Out::Tap { hit: Hit::Grab, soft: false });
                }
                if now - self.tidy.t0 > 460.0 {
                    self.tidy.step = Some(Step::Collect);
                }
            }
            Step::Collect => {
                if now < self.tidy.wait {
                    return;
                }
                let want: Vec<usize> = self.tidy.batch.iter().copied().filter(|&k| !self.icons[k].held() && !self.icons[k].home && self.icons[k].fly.is_none()).collect();
                if want.is_empty() {
                    self.tidy.next = None;
                    self.tidy.step = Some(if self.tidy.pile.is_empty() { Step::Pick } else { Step::Carry });
                    return;
                }
                let nose = self.nose;
                let next = match self.tidy.next.filter(|k| want.contains(k)) {
                    Some(k) => k,
                    None => {
                        let k = *want.iter().min_by(|&&a, &&b| dist((self.icons[a].x, self.icons[a].y), nose).total_cmp(&dist((self.icons[b].x, self.icons[b].y), nose))).unwrap_or(&want[0]);
                        let dx = self.icons[k].x - self.x;
                        self.tidy.face = if dx != 0.0 { dx.signum() } else { self.dir };
                        self.tidy.since = now;
                        self.tidy.next = Some(k);
                        k
                    }
                };
                let ahead = (nose.0 + self.tidy.face * 30.0, nose.1 + 20.0);
                let at = (self.icons[next].x, self.icons[next].y);
                if dist(ahead, at) < 60.0 || now - self.tidy.since > 2600.0 {
                    // head down, the icon into the mouth
                    self.go("idle", 120.0, now);
                    self.tidy.dip = now;
                    self.attach(next, now, 150.0, Hold::Mouth);
                    self.out.push(Out::Tap { hit: Hit::Grab, soft: false });
                    self.tidy.next = None;
                    self.tidy.wait = now + 380.0;
                    return;
                }
                let to = self.nose_stand(next, self.tidy.face);
                let run = dist(at, nose) > 120.0;
                self.walk_to(to, dt, run, now);
            }
            Step::Carry => {
                let sl: Vec<(f64, f64)> = self.tidy.pile.iter().filter_map(|&k| self.icons[k].slot).collect();
                if sl.is_empty() {
                    self.tidy.step = Some(Step::Pick);
                    return;
                }
                let cy = sl.iter().map(|s| s.1).sum::<f64>() / sl.len() as f64;
                let cx = sl.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
                let face = if cx > self.x { 1.0 } else { -1.0 };
                self.tidy.face = face;
                let to = (self.clamp_x(cx - face * (self.rel_nose.0 + 96.0)), self.clamp_y(cy - self.rel_nose.1 - 20.0));
                let run = dist(to, (self.x, self.y)) > 320.0;
                if self.walk_to(to, dt, run, now) {
                    self.dir = face;
                    self.go("idle", 140.0, now);
                    self.tidy.dip = now;
                    // the mouth opens: each icon flies to its slot, a beat after the one before
                    for (n, k) in std::mem::take(&mut self.tidy.pile).into_iter().enumerate() {
                        let i = &mut self.icons[k];
                        i.hold = Hold::No;
                        i.fly = Some(Fly { x0: i.x, y0: i.y, r0: i.rot, t0: now + 160.0 + n as f64 * 75.0, ms: 430.0 });
                    }
                    self.tidy.step = Some(Step::Deal);
                    self.tidy.t0 = now;
                }
            }
            Step::Deal => {
                // watches them land, then the next heap
                if self.tidy.batch.iter().all(|&k| self.icons[k].fly.is_none()) && now - self.tidy.t0 > 900.0 {
                    self.mind.swish = now;
                    self.tidy.step = Some(Step::Pick);
                }
            }
        }
    }

    /// The heap: on the desk in front of the paws, then hanging from the
    /// mouth by the icons' top edges; and the icons flying to their slots.
    fn hold(&mut self, now: f64) {
        let (f, nose) = (self.tidy.face, self.nose);
        let spot = self.tidy.spot.unwrap_or(nose);
        let moving = matches!(self.cur.name, "walk" | "run");
        let mut taps = 0;
        for i in &mut self.icons {
            if i.held() {
                if now < i.hop {
                    continue;
                }
                let j = i.j % HEAP.len();
                let (tx, ty, tr) = if i.hold == Hold::Heap {
                    let h = HEAP[j];
                    (spot.0 + f * h.0, spot.1 + h.1, h.2 * f)
                } else {
                    let swing = if moving { (now / 105.0 + i.j as f64 * 1.3).sin() * 6.0 } else { (now / 420.0 + i.j as f64).sin() * 1.5 };
                    let a = (FAN[j] + swing) * f;
                    let (m, r) = ((nose.0 - f * 6.0, nose.1 + 9.0), a * D2R);
                    (m.0 - r.sin() * 18.0, m.1 + r.cos() * 18.0, a)
                };
                let from = *i.from.get_or_insert((i.x, i.y, i.rot));
                let hp = (now - i.hop) / i.hop_ms;
                if hp < 1.0 {
                    // in the air: an arc from where it lay, turning on the way
                    let e = 1.0 - (1.0 - hp).powi(3);
                    let lift = if i.hold == Hold::Mouth { 8.0 } else { 16.0 + i.hop_ms * 0.08 };
                    i.x = from.0 + (tx - from.0) * e;
                    i.y = from.1 + (ty - from.1) * e - (hp * PI).sin() * lift;
                    i.rot = from.2 + (tr - from.2) * e + (hp * PI).sin() * 14.0 * f;
                } else {
                    if !i.tapped {
                        i.tapped = true;
                        i.from = None;
                        if i.hold == Hold::Heap {
                            taps += 1;
                        }
                    }
                    let k = if i.hold == Hold::Mouth { 0.55 } else { 0.4 };
                    i.x += (tx - i.x) * k;
                    i.y += (ty - i.y) * k;
                    i.rot += (tr - i.rot) * 0.3;
                }
                i.vx = 0.0;
                i.vy = 0.0;
                i.label += (0.0 - i.label) * 0.2;
            } else if let Some(fl) = i.fly {
                let p = (now - fl.t0) / fl.ms;
                if p < 0.0 {
                    continue;
                }
                let Some(slot) = i.slot else {
                    i.fly = None;
                    continue;
                };
                let q = p.min(1.0);
                let e = 1.0 - (1.0 - q).powi(3);
                i.x = fl.x0 + (slot.0 - fl.x0) * e;
                i.y = fl.y0 + (slot.1 - fl.y0) * e - (q * PI).sin() * 46.0;
                i.rot = fl.r0 * (1.0 - q) + (q * PI * 2.2).sin() * 9.0 * (1.0 - q);
                i.label = q;
                if q >= 1.0 {
                    i.fly = None;
                    i.home = true;
                    i.x = slot.0;
                    i.y = slot.1;
                    i.rot = 0.0;
                    i.label = 1.0;
                    taps += 1;
                }
            } else {
                i.label += (1.0 - i.label) * 0.2;
            }
        }
        if taps > 0 {
            self.out.push(Out::Tap { hit: Hit::Land, soft: false });
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
        self.out.push(Out::Tap { hit: Hit::Pat, soft: self.sw.soft });
    }

    /// What it wants from its bowl now: to eat (on the charger, once per
    /// plug-in) or to beg (the battery is low).
    fn need(&self) -> Option<Need> {
        let plugged = self.battery.is_some_and(|b| b.1);
        if self.want_eat {
            Some(Need::Eat)
        } else if !plugged && (self.hungry || self.battery.is_some_and(|b| b.0 < LOW)) {
            Some(Need::Beg)
        } else {
            None
        }
    }

    fn think(&mut self, now: f64, dt: f64) {
        let o = self.home();
        if !self.x.is_finite() {
            self.place_home();
        }
        self.mind.interest = (self.mind.interest + self.ptr.stim - dt / 1000.0 * 0.12).clamp(0.0, 1.0);
        self.ptr.stim = 0.0;
        self.mind.bored = (self.mind.bored - dt / 1000.0 * if self.mode == Mode::Ball { 0.02 } else { 0.09 }).max(0.0);
        if self.mind.interest > 0.3 {
            self.mind.quiet = now;
        }
        if self.doze {
            self.clicks.clear();
        }
        // three clicks on the cat: the yarn ball
        if self.clicks.len() >= 3 && !matches!(self.mode, Mode::Tidy | Mode::Away) {
            self.start_ball(now);
        }
        // asleep with the Mac, wherever it got to: it curls up there
        if self.doze && matches!(self.mode, Mode::Sit | Mode::Wash | Mode::Roam | Mode::Hello | Mode::Beg | Mode::Eat | Mode::Ball) {
            self.stop_all();
            self.hunt.act = None;
            self.set_mode(Mode::Lie, now);
            self.go("sleeping", 300.0, now);
        }
        let need = self.need();
        // a pointer held on the cat pets it
        if self.ptr.down && self.ptr.down_on_cat && self.held.is_none() && now - self.ptr.down_at > 350.0 && !self.petting && matches!(self.mode, Mode::Sit | Mode::Lie | Mode::Wash | Mode::Beg) && !self.doze {
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
        let me = (self.x, self.y);
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
            Mode::Sit => self.sit_think(now, need, o),
            Mode::Wash => {
                if (self.mind.interest > 0.7 && self.mind.bored < 0.5) || now - self.mode_t0 > 2600.0 {
                    self.set_mode(Mode::Sit, now);
                    self.go("sitting", 260.0, now);
                }
            }
            Mode::Lie => {
                if self.cur.name == "sleep_transition_to" && self.anim_done(now) {
                    self.go("sleeping", 120.0, now);
                }
                // it sleeps on until someone wakes it: a click, the ball, the
                // bed moved, a slap, the lid opened, hunger
                if self.doze {
                } else if self.wake_at > 0.0 {
                    if now > self.wake_at {
                        self.get_up(Mode::Sit, now);
                        self.mind.quiet = now;
                        self.mind.next_idle = now + 4000.0;
                    }
                } else if (need == Some(Need::Eat) && now - self.mode_t0 > 1200.0) || (need == Some(Need::Beg) && now - self.mode_t0 > 4000.0) {
                    self.get_up(Mode::Sit, now);
                    self.mind.quiet = now;
                }
            }
            Mode::Pet => {
                if self.cur.name == "sleep_transition_to" && self.anim_done(now) {
                    self.go("sleeping", 120.0, now);
                }
            }
            Mode::WakeUp => {
                if self.anim_done(now) {
                    if std::mem::take(&mut self.wake_hello) {
                        self.out.push(Out::Voice(Kind::Hello));
                        self.mind.ear = now;
                        self.mind.ear_side = 1.0;
                    }
                    if self.wake_next == Mode::Ball && self.ball.on {
                        self.set_mode(Mode::Ball, now);
                        self.hunt.act = None;
                        self.hunt.rest = now + 150.0;
                        self.go("idle", 220.0, now);
                    } else if dist(me, o) > 30.0 && !self.ball.on {
                        self.set_mode(Mode::WalkHome, now + 200.0);
                    } else {
                        self.set_mode(Mode::Sit, now);
                        self.go("sitting", 150.0, now);
                    }
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
                        self.edge = self.exit_edge();
                        self.set_mode(Mode::Flee, now);
                    }
                }
            }
            // off through the edge nearest its bed, a while out of sight, a peek, back home
            Mode::Flee => {
                if self.walk_to(self.edge_point(self.edge, true), dt, true, now) {
                    let away = self.rnd(1800.0, 3400.0);
                    self.set_mode(Mode::Gone, now + away);
                    self.go("idle", 100.0, now);
                }
            }
            Mode::Gone => {
                if now > self.mode_t0 {
                    let (x, y) = self.edge_point(self.edge, true);
                    (self.x, self.y) = (x, y);
                    let dir = match self.edge {
                        Edge::Right => -1.0,
                        Edge::Left => 1.0,
                        Edge::Bottom => {
                            if self.bed_at().0 > (self.ground.0 + self.ground.2) / 2.0 {
                                -1.0
                            } else {
                                1.0
                            }
                        }
                    };
                    self.set_flip(if dir < 0.0 { 1.0 } else { -1.0 });
                    self.set_mode(Mode::Peek, now);
                }
            }
            Mode::Peek => {
                let t = now - self.mode_t0;
                if t < 900.0 {
                    if self.walk_to(self.edge_point(self.edge, false), dt, false, now) {
                        self.go("idle", 160.0, now);
                    }
                } else if t < 2600.0 {
                    // it looks about before it comes out
                    self.go("idle", 160.0, now);
                    if self.mind.glance.is_none() {
                        let g = (self.rnd(self.w * 0.2, self.w * 0.8), self.rnd(self.h * 0.2, self.h * 0.7), now + 700.0);
                        self.mind.glance = Some(g);
                    }
                    if t > 1200.0 && t < 1260.0 {
                        self.mind.ear = now;
                        self.mind.ear_side = 1.0;
                    }
                } else {
                    self.mind.glance = None;
                    self.set_mode(Mode::WalkHome, now);
                    self.mind.bored = 0.5;
                    self.mind.next_roam = now + 40000.0;
                }
            }
            Mode::Hello => {
                if now - self.mode_t0 > 2200.0 {
                    self.sit_down(220.0, now);
                }
            }
            Mode::Tidy => self.tidy_think(now, dt),
            Mode::WalkHome => {
                if now < self.mode_t0 {
                    self.go("idle", 200.0, now);
                } else if need.is_some() && !self.ball.on {
                    self.set_mode(Mode::ToBowl, now);
                } else if self.walk_to(o, dt, false, now) {
                    self.dir = -1.0;
                    self.sit_down(180.0, now);
                }
            }
            Mode::Roam => {
                if self.mind.interest > 0.6 {
                    self.set_mode(Mode::Sit, now);
                    self.go("idle", 200.0, now);
                } else if need.is_some() {
                    self.set_mode(Mode::ToBowl, now);
                } else if self.walk_to(self.roam_to, dt, dist(self.roam_to, me) > 500.0, now) {
                    self.sit_down(200.0, now);
                    self.mind.quiet = now + self.rnd(4000.0, 14000.0);
                }
            }
            Mode::ToBowl => {
                let (sx, sy, face) = self.bowl_stand();
                if self.walk_to((sx, sy), dt, dist((sx, sy), me) > 500.0, now) {
                    self.dir = face;
                    match need {
                        Some(Need::Eat) => self.start_eat(now),
                        Some(Need::Beg) => {
                            self.set_mode(Mode::Beg, now);
                            self.feed.step = BegStep::Stand;
                            self.feed.t = now;
                            self.go("idle", 200.0, now);
                        }
                        None => self.sit_down(200.0, now),
                    }
                }
            }
            Mode::Eat => {
                // head down in the bowl, the app's eating sounds one after another; then a wash
                self.go("idle", 200.0, now);
                if now - self.mode_t0 > 450.0 && now >= self.feed.voice && now < self.feed.until {
                    self.out.push(Out::Voice(Kind::Eat));
                    self.feed.voice = now + CRUNCH_MS;
                }
                if now > self.feed.until || !self.want_eat {
                    self.want_eat = false;
                    self.set_mode(Mode::Wash, now);
                    self.go("clean", 300.0, now);
                    self.mind.quiet = now;
                    self.mind.next_roam = self.mind.next_roam.max(now + 20000.0);
                }
            }
            Mode::Beg => self.beg_think(now, need),
            Mode::Ball => self.ball_think(now, dt),
        }
        // the swipe's clock
        if let Some(p) = self.sw.phase {
            if now - self.sw.t0 > self.sw.dur(p) {
                if p == Phase::Strike && !self.sw.hit && self.sw.rake {
                    self.rake_in(now);
                } else if p == Phase::Strike && !self.sw.hit {
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
        self.turn(now);
    }

    fn sit_think(&mut self, now: f64, need: Option<Need>, o: (f64, f64)) {
        let me = (self.x, self.y);
        let free = self.sw.phase.is_none();
        if need.is_some() && free && !self.ball.on {
            return self.set_mode(Mode::ToBowl, now);
        }
        // standing about with nothing to do: it sits down
        if free && self.cur.name == "idle" && now - self.mode_t0 > 1500.0 {
            return self.sit_down(260.0, now);
        }
        if self.ball.on {
            // the ball behind it: it turns round to keep it in view
            if (self.ball.x - self.x).abs() > 60.0 && (self.ball.x - self.x).signum() != self.dir {
                self.dir = (self.ball.x - self.x).signum();
            }
            // a ball rolling past: it goes for it
            if !self.ball.held && self.ball.speed() > 180.0 && dist((self.ball.x, self.ball.y), me) < 420.0 && self.mind.bored < 0.6 {
                self.set_mode(Mode::Ball, now);
                self.hunt.act = None;
                return;
            }
        }
        if free && self.mind.bored >= 0.75 && self.mind.interest > 0.2 {
            // had enough: looks away, and maybe washes or lies down
            self.mind.interest = 0.0;
            let g = (self.rnd(self.w * 0.05, self.w * 0.4), self.rnd(self.h * 0.1, self.h * 0.6), now + self.rnd(1200.0, 2200.0));
            self.mind.glance = Some(g);
            let r = self.random();
            if r < 0.35 {
                self.set_mode(Mode::Wash, now);
                self.go("clean", 300.0, now);
            } else if r < 0.5 {
                self.lie_down(Mode::Lie, now);
            }
        }
        if self.mode != Mode::Sit {
            return;
        }
        // now and then it gets up and walks somewhere else on the desk
        if self.lives && free && self.mind.interest < 0.2 && now > self.mind.next_roam {
            self.mind.next_roam = now + self.rnd(25000.0, 60000.0);
            let g = self.ground;
            let (rx, ry) = (self.rnd(g.0 + (g.2 - g.0) * 0.1, g.0 + (g.2 - g.0) * 0.9), self.rnd(g.1 + (g.3 - g.1) * 0.2, g.1 + (g.3 - g.1) * 0.95));
            self.roam_to = (self.clamp_x(rx), self.clamp_y(ry));
            return self.set_mode(Mode::Roam, now);
        }
        if free && self.mind.interest < 0.3 && now > self.mind.next_idle {
            self.idle(now);
        }
        if self.mode == Mode::Sit && free && now - self.mind.quiet > 9000.0 && dist(me, o) > 30.0 {
            self.set_mode(Mode::WalkHome, now);
        }
        if self.mode == Mode::Sit && !self.lives && free && now - self.mode_t0 > 2500.0 && self.mind.interest < 0.3 && !self.ball.on {
            self.set_mode(Mode::Leave, now);
        }
    }

    /// Begging by the bowl: up on its hind legs with a meow, down, a sit
    /// and a stare, up again; until it is fed.
    fn beg_think(&mut self, now: f64, need: Option<Need>) {
        // a beg asked for with a full battery (a try from Settings) is one round
        if now - self.mode_t0 > 15000.0 && !self.battery.is_some_and(|b| b.0 < LOW && !b.1) {
            self.hungry = false;
        }
        let _ = need;
        match self.need() {
            Some(Need::Eat) => return self.start_eat(now),
            None => {
                if self.cur.name == "sitting" {
                    self.set_mode(Mode::Sit, now);
                } else {
                    self.sit_down(240.0, now);
                }
                self.mind.quiet = now;
                return;
            }
            Some(Need::Beg) => {}
        }
        let t = now - self.feed.t;
        let next = |l: &mut Live, s: BegStep| {
            l.feed.step = s;
            l.feed.t = now;
        };
        match self.feed.step {
            BegStep::Stand => {
                self.go("idle", 200.0, now);
                if t > 600.0 {
                    next(self, BegStep::Up);
                    self.go("beg", 220.0, now);
                    if now - self.feed.meow > 6000.0 {
                        self.feed.meow = now;
                        self.out.push(Out::Voice(Kind::Hungry));
                    }
                }
            }
            BegStep::Up => {
                if t > self.anim_ms("beg") {
                    next(self, BegStep::Down);
                    self.go("idle", 260.0, now);
                }
            }
            BegStep::Down => {
                if t > 400.0 {
                    next(self, BegStep::Sitting);
                    self.feed.wait = self.rnd(3000.0, 5200.0);
                    self.go("sit_tranistion_to", 180.0, now);
                }
            }
            BegStep::Sitting => {
                if self.cur.name == "sit_tranistion_to" && self.anim_done(now) {
                    self.go("sitting", 120.0, now);
                }
                if t > 1800.0 && t < 1860.0 {
                    self.mind.ear = now;
                    self.mind.ear_side = -1.0;
                }
                if t > self.feed.wait {
                    next(self, BegStep::Rise);
                    self.go("sit_transition_from", 150.0, now);
                }
            }
            BegStep::Rise => {
                if self.anim_done(now) {
                    next(self, BegStep::Stand);
                }
            }
        }
    }

    fn start_eat(&mut self, now: f64) {
        self.set_mode(Mode::Eat, now);
        // a meal is two or three of the eating sounds
        let n = if self.random() < 0.5 { 2.0 } else { 3.0 };
        self.feed.voice = now + 450.0;
        self.feed.until = now + 450.0 + n * CRUNCH_MS;
        self.go("idle", 260.0, now);
    }

    /// The body comes round to where it faces: a small hop, and the turn
    /// at the top of it. No turning back for a moment after a turn.
    fn turn(&mut self, now: f64) {
        if self.want_flip() != self.turn.2 && now - self.turn.0 < TURN_MS + 250.0 {
            self.dir = if self.turn.2 > 0.0 { -1.0 } else { 1.0 };
        }
        let want = self.want_flip();
        if self.turn.2 != want {
            self.turn = (now, self.flip, want);
        }
        let (t0, from, to) = self.turn;
        let tp = ((now - t0) / TURN_MS).min(1.0);
        self.flip = from + (to - from) * ease((tp - 0.36) / 0.28);
        self.hop = if (to - from).abs() > 0.6 { (PI * tp).sin() } else { 0.0 };
    }

    fn anim_ms(&self, name: &str) -> f64 {
        self.rig.anims.get(name).map_or(1000.0, |a| a.len) / Self::rate(name)
    }

    // ---------------------------------------------------------------- the ball

    /// Three clicks on the cat: a yarn ball drops in front of it.
    fn start_ball(&mut self, now: f64) {
        self.clicks.clear();
        if self.petting {
            self.petting = false;
            self.out.push(Out::Purr(false));
        }
        self.stop_all();
        let a = self.ball_area();
        let (x, y) = ((self.x + self.dir * 150.0).clamp(a.0, a.2), (self.y - 30.0).clamp(a.1, a.3));
        self.ball = Ball { on: true, x, y, z: 380.0, alpha: 1.0, touched: now, ..Ball::default() };
        self.ball_clicks.clear();
        self.mind.bored = 0.0;
        self.mind.interest = 1.0;
        self.out.push(Out::Chirp);
        if matches!(self.mode, Mode::Lie | Mode::Pet) {
            return self.get_up(Mode::Ball, now);
        }
        self.set_mode(Mode::Ball, now);
        self.hunt.act = None;
        self.hunt.rest = now + 500.0;
        self.go("idle", 200.0, now);
    }

    /// The ball let go of: it wants it again.
    fn throw_ball(&mut self, now: f64) {
        self.ball.touched = now;
        self.mind.bored = (self.mind.bored - 0.5).max(0.0);
        self.mind.interest = 1.0;
        if self.mode == Mode::Lie && !self.doze {
            return self.get_up(Mode::Ball, now);
        }
        if matches!(self.mode, Mode::Sit | Mode::Wash | Mode::WalkHome | Mode::Roam | Mode::SitDown) {
            self.set_mode(Mode::Ball, now);
            self.hunt.act = None;
            self.hunt.rest = now + 250.0;
            self.go("idle", 200.0, now);
        }
    }

    /// Three clicks on the ball: it shrinks away, and the cat goes home.
    fn put_ball_away(&mut self, now: f64) {
        self.ball.held = false;
        self.held = None;
        self.ball.gone = now;
        self.hunt.act = None;
        if !matches!(self.mode, Mode::Scare | Mode::Pet | Mode::Lie | Mode::WakeUp | Mode::Flee | Mode::Gone | Mode::Peek | Mode::Tidy) {
            self.set_mode(Mode::WalkHome, now + 500.0);
            self.go("idle", 200.0, now);
        }
        self.mind.bored = 1.0;
        self.mind.interest = 0.0;
        self.mind.next_roam = now + 30000.0;
    }

    fn kick_ball(&mut self, power: f64) {
        self.ball.vx = self.dir * self.rnd(240.0, 480.0) * power;
        self.ball.vy = self.rnd(-190.0, 190.0) * power;
        self.ball.vz = self.rnd(90.0, 240.0) * power;
        self.out.push(Out::Tap { hit: Hit::Ball, soft: false });
    }

    fn faces(&self, d: f64) -> bool {
        (self.flip - if d < 0.0 { 1.0 } else { -1.0 }).abs() < 0.3
    }

    fn strike(&mut self, act: Act, now: f64) {
        self.hunt.act = Some(act);
        self.hunt.t0 = now;
        self.hunt.hit = false;
        self.hunt.from = (self.x, self.y);
    }

    /// After the ball: it pounces on it, pins it, bats it away; a step
    /// back when it is too close; it watches it dangle in a hand.
    fn ball_think(&mut self, now: f64, dt: f64) {
        if !self.ball.on {
            self.set_mode(Mode::WalkHome, now + 300.0);
            self.mind.next_roam = now + 30000.0;
            return;
        }
        let me = (self.x, self.y);
        let ball = (self.ball.x, self.ball.y);
        match self.hunt.act {
            Some(Act::Jump) => {
                let t = (now - self.hunt.t0) / self.anim_ms("jump");
                let e = ease((t - 0.27) / 0.53);
                let (f, to) = (self.hunt.from, self.hunt.to);
                self.x = f.0 + (to.0 - f.0) * e;
                self.y = f.1 + (to.1 - f.1) * e;
                if !self.hunt.hit && t > 0.8 {
                    self.hunt.hit = true;
                    let paws = (self.x + LAND.0 * self.dir, self.y - LAND.1);
                    if !self.ball.held && self.ball.z < 34.0 && dist(ball, paws) < 56.0 {
                        // pinned under its front paws
                        self.hunt.act = Some(Act::Hold);
                        self.hunt.rest = now + self.rnd(700.0, 1300.0);
                        self.mind.bored += 0.08;
                        self.mind.ear = now;
                        return;
                    }
                    self.hunt.rest = now + 250.0;
                }
                if t >= 0.9 {
                    self.hunt.act = None;
                    self.go("idle", 140.0, now);
                }
                return;
            }
            Some(Act::Hold) => {
                self.ball.x = self.x + (LAND.0 + 6.0) * self.dir;
                self.ball.y = self.y - LAND.1 + 4.0;
                (self.ball.z, self.ball.vx, self.ball.vy, self.ball.vz) = (0.0, 0.0, 0.0, 0.0);
                self.go("idle", 140.0, now);
                if now > self.hunt.rest {
                    self.hunt.act = None;
                    self.hunt.rest = now + 150.0;
                }
                return;
            }
            Some(Act::Slam) => {
                let t = (now - self.hunt.t0) / self.anim_ms("scratch");
                if t < 0.45 {
                    // it shuffles so the paw comes down on the ball
                    let step = 160.0 * dt / 1000.0;
                    let (wx, wy) = (ball.0 - self.dir * SLAM.0 - self.x, ball.1 + SLAM.1 - self.y);
                    let f = self.hunt.from;
                    self.x = self.clamp_x((self.x + wx.clamp(-step, step)).clamp(f.0 - 36.0, f.0 + 36.0));
                    self.y = self.clamp_y((self.y + wy.clamp(-step, step)).clamp(f.1 - 14.0, f.1 + 14.0));
                }
                if !self.hunt.hit && t > 0.47 {
                    self.hunt.hit = true;
                    let paw = (self.x + SLAM.0 * self.dir, self.y - SLAM.1);
                    if !self.ball.held && self.ball.z < 30.0 && dist(ball, paw) < 48.0 {
                        self.kick_ball(1.0);
                        self.ball.touched = now;
                        self.mind.bored += self.rnd(0.1, 0.17);
                    }
                }
                if t >= 0.72 {
                    self.hunt.act = None;
                    self.go("idle", 160.0, now);
                    self.hunt.rest = self.hunt.rest.max(now + 200.0);
                }
                return;
            }
            None => {}
        }
        if now < self.hunt.rest {
            self.go("idle", 140.0, now);
            return;
        }
        let dx = ball.0 - self.x;
        // a turn only when the ball is clearly on the other side: no jitter
        let dir = if dx.abs() > 30.0 { dx.signum() } else { self.dir };
        if self.ball.held {
            // dangled: it watches, and rears up for it when it is close above
            self.dir = dir;
            let c = self.ball.top();
            let rx = (c.0 - self.x) * dir;
            let near = rx > -20.0 && rx < 140.0 && c.1 < self.y - 80.0 && c.1 > self.y - 260.0;
            self.go(if near { "beg" } else { "idle" }, 220.0, now);
            return;
        }
        let slow = self.ball.speed() < 40.0 && self.ball.z < 2.0;
        if self.mind.bored > 0.85 && slow {
            self.sit_down(240.0, now);
            self.mind.quiet = now;
            return;
        }
        // in front and in paw range: it bats it from where it stands
        let (rx, ry) = ((ball.0 - self.x) * dir, ball.1 - (self.y - SLAM.1));
        if rx > 74.0 && rx < 138.0 && ry.abs() < 32.0 && self.ball.z < 30.0 && self.ball.speed() < 260.0 {
            self.dir = dir;
            self.go("idle", 120.0, now);
            if self.faces(dir) {
                self.strike(Act::Slam, now);
                self.go("scratch", 70.0, now);
            }
            return;
        }
        // too close: a few steps back
        if slow && rx > -10.0 && rx <= 74.0 && ry.abs() < 50.0 && self.faces(dir) && dir == self.dir {
            self.go_back("walk", 140.0, now);
            self.x = self.clamp_x(self.x - dir * BACK * dt / 1000.0);
            return;
        }
        // a rolling ball: a pounce to where it will be when the paws come down
        let area = self.ball_area();
        let p = self.ball.at(self.anim_ms("jump") * 0.8, area);
        let pdir = if (p.0 - self.x).abs() > 30.0 { (p.0 - self.x).signum() } else { self.dir };
        let land = (self.clamp_x(p.0 - pdir * LAND.0), self.clamp_y(p.1 + LAND.1));
        let dj = dist(land, me);
        if !slow && dj > 90.0 && dj < 300.0 && now - self.hunt.last_jump > 900.0 && self.ball.z < 60.0 {
            self.dir = pdir;
            self.go("idle", 100.0, now);
            if self.faces(pdir) {
                self.strike(Act::Jump, now);
                self.hunt.to = land;
                self.hunt.last_jump = now;
                self.go("jump", 60.0, now);
            }
            return;
        }
        let q = self.ball.at(300.0, area);
        let qdir = if (q.0 - self.x).abs() > 30.0 { (q.0 - self.x).signum() } else { self.dir };
        let to = (self.clamp_x(q.0 - qdir * SLAM.0), self.clamp_y(q.1 + SLAM.1));
        self.walk_to(to, dt, dist(to, me) > 110.0, now);
    }

    /// The ball rolls and flies; what was let go of above the bottom falls.
    fn things(&mut self, now: f64, dt: f64) {
        if self.ball.on && self.ball.gone > 0.0 && now - self.ball.gone > 220.0 {
            self.ball.on = false;
            self.ball.gone = 0.0;
        }
        if self.ball.on && !self.ball.held && self.hunt.act != Some(Act::Hold) {
            let area = self.ball_area();
            if self.ball.step(dt, area).is_some_and(|v| v > 300.0) {
                self.out.push(Out::Tap { hit: Hit::Ball, soft: true });
            }
            // left alone a long while, it fades away
            if self.mode != Mode::Ball && now - self.ball.touched > 45000.0 {
                self.ball.alpha -= dt / 1000.0;
                if self.ball.alpha <= 0.0 {
                    self.ball.on = false;
                }
            }
        }
        let floor = self.ground.3;
        let (bed, bowl) = (self.held == Some(Held::Bed), self.held == Some(Held::Bowl));
        if Self::fall(&mut self.bed, bed, floor - 18.0, dt) {
            self.out.push(Out::Tap { hit: Hit::Bed, soft: false });
        }
        if Self::fall(&mut self.bowl, bowl, floor - 20.0, dt) {
            self.out.push(Out::Tap { hit: Hit::Bowl, soft: false });
        }
    }

    /// One step of a fall to `floor`, with a little bounce: true on a thud.
    fn fall(t: &mut Thing, held: bool, floor: f64, dt: f64) -> bool {
        let Some((x, y)) = t.at else { return false };
        if held || (y >= floor && t.vy <= 0.0) {
            if !held && y > floor {
                t.at = Some((x, floor));
            }
            t.vy = 0.0;
            return false;
        }
        let s = dt / 1000.0;
        t.vy += DROP * s;
        let y = y + t.vy * s;
        if y < floor {
            t.at = Some((x, y));
            return false;
        }
        t.at = Some((x, floor));
        let hard = t.vy > 320.0;
        if hard {
            t.vy = -t.vy * 0.22;
            t.at = Some((x, floor - 0.01));
        } else {
            t.vy = 0.0;
        }
        hard
    }

    // ---------------------------------------------------------------- flight

    /// The edge it may leave by: the side nearest its bed (or the bottom,
    /// when that is much nearer); every other edge is a wall.
    fn exit_edge(&self) -> Edge {
        let (b, g) = (self.bed_at(), self.ground);
        let sd = (b.0 - g.0).min(g.2 - b.0);
        if sd < 160.0 || sd < (self.h - b.1) * 2.0 {
            if b.0 < (g.0 + g.2) / 2.0 {
                Edge::Left
            } else {
                Edge::Right
            }
        } else {
            Edge::Bottom
        }
    }

    /// Where it goes out of sight through an edge (`out`), or peeks back from.
    fn edge_point(&self, e: Edge, out: bool) -> (f64, f64) {
        let (b, g) = (self.bed_at(), self.ground);
        match e {
            Edge::Left => (g.0 - if out { 130.0 } else { 32.0 }, b.1 - 6.0),
            Edge::Right => (g.2 + if out { 130.0 } else { 32.0 }, b.1 - 6.0),
            Edge::Bottom => (b.0, self.h + if out { 230.0 } else { 80.0 }),
        }
    }

    // ---------------------------------------------------------------- the pose

    /// Live one frame: think, move the icons, pose the body.
    pub fn tick(&mut self, now: f64) -> Scene {
        let dt = if self.last == 0.0 { 16.0 } else { (now - self.last).clamp(0.0, 50.0) };
        self.last = now;
        self.think(now, dt);
        self.physics(dt);
        self.things(now, dt);
        let mut scene = if self.mode == Mode::Away { Scene::default() } else { self.pose(now, dt) };
        self.hold(now);
        scene.icons = self.icons.iter().filter(|i| i.lifted).map(|i| Moving { id: i.id, x: i.x, y: i.y, rot: i.rot, label: i.label, front: i.hold == Hold::Mouth }).collect();
        if self.lives || self.mode != Mode::Away {
            scene.bed = Some(self.bed_at());
            let (x, y) = self.bowl_at();
            let (level, plugged) = self.battery.unwrap_or((0.8, false));
            // in front of the cat: the inside under its head, the front wall over its chin
            let front = self.mode != Mode::Away && y > self.y;
            scene.bowl = Some(BowlView { x, y, level, charging: plugged, low: level < LOW && !plugged, battery: self.battery.is_some(), front });
        }
        // the shadow stays on the ground through a hop
        scene.feet = scene.feet.map(|_| (self.x, self.y));
        if self.ball.on {
            let b = &self.ball;
            let scale = if b.gone > 0.0 { (1.0 - (now - b.gone) / 220.0).max(0.0) } else { 1.0 };
            scene.ball = Some(BallView { x: b.x, y: b.y, z: b.z, rot: b.rot, alpha: b.alpha.clamp(0.0, 1.0), scale });
        }
        scene
    }

    fn to_world(&self, p: (f64, f64), o: (f64, f64)) -> (f64, f64) {
        ((p.0 - o.0) / S, (o.1 - p.1) / S)
    }

    fn pose(&mut self, now: f64, dt: f64) -> Scene {
        let hop = if matches!(self.mode, Mode::Sit | Mode::Lie) { 5.0 } else { 11.0 };
        let o = (self.x, self.y - self.hop * hop);
        let fl = if self.flip < 0.0 { -1.0 } else { 1.0 };
        let rig = &self.rig;
        let local = |c: &Clip| rig.local(c.name, Self::clip_t(c, now));
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
        // where it looks: its bowl, up at people, the icon it works on, the
        // ball (it has eyes for nothing else), a glance, the pointer when
        // it cares, or ahead
        let icon = |k: Option<usize>| k.and_then(|k| self.icons.get(k)).map(|i| (i.x, i.y));
        let focus = match self.mode {
            Mode::Eat => {
                let b = self.bowl_at();
                Some((b.0, b.1 - BOWL_H))
            }
            Mode::Beg => Some(if self.ptr.on { (self.ptr.x, self.ptr.y) } else { (self.x + self.dir * 60.0, self.y - 400.0) }),
            Mode::Tidy => match self.tidy.step {
                Some(Step::Deal) => icon(self.tidy.batch.iter().copied().find(|&k| self.icons[k].fly.is_some()).or(self.tidy.batch.last().copied())),
                _ => icon(self.tidy.next),
            },
            Mode::Lie | Mode::Pet => icon(self.sw.at),
            _ => icon(self.sw.at).or(self.ball.on.then(|| self.ball.top())),
        };
        let mut tgt = None;
        if let Some(f) = focus {
            let g = self.to_world(f, o);
            tgt = Some((g.0 * fl, g.1));
        } else if let Some((gx, gy, _)) = self.mind.glance {
            let g = self.to_world((gx, gy), o);
            tgt = Some((g.0 * fl, g.1));
        } else if self.ptr.on && (((self.mind.interest > 0.08 || now - self.ptr.moved < 1500.0) && !matches!(self.mode, Mode::Lie | Mode::Pet)) || self.sw.phase.is_some() || self.mode == Mode::Hello) {
            tgt = Some(p);
        }
        let (mut la, mut lx, mut ly) = (0.0, 0.0, 0.0);
        if let (Some(t), Some(head)) = (tgt, head) {
            let (vx, vy) = (t.0 - head.x, t.1 - head.y);
            let d = vx.hypot(vy).max(1.0);
            let elev = vy.atan2(vx.abs() + 300.0);
            la = (-elev / D2R * 0.85).clamp(-26.0, 26.0);
            if self.mind.interest < 0.15 && self.mind.glance.is_none() && self.sw.phase.is_none() && focus.is_none() && self.mode != Mode::Hello {
                la *= 0.5;
            }
            lx = vx / d * 10.0;
            ly = vy / d * 7.0;
        }
        // with the heap in its mouth: the neck a little lower, the tail up
        let pushing = self.mode == Mode::Tidy && self.tidy.pile.iter().any(|&k| self.icons.get(k).is_some_and(|i| i.hold == Hold::Mouth));
        let dip = (now - self.tidy.dip) / 320.0;
        let nt = (now - self.nudge_t0) / 260.0;
        let flick = if (0.0..1.0).contains(&nt) { (nt * PI).sin() } else { 0.0 };
        // head down to the desk and up again: it picks up or lets go
        if self.mode == Mode::Tidy && (0.0..1.0).contains(&dip) {
            la = 24.0 * (dip * PI).sin();
        }
        // eating: the head down in the bowl, chewing
        let eating = self.mode == Mode::Eat && now - self.mode_t0 > 300.0;
        self.feed.k += (if eating { 1.0 } else { 0.0 } - self.feed.k) * (dt / 180.0).min(1.0);
        let fk = self.feed.k;
        if fk > 0.001 {
            la = la * (1.0 - fk) + (EAT_LOOK + (now / 85.0).sin() * EAT_CHEW) * fk;
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
        let swish = if (0.0..1.0).contains(&swish_t) { (swish_t * PI * 3.0).sin() * 14.0 * (1.0 - swish_t) } else { 0.0 };
        let tail = tail_base + swish + if crouch > 0.0 { (now / 60.0).sin() * 6.0 * crouch } else { 0.0 } + if pushing { 10.0 } else { 0.0 };
        let look_a = self.look.0;
        let ids = (self.ids.head, self.ids.neck, self.ids.body, self.ids.tail);
        let fix0 = move |n: usize, s: St| -> St {
            if n == ids.0 {
                St { a: s.a + look_a + 4.0 * scare + 1.5 * tr - 5.0 * crouch + 16.0 * flick, ..s }
            } else if n == ids.1 {
                St { a: s.a - 6.0 * scare + 3.0 * crouch + if pushing { 6.0 } else { 0.0 } + EAT_NECK * fk + 6.0 * flick, y: s.y + breath * 0.5, ..s }
            } else if n == ids.2 {
                St { a: s.a - 5.0 * scare - 2.0 * crouch + EAT_BODY * fk, x: s.x + 24.0 * scare + 2.0 * tr + 7.0 * crouch, y: s.y + 10.0 * scare + breath - 4.0 * crouch + EAT_DROP * fk, ..s }
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
        // claws out as the paw comes down on the pointer
        if self.mode == Mode::Ball && self.hunt.act == Some(Act::Slam) {
            let t = (now - self.hunt.t0) / self.anim_ms("scratch");
            self.sw.claws = self.sw.claws.max(ease((t - 0.38) / 0.08) * (1.0 - ease((t - 0.64) / 0.06)));
        }
        if let Some(t) = tt.filter(|_| self.sw.w > 0.01) {
            wd = self.reach_for(&l, &fix0, t, (s0, e, wr), (l1, l2), paw_tilt);
        }
        // the paw meets an icon during the strike
        if let Some(pw) = wd.bone(self.ids.paw) {
            let paw = (o.0 + pw.x * S * self.flip, o.1 - pw.y * S);
            if self.sw.phase == Some(Phase::Strike) && !self.sw.hit && self.sw.rake {
                self.rake_in(now);
            } else if self.sw.phase == Some(Phase::Strike) && !self.sw.hit {
                if let Some(last) = self.last_paw {
                    let swing = (paw.0 - last.0, paw.1 - last.1);
                    let r = self.cell.0.min(self.cell.1) * 0.85;
                    let hit = match self.sw.at {
                        Some(k) if self.icons.get(k).is_some_and(|i| dist((i.x, i.y), paw) < r * 1.25) => Some(k),
                        _ if self.sw.rake => None,
                        _ => (0..self.icons.len()).find(|&k| !self.icons[k].held() && self.icons[k].fly.is_none() && dist((self.icons[k].x, self.icons[k].y), paw) < r),
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
        let ear_flick = if (0.0..1.0).contains(&et) { (et * PI).sin() * 18.0 } else { 0.0 };
        let side = self.mind.ear_side;
        let head_c = self.head_centre(&wd);
        for k in 0..wd.objs.len() {
            let (n, s) = (wd.objs[k].name, wd.objs[k].st);
            if Some(n) == self.ids.ear_top {
                wd.objs[k].st = self.ear_turn(s, head_c, |e| e.a += ear_flick * side - 6.0 * crouch);
            }
            if Some(n) == self.ids.ear_back {
                wd.objs[k].st = self.ear_turn(s, head_c, |e| e.a += ear_flick * 0.5 * side);
            }
        }
        // a yawn: eyes shut, mouth wide
        let yt = (now - self.mind.yawn) / 1400.0;
        if (0.0..1.0).contains(&yt) && self.mode == Mode::Sit {
            let k = (yt * PI).sin();
            for ob in &mut wd.objs {
                if Some(ob.name) == self.ids.mouth {
                    ob.st.al = 1.0;
                    ob.st.sy *= 0.3 + 0.9 * k;
                    ob.st.sx *= 0.5 + 0.3 * k;
                }
            }
            // eyes shut through the middle of it: the closed-eye picture, and the pupils hidden
            if k > 0.35 && self.closed.is_some() {
                self.set_eyes(&mut wd, self.closed);
                for ob in &mut wd.objs {
                    if self.ids.pupils.contains(&ob.name) {
                        ob.st.al = 0.0;
                    }
                }
            }
        }
        // the pupils open wide when it hunts
        let hunting = matches!(self.mode, Mode::Sit | Mode::Ball | Mode::Tidy);
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

    /// The middle of the head's picture.
    fn head_centre(&self, wd: &World) -> Option<(f64, f64)> {
        wd.objs.iter().find(|o| self.file_name(o.st.f).contains("/head")).and_then(|o| self.centre(&o.st))
    }

    /// An ear turns and folds about its base on the head, so it stays on.
    fn ear_turn(&self, s: St, head: Option<(f64, f64)>, f: impl FnOnce(&mut St)) -> St {
        let mut e = s;
        let (Some(c), Some(h)) = (self.centre(&s), head) else {
            f(&mut e);
            return e;
        };
        let p = ((c.0 + h.0) / 2.0, (c.1 + h.1) / 2.0);
        let (sn, cs) = (-s.a * D2R).sin_cos();
        let (dx, dy) = (p.0 - s.x, p.1 - s.y);
        let l = ((dx * cs - dy * sn) / s.sx, (dx * sn + dy * cs) / s.sy);
        f(&mut e);
        let (sn, cs) = (e.a * D2R).sin_cos();
        let (lx, ly) = (l.0 * e.sx, l.1 * e.sy);
        let q = (e.x + lx * cs - ly * sn, e.y + lx * sn + ly * cs);
        e.x += p.0 - q.0;
        e.y += p.1 - q.1;
        e
    }

    /// The scare on the parts: ears flat, wide eyes, mouth open in a hiss, the tail puffed.
    fn scared(&self, wd: &mut World, k: f64, wob: f64) {
        let mut eyes = None;
        let head_c = self.head_centre(wd);
        for i in 0..wd.objs.len() {
            let n = wd.objs[i].name;
            if Some(n) == self.ids.ear_top {
                wd.objs[i].st = self.ear_turn(wd.objs[i].st, head_c, |e| {
                    e.a += -35.0 * k;
                    e.sy *= 1.0 - 0.25 * k;
                });
            }
            if Some(n) == self.ids.ear_back {
                wd.objs[i].st = self.ear_turn(wd.objs[i].st, head_c, |e| e.sy *= 1.0 - 0.35 * k);
            }
            let s = &mut wd.objs[i].st;
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
            if fl.abs() > 0.95 && self.mode != Mode::Tidy && self.feed.k < 0.05 && self.cur.name != "beg" {
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
        Scene { parts, feet: Some(org), ..Scene::default() }
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

    /// In its corner, sitting; returns the time.
    fn settled() -> (Live, f64) {
        let mut l = live();
        l.set_lives(true, 0.0);
        let t = run(&mut l, 0.0, 6000.0);
        (l, t)
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

    /// Runs it, keeping what it said.
    fn run_out(l: &mut Live, t: f64, ms: f64, out: &mut Vec<Out>) -> f64 {
        let mut t = t;
        let end = t + ms;
        while t < end {
            t += 16.7;
            let _ = l.tick(t);
            out.extend(l.take());
        }
        t
    }

    /// A click at `at`; returns the time after.
    fn click(l: &mut Live, t: f64, at: (f64, f64)) -> f64 {
        l.pointer(Some(at), true, t);
        let t = run(l, t, 50.0);
        l.pointer(Some(at), false, t);
        run(l, t, 100.0)
    }

    fn middle(l: &Live) -> (f64, f64) {
        let (x0, y0, x1, y1) = l.bounds.unwrap();
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    }

    /// Clicks it three times where it is; returns the time after.
    fn three_clicks(l: &mut Live, t: f64) -> f64 {
        let at = middle(l);
        let mut t = t;
        for _ in 0..3 {
            t = click(l, t, at);
        }
        t
    }

    fn voices(out: &[Out], k: Kind) -> usize {
        out.iter().filter(|o| **o == Out::Voice(k)).count()
    }

    #[test]
    fn it_walks_in_to_its_corner_and_sits() {
        let (mut l, t) = settled();
        assert_eq!(l.mode, Mode::Sit, "{:?}", l.mode);
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        let s = l.tick(t + 16.0);
        assert!(s.parts.len() > 20, "{} parts", s.parts.len());
        assert!(s.bed.is_some() && s.bowl.is_some());
        let (x0, y0, x1, y1) = l.bounds.unwrap();
        assert!(x1 - x0 > 60.0 && y1 - y0 > 90.0, "{:?}", l.bounds);
    }

    #[test]
    fn a_slap_sends_it_off_the_screen_it_peeks_back_and_comes_home() {
        let (mut l, mut t) = settled();
        l.pet(true, t);
        assert_eq!(l.mode, Mode::Pet);
        t = run(&mut l, t, 1000.0);
        l.slap(t);
        assert_eq!(l.mode, Mode::Scare);
        t = run(&mut l, t, 300.0);
        assert!(l.scare > 0.5, "scare {}", l.scare);
        let mut seen = Vec::new();
        let end = t + 20000.0;
        while t < end {
            t = run(&mut l, t, 50.0);
            if seen.last() != Some(&l.mode) {
                seen.push(l.mode);
            }
            if l.mode == Mode::Sit {
                break;
            }
        }
        for m in [Mode::Flee, Mode::Gone, Mode::Peek, Mode::WalkHome] {
            assert!(seen.contains(&m), "{m:?} missing from {seen:?}");
        }
        assert_eq!(l.mode, Mode::Sit, "{seen:?}");
        assert!(dist((l.x, l.y), l.home()) < 1.0);
    }

    #[test]
    fn three_clicks_bring_a_ball_it_pounces_on_and_bats_and_three_put_it_away() {
        let (mut l, mut t) = settled();
        t = three_clicks(&mut l, t);
        assert_eq!(l.mode, Mode::Ball);
        assert!(l.ball.on);
        assert!(l.take().contains(&Out::Chirp));
        let (mut slams, mut jumps, mut last) = (0, 0, None);
        let end = t + 12000.0;
        while t < end {
            t = run(&mut l, t, 16.7);
            if l.hunt.act != last {
                match l.hunt.act {
                    Some(Act::Slam) => slams += 1,
                    Some(Act::Jump) => jumps += 1,
                    _ => {}
                }
            }
            last = l.hunt.act;
        }
        assert!(slams >= 2, "{slams} slams, {jumps} jumps");
        assert!(jumps >= 1, "{slams} slams, {jumps} jumps");
        // three clicks on the ball put it away (a click does not lift it); it goes home
        while l.ball.speed() > 1.0 || l.ball.z > 0.0 || l.hunt.act.is_some() {
            t = run(&mut l, t, 16.7);
        }
        let at = l.ball.top();
        for _ in 0..3 {
            t = click(&mut l, t, at);
        }
        assert!(l.ball.gone > 0.0);
        l.mind.next_roam = f64::INFINITY;
        t = run(&mut l, t, 12000.0);
        assert!(!l.ball.on);
        assert!(matches!(l.mode, Mode::Sit | Mode::SitDown | Mode::Wash | Mode::Lie), "{:?}", l.mode);
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        assert!(l.tick(t + 16.0).ball.is_none());
    }

    #[test]
    fn a_ball_thrown_flies_with_the_hand() {
        let (mut l, mut t) = settled();
        t = three_clicks(&mut l, t);
        t = run(&mut l, t, 1500.0);
        let (x, y) = l.ball.top();
        l.pointer(Some((x, y)), true, t);
        assert!(l.held == Some(Held::Ball));
        for k in 1..8 {
            t += 16.7;
            l.pointer(Some((x - k as f64 * 14.0, y - k as f64 * 6.0)), true, t);
            let _ = l.tick(t);
        }
        l.pointer(Some((x - 120.0, y - 50.0)), false, t + 5.0);
        assert!(!l.ball.held);
        assert!(l.ball.vx < -300.0 && l.ball.vz > 100.0, "{:?}", (l.ball.vx, l.ball.vz));
    }

    #[test]
    fn its_bed_lifted_and_let_go_falls_to_the_bottom_and_it_goes_there() {
        let (mut l, mut t) = settled();
        let floor = l.ground.3 - 18.0;
        let b0 = l.bed_at();
        let at = (b0.0 - 74.0, b0.1 + 8.0);
        l.pointer(Some(at), true, t);
        assert_eq!(l.held, Some(Held::Bed));
        for k in 1..=30 {
            t += 16.7;
            l.pointer(Some((at.0 - k as f64 * 20.0, at.1 - k as f64 * 12.0)), true, t);
            let _ = l.tick(t);
        }
        assert!(l.bed_at().1 < floor - 300.0, "{:?}", l.bed_at());
        l.pointer(Some((at.0 - 600.0, at.1 - 360.0)), false, t);
        let mut out = Vec::new();
        t = run_out(&mut l, t, 2000.0, &mut out);
        let b = l.bed_at();
        assert!((b.1 - floor).abs() < 0.5, "{b:?}");
        assert!((b.0 - (b0.0 - 600.0)).abs() < 1.0, "{b:?}");
        assert!(out.contains(&Out::Tap { hit: Hit::Bed, soft: false }), "no thud");
        assert!(out.iter().any(|o| matches!(o, Out::Moved { .. })));
        l.mind.next_roam = f64::INFINITY;
        run(&mut l, t, 15000.0);
        assert!(matches!(l.mode, Mode::Sit | Mode::SitDown), "{:?}", l.mode);
        assert!(dist((l.x, l.y), l.home()) < 1.0);
    }

    #[test]
    fn on_the_charger_it_eats_once_with_its_sounds() {
        let (mut l, mut t) = settled();
        l.power(50, true);
        l.eat(t);
        let mut out = Vec::new();
        let (mut ate, mut deep) = (false, 0.0f64);
        let end = t + 30000.0;
        while t < end {
            t = run_out(&mut l, t, 50.0, &mut out);
            ate |= l.mode == Mode::Eat;
            deep = deep.max(l.feed.k);
        }
        assert!(ate);
        assert!(deep > 0.9, "head down {deep}");
        let n = voices(&out, Kind::Eat);
        assert!((2..=3).contains(&n), "{n} eating sounds");
        assert!(!l.want_eat);
        assert_ne!(l.mode, Mode::ToBowl);
    }

    #[test]
    fn on_a_low_battery_it_begs_by_its_bowl_until_fed() {
        let (mut l, mut t) = settled();
        l.power(6, false);
        l.beg(t);
        let mut out = Vec::new();
        let mut begged = false;
        let end = t + 25000.0;
        while t < end {
            t = run_out(&mut l, t, 50.0, &mut out);
            begged |= l.mode == Mode::Beg && l.feed.step == BegStep::Up;
        }
        assert!(begged);
        let n = voices(&out, Kind::Hungry);
        assert!((2..=5).contains(&n), "{n} meows");
        let b = l.bowl_at();
        assert!((l.x - b.0).abs() < 140.0 && (l.y - b.1).abs() < 10.0, "{:?} by {b:?}", (l.x, l.y));
        l.power(6, true);
        run(&mut l, t, 2000.0);
        assert!(!matches!(l.mode, Mode::Beg | Mode::ToBowl), "{:?}", l.mode);
    }

    #[test]
    fn it_sleeps_when_the_lid_closes_and_says_hello_once_up_after_it_opens() {
        let (mut l, mut t) = settled();
        l.doze(true, t);
        assert_eq!(l.mode, Mode::Lie);
        t = run(&mut l, t, 60000.0);
        assert_eq!(l.mode, Mode::Lie);
        assert!(l.fps() <= 4.0);
        l.doze(false, t);
        t = run(&mut l, t, 1000.0);
        l.hello(t);
        let mut out = Vec::new();
        t = run_out(&mut l, t, 1500.0, &mut out);
        assert_eq!(l.mode, Mode::Lie, "up too soon");
        assert_eq!(voices(&out, Kind::Hello), 0);
        run_out(&mut l, t, 3500.0, &mut out);
        assert_eq!(voices(&out, Kind::Hello), 1);
        assert!(!matches!(l.mode, Mode::Lie | Mode::WakeUp), "{:?}", l.mode);
    }

    #[test]
    fn asleep_before_it_came_it_lies_down_once_home() {
        let mut l = live();
        l.doze(true, 0.0);
        l.set_lives(true, 0.0);
        let t = run(&mut l, 0.0, 8000.0);
        assert_eq!(l.mode, Mode::Lie);
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        l.doze(false, t);
        run(&mut l, t, 5000.0);
        assert!(!matches!(l.mode, Mode::Lie | Mode::WakeUp), "{:?}", l.mode);
    }

    #[test]
    fn a_new_screen_size_keeps_it_on_its_bed() {
        let (mut l, t) = settled();
        l.lie_down(Mode::Lie, t);
        l.set_area(1496.0, 967.0, (0.0, 25.0, 1496.0, 890.0));
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        l.set_area(2056.0, 1329.0, (0.0, 25.0, 2056.0, 1250.0));
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        assert!(l.home().0 > 1800.0 && l.home().1 > 1200.0, "{:?}", l.home());
        // a bed people moved stands on the new bottom too, the cat on it
        l.set_places(Some(900.0), None);
        let t = run(&mut l, t, 100.0);
        (l.x, l.y) = l.home();
        l.set_area(1496.0, 967.0, (0.0, 25.0, 1496.0, 890.0));
        assert!((l.bed_at().1 - (890.0 - 18.0)).abs() < 0.5, "{:?}", l.bed_at());
        assert!(dist((l.x, l.y), l.home()) < 1.0);
        l.set_area(2056.0, 1329.0, (0.0, 25.0, 2056.0, 1250.0));
        run(&mut l, t, 500.0);
        assert!((l.bed_at().1 - (1250.0 - 18.0)).abs() < 0.5, "{:?}", l.bed_at());
        assert!(dist((l.x, l.y), l.home()) < 1.0, "{:?} home {:?}", (l.x, l.y), l.home());
    }

    #[test]
    fn a_click_on_a_cat_asleep_with_the_mac_wakes_it() {
        let (mut l, mut t) = settled();
        l.doze(true, t);
        t = run(&mut l, t, 2000.0);
        let at = middle(&l);
        t = click(&mut l, t, at);
        assert!(!l.doze);
        assert_eq!(l.mode, Mode::WakeUp);
        run(&mut l, t, 2500.0);
        assert!(!matches!(l.mode, Mode::Lie | Mode::WakeUp), "{:?}", l.mode);
    }

    #[test]
    fn asleep_it_sleeps_on_until_a_click_wakes_it() {
        let (mut l, mut t) = settled();
        l.lie_down(Mode::Lie, t);
        t = run(&mut l, t, 60000.0);
        assert_eq!(l.mode, Mode::Lie);
        assert_eq!(l.cur.name, "sleeping");
        let at = middle(&l);
        t = click(&mut l, t, at);
        assert_eq!(l.mode, Mode::WakeUp);
        run(&mut l, t, 2500.0);
        assert!(!matches!(l.mode, Mode::Lie | Mode::WakeUp), "{:?}", l.mode);
    }

    #[test]
    fn it_walks_about_the_desk_on_its_own_and_sits_there() {
        let (mut l, t) = settled();
        l.pointer(None, false, t);
        l.mind.next_roam = t + 1000.0;
        let mut t = t;
        let mut roamed = false;
        let end = t + 40000.0;
        while t < end {
            t = run(&mut l, t, 100.0);
            roamed |= l.mode == Mode::Roam;
            if roamed && l.mode == Mode::Sit {
                break;
            }
        }
        assert!(roamed, "never roamed");
        assert_eq!(l.mode, Mode::Sit);
        assert!(dist((l.x, l.y), l.roam_to) < 1.0);
    }

    #[test]
    fn the_tidy_lines_every_icon_up_in_its_slot() {
        let (mut l, mut t) = settled();
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
    fn it_carries_five_or_more_at_a_time_in_its_mouth() {
        let (mut l, mut t) = settled();
        let icons: Vec<(usize, f64, f64)> = (0..18).map(|k| (k + 1, 200.0 + (k % 6) as f64 * 150.0, 140.0 + (k / 6) as f64 * 180.0)).collect();
        let slots: Vec<(f64, f64)> = (0..18).map(|r| (1440.0 - (r / 8) as f64 * 100.0, 60.0 + (r % 8) as f64 * 92.0)).collect();
        l.tidy(icons, &slots, (47.0, 37.0), t);
        let t0 = t;
        let mut most = 0;
        while l.tidying() && t - t0 < 60_000.0 {
            t += 16.7;
            let _ = l.tick(t);
            let _ = l.take();
            most = most.max(l.icons.iter().filter(|i| i.hold == Hold::Mouth).count());
        }
        assert!(!l.tidying(), "never finished; step {:?}", l.tidy.step);
        assert!(most >= 5, "at most {most} in the mouth");
        assert!(t - t0 < 30_000.0, "18 icons took {} s", (t - t0) / 1000.0);
        assert!(l.icons.iter().all(|i| i.home && i.slot.is_some()));
    }

    /// Random desks (some icons already in Finder's grid at a tight pitch)
    /// and random slaps, one or two: every icon ends in its own slot and
    /// Finder is told so, in good time.
    #[test]
    fn random_desks_and_slaps_always_end_tidy() {
        let mut bad = 0;
        for seed in 1..60u64 {
            let mut r = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut rnd = move || { r ^= r << 13; r ^= r >> 7; r ^= r << 17; (r % 10_000) as f64 / 10_000.0 };
            let (mut l, mut t) = settled();
            let slots: Vec<(f64, f64)> = (0..40).map(|k| (1450.0 - (k / 9) as f64 * 88.0, 70.0 + (k % 9) as f64 * 92.0)).collect();
            let mut icons = Vec::new();
            let on_grid = (rnd() * 12.0) as usize;
            for &(x, y) in &slots[..on_grid] { icons.push((icons.len() + 1, x, y)); }
            let loose = 6 + (rnd() * 22.0) as usize;
            for _ in 0..loose { icons.push((icons.len() + 1, 60.0 + rnd() * 1250.0, 60.0 + rnd() * 780.0)); }
            let n = icons.len();
            let start: HashMap<usize, (f64, f64)> = icons.iter().map(|&(id, x, y)| (id, (x, y))).collect();
            l.tidy(icons, &slots, (47.0, 37.0), t);
            let t0 = t;
            let slaps = [if seed % 4 == 0 { f64::INFINITY } else { rnd() * 15_000.0 }, if rnd() < 0.5 { 5_000.0 + rnd() * 20_000.0 } else { f64::INFINITY }];
            let mut done = [false, false];
            let mut finder: HashMap<usize, (f64, f64)> = HashMap::new();
            let mut rests: HashMap<usize, u32> = HashMap::new();
            while (l.tidying() || done.iter().zip(slaps).any(|(d, s)| !d && s.is_finite())) && t - t0 < 300_000.0 {
                t += 16.7;
                for j in 0..2 { if !done[j] && t - t0 > slaps[j] { done[j] = true; l.slap(t); } }
                let _ = l.tick(t);
                for o in l.take() { if let Out::Rest { id, x, y } = o { finder.insert(id, (x, y)); *rests.entry(id).or_insert(0) += 1; } }
            }
            let took = (t - t0) / 1000.0;
            let mut msg = Vec::new();
            if l.tidying() { msg.push(format!("stuck step {:?}", l.tidy.step)); }
            for i in &l.icons {
                let Some(s) = i.slot else { msg.push(format!("{} no slot", i.id)); continue };
                if !i.home || dist((i.x, i.y), s) > 1.0 { msg.push(format!("{} not home", i.id)); }
                let f = finder.get(&i.id).copied().unwrap_or(start[&i.id]);
                if dist(f, s) > 1.0 { msg.push(format!("{} finder {:?} slot {:?}", i.id, f, s)); }
                // Finder is asked a few times at most, not every frame
                if rests.get(&i.id).copied().unwrap_or(0) > 8 { msg.push(format!("{} sent to Finder {} times", i.id, rests[&i.id])); }
            }
            if took > 10.0 + n as f64 * 2.5 { msg.push(format!("slow {took:.0} s for {n}")); }
            if !msg.is_empty() { bad += 1; eprintln!("seed {seed} n {n} slaps {:?} took {took:.0}: {}", slaps, msg.join("; ")); }
        }
        assert_eq!(bad, 0);
    }

    #[test]
    fn an_icon_pushed_along_is_sent_to_finder_once_it_stops_not_every_frame() {
        let (mut l, mut t) = settled();
        let icons: Vec<(usize, f64, f64)> = (0..26).map(|k| (k + 1, 160.0 + (k % 7) as f64 * 140.0, 120.0 + (k / 7) as f64 * 170.0)).collect();
        let slots: Vec<(f64, f64)> = (0..26).map(|r| (1440.0 - (r / 8) as f64 * 100.0, 60.0 + (r % 8) as f64 * 92.0)).collect();
        l.tidy(icons, &slots, (47.0, 37.0), t);
        let t0 = t;
        let mut rests: HashMap<usize, u32> = HashMap::new();
        while l.tidying() && t - t0 < 120_000.0 {
            t += 16.7;
            let _ = l.tick(t);
            for o in l.take() {
                if let Out::Rest { id, .. } = o {
                    *rests.entry(id).or_insert(0) += 1;
                }
            }
        }
        assert!(!l.tidying());
        let most = rests.values().copied().max().unwrap_or(0);
        assert!(most <= 3, "an icon was sent to Finder {most} times: {rests:?}");
    }

    #[test]
    fn switched_off_mid_tidy_it_lets_go_of_everything_and_goes_home() {
        let (mut l, mut t) = settled();
        let icons: Vec<(usize, f64, f64)> = (0..14).map(|k| (k + 1, 160.0 + (k % 7) as f64 * 140.0, 120.0 + (k / 7) as f64 * 170.0)).collect();
        let slots: Vec<(f64, f64)> = (0..14).map(|r| (1440.0 - (r / 8) as f64 * 100.0, 60.0 + (r % 8) as f64 * 92.0)).collect();
        l.tidy(icons, &slots, (47.0, 37.0), t);
        let t0 = t;
        // until it carries some in its mouth
        while !l.icons.iter().any(|i| i.hold == Hold::Mouth) && t - t0 < 30_000.0 {
            t += 16.7;
            let _ = l.tick(t);
        }
        assert!(l.icons.iter().any(|i| i.lifted), "nothing moved");
        let id = l.icons.iter().find(|i| i.lifted).unwrap().id;
        assert!(l.lifted(id));
        l.set_lives(false, t);
        l.end_tidy(t);
        assert!(!l.tidying() && !l.lifted(id));
        let _ = l.take();
        t = run_out(&mut l, t, 20_000.0, &mut Vec::new());
        assert!(l.take().iter().all(|o| !matches!(o, Out::Lift(_) | Out::Rest { .. })), "it moved icons after the switch went off");
        assert!(!l.shown(), "still on screen at {t}");
    }

    #[test]
    fn a_slap_at_any_moment_of_a_tidy_still_ends_with_every_icon_placed() {
        for slap_at in [1500.0, 3000.0, 4500.0, 6000.0, 7500.0, 9400.0, 11000.0, 13000.0] {
            let (mut l, mut t) = settled();
            let icons: Vec<(usize, f64, f64)> = (0..26).map(|k| (k + 1, 160.0 + (k % 7) as f64 * 140.0, 120.0 + (k / 7) as f64 * 170.0)).collect();
            let slots: Vec<(f64, f64)> = (0..26).map(|r| (1440.0 - (r / 8) as f64 * 100.0, 60.0 + (r % 8) as f64 * 92.0)).collect();
            l.tidy(icons, &slots, (47.0, 37.0), t);
            let t0 = t;
            let mut finder: HashMap<usize, (f64, f64)> = HashMap::new();
            let mut slapped = false;
            while l.tidying() && t - t0 < 120_000.0 {
                t += 16.7;
                if !slapped && t - t0 > slap_at {
                    slapped = true;
                    l.slap(t);
                }
                let _ = l.tick(t);
                for o in l.take() {
                    if let Out::Rest { id, x, y } = o {
                        finder.insert(id, (x, y));
                    }
                }
            }
            // a little more for the last icons to settle
            t = run_out(&mut l, t, 2000.0, &mut Vec::new());
            for o in l.take() {
                if let Out::Rest { id, x, y } = o {
                    finder.insert(id, (x, y));
                }
            }
            assert!(!l.tidying(), "slap at {slap_at}: never finished, step {:?}", l.tidy.step);
            assert!(t - t0 < 60_000.0, "slap at {slap_at}: took {} s", (t - t0) / 1000.0);
            for i in &l.icons {
                let s = i.slot.unwrap_or_else(|| panic!("slap at {slap_at}: icon {} has no slot", i.id));
                assert!(i.home && dist((i.x, i.y), s) < 1.0, "slap at {slap_at}: icon {} at {:?}, slot {:?}", i.id, (i.x, i.y), s);
                let f = finder.get(&i.id).copied().unwrap_or_else(|| panic!("slap at {slap_at}: Finder never told about icon {}", i.id));
                assert!(dist(f, s) < 1.0, "slap at {slap_at}: Finder has icon {} at {f:?}, slot {s:?}", i.id);
            }
            let mut seen: Vec<(f64, f64)> = Vec::new();
            for i in &l.icons {
                let s = i.slot.unwrap();
                assert!(!seen.iter().any(|o| dist(*o, s) < 1.0), "slap at {slap_at}: two icons in {s:?}");
                seen.push(s);
            }
        }
    }

    #[test]
    fn a_slap_mid_tidy_scares_it_and_then_it_carries_on() {
        let (mut l, mut t) = settled();
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
