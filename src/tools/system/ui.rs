//! Mac health rows for the panel (disk, memory, CPU, battery, network,
//! devices), the panel's title line, and the detail sheet each row opens.

use crate::kit::sync::LockExt;
use super::devices;
use super::devices_ui;
use super::monitor::{self, Level, Snapshot};
use super::ID;
use crate::core::tool::{Cx, PanelHeader, Size};
use crate::kit::glass::*;
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, Ui};

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Good => tint::HEALTH(),
        Level::Fair => AMBER(),
        Level::Critical => RED(),
    }
}

/// The panel's title line: the Mac's name and its overall state.
pub fn header(s: &Snapshot) -> PanelHeader {
    let title = if s.model.is_empty() { "Mac".to_string() } else { s.model.clone() };
    if !s.ready {
        return PanelHeader { title, status: String::new(), dot: TEXT_FAINT() };
    }
    let health = s.health();
    let status = match health {
        Level::Good => "",
        Level::Fair => "Needs a look",
        Level::Critical => "Needs attention",
    };
    PanelHeader { title, status: status.into(), dot: if health == Level::Good { GREEN() } else { level_color(health) } }
}

/// "1.44 TB" -> ("1.44", "TB")
fn split_num(s: &str) -> (String, String) {
    match s.split_once(' ') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (s.to_string(), String::new()),
    }
}

/// One reading, ready for any widget size.
struct Metric {
    glyph: Icon,
    label: String,
    big: String,
    unit: String,
    caption: String,
    bar: Option<(f32, Color32)>,
}

impl Metric {
    fn value(&self) -> String {
        if self.unit.is_empty() || self.unit == "%" { format!("{}{}", self.big, self.unit) } else { format!("{} {}", self.big, self.unit) }
    }
}

fn metric(s: &Snapshot, id: &str) -> Option<Metric> {
    let pct = |f: f32| (f / 100.0).clamp(0.01, 1.0);
    let (glyph, label, (big, unit), caption, bar_) = match id {
        "storage" => {
            let used = if s.disk_total > 0 { 1.0 - s.disk_free as f32 / s.disk_total as f32 } else { 0.0 };
            let c = if s.disk_level() == Level::Good { TEAL() } else { level_color(s.disk_level()) };
            (Icon::Disk, s.volume_name_or(), split_num(&human_bytes(s.disk_free)), "free".to_string(), Some((used.clamp(0.01, 1.0), c)))
        }
        "memory" => {
            let word = match s.mem_level() {
                Level::Good => "normal",
                Level::Fair => "elevated",
                Level::Critical => "high",
            };
            (Icon::Memory, "Memory".into(), (format!("{:.0}", s.mem_pressure), "%".into()), word.to_string(), Some((pct(s.mem_pressure), level_color(s.mem_level()))))
        }
        "cpu" => {
            let c = match s.thermal {
                0 => tint::HEALTH(),
                1 => AMBER(),
                _ => RED(),
            };
            (Icon::Cpu, "CPU".into(), (format!("{:.0}", s.cpu_load), "%".into()), s.thermal_word().to_lowercase(), Some((pct(s.cpu_load), c)))
        }
        "battery" => {
            if s.battery.present {
                let c = if s.battery.percent <= 20 { RED() } else { GREEN() };
                let frac = (s.battery.percent as f32 / 100.0).clamp(0.01, 1.0);
                (Icon::Battery(s.battery.percent), "Battery".into(), (s.battery.percent.to_string(), "%".into()), battery_status(s).to_lowercase(), Some((frac, c)))
            } else {
                (Icon::Battery(100), "Power".into(), ("AC".to_string(), String::new()), "Power adapter".to_string(), None)
            }
        }
        "network" => {
            let name = if s.wifi.name.is_empty() { "Network".to_string() } else { s.wifi.name.clone() };
            (Icon::Wifi, name, split_num(&human_rate(s.net_down)), format!("↑ {}", human_rate(s.net_up)), None)
        }
        "devices" => {
            let n = s.devices.iter().filter(|d| d.live()).count();
            let low = devices::lowest_first(&s.devices).into_iter().find(|d| d.live());
            let caption = if n == 0 { "None connected".to_string() } else { devices_ui::note(&s.devices) };
            match low.as_ref().and_then(|d| d.battery().map(|b| (d, b))) {
                Some((d, b)) => (d.kind.icon(), "Devices".into(), (b.to_string(), "%".into()), caption, Some(((b as f32 / 100.0).clamp(0.02, 1.0), devices::color(b)))),
                None => (Icon::Bluetooth, "Devices".into(), (n.to_string(), "connected".into()), caption, None),
            }
        }
        _ => return None,
    };
    Some(Metric { glyph, label, big, unit, caption, bar: bar_ })
}


