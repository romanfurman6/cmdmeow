//! Live Mac health readings for the menu panel. A background thread refreshes a
//! shared snapshot; slow sources (system_profiler, folder sizes) run less often.

use crate::kit::sync::LockExt;
use crate::kit::activity::{self, Every};
use crate::kit::fs::dir_size;
use crate::kit::mechanics as m;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    Good,
    Fair,
    Critical,
}

impl Level {
    pub fn worst(self, other: Level) -> Level {
        use Level::*;
        match (self, other) {
            (Critical, _) | (_, Critical) => Critical,
            (Fair, _) | (_, Fair) => Fair,
            _ => Good,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Battery {
    pub present: bool,
    pub percent: u8,
    pub charging: bool,
    pub plugged: bool,
    pub full: bool,
    pub minutes_left: Option<u32>,
    pub cycles: u32,
    pub design_cycles: u32,
    pub health_pct: Option<f32>,
    pub temp_c: Option<f32>,
    /// macOS's own verdict ("Good", "Fair", "Poor", "Check Battery"...)
    pub health_text: String,
    pub permanent_failure: bool,
}

pub use super::devices::Device;
use crate::kit::charge;
use super::devices;
use super::devices_store::{History, Seen};

#[derive(Clone, Debug, Default)]
pub struct Wifi {
    pub name: String,
    pub security: String,
    pub phy: String,
    pub channel: String,
    pub signal: String,
    pub wired: bool,
}

/// Where a speed test is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedPhase {
    #[default]
    Idle,
    Ping,
    Download,
    Upload,
    Done,
}

/// How long a speed test runs: Quick (a few seconds, fewer streams) or
/// Full (longer phases, more streams, more pings).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedMode {
    #[default]
    Quick,
    Full,
}

impl SpeedMode {
    pub fn label(self) -> &'static str {
        match self {
            SpeedMode::Quick => "Quick test",
            SpeedMode::Full => "Full test",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SpeedTest {
    pub mode: SpeedMode,
    pub running: bool,
    pub phase: SpeedPhase,
    /// The rate of the transfer under way, in Mbps (for the gauge).
    pub live_mbps: f64,
    pub progress: f32,
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    /// The provider and the test server's city, when the server says.
    pub server: Option<super::speedtest::ServerInfo>,
    pub error: Option<String>,
    /// Set by [`SpeedTest::stop`]; the test thread checks it between reads.
    pub cancel: Arc<AtomicBool>,
}

impl SpeedTest {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub ready: bool,
    pub model: String,
    pub chip: String,
    pub volume_name: String,
    pub disk_total: u64,
    pub disk_free: u64,
    pub smart_status: String,
    pub categories: Vec<(String, u64)>, // Applications, Documents, Downloads, Desktop
    pub trash_bytes: Option<u64>,
    /// The folders are being measured ([`measure_folders`]).
    pub sizing: bool,
    pub mem_total: u64,
    pub mem_pressure: f32,
    pub mem_pressure_level: u32,
    pub mem_active: u64,
    pub mem_wired: u64,
    pub mem_compressed: u64,
    pub mem_available: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub cpu_load: f32,
    pub cpu_user: f32,
    pub cpu_system: f32,
    pub cpu_history: Vec<f32>,
    /// Memory pressure, one sample per refresh (last 60).
    pub mem_history: Vec<f32>,
    /// Download + upload in bytes/s, one sample per refresh (last 60).
    pub net_history: Vec<f32>,
    pub thermal: u8, // 0 nominal, 1 fair, 2 serious, 3 critical
    pub uptime_secs: u64,
    pub battery: Battery,
    pub net_up: f64,
    pub net_down: f64,
    pub net_sent_total: u64,
    pub net_recv_total: u64,
    pub online: bool,
    pub wifi: Wifi,
    /// Bluetooth devices: connected ones, then ones seen before.
    pub devices: Vec<Device>,
    /// The ones the person hid (still listed on the Devices page).
    pub hidden_devices: Vec<Device>,
    /// Device alert and menu bar settings (changed from the Devices sheet).
    pub bt_prefs: devices::Prefs,
    pub speed: SpeedTest,
    /// Charge limit (held by the fan helper).
    pub charge: charge::View,
    /// Watts and clocks per part (Apple silicon), while on screen.
    pub power: Option<crate::kit::ioreport::Reading>,
    /// The whole Mac's draw in watts (SMC `PSTR`).
    pub sys_watts: Option<f32>,
    /// The busiest processes, busiest first.
    pub procs: Vec<super::procs::Proc>,
    /// Busiest apps on the network, while the Network sheet shows.
    pub apps: Vec<super::traffic::AppRate>,
}

impl Snapshot {
    pub fn disk_level(&self) -> Level {
        if self.disk_total == 0 {
            return Level::Good;
        }
        // the original raises "out of disk space" below this threshold;
        // half of it counts as critical here (estimated)
        let t = m::disk_alert_threshold(self.disk_total);
        if self.disk_free < t / 2 {
            Level::Critical
        } else if self.disk_free < t {
            Level::Fair
        } else {
            Level::Good
        }
    }
    pub fn mem_level(&self) -> Level {
        match self.mem_pressure_level {
            0 | m::KERNEL_PRESSURE_NORMAL => Level::Good,
            m::KERNEL_PRESSURE_WARN => Level::Fair,
            _ => Level::Critical,
        }
    }
    pub fn thermal_level(&self) -> Level {
        match self.thermal {
            0 | 1 => Level::Good,
            2 => Level::Fair,
            _ => Level::Critical,
        }
    }
    /// BatteryMonitor sub_25d4: "Check Battery" / "Permanent Battery Failure" or the
    /// failure flag → service; otherwise macOS's Good / Fair / Poor.
    pub fn battery_level(&self) -> Level {
        let b = &self.battery;
        match m::battery_health_state(&b.health_text, b.permanent_failure) {
            m::BatteryHealthState::Good | m::BatteryHealthState::Unknown => Level::Good,
            m::BatteryHealthState::Fair => Level::Fair,
            m::BatteryHealthState::Poor | m::BatteryHealthState::Service => Level::Critical,
        }
    }
    pub fn uptime_warning(&self) -> bool {
        self.uptime_secs > m::UPTIME_RESTART_INTERVAL_SECS
    }
    pub fn health(&self) -> Level {
        self.disk_level()
            .worst(self.mem_level())
            .worst(self.thermal_level())
            .worst(self.battery_level())
    }
    pub fn uptime_days(&self) -> u64 {
        self.uptime_secs / m::SECONDS_PER_DAY
    }
    pub fn trash_overfill(&self) -> bool {
        self.trash_bytes.map(|b| b > m::TRASH_SIZE_THRESHOLD).unwrap_or(false)
    }
    pub fn thermal_word(&self) -> &'static str {
        ["Cool", "Warm", "Hot", "Critical"][self.thermal.min(3) as usize]
    }
}

pub type Shared = Arc<Mutex<Snapshot>>;

/// Change the charge policy, save it and hand it to the helper.
pub fn change_charge(mon: &Shared, ctx: &egui::Context, f: impl FnOnce(&mut charge::Policy)) {
    let p = {
        let mut s = mon.lock_ok();
        f(&mut s.charge.policy);
        s.charge.policy = s.charge.policy.sanitize();
        s.charge.policy
    };
    charge::save(&p);
    let (mon, ctx) = (mon.clone(), ctx.clone());
    std::thread::spawn(move || {
        sync_charge(&mon, &p);
        ctx.request_repaint();
    });
}

/// Install (or update) the helper from the Battery sheet, then hand it the policy.
pub fn install_charge_helper(mon: &Shared, ctx: &egui::Context) {
    {
        let mut s = mon.lock_ok();
        if s.charge.installing {
            return;
        }
        s.charge.installing = true;
    }
    let (mon, ctx) = (mon.clone(), ctx.clone());
    std::thread::spawn(move || {
        let _ = crate::kit::helper::install();
        let p = {
            let mut s = mon.lock_ok();
            s.charge.installing = false;
            s.charge.policy
        };
        sync_charge(&mon, &p);
        ctx.request_repaint();
    });
}

/// Send the policy to the helper and keep what it answers.
fn sync_charge(mon: &Shared, p: &charge::Policy) {
    let r = charge::send(p);
    let mut s = mon.lock_ok();
    match r {
        Some(Some(st)) => {
            s.charge.helper_ok = Some(true);
            s.charge.status = st;
        }
        _ => s.charge.helper_ok = Some(false),
    }
}

/// Change the device alert / menu bar settings and save them. Hiding or
/// showing a device takes effect at once.
pub fn change_bt_prefs(mon: &Shared, f: impl FnOnce(&mut devices::Prefs)) {
    let prefs = {
        let mut s = mon.lock_ok();
        f(&mut s.bt_prefs);
        let mut all = std::mem::take(&mut s.devices);
        all.append(&mut s.hidden_devices);
        let (shown, hidden) = s.bt_prefs.split(devices::lowest_first(&all));
        s.devices = shown;
        s.hidden_devices = hidden;
        s.bt_prefs.clone()
    };
    prefs.save();
}

/// What the devices list remembers: last known charges and a week of history.
struct BtStore {
    seen: Seen,
    history: History,
}

static BT_STORE: std::sync::OnceLock<Mutex<BtStore>> = std::sync::OnceLock::new();

fn bt_store() -> &'static Mutex<BtStore> {
    BT_STORE.get_or_init(|| {
        let now = devices::now();
        let mut history: History = crate::kit::fs::load_json(&History::path());
        history.prune(now);
        Mutex::new(BtStore { seen: crate::kit::fs::load_json(&Seen::path()), history })
    })
}

/// Forget a device that isn't connected: its last charge, its history and
/// its settings.
pub fn forget_device(mon: &Shared, key: &str) {
    {
        let mut st = bt_store().lock_ok();
        st.seen.forget(key);
        st.history.forget(key);
        crate::kit::fs::save_json_later(&Seen::path(), &st.seen, false);
        crate::kit::fs::save_json_later(&History::path(), &st.history, false);
    }
    change_bt_prefs(mon, |p| {
        p.devices.remove(key);
    });
    let mut s = mon.lock_ok();
    s.devices.retain(|d| d.live() || d.key() != key);
    s.hidden_devices.retain(|d| d.live() || d.key() != key);
}

/// The full device list: what's connected (the report, the input
/// registry, Bluetooth LE and controllers), then what was seen before;
/// each with its trend and estimate. Updates the stores.
fn read_devices(profile: &devices::Profile) -> Vec<Device> {
    let mut list = profile.connected.clone();
    let mut io = run_cmd("ioreg", &["-r", "-l", "-k", "BatteryPercent"]);
    io.push_str(&run_cmd("ioreg", &["-r", "-l", "-k", "BatteryPercentSingle"]));
    devices::merge(&mut list, &devices::parse_ioreg(&io));
    devices::merge_extra(&mut list, &profile.paired_pods, &super::ble::adverts(), &super::ble::extra());
    let now = devices::now();
    let mut st = bt_store().lock_ok();
    let mut history_changed = false;
    for d in &list {
        if let Some(b) = d.battery() {
            history_changed |= st.history.record(&d.key(), now, b);
        }
    }
    if st.seen.update(&list, now) || SEEN_SAVE_EVERY.with_borrow_mut(|e| e.due(SEEN_SAVE)) {
        crate::kit::fs::save_json_later(&Seen::path(), &st.seen, false);
    }
    if history_changed && HISTORY_SAVE_EVERY.with_borrow_mut(|e| e.due(HISTORY_SAVE)) {
        crate::kit::fs::save_json_later(&History::path(), &st.history, false);
    }
    let away = st.seen.away(&list);
    list.extend(away);
    for d in &mut list {
        let key = d.key();
        d.trend = st.history.trend(&key, now);
        d.eta = if d.charging || !d.live() { None } else { st.history.eta(&key) };
    }
    list
}

const SEEN_SAVE: Duration = Duration::from_secs(10 * 60);
const HISTORY_SAVE: Duration = Duration::from_secs(15 * 60);
thread_local! {
    static SEEN_SAVE_EVERY: std::cell::RefCell<Every> = std::cell::RefCell::new(Every::default());
    static HISTORY_SAVE_EVERY: std::cell::RefCell<Every> = std::cell::RefCell::new(Every::default());
}

/// Mac Health is off: the monitor sleeps (no readings, no scans) until it is
/// turned back on.
static PAUSED: AtomicBool = AtomicBool::new(false);
static RESUME: activity::Kick = activity::Kick::new();

pub fn set_paused(paused: bool) {
    if PAUSED.swap(paused, Ordering::Relaxed) != paused && !paused {
        RESUME.kick();
    }
}

pub fn start(ctx: egui::Context, demo: bool) -> Shared {
    let policy = if demo { charge::Policy::default() } else { charge::load() };
    let charge = charge::View { policy, ..Default::default() };
    let shared: Shared = Arc::new(Mutex::new(Snapshot { bt_prefs: devices::Prefs::load(), charge, ..Snapshot::default() }));
    let s = shared.clone();
    std::thread::Builder::new()
        .name("monitor".into())
        .spawn(move || run(s, ctx, demo))
        .ok();
    shared
}

/// How often each slow source refreshes: (while on screen, while hidden).
/// Hidden, only what the menu-bar title and Bluetooth alerts need keeps going.
struct Cadence {
    shown: Duration,
    hidden: Option<Duration>,
}

impl Cadence {
    const fn secs(shown: u64, hidden: Option<u64>) -> Self {
        Cadence { shown: Duration::from_secs(shown), hidden: match hidden { Some(h) => Some(Duration::from_secs(h)), None => None } }
    }
    /// Due now? `None` while hidden means "not until it's shown again",
    /// except for the very first reading.
    fn due(&self, last: &mut Every) -> bool {
        match (activity::visible(), self.hidden) {
            (true, _) => last.due(self.shown),
            (false, Some(h)) => last.due(h),
            (false, None) => last.age().is_none() && last.due(self.shown),
        }
    }
}

const BATTERY_EVERY: Cadence = Cadence::secs(10, Some(60));
/// macOS's battery-health verdict changes over months.
const BATTERY_HEALTH_EVERY: Duration = Duration::from_secs(30 * 60);
/// Input devices (`ioreg`): batteries for the alerts and the menu-bar title.
const HID_EVERY: Cadence = Cadence::secs(10, Some(60));
/// The full Bluetooth report (`system_profiler`, slow). A device coming or
/// going refreshes it at once, so this is only for levels.
const BT_PROFILE_EVERY: Cadence = Cadence::secs(60, Some(10 * 60));
/// After a connect, look again this much later (levels arrive late).
const BT_RECHECK_AFTER: Duration = Duration::from_secs(12);
/// The network's name (`ipconfig`, fast).
const WIFI_EVERY: Cadence = Cadence::secs(60, None);
/// Channel, signal and so on (`system_profiler`, ~10 s as it may scan).
const WIFI_DETAILS_EVERY: Cadence = Cadence::secs(10 * 60, None);
/// The Trash's size, for the "Empty Trash" nudge (one folder only).
const TRASH_EVERY: Cadence = Cadence::secs(600, None);
/// The process list (only while on screen).
const PROCS_EVERY: Cadence = Cadence::secs(4, None);
/// A folder walk is running (on its own thread).
static SIZING: AtomicBool = AtomicBool::new(false);

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Run `f` on its own thread at background priority, one walk at a time.
fn in_background(name: &str, f: impl FnOnce() + Send + 'static) {
    if SIZING.swap(true, Ordering::Relaxed) {
        return;
    }
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        activity::low_priority();
        f();
        SIZING.store(false, Ordering::Relaxed);
    });
    if spawned.is_err() {
        SIZING.store(false, Ordering::Relaxed);
    }
}

