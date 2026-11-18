//! The Mac Health page in the full window (D6): a bento of six cards that
//! fits the window without scrolling. CPU (wide, with its history), memory
//! and the startup disk on top; network, battery and Bluetooth devices
//! below. Settings (device alerts, menu bar, hidden devices) sit behind
//! the Overview / Settings switch in the header.
//!
//! Each card shows real readings only: no chart is drawn for data the
//! monitor does not keep a history of.

use crate::kit::aicheck::{self, ui::Event};
use crate::kit::sync::LockExt;
use super::monitor::{self, Level, Snapshot};
use super::sheets::Action as SheetAction;
use crate::core::tool::{Cx, Recommendation};
use crate::kit::glass::*;
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2};

/// Below this body width the cards stack in one scrolling column.
const BENTO_MIN_W: f32 = 760.0;
/// Below this body height the bento scrolls.
const BENTO_MIN_H: f32 = 520.0;
/// Past this the cards stop growing and the grid stays at the top: charts
/// and rings don't stretch to fill a tall window.
const BENTO_MAX_H: f32 = 2.0 * 340.0 + sp::L;

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Good => tint::HEALTH(),
        Level::Fair => AMBER(),
        Level::Critical => RED(),
    }
}

pub fn page(mon: &monitor::Shared, ai: &mut aicheck::Check, recs: &[Recommendation], ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let s = mon.lock_ok().clone();
    let p = ui.painter().clone();
    let settings_id = ui.scope_id().with("health-settings");
    let show_settings = ui.ctx().data(|d| d.get_temp::<bool>(settings_id)).unwrap_or(false);

    // header: model, chip, uptime; the top recommendation and Overview / Settings on the right
    let mut sub: Vec<String> = Vec::new();
    if !s.model.is_empty() {
        sub.push(s.model.clone());
    }
    if !s.chip.is_empty() {
        sub.push(s.chip.clone());
    }
    if s.uptime_secs > 0 {
        sub.push(format!("up {}", human_duration(s.uptime_secs)));
    }
    let (sub, sub_col) = match recs.first() {
        Some(rec) => (rec.title.clone(), AMBER()),
        None => { let _ = sub; (String::new(), TEXT_DIM()) }
    };
    // AI Check: a chip after the title, on the overview
    let cw = if show_settings { 0.0 } else { aicheck::ui::chip_width(ai, ui, s.ready) };
    let rec_w = recs.first().map(|rec| button_width(&p, &rec.action, BTN_PRIMARY) + sp::M).unwrap_or(0.0);
    let sub_right = r.right() - sp::PAGE_EDGE - 180.0 - sp::M - rec_w - sp::M;
    let (btns, chip_at) = page_header_room(&p, r, "Mac Health", &sub, sub_col, cw, sub_right);
    let mut right = btns.right();
    let tr = Rect::from_min_max(Pos2::new(right - 180.0, btns.top()), Pos2::new(right, btns.bottom()));
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("health-tab"), &["Overview", "Settings"], show_settings as usize) {
        ui.ctx().data_mut(|d| d.insert_temp(settings_id, i == 1));
    }
    right = tr.left() - sp::M;
    if let Some(rec) = recs.first() {
        let bw = button_width(&p, &rec.action, BTN_PRIMARY);
        let br = Rect::from_min_max(Pos2::new(right - bw, btns.top()), Pos2::new(right, btns.bottom()));
        if pill_button(ui, br, &rec.action, BTN_PRIMARY, true).on_hover_text(&rec.body).clicked() {
            cx.push(rec.go.clone());
        }
    }

    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    let inner = body.shrink2(Vec2::new(sp::PAGE_EDGE, 0.0));

    if !show_settings {
        match aicheck::ui::chip(ai, ui, chip_at, s.ready) {
            Some(Event::Start) => ai.start(ui.ctx(), health_ask(&s)),
            Some(Event::Act(rec)) => {
                match rec.action.as_str() {
                    "quit" => {
                        // only the same process, still running, and the user's own
                        if let Some(pid) = quit_target(&mon.lock_ok().procs, &rec.key, &rec.name) {
                            super::procs::quit(pid);
                        }
                    }
                    _ => {
                        let _ = std::process::Command::new("open").args(["-a", "Activity Monitor"]).spawn();
                    }
                }
                ai.done(&rec);
            }
            // the page doesn't list processes: nothing to show (no locate)
            Some(Event::Show(_)) | None => {}
        }
    }
    let mut action = SheetAction::None;

    let mut dev_action = None;
    if show_settings {
        let w = inner.width().min(640.0);
        let h = super::devices_ui::settings_height(&s);
        let mut child = ui.new_child(UiBuilder::new().max_rect(body).id_salt("health-settings-page"));
        ScrollArea::vertical().id_salt("health-set-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + 2.0 * sp::L), Sense::hover());
            let sr = Rect::from_min_size(Pos2::new(area.left() + sp::PAGE_EDGE, area.top() + sp::L), Vec2::new(w, h));
            let mut sui = ui.new_child(UiBuilder::new().max_rect(sr).id_salt("health-set-sec"));
            dev_action = super::devices_ui::settings(&mut sui, sr, &s);
        });
    } else {
        // 3 columns (1.4 : 1 : 1) × 2 rows
        let fits = inner.width() >= BENTO_MIN_W;
        let total_h = if fits { (body.height() - 2.0 * sp::L).clamp(BENTO_MIN_H, BENTO_MAX_H) } else { 6.0 * 250.0 + 5.0 * sp::L };
        let mut child = ui.new_child(UiBuilder::new().max_rect(body).id_salt("health-page"));
        ScrollArea::vertical().id_salt("health-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total_h + 2.0 * sp::L), Sense::hover());
            let g = Rect::from_min_size(Pos2::new(inner.left(), area.top() + sp::L), Vec2::new(inner.width(), total_h));
            let rects: [Rect; 6] = if fits {
                let unit = (g.width() - 2.0 * sp::L) / 3.4;
                let (c0, c1) = (unit * 1.4, unit);
                let rh = (g.height() - sp::L) / 2.0;
                let x = [g.left(), g.left() + c0 + sp::L, g.left() + c0 + c1 + 2.0 * sp::L];
                let y = [g.top(), g.top() + rh + sp::L];
                // CPU, memory, disk / network, battery, devices
                [
                    Rect::from_min_size(Pos2::new(x[0], y[0]), Vec2::new(c0, rh)),
                    Rect::from_min_size(Pos2::new(x[2], y[0]), Vec2::new(c1, rh)),
                    Rect::from_min_size(Pos2::new(x[1], y[0]), Vec2::new(c1, rh)),
                    Rect::from_min_size(Pos2::new(x[1], y[1]), Vec2::new(c1, rh)),
                    Rect::from_min_size(Pos2::new(x[0], y[1]), Vec2::new(c0, rh)),
                    Rect::from_min_size(Pos2::new(x[2], y[1]), Vec2::new(c1, rh)),
                ]
            } else {
                std::array::from_fn(|i| Rect::from_min_size(Pos2::new(g.left(), g.top() + i as f32 * (250.0 + sp::L)), Vec2::new(g.width(), 250.0)))
            };
            let p = ui.painter().clone();
            for rr in rects {
                data_card(&p, rr);
            }
            // one ring size on the page: the disk and the battery ring match
            let ring_d = disk_ring_area(rects[1].shrink(sp::L), &s).size().min_elem().min(battery_ring_area(rects[3].shrink(sp::L), &s).size().min_elem()).min(DISK_RING_D);
            cpu_card(&p, rects[0].shrink(sp::L), &s);
            if let Some(a) = disk_card(ui, &p, rects[1].shrink(sp::L), &s, ring_d) {
                action = a;
            }
            memory_card(&p, rects[2].shrink(sp::L), &s);
            battery_card(&p, rects[3].shrink(sp::L), &s, ring_d);
            if let Some(a) = network_card(ui, &p, rects[4].shrink(sp::L), &s) {
                action = a;
            }
            dev_action = devices_card(ui, &p, rects[5].shrink(sp::L), &s);
        });
    }
    match action {
        SheetAction::Open(tool) => cx.open_page(tool, ""),
        SheetAction::SpeedTest(mode) => {
            // a test already under way just shows again
            monitor::start_speed_test(mon.clone(), ui.ctx().clone(), mode);
            super::speed::open(ui.ctx());
        }
        SheetAction::MeasureFolders => monitor::measure_folders(mon.clone(), ui.ctx().clone()),
        SheetAction::ToggleAlerts => monitor::change_bt_prefs(mon, |p| crate::kit::notify::flip(&mut p.alerts)),
        SheetAction::ToggleMenuBar => monitor::change_bt_prefs(mon, |p| p.menu_bar = !p.menu_bar),
        SheetAction::OpenBluetooth => super::devices::open_bluetooth_settings(),
        SheetAction::SetLimit(l) => monitor::change_charge(mon, ui.ctx(), |p| p.limit = l),
        SheetAction::ToggleSailing => monitor::change_charge(mon, ui.ctx(), |p| p.sailing = !p.sailing),
        SheetAction::ToggleHeat => monitor::change_charge(mon, ui.ctx(), |p| p.heat = !p.heat),
        SheetAction::InstallHelper => monitor::install_charge_helper(mon, ui.ctx()),
        SheetAction::Quit(pid) => {
            super::procs::quit(pid);
        }
        SheetAction::None | SheetAction::Close => {}
    }
    if let Some(a) = dev_action {
        use super::devices_ui::PageAction as P;
        match a {
            P::ToggleAlerts => monitor::change_bt_prefs(mon, |p| crate::kit::notify::flip(&mut p.alerts)),
            P::ToggleCharged => monitor::change_bt_prefs(mon, |p| crate::kit::notify::flip(&mut p.charged)),
            P::ChargedAt(v) => monitor::change_bt_prefs(mon, |p| p.charged_at = v),
            P::ToggleMenuBar => monitor::change_bt_prefs(mon, |p| p.menu_bar = !p.menu_bar),
            P::OpenBluetooth => super::devices::open_bluetooth_settings(),
            P::AlertBelow(key, v) => monitor::change_bt_prefs(mon, |p| p.set_device(&key, |d| d.alert_below = Some(v))),
            P::Hide(key, h) => monitor::change_bt_prefs(mon, |p| p.set_device(&key, |d| d.hidden = h)),
            P::Forget(key) => monitor::forget_device(mon, &key),
        }
    }
    match super::speed::modal(ui, &s.speed) {
        Some(super::speed::Ask::Run(mode)) => monitor::start_speed_test(mon.clone(), ui.ctx().clone(), mode),
        Some(super::speed::Ask::Stop) => monitor::stop_speed_test(mon, ui.ctx()),
        None => {}
    }
}

