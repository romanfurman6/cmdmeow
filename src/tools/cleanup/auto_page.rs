//! Cleanup's Auto tab and the Clean · Auto header tabs (SP27). See
//! `auto.rs` for what a run does.

use super::auto::{Every, GROUPS};
use super::engine::{Group, Phase};
use super::Cleanup as App;
use crate::kit::glass::*;
use crate::kit::sync::LockExt;
use egui::{Align2, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2};

const ROW_H: f32 = sp::SETTINGS_ROW_H;

fn tab_labels() -> [&'static str; 2] {
    [ts!("cleanup.tab_clean"), ts!("cleanup.tab_auto")]
}

/// Room the header control takes left of the page action (and its gap).
pub fn reserve(p: &egui::Painter) -> f32 {
    tabs_width(p, &tab_labels()) + sp::M
}

/// The page action slot's width (Stop / Skip / Scan again share it).
fn action_w(p: &egui::Painter) -> f32 {
    [ts!("cleanup.scan_again"), ts!("cleanup.stop"), ts!("cleanup.skip")].iter().map(|l| icon_pill_width(p, l, BTN_SECONDARY)).fold(0.0, f32::max)
}

/// The Clean · Auto tabs, left of the page
/// action when the phase has one (SP16 order).
pub fn header(app: &mut App, ui: &mut Ui, r: Rect, phase: &Phase) {
    let p = ui.painter().clone();
    let has_action = !app.auto_tab && matches!(phase, Phase::Scanning | Phase::Judging | Phase::Results);
    let right = r.right() - sp::PAGE_EDGE - if has_action { action_w(&p) + sp::M } else { 0.0 };
    let cy = r.top() + PAGE_HEADER_H / 2.0;
    let h = sp::HEADER_CONTROL_H / 2.0;
    let tw = tabs_width(&p, &tab_labels());
    let tr = Rect::from_min_max(Pos2::new(right - tw, cy - h), Pos2::new(right, cy + h));
    if let Some(k) = segmented(ui, tr, ui.scope_id().with("cleanup-tabs"), &tab_labels(), app.auto_tab as usize) {
        app.auto_tab = k == 1;
    }
}

/// The Auto tab.
pub fn tab(app: &mut App, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    page_header(&p, r, ts!("cleanup.name"), "", TEXT_DIM());
    let phase = app.clean.lock_ok().phase.clone();
    header(app, ui, r, &phase);
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
    ScrollArea::vertical().id_salt("cleanup-auto").auto_shrink([false, false]).show(&mut child, |ui| {
        let runs = app.auto.runs.len().min(5);
        let h = (body.height()).max(28.0 + 4.0 * ROW_H + 8.0 + sp::XL + 28.0 + runs.max(1) as f32 * ROW_H + 8.0 + 40.0);
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        let p = ui.painter().clone();
        let gap = 20.0;
        let x0 = area.left() + sp::PAGE_EDGE;
        let colw = (area.width() - 2.0 * sp::PAGE_EDGE - gap) / 2.0;
        let top = area.top() + 16.0;
        let mut changed = schedule_card(app, ui, &p, Pos2::new(x0, top), colw);
        changed |= groups_card(app, ui, &p, Pos2::new(x0 + colw + gap, top), colw);
        history_card(app, &p, Pos2::new(x0, top + 28.0 + 4.0 * ROW_H + 8.0 + sp::XL), colw);
        if changed && !app.demo {
            app.auto.save();
        }
    });
}

fn card_rows(p: &egui::Painter, x: f32, y: f32, w: f32, rows: usize) -> Rect {
    let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rows as f32 * ROW_H + 8.0));
    glass_card(p, r, R_CARD, 0.0, false);
    for k in 1..rows {
        let ly = r.top() + 4.0 + k as f32 * ROW_H;
        p.line_segment([Pos2::new(r.left() + 12.0, ly), Pos2::new(r.right() - 12.0, ly)], Stroke::new(1.0, white(13)));
    }
    r
}

fn row_y(card: Rect, k: usize) -> f32 {
    card.top() + 4.0 + k as f32 * ROW_H + ROW_H / 2.0
}

fn label(p: &egui::Painter, card: Rect, k: usize, text: &str) {
    p.text(Pos2::new(card.left() + 12.0, row_y(card, k)), Align2::LEFT_CENTER, text, font(13.0), TEXT());
}

fn switch_at(ui: &mut Ui, card: Rect, k: usize, id: &str, on: bool) -> bool {
    let sw = Rect::from_center_size(Pos2::new(card.right() - 27.0, row_y(card, k)), Vec2::new(30.0, 18.0));
    switch(ui, sw, ui.scope_id().with(("auto-sw", id)), on, true).clicked()
}