/// Measure the storage chart's folders (Applications, Documents, Downloads,
/// Desktop, Trash). Only on the user's click: it walks every file.
/// Documents, Downloads and Desktop are behind macOS's folder questions, so
/// without Full Disk Access the click asks for that instead.
pub fn measure_folders(shared: Shared, ctx: egui::Context) {
    use crate::kit::perms::{Perm, Status};
    if SIZING.load(Ordering::Relaxed) {
        return;
    }
    if Perm::FullDisk.status() != Status::Allowed {
        Perm::FullDisk.ask();
        return;
    }
    shared.lock_ok().sizing = true;
    ctx.request_repaint();
    let s = shared.clone();
    in_background("folder-sizes", move || {
        let home = home_dir();
        let places = [("Applications", PathBuf::from("/Applications")), ("Documents", home.join("Documents")), ("Downloads", home.join("Downloads")), ("Desktop", home.join("Desktop"))];
        // each folder on its own thread: the slowest one sets the wait
        let (cats, trash) = std::thread::scope(|sc| {
            let jobs: Vec<_> = places.iter().map(|(n, p)| sc.spawn(move || (n.to_string(), dir_size(p)))).collect();
            let t = home.join(".Trash");
            let trash = std::fs::read_dir(&t).ok().map(|_| dir_size(&t));
            (jobs.into_iter().map(|j| j.join().unwrap_or_default()).collect::<Vec<_>>(), trash)
        });
        let mut g = s.lock_ok();
        g.categories = cats;
        g.trash_bytes = trash;
        g.sizing = false;
        drop(g);
        ctx.request_repaint();
    });
}