/// Card title (13/600), an optional status chip after it, and an optional
/// big value on the right. Returns the y where the content starts.
fn card_title(p: &egui::Painter, r: Rect, title: &str, status: Option<(&str, Color32)>, big: Option<&str>) -> f32 {
    let mut right = r.right();
    // the title names the big number ("CPU load"), the number stays plain ("16%")
    if let Some(num) = big {
        let n = p.text(Pos2::new(r.right(), r.top() + 28.0), Align2::RIGHT_BOTTOM, num, ty::hero(), TEXT());
        right = n.left() - sp::M;
    }
    // the title sits on the big number's baseline in every card, so titles
    // in a row line up too
    let t = text_fit(p, Pos2::new(r.left(), r.top() + 26.0), Align2::LEFT_BOTTOM, title, ty::title(), TEXT(), right - r.left());
    if let Some((label, c)) = status {
        if t.right() + 80.0 < right {
            chip(p, Pos2::new(t.right() + sp::S, t.center().y), Align2::LEFT_CENTER, label, (with_alpha(c, 0.16), c));
        }
    }
    r.top() + 28.0 + sp::M
}

/// The page's stat ring (SP2 `stat_ring`) at `d`, centred in `area`.
fn ring_in(p: &egui::Painter, area: Rect, d: f32, frac: f32, color: Color32, value: &str, caption: &str) {
    if d < 48.0 {
        return;
    }
    stat_ring(p, area.center(), d, frac, color, value, caption);
}

