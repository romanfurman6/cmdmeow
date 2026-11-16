//! The cat's things on the desk: the yarn ball people throw and it bats
//! about, and what the stage draws of the ball, the bed and the bowl.
//!
//! Pure: points down from the top left of the screen, milliseconds.

/// The ball's radius, points.
pub const BR: f64 = 12.0;
/// Rolling friction, points/s².
const BFR: f64 = 380.0;
/// Pull down when it flies, points/s².
const GRAV: f64 = 1900.0;
/// The bowl's height, points.
pub const BOWL_H: f64 = 32.0;

/// The yarn ball: where it lies on the desk (`x`, `y`), how high above it
/// it flies (`z`), and how it moves.
#[derive(Debug, Clone, Default)]
pub struct Ball {
    pub on: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub rot: f64,
    /// in a hand
    pub held: bool,
    /// last thrown or dropped
    pub touched: f64,
    pub alpha: f64,
    /// put away at (it shrinks out), else 0
    pub gone: f64,
}

impl Ball {
    pub fn speed(&self) -> f64 {
        self.vx.hypot(self.vy)
    }

    /// The middle of the ball as drawn.
    pub fn top(&self) -> (f64, f64) {
        (self.x, self.y - BR - self.z)
    }

    pub fn under(&self, p: (f64, f64)) -> bool {
        let c = self.top();
        self.on && (p.0 - c.0).hypot(p.1 - c.1) < BR + 10.0
    }

    /// Where it will lie in `ms` (rolling slows it), kept in `area`.
    pub fn at(&self, ms: f64, area: (f64, f64, f64, f64)) -> (f64, f64) {
        let v = self.speed();
        let t = ms / 1000.0;
        if v < 1.0 {
            return (self.x, self.y);
        }
        let d = (v * t - 0.5 * BFR * t * t).min(v * v / (2.0 * BFR));
        ((self.x + self.vx / v * d).clamp(area.0, area.2), (self.y + self.vy / v * d).clamp(area.1, area.3))
    }

    /// One step of `dt` ms inside `area` (x0, y0, x1, y1 of where it may
    /// lie). Returns how hard it hit the desk, if it did.
    pub fn step(&mut self, dt: f64, area: (f64, f64, f64, f64)) -> Option<f64> {
        let s = dt / 1000.0;
        let mut hit = None;
        self.vz -= GRAV * s;
        self.z += self.vz * s;
        if self.z < 0.0 {
            self.z = 0.0;
            if self.vz < -140.0 {
                hit = Some(-self.vz);
                self.vz = -self.vz * 0.42;
            } else {
                self.vz = 0.0;
            }
        }
        self.x += self.vx * s;
        self.y += self.vy * s;
        let v = self.speed();
        if v > 0.0 && self.z < 1.0 {
            let k = (v - BFR * s).max(0.0) / v;
            self.vx *= k;
            self.vy *= k;
        }
        self.rot += self.vx * s / BR;
        let (x0, y0, x1, y1) = area;
        if self.x < x0 {
            self.x = x0;
            self.vx = self.vx.abs() * 0.55;
        }
        if self.x > x1 {
            self.x = x1;
            self.vx = -self.vx.abs() * 0.55;
        }
        if self.y < y0 {
            self.y = y0;
            self.vy = self.vy.abs() * 0.55;
        }
        if self.y > y1 {
            self.y = y1;
            self.vy = -self.vy.abs() * 0.55;
        }
        hit
    }
}

/// The ball as the stage draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallView {
    /// where it lies, and how high above that it is
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub rot: f64,
    pub alpha: f64,
    /// 1, shrinking to 0 as it is put away
    pub scale: f64,
}

/// The bowl as the stage draws it: the middle of its foot, the charge
/// (its food), and whether the front wall goes over the cat (its head is
/// in the bowl) or under it (the cat stands in front of it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BowlView {
    pub x: f64,
    pub y: f64,
    pub level: f64,
    pub charging: bool,
    pub low: bool,
    /// a battery to show on its side
    pub battery: bool,
    pub front: bool,
}

/// The dry food in the bowl: where each piece lies, from the middle of
/// the rim (points, y down), its radius and its shade (0–6). The first
/// ones lie at the bottom; a fuller bowl shows more of the list.
pub fn kibbles() -> Vec<(f64, f64, f64, u8)> {
    let mut out = Vec::new();
    for r in 0..5 {
        let w = 33.0 - r as f64 * 6.5;
        let y = 3.0 - r as f64 * 4.4;
        let n = ((w * 2.0 / 7.6).floor() as usize).max(1);
        for i in 0..n {
            let x = -w + (i as f64 + 0.5) * (2.0 * w / n as f64) + if r % 2 == 1 { 1.8 } else { -1.8 };
            let shade = ((i * 13 + r * 5) % 7) as u8;
            out.push((x, y + (i as f64 * 2.3 + r as f64).sin() * 1.2, 3.6 + ((i * 7 + r * 3) % 5) as f64 * 0.18, shade));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thrown_ball_bounces_rolls_and_stops() {
        let mut b = Ball { on: true, x: 500.0, y: 500.0, z: 200.0, vx: 300.0, vy: 0.0, ..Ball::default() };
        let area = (0.0, 0.0, 2000.0, 1000.0);
        let mut hits = 0;
        for _ in 0..600 {
            if b.step(16.7, area).is_some() {
                hits += 1;
            }
        }
        assert!(hits >= 2, "{hits} bounces");
        assert!(b.speed() < 1.0 && b.z == 0.0);
        assert!(b.x > 600.0 && b.x < 900.0, "rolled to {}", b.x);
    }

    #[test]
    fn the_walls_send_it_back() {
        let mut b = Ball { on: true, x: 990.0, y: 500.0, vx: 900.0, ..Ball::default() };
        for _ in 0..30 {
            b.step(16.7, (0.0, 0.0, 1000.0, 1000.0));
        }
        assert!(b.x <= 1000.0 && b.vx < 0.0);
    }

    #[test]
    fn a_full_bowl_has_a_heap_of_food() {
        let k = kibbles();
        assert!(k.len() > 18, "{}", k.len());
        assert!(k.iter().all(|p| p.0.abs() < 38.0));
    }
}