fn run(shared: Shared, ctx: egui::Context, demo: bool) {
    let mut sys = sysinfo::System::new();
    let mut nets = sysinfo::Networks::new_with_refreshed_list();
    let mut last_ticks = cpu_ticks();
    let mut last_net = Instant::now();
    // the slow Bluetooth report, refreshed from the input registry in between
    let mut bt_profile = devices::Profile::default();
    let mut bt_alerts = devices::Alerts::default();
    // the connected set, to refresh as soon as a device comes or goes; and
    // a second look a little later (the report lags behind a connect)
    let mut bt_connected: Option<Vec<String>> = None;
    let mut bt_recheck: Option<Instant> = None;
    let (mut battery_at, mut health_at, mut hid_at, mut profile_at) = (Every::default(), Every::default(), Every::default(), Every::default());
    let (mut wifi_at, mut wifi_details_at, mut sizes_at) = (Every::default(), Every::default(), Every::default());
    let mut wifi_info = Wifi::default();
    let started = Instant::now();
    let wifi_slot: Arc<Mutex<Option<Wifi>>> = Arc::new(Mutex::new(None));
    let wifi_busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut health_text = String::new();
    let mut psys = sysinfo::System::new();
    let mut procs_at = Every::default();
    #[cfg(target_os = "macos")]
    let mut sampler = if demo { None } else { crate::kit::ioreport::Sampler::open() };
    #[cfg(target_os = "macos")]
    let smc = if demo { None } else { crate::kit::smc::Smc::open() };
    // one-off facts
    {
        let (model, chip) = hardware();
        let (vol, smart) = disk_info();
        let mut s = shared.lock_ok();
        s.model = model;
        s.chip = chip;
        s.volume_name = vol;
        s.smart_status = smart;
    }
    loop {
        if PAUSED.load(Ordering::Relaxed) {
            super::ble::stop();
            while PAUSED.load(Ordering::Relaxed) {
                activity::nap(Duration::from_secs(3600), Some(&RESUME));
            }
            // fresh readings, not an hour-long gap
            last_ticks = cpu_ticks();
            last_net = Instant::now();
        }
        // fast readings every refresh: all plain system calls, no processes
        sys.refresh_memory();
        nets.refresh(true);
        let now_ticks = cpu_ticks();
        let (load, user, system) = cpu_split(last_ticks, now_ticks);
        last_ticks = now_ticks;
        let dt = last_net.elapsed().as_secs_f64().max(0.2);
        last_net = Instant::now();
        let (mut rx, mut tx, mut rx_total, mut tx_total) = (0u64, 0u64, 0u64, 0u64);
        for (name, data) in nets.iter() {
            if name.starts_with("en") {
                rx += data.received();
                tx += data.transmitted();
                rx_total += data.total_received();
                tx_total += data.total_transmitted();
            }
        }
        let (total, free) = crate::kit::fs::volume_space("/");
        let vm = vm_counts();
        let (swap_used, swap_total) = swap();
        // kernel pressure level (SystemResourcesMonitor sub_9504): 1 normal, 2 warning, else critical
        let pressure_level = sysctl_u32("kern.memorystatus_vm_pressure_level").unwrap_or(1);
        let thermal = crate::kit::native::thermal_state();
        let uptime = sysinfo::System::uptime();

        let shown = activity::visible();
        #[cfg(target_os = "macos")]
        let (power, sys_watts) = if shown {
            (sampler.as_mut().and_then(|s| s.read()), smc.as_ref().and_then(|s| s.read("PSTR")).filter(|w| w.is_finite() && *w > 0.0 && *w < 1000.0))
        } else {
            (None, None)
        };
        #[cfg(not(target_os = "macos"))]
        let (power, sys_watts) = (None, None);
        let procs = (!demo && PROCS_EVERY.due(&mut procs_at)).then(|| super::procs::top(super::procs::read(&mut psys), std::process::id(), super::procs::TOP_N));
        let battery_due = BATTERY_EVERY.due(&mut battery_at);
        // keep the helper's charge state fresh (and hand it the policy
        // again, in case it was reinstalled)
        let policy = shared.lock_ok().charge.policy;
        if battery_due && !demo && policy.active() {
            sync_charge(&shared, &policy);
        }
        let battery = battery_due.then(|| {
            let mut b = parse_battery(&run_cmd("ioreg", &["-rn", "AppleSmartBattery"]));
            if b.present {
                if health_text.is_empty() || health_at.due(BATTERY_HEALTH_EVERY) {
                    health_text = power_health_text();
                }
                b.health_text = health_text.clone();
            }
            b
        });
        // system_profiler takes ~9 s: on its own thread, so the first reading
        // (and every widget waiting for it) doesn't wait for it
        if WIFI_DETAILS_EVERY.due(&mut wifi_details_at) && !wifi_busy.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let (slot, busy) = (wifi_slot.clone(), wifi_busy.clone());
            std::thread::Builder::new()
                .name("wifi-details".into())
                .spawn(move || {
                    *slot.lock_ok() = Some(wifi_details());
                    busy.store(false, std::sync::atomic::Ordering::Relaxed);
                })
                .ok();
        }
        let fresh = wifi_slot.lock_ok().take();
        if let Some(w) = fresh {
            wifi_info = w;
            wifi_at.reset();
        }
        let wifi = WIFI_EVERY.due(&mut wifi_at).then(|| wifi_name(&wifi_info));
        // Bluetooth LE: starts once the panel has been opened and the
        // permission is given, then scans now and then
        if activity::visible() {
            super::ble::start();
        }
        super::ble::tick();
        if let Some(now_connected) = super::ble::connected() {
            if bt_connected.as_ref().is_some_and(|c| *c != now_connected) {
                profile_at.reset();
                hid_at.reset();
                bt_recheck = Some(Instant::now() + BT_RECHECK_AFTER);
            }
            bt_connected = Some(now_connected);
        }
        if bt_recheck.is_some_and(|t| Instant::now() >= t) {
            bt_recheck = None;
            profile_at.reset();
        }
        // hidden, the slow report is only for the alerts: skip it while
        // nothing is connected over Bluetooth
        let none_connected = bt_connected.as_ref().is_some_and(|c| c.is_empty());
        let slow_profile = (activity::visible() || !none_connected) && BT_PROFILE_EVERY.due(&mut profile_at);
        if slow_profile {
            bt_profile = devices::parse_profiler(&run_cmd("system_profiler", &["SPBluetoothDataType", "-json"]));
        }
        let devices = (HID_EVERY.due(&mut hid_at) || slow_profile).then(|| read_devices(&bt_profile));

        {
            let mut s = shared.lock_ok();
            if !s.ready {
                eprintln!("[monitor] first reading after {} ms", started.elapsed().as_millis());
            }
            s.ready = true;
            s.disk_total = total;
            s.disk_free = free;
            s.mem_total = sys.total_memory();
            s.mem_pressure_level = pressure_level;
            if let Some(vm) = vm {
                s.mem_active = vm.0;
                s.mem_wired = vm.1;
                // "Pressure: N%" = (wired + compressed) / physical memory (sub_90b8)
                s.mem_pressure = m::memory_pressure_pct(vm.1, vm.2, s.mem_total);
                s.mem_compressed = vm.2;
                s.mem_available = s.mem_total.saturating_sub(vm.0 + vm.1 + vm.2);
            }
            s.swap_used = swap_used;
            s.swap_total = swap_total;
            s.cpu_load = load;
            s.cpu_user = user;
            s.cpu_system = system;
            s.cpu_history.push(load);
            if s.cpu_history.len() > 60 {
                s.cpu_history.remove(0);
            }
            let pressure = s.mem_pressure;
            s.mem_history.push(pressure);
            if s.mem_history.len() > 60 {
                s.mem_history.remove(0);
            }
            s.thermal = thermal;
            s.uptime_secs = uptime;
            s.net_down = rx as f64 / dt;
            s.net_up = tx as f64 / dt;
            let rate = (s.net_down + s.net_up) as f32;
            s.net_history.push(rate);
            if s.net_history.len() > 60 {
                s.net_history.remove(0);
            }
            s.net_recv_total = rx_total;
            s.net_sent_total = tx_total;
            s.online = rx_total + tx_total > 0;
            if let Some(b) = battery {
                s.battery = b;
            }
            if shown {
                s.power = power;
                s.sys_watts = sys_watts;
            }
            if let Some(p) = procs {
                s.procs = p;
            }
            if let Some(d) = devices {
                let notes = bt_alerts.check(&d, &s.bt_prefs);
                // with alerts off, stay quiet but remember, so turning them
                // on doesn't replay old ones
                // each kind goes out only while its own switch is on
                for n in notes.into_iter().filter(|n| n.charged || s.bt_prefs.alerts) {
                    crate::kit::notify::send(&n.title, &n.body);
                }
                let (shown, hidden) = s.bt_prefs.split(d);
                s.devices = shown;
                s.hidden_devices = hidden;
            }
            if let Some(w) = wifi {
                s.wifi = w;
            }
        }
        activity::repaint(&ctx);

        // only the Trash is measured on its own (for the "Empty Trash" nudge);
        // the big folders wait for a click ([`measure_folders`])
        if !activity::recording() && !SIZING.load(Ordering::Relaxed) && TRASH_EVERY.due(&mut sizes_at) {
            let (shared, ctx) = (shared.clone(), ctx.clone());
            in_background("trash-size", move || {
                let trash = home_dir().join(".Trash");
                let bytes = std::fs::read_dir(&trash).ok().map(|_| dir_size(&trash));
                shared.lock_ok().trash_bytes = bytes;
                activity::repaint(&ctx);
            });
        }
        // the panel opening ends the nap: it shows fresh numbers right away
        activity::nap(Duration::from_millis(m::MONITOR_REFRESH_MS), None);
    }
}