fn level_status(l: Level) -> Color32 {
    match l {
        Level::Good => GREEN(),
        Level::Fair => AMBER(),
        Level::Critical => RED(),
    }
}

fn cpu_card(p: &egui::Painter, r: Rect, s: &Snapshot) {
    let load = format!("{:.0}%", s.cpu_load);
    let th = (["Nominal", "Fair", "Serious", "Critical"][s.thermal.min(3) as usize], level_status(s.thermal_level()));
    let y = card_title(p, r, "CPU load", if s.thermal_level() == Level::Good { None } else { Some(th) }, Some(&load));
    let foot = r.bottom() - 18.0;
    // the history with its time span under it, as in the widgets (W5)
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), foot - sp::M));
    let peak = s.cpu_history.iter().copied().fold(0.0f32, f32::max);
    tile::spark_span(p, gr, &s.cpu_history, level_color(s.thermal_level()), Some((peak * 1.15).max(25.0)));
    let mut x = r.left();
    for t in [format!("User {:.0}%", s.cpu_user), format!("System {:.0}%", s.cpu_system), format!("Idle {:.0}%", (100.0 - s.cpu_load).max(0.0))] {
        x = p.text(Pos2::new(x, foot + 9.0), Align2::LEFT_CENTER, t, ty::label(), white(166)).right() + sp::L;
    }
}

