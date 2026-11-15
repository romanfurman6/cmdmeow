//! The desktop cat: a drawn cat at the bottom of the screen that acts out
//! each reaction (it gets scared when slapped, curls up under a hand, eats
//! at the charger, asks for food, peeks in when the lid opens).
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
const HOME_IN: f64 = 170.0;
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
pub const CLIPS: [Clip; 17] = [
    Clip { name: "stand", n: 1, cols: 1, w: 436, h: 364, ox: 249.0, oy: 360.1 },
    Clip { name: "scare_in", n: 6, cols: 6, w: 419, h: 376, ox: 232.7, oy: 371.8 },
    Clip { name: "scare", n: 12, cols: 8, w: 408, h: 378, ox: 228.2, oy: 373.8 },
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
];

/// The tallest frame, in points: how tall the cat's strip of screen is.
pub fn tallest_pt() -> f64 {
    CLIPS.iter().map(|c| c.h).max().unwrap_or(0) as f64 / PX_PER_PT
}

fn clip(name: &str) -> usize {
    CLIPS.iter().position(|c| c.name == name).unwrap_or(0)
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

/// What the cat does for a reaction: `intro` once, then `hold` over and
/// over while the reaction lasts (a purr lasts while the hand stays), then
/// `outro`. Without a `hold` the intro is the whole act.
#[derive(Debug, Default)]
pub struct Script {
    pub intro: Vec<Shot>,
    pub hold: Vec<Shot>,
    pub outro: Vec<Shot>,
    /// The charger's bowl, its middle this many points from the left.
    pub bowl: Option<f64>,
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
        let f0 = if self.turned && name == "walk" { 10 } else { 0 };
        let speed = if name == "run" { RUN } else { WALK } / FPS;
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

    fn take(&mut self) -> Vec<Shot> {
        std::mem::take(&mut self.out)
    }
}

/// The script for `kind` on a screen `width` points wide.
pub fn script(kind: Kind, width: f64) -> Script {
    let home = (width - HOME_IN).max(width / 2.0);
    let mut a = Act::new(home, -1);
    let mut s = Script::default();
    match kind {
        Kind::Hiss => {
            // fur up, then off the screen: it ran away
            a.play("scare_in", 1);
            a.play("scare", 2);
            a.play("scare_out", 1);
            a.go("run", width + 140.0);
        }
        Kind::Purr => {
            // curls up, stays a ball while the hand stays, uncurls
            a.play("sleep_transition_to", 1);
            s.intro = a.take();
            a.play("sleeping", 1);
            s.hold = a.take();
            a.play("sleep_transition_from", 1);
            a.stand(10);
            s.outro = a.take();
            return s;
        }
        Kind::Eat => {
            s.bowl = Some(home - BOWL_AHEAD);
            a.play("eat_in", 1);
            a.play("eat", 6);
            a.play("eat_out", 1);
            a.stand(10);
        }
        Kind::Hungry => {
            a.play("beg", 2);
            a.stand(8);
        }
        Kind::Hello => {
            // in from the right edge: a look first, then the walk to its place
            a.x = width + PEEK_OUT;
            a.play("peek_in", 1);
            a.play("peek", 1);
            a.play("peek_out", 1);
            a.go("walk", home);
            a.stand(14);
        }
    }
    s.intro = a.take();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_in_its_sheet() {
        for k in Kind::ALL {
            let s = script(k, 1512.0);
            assert!(!s.intro.is_empty(), "{k:?}");
            for shot in s.intro.iter().chain(&s.hold).chain(&s.outro) {
                assert!(shot.f < CLIPS[shot.clip].n, "{k:?} {shot:?}");
                assert!(shot.dir == 1 || shot.dir == -1);
            }
        }
    }

    #[test]
    fn a_slapped_cat_runs_off_the_screen() {
        let s = script(Kind::Hiss, 1512.0);
        assert_eq!(CLIPS[s.intro[0].clip].name, "scare_in");
        let last = s.intro.last().unwrap();
        assert_eq!(CLIPS[last.clip].name, "run");
        assert_eq!(last.dir, 1, "it turned to the near edge");
        assert!(last.x > 1512.0 - 20.0, "{}", last.x);
        assert!(s.intro.iter().all(|x| CLIPS[x.clip].name != "jump"), "no jump: a scare");
    }

    #[test]
    fn a_purr_lasts_while_the_hand_stays() {
        let s = script(Kind::Purr, 1512.0);
        assert!(s.hold.iter().all(|x| CLIPS[x.clip].name == "sleeping"));
        assert_eq!(s.hold.len(), CLIPS[clip("sleeping")].n as usize);
        assert_eq!(CLIPS[s.intro[0].clip].name, "sleep_transition_to");
        assert_eq!(CLIPS[s.outro[0].clip].name, "sleep_transition_from");
    }

    #[test]
    fn hello_peeks_in_and_walks_home() {
        let s = script(Kind::Hello, 1512.0);
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
        let s = script(Kind::Eat, 1512.0);
        assert_eq!(s.bowl, Some(1512.0 - HOME_IN - BOWL_AHEAD));
        assert!(s.clips().iter().any(|c| CLIPS[*c].name == "eat"));
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
