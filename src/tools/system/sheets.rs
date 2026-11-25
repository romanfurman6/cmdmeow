//! Mac Health card details (the popup over the page): one template for
//! storage, memory, battery, CPU, network and devices. Header with a tint chip, a big number
//! with a stacked bar (or a sparkline), a legend card, a facts card and
//! buttons at the bottom right. The sheet reports what the user asked for
//! instead of acting on the app.

use super::monitor::{self, Level, Snapshot};
use crate::kit::glass::*;
use crate::kit::mechanics as m;
use egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sheet {
    Storage,
    Memory,
    Battery,
    Cpu,
    Network,
    Devices,
}

impl Sheet {
    /// The sheet for a widget id ("cpu").
    pub fn from_id(id: &str) -> Option<Sheet> {
        Some(match id {
            "storage" => Sheet::Storage,
            "memory" => Sheet::Memory,
            "battery" => Sheet::Battery,
            "cpu" => Sheet::Cpu,
            "network" => Sheet::Network,
            "devices" => Sheet::Devices,
            _ => return None,
        })
    }
}

/// What the user clicked in a sheet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Close,
    /// Open another tool's page by id ("cleanup", "lens").
    Open(&'static str),
    SpeedTest(super::monitor::SpeedMode),
    MeasureFolders,
    OpenBluetooth,
    /// Charge limit, a value from [`charge::LIMITS`].
    SetLimit(u8),
    ToggleSailing,
    ToggleHeat,
    InstallHelper,
    /// Quit a process (asked twice in the row first).
    Quit(u32),
}

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Good => GREEN(),
        Level::Fair => AMBER(),
        Level::Critical => RED(),
    }
}


struct Fact {
    label: String,
    value: String,
    col: Color32,
    hint: String,
}

fn fact(label: &str, value: impl Into<String>, col: Color32, hint: &str) -> Fact {
    Fact { label: label.into(), value: value.into(), col, hint: hint.into() }
}

struct Legend {
    color: Color32,
    label: String,
    value: String,
    value_col: Color32,
}

fn leg(color: Color32, label: &str, value: impl Into<String>) -> Legend {
    Legend { color, label: label.into(), value: value.into(), value_col: TEXT() }
}

/// A legend row with no colour square: nothing on the sheet is drawn in its
/// colour (C7).
fn leg_plain(label: &str, value: impl Into<String>, value_col: Color32) -> Legend {
    Legend { color: Color32::TRANSPARENT, label: label.into(), value: value.into(), value_col }
}

fn leg_dim(label: &str, value: impl Into<String>) -> Legend {
    Legend { color: TRACK(), label: label.into(), value: value.into(), value_col: TEXT_DIM() }
}