// ---------------------------------------------------------------- sources

fn run_cmd(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn hardware() -> (String, String) {
    let out = run_cmd("system_profiler", &["SPHardwareDataType", "-json"]);
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    let item = &v["SPHardwareDataType"][0];
    let model = item["machine_name"].as_str().unwrap_or("Mac").to_string();
    let chip = item["chip_type"].as_str().or(item["cpu_type"].as_str()).unwrap_or("").to_string();
    (model, chip)
}

fn disk_info() -> (String, String) {
    let out = run_cmd("diskutil", &["info", "/"]);
    let mut name = "Macintosh HD".to_string();
    let mut smart = String::new();
    for line in out.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("Volume Name:") {
            name = v.trim().to_string();
        }
        if let Some(v) = l.strip_prefix("SMART Status:") {
            smart = v.trim().to_string();
        }
    }
    (name, smart)
}

#[cfg(target_os = "macos")]
pub fn sysctl_u32(name: &str) -> Option<u32> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut v: u32 = 0;
    let mut len = std::mem::size_of::<u32>();
    let r = unsafe { libc::sysctlbyname(c.as_ptr(), &mut v as *mut u32 as *mut _, &mut len, std::ptr::null_mut(), 0) };
    (r == 0).then_some(v)
}

#[cfg(target_os = "macos")]
fn swap() -> (u64, u64) {
    #[repr(C)]
    struct XswUsage {
        total: u64,
        avail: u64,
        used: u64,
        pagesize: u32,
        encrypted: u32,
    }
    let c = std::ffi::CString::new("vm.swapusage").unwrap();
    let mut v: XswUsage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<XswUsage>();
    let r = unsafe { libc::sysctlbyname(c.as_ptr(), &mut v as *mut XswUsage as *mut _, &mut len, std::ptr::null_mut(), 0) };
    if r == 0 {
        (v.used, v.total)
    } else {
        (0, 0)
    }
}

