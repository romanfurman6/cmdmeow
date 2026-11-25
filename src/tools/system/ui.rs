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
    let title = if s.model.is_empty() { ts!("health.mac").to_string() } else { s.model.clone() };
    if !s.ready {
        return PanelHeader { title, status: String::new(), dot: TEXT_FAINT() };
    }
    let health = s.health();
    let status = match health {
        Level::Good => "",
        Level::Fair => ts!("health.status.fair"),
        Level::Critical => ts!("health.status.critical"),
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
            let c = if s.disk_level() == Level::Good { tint::HEALTH() } else { level_color(s.disk_level()) };
            let cap = if s.disk_total > 0 { t!("health.free_of", size = human_bytes(s.disk_total)).into_owned() } else { ts!("health.free").to_string() };
            (Icon::Disk, s.volume_name_or(), split_num(&human_bytes(s.disk_free)), cap, Some((used.clamp(0.01, 1.0), c)))
        }
        "memory" => {
            let word = match s.mem_level() {
                Level::Good => ts!("health.pressure_word.good"),
                Level::Fair => ts!("health.pressure_word.fair"),
                Level::Critical => ts!("health.pressure_word.critical"),
            };
            (Icon::Memory, ts!("health.memory").into(), (format!("{:.0}", s.mem_pressure), "%".into()), word.to_string(), Some((pct(s.mem_pressure), level_color(s.mem_level()))))
        }
        "cpu" => {
            let c = match s.thermal {
                0 => tint::HEALTH(),
                1 => AMBER(),
                _ => RED(),
            };
            // the hottest chip sensor as a number (the Fans tool reads it); the word without it
            let cap = crate::kit::smc::last_hot().map(|t| t!("unit.celsius", c = t : {:.0}).into_owned()).unwrap_or_else(|| s.thermal_word().to_lowercase());
            (Icon::Cpu, ts!("health.cpu").into(), (format!("{:.0}", s.cpu_load), "%".into()), cap, Some((pct(s.cpu_load), c)))
        }
        "battery" => {
            if s.battery.present {
                let c = if s.battery.percent <= 20 { RED() } else { GREEN() };
                let frac = (s.battery.percent as f32 / 100.0).clamp(0.01, 1.0);
                (Icon::Battery(s.battery.percent), ts!("health.battery").into(), (s.battery.percent.to_string(), "%".into()), battery_status(s).to_lowercase(), Some((frac, c)))
            } else {
                (Icon::Battery(100), ts!("health.power").into(), (ts!("health.ac").to_string(), String::new()), ts!("health.power_adapter").to_string(), None)
            }
        }
        "network" => {
            let name = s.wifi.label();
            let (n, u) = split_num(&human_rate(s.net_down));
            (Icon::Wifi, name, (format!("↓ {}", n.strip_suffix(".0").unwrap_or(&n)), u), format!("↑ {}", human_rate(s.net_up)), None)
        }
        "devices" => {
            let n = s.devices.iter().filter(|d| d.live()).count();
            let low = devices::lowest_first(&s.devices).into_iter().find(|d| d.live());
            let caption = if n == 0 { ts!("health.devices_none").to_string() } else { devices_ui::note(&s.devices) };
            match low.as_ref().and_then(|d| d.battery().map(|b| (d, b))) {
                Some((d, b)) => (d.kind.icon(), ts!("health.devices").into(), (b.to_string(), "%".into()), caption, Some(((b as f32 / 100.0).clamp(0.02, 1.0), devices::color(b)))),
                None => (Icon::Bluetooth, ts!("health.devices").into(), (n.to_string(), ts!("health.connected").into()), caption, None),
            }
        }
        _ => return None,
    };
    Some(Metric { glyph, label, big, unit, caption, bar: bar_ })
}