type Row = (Option<(f32, Color32)>, String, String);

/// Name · bar · value lines at the bottom of `r` (W5 columns shared in the
/// card), 22 pt each. Returns the top of the first line.
fn lines(p: &egui::Painter, r: Rect, rows: &[Row]) -> f32 {
    let top = r.bottom() - rows.len() as f32 * sp::LEGEND_H;
    lines_at(p, r.left(), r.width(), top, sp::LEGEND_H, rows);
    top
}

/// The same lines from `top` down, `lh` apart.
fn lines_at(p: &egui::Painter, left: f32, w: f32, top: f32, lh: f32, rows: &[Row]) {
    let cols = tile::bar_cols(p, &rows.iter().map(|l| (l.1.as_str(), l.2.as_str())).collect::<Vec<_>>());
    for (k, (frac, label, value)) in rows.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(left, top + k as f32 * lh), Vec2::new(w, lh));
        tile::bar_line(p, lr, label, *frac, value, cols);
    }
}

fn disk_rows(s: &Snapshot) -> Vec<Row> {
    let mut rows: Vec<Row> = vec![(None, "Used".to_string(), human_bytes(s.disk_total.saturating_sub(s.disk_free)))];
    if !s.smart_status.is_empty() {
        rows.push((None, "Drive health".to_string(), s.smart_status.clone()));
    }
    rows.push((None, "Trash".to_string(), s.trash_bytes.map(human_bytes).unwrap_or_else(|| "—".into())));
    rows
}

/// Where the disk ring may sit: under the title, over the lines and the button.
fn disk_ring_area(r: Rect, s: &Snapshot) -> Rect {
    let lt = r.bottom() - BTN_H_ROW - sp::M - disk_rows(s).len() as f32 * sp::LEGEND_H;
    Rect::from_min_max(Pos2::new(r.left(), r.top() + 28.0 + sp::M), Pos2::new(r.right(), lt - sp::M))
}

