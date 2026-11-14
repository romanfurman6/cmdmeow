//! Bluetooth device batteries: reading them (the Bluetooth report, the
//! faster input-device registry, AirPods adverts, Bluetooth LE battery
//! service and game controllers), AirPods split into left, right and case,
//! charging state, low and charged alerts, per-device settings and the
//! optional menu bar badge.

use crate::kit::glass::{Icon, AMBER, GREEN, RED, TEXT};
use crate::kit::mechanics as m;
use egui::Color32;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Earbuds,
    Headphones,
    Mouse,
    Keyboard,
    Trackpad,
    Gamepad,
    Phone,
    Tablet,
    #[default]
    Other,
}

impl Kind {
    pub fn icon(self) -> Icon {
        match self {
            Kind::Earbuds => Icon::Earbuds,
            Kind::Headphones => Icon::Headphones,
            Kind::Mouse => Icon::Mouse,
            Kind::Keyboard => Icon::Keyboard,
            Kind::Trackpad => Icon::Trackpad,
            Kind::Gamepad => Icon::Gamepad,
            Kind::Phone => Icon::Phone,
            Kind::Tablet => Icon::Tablet,
            Kind::Other => Icon::Bluetooth,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Earbuds | Kind::Headphones => "Headphones",
            Kind::Mouse => "Mouse",
            Kind::Keyboard => "Keyboard",
            Kind::Trackpad => "Trackpad",
            Kind::Gamepad => "Controller",
            Kind::Phone => "iPhone",
            Kind::Tablet => "iPad",
            Kind::Other => "Device",
        }
    }
    /// Guess the kind from macOS's minor type and the device's name.
    pub fn guess(minor: &str, name: &str) -> Kind {
        let n = name.to_lowercase();
        let t = minor.to_lowercase();
        let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
        if n.contains("iphone") || t == "phone" || t.contains("smartphone") {
            Kind::Phone
        } else if n.contains("ipad") || t.contains("tablet") {
            Kind::Tablet
        } else if n.contains("airpods max") {
            Kind::Headphones
        } else if any(&["airpods", "buds", "beats fit", "powerbeats", "studio buds"]) {
            Kind::Earbuds
        } else if t.contains("gamepad") || t.contains("joystick") || any(&["controller", "dualsense", "dualshock", "xbox", "gamepad", "joy-con", "8bitdo"]) {
            Kind::Gamepad
        } else if t.contains("mouse") || any(&["mouse", "mx master", "mx anywhere"]) {
            Kind::Mouse
        } else if t.contains("trackpad") || n.contains("trackpad") {
            Kind::Trackpad
        } else if t.contains("keyboard") || any(&["keyboard", "keys"]) {
            Kind::Keyboard
        } else if t.contains("head") || t.contains("audio") || t.contains("speaker") || n.contains("headphones") {
            Kind::Headphones
        } else {
            Kind::Other
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub name: String,
    pub kind: Kind,
    /// Bluetooth address, lower case with dashes ("aa-bb-cc-dd-ee-ff"), to
    /// match the sources; empty when unknown.
    pub address: String,
    /// macOS's product id (0x2014 for AirPods Pro 2), which AirPods also
    /// send in their adverts; 0 when unknown.
    pub product: u16,
    /// One battery (mouse, keyboard, over-ear headphones).
    pub main: Option<u8>,
    /// Earbuds report each side and the case separately.
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    /// The main battery, or an earbud, is charging.
    pub charging: bool,
    pub case_charging: bool,
    /// When it was last connected (unix seconds) for a device that isn't
    /// any more; None while connected.
    pub seen: Option<u64>,
    /// Charge over the last week, oldest first (see [`History::trend`]).
    #[serde(skip)]
    pub trend: Vec<u8>,
    /// How long the charge should last at the recent pace, in seconds.
    #[serde(skip)]
    pub eta: Option<u64>,
}

impl Device {
    /// The key settings, the cache and the history use: the address, or
    /// the name when there's none.
    pub fn key(&self) -> String {
        if self.address.is_empty() {
            self.name.to_lowercase()
        } else {
            self.address.clone()
        }
    }
    /// The same physical device (by address, or else by name).
    pub fn same(&self, o: &Device) -> bool {
        if !self.address.is_empty() && !o.address.is_empty() {
            return self.address == o.address;
        }
        self.name.eq_ignore_ascii_case(&o.name)
    }
    pub fn live(&self) -> bool {
        self.seen.is_none()
    }

    /// SF Symbols for this exact model, best first: macOS knows AirPods,
    /// AirPods Max, Beats and Magic accessories by name. Several spellings
    /// are listed because symbol names changed between macOS releases; the
    /// first one this Mac has wins, and the kind's own icon is the fallback.
    pub fn symbols(&self) -> &'static [&'static str] {
        let n = self.name.to_lowercase();
        match self.kind {
            _ if n.contains("airpods max") => &["airpods.max", "airpodsmax"],
            _ if n.contains("airpods pro") => &["airpods.pro", "airpodspro"],
            _ if n.contains("airpods 4") || n.contains("airpods (3") || n.contains("airpods 3") => &["airpods.gen3"],
            _ if n.contains("airpods") => &["airpods"],
            _ if n.contains("beats") && self.kind == Kind::Headphones => &["beats.headphones"],
            _ if n.contains("beats") || n.contains("powerbeats") => &["beats.earphones", "earbuds"],
            Kind::Earbuds => &["earbuds"],
            Kind::Headphones => &["headphones"],
            Kind::Mouse if n.contains("magic mouse") => &["magicmouse"],
            Kind::Mouse => &["computermouse"],
            Kind::Keyboard => &["keyboard"],
            Kind::Trackpad => &["rectangle.and.hand.point.up.left"],
            Kind::Gamepad => &["gamecontroller"],
            Kind::Phone => &["iphone"],
            Kind::Tablet => &["ipad"],
            Kind::Other => &[],
        }
    }

    /// The charge that matters: the single battery, or the emptier earbud.
    /// The case doesn't count (it's not in your ears).
    pub fn battery(&self) -> Option<u8> {
        self.main.or(match (self.left, self.right) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        })
    }
    pub fn split(&self) -> bool {
        self.main.is_none() && (self.left.is_some() || self.right.is_some() || self.case.is_some())
    }
    /// Both earbuds are within a few percent: shown as one number.
    pub fn pods_merged(&self) -> bool {
        self.split() && matches!((self.left, self.right), (Some(l), Some(r)) if l.abs_diff(r) <= m::BT_MERGE_PODS_PCT)
    }
    /// The parts of earbuds: (label, charge, charging). Left and right
    /// become one "Buds" when they're about the same.
    pub fn parts(&self) -> Vec<(&'static str, Option<u8>, bool)> {
        let mut v = if self.pods_merged() {
            vec![("Buds", self.battery(), self.charging)]
        } else {
            vec![("Left", self.left, self.charging), ("Right", self.right, self.charging)]
        };
        v.push(("Case", self.case, self.case_charging));
        v
    }
    /// "L 42 · R 38 · Case 71" (or "Buds 40 · Case 71") for earbuds, the
    /// kind otherwise; "2 h ago" for a device that isn't connected.
    pub fn detail(&self) -> String {
        self.detail_at(now())
    }
    pub fn detail_at(&self, now: u64) -> String {
        if let Some(t) = self.seen {
            return ago(now.saturating_sub(t));
        }
        if !self.split() {
            return self.kind.label().to_string();
        }
        let v = |o: Option<u8>| o.map(|x| x.to_string()).unwrap_or_else(|| "–".into());
        let mut s = if self.pods_merged() { format!("Buds {}", v(self.battery())) } else { format!("L {} · R {}", v(self.left), v(self.right)) };
        if let Some(c) = self.case {
            s.push_str(&format!(" · Case {c}"));
        }
        s
    }
    /// A short word for the menu bar: "AirPods", "Mouse", "Keyboard".
    pub fn short(&self) -> String {
        match self.kind {
            Kind::Earbuds | Kind::Headphones if self.name.contains("AirPods") => "AirPods".into(),
            Kind::Other => self.name.split_whitespace().last().unwrap_or("Device").to_string(),
            k => k.label().to_string(),
        }
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "just now", "12 min ago", "3 h ago", "2 d ago".
pub fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=172_799 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// How long a charge lasts: "<1 h", "5 h", "3 d".
pub fn duration_left(secs: u64) -> String {
    match secs {
        0..=3599 => "<1 h".into(),
        3600..=172_799 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// Green above the low line, amber when low, red when almost empty.
pub fn color(pct: u8) -> Color32 {
    if pct <= m::BT_CRITICAL_BATTERY_MAX_PCT {
        RED()
    } else if pct <= m::BT_LOW_BATTERY_MAX_PCT {
        AMBER()
    } else {
        GREEN()
    }
}

/// Text colour of a percentage: plain unless low.
pub fn text_color(pct: u8) -> Color32 {
    if pct <= m::BT_LOW_BATTERY_MAX_PCT {
        color(pct)
    } else {
        TEXT()
    }
}

pub fn is_low(d: &Device) -> bool {
    d.live() && !d.charging && d.battery().is_some_and(|b| b <= m::BT_LOW_BATTERY_MAX_PCT)
}

/// Connected devices first, emptiest first (unknown last), then the ones
/// that aren't connected, most recent first.
pub fn lowest_first(devs: &[Device]) -> Vec<Device> {
    let mut v = devs.to_vec();
    v.sort_by_key(|d| match d.seen {
        None => (0, d.battery().unwrap_or(u8::MAX) as u64),
        Some(t) => (1, u64::MAX - t),
    });
    v
}

fn norm_addr(a: &str) -> String {
    a.trim().to_lowercase().replace(':', "-")
}

fn pct(v: &Value) -> Option<u8> {
    match v {
        Value::String(s) => s.trim().trim_end_matches('%').trim().parse::<u8>().ok().map(|x| x.min(100)),
        Value::Number(n) => n.as_u64().map(|x| x.min(100) as u8),
        _ => None,
    }
}

/// "0x2014" (or "2014") as a number.
fn product_id(v: &Value) -> u16 {
    v.as_str().and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches("0X"), 16).ok()).unwrap_or(0)
}

fn device_of(name: &str, info: &Value) -> Device {
    let minor = info["device_minorType"].as_str().unwrap_or("");
    let mut d = Device {
        name: name.to_string(),
        kind: Kind::guess(minor, name),
        address: norm_addr(info["device_address"].as_str().unwrap_or("")),
        product: product_id(&info["device_productID"]),
        main: pct(&info["device_batteryLevelMain"]).or_else(|| pct(&info["device_batteryLevel"])),
        left: pct(&info["device_batteryLevelLeft"]),
        right: pct(&info["device_batteryLevelRight"]),
        case: pct(&info["device_batteryLevelCase"]),
        ..Default::default()
    };
    // over-ear headphones that only report "Main" stay single
    if d.kind == Kind::Earbuds && d.main.is_some() && (d.left.is_some() || d.right.is_some()) {
        d.main = None;
    }
    d
}

fn profiler_list(v: &Value, key: &str) -> Vec<Device> {
    let mut list = Vec::new();
    let Some(arr) = v["SPBluetoothDataType"][0][key].as_array() else { return list };
    for entry in arr {
        let Some(obj) = entry.as_object() else { continue };
        for (name, info) in obj {
            list.push(device_of(name, info));
        }
    }
    list
}

/// What `system_profiler SPBluetoothDataType -json` says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// Connected accessories. iPhone and iPad are left out: their battery
    /// comes from Bluetooth LE ([`Extra`]), not from here.
    pub connected: Vec<Device>,
    /// Paired but not connected earbuds: AirPods with the case open nearby
    /// are matched to these by product id.
    pub paired_pods: Vec<Device>,
}

pub fn parse_profiler(json: &str) -> Profile {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let connected = profiler_list(&v, "device_connected").into_iter().filter(|d| !matches!(d.kind, Kind::Phone | Kind::Tablet)).collect();
    let paired_pods = profiler_list(&v, "device_not_connected").into_iter().filter(|d| d.kind == Kind::Earbuds && d.product != 0).collect();
    Profile { connected, paired_pods }
}

/// One input device's battery from the HID registry.
#[derive(Clone, Debug, PartialEq)]
pub struct Hid {
    pub product: String,
    pub address: String,
    pub pct: u8,
    pub charging: Option<bool>,
}

/// Batteries from `ioreg -r -l -k BatteryPercent` (and `BatteryPercentSingle`,
/// which some other makers' devices use): Apple keyboards, mice and
/// trackpads and more, read without waiting for the slower Bluetooth report.
pub fn parse_ioreg(out: &str) -> Vec<Hid> {
    #[derive(Default)]
    struct Cur {
        product: Option<String>,
        address: Option<String>,
        pct: Option<u8>,
        flags: Option<u32>,
    }
    let mut list: Vec<Hid> = Vec::new();
    let mut cur = Cur::default();
    let flush = |c: &mut Cur, list: &mut Vec<Hid>| {
        if let (Some(p), Some(b)) = (c.product.take(), c.pct.take()) {
            let address = c.address.take().map(|a| norm_addr(&a)).unwrap_or_default();
            // BatteryStatusFlags: bit 0-1 set while charging, 4 = on battery
            let charging = c.flags.map(|f| f & 0b11 != 0);
            let h = Hid { product: p, address, pct: b.min(100), charging };
            // the two queries can list the same device
            if !list.iter().any(|x| x.product == h.product && x.address == h.address) {
                list.push(h);
            }
        }
        *c = Cur::default();
    };
    for line in out.lines() {
        let t = line.trim_start_matches([' ', '|']).trim();
        if t.starts_with("+-o") {
            flush(&mut cur, &mut list);
            continue;
        }
        let Some((k, v)) = t.split_once(" = ") else { continue };
        let k = k.trim().trim_matches('"');
        let v = v.trim();
        match k {
            "Product" => cur.product = Some(v.trim_matches('"').to_string()),
            "DeviceAddress" | "SerialNumber" if cur.address.is_none() && v.contains(['-', ':']) => cur.address = Some(v.trim_matches('"').to_string()),
            "BatteryPercent" | "BatteryPercentSingle" if cur.pct.is_none() => cur.pct = v.parse::<u8>().ok(),
            "BatteryStatusFlags" => cur.flags = v.parse::<u32>().ok(),
            _ => {}
        }
    }
    flush(&mut cur, &mut list);
    list
}

/// Put the fresher HID readings into the list: matched by address, then
/// by product name; unmatched ones are added.
pub fn merge(list: &mut Vec<Device>, hid: &[Hid]) {
    for h in hid {
        let at = list
            .iter()
            .position(|d| !h.address.is_empty() && d.address == h.address)
            .or_else(|| list.iter().position(|d| d.name.eq_ignore_ascii_case(&h.product) || d.name.ends_with(&h.product)));
        match at {
            Some(i) => {
                if !list[i].split() {
                    list[i].main = Some(h.pct);
                }
                if let Some(c) = h.charging {
                    list[i].charging = c;
                }
            }
            None => list.push(Device {
                name: h.product.clone(),
                kind: Kind::guess("", &h.product),
                address: h.address.clone(),
                main: Some(h.pct),
                charging: h.charging.unwrap_or(false),
                ..Default::default()
            }),
        }
    }
}

// --------------------------------------------------------- AirPods adverts

/// What AirPods (and Beats) say in their Bluetooth LE adverts while the
/// case is open or they're out: 10% steps per part, and what's charging.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PodsAdvert {
    pub model: u16,
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    pub left_charging: bool,
    pub right_charging: bool,
    pub case_charging: bool,
    pub rssi: i16,
}

