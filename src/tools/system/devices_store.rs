//! What the devices list remembers between readings and launches: each
//! device's last known charge (so one that went away still shows, dimmed,
//! with when it was seen) and a week of charge history for the trend and
//! "lasts about" estimate.

use super::devices::Device;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Devices not seen for this long are forgotten.
const FORGET_AFTER: u64 = 30 * 86_400;
/// History kept per device.
const HISTORY_SPAN: u64 = 7 * 86_400;
/// One trend point per this many seconds (56 points a week).
const TREND_STEP: u64 = 3 * 3600;
/// A new history point at least this often, even when nothing changed.
const POINT_EVERY: u64 = 15 * 60;
/// …and no more often than this, even when it did.
const POINT_MIN_GAP: u64 = 60;

/// The last known state of every device that had a battery.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub devices: Vec<Device>,
}

impl Seen {
    pub fn path() -> PathBuf {
        crate::kit::fs::data_path("devices-seen.json")
    }

    /// Remember the connected devices as of `now`. True when the set of
    /// remembered devices changed (worth saving now).
    pub fn update(&mut self, live: &[Device], now: u64) -> bool {
        let mut changed = false;
        for d in live.iter().filter(|d| d.live() && d.battery().is_some()) {
            let mut keep = d.clone();
            keep.seen = Some(now);
            keep.trend.clear();
            keep.eta = None;
            match self.devices.iter_mut().find(|s| s.same(d)) {
                Some(s) => *s = keep,
                None => {
                    self.devices.push(keep);
                    changed = true;
                }
            }
        }
        let before = self.devices.len();
        self.devices.retain(|s| s.seen.is_some_and(|t| now.saturating_sub(t) < FORGET_AFTER));
        // a name-only copy of a connected device ("AirPods Max" next to
        // "Roman's AirPods Max" with its address) is the same one, seen
        // before its address was known
        self.devices.retain(|s| !(s.address.is_empty() && live.iter().any(|d| d.live() && !d.address.is_empty() && d.kind == s.kind && ends_with_name(&d.name, &s.name))));
        changed || self.devices.len() != before
    }

    /// Remembered devices that aren't connected now, as of when last seen.
    /// Charging stops counting once it's away.
    pub fn away(&self, live: &[Device]) -> Vec<Device> {
        self.devices
            .iter()
            .filter(|s| !live.iter().any(|d| d.same(s)))
            .map(|s| Device { charging: false, case_charging: false, ..s.clone() })
            .collect()
    }

    /// Forget one device.
    pub fn forget(&mut self, key: &str) {
        self.devices.retain(|d| d.key() != key);
    }
}

/// `long` ends with `short` as whole words, ignoring case ("Roman's AirPods Max", "AirPods Max").
fn ends_with_name(long: &str, short: &str) -> bool {
    let (l, sh) = (long.to_lowercase(), short.to_lowercase());
    l != sh && l.ends_with(&sh) && l[..l.len() - sh.len()].ends_with([' ', '\u{a0}'])
}

/// Each device's charge over the last week: (unix seconds, percent).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub by: BTreeMap<String, Vec<(u64, u8)>>,
}

impl History {
    pub fn path() -> PathBuf {
        crate::kit::fs::data_path("devices-history.json")
    }

    /// Add a reading. True when a point was added.
    pub fn record(&mut self, key: &str, now: u64, pct: u8) -> bool {
        let pts = self.by.entry(key.to_string()).or_default();
        let add = match pts.last() {
            None => true,
            Some(&(t, v)) => {
                let gap = now.saturating_sub(t);
                gap >= POINT_EVERY || (v != pct && gap >= POINT_MIN_GAP)
            }
        };
        if add {
            pts.push((now, pct));
        }
        pts.retain(|(t, _)| now.saturating_sub(*t) <= HISTORY_SPAN);
        add
    }

    /// Drop devices with no points left in the window.
    pub fn prune(&mut self, now: u64) {
        for pts in self.by.values_mut() {
            pts.retain(|(t, _)| now.saturating_sub(*t) <= HISTORY_SPAN);
        }
        self.by.retain(|_, v| !v.is_empty());
    }

    pub fn forget(&mut self, key: &str) {
        self.by.remove(key);
    }

    /// One point per 3 hours over the week, oldest first, starting at the
    /// first reading; a gap repeats the last known value.
    pub fn trend(&self, key: &str, now: u64) -> Vec<u8> {
        let Some(pts) = self.by.get(key).filter(|p| !p.is_empty()) else { return vec![] };
        let start = now.saturating_sub(HISTORY_SPAN);
        let buckets = (HISTORY_SPAN / TREND_STEP) as usize;
        let mut out: Vec<Option<u8>> = vec![None; buckets];
        for &(t, v) in pts.iter().filter(|(t, _)| *t >= start && *t <= now) {
            let i = (((t - start) / TREND_STEP) as usize).min(buckets - 1);
            out[i] = Some(v);
        }
        let mut last = None;
        out.into_iter()
            .filter_map(|v| {
                last = v.or(last);
                last
            })
            .collect()
    }