/// (app memory, wired, compressed) bytes, as `vm_stat` and the original's
/// RAMService compute them (SystemResourcesMonitor sub_90b8):
/// app memory = anonymous (internal) − purgeable pages. Read straight from
/// the kernel instead of starting `vm_stat` every refresh.
#[cfg(target_os = "macos")]
fn vm_counts() -> Option<(u64, u64, u64)> {
    unsafe {
        let mut info: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        #[allow(deprecated)]
        let host = libc::mach_host_self();
        let r = libc::host_statistics64(host, libc::HOST_VM_INFO64, &mut info as *mut _ as *mut i32, &mut count);
        if r != 0 {
            return None;
        }
        #[allow(deprecated)]
        let page = libc::vm_page_size as u64;
        Some(mem_split(info.internal_page_count as u64, info.purgeable_count as u64, info.wire_count as u64, info.compressor_page_count as u64, page))
    }
}

/// (app memory, wired, compressed) bytes from page counts.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mem_split(internal: u64, purgeable: u64, wired: u64, compressed: u64, page: u64) -> (u64, u64, u64) {
    (internal.saturating_sub(purgeable) * page, wired * page, compressed * page)
}

#[cfg(target_os = "macos")]
fn cpu_ticks() -> [u64; 4] {
    unsafe {
        let mut info: libc::host_cpu_load_info = std::mem::zeroed();
        let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
        #[allow(deprecated)]
        let host = libc::mach_host_self();
        let r = libc::host_statistics(host, libc::HOST_CPU_LOAD_INFO, &mut info as *mut _ as *mut i32, &mut count);
        if r != 0 {
            return [0; 4];
        }
        let t = info.cpu_ticks;
        [t[0] as u64, t[1] as u64, t[2] as u64, t[3] as u64] // user, system, idle, nice
    }
}