/// Read Apple's "proximity pairing" message out of an advert's
/// manufacturer data (company id 0x004C first). Layout after the company
/// id: type 0x07, length, prefix, model (2 bytes), status, pod levels
/// (one nibble each), charging flags and case level (a nibble each).
/// A level nibble is tens of percent; 15 means "not there".
pub fn parse_proximity(data: &[u8], rssi: i16) -> Option<PodsAdvert> {
    let d = data.strip_prefix(&[0x4c, 0x00])?;
    if d.len() < 9 || d[0] != 0x07 {
        return None;
    }
    let level = |n: u8| (n <= 10).then_some(n * 10);
    // little-endian: AirPods Pro 2 send 14 20 for product 0x2014
    let model = u16::from_le_bytes([d[3], d[4]]);
    let status = d[5];
    // which nibble is the left pod flips with the pod that's sending
    let flipped = status & 0x20 == 0;
    let (hi, lo) = (d[6] >> 4, d[6] & 0x0f);
    let (l, r) = if flipped { (lo, hi) } else { (hi, lo) };
    let flags = d[7] >> 4;
    let (lc, rc) = if flipped { (flags & 0b10 != 0, flags & 0b01 != 0) } else { (flags & 0b01 != 0, flags & 0b10 != 0) };
    let case = level(d[7] & 0x0f);
    let a = PodsAdvert { model, left: level(l), right: level(r), case, left_charging: lc && level(l).is_some(), right_charging: rc && level(r).is_some(), case_charging: flags & 0b100 != 0 && case.is_some(), rssi };
    (a.left.is_some() || a.right.is_some() || a.case.is_some()).then_some(a)
}