/// A Mac Health widget at `size`; a click opens its sheet (the overview
/// opens the Mac Health page).
pub fn widget(mon: &monitor::Shared, id: &str, size: Size, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let s = mon.lock_ok().clone();
    let wid = ui.scope_id().with(("metric", id));
    let sel = cx.is_detail_open(ID, id);
    if id == "devices" {
        if devices_ui::widget(&s, size, ui, r, wid, sel).clicked() {
            cx.toggle_detail(ID, id);
        }
        return;
    }
    let Some(mt) = metric(&s, id) else { return };
    let col = tint::HEALTH();
    let resp = match size {
        Size::S11 => match mt.bar {
            Some((f, c)) => {
                // a name with the number: "CPU 16%", "1.44 TB free"
                let cap = if id == "storage" || id == "battery" { mt.value() } else { format!("{} {}", short_label(id), mt.value()) };
                tile::small_ring(ui, r, wid, mt.glyph, f, c, &cap, sel)
            }
            None => tile::small(ui, r, wid, mt.glyph, col, &mt.value().replace("/sec", "/s"), &mt.label, sel),
        },
        Size::S12 => tile::wide(ui, r, wid, mt.glyph, col, &mt.label, &mt.value(), mt.bar.ok_or(mt.caption.as_str()), sel),
        // Storage's Row is its old Medium: the free space, a bar across, the folders
        Size::S14 if id == "storage" => {
            let resp = tile::card(ui, r, wid, sel);
            let p = ui.painter();
            let body = tile::head(p, r, mt.glyph, col, &mt.label, "");
            let big = p.text(Pos2::new(body.left(), body.bottom() - 12.0), Align2::LEFT_BOTTOM, &mt.big, bold(28.0), TEXT());
            p.text(Pos2::new(big.right() + 4.0, big.bottom() - 5.0), Align2::LEFT_BOTTOM, &mt.unit, font(13.0), TEXT_DIM());
            if s.disk_total > 0 {
                p.text(Pos2::new(body.right(), big.bottom() - 5.0), Align2::RIGHT_BOTTOM, format!("of {}", human_bytes(s.disk_total)), font(12.0), TEXT_DIM());
                let used = 1.0 - s.disk_free as f32 / s.disk_total as f32;
                bar(p, Rect::from_min_size(Pos2::new(body.left(), body.bottom() - 6.0), egui::Vec2::new(body.width(), 6.0)), used, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
            }
            resp
        }
        Size::S14 => {
            // CPU's graph is the point of its Row, so it gets most of the width
            let ratio = if id == "cpu" { 0.32 } else { 0.47 };
            let (resp, right) = tile::row_split_at(ui, r, wid, mt.glyph, col, &mt.label, &mt.value(), &mt.caption, sel, ratio);
            row_body(&s, id, &mt, ui.painter(), right);
            resp
        }
        _ => {
            let resp = tile::card(ui, r, wid, sel);
            let p = ui.painter();
            let note = if id == "network" { "" } else { mt.caption.as_str() };
            let body = tile::head(p, r, mt.glyph, col, &mt.label, note);
            medium_body(&s, id, &mt, p, body);
            resp
        }
    };
    if resp.clicked() {
        cx.toggle_detail(ID, id);
    }
}

/// A list line: label, optional bar, value.
type Line = (String, Option<(f32, Color32)>, String);

/// A history graph with its time span under it ("2 min ago" … "now").
fn spark_span(p: &egui::Painter, r: Rect, values: &[f32], color: Color32) {
    let gr = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - 13.0));
    tile::spark(p, gr, values, color, None);
    let span = values.len() as u64 * crate::kit::mechanics::MONITOR_REFRESH_MS / 1000;
    let start = if span >= 60 { format!("{} min ago", span / 60) } else { format!("{span} s ago") };
    p.text(Pos2::new(r.left(), r.bottom()), Align2::LEFT_BOTTOM, start, ty::caption(), ty::caption_color());
    p.text(Pos2::new(r.right(), r.bottom()), Align2::RIGHT_BOTTOM, "now", ty::caption(), ty::caption_color());
}

