//! What the Fan Control tool knows: fans, temperature sensors grouped by
//! part of the chip, thermal pressure and a short history. Pure data, so it
//! is testable everywhere.

use egui::Color32;
use std::collections::VecDeque;

/// One fan as the SMC reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Fan {
    pub rpm: f32,
    pub min: f32,
    pub max: f32,
}

impl Fan {
    /// Speed as a fraction of the fan's maximum.
    pub fn frac(&self) -> f32 {
        if self.max > 0.0 {
            (self.rpm / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Part of the Mac a sensor sits in, in the order they are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Group {
    PCores,
    ECores,
    Cpu,
    Gpu,
    Soc,
    Memory,
    Ssd,
    Battery,
    Other,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::PCores => "P-cores",
            Group::ECores => "E-cores",
            Group::Cpu => "CPU",
            Group::Gpu => "GPU",
            Group::Soc => "Chip",
            Group::Memory => "Memory",
            Group::Ssd => "SSD",
            Group::Battery => "Battery",
            Group::Other => "Other",
        }
    }
    /// Groups drawn as blocks of cells in the heat map; the rest are rows.
    pub fn is_core(self) -> bool {
        matches!(
            self,
            Group::PCores | Group::ECores | Group::Cpu | Group::Gpu
        )
    }
}

/// Group of a HID temperature sensor, by its product name. `None` for
/// sensors that aren't worth showing (calibration, duplicates).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn group_of_hid(name: &str) -> Option<Group> {
    let n = name.to_ascii_lowercase();
    if n.contains("tcal") {
        return None;
    }
    Some(if n.starts_with("pacc") {
        Group::PCores
    } else if n.starts_with("eacc") {
        Group::ECores
    } else if n.starts_with("gpu") {
        Group::Gpu
    } else if n.starts_with("nand") || n.contains("ssd") {
        Group::Ssd
    } else if n.contains("battery") || n.contains("gas gauge") {
        Group::Battery
    } else if n.starts_with("dram") || n.contains("mem") {
        Group::Memory
    } else if n.starts_with("soc")
        || n.starts_with("pmgr")
        || n.contains("tdie")
        || n.starts_with("ane")
        || n.starts_with("isp")
    {
        Group::Soc
    } else {
        Group::Other
    })
}

/// Group of an SMC temperature key (`Tp01`, `Te05`, `Tg0f` …). Used for
/// the parts the HID hub doesn't name, which is the cores on newer chips.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn group_of_smc(key: &str) -> Option<Group> {
    let b = key.as_bytes();
    if b.len() != 4 || b[0] != b'T' {
        return None;
    }
    match b[1] {
        b'e' => Some(Group::ECores),
        b'p' | b'f' => Some(Group::Cpu),
        b'g' => Some(Group::Gpu),
        b'B' => Some(Group::Battery),
        b'm' => Some(Group::Memory),
        _ => None,
    }
}

/// A temperature reading.
#[derive(Clone, Debug, PartialEq)]
pub struct Sensor {
    pub name: String,
    pub group: Group,
    pub celsius: f32,
}

/// Whether a reading is a believable temperature.
pub fn plausible(c: f32) -> bool {
    c.is_finite() && (5.0..=130.0).contains(&c)
}

/// One group's summary.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupStat {
    pub group: Group,
    pub max: f32,
    pub avg: f32,
    /// Every sensor's reading, in order (cells of the heat map).
    pub cells: Vec<f32>,
}

/// History length: 10 minutes at one sample every 2 s.
pub const HISTORY: usize = 300;
/// Seconds between samples.
pub const SAMPLE_SECS: u64 = 2;

/// Everything the tool shows.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// False until the first reading arrived.
    pub loaded: bool,
    /// The SMC answered (so "no fans" really means no fans).
    pub has_smc: bool,
    pub fans: Vec<Fan>,
    pub sensors: Vec<Sensor>,
    /// 0 nominal, 1 fair, 2 serious, 3 critical.
    pub thermal: u8,
    /// "Apple M3 Max".
    pub chip: String,
    /// (average fan rpm, hottest °C), oldest first.
    pub history: VecDeque<(f32, f32)>,
}

impl Snapshot {
    /// The SMC works and lists no fans: a fanless Mac.
    pub fn fanless(&self) -> bool {
        self.loaded && self.has_smc && self.fans.is_empty()
    }

    pub fn avg_rpm(&self) -> Option<f32> {
        (!self.fans.is_empty())
            .then(|| self.fans.iter().map(|f| f.rpm).sum::<f32>() / self.fans.len() as f32)
    }

