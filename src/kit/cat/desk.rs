//! The desktop cat: a drawn cat at the bottom of the screen that acts out
//! each reaction (it gets scared when slapped, curls up under a hand, eats
//! at the charger, asks for food, peeks in when the lid opens). When it
//! lives on screen it sits in the bottom right corner between acts, washes
//! now and then, falls asleep when nothing happens, bats at a pointer that
//! rests on it, and carries the desktop's files into a box folder.
//!
//! Plain timing here: which picture shows in each frame and where. The
//! pictures are sprite sheets in `assets/cat/sprites` (2 pixels a point);
//! `screen` plays a script with Core Animation, so the cat's thread does
//! no work while it moves.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::Kind;

/// Frames a second the clips were drawn at.
pub const FPS: f64 = 24.0;
/// Sheet pixels a point.
pub const PX_PER_PT: f64 = 2.0;
/// The ground lies this far below each clip's origin, in sheet pixels.
const GROUND: f64 = 13.0;
/// A turn leaves the cat this far from where it stood (times its old
/// direction, negated), in points.
const TURN_SHIFT: f64 = -9.95;
/// Walking and running speeds that match the paws' stride, points a second.
const WALK: f64 = 109.2;
const RUN: f64 = 216.7;
/// Where the cat sits: this far in from the right edge of the screen.
pub const HOME_IN: f64 = 170.0;
/// The tidy box stands this far left of where the cat sits.
const BOX_LEFT: f64 = 150.0;
/// Past this distance the cat runs (with a file too).
const RUN_PAST: f64 = 300.0;
/// The size of a file's icon on the ground and in the mouth, in points.
pub const ICON_PT: f64 = 56.0;
/// A file falls from its place to the ground in this many frames.
pub const FALL_FRAMES: usize = 11;
/// The file is held in the mouth from this frame of the pick-up and let go at this frame of the drop.
const PICK_AT: u32 = 4;
const DROP_AT: u32 = 7;
/// Most files carried in one tidy.
pub const TIDY_MAX: usize = 8;
/// Peeking, the cat stands this far past the edge.
const PEEK_OUT: f64 = 50.0;
/// The bowl stands this far in front of an eating cat.
const BOWL_AHEAD: f64 = 90.0;

/// One sprite sheet: `n` frames of `w`×`h` pixels in rows of `cols`;
/// the cat's feet are at (`ox`, `oy` + GROUND) in each frame.
#[derive(Debug)]
pub struct Clip {
    pub name: &'static str,
    pub n: u32,
    pub cols: u32,
    pub w: u32,
    pub h: u32,
    pub ox: f32,
    pub oy: f32,
}

impl Clip {
    /// Where the feet are, as a share of the frame from its bottom left
    /// (a layer's anchor point).
    pub fn anchor(&self) -> (f64, f64) {
        (self.ox as f64 / self.w as f64, (self.h as f64 - self.oy as f64 - GROUND) / self.h as f64)
    }

    /// The frame's size in points.
    pub fn size_pt(&self) -> (f64, f64) {
        (self.w as f64 / PX_PER_PT, self.h as f64 / PX_PER_PT)
    }

    /// Frame `f`'s rectangle in the sheet, in pixels from the top left.
    pub fn rect(&self, f: u32) -> (u32, u32, u32, u32) {
        ((f % self.cols) * self.w, (f / self.cols) * self.h, self.w, self.h)
    }
}