/// A battery from elsewhere: the Bluetooth LE battery service (iPhone,
/// iPad, other makers' devices) or a game controller.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub name: String,
    pub pct: u8,
    pub charging: Option<bool>,
    /// Bluetooth address ("aa-bb-…") when the source knows it: matched
    /// first, since IOBluetooth's name ("AirPods Max") can differ from the
    /// system report's ("Roman's AirPods Max").
    pub address: String,
}

/// Adverts this weak are someone else's AirPods.
pub const NEAR_RSSI: i16 = -65;

/// Add what Bluetooth LE and game controllers know:
/// - an advert for connected AirPods fills the case and charging state
///   (macOS reports exact levels but not what's charging);
/// - an advert from paired AirPods that aren't connected (case open on
///   the desk) shows them;
/// - batteries from the LE battery service and controllers are added, or
///   fill a device that had none.
pub fn merge_extra(list: &mut Vec<Device>, paired_pods: &[Device], adverts: &[PodsAdvert], extra: &[Extra]) {
    let mut best: HashMap<u16, PodsAdvert> = HashMap::new();
    for a in adverts.iter().filter(|a| a.rssi >= NEAR_RSSI) {
        if best.get(&a.model).is_none_or(|b| a.rssi > b.rssi) {
            best.insert(a.model, *a);
        }
    }
    let mut models: Vec<_> = best.into_values().collect();
    models.sort_by_key(|a| a.model);
    for a in models {
        // the connected pair of this model; macOS sometimes reports no
        // product id for it, so a lone connected pair without one counts too
        // (Apple's only: other earbuds must never take a stranger's advert)
        let apple = |d: &Device| {
            let n = d.name.to_lowercase();
            n.contains("airpods") || n.contains("beats")
        };
        let unknown: Vec<usize> = (0..list.len()).filter(|&i| list[i].kind == Kind::Earbuds && list[i].product == 0 && list[i].live() && apple(&list[i])).collect();
        let conn = list.iter().position(|d| d.product == a.model && d.kind == Kind::Earbuds && d.live()).or(match unknown[..] {
            [i] => Some(i),
            _ => None,
        });
        if let Some(i) = conn {
            let d = &mut list[i];
            if d.main.is_none() {
                d.left = d.left.or(a.left);
                d.right = d.right.or(a.right);
                d.case = d.case.or(a.case);
                d.charging = a.left_charging || a.right_charging;
                d.case_charging = a.case_charging;
            }
        } else if let [p] = paired_pods.iter().filter(|p| p.product == a.model).collect::<Vec<_>>()[..] {
            // only when exactly one paired pair is that model, and it isn't
            // the connected pair listed under another address
            if !list.iter().any(|d| d.same(p) || (d.kind == Kind::Earbuds && d.live() && d.name.eq_ignore_ascii_case(&p.name))) {
                list.push(Device { left: a.left, right: a.right, case: a.case, charging: a.left_charging || a.right_charging, case_charging: a.case_charging, ..p.clone() });
            }
        }
    }
    for e in extra {
        let at = list.iter().position(|d| !e.address.is_empty() && d.address == e.address).or_else(|| list.iter().position(|d| d.name.eq_ignore_ascii_case(&e.name)));
        match at.map(|i| &mut list[i]) {
            Some(d) => {
                if d.battery().is_none() {
                    d.main = Some(e.pct);
                }
                if let Some(c) = e.charging {
                    d.charging = c;
                }
            }
            None => list.push(Device { name: e.name.clone(), kind: Kind::guess("", &e.name), main: Some(e.pct), charging: e.charging.unwrap_or(false), ..Default::default() }),
        }
    }
}

