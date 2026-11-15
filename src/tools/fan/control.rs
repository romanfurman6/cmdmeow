//! What the user asked the fans to do (mode, fixed speed, curve) and how
//! that turns into a speed per fan. Saved between launches in
//! `fan.json`. Pure logic: the helper does the writing.

use super::data::{Fan, Group, Snapshot};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Any sensor at or above this gives the fans back to macOS.
pub const TRIP_C: f32 = 95.0;
/// …until everything cools below this.
pub const RESUME_C: f32 = 80.0;
/// Below this Quiet leaves the fans to macOS, which stops them when cool.
pub const QUIET_FROM_C: f32 = 60.0;
/// …and once holding, Quiet lets go only below this, so a temperature
/// hovering at 60 °C doesn't flip the fans every few seconds.
pub const QUIET_UNTIL_C: f32 = 55.0;
/// How much of each new reading the smoothed temperature takes in.
const SMOOTH_ALPHA: f32 = 0.3;
/// Curve x range, °C.
pub const CURVE_LO: f32 = 30.0;
pub const CURVE_HI: f32 = 95.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Auto,
    Quiet,
    Fixed,
    Curve,
    Max,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Auto, Mode::Quiet, Mode::Fixed, Mode::Curve, Mode::Max];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Auto => "Auto",
            Mode::Quiet => "Quiet",
            Mode::Fixed => "Fixed",
            Mode::Curve => "Curve",
            Mode::Max => "Max",
        }
    }
    pub fn index(self) -> usize {
        Mode::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
}

/// Which temperature a curve follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    #[default]
    Hottest,
    Cpu,
    Gpu,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::Hottest, Source::Cpu, Source::Gpu];
    pub fn label(self) -> &'static str {
        match self {
            Source::Hottest => "Hottest",
            Source::Cpu => "CPU",
            Source::Gpu => "GPU",
        }
    }
    /// The temperature this source reads now, falling back to the hottest part.
    pub fn read(self, s: &Snapshot) -> Option<f32> {
        let pick = |want: fn(Group) -> bool| s.groups().into_iter().filter(|g| want(g.group)).map(|g| g.max).reduce(f32::max);
        let v = match self {
            Source::Hottest => None,
            Source::Cpu => pick(|g| matches!(g, Group::PCores | Group::ECores | Group::Cpu)),
            Source::Gpu => pick(|g| g == Group::Gpu),
        };
        v.or_else(|| s.hottest().map(|h| h.1))
    }
}

/// Three points, (°C, fraction of the fan's range from min to max), in
/// rising temperature. Below the first point the fans stay at its speed,
/// above the last at the last one's.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub pts: [(f32, f32); 3],
    #[serde(default)]
    pub source: Source,
}

impl Default for Curve {
    fn default() -> Self {
        Curve { pts: Preset::Balanced.points(), source: Source::Hottest }
    }
}

impl Curve {
    /// Fraction of the range at `t` °C.
    pub fn at(&self, t: f32) -> f32 {
        let p = &self.pts;
        if t <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t <= b.0 {
                let k = if b.0 > a.0 { (t - a.0) / (b.0 - a.0) } else { 1.0 };
                return a.1 + (b.1 - a.1) * k;
            }
        }
        p[2].1
    }

    /// Move point `i` to (t, f), keeping the points in order 3 °C apart,
    /// inside the chart, and the speed never falling as it gets hotter.
    pub fn drag(&mut self, i: usize, t: f32, f: f32) {
        let lo_t = if i == 0 { CURVE_LO } else { self.pts[i - 1].0 + 3.0 };
        let hi_t = if i == 2 { CURVE_HI } else { self.pts[i + 1].0 - 3.0 };
        let lo_f = if i == 0 { 0.0 } else { self.pts[i - 1].1 };
        let hi_f = if i == 2 { 1.0 } else { self.pts[i + 1].1 };
        self.pts[i] = (t.clamp(lo_t, hi_t.max(lo_t)).round(), f.clamp(lo_f, hi_f.max(lo_f)));
    }

    pub fn preset(&self) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.points() == self.pts)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Quiet,
    Balanced,
    Cool,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Quiet, Preset::Balanced, Preset::Cool];
    pub fn label(self) -> &'static str {
        match self {
            Preset::Quiet => "Quiet",
            Preset::Balanced => "Balanced",
            Preset::Cool => "Cool",
        }
    }
    pub fn points(self) -> [(f32, f32); 3] {
        match self {
            Preset::Quiet => [(55.0, 0.0), (75.0, 0.18), (88.0, 0.65)],
            Preset::Balanced => [(45.0, 0.0), (65.0, 0.3), (85.0, 0.85)],
            Preset::Cool => [(40.0, 0.15), (58.0, 0.55), (74.0, 1.0)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub mode: Mode,
    /// Fixed speed, fraction of the range from min to max.
    pub fixed: f32,
    pub curve: Curve,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Auto, fixed: 0.4, curve: Curve::default() }
    }
}