// Other platforms build (so the tests run anywhere) but read nothing.
#[cfg(not(target_os = "macos"))]
pub fn sysctl_u32(_name: &str) -> Option<u32> {
    None
}
#[cfg(not(target_os = "macos"))]
fn vm_counts() -> Option<(u64, u64, u64)> {
    None
}
#[cfg(not(target_os = "macos"))]
fn swap() -> (u64, u64) {
    (0, 0)
}
#[cfg(not(target_os = "macos"))]
fn cpu_ticks() -> [u64; 4] {
    [0; 4]
}

fn cpu_split(a: [u64; 4], b: [u64; 4]) -> (f32, f32, f32) {
    let d: Vec<f64> = (0..4).map(|i| b[i].saturating_sub(a[i]) as f64).collect();
    let total = d.iter().sum::<f64>();
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let user = ((d[0] + d[3]) / total * 100.0) as f32;
    let system = (d[1] / total * 100.0) as f32;
    (user + system, user, system)
}

/// Battery readings from `ioreg -rn AppleSmartBattery`.
fn parse_battery(out: &str) -> Battery {
    let mut b = Battery::default();
    if out.trim().is_empty() {
        return b;
    }
    b.present = true;
    let mut design = 0f32;
    let mut nominal = 0f32;
    let mut raw_max = 0f32;
    let (mut to_empty, mut to_full) = (None, None);
    for line in out.lines() {
        let l = line.trim_start_matches([' ', '|']).trim();
        let Some((k, v)) = l.split_once(" = ") else { continue };
        let k = k.trim_matches('"');
        let v = v.trim();
        let n = || v.parse::<f64>().ok();
        match k {
            "CurrentCapacity" => b.percent = n().unwrap_or(0.0).clamp(0.0, 100.0) as u8,
            "IsCharging" => b.charging = v == "Yes",
            "ExternalConnected" => b.plugged = v == "Yes",
            "FullyCharged" => b.full = v == "Yes",
            "AvgTimeToEmpty" => to_empty = n(),
            "AvgTimeToFull" => to_full = n(),
            "PermanentFailureStatus" => b.permanent_failure = n().unwrap_or(0.0) != 0.0,
            "CycleCount" => b.cycles = n().unwrap_or(0.0) as u32,
            "DesignCycleCount9C" => b.design_cycles = n().unwrap_or(0.0) as u32,
            "DesignCapacity" => design = n().unwrap_or(0.0) as f32,
            "NominalChargeCapacity" => nominal = n().unwrap_or(0.0) as f32,
            "AppleRawMaxCapacity" => raw_max = n().unwrap_or(0.0) as f32,
            "Temperature" => b.temp_c = n().map(|t| t as f32 / m::BATTERY_TEMP_DIVISOR),
            _ => {}
        }
    }
    // AvgTimeToEmpty / AvgTimeToFull, 65535 = unknown. Picked after the loop:
    // ioreg lists keys alphabetically, so "IsCharging" comes after both.
    let t = if b.charging { to_full } else { to_empty };
    b.minutes_left = t.filter(|&t| t > 0.0 && t < m::BATTERY_TIME_UNKNOWN).map(|t| t as u32);
    // Apple Silicon reports health from AppleRawMaxCapacity
    let max = if raw_max > 0.0 { raw_max } else { nominal };
    if design > 0.0 && max > 0.0 {
        b.health_pct = Some((max / design * 100.0).min(100.0));
    }
    b
}