/// The sheets (`<name>.webp`). Made by the design thread's sheet script;
/// the numbers come with the pictures.
pub const CLIPS: [Clip; 26] = [
    Clip { name: "stand", n: 1, cols: 1, w: 436, h: 364, ox: 249.0, oy: 360.1 },
    Clip { name: "scare_in", n: 6, cols: 6, w: 419, h: 376, ox: 232.7, oy: 371.8 },
    Clip { name: "scare", n: 12, cols: 8, w: 395, h: 378, ox: 228.2, oy: 373.8 },
    Clip { name: "scare_out", n: 10, cols: 8, w: 448, h: 375, ox: 248.3, oy: 371.2 },
    Clip { name: "turn", n: 9, cols: 8, w: 460, h: 376, ox: 241.2, oy: 367.2 },
    Clip { name: "run", n: 19, cols: 8, w: 499, h: 405, ox: 239.8, oy: 392.6 },
    Clip { name: "walk", n: 19, cols: 8, w: 456, h: 383, ox: 248.3, oy: 367.9 },
    Clip { name: "peek_in", n: 8, cols: 8, w: 473, h: 353, ox: 274.3, oy: 348.4 },
    Clip { name: "peek", n: 24, cols: 8, w: 478, h: 367, ox: 278.8, oy: 362.0 },
    Clip { name: "peek_out", n: 6, cols: 6, w: 470, h: 356, ox: 272.4, oy: 351.0 },
    Clip { name: "sleep_transition_to", n: 25, cols: 8, w: 502, h: 378, ox: 260.0, oy: 364.0 },
    Clip { name: "sleeping", n: 38, cols: 8, w: 395, h: 256, ox: 255.4, oy: 243.1 },
    Clip { name: "sleep_transition_from", n: 25, cols: 8, w: 512, h: 376, ox: 255.4, oy: 363.4 },
    Clip { name: "eat_in", n: 6, cols: 6, w: 530, h: 357, ox: 324.4, oy: 352.3 },
    Clip { name: "eat", n: 12, cols: 8, w: 543, h: 310, ox: 327.6, oy: 279.5 },
    Clip { name: "eat_out", n: 6, cols: 6, w: 525, h: 357, ox: 320.4, oy: 352.3 },
    Clip { name: "beg", n: 38, cols: 8, w: 509, h: 452, ox: 248.3, oy: 435.5 },
    Clip { name: "sit_tranistion_to", n: 13, cols: 8, w: 491, h: 374, ox: 248.3, oy: 364.0 },
    Clip { name: "sitting", n: 48, cols: 8, w: 335, h: 380, ox: 209.3, oy: 370.5 },
    Clip { name: "sit_transition_from", n: 13, cols: 8, w: 490, h: 373, ox: 248.3, oy: 364.0 },
    Clip { name: "clean", n: 48, cols: 8, w: 386, h: 376, ox: 260.0, oy: 366.6 },
    Clip { name: "scratch", n: 24, cols: 8, w: 547, h: 405, ox: 326.3, oy: 384.2 },
    Clip { name: "carry_pick", n: 12, cols: 8, w: 546, h: 371, ox: 319.2, oy: 366.6 },
    Clip { name: "carry", n: 19, cols: 8, w: 463, h: 391, ox: 236.0, oy: 375.7 },
    Clip { name: "carry_run", n: 19, cols: 8, w: 464, h: 410, ox: 230.8, oy: 397.8 },
    Clip { name: "carry_drop", n: 12, cols: 8, w: 540, h: 357, ox: 319.2, oy: 352.3 },
];

/// The tallest frame, in points: how tall the cat's strip of screen is.
pub fn tallest_pt() -> f64 {
    CLIPS.iter().map(|c| c.h).max().unwrap_or(0) as f64 / PX_PER_PT
}

fn clip(name: &str) -> usize {
    CLIPS.iter().position(|c| c.name == name).unwrap_or(0)
}

/// The carrying clips come with a second sheet (`<name>_o.webp`): the
/// nose, mouth and whiskers, drawn over the file in the mouth.
pub fn has_overlay(i: usize) -> bool {
    CLIPS[i].name.starts_with("carry")
}