impl Settings {
    /// The speed to hold each fan at, `None` = leave it to macOS. `temp` is
    /// the curve's source temperature.
    pub fn targets(&self, fans: &[Fan], temp: Option<f32>) -> Vec<Option<f32>> {
        let frac = match self.mode {
            Mode::Auto => None,
            // cool: let macOS stop the fans; warm: hold them at minimum
            Mode::Quiet => temp.filter(|t| *t >= QUIET_FROM_C).map(|_| 0.0),
            Mode::Fixed => Some(self.fixed.clamp(0.0, 1.0)),
            Mode::Curve => temp.map(|t| self.curve.at(t)),
            Mode::Max => Some(1.0),
        };
        fans.iter().map(|f| frac.map(|k| speed(f, k))).collect()
    }

    /// [`Settings::targets`] on a smoothed temperature, with Quiet's
    /// hysteresis, so a sensor spike doesn't make the fans hunt.
    pub fn steady_targets(&self, fans: &[Fan], temp: Option<f32>, st: &mut Steady) -> Vec<Option<f32>> {
        let t = temp.map(|t| st.smooth(t));
        if self.mode == Mode::Quiet {
            let on = t.is_some_and(|t| t >= QUIET_FROM_C || (st.quiet_on && t >= QUIET_UNTIL_C));
            st.quiet_on = on;
            return fans.iter().map(|f| on.then(|| speed(f, 0.0))).collect();
        }
        st.quiet_on = false;
        self.targets(fans, t)
    }

    fn sanitize(mut self) -> Self {
        self.fixed = if self.fixed.is_finite() { self.fixed.clamp(0.0, 1.0) } else { 0.4 };
        let ok = self.curve.pts.iter().all(|(t, f)| t.is_finite() && f.is_finite()) && self.curve.pts.windows(2).all(|w| w[0].0 < w[1].0);
        if !ok {
            self.curve = Curve::default();
        }
        self
    }
}

/// What the fan loop carries between samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steady {
    /// Smoothed source temperature.
    pub ema: Option<f32>,
    /// Quiet is holding the fans.
    pub quiet_on: bool,
}

impl Steady {
    fn smooth(&mut self, t: f32) -> f32 {
        let v = match self.ema {
            Some(e) if e.is_finite() => e + SMOOTH_ALPHA * (t - e),
            _ => t,
        };
        self.ema = Some(v);
        v
    }
}

/// rpm at `frac` of the fan's range, rounded to 10.
pub fn speed(f: &Fan, frac: f32) -> f32 {
    ((f.min + (f.max - f.min) * frac.clamp(0.0, 1.0)) / 10.0).round() * 10.0
}

/// The 95 °C safety with hysteresis: tripped at [`TRIP_C`], clear below
/// [`RESUME_C`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Guard {
    pub tripped: bool,
    /// The temperature that tripped it.
    pub peak: f32,
}

impl Guard {
    /// Feed the hottest sensor; true while the fans must stay on Auto.
    pub fn update(&mut self, hottest: f32) -> bool {
        if hottest >= TRIP_C {
            if !self.tripped {
                self.peak = hottest;
            }
            self.tripped = true;
            self.peak = self.peak.max(hottest);
        } else if hottest < RESUME_C {
            self.tripped = false;
        }
        self.tripped
    }
}

/// The hottest reading of any sensor that matters for the safety (all but
/// battery, SSD and other).
pub fn hottest(s: &Snapshot) -> Option<f32> {
    s.hottest().map(|h| h.1)
}

fn path() -> PathBuf {
    crate::kit::fs::data_path("fan.json")
}

pub fn load() -> Settings {
    crate::kit::fs::load_json::<Settings>(&path()).sanitize()
}