/// Everything one sheet shows, filled per kind and drawn by one template.
struct Spec {
    icon: Icon,
    chip: Color32,
    title: String,
    sub: String,
    hero: Option<(String, String, String)>,
    segs: Vec<(f32, Color32)>,
    spark: Vec<f32>,
    legend: Vec<Legend>,
    speed: bool,
    facts: Vec<Fact>,
    /// Busiest apps on the network (a card of their own, over the facts).
    apps: Vec<Legend>,
    /// A refresh button in the header, beside close (tip, action).
    refresh: Option<(&'static str, Action)>,
    /// Busiest processes (CPU sheet).
    procs: Vec<super::procs::Proc>,
    buttons: Vec<(&'static str, u8, Action)>,
}

impl Spec {
    fn new(icon: Icon, chip: Color32, title: impl Into<String>, sub: impl Into<String>) -> Self {
        Spec { icon, chip, title: title.into(), sub: sub.into(), hero: None, segs: vec![], spark: vec![], legend: vec![], speed: false, facts: vec![], apps: vec![], refresh: None, procs: vec![], buttons: vec![] }
    }
}

fn split_num(s: &str) -> (String, String) {
    match s.split_once(' ') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (s.to_string(), String::new()),
    }
}

/// The battery's state in words, as the sheet's header and the page's chip
/// say it.
pub fn battery_state(s: &Snapshot) -> String {
    let b = &s.battery;
    let c = &s.charge;
    if !b.present {
        ts!("health.power_adapter").to_string()
    } else if ours(s) && c.status.state.hot {
        ts!("health.bat.paused_warm").to_string()
    } else if battery_held(s) {
        t!("health.bat.held", n = c.policy.limit).into_owned()
    } else if b.full || (b.plugged && b.percent >= 100) {
        ts!("health.bat.full").into()
    } else if b.charging {
        ts!("health.bat.charging").into()
    } else if b.plugged {
        ts!("health.bat.not_charging_adapter").into()
    } else {
        ts!("health.bat.on_battery").into()
    }
}

/// Our charge limit is holding the battery where it is.
pub fn battery_held(s: &Snapshot) -> bool {
    s.battery.present && ours(s) && !s.charge.status.state.hot && s.charge.status.state.held
}

/// Plugged in, not charging, and our charging helper is in charge.
fn ours(s: &Snapshot) -> bool {
    let (b, c) = (&s.battery, &s.charge);
    b.plugged && !b.charging && c.policy.active() && c.helper_ok == Some(true)
}

fn spec(kind: Sheet, s: &Snapshot) -> Spec {
    match kind {
        Sheet::Storage => {
            let name = if s.volume_name.is_empty() { ts!("health.macintosh_hd").to_string() } else { s.volume_name.clone() };
            let mut sp = Spec::new(Icon::Disk, tint::HEALTH(), name, t!("health.drive", size = human_bytes(s.disk_total)).into_owned());
            let total = s.disk_total.max(1) as f32;
            let used = s.disk_total.saturating_sub(s.disk_free);
            let (n, u) = split_num(&human_bytes(s.disk_free));
            sp.hero = Some((n, u, t!("health.free_of", size = human_bytes(s.disk_total)).into_owned()));
            // three coloured series at most; the rest joins Other (C5)
            let shown = &s.categories[..s.categories.len().min(3)];
            let shown_sum: u64 = shown.iter().map(|c| c.1).sum();
            let other = used.saturating_sub(shown_sum);
            for (i, (label, b)) in shown.iter().enumerate() {
                let c = series::monitor(i);
                sp.segs.push((*b as f32 / total, c));
                sp.legend.push(leg(c, label, human_bytes(*b)));
            }
            sp.segs.push((other as f32 / total, OTHER()));
            sp.legend.push(leg(OTHER(), ts!("health.other"), human_bytes(other)));
            sp.legend.push(leg_dim(ts!("health.free_legend"), human_bytes(s.disk_free)));
            // drive health only when it fails (T1), as on the page's card
            if !s.smart_status.is_empty() && !s.smart_status.eq_ignore_ascii_case("Verified") {
                sp.facts.push(fact(ts!("health.drive_health"), s.smart_status.clone(), RED(), ts!("health.drive_health_hint")));
            }
            match s.trash_bytes {
                Some(b) => {
                    let hint = if s.trash_overfill() { ts!("health.trash_hint").to_string() } else { String::new() };
                    sp.facts.push(fact(ts!("health.trash"), human_bytes(b), if s.trash_overfill() { AMBER() } else { TEXT() }, &hint));
                }
                None => sp.facts.push(fact(ts!("health.trash"), "—", TEXT(), ts!("health.trash_no_access"))),
            }
            sp.refresh = Some((ts!("health.measure_again"), Action::MeasureFolders));
            sp.buttons = vec![(ts!("health.disk_map"), BTN_SECONDARY, Action::Open("lens")), (ts!("health.free_up_space"), BTN_PRIMARY, Action::Open("cleanup"))];
            sp
        }
        Sheet::Memory => {
            let mut sp = Spec::new(Icon::Memory, tint::HEALTH(), ts!("health.memory"), t!("health.memory_total", size = human_bytes(s.mem_total)).into_owned());
            let total = s.mem_total.max(1) as f32;
            let (n, u) = split_num(&human_bytes(s.mem_available));
            sp.hero = Some((n, u, ts!("health.available").into()));
            sp.segs = vec![(s.mem_active as f32 / total, series::monitor(0)), (s.mem_wired as f32 / total, series::monitor(1)), (s.mem_compressed as f32 / total, series::monitor(2))];
            sp.legend = vec![
                leg(series::monitor(0), ts!("health.mem.apps"), human_bytes(s.mem_active)),
                leg(series::monitor(1), ts!("health.mem.wired"), human_bytes(s.mem_wired)),
                leg(series::monitor(2), ts!("health.mem.compressed"), human_bytes(s.mem_compressed)),
                leg_dim(ts!("health.available_legend"), human_bytes(s.mem_available)),
            ];
            let l = s.mem_level();
            let word = match l {
                Level::Good => ts!("health.pressure_word.good"),
                Level::Fair => ts!("health.pressure_word.fair"),
                Level::Critical => ts!("health.pressure_word.critical"),
            };
            sp.facts.push(fact(ts!("health.pressure_fact"), t!("health.pressure_value", pct = s.mem_pressure : {:.0}, word = word).into_owned(), level_color(l), ""));
            sp.facts.push(fact(ts!("health.swap_used"), human_bytes(s.swap_used), TEXT(), ""));
            sp
        }
        Sheet::Battery => {
            let b = &s.battery;
            let c = &s.charge;
            let state = battery_state(s);
            let ours = ours(s);
            let mut sp = Spec::new(Icon::Battery(b.percent), tint::HEALTH(), ts!("health.battery"), state);
            if !b.present {
                sp.facts.push(fact(ts!("health.power"), ts!("health.adapter"), TEXT(), ""));
                return sp;
            }
            sp.hero = Some((b.percent.to_string(), "%".into(), ts!("health.ring_charge").into()));
            sp.segs = vec![(b.percent as f32 / 100.0, if b.percent <= 20 { RED() } else { GREEN() })];
            let hv = b.health_pct.map(|h| format!("{h:.0}%")).unwrap_or_else(|| "—".into());
            let hd = match m::battery_health_state(&b.health_text, b.permanent_failure) {
                m::BatteryHealthState::Good => {
                    ""
                }
                m::BatteryHealthState::Fair => "",
                m::BatteryHealthState::Poor | m::BatteryHealthState::Service => ts!("health.replace_soon"),
                m::BatteryHealthState::Unknown => "",
            };
            sp.facts.push(fact(ts!("health.health"), hv, level_color(s.battery_level()), hd));
            sp.facts.push(fact(ts!("health.charge_cycles"), t!("health.cycles_value", n = b.cycles, total = if b.design_cycles > 0 { b.design_cycles } else { 1000 }).into_owned(), TEXT(), ""));
            match b.temp_c {
                Some(t) => sp.facts.push(fact(ts!("health.temperature"), t!("unit.celsius", c = t : {:.0}).into_owned(), if t < 35.0 { TEXT() } else { AMBER() }, "")),
                None => sp.facts.push(fact(ts!("health.temperature"), "—", TEXT(), "")),
            }
            let time = if ours && (c.status.state.hot || c.status.state.held) {
                None
            } else if b.charging {
                b.minutes_left.map(|m| (t!("health.until_full", time = human_duration(m as u64 * 60)).into_owned(), ""))
            } else if !b.plugged {
                b.minutes_left.map(|m| (t!("health.about_left", time = human_duration(m as u64 * 60)).into_owned(), ""))
            } else if !b.full && b.percent < 100 {
                Some((ts!("health.paused_by_macos").to_string(), ""))
            } else {
                None
            };
            if let Some((v, h)) = time {
                sp.facts.push(fact(if b.plugged && !b.charging { ts!("health.why_not_charging") } else { ts!("health.time") }, v, TEXT(), h));
            }
            // the charge limit and its switches live in Mac Health › Settings
            if b.present {
                sp.buttons.push((ts!("health.charging_settings"), BTN_SECONDARY, Action::Open("health-settings")));
            }
            sp
        }
        Sheet::Cpu => {
            let sub = if s.chip.is_empty() { ts!("health.processor").to_string() } else { s.chip.clone() };
            let mut sp = Spec::new(Icon::Cpu, tint::HEALTH(), ts!("health.cpu"), sub);
            sp.hero = Some((format!("{:.0}", s.cpu_load), "%".into(), ts!("health.load").into()));
            sp.spark = s.cpu_history.clone();
            sp.legend = vec![
                // only the total load is drawn, so the rows carry no squares
                leg_plain(ts!("health.user"), format!("{:.0}%", s.cpu_user), TEXT()),
                leg_plain(ts!("health.system"), format!("{:.0}%", s.cpu_system), TEXT()),
            ];
            if let Some(pw) = &s.power {
                // whole-Mac watts beside user / system, so the cores get a column of their own
                let parts = (pw.cpu_w > 0.0).then_some(pw.cpu_w + pw.gpu_w + pw.ane_w);
                if let Some(total) = s.sys_watts.or(parts) {
                    sp.legend.push(leg_plain(ts!("health.power"), watts(total), TEXT()));
                }
                for (label, u) in [(ts!("health.e_cores"), pw.ecpu), (ts!("health.p_cores"), pw.pcpu), (ts!("health.gpu"), pw.gpu)] {
                    sp.legend.push(leg_plain(label, usage_text(u), TEXT()));
                }
            }
            let tc = match s.thermal {
                0 => GREEN(),
                1 => AMBER(),
                _ => RED(),
            };
            sp.facts.push(fact(ts!("health.thermal_pressure"), [ts!("health.thermal.nominal"), ts!("health.thermal.fair"), ts!("health.thermal.serious"), ts!("health.thermal.critical")][s.thermal.min(3) as usize], tc, ""));
            sp.procs = s.procs.clone();
            sp.facts.push(fact(ts!("health.uptime"), human_duration(s.uptime_secs), if s.uptime_warning() { AMBER() } else { TEXT() }, ""));
            sp
        }
        Sheet::Network => {
            let title = if s.wifi.name.is_empty() { ts!("health.network").to_string() } else { s.wifi.name.clone() };
            let mut sub = if s.wifi.wired { ts!("health.wired").to_string() } else { ts!("health.wifi").to_string() };
            if !s.wifi.phy.is_empty() {
                sub.push_str(&format!(" · {}", s.wifi.phy));
            }
            let mut sp = Spec::new(Icon::Wifi, tint::HEALTH(), title, sub);
            // download first, like the widget and the per-app rows
            let down = human_rate(s.net_down);
            let (n, u) = split_num(&down);
            sp.hero = Some((n, u, t!("health.down_up", up = human_rate(s.net_up)).into_owned()));
            sp.speed = true;
            let dash = |v: &str| if v.is_empty() { "—".to_string() } else { v.to_string() };
            for a in &s.apps {
                sp.apps.push(leg_plain(&a.name, format!("{} ↓  {} ↑", human_rate(a.down), human_rate(a.up)), TEXT_DIM()));
            }
            sp.facts.push(fact(ts!("health.security"), dash(&title_case(&s.wifi.security)), TEXT(), ""));
            sp.facts.push(fact(ts!("health.channel"), dash(&channel_text(&s.wifi.channel)), TEXT(), ""));
            sp.facts.push(fact(ts!("health.signal"), dash(s.wifi.signal.split(" / ").next().unwrap_or("").trim()), TEXT(), ""));
            use super::monitor::SpeedMode;
            sp.buttons = vec![(SpeedMode::Full.label(), BTN_SECONDARY, Action::SpeedTest(SpeedMode::Full)), (SpeedMode::Quick.label(), BTN_PRIMARY, Action::SpeedTest(SpeedMode::Quick))];
            sp
        }
        Sheet::Devices => {
            let devs = super::devices::lowest_first(&s.devices);
            let mut sp = Spec::new(Icon::Bluetooth, tint::NEUTRAL(), ts!("health.devices"), super::devices_ui::note(&devs));
            for d in &devs {
                let mut label = format!("{} · {}", d.name, d.detail());
                if d.charging && d.live() {
                    label.push_str(ts!("health.charging_suffix"));
                }
                match d.battery() {
                    Some(b) if d.live() => sp.legend.push(Legend { color: super::devices::color(b), label, value: format!("{b}%"), value_col: super::devices::text_color(b) }),
                    Some(b) => sp.legend.push(Legend { color: with_alpha(super::devices::color(b), 0.4), label, value: format!("{b}%"), value_col: TEXT_FAINT() }),
                    None => sp.legend.push(Legend { color: TEXT_FAINT(), label: d.name.clone(), value: "–".into(), value_col: TEXT_FAINT() }),
                }
            }
            sp.buttons = vec![(ts!("health.bt_settings"), BTN_SECONDARY, Action::OpenBluetooth)];
            sp
        }
    }
}

/// "WPA3 PERSONAL" -> "WPA3 Personal".
pub(super) fn title_case(s: &str) -> String {
    s.split(' ').map(|w| if w.chars().any(|c| c.is_ascii_digit()) || w.len() <= 3 { w.to_string() } else { let l = w.to_lowercase(); let mut c = l.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
}

/// "149 (5GHz, 80MHz)" -> "149 · 5 GHz · 80 MHz".
pub(super) fn channel_text(s: &str) -> String {
    s.replace(" (", " · ").replace(", ", " · ").replace(')', "").replace("GHz", " GHz").replace("MHz", " MHz").replace("  ", " ")
}

/// "31% · 1.4 GHz" (no clock while idle).
pub(super) fn usage_text(u: crate::kit::ioreport::Usage) -> String {
    let pct = format!("{:.0}%", u.busy * 100.0);
    if u.mhz <= 0.0 || u.busy < 0.005 {
        pct
    } else if u.mhz >= 1000.0 {
        format!("{pct} · {:.1} GHz", u.mhz / 1000.0)
    } else {
        format!("{pct} · {:.0} MHz", u.mhz)
    }
}

/// "3.4 W", "0.12 W".
pub(super) fn watts(w: f32) -> String {
    if w < 1.0 {
        format!("{w:.2} W")
    } else {
        format!("{w:.1} W")
    }
}

/// Draw one sheet into `rect` and report what was clicked.
/// A button whose work is already running.
fn busy(act: Action, s: &Snapshot) -> bool {
    (matches!(act, Action::SpeedTest(_)) && s.speed.running) || (act == Action::MeasureFolders && s.sizing) || (act == Action::InstallHelper && s.charge.installing)
}

fn busy_label(act: Action) -> &'static str {
    match act {
        Action::MeasureFolders => ts!("health.measuring"),
        Action::InstallHelper => ts!("health.installing"),
        _ => ts!("health.testing"),
    }
}

/// Height the details popup needs at `w`: measured with a painter that
/// draws nothing.
pub fn details_height(ui: &Ui, kind: Sheet, s: &Snapshot, w: f32) -> f32 {
    let sp = spec(kind, s);
    let p = ui.painter().with_clip_rect(Rect::NOTHING);
    let body_h = body(&p, ui.ctx(), &sp, kind, s, 16.0, w - 32.0, 0.0);
    let procs = procs_h(sp.procs.len().min(super::procs::TOP_N));
    let btns = if sp.buttons.is_empty() { 0.0 } else { BTN_H + 16.0 };
    SHEET_HEADER_H + 16.0 + body_h + procs + btns + 16.0
}

/// The details popup on the Mac Health page: the sheet's content in `rect`.
pub fn details(ui: &mut Ui, rect: Rect, kind: Sheet, s: &Snapshot) -> Action {
    let sp = spec(kind, s);

    let mut action = Action::None;
    if sheet_header(ui, rect, sp.icon, sp.chip, &sp.title, &sp.sub) {
        action = Action::Close;
    }
    let p = ui.painter().clone();
    // refresh sits beside close, like AI Usage's
    if let Some((tip, act)) = sp.refresh {
        let rb = Rect::from_center_size(Pos2::new(rect.right() - 27.0 - 34.0, rect.top() + 29.0), Vec2::splat(26.0));
        if busy(act, s) {
            spinner(&p, rb.center(), 6.0, ui.input(|i| i.time), TEXT_DIM());
            poll(ui.ctx());
        } else if round_button(ui, rb, Icon::Refresh, tip).clicked() {
            action = act;
        }
    }
    let x0 = rect.left() + 16.0;
    let x1 = rect.right() - 16.0;
    let top = rect.top() + SHEET_HEADER_H + 16.0;
    let h = body(&p, ui.ctx(), &sp, kind, s, x0, x1 - x0, top);
    let pa = procs_card(ui, &sp.procs[..sp.procs.len().min(super::procs::TOP_N)], x0, x1 - x0, top + h + GROUP_GAP);
    if pa != Action::None {
        action = pa;
    }
    let a = buttons(ui, &sp, s, x1, rect.bottom() - 16.0);
    if a != Action::None {
        action = a;
    }
    action
}


/// A row's rect for the shared row painters (`legend_row`, `fact_row`),
/// which inset their text 12 from the edge: in the popup there is no card
/// around the rows (S2, no frame inside a frame), so the text sits on the
/// popup's own text column at `x0` and the values end at `x0 + w`.
fn bare(x0: f32, w: f32, y: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(x0 - 12.0, y), Vec2::new(w + 24.0, h))
}

/// A hairline between two groups of rows, `GROUP_GAP` above the next one.
fn divider(p: &egui::Painter, x0: f32, w: f32, y: f32) -> f32 {
    hairline(p, Pos2::new(x0, y), Pos2::new(x0 + w, y));
    GROUP_GAP
}

/// Space around the hairline between groups.
const GROUP_GAP: f32 = 12.0;

/// Legends this wide get two columns.
const LEGEND_TWO_COLUMNS_MIN: f32 = 400.0;

/// The content under a header: hero number with its bar or sparkline, the
/// legend card, the speed test card and the facts card. Returns the height.
fn body(p: &egui::Painter, ctx: &egui::Context, sp: &Spec, kind: Sheet, s: &Snapshot, x0: f32, w: f32, top: f32) -> f32 {
    let mut y = top;
    // hero
    if let Some((big, unit, cap)) = &sp.hero {
        y += hero_number(p, Pos2::new(x0, y), big, unit, cap) + 10.0;
        if !sp.segs.is_empty() {
            stacked_bar(p, Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 10.0)), &sp.segs);
            y += 10.0 + 14.0;
        } else if sp.spark.len() > 1 {
            let hr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 48.0));
            let n = sp.spark.len();
            // scale to the recent peak (at least 25%) so a calm CPU still shows its shape
            let top_v = (sp.spark.iter().copied().fold(0.0, f32::max) * 1.25).clamp(25.0, 100.0);
            let pts: Vec<Pos2> = sp.spark.iter().enumerate().map(|(i, v)| Pos2::new(hr.left() + hr.width() * i as f32 / (n - 1) as f32, hr.bottom() - 2.0 - (hr.height() - 4.0) * (v / top_v).clamp(0.0, 1.0))).collect();
            // soft fill under the line, then the line
            let mut mesh = egui::epaint::Mesh::default();
            let fill = with_alpha(tint::HEALTH(), 0.14);
            for (i, q) in pts.iter().enumerate() {
                mesh.colored_vertex(*q, fill);
                mesh.colored_vertex(Pos2::new(q.x, hr.bottom()), with_alpha(tint::HEALTH(), 0.0));
                if i > 0 {
                    let k = (i as u32) * 2;
                    mesh.add_triangle(k - 2, k - 1, k);
                    mesh.add_triangle(k - 1, k + 1, k);
                }
            }
            p.add(mesh);
            p.add(egui::epaint::PathShape::line(pts, egui::epaint::PathStroke::new(1.6, tint::HEALTH())));
            hairline(p, hr.left_bottom(), hr.right_bottom());
            y += 48.0 + 14.0;
        } else {
            y += 14.0;
        }
    }

    // legend card
    if !sp.legend.is_empty() {
        // wide enough (the window page): two columns, split by a hairline
        // device names are long: one column, so they're never cut (R10)
        let cols = if kind != Sheet::Devices && w >= LEGEND_TWO_COLUMNS_MIN && sp.legend.len() >= 4 { 2 } else { 1 };
        let rows = sp.legend.len().div_ceil(cols);
        let h = rows as f32 * SHEET_ROW_H;
        // two columns with a gutter, no rule between them
        let gut = 24.0;
        let cw = (w - gut * (cols - 1) as f32) / cols as f32;
        for (i, l) in sp.legend.iter().enumerate() {
            let (c, r) = (i / rows, i % rows);
            let rr = bare(x0 + c as f32 * (cw + gut), cw, y + r as f32 * SHEET_ROW_H, SHEET_ROW_H);
            legend_row(p, rr, l.color, &l.label, &l.value, l.value_col);
        }
        y += h + GROUP_GAP;
    } else if kind == Sheet::Devices {
        y += paragraph(p, Pos2::new(x0, y), ts!("health.no_devices"), font(13.0), TEXT_DIM(), w) + 14.0;
    }

    // speed test card: its title over it (R2), ping on the title line
    if sp.speed {
        let sp_ = &s.speed;
        if y > top {
            y += divider(p, x0, w, y);
        }
        card_title(p, Pos2::new(x0, y), ts!("health.speed_test"));
        if let Some(l) = sp_.latency_ms {
            p.text(Pos2::new(x0 + w, y), Align2::RIGHT_TOP, match sp_.jitter_ms {
                Some(j) => t!("health.ping_jitter", ms = l : {:.0}, jitter = j : {:.0}).into_owned(),
                None => t!("health.ping_ms", ms = l : {:.0}).into_owned(),
            }, ty::label(), TEXT_DIM());
        }
        y += CARD_TITLE_H;
        // the note under the numbers wraps (no "…"); the card grows with it
        let note: Option<(String, Color32)> = if sp_.running {
            None
        } else if let Some(e) = &sp_.error {
            Some((e.clone(), AMBER()))
        } else {
            sp_.down_mbps.map(|d| {
                let good: Vec<&str> = monitor::good_for(d).into_iter().map(monitor::good_for_label).collect();
                let txt = if good.is_empty() { ts!("health.very_slow").to_string() } else { t!("health.good_for", list = good.join(", ").to_lowercase()).into_owned() };
                (txt, TEXT_DIM())
            })
        };
        let extra = note.as_ref().map_or(0.0, |(t, c)| {
            let one = p.layout_no_wrap("Ag".into(), ty::label(), *c).size().y;
            (p.layout(t.clone(), ty::label(), *c, w - 24.0).size().y - one).max(0.0)
        });
        let lr = Rect::from_min_size(Pos2::new(x0 - 12.0, y - 12.0), Vec2::new(w + 24.0, 92.0 + extra));
        for (k, (label, v)) in [(ts!("health.download"), sp_.down_mbps), (ts!("health.upload"), sp_.up_mbps)].iter().enumerate() {
            let x = lr.left() + 12.0 + k as f32 * (w / 2.0);
            p.text(Pos2::new(x, lr.top() + 12.0), Align2::LEFT_TOP, *label, ty::label(), TEXT_DIM());
            let (txt, col) = match v {
                Some(v) => (format!("{v:.0}"), TEXT()),
                None => ("–".to_string(), TEXT_FAINT()),
            };
            let g = p.layout_no_wrap(txt, ty::hero(), col);
            let gw = g.size().x;
            let gh = g.size().y;
            p.galley(Pos2::new(x, lr.top() + 30.0), g, col);
            p.text(Pos2::new(x + gw + 4.0, lr.top() + 30.0 + gh * 0.8), Align2::LEFT_BOTTOM, "Mbps", bold(13.0), TEXT_SOFT());
        }
        let note_y = lr.top() + 70.0;
        if sp_.running {
            bar(p, Rect::from_min_size(Pos2::new(lr.left() + 12.0, note_y + 4.0), Vec2::new(w - 24.0, 4.0)), sp_.progress, tint::HEALTH());
            ctx.request_repaint();
        } else if let Some((txt, col)) = &note {
            text_wrap(p, Pos2::new(lr.left() + 12.0, note_y), Align2::LEFT_TOP, txt, ty::label(), *col, w - 24.0, usize::MAX);
        }
        y += lr.height() - 12.0 + GROUP_GAP;
    }

    // busiest apps, then the facts
    if !sp.apps.is_empty() {
        if y > top {
            y += divider(p, x0, w, y);
        }
        card_title(p, Pos2::new(x0, y), ts!("health.apps"));
        y += CARD_TITLE_H;
        for (i, l) in sp.apps.iter().enumerate() {
            legend_row(p, bare(x0, w, y + i as f32 * SHEET_ROW_H, SHEET_ROW_H), l.color, &l.label, &l.value, l.value_col);
        }
        y += sp.apps.len() as f32 * SHEET_ROW_H + GROUP_GAP;
    }
    if !sp.facts.is_empty() {
        if y > top {
            y += divider(p, x0, w, y);
        }
        y += facts_rows(p, &sp.facts, x0, w, y) + GROUP_GAP;
    }
    // the last group's gap goes: the caller sets what follows
    y - top - GROUP_GAP
}