/// macOS's battery verdict, the same text the original reads from the power
/// source ("BatteryHealth" / "BatteryHealthCondition").
fn power_health_text() -> String {
    let out = run_cmd("system_profiler", &["SPPowerDataType", "-json"]);
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    if let Some(arr) = v["SPPowerDataType"].as_array() {
        for e in arr {
            if let Some(h) = e["sppower_battery_health_info"]["sppower_battery_health"].as_str() {
                return h.to_string();
            }
        }
    }
    String::new()
}

/// Wi-Fi details from `system_profiler`. Slow: it can take ~10 s, as it
/// may scan for networks, so it runs rarely; [`wifi_name`] fills in between.
fn wifi_details() -> Wifi {
    let mut w = Wifi::default();
    let out = run_cmd("system_profiler", &["SPAirPortDataType", "-json"]);
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    if let Some(ifaces) = v["SPAirPortDataType"][0]["spairport_airport_interfaces"].as_array() {
        for i in ifaces {
            let cur = &i["spairport_current_network_information"];
            if cur.is_object() {
                w.name = cur["_name"].as_str().unwrap_or("").to_string();
                w.security = cur["spairport_security_mode"]
                    .as_str()
                    .unwrap_or("")
                    .replace("spairport_security_mode_", "")
                    .replace('_', " ")
                    .to_uppercase();
                w.phy = cur["spairport_network_phymode"].as_str().unwrap_or("").to_string();
                w.channel = cur["spairport_network_channel"].as_str().unwrap_or("").to_string();
                w.signal = cur["spairport_signal_noise"].as_str().unwrap_or("").to_string();
            }
        }
    }
    w
}

/// The network's name and security from `ipconfig` (fast), over what
/// [`wifi_details`] last found.
fn wifi_name(details: &Wifi) -> Wifi {
    let mut w = details.clone();
    if w.name.contains("redacted") {
        w.name.clear();
    }
    // macOS hides the network name without Location access
    apply_ipconfig(&mut w, &run_cmd("ipconfig", &["getsummary", "en0"]));
    if w.name.is_empty() {
        w.name = if w.security.is_empty() { "Network".into() } else { "Wi-Fi".into() };
    }
    w
}

fn apply_ipconfig(w: &mut Wifi, out: &str) {
    for line in out.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("SSID : ") {
            if !v.contains("redacted") {
                w.name = v.to_string();
            }
        }
        if let Some(v) = l.strip_prefix("Security : ") {
            if w.security.is_empty() {
                w.security = v.replace('_', " ");
            }
        }
    }
}

// ---------------------------------------------------------------- speed test

pub use super::speedtest::start as start_speed_test;

/// "Good for" activities, by download speed in Mbps.
pub fn good_for(mbps: f64) -> Vec<&'static str> {
    m::GOOD_FOR
        .iter()
        .filter(|(_, need)| mbps >= *need)
        .map(|(name, _)| *name)
        .collect()
}

// ---------------------------------------------------------------- demo data