/// The right half of a Row metric: its breakdown, or the CPU's history.
fn row_body(s: &Snapshot, id: &str, mt: &Metric, p: &egui::Painter, r: Rect) {
    let lines: Vec<Line> = match id {
        "storage" => {
            let tot = s.disk_total.max(1) as f32;
            let used = s.disk_total.saturating_sub(s.disk_free);
            let mut v: Vec<Line> = vec![("Used".into(), mt.bar, human_bytes(used))];
            v.extend(s.categories.iter().take(2).map(|(n, b)| (n.clone(), Some(((*b as f32 / tot).clamp(0.01, 1.0), tint::HEALTH())), human_bytes(*b))));
            v
        }
        "memory" => {
            let top = s.mem_active.max(s.mem_wired).max(s.mem_compressed).max(1) as f32;
            [("App", s.mem_active), ("Wired", s.mem_wired), ("Compressed", s.mem_compressed)].into_iter().map(|(n, b)| (n.to_string(), Some(((b as f32 / top).clamp(0.01, 1.0), tint::HEALTH())), human_bytes(b))).collect()
        }
        "cpu" => {
            // the graph, with its time span under it
            spark_span(p, r, &s.cpu_history, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
            return;
        }
        "battery" => {
            let b = &s.battery;
            let mut v: Vec<Line> = vec![("Charge".into(), mt.bar, format!("{}%", b.percent))];
            if let Some(h) = b.health_pct {
                v.push(("Health".into(), Some(((h / 100.0).clamp(0.01, 1.0), if h < 80.0 { AMBER() } else { GREEN() })), format!("{h:.0}%")));
            }
            if b.cycles > 0 {
                v.push(("Cycles".into(), None, b.cycles.to_string()));
            }
            if !b.present {
                v.clear();
            }
            v
        }
        "network" => {
            // the left half has both speeds; here, how they moved
            if s.net_history.len() > 1 {
                spark_span(p, r, &s.net_history, tint::HEALTH());
                return;
            }
            vec![("Today".into(), None, human_bytes(s.net_recv_total))]
        }
        _ => Vec::new(),
    };
    let empty = "On power adapter";
    tile::lines(p, r, &lines, empty);
}

/// The body of a Medium metric: the big value with its context on one line,
/// a graph or bar that uses the width, then the breakdown. Bars in a list
/// are scaled to the list's largest item so they read as a comparison.
fn medium_body(s: &Snapshot, id: &str, mt: &Metric, p: &egui::Painter, body: Rect) {
    let big = p.text(body.left_top(), Align2::LEFT_TOP, &mt.big, bold(28.0), TEXT());
    let unit = p.text(Pos2::new(big.right() + 4.0, big.bottom() - 5.0), Align2::LEFT_BOTTOM, &mt.unit, font(13.0), TEXT_DIM());
    // right of the number: a live graph where there is history
    let beside = Rect::from_min_max(Pos2::new(unit.right() + 16.0, big.top() + 4.0), Pos2::new(body.right(), big.bottom() - 2.0));
    let mut y = big.bottom() + 8.0;
    let rel = |items: &[(String, u64)]| -> Vec<Line> {
        let hi = items.iter().map(|(_, b)| *b).max().unwrap_or(1).max(1) as f32;
        items.iter().map(|(n, b)| (n.clone(), Some(((*b as f32 / hi).clamp(0.02, 1.0), tint::HEALTH())), human_bytes(*b))).collect()
    };
    let lines: Vec<Line> = match id {
        "storage" => {
            // used vs free across the width, then the biggest folders
            if s.disk_total > 0 {
                let used = 1.0 - s.disk_free as f32 / s.disk_total as f32;
                let br = Rect::from_min_size(Pos2::new(body.left(), y), egui::Vec2::new(body.width(), 6.0));
                bar(p, br, used, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
                text_fit(p, Pos2::new(body.right(), big.center().y), Align2::RIGHT_CENTER, &format!("of {}", human_bytes(s.disk_total)), font(12.0), TEXT_DIM(), beside.width());
                y += 6.0 + 10.0;
            }
            let items: Vec<(String, u64)> = s.categories.iter().take(3).cloned().collect();
            rel(&items)
        }
        "memory" => {
            if s.mem_history.len() > 1 {
                tile::spark_to(p, beside, &s.mem_history, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()), (s.mem_history.iter().copied().fold(0.0f32, f32::max) * 1.5).max(30.0));
            }
            let items = vec![("App memory".to_string(), s.mem_active), ("Wired".to_string(), s.mem_wired), ("Compressed".to_string(), s.mem_compressed)];
            rel(&items)
        }
        "cpu" => {
            let list = Rect::from_min_max(Pos2::new(body.left(), y), body.max);
            spark_span(p, list.shrink2(egui::Vec2::new(0.0, 2.0)), &s.cpu_history, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
            Vec::new()
        }
        "network" => {
            let list = Rect::from_min_max(Pos2::new(body.left(), y), Pos2::new(body.right(), body.bottom() - 2.0 * 22.0 - 4.0));
            if s.net_history.len() > 1 && list.height() > 30.0 {
                spark_span(p, list, &s.net_history, tint::HEALTH());
                y = list.bottom() + 4.0;
            }
            vec![("Upload".into(), None, human_rate(s.net_up)), ("Today".into(), None, format!("{} in · {} out", human_bytes(s.net_recv_total), human_bytes(s.net_sent_total)))]
        }
        _ => vec![(mt.caption.clone(), mt.bar, String::new())],
    };
    if lines.is_empty() {
        return;
    }
    let list = Rect::from_min_max(Pos2::new(body.left(), y), body.max);
    let lh = (list.height() / lines.len() as f32).min(24.0);
    for (k, (label, frac, value)) in lines.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(list.left(), list.top() + k as f32 * lh), egui::Vec2::new(list.width(), lh));
        tile::bar_line(p, lr, label, *frac, value);
    }
}