/// Fact rows on the popup, no card. Returns their height.
fn facts_rows(p: &egui::Painter, facts: &[Fact], x0: f32, w: f32, y: f32) -> f32 {
    let mut fy = y;
    for f in facts {
        fy += fact_row(p, bare(x0, w, fy, 40.0), &f.label, &f.value, f.col, &f.hint);
    }
    fy - y
}

const PROC_ROW_H: f32 = SHEET_ROW_H;
/// How long a first click on Quit waits for the second.
const QUIT_ARM_SECS: f64 = 4.0;

/// Height of the processes card plus the gap above it.
fn procs_h(n: usize) -> f32 {
    if n == 0 { 0.0 } else { 2.0 * GROUP_GAP + CARD_TITLE_H + n as f32 * PROC_ROW_H }
}

/// The busiest processes in one card at `y`; Quit (twice) on the user's own.
fn procs_card(ui: &mut Ui, procs: &[super::procs::Proc], x0: f32, w: f32, y: f32) -> Action {
    if procs.is_empty() {
        return Action::None;
    }
    let p = ui.painter().clone();
    let y = y + divider(&p, x0, w, y);
    card_title(&p, Pos2::new(x0, y), ts!("health.top_processes"));
    let y = y + CARD_TITLE_H;
    let now = ui.input(|i| i.time);
    let arm_id = ui.scope_id().with("proc-quit-armed");
    let armed: Option<(u32, f64)> = ui.data(|d| d.get_temp(arm_id)).filter(|(_, t): &(u32, f64)| now - t < QUIT_ARM_SECS);
    let mut action = Action::None;
    for (k, pr) in procs.iter().enumerate() {
        let rr = bare(x0, w, y + k as f32 * PROC_ROW_H, PROC_ROW_H);
        let hover = ui.rect_contains_pointer(rr);
        let is_armed = armed.is_some_and(|(pid, _)| pid == pr.pid);
        let mut right = rr.right() - 12.0;
        if pr.mine && (hover || is_armed) {
            let label = if is_armed { ts!("health.quit_confirm") } else { ts!("health.quit") };
            let kind = if is_armed { BTN_DANGER } else { BTN_SECONDARY };
            let bw = button_width(&p, label, kind);
            let br = Rect::from_min_size(Pos2::new(right - bw, rr.center().y - 14.0), Vec2::new(bw, 28.0));
            if pill_button(ui, br, label, kind, true).clicked() {
                if is_armed {
                    action = Action::Quit(pr.pid);
                    ui.data_mut(|d| d.remove::<(u32, f64)>(arm_id));
                } else {
                    ui.data_mut(|d| d.insert_temp(arm_id, (pr.pid, now)));
                }
            }
            right = br.left() - 8.0;
        } else {
            let t = p.text(Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, format!("{:.0}%", pr.cpu), font(13.0), TEXT_DIM());
            right = t.left() - 8.0;
        }
        text_fit(&p, Pos2::new(rr.left() + 12.0, rr.center().y), Align2::LEFT_CENTER, &pr.name, font(13.0), TEXT(), right - rr.left() - 12.0);
    }
    if armed.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    action
}

/// A sheet's buttons in a row ending at `right`, sitting on `bottom`.
fn buttons(ui: &mut Ui, sp: &Spec, s: &Snapshot, right: f32, bottom: f32) -> Action {
    let p = ui.painter().clone();
    let mut action = Action::None;
    let mut bx = right;
    let by = bottom - BTN_H;
    for (label, kind_, act) in sp.buttons.iter().rev() {
        let bw = button_width(&p, label, *kind_);
        let br = Rect::from_min_size(Pos2::new(bx - bw, by), Vec2::new(bw, BTN_H));
        let enabled = !busy(*act, s);
        if pill_button(ui, br, if enabled { label } else { busy_label(*act) }, *kind_, enabled).clicked() {
            action = *act;
        }
        bx -= bw + 8.0;
    }
    action
}