/// A Mac Health widget at `size`; a click opens the Mac Health page with
/// that card's details.
pub fn widget(mon: &monitor::Shared, id: &str, size: Size, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let s = mon.lock_ok().clone();
    let wid = ui.scope_id().with(("metric", id));
    if id == "devices" {
        if devices_ui::widget(&s, size, ui, r, wid, false).clicked() {
            cx.open_page(ID, id);
        }
        return;
    }
    let Some(mt) = metric(&s, id) else { return };
    let col = tint::HEALTH();
    let caption = cap_first(&mt.caption);
    let resp = match size {
        // W4: the ring and the value; the name and the rest in the tip
        Size::S11 => match mt.bar {
            Some((f, c)) => tile::small_ring(ui, r, wid, mt.glyph, f, c, &mt.value(), false).on_hover_text(format!("{} · {}", mt.label, caption)),
            None => tile::small(ui, r, wid, mt.glyph, col, &mt.value().replace("/sec", "/s"), &mt.label, false),
        },
        Size::S12 => tile::wide_full(ui, r, wid, mt.glyph, col, mt.bar, &mt.label, &mt.value(), &caption, ty::caption_color(), false),
        Size::S14 => {
            let (resp, right) = tile::row_split_full(ui, r, wid, mt.glyph, col, mt.bar, &mt.label, &mt.value(), &caption, ty::caption_color(), false);
            row_body(&s, id, &mt, ui.painter(), right);
            resp
        }
        _ => {
            let resp = tile::card(ui, r, wid, false);
            let p = ui.painter();
            // W6: the caption on the right; Wi-Fi shows today's totals there
            let note = if id == "network" { t!("health.today_in_out", received = human_bytes(s.net_recv_total), sent = human_bytes(s.net_sent_total)).into_owned() } else { caption.clone() };
            let body = tile::head(p, r, mt.glyph, col, &mt.label, &note);
            medium_body(&s, id, &mt, p, body);
            resp
        }
    };
    if resp.clicked() {
        cx.open_page(ID, id);
    }
}

/// A list line: label, optional bar, value.
type Line = (String, Option<(f32, Color32)>, String);

fn spark_span(p: &egui::Painter, r: Rect, values: &[f32], color: Color32) {
    tile::spark_span(p, r, values, color, None);
}

