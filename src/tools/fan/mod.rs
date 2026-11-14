//! Fan Control tool: fan speeds and chip temperatures, live, and the fan
//! mode (Auto, Quiet, Fixed, Curve, Max). A card for the panel, a detail
//! sheet and a page in the full window. Reading needs no admin rights;
//! changing fan speed goes through the root helper in [`helper`].

use crate::kit::sync::LockExt;
pub mod control;
pub mod data;
pub mod helper;
mod page;
pub mod probe;
mod ui;

use crate::core::tool::{Badge, Cx, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::activity::{self, Kick};
use crate::kit::glass::{tint, Icon};
use control::{Guard, Mode, Settings};
use data::Snapshot;
use egui::{Rect, Ui};
use helper::{ask, Request};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ID: ToolId = "fan";
const CARD: &str = "fans";
const SHEET: &str = "fans";

pub type Shared = Arc<Mutex<Snapshot>>;

/// Whether the helper is there to take orders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HelperState {
    #[default]
    Unknown,
    Missing,
    /// Running, but from an older Cmd+Meow.
    Outdated,
    Ready,
}

/// The control side, shared between the UI and the background thread.
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub settings: Settings,
    pub helper: HelperState,
    pub guard: Guard,
    /// The helper holds the fans now.
    pub manual: bool,
    /// The admin prompt is up.
    pub busy: bool,
    /// Last install error, shown under the install button.
    pub error: String,
}

impl Live {
    /// The mode wants the helper and it isn't ready.
    pub fn needs_helper(&self) -> bool {
        self.settings.mode != Mode::Auto && self.helper != HelperState::Ready
    }
}

/// What the UI asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// New settings; `save` once the user lets go of a slider or point.
    Set(Settings, bool),
    Install,
    Uninstall,
}

/// Fan Control's privileged helper, as a setup step.
const HELPER: crate::kit::perms::Perm = crate::kit::perms::Perm::Own { id: "fan-helper", name: "Fan helper", icon: crate::kit::glass::Icon::Lock, why: "", ask: "Install", hint: "" };

pub struct FanControl {
    snap: Shared,
    live: Arc<Mutex<Live>>,
    on: Arc<AtomicBool>,
    /// Wakes the background thread early after a change.
    kick: Arc<Kick>,
    demo: bool,
    ctx: egui::Context,
}

impl FanControl {
    pub fn new(env: &ToolEnv) -> Self {
        let on = Arc::new(AtomicBool::new(true));
        let kick = Arc::new(Kick::new());
        let live = Arc::new(Mutex::new(Live { settings: if env.demo { Settings::default() } else { control::load() }, ..Default::default() }));
        let snap = if env.demo { Arc::new(Mutex::new(data::demo())) } else { start(env.ctx.clone(), on.clone(), live.clone(), kick.clone()) };
        FanControl { snap, live, on, kick, demo: env.demo, ctx: env.ctx.clone() }
    }

    fn run(&mut self, cmds: Vec<Cmd>, ctx: &egui::Context) {
        for c in cmds {
            match c {
                Cmd::Set(s, save) => {
                    if save && !self.demo {
                        control::save(&s);
                    }
                    self.live.lock_ok().settings = s;
                    self.kick.kick();
                }
                Cmd::Install | Cmd::Uninstall if self.demo => {}
                Cmd::Install | Cmd::Uninstall => {
                    let install = c == Cmd::Install;
                    {
                        let mut l = self.live.lock_ok();
                        if l.busy {
                            continue;
                        }
                        l.busy = true;
                        l.error.clear();
                    }
                    let (live, kick, ctx) = (self.live.clone(), self.kick.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let r = if install { helper::install() } else { helper::uninstall() };
                        let mut l = live.lock_ok();
                        l.busy = false;
                        l.error = r.err().unwrap_or_default();
                        if !install && l.error.is_empty() {
                            l.helper = HelperState::Missing;
                            l.manual = false;
                        }
                        drop(l);
                        kick.kick();
                        ctx.request_repaint();
                    });
                }
            }
        }
    }

    fn view(&self) -> (Snapshot, Live) {
        (self.snap.lock_ok().clone(), self.live.lock_ok().clone())
    }
}

impl Drop for FanControl {
    fn drop(&mut self) {
        if !self.demo && self.live.lock_ok().manual {
            ask(&Request::Auto);
        }
    }
}