fn schedule_card(app: &mut App, ui: &mut Ui, p: &egui::Painter, at: Pos2, w: f32) -> bool {
    let mut changed = false;
    card_title(p, at, ts!("cleanup.auto.title"));
    let card = card_rows(p, at.x, at.y + 28.0, w, 4);
    // run: Off · Daily · Weekly · Monthly
    label(p, card, 0, ts!("cleanup.auto.run"));
    let ry = row_y(card, 0);
    let labels: Vec<&str> = Every::ALL.iter().map(|e| e.label()).collect();
    let sr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 280.0, ry - 13.0), Pos2::new(card.right() - 12.0, ry + 13.0));
    let cur = Every::ALL.iter().position(|e| *e == app.auto.every).unwrap_or(0);
    if let Some(k) = segmented(ui, sr, ui.scope_id().with("auto-every"), &labels, cur) {
        app.auto.every = Every::ALL[k];
        changed = true;
    }
    // disk almost full
    label(p, card, 1, ts!("cleanup.auto.low_disk"));
    p.text(Pos2::new(card.right() - 52.0, row_y(card, 1)), Align2::RIGHT_CENTER, ts!("cleanup.auto.under_10"), font(12.0), TEXT_FAINT());
    if switch_at(ui, card, 1, "low", app.auto.low_disk) {
        app.auto.low_disk = !app.auto.low_disk;
        changed = true;
    }
    label(p, card, 2, ts!("cleanup.auto.notify"));
    if switch_at(ui, card, 2, "notify", app.auto.notify) {
        app.auto.notify = !app.auto.notify;
        changed = true;
    }
    // last run + Run now
    let ry = row_y(card, 3);
    let busy = app.auto_run.busy;
    let last = match app.auto.last() {
        _ if busy => ts!("cleanup.cleaning").to_string(),
        Some(l) => t!("cleanup.auto.last_line", when = super::engine::days_ago(l.when), size = human_bytes(l.bytes)).into_owned(),
        None => ts!("cleanup.auto.never").to_string(),
    };
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, last, font(13.0), TEXT_DIM());
    let bl = ts!("cleanup.auto.run_now");
    let bw = button_width(p, bl, BTN_SECONDARY) - 6.0;
    let br = Rect::from_min_max(Pos2::new(card.right() - 12.0 - bw, ry - 14.0), Pos2::new(card.right() - 12.0, ry + 14.0));
    let page_busy = matches!(app.clean.lock_ok().phase, Phase::Scanning | Phase::Judging | Phase::Cleaning);
    if pill_button(ui, br, bl, BTN_SECONDARY, !busy && !page_busy && !app.auto.picked().is_empty()).clicked() && !app.demo {
        let s = app.auto.clone();
        app.auto_run.start(&s, false, ui.ctx());
    }
    changed
}

fn groups_card(app: &mut App, ui: &mut Ui, p: &egui::Painter, at: Pos2, w: f32) -> bool {
    let mut changed = false;
    card_title(p, at, ts!("cleanup.auto.groups"));
    let card = card_rows(p, at.x, at.y + 28.0, w, GROUPS.len());
    for (k, g) in GROUPS.into_iter().enumerate() {
        let cy = row_y(card, k);
        let col = junk_ramp(Group::ALL.iter().position(|x| *x == g).unwrap_or(0));
        let ic = Pos2::new(card.left() + 12.0 + 12.0, cy);
        p.circle_filled(ic, 12.0, with_alpha(col, 0.14));
        super::page::group_icon(p, g, Rect::from_center_size(ic, Vec2::splat(14.0)), col);
        p.text(Pos2::new(ic.x + 12.0 + sp::M, cy), Align2::LEFT_CENTER, g.label(), font(13.0), TEXT());
        if switch_at(ui, card, k, g.title(), app.auto.on(g)) {
            app.auto.flip(g);
            changed = true;
        }
    }
    changed
}

fn history_card(app: &App, p: &egui::Painter, at: Pos2, w: f32) {
    if app.auto.runs.is_empty() {
        return;
    }
    card_title(p, at, ts!("cleanup.auto.history"));
    let runs: Vec<_> = app.auto.runs.iter().rev().take(5).collect();
    let card = card_rows(p, at.x, at.y + 28.0, w, runs.len());
    for (k, r) in runs.into_iter().enumerate() {
        let cy = row_y(card, k);
        let day = chrono::DateTime::from_timestamp(r.when, 0).map(|d| d.with_timezone(&chrono::Local).format("%b %-d").to_string()).unwrap_or_default();
        let dr = p.text(Pos2::new(card.left() + 12.0, cy), Align2::LEFT_CENTER, day, font(13.0), TEXT());
        if r.low_disk {
            p.text(Pos2::new(dr.right() + sp::S, cy), Align2::LEFT_CENTER, ts!("cleanup.auto.disk_full"), font(12.0), TEXT_FAINT());
        }
        let sz = p.text(Pos2::new(card.right() - 12.0, cy), Align2::RIGHT_CENTER, human_bytes(r.bytes), bold(13.0), TEXT());
        p.text(Pos2::new(sz.left() - sp::M, cy), Align2::RIGHT_CENTER, tn!("cleanup.auto.items", n = r.items), font(12.0), TEXT_DIM());
    }
}
