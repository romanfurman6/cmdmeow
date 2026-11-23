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
const BENTO_MIN_W: f32 = 860.0;
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
    let p = ui.painter().clone();
    let settings_id = ui.scope_id().with("health-settings");
    let show_settings = ui.ctx().data(|d| d.get_temp::<bool>(settings_id)).unwrap_or(false);
    // the network card lists apps by traffic: nettop runs while it shows
    if !show_settings {
        super::traffic::want(mon, ui.ctx());
    }
    let s = mon.lock_ok().clone();

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
                // CPU and network are two columns wide: a chart and its lines side by side
                let unit = (g.width() - 2.0 * sp::L) / 4.0;
                let (c0, c1) = (unit * 2.0, unit);
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
            let ring_d = disk_ring_area(rects[1].shrink(sp::L), &s).size().min_elem().min(DISK_RING_D);
            if let Some(a) = cpu_card(ui, &p, rects[0].shrink(sp::L), &s) {
                action = a;
            }
            if let Some(a) = disk_card(ui, &p, rects[1].shrink(sp::L), &s, ring_d) {
                action = a;
            }
            memory_card(&p, rects[2].shrink(sp::L), &s);
            if let Some(a) = battery_card(ui, &p, rects[3].shrink(sp::L), &s) {
                action = a;
            }
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

