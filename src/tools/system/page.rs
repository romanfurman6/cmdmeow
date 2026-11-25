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
use super::sheets::{Action as SheetAction, Sheet};
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

/// `route` is a card's widget id from a panel widget or a tip ("cpu"):
/// the page opens on the overview with that card's details.
pub fn page(mon: &monitor::Shared, ai: &mut aicheck::Check, recs: &[Recommendation], route: Option<String>, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let s = mon.lock_ok().clone();
    let p = ui.painter().clone();
    let settings_id = ui.scope_id().with("health-settings");
    if let Some(route) = route {
        ui.ctx().data_mut(|d| d.insert_temp(settings_id, false));
        open_details(ui.ctx(), Sheet::from_id(&route));
    }
    if ui.ctx().data(|d| d.get_temp::<Sheet>(details_id())) == Some(Sheet::Network) {
        super::traffic::want(mon, ui.ctx());
    }
    // screenshots: CMDMEOW_HEALTH_TAB=settings opens the Settings tab
    let show_settings = ui.ctx().data(|d| d.get_temp::<bool>(settings_id)).unwrap_or(std::env::var("CMDMEOW_HEALTH_TAB").is_ok_and(|v| v == "settings"));

    // header: model, chip, uptime; the top recommendation and Overview / Settings on the right
    let (sub, sub_col) = match recs.first() {
        Some(rec) => (rec.title.clone(), AMBER()),
        None => (String::new(), TEXT_DIM()),
    };
    // AI Check: a chip left of the tabs (SP17), on the overview
    let cw = if show_settings { 0.0 } else { aicheck::ui::chip_width(ai, ui, s.ready) };
    // a tip without a fix has no button
    let rec_btn = recs.first().filter(|rec| !rec.action.is_empty());
    let rec_w = rec_btn.map(|rec| button_width(&p, &rec.action, BTN_PRIMARY)).unwrap_or(0.0);
    // SP16: the fix at the right edge, the tabs next to it
    let tabs = [ts!("health.tab.overview"), ts!("health.tab.settings")];
    let tw = crate::kit::glass::tabs_width(&p, &tabs);
    let sub_right = r.right() - sp::PAGE_EDGE - tw - sp::M - if rec_w > 0.0 { rec_w + sp::M } else { 0.0 } - if cw > 0.0 { cw + sp::M } else { 0.0 };
    let (btns, _) = page_header_room(&p, r, ts!("health.name"), &sub, sub_col, 0.0, sub_right);
    let (tr, br) = header_controls(btns, tw, rec_w);
    let chip_at = Pos2::new(tr.left() - sp::M - cw, tr.center().y);
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("health-tab"), &[ts!("health.tab.overview"), ts!("health.tab.settings")], show_settings as usize) {
        ui.ctx().data_mut(|d| d.insert_temp(settings_id, i == 1));
    }
    if let Some(rec) = rec_btn {
        if pill_button(ui, br, &rec.action, BTN_PRIMARY, true).clicked() {
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
        let bh = battery_settings_height(&s);
        let h = bh + super::devices_ui::settings_height(&s);
        let mut child = ui.new_child(UiBuilder::new().max_rect(body).id_salt("health-settings-page"));
        ScrollArea::vertical().id_salt("health-set-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + 2.0 * sp::L), Sense::hover());
            let br = Rect::from_min_size(Pos2::new(area.left() + sp::PAGE_EDGE, area.top() + sp::L), Vec2::new(w, bh));
            if let Some(a) = battery_settings(ui, br, &s) {
                action = a;
            }
            let sr = Rect::from_min_size(Pos2::new(br.left(), br.bottom()), Vec2::new(w, h - bh));
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
            // each card opens its details; the card's own buttons sit on top
            const KINDS: [Sheet; 6] = [Sheet::Cpu, Sheet::Storage, Sheet::Memory, Sheet::Battery, Sheet::Network, Sheet::Devices];
            for (rr, kind) in rects.iter().zip(KINDS) {
                let resp = ui.interact(*rr, ui.scope_id().with(("health-card", kind as u8)), Sense::click());
                data_card(&p, *rr);
                if resp.hovered() {
                    p.rect_filled(*rr, tile::R_TILE, white(6));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    open_details(ui.ctx(), Some(kind));
                }
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
            P::OpenBluetooth => super::devices::open_bluetooth_settings(),
            P::AlertBelow(key, v) => monitor::change_bt_prefs(mon, |p| p.set_device(&key, |d| d.alert_below = Some(v))),
            P::Hide(key, h) => monitor::change_bt_prefs(mon, |p| p.set_device(&key, |d| d.hidden = h)),
            P::Forget(key) => monitor::forget_device(mon, &key),
        }
    }
    if let Some(a) = details_popup(ui, &s) {
        match a {
            SheetAction::Close => open_details(ui.ctx(), None),
            SheetAction::Open("health-settings") => {
                open_details(ui.ctx(), None);
                ui.ctx().data_mut(|d| d.insert_temp(settings_id, true));
            }
            SheetAction::Open(tool) => {
                open_details(ui.ctx(), None);
                cx.open_page(tool, "");
            }
            SheetAction::SpeedTest(mode) => {
                open_details(ui.ctx(), None);
                monitor::start_speed_test(mon.clone(), ui.ctx().clone(), mode);
                super::speed::open(ui.ctx());
            }
            SheetAction::MeasureFolders => monitor::measure_folders(mon.clone(), ui.ctx().clone()),
            SheetAction::OpenBluetooth => super::devices::open_bluetooth_settings(),
            SheetAction::Quit(pid) => {
                super::procs::quit(pid);
            }
            _ => {}
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
    bold(std::env::var("CMDMEOW_CARD_TITLE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(15.0))
}

/// Card title (15/600), an optional status chip after it, and an optional
/// big value on the right. Returns the y where the content starts.
fn card_title(p: &egui::Painter, r: Rect, title: &str, status: Option<(&str, Color32)>, big: Option<&str>) -> f32 {
    let mut right = r.right();
    // the title names the big number ("CPU load"), the number stays plain ("16%")
    if let Some(num) = big {
        let n = p.text(Pos2::new(r.right(), r.top() + 28.0), Align2::RIGHT_BOTTOM, num, ty::hero(), TEXT());
        right = n.left() - sp::M;
    }
    // the title sits at the card's top edge in every card, level with the
    // big number's top, so titles in a row line up too
    // a long title in a narrow card steps down to 13 rather than cut (R10)
    let mut f = title_font();
    while f.size > 13.0 && p.layout_no_wrap(title.to_owned(), f.clone(), TEXT()).size().x + 14.0 > right - r.left() {
        f.size -= 1.0;
    }
    let t = text_fit(p, Pos2::new(r.left(), r.top()), Align2::LEFT_TOP, title, f.clone(), TEXT(), right - r.left() - 14.0);
    // the card opens its details: a chevron after the title says so
    let ch = p.text(Pos2::new(t.right() + 4.0, t.bottom()), Align2::LEFT_BOTTOM, "›", f, TEXT_FAINT());
    if let Some((label, c)) = status {
        if ch.right() + 80.0 < right {
            chip(p, Pos2::new(ch.right() + sp::S, t.center().y), Align2::LEFT_CENTER, label, (with_alpha(c, 0.16), c));
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
    let th = ([ts!("health.thermal.nominal"), ts!("health.thermal.fair"), ts!("health.thermal.serious"), ts!("health.thermal.critical")][s.thermal.min(3) as usize], level_status(s.thermal_level()));
    let y = card_title(p, r, ts!("health.cpu_load"), if s.thermal_level() == Level::Good { None } else { Some(th) }, Some(&load));
    let foot = r.bottom() - 18.0;
    // the history with its time span under it, as in the widgets (W5);
    // the stats are a block of their own, `sp::L` below the captions
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), foot - sp::L));
    let peak = s.cpu_history.iter().copied().fold(0.0f32, f32::max);
    tile::spark_span(p, gr, &s.cpu_history, level_color(s.thermal_level()), Some(peak.max(5.0)));
    let mut x = r.left();
    for t in [t!("health.cpu_user", pct = s.cpu_user : {:.0}).into_owned(), t!("health.cpu_system", pct = s.cpu_system : {:.0}).into_owned()] {
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
    // no Used: the ring says free of total (T2); drive health only when it isn't fine (T1)
    let mut rows: Vec<Row> = Vec::new();
    if !s.smart_status.is_empty() && !s.smart_status.eq_ignore_ascii_case("Verified") {
        rows.push((None, ts!("health.drive_health").to_string(), s.smart_status.clone()));
    }
    rows.push((None, ts!("health.trash").to_string(), s.trash_bytes.map(human_bytes).unwrap_or_else(|| "—".into())));
    rows
}

/// Where the disk ring may sit: under the title, over the lines and the button.
fn disk_ring_area(r: Rect, s: &Snapshot) -> Rect {
    let lt = r.bottom() - BTN_H_ROW - sp::M - disk_rows(s).len() as f32 * sp::LEGEND_H;
    Rect::from_min_max(Pos2::new(r.left(), r.top() + 28.0 + sp::M), Pos2::new(r.right(), lt - sp::M))
}

fn disk_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot, ring_d: f32) -> Option<SheetAction> {
    let name = if s.volume_name.is_empty() { ts!("health.macintosh_hd").to_string() } else { s.volume_name.clone() };
    card_title(p, r, &name, None, None);
    let used = if s.disk_total > 0 { 1.0 - s.disk_free as f32 / s.disk_total as f32 } else { 0.0 };
    let bottom = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - BTN_H_ROW - sp::M));
    lines(p, bottom, &disk_rows(s));
    let col = if s.disk_level() == Level::Good { tint::HEALTH() } else { level_color(s.disk_level()) };
    // SP2: the free space whole, "free of <total>" under it, as on the start ring
    ring_in(p, disk_ring_area(r, s), ring_d, used, col, &human_bytes(s.disk_free), &t!("health.free_of", size = human_bytes(s.disk_total)));
    // footer: the way to free space
    let fy = r.bottom() - BTN_H_ROW;
    let mut out = None;
    let bw = button_width(p, ts!("health.free_up_space"), BTN_SECONDARY);
    let br = Rect::from_min_size(Pos2::new(r.left(), fy), Vec2::new(bw, BTN_H_ROW));
    if pill_button(ui, br, ts!("health.free_up_space"), BTN_SECONDARY, true).clicked() {
        out = Some(SheetAction::Open("cleanup"));
    }
    out
}

fn memory_card(p: &egui::Painter, r: Rect, s: &Snapshot) {
    let word = match s.mem_level() {
        Level::Good => ts!("health.pressure.good"),
        Level::Fair => ts!("health.pressure.fair"),
        Level::Critical => ts!("health.pressure.critical"),
    };
    let pct = format!("{:.0}%", s.mem_pressure);
    let y = card_title(p, r, ts!("health.memory_pressure"), if s.mem_level() == Level::Good { None } else { Some((word, level_status(s.mem_level()))) }, Some(&pct));
    // what holds the memory as name · bar · value lines like the widgets
    // (W5), bars against the largest so they read as a comparison
    let hi = s.mem_active.max(s.mem_wired).max(s.mem_compressed).max(1) as f32;
    let bar_of = |b: u64| Some(((b as f32 / hi).clamp(0.01, 1.0), tint::HEALTH()));
    let rows: Vec<Row> = vec![
        (bar_of(s.mem_active), ts!("health.mem.apps").to_string(), human_bytes(s.mem_active)),
        (bar_of(s.mem_wired), ts!("health.mem.wired").to_string(), human_bytes(s.mem_wired)),
        (bar_of(s.mem_compressed), ts!("health.mem.compressed").to_string(), human_bytes(s.mem_compressed)),
        (None, ts!("health.swap_used").to_string(), human_bytes(s.swap_used)),
        (None, ts!("health.installed").to_string(), human_bytes(s.mem_total)),
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
            rows.push((None, ts!("health.time_left").to_string(), human_duration(min as u64 * 60)));
        }
    }
    if let Some(h) = b.health_pct {
        rows.push((None, ts!("health.health").to_string(), format!("{h:.0}%")));
    } else if !b.health_text.is_empty() {
        rows.push((None, ts!("health.health").to_string(), b.health_text.clone()));
    }
    if b.cycles > 0 {
        let of = if b.design_cycles > 0 { t!("health.cycles_of", n = b.design_cycles).into_owned() } else { String::new() };
        rows.push((None, ts!("health.cycles").to_string(), format!("{}{of}", b.cycles)));
    }
    if let Some(t) = b.temp_c {
        rows.push((None, ts!("health.temperature").to_string(), t!("unit.celsius", c = t : {:.0}).into_owned()));
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
        card_title(p, r, ts!("health.power"), Some((ts!("health.no_battery"), TEXT_DIM())), None);
        return;
    }
    // the same words as the battery sheet's header (C6)
    let state = if super::sheets::battery_held(s) { ts!("health.bat.held_short").to_string() } else { super::sheets::battery_state(s) };
    let scol = if b.charging { GREEN() } else if !b.plugged && b.percent <= 20 { RED() } else { TEXT_DIM() };
    card_title(p, r, ts!("health.battery"), Some((&state, scol)), None);
    let col = if b.percent <= 20 { RED() } else { GREEN() };
    lines(p, r, &battery_rows(s));
    ring_in(p, battery_ring_area(r, s), ring_d, b.percent as f32 / 100.0, col, &format!("{}%", b.percent), ts!("health.ring_charge"));
}

fn devices_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<super::devices_ui::PageAction> {
    let low = s.devices.iter().filter(|d| super::devices::is_low(d)).count();
    let status = if low > 0 { Some((super::devices_ui::note(&s.devices), AMBER())) } else { None };
    let y = card_title(p, r, ts!("health.devices"), status.as_ref().map(|(t, c)| (t.as_str(), *c)), None);
    super::devices_ui::card_body(ui, p, Rect::from_min_max(Pos2::new(r.left(), y), r.max), s)
}

fn network_card(ui: &mut Ui, p: &egui::Painter, r: Rect, s: &Snapshot) -> Option<SheetAction> {
    // the network's own name when macOS gives it (not just "Wi-Fi")
    let named = !s.wifi.name.is_empty() && !s.wifi.name.eq_ignore_ascii_case("wi-fi") && !s.wifi.name.eq_ignore_ascii_case("ethernet");
    let title = if named { s.wifi.name.clone() } else if s.wifi.wired { ts!("health.ethernet").to_string() } else if s.wifi.name.is_empty() { ts!("health.network").to_string() } else { ts!("health.wifi").to_string() };
    let status = if !s.online { Some((ts!("health.offline"), RED())) } else { None };
    // the download as the big number, like the Wi-Fi widget; upload in the lines
    let down = format!("↓ {}", human_rate(s.net_down));
    let y = card_title(p, r, &title, status, Some(&down));
    let mut rows: Vec<Row> = vec![(None, ts!("health.upload").to_string(), format!("↑ {}", human_rate(s.net_up)))];
    rows.push((None, ts!("health.today").to_string(), t!("health.in_out", received = human_bytes(s.net_recv_total), sent = human_bytes(s.net_sent_total)).into_owned()));
    if !s.wifi.signal.is_empty() {
        rows.push((None, ts!("health.signal").to_string(), s.wifi.signal.split(" / ").next().unwrap_or("").trim().to_string()));
    }
    // the speed only once there is one: the buttons say a test can run (T3)
    let sp_ = &s.speed;
    if sp_.running {
        rows.push((None, ts!("health.speed").to_string(), t!("health.testing_pct", pct = sp_.progress * 100.0 : {:.0}).into_owned()));
    } else if let (Some(d), Some(u)) = (sp_.down_mbps, sp_.up_mbps) {
        rows.push((None, ts!("health.speed").to_string(), t!("health.down_up_mbps", down = d : {:.0}, up = u : {:.0}).into_owned()));
    }
    let bottom = Rect::from_min_max(r.min, Pos2::new(r.right(), r.bottom() - BTN_H_ROW - sp::M));
    let lt = lines(p, bottom, &rows);
    let gr = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), lt - sp::M));
    if gr.height() > 40.0 {
        // scale to the usual traffic, not to one burst: a burst stops at the top
        let mut sorted = s.net_history.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let p75 = sorted.get(sorted.len() * 3 / 4).copied().unwrap_or(0.0);
        let cap = (p75 * 3.0).max(sorted.last().copied().unwrap_or(0.0) * 0.25).max(1.0);
        let clipped: Vec<f32> = s.net_history.iter().map(|v| v.min(cap)).collect();
        tile::spark_span(p, gr, &clipped, tint::HEALTH(), Some(cap));
    }
    let mut out = None;
    // the two tests side by side; each opens the dial in its mode
    let mut x = r.left();
    for mode in [super::monitor::SpeedMode::Quick, super::monitor::SpeedMode::Full] {
        let label = mode.label();
        let bw = button_width(p, label, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(x, r.bottom() - BTN_H_ROW), Vec2::new(bw, BTN_H_ROW));
        let tip = if mode == super::monitor::SpeedMode::Quick { ts!("health.speed_quick_tip") } else { ts!("health.speed_full_tip_short") };
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
            ("quit", "quit that process (like Activity Monitor's Quit)", ts!("health.ai.quit")),
            ("activity", "open Activity Monitor to look at it", ts!("health.ai.activity")),
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

// ======================================================================= details popup

fn details_id() -> egui::Id {
    egui::Id::unique("health-details")
}

/// Open (or close, with None) a card's details popup.
fn open_details(ctx: &egui::Context, kind: Option<Sheet>) {
    ctx.data_mut(|d| match kind {
        Some(k) => {
            d.insert_temp(details_id(), k);
        }
        None => d.remove::<Sheet>(details_id()),
    });
}

/// Width of the details popup.
const DETAILS_W: f32 = 440.0;

/// A card's details over the page: everything the panel sheet had, with the
/// card's actions. Esc, the close button or a click outside closes it.
fn details_popup(ui: &mut Ui, s: &Snapshot) -> Option<SheetAction> {
    let ctx = ui.ctx().clone();
    // screenshots: CMDMEOW_HEALTH_DETAILS=cpu|storage|memory|battery|network|devices
    if let Ok(v) = std::env::var("CMDMEOW_HEALTH_DETAILS") {
        if ctx.cumulative_pass_nr() < 3 {
            open_details(&ctx, Some(Sheet::from_id(&v).unwrap_or(Sheet::Cpu)));
        }
    }
    let kind = ctx.data(|d| d.get_temp::<Sheet>(details_id()))?;
    let w = DETAILS_W.min(ctx.content_rect().width() - 32.0);
    let h = super::sheets::details_height(ui, kind, s, w);
    let (a, close) = crate::kit::modal::details(&ctx, details_id().with(kind as u8), w, h, |ui, m| super::sheets::details(ui, m, kind, s));
    if close {
        Some(SheetAction::Close)
    } else if a != SheetAction::None {
        Some(a)
    } else {
        None
    }
}

// ======================================================================= battery settings

const SET_ROW_H: f32 = sp::SETTINGS_ROW_H;
/// A section title and the gap under it (R2), as the Bluetooth section.
const SET_TITLE_H: f32 = 14.0 + sp::M;

fn battery_settings_rows(s: &Snapshot) -> usize {
    if !s.battery.present {
        return 0;
    }
    let c = &s.charge;
    1 + if c.needs_helper() || c.installing { 1 } else if c.policy.active() { 2 } else { 0 }
}

/// The battery section's height with the gap under it (0 on a Mac without one).
fn battery_settings_height(s: &Snapshot) -> f32 {
    let n = battery_settings_rows(s);
    if n == 0 { 0.0 } else { SET_TITLE_H + n as f32 * SET_ROW_H + sp::XL }
}

/// Mac Health › Settings › Battery: the charge limit, then its two switches
/// (or the helper it needs).
fn battery_settings(ui: &mut Ui, r: Rect, s: &Snapshot) -> Option<SheetAction> {
    let n = battery_settings_rows(s);
    if n == 0 {
        return None;
    }
    use crate::kit::charge;
    let p = ui.painter().clone();
    crate::kit::glass::card_title(&p, Pos2::new(r.left(), r.top()), ts!("health.battery"));
    let card = Rect::from_min_size(Pos2::new(r.left(), r.top() + SET_TITLE_H), Vec2::new(r.width(), n as f32 * SET_ROW_H));
    glass_card(&p, card, R_CARD, 0.0, false);
    let row = |k: usize| Rect::from_min_size(Pos2::new(card.left(), card.top() + k as f32 * SET_ROW_H), Vec2::new(card.width(), SET_ROW_H));
    let c = &s.charge;
    let mut out = None;
    // the limit
    let rr = row(0);
    let labels: Vec<String> = charge::LIMITS.iter().map(|l| if *l >= 100 { ts!("health.limit_off").to_string() } else { format!("{l}%") }).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let sel = charge::LIMITS.iter().position(|l| *l == c.policy.limit).unwrap_or(0);
    let seg = Rect::from_min_size(Pos2::new(rr.right() - sp::L - 200.0, rr.center().y - sp::CONTROL_H / 2.0), Vec2::new(200.0, sp::CONTROL_H));
    if let Some(i) = segmented(ui, seg, ui.scope_id().with("health-set-limit"), &labels, sel) {
        out = Some(SheetAction::SetLimit(charge::LIMITS[i]));
    }
    text_fit(&p, Pos2::new(rr.left() + sp::L, rr.center().y), Align2::LEFT_CENTER, ts!("health.charge_limit"), font(13.0), TEXT(), seg.left() - rr.left() - 2.0 * sp::L);
    if c.needs_helper() || c.installing {
        let rr = row(1);
        hairline(&p, Pos2::new(rr.left() + sp::L, rr.top()), Pos2::new(rr.right() - sp::L, rr.top()));
        let label = if c.installing { ts!("health.installing") } else { ts!("health.install") };
        let bw = button_width(&p, label, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(rr.right() - sp::L - bw, rr.center().y - BTN_H_ROW / 2.0), Vec2::new(bw, BTN_H_ROW));
        if pill_button(ui, br, label, BTN_SECONDARY, !c.installing).clicked() {
            out = Some(SheetAction::InstallHelper);
        }
        text_fit(&p, Pos2::new(rr.left() + sp::L, rr.center().y), Align2::LEFT_CENTER, ts!("health.charging_helper"), font(13.0), TEXT(), br.left() - rr.left() - 2.0 * sp::L);
    } else if c.policy.active() {
        let sw = [
            (t!("health.sail", n = c.policy.limit.saturating_sub(charge::SAIL_GAP)).into_owned(), c.policy.sailing, SheetAction::ToggleSailing),
            (t!("health.heat", c = charge::HEAT_C : {:.0}).into_owned(), c.policy.heat, SheetAction::ToggleHeat),
        ];
        for (k, (label, on, act)) in sw.iter().enumerate() {
            let rr = row(k + 1);
            hairline(&p, Pos2::new(rr.left() + sp::L, rr.top()), Pos2::new(rr.right() - sp::L, rr.top()));
            let sr = Rect::from_min_size(Pos2::new(rr.right() - 42.0, rr.center().y - 9.0), Vec2::new(30.0, 18.0));
            if switch(ui, sr, ui.scope_id().with(("health-set-switch", k)), *on, true).clicked() {
                out = Some(*act);
            }
            text_fit(&p, Pos2::new(rr.left() + sp::L, rr.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT(), sr.left() - rr.left() - 2.0 * sp::L);
        }
    }
    out
}