// ------------------------------------------------------------------ alerts

/// Remembers which alerts went out so each device alerts once per step:
/// once when low, once more when almost empty. It forgets a device once it
/// is charged past the reset line, so hovering around 20% doesn't nag.
/// Charging devices never get a low alert; they get one "charged" alert
/// when they reach the chosen level, if that's on.
/// One notification: a low-battery one (sent while low alerts are on) or
/// a charged one (sent while the charged alert is on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub charged: bool,
    pub title: String,
    pub body: String,
}

#[derive(Default)]
pub struct Alerts {
    fired: HashMap<String, u8>, // device key -> 1 low, 2 critical
    charged: HashMap<String, u8>, // device key -> the level it was announced at
}

impl Alerts {
    /// The notifications to send now.
    pub fn check(&mut self, devs: &[Device], prefs: &Prefs) -> Vec<Note> {
        let mut out = Vec::new();
        for d in devs.iter().filter(|d| d.live()) {
            let Some(b) = d.battery() else { continue };
            let key = d.key();
            if prefs.device(&key).hidden {
                continue;
            }
            // charged
            if d.charging {
                if prefs.charged && b >= prefs.charged_at && self.charged.get(&key) != Some(&prefs.charged_at) {
                    self.charged.insert(key.clone(), prefs.charged_at);
                    out.push(Note { charged: true, title: format!("{} charged", d.name), body: format!("At {b}%") });
                }
            } else {
                self.charged.remove(&key);
            }
            // low
            let below = prefs.alert_below(&key);
            if b >= below.saturating_add(10).max(m::BT_ALERT_RESET_PCT) {
                self.fired.remove(&key);
                continue;
            }
            if d.charging {
                continue;
            }
            let step = if below > m::BT_CRITICAL_BATTERY_MAX_PCT + 1 && b <= m::BT_CRITICAL_BATTERY_MAX_PCT {
                2
            } else if b < below {
                1
            } else {
                0
            };
            let had = self.fired.get(&key).copied().unwrap_or(0);
            if step > had {
                self.fired.insert(key, step);
                let verb = if d.kind == Kind::Earbuds || d.name.ends_with('s') { "are" } else { "is" };
                let title = format!("{} {verb} at {b}%", d.name);
                let body = if d.split() {
                    let side = match (d.left, d.right) {
                        (Some(l), Some(r)) if l < r && !d.pods_merged() => "Left AirPod is almost empty. ",
                        (Some(l), Some(r)) if r < l && !d.pods_merged() => "Right AirPod is almost empty. ",
                        _ => "",
                    };
                    format!("{side}Put them in the case.")
                } else if step == 2 {
                    "Almost empty. Charge it now.".to_string()
                } else {
                    "Charge it soon.".to_string()
                };
                out.push(Note { charged: false, title, body });
            }
        }
        out
    }
}