// Where the mouth is in each carrying frame: points ahead of the feet and
// up from the ground, and the head's angle in degrees (from the sheet script).
const MOUTH_CARRY_PICK: [(f32, f32, f32); 12] = [(70.7, 94.3, 1085.7), (77.0, 84.6, 1096.6), (82.1, 71.7, 1109.6), (84.0, 60.1, 1120.5), (84.2, 55.2, 1125.1), (84.0, 51.1, 1128.6), (84.2, 55.2, 1125.1), (83.4, 64.5, 1116.8), (77.0, 83.7, 1098.6), (65.3, 99.6, 1080.4), (58.7, 105.2, 1072.1), (62.7, 102.1, 1076.1)];
const MOUTH_CARRY: [(f32, f32, f32); 19] = [(62.7, 102.1, 1076.1), (62.4, 101.7, 1076.1), (62.1, 101.4, 1076.0), (61.8, 102.6, 1076.0), (61.4, 104.5, 1076.0), (61.2, 106.4, 1076.0), (60.8, 108.3, 1075.9), (60.5, 110.3, 1075.9), (60.1, 109.8, 1075.9), (59.8, 109.3, 1075.9), (59.9, 108.0, 1075.9), (60.2, 106.1, 1075.9), (60.6, 104.6, 1075.9), (60.8, 105.2, 1075.9), (61.2, 105.7, 1076.0), (61.5, 106.3, 1076.0), (61.8, 106.9, 1076.0), (62.1, 106.3, 1076.0), (62.4, 104.2, 1076.1)];
const MOUTH_CARRY_RUN: [(f32, f32, f32); 19] = [(72.5, 105.0, 1080.2), (71.2, 106.9, 1076.4), (69.9, 108.8, 1072.5), (68.4, 112.6, 1068.7), (66.9, 117.1, 1064.8), (65.4, 121.5, 1061.0), (63.8, 125.8, 1057.1), (62.1, 130.0, 1053.3), (60.5, 131.4, 1049.4), (58.8, 132.5, 1045.6), (59.0, 130.7, 1046.1), (60.7, 126.7, 1049.9), (62.3, 123.2, 1053.7), (64.0, 121.9, 1057.5), (65.5, 120.6, 1061.3), (67.0, 119.0, 1065.1), (68.5, 117.4, 1068.9), (69.9, 114.5, 1072.6), (71.2, 109.8, 1076.4)];
const MOUTH_CARRY_DROP: [(f32, f32, f32); 12] = [(66.6, 98.5, 1081.2), (74.6, 88.2, 1093.3), (81.2, 73.9, 1107.8), (83.9, 60.8, 1120.0), (84.2, 55.2, 1125.1), (84.0, 51.1, 1128.6), (84.2, 55.2, 1125.1), (84.0, 60.1, 1120.5), (82.1, 71.7, 1109.6), (77.0, 84.6, 1096.6), (70.7, 94.3, 1085.7), (67.5, 97.9, 1081.1)];

/// Where the mouth is in frame `f` of clip `i`: points ahead of the feet,
/// up from the ground, and how far the file in it leans (degrees, toward
/// the way the cat faces). None for clips that carry nothing.
pub fn mouth(i: usize, f: u32) -> Option<(f64, f64, f64)> {
    let table: &[(f32, f32, f32)] = match CLIPS[i].name {
        "carry_pick" => &MOUTH_CARRY_PICK,
        "carry" => &MOUTH_CARRY,
        "carry_run" => &MOUTH_CARRY_RUN,
        "carry_drop" => &MOUTH_CARRY_DROP,
        _ => return None,
    };
    // the head's angle when it bites: the file hangs straight there
    let bite = MOUTH_CARRY_PICK[PICK_AT as usize].2 as f64;
    table.get(f as usize).map(|&(x, y, a)| (x as f64, y as f64, 0.4 * (bite - a as f64)))
}

/// One frame on screen: frame `f` of clip `clip`, the feet at `x` points
/// from the screen's left edge, facing left (-1) or right (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub clip: usize,
    pub f: u32,
    pub x: f64,
    pub dir: i8,
}

/// How the cat rests between acts when it lives on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rest {
    /// Not on screen (or it comes and goes for each reaction).
    #[default]
    Away,
    /// Sitting in the corner.
    Sit,
    /// Curled up asleep in the corner.
    Sleep,
}

