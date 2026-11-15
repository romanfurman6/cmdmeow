//! Charge limit: keep the battery under a set charge while the adapter is
//! in. The fan helper (root) does the switching, so the limit holds while
//! Cmd+Meow is closed; this file holds the rule both sides share and the
//! app's side of asking for it.
//!
//! - Limit: charging stops at the limit.
//! - Sailing: once stopped, it starts again only after the charge drifts
//!   [`SAIL_GAP`] points below the limit (fewer short top-ups).
//! - Heat protection: no charging while the battery is at [`HEAT_C`] or
//!   warmer, until it cools under [`HEAT_RESUME_C`].
//!
//! Unplugged, charging is always allowed again, so nothing is left stuck.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use serde::{Deserialize, Serialize};

/// The limits offered, 100 = off.
pub const LIMITS: [u8; 4] = [100, 70, 80, 90];
/// Anything lower than this is refused (and treated as off).
pub const MIN_LIMIT: u8 = 50;
pub const SAIL_GAP: u8 = 5;
pub const HEAT_C: f32 = 35.0;
pub const HEAT_RESUME_C: f32 = 32.0;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Policy {
    /// Stop charging at this percent; 100 = no limit.
    pub limit: u8,
    pub sailing: bool,
    pub heat: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { limit: 100, sailing: true, heat: true }
    }
}

impl Policy {
    /// A limit is set (sailing and heat protection only work with one).
    pub fn active(&self) -> bool {
        self.limit < 100
    }

    pub fn sanitize(mut self) -> Self {
        if !(MIN_LIMIT..=100).contains(&self.limit) {
            self.limit = 100;
        }
        self
    }

    /// Charging starts again below this percent.
    pub fn resume_below(&self) -> u8 {
        if self.sailing {
            self.limit.saturating_sub(SAIL_GAP)
        } else {
            self.limit
        }
    }
}

/// What the rule needs to know about the battery now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Battery {
    pub percent: u8,
    pub plugged: bool,
    pub temp_c: Option<f32>,
}

/// The rule's memory between readings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Stopped at the limit (sailing down to the resume point).
    pub held: bool,
    /// Stopped because the battery is warm.
    pub hot: bool,
}

impl State {
    /// Charging should be off now.
    pub fn inhibit(&self) -> bool {
        self.held || self.hot
    }
}

/// Update `st` for this reading; true when charging should be off.
pub fn decide(p: &Policy, b: Battery, st: &mut State) -> bool {
    if !p.active() || !b.plugged {
        *st = State::default();
        return false;
    }
    st.hot = p.heat
        && match b.temp_c {
            Some(t) if t >= HEAT_C => true,
            Some(t) if t < HEAT_RESUME_C => false,
            Some(_) => st.hot,
            None => false,
        };
    if b.percent >= p.limit {
        st.held = true;
    } else if b.percent < p.resume_below() {
        st.held = false;
    }
    st.inhibit()
}

/// The internal battery now, from `ioreg` (`None` without one).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn read_battery() -> Option<Battery> {
    let out = std::process::Command::new("/usr/sbin/ioreg").args(["-rn", "AppleSmartBattery"]).output().ok()?;
    parse_ioreg(&String::from_utf8_lossy(&out.stdout))
}

/// The three values the rule needs from `ioreg -rn AppleSmartBattery`.
fn parse_ioreg(out: &str) -> Option<Battery> {
    let mut b = Battery::default();
    let mut seen = false;
    for line in out.lines() {
        let l = line.trim_start_matches([' ', '|']).trim();
        let Some((k, v)) = l.split_once(" = ") else { continue };
        let v = v.trim();
        match k.trim_matches('"') {
            // a percentage on Apple silicon
            "CurrentCapacity" => {
                b.percent = v.parse::<f32>().ok()?.clamp(0.0, 100.0) as u8;
                seen = true;
            }
            "ExternalConnected" => b.plugged = v == "Yes",
            "Temperature" => b.temp_c = v.parse::<f32>().ok().map(|t| t / super::mechanics::BATTERY_TEMP_DIVISOR),
            _ => {}
        }
    }
    seen.then_some(b)
}

/// What the helper says about charging.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Status {
    /// This Mac has keys the helper knows how to switch.
    pub supported: bool,
    pub policy: Option<Policy>,
    pub state: State,
}

/// The app's view of charge control, kept in the Mac Health snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub policy: Policy,
    /// `None` until the helper was asked; `Some(false)` when it isn't
    /// running or is from an older Cmd+Meow.
    pub helper_ok: Option<bool>,
    pub status: Status,
    /// The admin prompt is up.
    pub installing: bool,
}