fn disk_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot, ring_d: f32) -> Option<SheetAction> {
    let name = if s.volume_name.is_empty() { "Macintosh HD".to_string() } else { s.volume_name.clone() };
    card_title(p, r, &name, None, None);
    let used = if s.disk_total > 0 { 1.0 - s.disk_free as f32 / s.disk_total as f32 } else { 0.0 };
    let bottom = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - BTN_H_ROW - sp::M));
    lines(p, bottom, &disk_rows(s));
    let col = if s.disk_level() == Level::Good { tint::HEALTH() } else { level_color(s.disk_level()) };
    // SP2: the free space whole, "free of <total>" under it, as on the start ring
    ring_in(p, disk_ring_area(r, s), ring_d, used, col, &human_bytes(s.disk_free), &format!("free of {}", human_bytes(s.disk_total)));
    // footer: the way to free space
    let fy = r.bottom() - BTN_H_ROW;
    let mut out = None;
    let bw = button_width(p, "Free up space", BTN_SECONDARY);
    let br = Rect::from_min_size(Pos2::new(r.left(), fy), Vec2::new(bw, BTN_H_ROW));
    if pill_button(ui, br, "Free up space", BTN_SECONDARY, true).clicked() {
        out = Some(SheetAction::Open("cleanup"));
    }
    out
}

fn memory_card(p: &egui::Painter, r: Rect, s: &Snapshot) {
    let word = match s.mem_level() {
        Level::Good => "Normal",
        Level::Fair => "Elevated",
        Level::Critical => "High",
    };
    let pct = format!("{:.0}%", s.mem_pressure);
    let y = card_title(p, r, "Memory pressure", if s.mem_level() == Level::Good { None } else { Some((word, level_status(s.mem_level()))) }, Some(&pct));
    // what holds the memory as name · bar · value lines like the widgets
    // (W5), bars against the largest so they read as a comparison
    let hi = s.mem_active.max(s.mem_wired).max(s.mem_compressed).max(1) as f32;
    let bar_of = |b: u64| Some(((b as f32 / hi).clamp(0.01, 1.0), tint::HEALTH()));
    let rows: Vec<Row> = vec![
        (bar_of(s.mem_active), "Apps".to_string(), human_bytes(s.mem_active)),
        (bar_of(s.mem_wired), "Wired".to_string(), human_bytes(s.mem_wired)),
        (bar_of(s.mem_compressed), "Compressed".to_string(), human_bytes(s.mem_compressed)),
        (None, "Swap used".to_string(), human_bytes(s.swap_used)),
        (None, "Installed".to_string(), human_bytes(s.mem_total)),
    ];
    // no pressure graph: it barely moves, so it says nothing (as the
    // Memory widget); the lines take the card from the top, sheet rows 32
    let lh = ((r.bottom() - y) / rows.len() as f32).min(SHEET_ROW_H);
    lines_at(p, r.left(), r.width(), y, lh, &rows);
}

fn battery_rows(s: &Snapshot) -> Vec<Row> {
    let b = &s.battery;
    let mut rows = Vec::new();
    if !b.plugged {
        if let Some(min) = b.minutes_left {
            rows.push((None, "Time left".to_string(), human_duration(min as u64 * 60)));
        }
    }
    if let Some(h) = b.health_pct {
        rows.push((None, "Health".to_string(), format!("{h:.0}%")));
    } else if !b.health_text.is_empty() {
        rows.push((None, "Health".to_string(), b.health_text.clone()));
    }
    if b.cycles > 0 {
        let of = if b.design_cycles > 0 { format!(" of {}", b.design_cycles) } else { String::new() };
        rows.push((None, "Cycles".to_string(), format!("{}{of}", b.cycles)));
    }
    if let Some(t) = b.temp_c {
        rows.push((None, "Temperature".to_string(), format!("{t:.0} °C")));
    }
    rows
}

