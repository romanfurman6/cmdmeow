//! The cuts made to a video, as the stretches of it that are kept. Pure, so
//! it's tested on any platform. Times are seconds of the source file.

/// Shorter than this isn't worth keeping (or cutting).
pub const MIN_PIECE: f64 = 0.05;

#[derive(Clone, Debug, PartialEq)]
pub struct Edl {
    /// The source's length.
    pub duration: f64,
    /// Kept stretches, in order, not touching each other.
    pub keep: Vec<(f64, f64)>,
}

impl Edl {
    pub fn new(duration: f64) -> Edl {
        let duration = duration.max(0.0);
        Edl { duration, keep: vec![(0.0, duration)] }
    }

    /// Nothing cut or trimmed.
    pub fn untouched(&self) -> bool {
        self.keep.len() == 1 && self.keep[0].0 <= 0.0 && self.keep[0].1 >= self.duration
    }

    /// Length after the cuts.
    pub fn length(&self) -> f64 {
        self.keep.iter().map(|(a, b)| b - a).sum()
    }

    /// Where the kept video starts and ends in the source.
    pub fn start(&self) -> f64 {
        self.keep.first().map(|k| k.0).unwrap_or(0.0)
    }

    pub fn end(&self) -> f64 {
        self.keep.last().map(|k| k.1).unwrap_or(0.0)
    }

    /// Remove `a..b` (source seconds). Returns false when it would leave
    /// nothing, or remove nothing.
    pub fn cut(&mut self, a: f64, b: f64) -> bool {
        let (a, b) = (a.min(b).max(0.0), a.max(b).min(self.duration));
        if b - a < MIN_PIECE {
            return false;
        }
        let mut next = Vec::with_capacity(self.keep.len() + 1);
        for &(s, e) in &self.keep {
            if e <= a || s >= b {
                next.push((s, e));
                continue;
            }
            if a - s >= MIN_PIECE {
                next.push((s, a));
            }
            if e - b >= MIN_PIECE {
                next.push((b, e));
            }
        }
        if next.is_empty() || next == self.keep {
            return false;
        }
        self.keep = next;
        true
    }

    /// Drop everything before `t`.
    pub fn trim_start(&mut self, t: f64) -> bool {
        let t = t.max(0.0);
        if t <= self.start() {
            // trimming back out: give the first piece its start back
            if let Some(k) = self.keep.first_mut() {
                if t < k.0 {
                    k.0 = t;
                    return true;
                }
            }
            return false;
        }
        self.cut(0.0, t)
    }

    /// Drop everything after `t`.
    pub fn trim_end(&mut self, t: f64) -> bool {
        let t = t.min(self.duration);
        if t >= self.end() {
            let d = self.duration;
            if let Some(k) = self.keep.last_mut() {
                if t > k.1 {
                    k.1 = t.min(d);
                    return true;
                }
            }
            return false;
        }
        self.cut(t, self.duration)
    }

    /// Is source time `t` kept?
    pub fn kept(&self, t: f64) -> bool {
        self.keep.iter().any(|&(a, b)| t >= a && t < b)
    }

    /// Source `t` → time in the edited video (None if cut).
    pub fn to_edited(&self, t: f64) -> Option<f64> {
        let mut before = 0.0;
        for &(a, b) in &self.keep {
            if t >= a && t < b {
                return Some(before + t - a);
            }
            before += b - a;
        }
        None
    }

    /// How much kept video comes before source `t` (its place in the edited
    /// video, also for a `t` inside a cut: where the cut closes up).
    pub fn edited_at(&self, t: f64) -> f64 {
        self.keep.iter().map(|&(a, b)| (t.min(b) - a).max(0.0)).sum()
    }

    /// Edited time → source time (clamped into the video).
    pub fn to_source(&self, e: f64) -> f64 {
        let mut left = e.max(0.0);
        for &(a, b) in &self.keep {
            if left < b - a {
                return a + left;
            }
            left -= b - a;
        }
        self.end()
    }

    /// Where playback at source `t` should be: `t` if kept, else the start
    /// of the next kept piece (None past the end).
    pub fn playable(&self, t: f64) -> Option<f64> {
        if self.kept(t) {
            return Some(t);
        }
        self.keep.iter().find(|k| k.0 >= t).map(|k| k.0)
    }

    /// The removed stretches, for drawing.
    pub fn removed(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut at = 0.0;
        for &(a, b) in &self.keep {
            if a > at {
                out.push((at, a));
            }
            at = b;
        }
        if self.duration > at {
            out.push((at, self.duration));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutting_the_middle_leaves_two_pieces() {
        let mut e = Edl::new(10.0);
        assert!(e.untouched());
        assert!(e.cut(4.0, 6.0));
        assert_eq!(e.keep, vec![(0.0, 4.0), (6.0, 10.0)]);
        assert_eq!(e.length(), 8.0);
        assert_eq!(e.removed(), vec![(4.0, 6.0)]);
        assert!(!e.untouched());
    }

    #[test]
    fn trimming_both_ends() {
        let mut e = Edl::new(10.0);
        assert!(e.trim_start(1.0));
        assert!(e.trim_end(9.0));
        assert_eq!(e.keep, vec![(1.0, 9.0)]);
        assert_eq!(e.removed(), vec![(0.0, 1.0), (9.0, 10.0)]);
        // dragging the handle back out
        assert!(e.trim_start(0.5));
        assert_eq!(e.start(), 0.5);
        assert!(e.trim_end(10.0));
        assert_eq!(e.end(), 10.0);
    }

    #[test]
    fn times_map_both_ways_around_cuts() {
        let mut e = Edl::new(10.0);
        e.cut(2.0, 5.0);
        assert_eq!(e.to_edited(1.0), Some(1.0));
        assert_eq!(e.to_edited(3.0), None);
        assert_eq!(e.to_edited(6.0), Some(3.0));
        assert_eq!(e.to_source(3.0), 6.0);
        assert_eq!(e.to_source(100.0), 10.0);
        assert_eq!(e.playable(3.0), Some(5.0));
        assert_eq!(e.playable(10.5), None);
        assert_eq!(e.edited_at(3.0), 2.0, "inside a cut: where it closes up");
        assert_eq!(e.edited_at(7.0), 4.0);
        assert_eq!(e.edited_at(20.0), 7.0);
    }

    #[test]
    fn nothing_left_or_nothing_cut_is_refused() {
        let mut e = Edl::new(10.0);
        assert!(!e.cut(0.0, 10.0), "can't cut everything");
        assert!(!e.cut(3.0, 3.01), "too short to matter");
        e.cut(2.0, 5.0);
        assert!(!e.cut(3.0, 4.0), "already gone");
        assert!(e.cut(1.0, 6.0), "overlapping cuts merge");
        assert_eq!(e.keep, vec![(0.0, 1.0), (6.0, 10.0)]);
    }
}
