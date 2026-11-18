//! What the devices list remembers between readings and launches: each
//! device's last known charge (so one that went away still shows, dimmed,
//! with when it was seen) and a week of charge history for the trend and
//! "lasts about" estimate.

use super::devices::{ends_with_name, Device};
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

    /// Give each connected device whose address rotates (AirPods that are
    /// with the iPhone) the identity of the known pair it is, so it keeps
    /// one entry, one history and one set of settings. One no pair is known
    /// for goes by its name, never by the passing address.
    pub fn identify(&self, live: &mut [Device]) {
        for d in live.iter_mut().filter(|d| d.rotating()) {
            match self.devices.iter().find(|s| !s.rotating() && s.same(d)) {
                Some(s) => d.adopt(s),
                None => d.address.clear(),
            }
        }
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
            if keep.rotating() {
                keep.address.clear();
            }
            match self.devices.iter_mut().find(|s| s.same(d)) {
                Some(s) => {
                    // a rotating sighting of a known pair keeps the pair's identity
                    if keep.rotating() && !s.rotating() {
                        keep.adopt(s);
                    }
                    *s = keep
                }
                None => {
                    self.devices.push(keep);
                    changed = true;
                }
            }
        }
        let before = self.devices.len();
        self.drop_rotating_copies();
        self.devices.retain(|s| s.seen.is_some_and(|t| now.saturating_sub(t) < FORGET_AFTER));
        // a name-only copy of a connected device ("AirPods Max" next to
        // "Roman's AirPods Max" with its address) is the same one, seen
        // before its address was known
        self.devices.retain(|s| !(s.address.is_empty() && live.iter().any(|d| d.live() && !d.address.is_empty() && d.kind == s.kind && ends_with_name(&d.name, &s.name))));
        changed || self.devices.len() != before
    }

    /// Older builds stored AirPods with the iPhone once per address they
    /// passed through (13 "AirPods Pro" in an afternoon). Keep one: the
    /// known pair, or else the latest sighting, by name.
    fn drop_rotating_copies(&mut self) {
        let all = &self.devices;
        let drop: Vec<bool> = (0..all.len())
            .map(|i| {
                let s = &all[i];
                s.rotating()
                    && (0..all.len()).any(|j| {
                        let o = &all[j];
                        j != i && o.same(s) && (!o.rotating() || o.seen > s.seen || (o.seen == s.seen && j < i))
                    })
            })
            .collect();
        let mut i = 0;
        self.devices.retain(|_| {
            i += 1;
            !drop[i - 1]
        });
        for s in self.devices.iter_mut().filter(|s| s.rotating()) {
            s.address.clear();
        }
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

    /// Keep only these devices' history. True when something was dropped.
    pub fn keep_only(&mut self, keys: &[String]) -> bool {
        let before = self.by.len();
        self.by.retain(|k, _| keys.contains(k));
        self.by.len() != before
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

    fn pods(addr: &str, product: u16, l: Option<u8>, r: Option<u8>, seen: Option<u64>) -> Device {
        Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, address: addr.into(), product, left: l, right: r, case: Some(87), seen, ..Default::default() }
    }

    /// Roman's devices-seen.json on 2026-11-16: his AirPods Pro (with the
    /// iPhone) stored once per passing address, 13 entries in one afternoon.
    fn romans_seen() -> Seen {
        let mut v = vec![pods("38-c4-3a-6b-d7-a0", 8228, Some(83), Some(84), Some(1794750058))];
        let rot: [(&str, Option<u8>, Option<u8>, u64); 12] = [
            ("d2-7d-79-11-54-4c", None, Some(60), 1794747794),
            ("ed-ec-32-c4-71-fe", None, Some(60), 1794748593),
            ("dd-67-a3-7d-fb-8c", None, Some(60), 1794749456),
            ("f5-31-4e-d9-a1-50", Some(80), Some(80), 1794750367),
            ("e9-b9-62-de-9a-4a", Some(80), Some(80), 1794751230),
            ("c5-86-de-cc-dd-98", Some(80), Some(80), 1794752119),
            ("ed-22-c9-b1-12-f9", Some(80), Some(80), 1794753042),
            ("e1-15-7a-49-d8-37", Some(80), Some(70), 1794753966),
            ("f6-5b-a3-fb-13-cd", Some(80), Some(70), 1794754827),
            ("e0-16-c4-cc-e2-5b", None, Some(50), 1794755751),
            ("c2-16-e3-47-0c-14", Some(80), Some(70), 1794756826),
            ("f6-e5-5c-3f-26-16", Some(70), Some(70), 1794757614),
        ];
        v.extend(rot.iter().map(|&(a, l, r, t)| pods(a, 0, l, r, Some(t))));
        v.insert(1, Device { name: "Roman\u{2019}s AirPods Max".into(), kind: Kind::Headphones, address: "70-f9-4a-b5-0d-68".into(), product: 8237, main: Some(82), seen: Some(1794833787), ..Default::default() });
        v.insert(2, Device { name: "Roman Furman\u{2019}s iPhone (2)".into(), kind: Kind::Phone, main: Some(34), seen: Some(1794832719), ..Default::default() });
        Seen { devices: v }
    }

    #[test]
    fn airpods_under_passing_addresses_are_one_pair() {
        let mut s = romans_seen();
        let now = 1794840000;
        s.update(&[], now);
        let names: Vec<_> = s.devices.iter().map(|d| (d.name.as_str(), d.address.as_str())).collect();
        assert_eq!(names, [("AirPods Pro", "38-c4-3a-6b-d7-a0"), ("Roman\u{2019}s AirPods Max", "70-f9-4a-b5-0d-68"), ("Roman Furman\u{2019}s iPhone (2)", "")]);
        assert_eq!(s.away(&[]).iter().filter(|d| d.name == "AirPods Pro").count(), 1);
    }

    #[test]
    fn a_new_passing_address_is_the_known_pair() {
        let mut s = romans_seen();
        s.update(&[], 1);
        // with the iPhone again, under an address never seen before
        let mut live = vec![pods("c9-01-02-03-04-05", 0, Some(60), Some(60), None)];
        s.identify(&mut live);
        assert_eq!((live[0].address.as_str(), live[0].product), ("38-c4-3a-6b-d7-a0", 8228));
        assert_eq!(live[0].key(), "38-c4-3a-6b-d7-a0", "one history, one set of settings");
        for t in 2..20 {
            // even without identify, update never adds a copy
            s.update(&[pods(&format!("d0-00-00-00-00-{t:02x}"), 0, Some(50), Some(50), None)], t);
        }
        let pairs: Vec<_> = s.devices.iter().filter(|d| d.kind == Kind::Earbuds).collect();
        assert_eq!(pairs.len(), 1);
        assert_eq!((pairs[0].address.as_str(), pairs[0].product, pairs[0].left), ("38-c4-3a-6b-d7-a0", 8228, Some(50)));
        assert!(s.away(&[pods("e7-00-00-00-00-01", 0, Some(50), None, None)]).iter().all(|d| d.kind != Kind::Earbuds), "connected, so not listed as away");
    }

    #[test]
    fn unknown_pods_under_passing_addresses_go_by_name() {
        let mut s = Seen::default();
        for t in 1..10 {
            let mut live = vec![pods(&format!("d0-00-00-00-00-{t:02x}"), 0, Some(80), Some(80), None)];
            s.identify(&mut live);
            assert_eq!(live[0].key(), "airpods pro");
            s.update(&live, t);
        }
        assert_eq!(s.devices.len(), 1);
        assert!(s.devices[0].address.is_empty());
    }

    #[test]
    fn two_real_pairs_with_one_name_stay_two() {
        let mut s = Seen::default();
        s.update(&[pods("aa-00-00-00-00-01", 8228, Some(80), Some(80), None), pods("aa-00-00-00-00-02", 8228, Some(40), Some(40), None)], 1);
        s.update(&[], 2);
        assert_eq!(s.devices.len(), 2, "both have a product id: matched by address");
    }

    /// Roman's Mac on 2026-11-18: the Bluetooth report lists his AirPods
    /// Pro twice, connected with their address and product id, and again
    /// over LE under a passing address with only the case level.
    const ROMANS_PROFILER: &str = r#"{"SPBluetoothDataType":[{"controller_properties":{},"device_connected":[
        {"AirPods Pro":{"device_address":"38:C4:3A:6B:D7:A0","device_vendorID":"0x004C","device_productID":"0x2024","device_batteryLevelCase":"100%","device_batteryLevelLeft":"90%","device_batteryLevelRight":"90%","device_firmwareVersion":"9B47","device_minorType":"Headphones","device_rssi":"-66"}},
        {"AirPods Pro":{"device_address":"EA:31:5C:50:D7:4C","device_batteryLevelCase":"100%","device_firmwareVersion":"0A37k","device_rssi":"-58","device_services":"0x400000 < BLE >"}},
        {"Roman Furman’s iPhone (2)":{"device_address":"98:E8:59:7E:DA:3A","device_rssi":"-43","device_services":"0x400000 < BLE >"}}
    ],"device_not_connected":[
        {"Roman’s AirPods Max":{"device_address":"70:F9:4A:B5:0D:68","device_vendorID":"0x004C","device_productID":"0x202D","device_firmwareVersion":"9B47","device_minorType":"Headphones"}}
    ]}]}"#;

    /// Roman's devices-seen.json on 2026-11-18.
    fn romans_seen_1118() -> Seen {
        Seen {
            devices: vec![
                Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, address: "38-c4-3a-6b-d7-a0".into(), product: 8228, left: Some(91), right: Some(92), case: Some(100), seen: Some(1795021635), ..Default::default() },
                Device { name: "Roman\u{2019}s AirPods Max".into(), kind: Kind::Headphones, address: "70-f9-4a-b5-0d-68".into(), product: 8237, main: Some(26), seen: Some(1795018167), ..Default::default() },
                Device { name: "Roman Furman\u{2019}s iPhone (2)".into(), kind: Kind::Phone, main: Some(70), seen: Some(1795021635), ..Default::default() },
            ],
        }
    }

    /// What the app lists from one reading, as `read_devices` builds it.
    fn reading(s: &mut Seen, profiler: &str, now: u64) -> Vec<Device> {
        let p = super::super::devices::parse_profiler(profiler);
        let mut list = p.connected.clone();
        s.identify(&mut list);
        super::super::devices::merge_extra(&mut list, &p.paired_pods, &[], &[]);
        super::super::devices::collapse(&mut list);
        s.update(&list, now);
        let away = s.away(&list);
        list.extend(away);
        list
    }

    #[test]
    fn a_pair_listed_twice_by_macos_is_one_row() {
        for mut s in [romans_seen_1118(), Seen::default()] {
            for t in 0..3 {
                let list = reading(&mut s, ROMANS_PROFILER, 1795022000 + t * 60);
                let pods: Vec<_> = list.iter().filter(|d| d.name.contains("AirPods Pro")).collect();
                assert_eq!(pods.len(), 1, "one row, not \"L – · R – · Case 100\" next to it: {pods:?}");
                let p = pods[0];
                assert_eq!((p.address.as_str(), p.product, p.left, p.right, p.case, p.live()), ("38-c4-3a-6b-d7-a0", 0x2024, Some(90), Some(90), Some(100), true));
                assert_eq!(p.detail(), "Buds 90 · Case 100");
            }
            assert_eq!(s.devices.iter().filter(|d| d.kind == Kind::Earbuds).count(), 1);
        }
    }

    #[test]
    fn collapse_keeps_different_devices_apart() {
        let mut v = vec![
            pods("aa-00-00-00-00-01", 8228, Some(80), Some(80), None),
            pods("aa-00-00-00-00-02", 8228, Some(40), Some(40), None),
            dev("Mouse", 50),
            Device { name: "Keyboard".into(), kind: Kind::Keyboard, main: Some(60), ..Default::default() },
        ];
        super::super::devices::collapse(&mut v);
        assert_eq!(v.len(), 4, "two real pairs with one name stay two");
        // a passing-address copy with only the case joins the pair it is, wherever it comes
        let mut v = vec![Device { case: Some(100), left: None, right: None, ..pods("ea-31-5c-50-d7-4c", 0, None, None, None) }, pods("38-c4-3a-6b-d7-a0", 8228, Some(90), Some(90), None)];
        v[1].case = None;
        super::super::devices::collapse(&mut v);
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].address.as_str(), v[0].left, v[0].case), ("38-c4-3a-6b-d7-a0", Some(90), Some(100)));
    }

    #[test]
    fn history_of_passing_addresses_is_dropped() {
        let mut h = History::default();
        h.record("38-c4", 1, 80);
        h.record("d2-7d", 1, 60);
        assert!(h.keep_only(&["38-c4".to_string()]));
        assert_eq!(h.by.keys().collect::<Vec<_>>(), ["38-c4"]);
        assert!(!h.keep_only(&["38-c4".to_string()]));
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