/// Fixed readings for screenshots, so the picture never shows personal data.
pub fn demo_snapshot() -> Snapshot {
    Snapshot {
        ready: true,
        model: "MacBook Pro".into(),
        chip: "Apple M3 Max".into(),
        volume_name: "Macintosh HD".into(),
        disk_total: 2_000_000_000_000,
        disk_free: 1_440_000_000_000,
        smart_status: "Verified".into(),
        categories: vec![
            ("Applications".into(), 65_600_000_000),
            ("Documents".into(), 940_700_000),
            ("Downloads".into(), 596_900_000),
            ("Desktop".into(), 188_000),
        ],
        trash_bytes: Some(46_100_000),
        sizing: false,
        mem_total: 68_719_476_736,
        mem_pressure: 17.0,
        mem_pressure_level: 1,
        mem_active: 24_000_000_000,
        mem_wired: 7_600_000_000,
        mem_compressed: 7_000_000_000,
        mem_available: 30_000_000_000,
        swap_used: 4_190_000,
        swap_total: 1_073_741_824,
        cpu_load: 16.0,
        cpu_user: 11.0,
        cpu_system: 5.0,
        cpu_history: demo_series(60, 7, 14.0, 9.0, &[(22, 38.0), (41, 31.0)]),
        mem_history: demo_series(60, 3, 17.0, 3.0, &[]),
        net_history: demo_series(60, 11, 40_000.0, 60_000.0, &[(18, 3_200_000.0), (47, 900_000.0)]),
        thermal: 0,
        uptime_secs: 3 * 86_400 + 5 * 3600,
        battery: Battery {
            present: true,
            percent: 80,
            charging: false,
            plugged: true,
            full: false,
            minutes_left: None,
            cycles: 13,
            design_cycles: 1000,
            health_pct: Some(100.0),
            temp_c: Some(30.3),
            health_text: "Good".into(),
            permanent_failure: false,
        },
        net_up: 3_200_000.0,
        net_down: 20_000.0,
        net_sent_total: 1_200_000_000,
        net_recv_total: 5_400_000_000,
        online: true,
        wifi: Wifi { name: "Home Wi-Fi".into(), security: "WPA3 PERSONAL".into(), phy: "802.11ax".into(), channel: "149 (5GHz, 80MHz)".into(), signal: "-52 dBm / -92 dBm".into(), wired: false },
        devices: devices::demo(),
        hidden_devices: vec![],
        bt_prefs: devices::Prefs::default(),
        speed: SpeedTest::default(),
        charge: charge::View {
            policy: charge::Policy { limit: 80, ..Default::default() },
            helper_ok: Some(true),
            status: charge::Status { supported: true, policy: None, state: charge::State { held: true, hot: false } },
            installing: false,
        },
        power: Some(crate::kit::ioreport::Reading {
            cpu_w: 3.4,
            gpu_w: 0.8,
            ane_w: 0.0,
            ecpu: crate::kit::ioreport::Usage { busy: 0.31, mhz: 1420.0 },
            pcpu: crate::kit::ioreport::Usage { busy: 0.12, mhz: 3180.0 },
            gpu: crate::kit::ioreport::Usage { busy: 0.06, mhz: 440.0 },
        }),
        sys_watts: Some(11.6),
        procs: vec![
            super::procs::Proc { pid: 812, name: "Safari".into(), cpu: 24.0, mine: true },
            super::procs::Proc { pid: 406, name: "WindowServer".into(), cpu: 11.5, mine: false },
            super::procs::Proc { pid: 1290, name: "Xcode".into(), cpu: 6.2, mine: true },
        ],
        apps: [("Safari", 2_400_000.0, 61_000.0), ("Slack", 180_000.0, 24_000.0), ("Music", 96_000.0, 2_000.0)]
            .iter()
            .map(|(n, d, u)| super::traffic::AppRate { name: n.to_string(), down: *d, up: *u })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IOREG: &str = r#"+-o AppleSmartBattery  <class AppleSmartBattery>
    {
      "AppleRawMaxCapacity" = 4500
      "AvgTimeToEmpty" = 65535
      "AvgTimeToFull" = 95
      "CurrentCapacity" = 62
      "CycleCount" = 13
      "DesignCapacity" = 5000
      "DesignCycleCount9C" = 1000
      "ExternalConnected" = Yes
      "FullyCharged" = No
      "IsCharging" = Yes
      "NominalChargeCapacity" = 4400
      "PermanentFailureStatus" = 0
      "Temperature" = 3030
    }
"#;

    #[test]
    fn battery_time_left_follows_charging_state() {
        // IsCharging comes after the time keys; charging must still pick AvgTimeToFull
        let b = parse_battery(IOREG);
        assert!(b.present && b.charging && b.plugged && !b.full);
        assert_eq!(b.percent, 62);
        assert_eq!(b.minutes_left, Some(95));
        assert_eq!(b.cycles, 13);
        assert_eq!(b.design_cycles, 1000);
        assert_eq!(b.health_pct, Some(90.0));
        assert_eq!(b.temp_c, Some(30.3));
        assert!(!b.permanent_failure);

        let on_battery = IOREG.replace("\"IsCharging\" = Yes", "\"IsCharging\" = No").replace("\"AvgTimeToEmpty\" = 65535", "\"AvgTimeToEmpty\" = 240");
        let b = parse_battery(&on_battery);
        assert!(!b.charging);
        assert_eq!(b.minutes_left, Some(240));

        // 65535 means unknown
        let unknown = IOREG.replace("\"IsCharging\" = Yes", "\"IsCharging\" = No");
        assert_eq!(parse_battery(&unknown).minutes_left, None);
    }

    #[test]
    fn no_battery() {
        let b = parse_battery("  \n");
        assert!(!b.present);
    }

    #[test]
    fn memory_split_from_pages() {
        // 16 KB pages: app = anonymous − purgeable
        assert_eq!(mem_split(1000, 200, 300, 50, 16384), (800 * 16384, 300 * 16384, 50 * 16384));
        assert_eq!(mem_split(10, 20, 0, 0, 4096), (0, 0, 0));
    }

    #[test]
    fn wifi_name_from_ipconfig() {
        let mut w = Wifi::default();
        apply_ipconfig(&mut w, "  SSID : <redacted>\n  Security : WPA2_Personal\n");
        assert_eq!((w.name.as_str(), w.security.as_str()), ("", "WPA2 Personal"));
        apply_ipconfig(&mut w, "  SSID : Home\n");
        assert_eq!(w.name, "Home");
    }

    #[test]
    fn cpu_split_shares() {
        let (load, user, sys) = cpu_split([0, 0, 0, 0], [30, 10, 50, 10]);
        assert!((user - 40.0).abs() < 1e-4); // user + nice
        assert!((sys - 10.0).abs() < 1e-4);
        assert!((load - 50.0).abs() < 1e-4);
        assert_eq!(cpu_split([5; 4], [5; 4]), (0.0, 0.0, 0.0));
        // counters going backwards never underflow
        assert_eq!(cpu_split([9; 4], [1; 4]), (0.0, 0.0, 0.0));
    }

    #[test]
    fn levels() {
        assert_eq!(Level::Good.worst(Level::Fair), Level::Fair);
        assert_eq!(Level::Critical.worst(Level::Good), Level::Critical);
        let mut s = demo_snapshot();
        assert_eq!(s.health(), Level::Good);
        // 2 TB disk → threshold 100 GB
        s.disk_free = 80_000_000_000;
        assert_eq!(s.disk_level(), Level::Fair);
        s.disk_free = 40_000_000_000;
        assert_eq!(s.disk_level(), Level::Critical);
        assert_eq!(s.health(), Level::Critical);
        s.mem_pressure_level = 2;
        assert_eq!(s.mem_level(), Level::Fair);
        s.mem_pressure_level = 4;
        assert_eq!(s.mem_level(), Level::Critical);
        s.thermal = 9;
        assert_eq!(s.thermal_word(), "Critical");
        s.uptime_secs = 8 * 86_400;
        assert!(s.uptime_warning());
        assert_eq!(s.uptime_days(), 8);
    }

    #[test]
    fn good_for_by_speed() {
        assert!(good_for(0.1).is_empty());
        assert_eq!(good_for(4.0), vec!["Messaging", "Audio calls", "Music streaming", "Video calls"]);
        assert_eq!(good_for(1000.0).len(), m::GOOD_FOR.len());
    }


}

/// Sample data that looks like a real reading: noise around `base` plus a
/// few spikes, not a smooth curve.
fn demo_series(n: usize, seed: u32, base: f32, spread: f32, spikes: &[(usize, f32)]) -> Vec<f32> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..n)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let noise = (x % 1000) as f32 / 1000.0;
            spikes.iter().find(|(k, _)| *k == i).map(|(_, v)| *v).unwrap_or(base + noise * spread)
        })
        .collect()
}