impl Rest {
    /// The picture it rests on: a clip and frame.
    pub fn shot(self, width: f64) -> Option<Shot> {
        let x = home(width);
        match self {
            Rest::Away => None,
            Rest::Sit => Some(Shot { clip: clip("sitting"), f: 0, x, dir: -1 }),
            Rest::Sleep => Some(Shot { clip: clip("sleeping"), f: 0, x, dir: -1 }),
        }
    }
}

/// Something the cat does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    React(Kind),
    /// Bats at a pointer resting on it, for as long as it rests there.
    Play,
    /// Small things it does now and then while it sits: a blink, a wash.
    Blink,
    Wash,
    /// Nothing happened for a while: it curls up.
    Nap,
}

/// What the cat does for an act: `intro` once, then `hold` over and
/// over while the act lasts (a purr lasts while the hand stays), then
/// `outro`. Without a `hold` the intro is the whole act.
#[derive(Debug, Default)]
pub struct Script {
    pub intro: Vec<Shot>,
    pub hold: Vec<Shot>,
    pub outro: Vec<Shot>,
    /// The charger's bowl, its middle this many points from the left.
    pub bowl: Option<f64>,
    /// How it rests once the act is over.
    pub rest: Rest,
    /// The tidy box, its middle this many points from the left.
    pub tidy_box: Option<f64>,
    /// The files it fetches in a tidy, in order.
    pub fetch: Vec<Fetch>,
    /// The frame of the intro where the reaction's sound starts (the hiss
    /// as the fur goes up, the crunch at the first bite); the intro's
    /// length is the start of the hold (the purr while it lies curled up).
    pub voice: Option<usize>,
}

/// One file carried to the box in a tidy. The numbers are frames of the
/// intro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fetch {
    /// Which of the files given to `tidy`.
    pub file: usize,
    /// It starts to fall here (and the real file goes into the box folder)…
    pub fall: usize,
    /// …and lies on the ground at `x` until the mouth takes it here…
    pub x: f64,
    pub pick: usize,
    /// …and drops into the box here.
    pub drop: usize,
}

impl Script {
    /// The sheets it shows.
    pub fn clips(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.intro.iter().chain(&self.hold).chain(&self.outro).map(|s| s.clip).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Seconds `shots` take.
    pub fn secs(shots: &[Shot]) -> f64 {
        shots.len() as f64 / FPS
    }
}

/// Where the cat sits on a screen `width` points wide.
pub fn home(width: f64) -> f64 {
    (width - HOME_IN).max(width / 2.0)
}

/// Builds a script frame by frame, as the design canvas plays it.
struct Act {
    x: f64,
    dir: i8,
    /// A walk right after a turn starts mid-stride, where the turn left the legs.
    turned: bool,
    out: Vec<Shot>,
}

impl Act {
    fn new(x: f64, dir: i8) -> Act {
        Act { x, dir, turned: false, out: Vec::new() }
    }

    fn play(&mut self, name: &str, loops: u32) {
        let c = clip(name);
        for _ in 0..loops {
            for f in 0..CLIPS[c].n {
                self.out.push(Shot { clip: c, f, x: self.x, dir: self.dir });
            }
        }
        self.turned = false;
    }

    /// Stand still for `frames`.
    fn stand(&mut self, frames: u32) {
        let c = clip("stand");
        for _ in 0..frames {
            self.out.push(Shot { clip: c, f: 0, x: self.x, dir: self.dir });
        }
    }

    fn turn(&mut self) {
        self.play("turn", 1);
        self.x -= TURN_SHIFT * self.dir as f64;
        self.dir = -self.dir;
        self.turned = true;
    }

