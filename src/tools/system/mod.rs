//! Mac health tool: live readings of disk, memory, battery, CPU, network and
//! Bluetooth devices, shown as panel widgets; a click opens the card's details on the page.

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
    /// A card to open on the page, from a panel widget or a tip ("cpu").
    route: Option<String>,
}

impl System {
    pub fn new(env: &ToolEnv) -> Self {
        let mon = if env.demo { Arc::new(Mutex::new(monitor::demo_snapshot())) } else { monitor::start(env.ctx.clone(), false) };
        System { mon, ai: crate::kit::aicheck::Check::new(ID, env.demo), route: None }
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
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let mut recs = self.recommendations();
        recs.sort_by(|a, b| b.priority.cmp(&a.priority));
        page::page(&self.mon, &mut self.ai, &recs, self.route.take(), ui, rect, cx);
    }
    fn open_page(&mut self, route: &str) {
        self.route = (!route.is_empty()).then(|| route.to_string());
    }

    /// Ranked like the original's health recommendations (AssistantDomain:
    /// drive health 100, battery health 80, out of disk space 20, Trash
    /// overfill 5); memory and restart are slotted in by us.
    fn recommendations(&self) -> Vec<Recommendation> {
        let s = self.mon.lock_ok().clone();
        // a card's details on this page
        let card = |d: &str| Action::OpenPage { tool: ID.into(), route: d.into() };
        let page = |t: &str| Action::OpenPage { tool: t.into(), route: String::new() };
        let rec = |priority: u32, title: String, action: &str, go: Action| Recommendation { priority, title, body: String::new(), action: action.into(), go };
        let mut v = Vec::new();
        if !s.smart_status.is_empty() && !s.smart_status.eq_ignore_ascii_case("Verified") {
            v.push(rec(m::PRIORITY_DRIVE_HEALTH, "Drive is failing · back up now".into(), "Disk details", card("storage")));
        }
        if s.battery_level() == Level::Critical {
            v.push(rec(m::PRIORITY_BATTERY_HEALTH, "Battery needs service".into(), "Battery details", card("battery")));
        }
        if s.disk_level() != Level::Good {
            v.push(rec(m::PRIORITY_OUT_OF_DISK, format!("Less than {} free", human_bytes(m::disk_alert_threshold(s.disk_total))), "Open Disk Map", page("lens")));
        }
        if s.mem_level() != Level::Good {
            v.push(rec(15, "Memory pressure high".into(), "Memory details", card("memory")));
        }
        if s.uptime_warning() {
            // macOS asks before it restarts
            v.push(rec(10, format!("Restart recommended · up {} days", s.uptime_days()), "Restart…", Action::Restart));
        }
        if s.trash_overfill() {
            v.push(rec(m::PRIORITY_TRASH_OVERFILL, format!("Trash is full · {}", human_bytes(s.trash_bytes.unwrap_or(0))), "Empty Trash", Action::EmptyTrash));
        }
        v
    }

    fn demo_view(&mut self, view: &str) -> Option<View> {
        if view == "health-checking" {
            self.ai.set_demo_running();
            return Some(View::Page);
        }
        if view == "health-plain" {
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
        // a widget id: the page with that card's details
        let w = WIDGETS.iter().find(|w| w.id == view)?;
        self.route = Some(w.id.to_string());
        Some(View::Page)
    }
}