trait VolName {
    fn volume_name_or(&self) -> String;
}
impl VolName for Snapshot {
    fn volume_name_or(&self) -> String {
        if self.volume_name.is_empty() {
            "Macintosh HD".into()
        } else {
            self.volume_name.clone()
        }
    }
}

fn battery_status(s: &Snapshot) -> String {
    let b = &s.battery;
    if !b.present {
        return "Power adapter".into();
    }
    if b.full || (b.plugged && b.percent >= 100) {
        "Fully charged".into()
    } else if b.charging {
        "Charging".into()
    } else if b.plugged {
        "Not charging".into()
    } else if let Some(min) = b.minutes_left {
        format!("{} remaining", human_duration(min as u64 * 60))
    } else {
        "Calculating…".into()
    }
}

// ======================================================================= details

/// The detail sheet for a widget id; the sheet draws its own header.
pub fn detail(mon: &monitor::Shared, d: &str, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    use super::sheets::{self, Action, Sheet};
    let kind = match d {
        "storage" => Sheet::Storage,
        "memory" => Sheet::Memory,
        "battery" => Sheet::Battery,
        "cpu" => Sheet::Cpu,
        "network" => Sheet::Network,
        "devices" => Sheet::Devices,
        _ => return,
    };
    if kind == Sheet::Network {
        super::traffic::want(mon, ui.ctx());
    }
    let s = mon.lock_ok().clone();
    match sheets::sheet(ui, rect, kind, &s) {
        Action::Close => cx.close_detail(),
        Action::Open(tool) => cx.open_page(tool, ""),
        Action::SpeedTest(mode) => {
            // the same modal as on the Mac Health page
            monitor::start_speed_test(mon.clone(), ui.ctx().clone(), mode);
            super::speed::open(ui.ctx());
        }
        Action::MeasureFolders => monitor::measure_folders(mon.clone(), ui.ctx().clone()),
        Action::ToggleAlerts => monitor::change_bt_prefs(mon, |p| crate::kit::notify::flip(&mut p.alerts)),
        Action::ToggleMenuBar => monitor::change_bt_prefs(mon, |p| p.menu_bar = !p.menu_bar),
        Action::OpenBluetooth => devices::open_bluetooth_settings(),
        Action::SetLimit(l) => monitor::change_charge(mon, ui.ctx(), |p| p.limit = l),
        Action::ToggleSailing => monitor::change_charge(mon, ui.ctx(), |p| p.sailing = !p.sailing),
        Action::ToggleHeat => monitor::change_charge(mon, ui.ctx(), |p| p.heat = !p.heat),
        Action::InstallHelper => monitor::install_charge_helper(mon, ui.ctx()),
        Action::Quit(pid) => {
            super::procs::quit(pid);
        }
        Action::None => {}
    }
    if kind == Sheet::Network {
        if let Some(mode) = super::speed::modal(ui, &s.speed) {
            monitor::start_speed_test(mon.clone(), ui.ctx().clone(), mode);
        }
    }
}

/// The one-word name a 1:1 tile has room for.
fn short_label(id: &str) -> &'static str {
    match id {
        "cpu" => "CPU",
        "memory" => "Mem",
        "battery" => "Battery",
        "network" => "Wi-Fi",
        _ => "",
    }
}