pub fn save(s: &Settings) {
    crate::kit::fs::save_json(&path(), s, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan() -> Fan {
        Fan { rpm: 2000.0, min: 1200.0, max: 5700.0 }
    }

    #[test]
    fn curve_interpolates_and_holds_ends() {
        let c = Curve { pts: [(40.0, 0.0), (60.0, 0.5), (80.0, 1.0)], source: Source::Hottest };
        assert_eq!(c.at(20.0), 0.0);
        assert_eq!(c.at(40.0), 0.0);
        assert_eq!(c.at(50.0), 0.25);
        assert_eq!(c.at(70.0), 0.75);
        assert_eq!(c.at(99.0), 1.0);
    }

    #[test]
    fn drag_keeps_points_ordered_and_rising() {
        let mut c = Curve { pts: [(40.0, 0.2), (60.0, 0.5), (80.0, 0.9)], source: Source::Hottest };
        c.drag(1, 90.0, 0.0);
        assert_eq!(c.pts[1], (77.0, 0.2));
        c.drag(0, 10.0, 0.9);
        assert_eq!(c.pts[0], (CURVE_LO, 0.2));
        c.drag(2, 120.0, 2.0);
        assert_eq!(c.pts[2], (CURVE_HI, 1.0));
    }

    #[test]
    fn quiet_holds_between_55_and_60_and_spikes_are_smoothed() {
        let fans = [fan()];
        let s = Settings { mode: Mode::Quiet, ..Default::default() };
        let mut st = Steady::default();
        assert_eq!(s.steady_targets(&fans, Some(58.0), &mut st), vec![None]);
        // warm for a while: Quiet takes the fans
        for _ in 0..10 {
            s.steady_targets(&fans, Some(62.0), &mut st);
        }
        assert!(st.quiet_on);
        // back to 57: still held (no flapping around 60)
        for _ in 0..10 {
            s.steady_targets(&fans, Some(57.0), &mut st);
        }
        assert!(st.quiet_on);
        for _ in 0..10 {
            s.steady_targets(&fans, Some(50.0), &mut st);
        }
        assert!(!st.quiet_on);
        // one hot reading moves a curve only part of the way
        let mut c = Settings { mode: Mode::Curve, ..Default::default() };
        c.curve.pts = [(40.0, 0.0), (60.0, 0.5), (80.0, 1.0)];
        let mut st = Steady::default();
        let calm = c.steady_targets(&fans, Some(40.0), &mut st)[0].unwrap();
        let spike = c.steady_targets(&fans, Some(80.0), &mut st)[0].unwrap();
        assert!(spike > calm && spike < speed(&fans[0], 1.0), "{calm} {spike}");
    }

    #[test]
    fn targets_per_mode() {
        let fans = [fan(), Fan { rpm: 0.0, min: 1000.0, max: 2000.0 }];
        let mut s = Settings::default();
        assert_eq!(s.targets(&fans, Some(70.0)), vec![None, None]);
        s.mode = Mode::Quiet;
        assert_eq!(s.targets(&fans, Some(70.0)), vec![Some(1200.0), Some(1000.0)]);
        assert_eq!(s.targets(&fans, Some(45.0)), vec![None, None]);
        s.mode = Mode::Max;
        assert_eq!(s.targets(&fans, None), vec![Some(5700.0), Some(2000.0)]);
        s.mode = Mode::Fixed;
        s.fixed = 0.5;
        assert_eq!(s.targets(&fans, None), vec![Some(3450.0), Some(1500.0)]);
        s.mode = Mode::Curve;
        s.curve.pts = [(40.0, 0.0), (60.0, 0.5), (80.0, 1.0)];
        assert_eq!(s.targets(&fans, Some(60.0)), vec![Some(3450.0), Some(1500.0)]);
        assert_eq!(s.targets(&fans, None), vec![None, None]);
    }

    #[test]
    fn guard_trips_at_95_and_resumes_below_80() {
        let mut g = Guard::default();
        assert!(!g.update(90.0));
        assert!(g.update(96.0));
        assert!(g.update(85.0));
        assert_eq!(g.peak, 96.0);
        assert!(!g.update(79.0));
    }

    #[test]
    fn presets_are_recognised_and_bad_saves_reset() {
        let mut c = Curve::default();
        assert_eq!(c.preset(), Some(Preset::Balanced));
        c.drag(1, 66.0, 0.3);
        assert_eq!(c.preset(), None);
        let bad = Settings { mode: Mode::Curve, fixed: f32::NAN, curve: Curve { pts: [(80.0, 0.0), (60.0, 0.5), (90.0, 1.0)], source: Source::Gpu } }.sanitize();
        assert_eq!(bad.fixed, 0.4);
        assert_eq!(bad.curve, Curve::default());
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), Settings::default());
    }
}