/// Read the sensors every [`data::SAMPLE_SECS`] on a background thread
/// while the tool is on, and tell the helper what the fans should do.
fn start(ctx: egui::Context, on: Arc<AtomicBool>, live: Arc<Mutex<Live>>, kick: Arc<Kick>) -> Shared {
    let snap: Shared = Arc::new(Mutex::new(Snapshot::default()));
    let out = snap.clone();
    std::thread::Builder::new()
        .name("fan-probe".into())
        .spawn(move || {
            let mut probe = probe::Probe::open();
            let chip = chip_name();
            let mut held = false;
            let mut steady = control::Steady::default();
            let mut read_at = activity::Every::default();
            loop {
                let on_now = on.load(Ordering::Relaxed);
                if on_now && read_at.due(Duration::from_secs(data::SAMPLE_SECS)) {
                    let r = probe.read();
                    let thermal = crate::kit::native::thermal_state();
                    let mut s = out.lock_ok();
                    s.loaded = true;
                    s.has_smc = r.has_smc;
                    s.fans = r.fans;
                    s.sensors = data::merge(r.hid, r.smc_temps);
                    s.thermal = thermal;
                    s.chip = chip.clone();
                    s.push_history();
                }
                // what the fans should do
                let s = out.lock_ok().clone();
                let (settings, tripped) = {
                    let mut l = live.lock_ok();
                    let tripped = control::hottest(&s).is_some_and(|h| l.guard.update(h));
                    (l.settings.clone(), tripped)
                };
                let want = on_now && s.loaded && !s.fans.is_empty() && settings.mode != Mode::Auto && !tripped;
                if !want {
                    steady = control::Steady::default();
                }
                // nothing to hold and nobody looking (or the tool is off):
                // leave the helper alone, its state is asked when shown
                if !want && !held && !(on_now && activity::visible()) {
                    let wait = if on_now { Duration::from_secs(data::SAMPLE_SECS) } else { Duration::from_secs(3600) };
                    activity::nap(wait, Some(&kick));
                    continue;
                }
                let reply = if want {
                    let temp = settings.curve.source.read(&s);
                    let rpm = settings.steady_targets(&s.fans, temp, &mut steady);
                    if rpm.iter().any(Option::is_some) {
                        held = true;
                        ask(&Request::Set { rpm })
                    } else {
                        ask(&Request::Auto)
                    }
                } else if held {
                    held = false;
                    ask(&Request::Auto)
                } else {
                    ask(&Request::Hello)
                };
                {
                    let mut l = live.lock_ok();
                    l.helper = match &reply {
                        None => HelperState::Missing,
                        Some(r) if r.version != helper::VERSION => HelperState::Outdated,
                        Some(_) => HelperState::Ready,
                    };
                    l.manual = reply.as_ref().is_some_and(|r| r.manual);
                }
                activity::repaint(&ctx);
                // sleep until the next sample, a change from the UI, or the
                // panel opening
                activity::nap(Duration::from_secs(data::SAMPLE_SECS), Some(&kick));
            }
        })
        .ok();
    snap
}

/// "Apple M3 Max", from `machdep.cpu.brand_string`.
fn chip_name() -> String {
    #[cfg(target_os = "macos")]
    {
        let name = std::ffi::CString::new("machdep.cpu.brand_string").unwrap();
        let mut buf = [0u8; 128];
        let mut len = buf.len();
        let ok = unsafe { libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) } == 0;
        if ok {
            return String::from_utf8_lossy(&buf[..len.saturating_sub(1)]).trim().to_string();
        }
    }
    String::new()
}

fn badge() -> Badge {
    Badge { icon: Icon::Fan, color: tint::FAN() }
}

impl Tool for FanControl {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Fan Control"
    }
    fn about(&self) -> &'static str {
        "Chip temperatures and manual fan speed"
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::Need;
        vec![Need::may(HELPER)]
    }

    fn own_status(&self, id: &str) -> crate::kit::perms::Status {
        let l = self.live.lock_ok();
        if id != "fan-helper" || l.busy {
            crate::kit::perms::Status::NotAsked
        } else if l.helper == HelperState::Ready {
            crate::kit::perms::Status::Allowed
        } else {
            crate::kit::perms::Status::NotAsked
        }
    }

    fn own_progress(&self, id: &str) -> Option<(bool, String)> {
        let l = self.live.lock_ok();
        (id == "fan-helper").then(|| (l.busy, l.error.clone()))
    }

    fn ask_own(&mut self, id: &str) {
        if id == "fan-helper" {
            let ctx = self.ctx.clone();
            self.run(vec![Cmd::Install], &ctx);
        }
    }

    fn section(&self) -> &'static str {
        "Monitor"
    }
    // no live number in the sidebar (Roman 2026-11-13): the page shows it
    fn sidebar_meta(&self) -> String {
        String::new()
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: "Fans", about: "Fan speed and the hottest part, live.", sizes: UP_TO_MEDIUM, size: Size::S14, default_on: true }]
    }
    fn widget_loading(&self, _widget: &str) -> bool {
        !self.snap.lock_ok().loaded
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let s = self.snap.lock_ok().clone();
        ui::widget(&s, size, ui, rect, cx);
    }
    fn detail_ui(&mut self, _detail: &str, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let (s, l) = self.view();
        let cmds = ui::sheet(&s, &l, ui, rect, cx);
        self.run(cmds, ui.ctx());
    }
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let (s, l) = self.view();
        let cmds = page::page(&s, &l, ui, rect, cx);
        self.run(cmds, ui.ctx());
    }
    fn set_on(&mut self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
        self.kick.kick();
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "fan" => Some(View::Page),
            "fan-detail" => Some(View::Detail(SHEET)),
            "fan-curve" | "fan-fixed" | "fan-install" => {
                let mut l = self.live.lock_ok();
                l.settings.mode = match view {
                    "fan-curve" => Mode::Curve,
                    "fan-fixed" => Mode::Fixed,
                    _ => Mode::Quiet,
                };
                l.helper = if view == "fan-install" { HelperState::Missing } else { HelperState::Ready };
                l.manual = view != "fan-install";
                Some(View::Page)
            }
            "fan-air" => {
                *self.snap.lock_ok() = data::demo_fanless();
                Some(View::Page)
            }
            "fan-air-detail" => {
                *self.snap.lock_ok() = data::demo_fanless();
                Some(View::Detail(SHEET))
            }
            _ => None,
        }
    }
}