    /// Walk (or run) to `to`, turning first if it lies behind. The stride
    /// is stretched a little so the last step ends exactly there.
    fn go(&mut self, name: &str, to: f64) {
        if (to - self.x).abs() < 1.0 {
            return;
        }
        let toward = if to > self.x { 1 } else { -1 };
        if toward != self.dir {
            self.turn();
        }
        let c = clip(name);
        let n = CLIPS[c].n;
        let f0 = if self.turned && (name == "walk" || name == "carry") { 10 } else { 0 };
        let speed = if name.ends_with("run") { RUN } else { WALK } / FPS;
        let d = (to - self.x).abs();
        // frames that end where a stride starts again, the closest to the speed
        let first = (n - f0) % n;
        let frames = (0..200).map(|k| first + k * n).filter(|t| *t >= 1).min_by(|a, b| (*a as f64 * speed - d).abs().total_cmp(&(*b as f64 * speed - d).abs())).unwrap_or(n);
        let step = d / frames as f64 * self.dir as f64;
        for i in 0..frames {
            self.out.push(Shot { clip: c, f: (f0 + i) % n, x: self.x + step * i as f64, dir: self.dir });
        }
        self.x = to;
        self.turned = false;
    }

    /// Get to `to` facing `dir`: walk, or run when it is far (`run`
    /// names the running clip), and turn at the end if need be.
    fn reach(&mut self, to: f64, dir: i8, walk: &str, run: &str) {
        let way = if (to - self.x).abs() < 1.0 { self.dir } else if to > self.x { 1 } else { -1 };
        // a turn moves the feet: stop short of `to` so the turn ends on it
        let stop = if way == dir { to } else { to + TURN_SHIFT * -dir as f64 };
        let name = if (stop - self.x).abs() > RUN_PAST { run } else { walk };
        self.go(name, stop);
        if self.dir != dir {
            self.turn();
        }
        self.x = to;
    }

    /// From resting to standing.
    fn get_up(&mut self, from: Rest) {
        match from {
            Rest::Sit => self.play("sit_transition_from", 1),
            Rest::Sleep => {
                self.play("sleep_transition_from", 1);
                self.stand(4);
            }
            Rest::Away => {}
        }
    }

    /// Back to the corner, facing out, and down on its bottom.
    fn sit_home(&mut self, width: f64) {
        self.reach(home(width), -1, "walk", "run");
        self.play("sit_tranistion_to", 1);
    }

