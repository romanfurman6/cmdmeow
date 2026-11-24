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

fn widgets() -> [WidgetSpec; 6] {
    [
        WidgetSpec { id: "storage", title: ts!("health.widget.storage.title"), about: ts!("health.widget.storage.about"), sizes: &[Size::S11, Size::S12, Size::S14], size: Size::S12, default_on: true },
        WidgetSpec { id: "memory", title: ts!("health.widget.memory.title"), about: ts!("health.widget.memory.about"), sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
        WidgetSpec { id: "cpu", title: ts!("health.widget.cpu.title"), about: ts!("health.widget.cpu.about"), sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
        WidgetSpec { id: "battery", title: ts!("health.widget.battery.title"), about: ts!("health.widget.battery.about"), sizes: &[Size::S11, Size::S12, Size::S14], size: Size::S12, default_on: true },
        WidgetSpec { id: "network", title: ts!("health.widget.network.title"), about: ts!("health.widget.network.about"), sizes: SMALL_TO_MEDIUM, size: Size::S12, default_on: true },
        WidgetSpec { id: "devices", title: ts!("health.widget.devices.title"), about: ts!("health.widget.devices.about"), sizes: &[Size::S11, Size::S12, Size::S14, Size::S24, Size::S44], size: Size::S14, default_on: true },
    ]
}

impl Tool for System {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        ts!("health.name")
    }
    fn about(&self) -> &'static str {
        ts!("health.about")
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
        widgets().to_vec()
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
            v.push(rec(m::PRIORITY_DRIVE_HEALTH, ts!("health.rec.drive").into(), ts!("health.rec.disk_details"), card("storage")));
        }
        if s.battery_level() == Level::Critical {
            v.push(rec(m::PRIORITY_BATTERY_HEALTH, ts!("health.rec.battery").into(), ts!("health.rec.battery_details"), card("battery")));
        }
        if s.disk_level() != Level::Good {
            v.push(rec(m::PRIORITY_OUT_OF_DISK, t!("health.rec.low_disk", size = human_bytes(m::disk_alert_threshold(s.disk_total))).into_owned(), ts!("health.rec.open_disk_map"), page("lens")));
        }
        if s.mem_level() != Level::Good {
            v.push(rec(15, ts!("health.rec.memory").into(), ts!("health.rec.memory_details"), card("memory")));
        }
        if s.uptime_warning() {
            // macOS asks before it restarts
            v.push(rec(10, tn!("health.rec.restart", n = s.uptime_days()).into_owned(), ts!("health.rec.restart_action"), Action::Restart));
        }
        if s.trash_overfill() {
            v.push(rec(m::PRIORITY_TRASH_OVERFILL, t!("health.rec.trash_full", size = human_bytes(s.trash_bytes.unwrap_or(0))).into_owned(), ts!("health.rec.empty_trash"), Action::EmptyTrash));
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
        let w = widgets().into_iter().find(|w| w.id == view)?;
        self.route = Some(w.id.to_string());
        Some(View::Page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_and_trash_tips_read_in_english() {
        let mut s = monitor::demo_snapshot();
        s.uptime_secs = 9 * m::SECONDS_PER_DAY;
        s.trash_bytes = Some(m::TRASH_SIZE_THRESHOLD + 1);
        let sys = System { mon: Arc::new(Mutex::new(s.clone())), ai: crate::kit::aicheck::Check::new(ID, true), route: None };
        let recs = sys.recommendations();
        let restart = recs.iter().find(|r| r.go == Action::Restart).unwrap();
        assert_eq!((restart.title.as_str(), restart.action.as_str()), ("Restart recommended · up 9 days", "Restart…"));
        let trash = recs.iter().find(|r| r.go == Action::EmptyTrash).unwrap();
        assert_eq!(trash.title, format!("Trash is full · {}", human_bytes(m::TRASH_SIZE_THRESHOLD + 1)));
        assert_eq!(trash.action, "Empty Trash");
        assert_eq!(tn!("health.rec.restart", n = 1u64), "Restart recommended · up 1 day");
        // the device row menu
        assert_eq!([ts!("health.alert_below"), ts!("health.never"), ts!("health.hide"), ts!("health.forget")], ["Alert below", "Never", "Hide", "Forget"]);
    }
}