/// The card title's font. Proto: CMDMEOW_CARD_TITLE=<pt> tries sizes.
fn title_font() -> egui::FontId {
    let pt = std::env::var("CMDMEOW_CARD_TITLE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(15.0);
    bold(pt)
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
    // a long title in a narrow card steps down to 13 rather than cut (R10)
    let mut f = title_font();
    while f.size > 13.0 && p.layout_no_wrap(title.to_owned(), f.clone(), TEXT()).size().x > right - r.left() {
        f.size -= 1.0;
    }
    let t = text_fit(p, Pos2::new(r.left(), r.top() + 26.0), Align2::LEFT_BOTTOM, title, f, TEXT(), right - r.left());
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

fn cpu_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<SheetAction> {
    let load = format!("{:.0}%", s.cpu_load);
    let th = (["Nominal", "Fair", "Serious", "Critical"][s.thermal.min(3) as usize], level_status(s.thermal_level()));
    let y = card_title(p, r, "CPU load", if s.thermal_level() == Level::Good { None } else { Some(th) }, Some(&load));
    // left: the history, user / system / idle under it; right: the cores,
    // GPU and power, then the busiest processes
    let split = r.left() + (r.width() - sp::XL) / 2.0;
    let foot = r.bottom() - 18.0;
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(split, foot - sp::M));
    let peak = s.cpu_history.iter().copied().fold(0.0f32, f32::max);
    tile::spark_span(p, gr, &s.cpu_history, level_color(s.thermal_level()), Some((peak * 1.15).max(25.0)));
    let mut x = r.left();
    // no Idle: it is 100 minus the big number (T2)
    for t in [format!("User {:.0}%", s.cpu_user), format!("System {:.0}%", s.cpu_system)] {
        x = p.text(Pos2::new(x, foot + 9.0), Align2::LEFT_CENTER, t, ty::label(), white(166)).right() + sp::L;
    }
    let rx = split + sp::XL;
    let rw = r.right() - rx;
    let mut rows: Vec<Row> = Vec::new();
    if let Some(pw) = &s.power {
        for (label, u) in [("E-cores", pw.ecpu), ("P-cores", pw.pcpu), ("GPU", pw.gpu)] {
            rows.push((Some((u.busy.clamp(0.01, 1.0), tint::HEALTH())), label.to_string(), super::sheets::usage_text(u)));
        }
        // the whole Mac; without that sensor, what the chip draws, unless its counters stopped
        let parts = (pw.cpu_w > 0.0).then_some(pw.cpu_w + pw.gpu_w + pw.ane_w);
        if let Some(total) = s.sys_watts.or(parts) {
            rows.push((None, "Power".to_string(), super::sheets::watts(total)));
        }
    }
    lines_at(p, rx, rw, y, sp::LEGEND_H, &rows);
    let py = y + rows.len() as f32 * sp::LEGEND_H + if rows.is_empty() { 0.0 } else { sp::L };
    procs_list(ui, p, Rect::from_min_max(Pos2::new(rx, py), Pos2::new(r.right(), r.bottom())), &s.procs)
}

/// How long a first click on Quit waits for the second.
const QUIT_ARM_SECS: f64 = 4.0;

/// The busiest processes as name · bar · % lines under a "Processes"
/// label, as many as fit in `r`. The user's own get Quit on hover (twice).
fn procs_list(ui: &mut Ui, p: &egui::Painter, r: Rect, procs: &[super::procs::Proc]) -> Option<SheetAction> {
    if procs.is_empty() || r.height() < 18.0 + sp::LEGEND_H {
        return None;
    }
    p.text(Pos2::new(r.left(), r.top()), Align2::LEFT_TOP, "Processes", ty::section(), TEXT());
    let top = r.top() + 18.0 + sp::S;
    let n = (((r.bottom() - top) / sp::LEGEND_H).floor() as usize).min(procs.len());
    let shown = &procs[..n];
    let hi = shown.iter().map(|pr| pr.cpu).fold(1.0f32, f32::max);
    let vals: Vec<String> = shown.iter().map(|pr| format!("{:.0}%", pr.cpu)).collect();
    let cols = tile::bar_cols(p, &shown.iter().zip(&vals).map(|(pr, v)| (pr.name.as_str(), v.as_str())).collect::<Vec<_>>());
    let now = ui.input(|i| i.time);
    let arm_id = ui.scope_id().with("health-quit-armed");
    let armed: Option<(u32, f64)> = ui.data(|d| d.get_temp(arm_id)).filter(|(_, t): &(u32, f64)| now - t < QUIT_ARM_SECS);
    let mut out = None;
    for (k, (pr, v)) in shown.iter().zip(&vals).enumerate() {
        let lr = Rect::from_min_size(Pos2::new(r.left(), top + k as f32 * sp::LEGEND_H), Vec2::new(r.width(), sp::LEGEND_H));
        let is_armed = armed.is_some_and(|(pid, _)| pid == pr.pid);
        if pr.mine && (ui.rect_contains_pointer(lr) || is_armed) {
            let label = if is_armed { "Quit?" } else { "Quit" };
            let kind = if is_armed { BTN_DANGER } else { BTN_SECONDARY };
            let bw = button_width(p, label, kind);
            let br = Rect::from_min_size(Pos2::new(lr.right() - bw, lr.top()), Vec2::new(bw, sp::LEGEND_H));
            tile::bar_line(p, Rect::from_min_max(lr.min, Pos2::new(br.left() - sp::S, lr.bottom())), &pr.name, Some(((pr.cpu / hi).clamp(0.01, 1.0), tint::HEALTH())), v, cols);
            if pill_button(ui, br, label, kind, true).clicked() {
                if is_armed {
                    out = Some(SheetAction::Quit(pr.pid));
                    ui.data_mut(|d| d.remove::<(u32, f64)>(arm_id));
                } else {
                    ui.data_mut(|d| d.insert_temp(arm_id, (pr.pid, now)));
                }
            }
        } else {
            tile::bar_line(p, lr, &pr.name, Some(((pr.cpu / hi).clamp(0.01, 1.0), tint::HEALTH())), v, cols);
        }
    }
    if armed.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    out
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
    // no Used: the ring says free of total (T2); drive health only when it isn't fine (T1)
    let mut rows: Vec<Row> = Vec::new();
    if !s.smart_status.is_empty() && !s.smart_status.eq_ignore_ascii_case("Verified") {
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


fn battery_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<SheetAction> {
    let b = &s.battery;
    if !b.present {
        card_title(p, r, "Power", Some(("No battery", TEXT_DIM())), None);
        return None;
    }
    // the charge as the big number, like CPU and memory; the state is the first line
    let y = card_title(p, r, "Battery", None, Some(&format!("{}%", b.percent)));
    let col = if b.percent <= 20 { RED() } else { GREEN() };
    let state = super::sheets::battery_state(s);
    let state = if state.starts_with("Held at") { "Held at limit".to_string() } else { state };
    let _ = col;
    let mut rows: Vec<Row> = vec![(None, "State".to_string(), state)];
    rows.extend(battery_rows(s));
    lines_at(p, r.left(), r.width(), y, sp::LEGEND_H, &rows);
    // charging controls at the bottom: limit, then its switches or the helper
    let c = &s.charge;
    let mut out = None;
    let mut by = r.bottom();
    if c.needs_helper() || c.installing {
        let label = if c.installing { "Installing…" } else { "Install helper" };
        let bw = button_width(p, label, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(r.left(), by - BTN_H_ROW), Vec2::new(bw, BTN_H_ROW));
        if pill_button(ui, br, label, BTN_SECONDARY, !c.installing).clicked() {
            out = Some(SheetAction::InstallHelper);
        }
        by = br.top() - sp::M;
    } else if c.policy.active() {
        let sw = [
            (format!("Charge again below {}%", c.policy.limit.saturating_sub(crate::kit::charge::SAIL_GAP)), c.policy.sailing, SheetAction::ToggleSailing),
            (format!("Pause at {:.0} °C", crate::kit::charge::HEAT_C), c.policy.heat, SheetAction::ToggleHeat),
        ];
        for (k, (label, on, act)) in sw.iter().enumerate().rev() {
            let rr = Rect::from_min_max(Pos2::new(r.left(), by - SWITCH_ROW_H), Pos2::new(r.right(), by));
            let sr = Rect::from_min_size(Pos2::new(rr.right() - 36.0, rr.center().y - 10.0), Vec2::new(36.0, 20.0));
            if switch(ui, sr, ui.scope_id().with(("health-charge-switch", k)), *on, true).clicked() {
                out = Some(*act);
            }
            text_fade(p, Pos2::new(rr.left(), rr.center().y), label, font(12.0), white(166), sr.left() - rr.left() - sp::S);
            by = rr.top();
        }
    }
    // the limit: Off and the three stops
    let lr = Rect::from_min_max(Pos2::new(r.left(), by - 28.0), Pos2::new(r.right(), by));
    let lw = p.text(Pos2::new(lr.left(), lr.center().y), Align2::LEFT_CENTER, "Limit", font(12.0), white(166)).width();
    let sr = Rect::from_min_max(Pos2::new(lr.left() + lw + sp::M, lr.top()), lr.max);
    let labels: Vec<String> = crate::kit::charge::LIMITS.iter().map(|l| if *l >= 100 { "Off".to_string() } else { format!("{l}%") }).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let sel = crate::kit::charge::LIMITS.iter().position(|l| *l == c.policy.limit).unwrap_or(0);
    if let Some(i) = segmented(ui, sr, ui.scope_id().with("health-charge-limit"), &labels, sel) {
        out = Some(SheetAction::SetLimit(crate::kit::charge::LIMITS[i]));
    }
    out
}

/// A switch line in a card.
const SWITCH_ROW_H: f32 = 30.0;

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
    let down = format!("↓ {}", human_rate(s.net_down));
    let y = card_title(p, r, &title, status, Some(&down));
    // left: the history over the speed test buttons; right: the lines, then the apps
    let split = r.left() + (r.width() - sp::XL) / 2.0;
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(split, r.bottom() - BTN_H_ROW - sp::M));
    if gr.height() > 40.0 {
        tile::spark_span(p, gr, &s.net_history, tint::HEALTH(), None);
    }
    let rx = split + sp::XL;
    let rw = r.right() - rx;
    let dash = |v: String| if v.is_empty() { "—".to_string() } else { v };
    let mut rows: Vec<Row> = vec![(None, "Upload".to_string(), format!("↑ {}", human_rate(s.net_up)))];
    rows.push((None, "Today".to_string(), format!("{} in · {} out", human_bytes(s.net_recv_total), human_bytes(s.net_sent_total))));
    if !s.wifi.wired {
        rows.push((None, "Signal".to_string(), dash(s.wifi.signal.split(" / ").next().unwrap_or("").trim().to_string())));
        rows.push((None, "Security".to_string(), dash(super::sheets::title_case(&s.wifi.security))));
        rows.push((None, "Channel".to_string(), dash(super::sheets::channel_text(&s.wifi.channel))));
    }
    // the speed only once there is one: the buttons say a test can run (T3)
    let sp_ = &s.speed;
    if sp_.running {
        rows.push((None, "Speed".to_string(), format!("Testing… {:.0}%", sp_.progress * 100.0)));
    } else if let (Some(d), Some(u)) = (sp_.down_mbps, sp_.up_mbps) {
        rows.push((None, "Speed".to_string(), format!("{d:.0} down · {u:.0} up Mbps")));
    }
    lines_at(p, rx, rw, y, sp::LEGEND_H, &rows);
    // the busiest apps on the network now (nettop runs while this shows)
    let ay = y + rows.len() as f32 * sp::LEGEND_H + sp::L;
    if !s.apps.is_empty() && r.bottom() - ay >= 18.0 + sp::LEGEND_H {
        p.text(Pos2::new(rx, ay), Align2::LEFT_TOP, "Apps", ty::section(), TEXT());
        let top = ay + 18.0 + sp::S;
        let n = (((r.bottom() - top) / sp::LEGEND_H).floor() as usize).min(s.apps.len());
        let hi = s.apps[..n].iter().map(|a| a.down + a.up).fold(1.0f64, f64::max);
        let app_rows: Vec<Row> = s.apps[..n].iter().map(|a| (Some(((((a.down + a.up) / hi) as f32).clamp(0.01, 1.0), tint::HEALTH())), a.name.clone(), format!("↓ {}", human_rate(a.down)))).collect();
        lines_at(p, rx, rw, top, sp::LEGEND_H, &app_rows);
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