// ------------------------------------------------------------------- prefs

/// One device's own settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevicePrefs {
    /// Left out of the widget, the menu bar and alerts.
    pub hidden: bool,
    /// Alert below this charge; 0 = never; None = the default (20%).
    pub alert_below: Option<u8>,
}

/// The choices for a device's low alert.
pub const ALERT_CHOICES: [u8; 5] = [0, 10, 20, 30, 50];
/// The choices for the charged alert.
pub const CHARGED_CHOICES: [u8; 3] = [80, 90, 100];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// macOS notification when a device runs low.
    pub alerts: bool,
    /// Show the emptiest device in the menu bar while it is low.
    pub menu_bar: bool,
    /// macOS notification when a charging device reaches `charged_at`.
    pub charged: bool,
    pub charged_at: u8,
    /// Per device, by [`Device::key`].
    pub devices: BTreeMap<String, DevicePrefs>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { alerts: true, menu_bar: true, charged: false, charged_at: 100, devices: BTreeMap::new() }
    }
}

fn prefs_path() -> PathBuf {
    crate::kit::fs::data_path("devices.json")
}

impl Prefs {
    pub fn load() -> Prefs {
        crate::kit::fs::load_json(&prefs_path())
    }
    pub fn save(&self) {
        crate::kit::fs::save_json(&prefs_path(), self, true);
    }
    pub fn device(&self, key: &str) -> DevicePrefs {
        self.devices.get(key).copied().unwrap_or_default()
    }
    pub fn alert_below(&self, key: &str) -> u8 {
        self.device(key).alert_below.unwrap_or(m::BT_LOW_BATTERY_MAX_PCT + 1)
    }
    /// Change one device's settings; defaults aren't stored.
    pub fn set_device(&mut self, key: &str, f: impl FnOnce(&mut DevicePrefs)) {
        let mut d = self.device(key);
        f(&mut d);
        if d.alert_below == Some(m::BT_LOW_BATTERY_MAX_PCT + 1) {
            d.alert_below = None;
        }
        if d == DevicePrefs::default() {
            self.devices.remove(key);
        } else {
            self.devices.insert(key.to_string(), d);
        }
    }
    /// Split a list into (shown, hidden).
    pub fn split(&self, all: Vec<Device>) -> (Vec<Device>, Vec<Device>) {
        all.into_iter().partition(|d| !self.device(&d.key()).hidden)
    }
}

/// The menu bar text: the emptiest connected device while it is below its
/// alert line and not charging ("Mouse 14%"), nothing otherwise.
pub fn tray_title(devs: &[Device], prefs: &Prefs) -> Option<String> {
    if !prefs.menu_bar {
        return None;
    }
    let d = lowest_first(devs).into_iter().find(|d| {
        d.live() && !d.charging && !prefs.device(&d.key()).hidden && d.battery().is_some_and(|b| b < prefs.alert_below(&d.key()))
    })?;
    Some(format!("{} {}%", d.short(), d.battery()?))
}

/// Opens System Settings at Bluetooth.
pub fn open_bluetooth_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.BluetoothSettings").spawn();
    }
}

