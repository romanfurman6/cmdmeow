//! Mac health tool: live readings of disk, memory, battery, CPU, network and
//! Bluetooth devices, shown as panel widgets with a detail sheet each.

use crate::kit::sync::LockExt;
mod ble;
mod devices;
mod devices_store;
mod devices_ui;
pub mod monitor;
mod page;
mod procs;
mod sheets;
mod speed;
mod speedtest;
mod traffic;
mod ui;

use crate::core::tool::{Action, Badge, Cx, PanelHeader, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::glass::{human_bytes, tint, Icon};
use crate::kit::mechanics as m;
use egui::{Rect, Ui};
use monitor::Level;
use std::sync::{Arc, Mutex};

pub const ID: ToolId = "system";

pub struct System {
    mon: monitor::Shared,
    ai: crate::kit::aicheck::Check,
}

impl System {
    pub fn new(env: &ToolEnv) -> Self {
        let mon = if env.demo { Arc::new(Mutex::new(monitor::demo_snapshot())) } else { monitor::start(env.ctx.clone(), false) };
        System { mon, ai: crate::kit::aicheck::Check::new(ID, env.demo) }
    }
}

const SMALL_TO_MEDIUM: &[Size] = UP_TO_MEDIUM;

const WIDGETS: [WidgetSpec; 6] = [
    WidgetSpec { id: "storage", title: "Storage", about: "Free space on your startup disk.", sizes: &[Size::S11, Size::S12, Size::S14], size: Size::S12, default_on: true },
    WidgetSpec { id: "memory", title: "Memory", about: "Memory pressure and what uses it.", sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
    WidgetSpec { id: "cpu", title: "CPU", about: "Load and temperature, with the last minute.", sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
    WidgetSpec { id: "battery", title: "Battery", about: "Charge and time left.", sizes: &[Size::S11, Size::S12, Size::S14], size: Size::S12, default_on: true },
    WidgetSpec { id: "network", title: "Network", about: "Wi-Fi and live speed.", sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
    WidgetSpec { id: "devices", title: "Devices", about: "Bluetooth devices and their batteries.", sizes: &[Size::S11, Size::S12, Size::S14, Size::S24, Size::S44], size: Size::S14, default_on: true },
];

impl Tool for System {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Mac Health"
    }
    fn about(&self) -> &'static str {
        "Live disk, memory, CPU, battery, network and connected devices"
    }
    fn busy(&self) -> bool {
        self.ai.phase() == crate::kit::aicheck::Phase::Running
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::Pulse, color: tint::HEALTH() }
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::may(Perm::FullDisk), Need::may(Perm::Bluetooth), Need::may(Perm::Notifications)]
    }
    fn set_on(&mut self, on: bool) {
        monitor::set_paused(!on);
    }

    fn section(&self) -> &'static str {
        "Monitor"
    }
    fn panel_header(&self) -> Option<PanelHeader> {
        Some(ui::header(&self.mon.lock_ok()))
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        WIDGETS.to_vec()
    }
    fn widget_loading(&self, _widget: &str) -> bool {
        !self.mon.lock_ok().ready
    }
    fn widget_ui(&mut self, widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::widget(&self.mon, widget, size, ui, rect, cx);
    }
    fn detail_ui(&mut self, detail: &str, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::detail(&self.mon, detail, ui, rect, cx);
    }

    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let mut recs = self.recommendations();
        recs.sort_by(|a, b| b.priority.cmp(&a.priority));
        page::page(&self.mon, &mut self.ai, &recs, ui, rect, cx);
    }

    /// Ranked like the original's health recommendations (AssistantDomain:
    /// drive health 100, battery health 80, out of disk space 20, Trash
    /// overfill 5); memory and restart are slotted in by us.
    fn recommendations(&self) -> Vec<Recommendation> {
        let s = self.mon.lock_ok().clone();
        let detail = |d: &str| Action::ShowDetail { tool: ID.into(), detail: d.into(), toggle: true };
        let page = |t: &str| Action::OpenPage { tool: t.into(), route: String::new() };
        let mut v = Vec::new();
        if !s.smart_status.is_empty() && !s.smart_status.eq_ignore_ascii_case("Verified") {
            v.push(Recommendation { priority: m::PRIORITY_DRIVE_HEALTH, title: "Your drive needs attention".into(), body: "Back up immediately. This drive is in bad condition and should be replaced.".into(), action: "Show Disk".into(), go: detail("storage") });
        }
        if s.battery_level() == Level::Critical {
            v.push(Recommendation { priority: m::PRIORITY_BATTERY_HEALTH, title: "Battery health is critical".into(), body: "Your battery is in bad condition and should be replaced soon.".into(), action: "Show Battery".into(), go: detail("battery") });
        }
        if s.disk_level() != Level::Good {
            v.push(Recommendation { priority: m::PRIORITY_OUT_OF_DISK, title: "Low disk space".into(), body: format!("Less than {} is free. See what takes the most space.", human_bytes(m::disk_alert_threshold(s.disk_total))), action: "Open Disk Map".into(), go: page("lens") });
        }
        if s.mem_level() != Level::Good {
            v.push(Recommendation { priority: 15, title: "Memory is under pressure".into(), body: "Quitting a few unused apps will give your Mac room to breathe.".into(), action: "Show Memory".into(), go: detail("memory") });
        }
        if s.uptime_warning() {
            v.push(Recommendation { priority: 10, title: "Time for a restart".into(), body: format!("Your Mac has been running for {} days. A restart clears temporary files and memory.", s.uptime_days()), action: "Show CPU".into(), go: detail("cpu") });
        }
        if s.trash_overfill() {
            v.push(Recommendation { priority: m::PRIORITY_TRASH_OVERFILL, title: "Empty Trash Bins".into(), body: format!("Your Trash holds {}. Empty it to get the space back.", human_bytes(s.trash_bytes.unwrap_or(0))), action: "Review Cleanup".into(), go: page("cleanup") });
        }
        v
    }

    /// The emptiest Bluetooth device while it is low ("Mouse 14%").
    fn tray_title(&self) -> Option<String> {
        let s = self.mon.lock_ok();
        devices::tray_title(&s.devices, &s.bt_prefs)
    }

    fn demo_view(&mut self, view: &str) -> Option<View> {
        if view == "health-checking" {
            self.ai.set_demo_running();
            return Some(View::Page);
        }
        if view == "health" {
            use crate::kit::aicheck::Check;
            self.ai.set_demo(
                "Chrome helpers are the main load",
                vec![
                    Check::demo_rec("quit", "Quit", "Quit Google Chrome Helper", "68% CPU", "A background tab is playing video", "Google Chrome"),
                    Check::demo_rec("monitor", "Activity Monitor", "Check Docker", "3.2 GB", "VM memory stays reserved while it runs", "Docker"),
                ],
            );
            return Some(View::Page);
        }
        WIDGETS.iter().filter(|w| w.id != "health").find(|w| w.id == view).map(|w| View::Detail(w.id))
    }
}