    /// How long the charge should last, from the pace of the current
    /// discharge (the readings since it last went up). Needs an hour and
    /// a few percent of it to say anything.
    pub fn eta(&self, key: &str) -> Option<u64> {
        let pts = self.by.get(key)?;
        let &(t1, v1) = pts.last()?;
        let mut t0 = t1;
        let mut v0 = v1;
        for &(t, v) in pts.iter().rev().skip(1) {
            if v < v0 {
                break; // it went up after this: a charge
            }
            t0 = t;
            v0 = v;
        }
        let (span, drop) = (t1.saturating_sub(t0), v0.saturating_sub(v1));
        if span < 3600 || drop < 3 {
            return None;
        }
        let secs = v1 as f64 * span as f64 / drop as f64;
        Some((secs as u64).min(60 * 86_400))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::system::devices::Kind;

    fn dev(name: &str, b: u8) -> Device {
        Device { name: name.into(), kind: Kind::Mouse, main: Some(b), ..Default::default() }
    }

    #[test]
    fn remembers_devices_that_went_away() {
        let mut s = Seen::default();
        assert!(s.update(&[dev("Mouse", 40), Device { name: "Speaker".into(), ..Default::default() }], 100));
        assert_eq!(s.devices.len(), 1, "only devices with a battery");
        assert!(!s.update(&[dev("Mouse", 38)], 200), "same set");
        assert_eq!(s.devices[0].main, Some(38));
        assert!(s.away(&[dev("Mouse", 38)]).is_empty());
        let mut charging = dev("Mouse", 38);
        charging.charging = true;
        s.update(&[charging], 300);
        let away = s.away(&[]);
        assert_eq!((away[0].main, away[0].seen, away[0].charging), (Some(38), Some(300), false));
        assert!(s.update(&[], 300 + FORGET_AFTER), "forgotten after a month");
        assert!(s.devices.is_empty());
    }

    #[test]
    fn drops_a_name_only_copy_of_a_connected_device() {
        let mut s = Seen::default();
        s.update(&[dev("AirPods Max", 80)], 1);
        let mut real = dev("Roman\u{2019}s AirPods Max", 76);
        real.address = "70-f9".into();
        s.update(&[real.clone()], 2);
        assert!(s.away(&[real]).is_empty(), "the copy is gone");
        assert!(ends_with_name("Roman's AirPods Max", "airpods max"));
        assert!(!ends_with_name("MyAirPods Max", "AirPods Max"));
    }

    #[test]
    fn matches_by_address_when_renamed() {
        let mut s = Seen::default();
        let mut a = dev("Mouse", 40);
        a.address = "aa".into();
        s.update(&[a.clone()], 1);
        a.name = "Roman's Mouse".into();
        assert!(s.away(&[a.clone()]).is_empty());
        s.update(&[a], 2);
        assert_eq!(s.devices.len(), 1);
        s.forget("aa");
        assert!(s.devices.is_empty());
    }

    #[test]
    fn history_points_and_window() {
        let mut h = History::default();
        assert!(h.record("m", 0, 90));
        assert!(!h.record("m", 30, 89), "too soon");
        assert!(h.record("m", 90, 89), "changed");
        assert!(!h.record("m", 200, 89), "same value");
        assert!(h.record("m", 90 + POINT_EVERY, 89), "but at least every 15 min");
        h.record("m", HISTORY_SPAN + 2000, 50);
        assert_eq!(h.by["m"].len(), 1, "older than a week dropped");
        h.prune(3 * HISTORY_SPAN);
        assert!(h.by.is_empty());
    }

    #[test]
    fn trend_fills_gaps_and_starts_at_first_reading() {
        let mut h = History::default();
        let now = HISTORY_SPAN * 2;
        let start = now - HISTORY_SPAN;
        h.by.insert("m".into(), vec![(start + 10 * TREND_STEP, 80), (start + 13 * TREND_STEP + 5, 70), (now, 60)]);
        let t = h.trend("m", now);
        assert_eq!(t.len(), 56 - 10);
        assert_eq!(&t[..4], &[80, 80, 80, 70]);
        assert_eq!(*t.last().unwrap(), 60);
        assert!(h.trend("x", now).is_empty());
    }

    #[test]
    fn eta_from_the_current_discharge() {
        let mut h = History::default();
        // charged to 100 at hour 0, 80 at hour 10: 2%/h, 40 h left
        h.by.insert("m".into(), vec![(0, 30), (3600, 100), (3600 * 6, 90), (3600 * 11, 80)]);
        assert_eq!(h.eta("m"), Some(40 * 3600));
        // not enough yet
        h.by.insert("n".into(), vec![(0, 100), (1800, 98)]);
        assert_eq!(h.eta("n"), None);
        // charging now: no estimate
        h.by.insert("c".into(), vec![(0, 40), (3600 * 3, 70)]);
        assert_eq!(h.eta("c"), None);
    }
}