    /// Average fan speed as a fraction of the fans' maximum.
    pub fn avg_frac(&self) -> Option<f32> {
        (!self.fans.is_empty())
            .then(|| self.fans.iter().map(Fan::frac).sum::<f32>() / self.fans.len() as f32)
    }

    /// Per-group summaries, in [`Group`] order. Battery and SSD readings are
    /// left out of "hottest" because they run cool and lag behind.
    pub fn groups(&self) -> Vec<GroupStat> {
        let mut out: Vec<GroupStat> = Vec::new();
        let mut sorted: Vec<&Sensor> = self.sensors.iter().collect();
        sorted.sort_by_key(|s| s.group);
        for s in sorted {
            match out.last_mut() {
                Some(g) if g.group == s.group => g.cells.push(s.celsius),
                _ => out.push(GroupStat {
                    group: s.group,
                    max: 0.0,
                    avg: 0.0,
                    cells: vec![s.celsius],
                }),
            }
        }
        for g in &mut out {
            g.max = g.cells.iter().copied().fold(f32::MIN, f32::max);
            g.avg = g.cells.iter().sum::<f32>() / g.cells.len() as f32;
        }
        out
    }

    /// The hottest group (by its hottest sensor) and that temperature.
    pub fn hottest(&self) -> Option<(Group, f32)> {
        self.groups()
            .into_iter()
            .filter(|g| !matches!(g.group, Group::Battery | Group::Ssd | Group::Other))
            .map(|g| (g.group, g.max))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Add the current reading to the history.
    pub fn push_history(&mut self) {
        let Some((_, hot)) = self.hottest() else {
            return;
        };
        self.history.push_back((self.avg_rpm().unwrap_or(0.0), hot));
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    pub fn thermal_word(&self) -> &'static str {
        ["Nominal", "Fair", "Serious", "Critical"][self.thermal.min(3) as usize]
    }
}

/// Group sensors from the HID hub and fill gaps from SMC keys: a group
/// that HID already covers is taken from HID only.
pub fn merge(hid: Vec<Sensor>, smc: Vec<Sensor>) -> Vec<Sensor> {
    let hid = dedupe(hid);
    let has = |g: Group| hid.iter().any(|s| s.group == g);
    let cores_known = has(Group::PCores) || has(Group::ECores);
    let extra: Vec<Sensor> = smc
        .into_iter()
        .filter(|s| !has(s.group))
        .filter(|s| !(cores_known && s.group == Group::Cpu))
        .collect();
    let mut all = hid;
    all.extend(extra);
    all.retain(|s| plausible(s.celsius));
    all
}

/// The HID hub lists some sensors several times under one name (each
/// `PMU tdieN` three times on M5, the battery six times): keep one per name,
/// averaging the readings that are believable.
pub fn dedupe(sensors: Vec<Sensor>) -> Vec<Sensor> {
    let mut out: Vec<(Sensor, u32)> = Vec::new();
    for s in sensors.into_iter().filter(|s| plausible(s.celsius)) {
        match out.iter_mut().find(|(o, _)| o.name == s.name) {
            Some((o, n)) => {
                o.celsius = (o.celsius * *n as f32 + s.celsius) / (*n + 1) as f32;
                *n += 1;
            }
            None => out.push((s, 1)),
        }
    }
    out.into_iter().map(|(s, _)| s).collect()
}

// ---------------------------------------------------------------- colours

/// Heat scale stops: (°C, colour).
const HEAT: [(f32, [u8; 3]); 5] = [
    (30.0, [0x2F, 0x6E, 0x9E]),
    (50.0, [0x3F, 0xA7, 0xC4]),
    (68.0, [0xE3, 0xC0, 0x4A]),
    (82.0, [0xF0, 0x8A, 0x2C]),
    (95.0, [0xE5, 0x48, 0x3B]),
];

/// Fill colour for a temperature on the heat scale.
pub fn heat(c: f32) -> Color32 {
    let rgb = |s: [u8; 3]| Color32::from_rgb(s[0], s[1], s[2]);
    if c <= HEAT[0].0 {
        return rgb(HEAT[0].1);
    }
    for w in HEAT.windows(2) {
        let ((t0, a), (t1, b)) = (w[0], w[1]);
        if c <= t1 {
            let f = (c - t0) / (t1 - t0);
            let l = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * f).round() as u8;
            return Color32::from_rgb(l(0), l(1), l(2));
        }
    }
    rgb(HEAT[4].1)
}