    fn take(&mut self) -> Vec<Shot> {
        std::mem::take(&mut self.out)
    }
}

/// The script for `what` on a screen `width` points wide. `from`: how
/// the cat rests now; `live`: it stays in the corner afterwards (else it
/// goes once the act is over).
pub fn script(what: Do, width: f64, from: Rest, live: bool) -> Script {
    let home = home(width);
    let mut a = Act::new(home, -1);
    let mut s = Script { rest: if live { Rest::Sit } else { Rest::Away }, ..Script::default() };
    let away = from == Rest::Away;
    match what {
        Do::React(Kind::Hiss) => {
            // fur up, then off the screen: it ran away
            a.get_up(from);
            s.voice = Some(a.out.len());
            a.play("scare_in", 1);
            a.play("scare", 2);
            a.play("scare_out", 1);
            a.go("run", width + 140.0);
            if live {
                // a while later it looks back in, and comes home
                a.x = width + PEEK_OUT;
                a.dir = -1;
                for _ in 0..48 {
                    a.out.push(Shot { clip: clip("stand"), f: 0, x: width + 400.0, dir: -1 });
                }
                a.play("peek_in", 1);
                a.play("peek", 1);
                a.play("peek_out", 1);
                a.sit_home(width);
            }
        }
        Do::React(Kind::Purr) => {
            // curls up, stays a ball while the hand stays, uncurls
            if from != Rest::Sleep {
                a.get_up(from);
                a.play("sleep_transition_to", 1);
            }
            s.intro = a.take();
            s.voice = Some(s.intro.len());
            a.play("sleeping", 1);
            s.hold = a.take();
            a.play("sleep_transition_from", 1);
            if live {
                a.stand(4);
                a.play("sit_tranistion_to", 1);
            } else {
                a.stand(10);
            }
            s.outro = a.take();
            return s;
        }
        Do::React(Kind::Eat) => {
            s.bowl = Some(home - BOWL_AHEAD);
            a.get_up(from);
            a.play("eat_in", 1);
            s.voice = Some(a.out.len());
            a.play("eat", 6);
            a.play("eat_out", 1);
            if live {
                a.play("sit_tranistion_to", 1);
            } else {
                a.stand(10);
            }
        }
        Do::React(Kind::Hungry) => {
            a.get_up(from);
            s.voice = Some(a.out.len());
            a.play("beg", 2);
            if live {
                a.play("sit_tranistion_to", 1);
            } else {
                a.stand(8);
            }
        }
        Do::React(Kind::Hello) => {
            // in from the right edge: a look first, then the walk to its place;
            // one already sitting in its corner gets up and has the look there
            if from == Rest::Away {
                a.x = width + PEEK_OUT;
            } else {
                a.get_up(from);
            }
            a.play("peek_in", 1);
            s.voice = Some(a.out.len());
            a.play("peek", 1);
            a.play("peek_out", 1);
            if live {
                a.sit_home(width);
            } else {
                a.go("walk", home);
                a.stand(14);
            }
        }
        Do::Play => {
            a.get_up(if away { Rest::Sit } else { from });
            s.intro = a.take();
            a.play("scratch", 1);
            s.hold = a.take();
            a.stand(3);
            a.play("sit_tranistion_to", 1);
            s.outro = a.take();
            s.rest = Rest::Sit;
            return s;
        }
        Do::Blink => {
            a.play("sitting", 1);
            s.rest = Rest::Sit;
        }
        Do::Wash => {
            a.play("clean", 1);
            s.rest = Rest::Sit;
        }
        Do::Nap => {
            a.get_up(from);
            a.play("sleep_transition_to", 1);
            s.rest = Rest::Sleep;
        }
    }
    s.intro = a.take();
    s
}

/// The tidy: the cat fetches the files at `files` (where each one lies on
/// screen: points from the left) one by one and drops them into a box
/// beside its corner, then sits again. Up to `TIDY_MAX` files, the
/// nearest to the box.
pub fn tidy(width: f64, from: Rest, files: &[f64]) -> Script {
    let home = home(width);
    let bx = home - BOX_LEFT;
    let mut a = Act::new(home, -1);
    let mut s = Script { rest: Rest::Sit, tidy_box: Some(bx), ..Script::default() };
    a.get_up(from);
    let reach_pick = MOUTH_CARRY_PICK[PICK_AT as usize].0 as f64;
    let reach_drop = MOUTH_CARRY_DROP[DROP_AT as usize].0 as f64;
    // nearest to the box first
    let mut order: Vec<(usize, f64)> = files.iter().copied().enumerate().collect();
    order.sort_by(|p, q| (p.1 - bx).abs().total_cmp(&(q.1 - bx).abs()));
    order.truncate(TIDY_MAX);
    for (file, fx) in order {
        // it lands where the cat can stand beside it on screen
        let x = fx.clamp(reach_pick + 40.0, width - reach_pick - 40.0);
        // the cat faces the box while it picks the file up, so it stands
        // on the far side
        let d: i8 = if bx >= x { 1 } else { -1 };
        let stand = x - d as f64 * reach_pick;
        let start = a.out.len();
        a.reach(stand, d, "walk", "run");
        // the file falls while the cat comes; on the ground by the time it is there
        let fall = a.out.len().saturating_sub(FALL_FRAMES + 3).max(start);
        let landed = fall + FALL_FRAMES;
        while a.out.len() < landed {
            a.stand(1);
        }
        let pick = a.out.len() + PICK_AT as usize;
        a.play("carry_pick", 1);
        let at = bx - d as f64 * reach_drop;
        let far = (at - a.x).abs() > RUN_PAST;
        a.go(if far { "carry_run" } else { "carry" }, at);
        let drop = a.out.len() + DROP_AT as usize;
        a.play("carry_drop", 1);
        s.fetch.push(Fetch { file, fall, x, pick, drop });
    }
    a.sit_home(width);
    s.intro = a.take();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_in_its_sheet() {
        for k in Kind::ALL {
            let s = script(Do::React(k), 1512.0, Rest::Away, false);
            assert!(!s.intro.is_empty(), "{k:?}");
            for shot in s.intro.iter().chain(&s.hold).chain(&s.outro) {
                assert!(shot.f < CLIPS[shot.clip].n, "{k:?} {shot:?}");
                assert!(shot.dir == 1 || shot.dir == -1);
            }
        }
    }

    #[test]
    fn each_sound_starts_with_its_moment() {
        let at = |k: Kind, from: Rest| {
            let s = script(Do::React(k), 1512.0, from, true);
            let v = s.voice.expect("a reaction has a sound");
            let shot = s.intro.get(v).or(s.hold.first()).copied().unwrap();
            (CLIPS[shot.clip].name, v, s.intro.len())
        };
        for from in [Rest::Away, Rest::Sit, Rest::Sleep] {
            assert_eq!(at(Kind::Hiss, from).0, "scare_in", "{from:?}");
            assert_eq!(at(Kind::Eat, from).0, "eat", "{from:?}");
            assert_eq!(at(Kind::Hungry, from).0, "beg", "{from:?}");
            assert_eq!(at(Kind::Hello, from).0, "peek", "{from:?}");
            let (name, v, intro) = at(Kind::Purr, from);
            assert_eq!((name, v), ("sleeping", intro), "the purr starts as it lies curled up ({from:?})");
        }
        // getting up from the corner comes first, in silence
        assert!(at(Kind::Hiss, Rest::Sit).1 > 0);
    }

    #[test]
    fn a_cat_at_home_says_hello_where_it_sits() {
        for from in [Rest::Sit, Rest::Sleep] {
            let s = script(Do::React(Kind::Hello), 1512.0, from, true);
            let home = home(1512.0);
            assert!(s.intro.iter().all(|x| (x.x - home).abs() < 1.0), "{from:?}: no jump to the edge");
        }
    }

    #[test]
    fn a_slapped_cat_runs_off_the_screen() {
        let s = script(Do::React(Kind::Hiss), 1512.0, Rest::Away, false);
        assert_eq!(CLIPS[s.intro[0].clip].name, "scare_in");
        let last = s.intro.last().unwrap();
        assert_eq!(CLIPS[last.clip].name, "run");
        assert_eq!(last.dir, 1, "it turned to the near edge");
        assert!(last.x > 1512.0 - 20.0, "{}", last.x);
        assert!(s.intro.iter().all(|x| CLIPS[x.clip].name != "jump"), "no jump: a scare");
    }

    #[test]
    fn a_purr_lasts_while_the_hand_stays() {
        let s = script(Do::React(Kind::Purr), 1512.0, Rest::Away, false);
        assert!(s.hold.iter().all(|x| CLIPS[x.clip].name == "sleeping"));
        assert_eq!(s.hold.len(), CLIPS[clip("sleeping")].n as usize);
        assert_eq!(CLIPS[s.intro[0].clip].name, "sleep_transition_to");
        assert_eq!(CLIPS[s.outro[0].clip].name, "sleep_transition_from");
    }

    #[test]
    fn hello_peeks_in_and_walks_home() {
        let s = script(Do::React(Kind::Hello), 1512.0, Rest::Away, false);
        assert!(s.intro[0].x > 1512.0, "starts past the edge");
        let walk: Vec<_> = s.intro.iter().filter(|x| CLIPS[x.clip].name == "walk").collect();
        assert!(!walk.is_empty());
        assert_eq!(walk.last().unwrap().f, CLIPS[clip("walk")].n - 1, "ends at the end of a stride");
        assert!((s.intro.last().unwrap().x - (1512.0 - HOME_IN)).abs() < 0.5);
        // steps stay close to the paws' speed
        let d = (walk[1].x - walk[0].x).abs() * FPS;
        assert!((d - WALK).abs() < WALK * 0.3, "{d}");
    }

    #[test]
    fn the_bowl_stands_at_the_mouth() {
        let s = script(Do::React(Kind::Eat), 1512.0, Rest::Away, false);
        assert_eq!(s.bowl, Some(1512.0 - HOME_IN - BOWL_AHEAD));
        assert!(s.clips().iter().any(|c| CLIPS[*c].name == "eat"));
    }

    fn all_in_sheets(s: &Script) {
        for shot in s.intro.iter().chain(&s.hold).chain(&s.outro) {
            assert!(shot.f < CLIPS[shot.clip].n, "{shot:?}");
            assert!(shot.dir == 1 || shot.dir == -1);
        }
    }

    #[test]
    fn a_cat_that_lives_here_comes_back_to_its_corner() {
        let w = 1512.0;
        let every = Kind::ALL.map(Do::React).into_iter().chain([Do::Play, Do::Blink, Do::Wash, Do::Nap]);
        for what in every {
            for from in [Rest::Sit, Rest::Sleep] {
                let s = script(what, w, from, true);
                all_in_sheets(&s);
                assert_ne!(s.rest, Rest::Away, "{what:?}");
                let last = s.outro.last().or(s.intro.last()).unwrap();
                assert!((last.x - home(w)).abs() < 0.5, "{what:?} ends at {}", last.x);
                assert_eq!(last.dir, -1, "{what:?} faces out");
                if s.rest == Rest::Sit {
                    let name = CLIPS[last.clip].name;
                    assert!(name == "sit_tranistion_to" || name == "sitting" || name == "clean", "{what:?} ends on {name}");
                }
            }
        }
        // a sleeping cat gets up before it does anything
        let s = script(Do::React(Kind::Eat), w, Rest::Sleep, true);
        assert_eq!(CLIPS[s.intro[0].clip].name, "sleep_transition_from");
        let s = script(Do::Play, w, Rest::Sit, true);
        assert_eq!(CLIPS[s.intro[0].clip].name, "sit_transition_from");
        assert!(s.hold.iter().all(|x| CLIPS[x.clip].name == "scratch"));
    }

    #[test]
    fn a_tidy_carries_each_file_to_the_box() {
        let w = 1512.0;
        let files = [1460.0, 1440.0, 300.0, 900.0, 1200.0];
        let s = tidy(w, Rest::Sit, &files);
        all_in_sheets(&s);
        let bx = s.tidy_box.unwrap();
        assert_eq!(s.fetch.len(), files.len());
        let mut seen: Vec<usize> = s.fetch.iter().map(|f| f.file).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
        let many = [700.0; 20];
        assert_eq!(tidy(w, Rest::Sit, &many).fetch.len(), TIDY_MAX);
        for f in &s.fetch {
            assert!(f.fall + FALL_FRAMES <= f.pick, "{f:?}");
            assert!(f.pick < f.drop && f.drop < s.intro.len());
            // the mouth is at the file when it bites, and over the box when it lets go
            let p = s.intro[f.pick];
            let (mx, _, _) = mouth(p.clip, p.f).unwrap();
            assert!((p.x + p.dir as f64 * mx - f.x).abs() < 1.0, "{f:?} {p:?}");
            let d = s.intro[f.drop];
            let (mx, _, _) = mouth(d.clip, d.f).unwrap();
            assert!((d.x + d.dir as f64 * mx - bx).abs() < 1.0, "{f:?} {d:?}");
            // held all the way
            for i in f.pick..=f.drop {
                assert!(mouth(s.intro[i].clip, s.intro[i].f).is_some(), "frame {i} of {f:?}");
            }
            assert!(f.x > 0.0 && f.x < w);
        }
        let last = s.intro.last().unwrap();
        assert!((last.x - home(w)).abs() < 0.5);
        assert_eq!(CLIPS[last.clip].name, "sit_tranistion_to");
        assert!(s.intro.iter().any(|x| CLIPS[x.clip].name == "carry_run"), "runs to far files");
    }

    #[test]
    fn feet_anchor_inside_the_frame() {
        for c in &CLIPS {
            let (ax, ay) = c.anchor();
            assert!((0.0..=1.0).contains(&ax), "{}", c.name);
            assert!((-0.05..=0.2).contains(&ay), "{} {ay}", c.name);
        }
    }
}