/// The devices the demo screenshots show.
pub fn demo() -> Vec<Device> {
    let trend = |from: u8, to: u8| (0..40).map(|i| (from as i32 + (to as i32 - from as i32) * i / 39) as u8).collect::<Vec<u8>>();
    vec![
        Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, left: Some(42), right: Some(38), case: Some(71), case_charging: true, ..Default::default() },
        Device { name: "Magic Keyboard".into(), kind: Kind::Keyboard, main: Some(82), trend: trend(100, 82), eta: Some(19 * 86_400), ..Default::default() },
        Device { name: "MX Master 3S".into(), kind: Kind::Mouse, main: Some(14), trend: trend(60, 14), eta: Some(30 * 3600), ..Default::default() },
        Device { name: "Magic Trackpad".into(), kind: Kind::Trackpad, main: Some(67), charging: true, ..Default::default() },
        Device { name: "Beats Studio Pro".into(), kind: Kind::Headphones, main: Some(55), seen: Some(now() - 3 * 3600), ..Default::default() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_symbols() {
        let d = |n: &str| Device { name: n.into(), kind: Kind::guess("", n), ..Default::default() };
        assert_eq!(d("Roman's AirPods Max").symbols()[0], "airpods.max");
        assert_eq!(d("AirPods Pro").symbols()[0], "airpods.pro");
        assert_eq!(d("AirPods").symbols(), &["airpods"]);
        assert_eq!(d("Magic Mouse").symbols(), &["magicmouse"]);
        assert_eq!(d("MX Master 3S").symbols(), &["computermouse"]);
        assert_eq!(d("Magic Keyboard").symbols(), &["keyboard"]);
        assert_eq!(d("DualSense Wireless Controller").symbols(), &["gamecontroller"]);
        assert_eq!(d("Roman's iPhone").symbols(), &["iphone"]);
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::guess("", "Xbox Wireless Controller"), Kind::Gamepad);
        assert_eq!(Kind::guess("Gamepad", "Pro"), Kind::Gamepad);
        assert_eq!(Kind::guess("", "Roman's iPad"), Kind::Tablet);
        assert_eq!(Kind::guess("Headphones", "Sony WH-1000XM5"), Kind::Headphones);
        assert_eq!(Kind::guess("", "Galaxy Buds2"), Kind::Earbuds);
    }

    const PROFILER: &str = r#"{"SPBluetoothDataType":[{"controller_properties":{},"device_connected":[
        {"Alex's AirPods Pro":{"device_address":"AA:BB:CC:DD:EE:01","device_batteryLevelCase":"71%","device_batteryLevelLeft":"42%","device_batteryLevelRight":"38%","device_minorType":"Headphones","device_productID":"0x2014"}},
        {"Magic Keyboard":{"device_address":"AA:BB:CC:DD:EE:02","device_batteryLevelMain":"82%","device_minorType":"Keyboard"}},
        {"MX Master 3S":{"device_address":"AA:BB:CC:DD:EE:03","device_minorType":"Mouse"}},
        {"Alex's iPhone":{"device_address":"AA:BB:CC:DD:EE:04","device_minorType":"Smartphone"}},
        {"AirPods Max":{"device_batteryLevelMain":"37%","device_minorType":"Headphones"}}
    ],"device_not_connected":[
        {"Old AirPods":{"device_address":"AA:BB:CC:DD:EE:09","device_minorType":"Headphones","device_productID":"0x200F"}},
        {"Sony WH-1000XM5":{"device_address":"AA:BB:CC:DD:EE:0A","device_minorType":"Headphones"}}
    ]}]}"#;

    #[test]
    fn profiler_splits_airpods_and_skips_phones() {
        let p = parse_profiler(PROFILER);
        let v = &p.connected;
        assert_eq!(v.len(), 4);
        let pods = &v[0];
        assert_eq!(pods.kind, Kind::Earbuds);
        assert_eq!(pods.product, 0x2014);
        assert_eq!((pods.left, pods.right, pods.case), (Some(42), Some(38), Some(71)));
        assert_eq!(pods.battery(), Some(38));
        assert_eq!(pods.detail(), "Buds 38 · Case 71");
        assert_eq!(pods.address, "aa-bb-cc-dd-ee-01");
        assert_eq!(v[1].kind, Kind::Keyboard);
        assert_eq!(v[1].battery(), Some(82));
        assert_eq!(v[2].kind, Kind::Mouse);
        assert_eq!(v[2].battery(), None);
        assert_eq!(v[3].kind, Kind::Headphones);
        assert_eq!(v[3].battery(), Some(37));
        assert!(!v[3].split());
        assert_eq!(p.paired_pods.len(), 1);
        assert_eq!((p.paired_pods[0].name.as_str(), p.paired_pods[0].product), ("Old AirPods", 0x200f));
    }

    #[test]
    fn one_airpod_in() {
        let d = Device { name: "AirPods".into(), kind: Kind::Earbuds, left: Some(51), ..Default::default() };
        assert_eq!(d.battery(), Some(51));
        assert_eq!(d.detail(), "L 51 · R –");
    }

    #[test]
    fn close_pods_merge() {
        let mut d = Device { name: "AirPods".into(), kind: Kind::Earbuds, left: Some(42), right: Some(38), case: Some(71), ..Default::default() };
        assert!(d.pods_merged());
        assert_eq!(d.detail(), "Buds 38 · Case 71");
        assert_eq!(d.parts().iter().map(|p| p.0).collect::<Vec<_>>(), ["Buds", "Case"]);
        d.left = Some(80);
        assert!(!d.pods_merged());
        assert_eq!(d.parts().len(), 3);
    }

    #[test]
    fn device_away_shows_when_it_was_seen() {
        let d = Device { name: "Mouse".into(), main: Some(40), seen: Some(1000), ..Default::default() };
        assert_eq!(d.detail_at(1000 + 2 * 3600 + 5), "2 h ago");
        assert_eq!(d.detail_at(1030), "just now");
        assert_eq!(ago(3 * 86_400), "3 d ago");
        assert_eq!(duration_left(5 * 3600 + 10), "5 h");
        assert_eq!(duration_left(80 * 3600), "3 d");
        assert!(!is_low(&Device { main: Some(5), ..d }));
    }

    const IOREG: &str = r#"+-o AppleDeviceManagementHIDEventService  <class AppleDeviceManagementHIDEventService, id 0x100000a1b>
    {
      "BatteryStatusFlags" = 3
      "Product" = "Magic Keyboard"
      "DeviceAddress" = "aa-bb-cc-dd-ee-02"
      "BatteryPercent" = 79
    }
+-o AppleDeviceManagementHIDEventService  <class AppleDeviceManagementHIDEventService, id 0x100000a1c>
    {
      "BatteryStatusFlags" = 4
      "Product" = "Magic Trackpad"
      "BatteryPercent" = 55
    }