/// Readable text colour for a temperature (on dark cards).
pub fn heat_text(c: f32) -> Color32 {
    if c >= 90.0 {
        Color32::from_rgb(0xFF, 0x6B, 0x5E)
    } else if c >= 80.0 {
        Color32::from_rgb(0xFF, 0xA2, 0x4D)
    } else if c >= 68.0 {
        Color32::from_rgb(0xF2, 0xD3, 0x6B)
    } else {
        Color32::from_rgb(0xF5, 0xF5, 0xF7)
    }
}

/// "2 140": thousands split by a space.
pub fn rpm_text(rpm: f32) -> String {
    let n = rpm.round().max(0.0) as u64;
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------- sample data

/// A MacBook Pro with two fans after a build (screenshots, `--view fan`).
pub fn demo() -> Snapshot {
    let s = |name: &str, group, c| Sensor {
        name: name.into(),
        group,
        celsius: c,
    };
    let mut sensors = Vec::new();
    for (i, c) in [
        66.0, 69.0, 71.0, 68.0, 64.0, 67.0, 62.0, 65.0, 63.0, 66.0, 61.0, 64.0,
    ]
    .iter()
    .enumerate()
    {
        sensors.push(s(&format!("pACC MTR Temp Sensor{i}"), Group::PCores, *c));
    }
    for (i, c) in [49.0, 52.0, 51.0, 50.0].iter().enumerate() {
        sensors.push(s(&format!("eACC MTR Temp Sensor{i}"), Group::ECores, *c));
    }
    for (i, c) in [58.0, 61.0, 63.0, 60.0, 57.0, 59.0, 62.0, 60.0, 56.0, 58.0]
        .iter()
        .enumerate()
    {
        sensors.push(s(&format!("GPU MTR Temp Sensor{i}"), Group::Gpu, *c));
    }
    sensors.extend([
        s("PMU tdie1", Group::Soc, 57.0),
        s("PMU tdie2", Group::Soc, 55.0),
        s("DRAM", Group::Memory, 58.0),
        s("NAND CH0 temp", Group::Ssd, 41.0),
        s("gas gauge battery", Group::Battery, 33.0),
    ]);
    let rpm = [
        44.0, 42.0, 43.0, 41.0, 45.0, 52.0, 70.0, 88.0, 92.0, 86.0, 78.0, 70.0, 64.0, 60.0, 58.0,
        57.0, 55.0, 56.0, 54.0, 53.0,
    ];
    let hot = [
        58.0, 57.0, 58.0, 57.0, 60.0, 70.0, 82.0, 88.0, 88.0, 85.0, 80.0, 76.0, 73.0, 72.0, 71.0,
        71.0, 70.0, 71.0, 71.0, 71.0,
    ];
    let mut history = VecDeque::new();
    for i in 0..HISTORY {
        let x = i as f32 / (HISTORY - 1) as f32 * (rpm.len() - 1) as f32;
        let (k, f) = (x.floor() as usize, x.fract());
        let lerp = |a: &[f32; 20]| a[k] + (a[(k + 1).min(19)] - a[k]) * f;
        history.push_back((1200.0 + lerp(&rpm) / 100.0 * 4500.0 * 0.5, lerp(&hot)));
    }
    Snapshot {
        loaded: true,
        has_smc: true,
        fans: vec![
            Fan {
                rpm: 2140.0,
                min: 1200.0,
                max: 5700.0,
            },
            Fan {
                rpm: 2310.0,
                min: 1200.0,
                max: 6200.0,
            },
        ],
        sensors,
        thermal: 0,
        chip: "Apple M3 Max".into(),
        history,
    }
}

/// A fanless MacBook Air, a little warm.
pub fn demo_fanless() -> Snapshot {
    let s = |name: &str, group, c| Sensor {
        name: name.into(),
        group,
        celsius: c,
    };
    let mut d = demo();
    d.fans.clear();
    d.chip = "Apple M3".into();
    d.thermal = 1;
    d.sensors = vec![
        s("p1", Group::PCores, 74.0),
        s("p2", Group::PCores, 72.0),
        s("p3", Group::PCores, 73.0),
        s("p4", Group::PCores, 70.0),
        s("e1", Group::ECores, 55.0),
        s("e2", Group::ECores, 54.0),
        s("e3", Group::ECores, 55.0),
        s("e4", Group::ECores, 53.0),
        s("g1", Group::Gpu, 66.0),
        s("g2", Group::Gpu, 64.0),
        s("g3", Group::Gpu, 65.0),
        s("g4", Group::Gpu, 63.0),
        s("n", Group::Ssd, 44.0),
        s("b", Group::Battery, 36.0),
    ];
    for h in d.history.iter_mut() {
        h.0 = 0.0;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_names_map_to_groups() {
        assert_eq!(group_of_hid("pACC MTR Temp Sensor3"), Some(Group::PCores));
        assert_eq!(group_of_hid("eACC MTR Temp Sensor0"), Some(Group::ECores));
        assert_eq!(group_of_hid("GPU MTR Temp Sensor1"), Some(Group::Gpu));
        assert_eq!(group_of_hid("NAND CH0 temp"), Some(Group::Ssd));
        assert_eq!(group_of_hid("gas gauge battery"), Some(Group::Battery));
        assert_eq!(group_of_hid("PMU tdie4"), Some(Group::Soc));
        assert_eq!(group_of_hid("PMU tcal"), None);
        assert_eq!(group_of_hid("PMU TP3w"), Some(Group::Other));
    }

    #[test]
    fn smc_keys_map_to_groups() {
        assert_eq!(group_of_smc("Te05"), Some(Group::ECores));
        assert_eq!(group_of_smc("Tp01"), Some(Group::Cpu));
        assert_eq!(group_of_smc("Tg0f"), Some(Group::Gpu));
        assert_eq!(group_of_smc("TB1T"), Some(Group::Battery));
        assert_eq!(group_of_smc("TW0P"), None);
        assert_eq!(group_of_smc("F0Ac"), None);
        assert_eq!(group_of_smc("Tp0"), None);
    }

    #[test]
    fn merge_prefers_hid_and_drops_junk() {
        let s = |g, c| Sensor {
            name: format!("{g:?}{c}"),
            group: g,
            celsius: c,
        };
        let hid = vec![
            s(Group::PCores, 70.0),
            s(Group::Soc, 0.0),
            s(Group::Soc, 55.0),
        ];
        let smc = vec![
            s(Group::Cpu, 71.0),
            s(Group::Gpu, 60.0),
            s(Group::Soc, 99.0),
            s(Group::Gpu, 300.0),
        ];
        let m = merge(hid, smc);
        let groups: Vec<Group> = m.iter().map(|s| s.group).collect();
        assert_eq!(groups, vec![Group::PCores, Group::Soc, Group::Gpu]);
        // no HID cores: SMC CPU readings are used
        let m = merge(vec![s(Group::Soc, 50.0)], vec![s(Group::Cpu, 71.0)]);
        assert!(m.iter().any(|s| s.group == Group::Cpu));
    }

    #[test]
    fn same_name_hid_sensors_are_averaged() {
        let s = |n: &str, c| Sensor {
            name: n.into(),
            group: Group::Soc,
            celsius: c,
        };
        let d = dedupe(vec![
            s("PMU tdie1", 74.0),
            s("PMU tdie2", 72.0),
            s("PMU tdie1", 76.0),
            s("PMU tdie1", 0.0),
        ]);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].celsius, 75.0);
        assert_eq!(d[1].celsius, 72.0);
    }

    #[test]
    fn groups_and_hottest() {
        let d = demo();
        let g = d.groups();
        assert_eq!(g[0].group, Group::PCores);
        assert_eq!(g[0].max, 71.0);
        assert_eq!(g[0].cells.len(), 12);
        assert_eq!(d.hottest(), Some((Group::PCores, 71.0)));
        assert!(!d.fanless());
        assert!(demo_fanless().fanless());
        assert_eq!(d.avg_rpm(), Some(2225.0));
    }

    #[test]
    fn history_is_capped() {
        let mut d = demo();
        for _ in 0..10 {
            d.push_history();
        }
        assert_eq!(d.history.len(), HISTORY);
        assert_eq!(d.history.back().unwrap().1, 71.0);
    }

    #[test]
    fn heat_scale_and_rpm_text() {
        assert_eq!(heat(10.0), Color32::from_rgb(0x2F, 0x6E, 0x9E));
        assert_eq!(heat(200.0), Color32::from_rgb(0xE5, 0x48, 0x3B));
        assert_eq!(heat(68.0), Color32::from_rgb(0xE3, 0xC0, 0x4A));
        assert_eq!(rpm_text(2140.4), "2 140");
        assert_eq!(rpm_text(980.0), "980");
        assert_eq!(rpm_text(12345.0), "12 345");
    }
}