/// Where the battery ring may sit: under the title, over the lines.
fn battery_ring_area(r: Rect, s: &Snapshot) -> Rect {
    if !s.battery.present {
        return Rect::from_min_size(r.min, Vec2::splat(f32::INFINITY));
    }
    let lt = r.bottom() - battery_rows(s).len() as f32 * sp::LEGEND_H;
    Rect::from_min_max(Pos2::new(r.left(), r.top() + 28.0 + sp::M), Pos2::new(r.right(), lt - sp::M))
}

fn battery_card(p: &egui::Painter, r: Rect, s: &Snapshot, ring_d: f32) {
    let b = &s.battery;
    if !b.present {
        card_title(p, r, "Power", Some(("No battery", TEXT_DIM())), None);
        return;
    }
    // the same words as the battery sheet's header (C6)
    let state = super::sheets::battery_state(s);
    let scol = if b.charging { GREEN() } else if !b.plugged && b.percent <= 20 { RED() } else { TEXT_DIM() };
    card_title(p, r, "Battery", Some((&state, scol)), None);
    let col = if b.percent <= 20 { RED() } else { GREEN() };
    lines(p, r, &battery_rows(s));
    ring_in(p, battery_ring_area(r, s), ring_d, b.percent as f32 / 100.0, col, &format!("{}%", b.percent), "charge");
}

fn devices_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<super::devices_ui::PageAction> {
    let low = s.devices.iter().filter(|d| super::devices::is_low(d)).count();
    let status = if low > 0 { Some((super::devices_ui::note(&s.devices), AMBER())) } else { None };
    let y = card_title(p, r, "Devices", status.as_ref().map(|(t, c)| (t.as_str(), *c)), None);
    super::devices_ui::card_body(ui, p, Rect::from_min_max(Pos2::new(r.left(), y), r.max), s)
}

fn network_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<SheetAction> {
    // the network's own name when macOS gives it (not just "Wi-Fi")
    let named = !s.wifi.name.is_empty() && !s.wifi.name.eq_ignore_ascii_case("wi-fi") && !s.wifi.name.eq_ignore_ascii_case("ethernet");
    let title = if named { s.wifi.name.clone() } else if s.wifi.wired { "Ethernet".to_string() } else if s.wifi.name.is_empty() { "Network".to_string() } else { "Wi-Fi".to_string() };
    let status = if !s.online { Some(("Offline", RED())) } else { None };
    // the download as the big number, like the Wi-Fi widget; upload in the lines
    let down = human_rate(s.net_down);
    let y = card_title(p, r, &title, status, Some(&down));
    let mut rows: Vec<Row> = vec![(None, "Upload".to_string(), human_rate(s.net_up))];
    rows.push((None, "Today".to_string(), format!("{} in · {} out", human_bytes(s.net_recv_total), human_bytes(s.net_sent_total))));
    if !s.wifi.signal.is_empty() {
        rows.push((None, "Signal".to_string(), s.wifi.signal.clone()));
    }
    let sp_ = &s.speed;
    let speed = if sp_.running {
        format!("Testing… {:.0}%", sp_.progress * 100.0)
    } else if let (Some(d), Some(u)) = (sp_.down_mbps, sp_.up_mbps) {
        format!("{d:.0} down · {u:.0} up Mbps")
    } else {
        "Not tested".to_string()
    };
    rows.push((None, "Speed".to_string(), speed));
    let bottom = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - BTN_H_ROW - sp::M));
    let lt = lines(p, bottom, &rows);
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), lt - sp::M));
    if gr.height() > 40.0 {
        tile::spark_span(p, gr, &s.net_history, tint::HEALTH(), None);
    }
    let mut out = None;
    // the two tests side by side; each opens the dial in its mode
    let mut x = r.left();
    for mode in [super::monitor::SpeedMode::Quick, super::monitor::SpeedMode::Full] {
        let label = mode.label();
        let bw = button_width(p, label, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(x, r.bottom() - BTN_H_ROW), Vec2::new(bw, BTN_H_ROW));
        let tip = if mode == super::monitor::SpeedMode::Quick { "About 6 seconds" } else { "About 20 seconds, more streams" };
        if pill_button(ui, br, label, BTN_SECONDARY, !s.speed.running).on_hover_text(tip).clicked() {
            out = Some(SheetAction::SpeedTest(mode));
        }
        x = br.right() + sp::S;
    }
    out
}