+-o SomeHeadset  <class IOService, id 0x100000a1d>
    {
      "Product" = "Jabra Evolve2"
      "BatteryPercentSingle" = 61
    }
"#;

    #[test]
    fn ioreg_is_merged_by_address_and_adds_new_devices() {
        let hid = parse_ioreg(IOREG);
        assert_eq!(hid.len(), 3);
        assert_eq!(hid[0], Hid { product: "Magic Keyboard".into(), address: "aa-bb-cc-dd-ee-02".into(), pct: 79, charging: Some(true) });
        assert_eq!(hid[1].charging, Some(false));
        assert_eq!((hid[2].pct, hid[2].charging), (61, None));
        let mut v = parse_profiler(PROFILER).connected;
        merge(&mut v, &hid);
        assert_eq!(v[1].battery(), Some(79));
        assert!(v[1].charging);
        let pad = v.iter().find(|d| d.name == "Magic Trackpad").unwrap();
        assert_eq!((pad.kind, pad.battery(), pad.charging), (Kind::Trackpad, Some(55), false));
        assert!(v.iter().any(|d| d.name == "Jabra Evolve2" && d.battery() == Some(61)));
    }

    #[test]
    fn ioreg_twice_lists_a_device_once() {
        let twice = format!("{IOREG}{IOREG}");
        assert_eq!(parse_ioreg(&twice).len(), 3);
    }

    /// manufacturer data: 4c 00 | 07 19 01 | model | status | pods | flags+case | ...
    fn advert(model: u16, status: u8, pods: u8, flags_case: u8) -> Vec<u8> {
        let [a, b] = model.to_le_bytes();
        let mut v = vec![0x4c, 0x00, 0x07, 0x19, 0x01, a, b, status, pods, flags_case, 0x00, 0x00];
        v.resize(2 + 27, 0);
        v
    }

    #[test]
    fn proximity_message() {
        // not flipped (status bit 0x20 set): left = high nibble
        let a = parse_proximity(&advert(0x2014, 0x2b, 0x86, 0x57), -50).unwrap();
        assert_eq!((a.model, a.left, a.right, a.case), (0x2014, Some(80), Some(60), Some(70)));
        // flags 0b0101: left and case charging
        assert!(a.left_charging && !a.right_charging && a.case_charging);
        // flipped: left = low nibble, charging bits swap
        let b = parse_proximity(&advert(0x2014, 0x0b, 0x86, 0x17), -50).unwrap();
        assert_eq!((b.left, b.right), (Some(60), Some(80)));
        assert!(!b.left_charging && b.right_charging);
        // 15 = not there; a pod that isn't there isn't charging
        let c = parse_proximity(&advert(0x200e, 0x2b, 0xf9, 0x3f), -50).unwrap();
        assert_eq!((c.left, c.right, c.case), (None, Some(90), None));
        assert!(!c.left_charging && c.right_charging && !c.case_charging);
        // nothing known, other messages, other companies, too short
        assert!(parse_proximity(&advert(0x2014, 0x2b, 0xff, 0x0f), -50).is_none());
        assert!(parse_proximity(&[0x4c, 0x00, 0x10, 0x05, 0x01, 0x18, 0x00, 0, 0, 0, 0], -50).is_none());
        let mut other = advert(0x2014, 0x2b, 0x86, 0x57);
        other[0] = 0x06;
        assert!(parse_proximity(&other, -50).is_none());
        assert!(parse_proximity(&[0x4c, 0x00, 0x07, 0x19], -50).is_none());
    }

    #[test]
    fn adverts_fill_connected_pods_and_show_paired_ones() {
        let p = parse_profiler(PROFILER);
        let mut v = p.connected.clone();
        let near = |model, pods, fc, rssi| PodsAdvert { rssi, ..parse_proximity(&advert(model, 0x2b, pods, fc), rssi).unwrap() };
        let ads = [
            near(0x2014, 0x44, 0x57, -50), // the connected pair: charging info only
            near(0x200f, 0x99, 0x0a, -55), // the paired pair, case open
            near(0x2024, 0x99, 0x0a, -40), // not ours
            near(0x200f, 0x11, 0x01, -80), // someone else's, far away
        ];
        let extra = [Extra { name: "Alex's iPhone".into(), pct: 64, charging: Some(true), address: String::new() }, Extra { name: "MX Master 3S".into(), pct: 33, charging: None, address: String::new() }];
        merge_extra(&mut v, &p.paired_pods, &ads, &extra);
        let pods = &v[0];
        assert_eq!((pods.left, pods.right, pods.case), (Some(42), Some(38), Some(71)), "exact levels stay");
        assert!(pods.charging && pods.case_charging);
        let old = v.iter().find(|d| d.name == "Old AirPods").unwrap();
        assert_eq!((old.left, old.right, old.case, old.address.as_str()), (Some(90), Some(90), Some(100), "aa-bb-cc-dd-ee-09"));
        let phone = v.iter().find(|d| d.name == "Alex's iPhone").unwrap();
        assert_eq!((phone.kind, phone.main, phone.charging), (Kind::Phone, Some(64), true));
        assert_eq!(v.iter().find(|d| d.name == "MX Master 3S").unwrap().main, Some(33));
        assert_eq!(v.len(), 6);
    }

    #[test]
    fn advert_of_connected_pods_never_adds_a_second_pair() {
        // connected pair without a product id, and the same pods also in
        // the paired list under another address: one row only
        let mut v = vec![Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, address: "aa-01".into(), left: Some(79), right: Some(79), ..Default::default() }];
        let paired = [Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, address: "aa-02".into(), product: 0x2014, ..Default::default() }];
        let ad = PodsAdvert { rssi: -40, ..parse_proximity(&advert(0x2014, 0x2b, 0xff, 0x01), -40).unwrap() };
        merge_extra(&mut v, &paired, &[ad], &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].case, Some(10), "the advert fills the connected pair");
        // a connected pair with a known id of another model: same name still
        // keeps the paired pods out
        let mut v = vec![Device { name: "AirPods Pro".into(), kind: Kind::Earbuds, product: 0x200e, left: Some(50), right: Some(50), ..Default::default() }];
        merge_extra(&mut v, &paired, &[ad], &[]);
        assert_eq!(v.len(), 1);
        // other makers' earbuds without a product id keep their own data
        let sony = Device { name: "WF-1000XM5".into(), kind: Kind::Earbuds, address: "bb-01".into(), left: Some(60), right: Some(60), ..Default::default() };
        let mut v = vec![sony.clone()];
        merge_extra(&mut v, &[], &[ad], &[]);
        assert_eq!(v, vec![sony]);
    }

    fn mouse(b: u8) -> Vec<Device> {
        vec![Device { name: "MX Master 3S".into(), kind: Kind::Mouse, main: Some(b), ..Default::default() }]
    }

    #[test]
    fn alerts_fire_once_per_step_and_reset_after_charging() {
        let p = Prefs::default();
        let mut a = Alerts::default();
        assert!(a.check(&mouse(40), &p).is_empty());
        let n = a.check(&mouse(19), &p);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "MX Master 3S is at 19%");
        assert!(a.check(&mouse(18), &p).is_empty());
        assert!(a.check(&mouse(22), &p).is_empty(), "no reset below the reset line");
        assert!(a.check(&mouse(19), &p).is_empty());
        assert_eq!(a.check(&mouse(9), &p).len(), 1);
        assert!(a.check(&mouse(5), &p).is_empty());
        assert!(a.check(&mouse(80), &p).is_empty());
        assert_eq!(a.check(&mouse(15), &p).len(), 1, "alerts again after a charge");
    }

    #[test]
    fn no_low_alert_while_charging_or_away_and_one_charged_alert() {
        let mut p = Prefs { charged: true, charged_at: 90, ..Prefs::default() };
        let mut a = Alerts::default();
        let mut d = mouse(10);
        d[0].charging = true;
        assert!(a.check(&d, &p).is_empty());
        d[0].main = Some(91);
        let n = a.check(&d, &p);
        assert_eq!(n, vec![Note { charged: true, title: "MX Master 3S charged".into(), body: "At 91%".into() }]);
        d[0].main = Some(100);
        assert!(a.check(&d, &p).is_empty(), "once per charge");
        p.charged_at = 100;
        assert_eq!(a.check(&d, &p).len(), 1, "again at a new level");
        p.charged_at = 90;
        a.check(&d, &p);
        d[0].charging = false;
        a.check(&d, &p);
        d[0].charging = true;
        assert_eq!(a.check(&d, &p).len(), 1, "again after the next charge");
        p.charged = false;
        let mut away = mouse(3);
        away[0].seen = Some(1);
        assert!(Alerts::default().check(&away, &p).is_empty());
    }

    #[test]
    fn per_device_alert_line_and_hiding() {
        let mut p = Prefs::default();
        let key = mouse(0)[0].key();
        p.set_device(&key, |d| d.alert_below = Some(50));
        let mut a = Alerts::default();
        assert_eq!(a.check(&mouse(45), &p).len(), 1);
        assert!(a.check(&mouse(60), &p).is_empty(), "resets 10 above its line");
        assert_eq!(a.check(&mouse(45), &p).len(), 1);
        p.set_device(&key, |d| d.alert_below = Some(0));
        assert!(Alerts::default().check(&mouse(2), &p).is_empty(), "off");
        p.set_device(&key, |d| {
            d.alert_below = None;
            d.hidden = true;
        });
        assert!(Alerts::default().check(&mouse(2), &p).is_empty(), "hidden");
        assert_eq!(tray_title(&mouse(2), &p), None);
        let (shown, hidden) = p.split(demo().into_iter().chain(mouse(2)).collect());
        assert_eq!(hidden.len(), 2);
        assert_eq!(shown.len(), demo().len() - 1, "the demo mouse has the same name");
        p.set_device(&key, |d| d.hidden = false);
        assert!(p.devices.is_empty(), "defaults aren't stored");
    }

    #[test]
    fn tray_shows_only_the_low_device() {
        let p = Prefs::default();
        let mut v = demo();
        assert_eq!(tray_title(&v, &p).as_deref(), Some("Mouse 14%"));
        assert_eq!(tray_title(&v, &Prefs { menu_bar: false, ..p.clone() }), None);
        v[2].main = Some(60);
        assert_eq!(tray_title(&v, &p), None);
        v[0].right = Some(6);
        assert_eq!(tray_title(&v, &p).as_deref(), Some("AirPods 6%"));
        v[0].charging = true;
        assert_eq!(tray_title(&v, &p), None, "not while charging");
    }

    #[test]
    fn lowest_first_puts_unknown_and_away_last() {
        let mut v = demo();
        v.push(Device { name: "Speaker".into(), ..Default::default() });
        let names: Vec<_> = lowest_first(&v).into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["MX Master 3S", "AirPods Pro", "Magic Trackpad", "Magic Keyboard", "Speaker", "Beats Studio Pro"]);
    }

    #[test]
    fn old_prefs_still_load() {
        let p: Prefs = serde_json::from_str(r#"{"alerts":false,"menu_bar":true}"#).unwrap();
        assert_eq!((p.alerts, p.charged, p.charged_at), (false, false, 100));
    }
}