impl View {
    /// A limit is chosen but the helper can't hold it.
    pub fn needs_helper(&self) -> bool {
        self.policy.active() && self.helper_ok == Some(false)
    }
}

fn path() -> std::path::PathBuf {
    crate::kit::fs::data_path("charge.json")
}

pub fn load() -> Policy {
    crate::kit::fs::load_json::<Policy>(&path()).sanitize()
}

pub fn save(p: &Policy) {
    crate::kit::fs::save_json(&path(), p, true);
}

/// Hand the policy to the helper. `None` if it isn't running; `Some(None)`
/// when it runs but is too old to know charging.
pub fn send(p: &Policy) -> Option<Option<Status>> {
    use super::helper::{ask, Request, VERSION};
    let r = ask(&Request::Charge { policy: *p })?;
    Some((r.version == VERSION).then_some(r.charge))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bat(percent: u8, temp: f32) -> Battery {
        Battery { percent, plugged: true, temp_c: Some(temp) }
    }

    #[test]
    fn off_or_unplugged_always_allows_charging() {
        let mut st = State { held: true, hot: true };
        assert!(!decide(&Policy::default(), bat(95, 40.0), &mut st));
        assert_eq!(st, State::default());
        let p = Policy { limit: 80, ..Default::default() };
        let mut st = State { held: true, hot: true };
        assert!(!decide(&p, Battery { percent: 90, plugged: false, temp_c: Some(40.0) }, &mut st));
        assert_eq!(st, State::default());
    }

    #[test]
    fn stops_at_limit_and_sails_down() {
        let p = Policy { limit: 80, sailing: true, heat: false };
        let mut st = State::default();
        assert!(!decide(&p, bat(79, 30.0), &mut st));
        assert!(decide(&p, bat(80, 30.0), &mut st));
        // sailing: stays off until under 75
        assert!(decide(&p, bat(78, 30.0), &mut st));
        assert!(decide(&p, bat(75, 30.0), &mut st));
        assert!(!decide(&p, bat(74, 30.0), &mut st));
        // and charges up again
        assert!(!decide(&p, bat(77, 30.0), &mut st));
        assert!(decide(&p, bat(80, 30.0), &mut st));
    }

    #[test]
    fn without_sailing_resumes_right_under_the_limit() {
        let p = Policy { limit: 80, sailing: false, heat: false };
        let mut st = State::default();
        assert!(decide(&p, bat(85, 30.0), &mut st));
        assert!(decide(&p, bat(80, 30.0), &mut st));
        assert!(!decide(&p, bat(79, 30.0), &mut st));
    }

    #[test]
    fn heat_protection_has_hysteresis() {
        let p = Policy { limit: 90, sailing: false, heat: true };
        let mut st = State::default();
        assert!(!decide(&p, bat(50, 34.0), &mut st));
        assert!(decide(&p, bat(50, 35.0), &mut st));
        assert!(st.hot);
        assert!(decide(&p, bat(50, 33.0), &mut st));
        assert!(!decide(&p, bat(50, 31.9), &mut st));
        // heat off: warm battery still charges
        let p = Policy { heat: false, ..p };
        assert!(!decide(&p, bat(50, 40.0), &mut st));
        // unknown temperature never blocks
        let p = Policy { heat: true, ..p };
        assert!(!decide(&p, Battery { percent: 50, plugged: true, temp_c: None }, &mut st));
    }

    #[test]
    fn reads_ioreg() {
        let out = "+-o AppleSmartBattery  <class AppleSmartBattery>\n    {\n      \"CurrentCapacity\" = 81\n      \"ExternalConnected\" = Yes\n      \"Temperature\" = 3050\n      \"AppleRawCurrentCapacity\" = 5123\n    }\n";
        let b = parse_ioreg(out).unwrap();
        assert_eq!(b.percent, 81);
        assert!(b.plugged);
        assert_eq!(b.temp_c, Some(3050.0 / crate::kit::mechanics::BATTERY_TEMP_DIVISOR));
        assert!(parse_ioreg("").is_none());
    }

    #[test]
    fn bad_limits_mean_off() {
        assert_eq!(Policy { limit: 10, ..Default::default() }.sanitize().limit, 100);
        assert_eq!(Policy { limit: 150, ..Default::default() }.sanitize().limit, 100);
        assert_eq!(Policy { limit: 80, ..Default::default() }.sanitize().limit, 80);
        assert_eq!(serde_json::from_str::<Policy>("{}").unwrap(), Policy::default());
    }
}