/// The pid an AI Check "Quit" may signal: the rec's process must still be in
/// the live list under the same name and run as the user (a pid can be
/// reused by another process while the rec sits on screen).
fn quit_target(live: &[super::procs::Proc], key: &str, name: &str) -> Option<u32> {
    let pid = key.strip_prefix("pid:")?.parse::<u32>().ok()?;
    live.iter().find(|p| p.pid == pid && p.name == name && p.mine).map(|p| p.pid)
}

/// What AI Check gets: the busiest processes and how the Mac is doing.
fn health_ask(s: &monitor::Snapshot) -> aicheck::Ask {
    let items = s
        .procs
        .iter()
        .map(|p| aicheck::Item {
            key: format!("pid:{}", p.pid),
            line: format!("{} (pid {}) · {:.0}% CPU{}", p.name, p.pid, p.cpu, if p.mine { "" } else { " · system process, can't be quit" }),
            icon: None,
            name: p.name.clone(),
        })
        .collect();
    let gb = |b: u64| human_bytes(b);
    let mut facts = vec![
        format!("Mac: {} {}", s.model, s.chip),
        format!("Up {}", human_duration(s.uptime_secs)),
        // the monitor keeps these as percentages already (0–100)
        format!("CPU load {:.0}% (user {:.0}%, system {:.0}%)", s.cpu_load, s.cpu_user, s.cpu_system),
        format!("Memory: pressure {:.0}%, {} total, {} compressed, swap {} of {}", s.mem_pressure, gb(s.mem_total), gb(s.mem_compressed), gb(s.swap_used), gb(s.swap_total)),
        format!("Thermal state: {}", ["nominal", "fair", "serious", "critical"][s.thermal.min(3) as usize]),
        format!("Disk: {} free of {}", gb(s.disk_free), gb(s.disk_total)),
    ];
    if s.battery.present {
        facts.push(format!("Battery {}%{}, {} cycles", s.battery.percent, if s.battery.charging { " charging" } else { "" }, s.battery.cycles));
    }
    aicheck::Ask {
        task: "This is Mac Health: how this Mac is doing right now and its busiest processes. Point out what is slowing it down, heating it or draining the battery, and what to do about it. Only suggest quitting a process the user runs.".into(),
        actions: vec![
            ("quit", "quit that process (like Activity Monitor's Quit)", "Quit"),
            ("activity", "open Activity Monitor to look at it", "Open Activity Monitor"),
        ],
        items,
        facts,
        answer: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::system::procs::Proc;

    #[test]
    fn ai_check_gets_percentages_as_shown() {
        let s = monitor::demo_snapshot();
        let facts = health_ask(&s).facts.join("\n");
        assert!(facts.contains(&format!("CPU load {:.0}%", s.cpu_load)), "{facts}");
        assert!(facts.contains(&format!("pressure {:.0}%", s.mem_pressure)), "{facts}");
    }

    #[test]
    fn ai_quit_only_the_same_live_process_of_the_user() {
        let live = vec![Proc { pid: 10, name: "Chrome".into(), cpu: 60.0, mine: true }, Proc { pid: 11, name: "WindowServer".into(), cpu: 30.0, mine: false }];
        assert_eq!(quit_target(&live, "pid:10", "Chrome"), Some(10));
        // pid reused by another process
        assert_eq!(quit_target(&live, "pid:10", "Docker"), None);
        // exited
        assert_eq!(quit_target(&live, "pid:12", "Docker"), None);
        // not the user's
        assert_eq!(quit_target(&live, "pid:11", "WindowServer"), None);
        assert_eq!(quit_target(&live, "10", "Chrome"), None);
    }
}