/// The right half of a Row metric: its breakdown, or the CPU's history.
fn row_body(s: &Snapshot, id: &str, mt: &Metric, p: &egui::Painter, r: Rect) {
    let lines: Vec<Line> = match id {
        "storage" => {
            // one disk bar across the width, used and the total under it
            if s.disk_total > 0 {
                let used = s.disk_total.saturating_sub(s.disk_free);
                let top = r.center().y - (8.0 + 6.0 + 14.0) / 2.0;
                let br = Rect::from_min_size(Pos2::new(r.left(), top), egui::Vec2::new(r.width(), 8.0));
                bar(p, br, used as f32 / s.disk_total as f32, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
                let y = br.bottom() + 6.0 + 7.0;
                let tr = p.text(Pos2::new(r.right(), y), Align2::RIGHT_CENTER, human_bytes(s.disk_total), ty::caption(), ty::caption_color());
                text_fit(p, Pos2::new(r.left(), y), Align2::LEFT_CENTER, &t!("health.used", size = human_bytes(used)), ty::caption(), ty::caption_color(), tr.left() - r.left() - 8.0);
                return;
            }
            Vec::new()
        }
        "memory" => {
            let top = s.mem_active.max(s.mem_wired).max(s.mem_compressed).max(1) as f32;
            [(ts!("health.mem.apps"), s.mem_active), (ts!("health.mem.wired"), s.mem_wired), (ts!("health.mem.compressed"), s.mem_compressed)].into_iter().map(|(n, b)| (n.to_string(), Some(((b as f32 / top).clamp(0.01, 1.0), tint::HEALTH())), human_bytes(b))).collect()
        }
        "cpu" => {
            // the graph, with its time span under it
            spark_span(p, r, &s.cpu_history, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
            return;
        }
        "battery" => {
            let b = &s.battery;
            let mut v: Vec<Line> = vec![(ts!("health.charge").into(), mt.bar, format!("{}%", b.percent))];
            if let Some(h) = b.health_pct {
                v.push((ts!("health.health").into(), Some(((h / 100.0).clamp(0.01, 1.0), if h < 80.0 { AMBER() } else { GREEN() })), format!("{h:.0}%")));
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
            vec![(ts!("health.today").into(), None, human_bytes(s.net_recv_total))]
        }
        _ => Vec::new(),
    };
    let empty = ts!("health.on_adapter");
    tile::lines(p, r, &lines, empty);
}

/// The body of a Medium metric: the big value with its context on one line,
/// a graph or bar that uses the width, then the breakdown. Bars in a list
/// are scaled to the list's largest item so they read as a comparison.
fn medium_body(s: &Snapshot, id: &str, mt: &Metric, p: &egui::Painter, body: Rect) {
    let big = p.text(body.left_top(), Align2::LEFT_TOP, &mt.big, bold(28.0), TEXT());
    let gap = if mt.unit == "%" { 0.0 } else { 4.0 };
    let unit = p.text(Pos2::new(big.right() + gap, big.bottom() - 5.0), Align2::LEFT_BOTTOM, &mt.unit, font(13.0), TEXT_DIM());
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
                text_fit(p, Pos2::new(body.right(), big.center().y), Align2::RIGHT_CENTER, &t!("health.of", size = human_bytes(s.disk_total)), font(12.0), TEXT_DIM(), beside.width());
                y += 6.0 + 10.0;
            }
            let items: Vec<(String, u64)> = s.categories.iter().take(3).map(|(n, b)| (crate::kit::fs::place_name(n), *b)).collect();
            rel(&items)
        }
        "memory" => {
            let items = vec![(ts!("health.mem.apps").to_string(), s.mem_active), (ts!("health.mem.wired").to_string(), s.mem_wired), (ts!("health.mem.compressed").to_string(), s.mem_compressed)];
            rel(&items)
        }
        "cpu" => {
            let list = Rect::from_min_max(Pos2::new(body.left(), y), body.max);
            spark_span(p, list.shrink2(egui::Vec2::new(0.0, 2.0)), &s.cpu_history, mt.bar.map(|b| b.1).unwrap_or(tint::HEALTH()));
            Vec::new()
        }
        "network" => {
            let list = Rect::from_min_max(Pos2::new(body.left(), y), Pos2::new(body.right(), body.bottom() - 22.0 - 4.0));
            if s.net_history.len() > 1 && list.height() > 30.0 {
                spark_span(p, list, &s.net_history, tint::HEALTH());
                y = list.bottom() + 4.0;
            }
            vec![(ts!("health.upload").into(), None, mt.caption.clone())]
        }
        _ => vec![(mt.caption.clone(), mt.bar, String::new())],
    };
    if lines.is_empty() {
        return;
    }
    let list = Rect::from_min_max(Pos2::new(body.left(), y), body.max);
    let lh = (list.height() / lines.len() as f32).min(24.0);
    let cols = tile::bar_cols(p, &lines.iter().map(|l| (l.0.as_str(), l.2.as_str())).collect::<Vec<_>>());
    for (k, (label, frac, value)) in lines.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(list.left(), list.top() + k as f32 * lh), egui::Vec2::new(list.width(), lh));
        tile::bar_line(p, lr, label, *frac, value, cols);
    }
}

trait VolName {
    fn volume_name_or(&self) -> String;
}
impl VolName for Snapshot {
    fn volume_name_or(&self) -> String {
        if self.volume_name.is_empty() {
            ts!("health.macintosh_hd").into()
        } else {
            self.volume_name.clone()
        }
    }
}

fn battery_status(s: &Snapshot) -> String {
    let b = &s.battery;
    if !b.present {
        return ts!("health.power_adapter").into();
    }
    if b.full || (b.plugged && b.percent >= 100) {
        ts!("health.bat.full").into()
    } else if b.charging {
        ts!("health.bat.charging").into()
    } else if b.plugged {
        ts!("health.bat.not_charging").into()
    } else if let Some(min) = b.minutes_left {
        t!("health.bat.remaining", time = human_duration(min as u64 * 60)).into_owned()
    } else {
        ts!("health.bat.calculating").into()
    }
}

/// The one-word name a 1:1 tile has room for.
/// "normal" -> "Normal": a caption line starts with a capital.
fn cap_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

