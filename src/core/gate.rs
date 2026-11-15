//! A tool whose required macOS permission is missing stays off: its page is
//! a permission screen for that tool ([`perm_page`]), its widgets show a lock
//! and don't react (a click asks). Statuses come from the shared
//! [`super::onboard::PermWatch`], read in the background; nothing here asks
//! macOS on its own.

use super::onboard;
use super::shell::App;
use crate::kit::glass::*;
use crate::kit::perms::{Perm, Status};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};


/// Keep the statuses of every enabled tool's needs fresh (at most once a second).
pub fn watch(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let list = onboard::rows(app);
    let (tools, w) = (&app.tools, &mut app.perm_watch);
    w.poll(tools, &list, now);
    // statuses land off the ui thread; look again in a second
    if list.iter().any(|r| w.known(r.perm) != Some(Status::Allowed)) {
        ctx.request_repaint_after(crate::kit::glass::next_second());
    }
}

/// The first required system permission tool `i` is known to lack.
/// Screenshots never lock (unless a view set sample statuses).
pub fn missing(app: &App, i: usize) -> Option<Perm> {
    if app.demo && app.perm_watch.demo.is_none() {
        return None;
    }
    let t = app.tools.get(i)?;
    t.needs()
        .into_iter()
        .filter(|n| !n.optional && !matches!(n.perm, Perm::Own { .. }))
        .map(|n| n.perm)
        .find(|p| app.perm_watch.known(*p).is_some_and(|s| s != Status::Allowed))
}

/// The page of a tool whose required permission is missing (Permission →
/// Empty → Full): the tool's name and nothing else in the header, a tile
/// with the permission's icon, its name and why, and one big button. A tool
/// that needs more than one, or has optional ones, gets a chip per
/// permission under it (a tick once allowed; a click asks).
pub fn perm_page(app: &mut App, ui: &mut Ui, content: Rect, i: usize) {
    use crate::kit::empty::{self, Lead, HERO_H, TILE};
    let now = ui.ctx().input(|i| i.time);
    let p = ui.painter().clone();
    let (name, tint, needs) = {
        let t = &app.tools[i];
        let needs: Vec<_> = t.needs().into_iter().filter(|n| !matches!(n.perm, Perm::Own { .. })).collect();
        (t.name(), t.badge().color, needs)
    };
    page_header(&p, content, name, "", TEXT_DIM());
    let ok = |app: &App, perm: Perm| app.perm_watch.status(perm) == Status::Allowed;
    // the same one the lock is for: a status still being read isn't missing
    let Some(hero) = needs.iter().filter(|n| !n.optional).map(|n| n.perm).find(|&perm| app.perm_watch.known(perm).is_some_and(|s| s != Status::Allowed)) else { return };
    let required = needs.iter().filter(|n| !n.optional).count();
    let chips: Vec<Perm> = needs.iter().filter(|n| n.optional || required > 1).map(|n| n.perm).collect();
    // the block is centred in the body: tile, name, why, button, chips
    let body = Rect::from_min_max(Pos2::new(content.left(), content.top() + PAGE_HEADER_H), content.max);
    let chips_h = if chips.is_empty() { 0.0 } else { sp::L + 34.0 };
    let h = TILE.y + sp::XL + 20.0 + sp::M + 16.0 + sp::XL + HERO_H + chips_h;
    let top = body.center().y - h / 2.0 - PAGE_HEADER_H / 2.0;
    let cx = body.center().x;
    let tile = Rect::from_center_size(Pos2::new(cx, top + TILE.y / 2.0), TILE);
    empty::tile(&p, tile, tint);
    icon(&p, hero.icon(), Rect::from_center_size(tile.center(), Vec2::splat(44.0)), tint);
    // a lock on the tile's corner, like on a locked widget
    let lc = tile.right_bottom() - Vec2::splat(4.0);
    p.circle_filled(lc, 16.0, BG());
    p.circle_filled(lc, 13.0, RAISED());
    icon(&p, Icon::Lock, Rect::from_center_size(lc, Vec2::splat(12.0)), TEXT_SOFT());
    let mut y = tile.bottom() + sp::XL;
    p.text(Pos2::new(cx, y), Align2::CENTER_TOP, hero.name(), bold(17.0), TEXT());
    y += 20.0 + sp::M;
    p.text(Pos2::new(cx, y), Align2::CENTER_TOP, hero.why(), font(13.0), TEXT_DIM());
    y += 16.0 + sp::XL + HERO_H / 2.0;
    let id = ui.scope_id().with(("perm-page", i));
    let status = app.perm_watch.status(hero);
    let mut ask: Option<Perm> = None;
    if app.perm_watch.waiting(hero, now) {
        // asked: a quiet button with a spinner until macOS answers
        let label = if hero.answered_in_place() { hero.ask_label() } else { "System Settings" };
        let r = empty::hero_button(ui, Pos2::new(cx, y), id.with("wait"), label, None, Lead::Dot(egui::Color32::TRANSPARENT), None, true, 0.0);
        let tw = p.layout_no_wrap(label.to_owned(), bold(16.0), TEXT()).size().x;
        let lead = 12.0 + sp::S + 2.0;
        spinner(&p, Pos2::new(cx - (lead + tw) / 2.0 + 6.0, y), 6.0, now, ACTION());
        if r.clicked() {
            ask = Some(hero);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    } else {
        let (label, tint) = if status == Status::Denied { ("Open Settings", None) } else { (hero.ask_label(), Some(ACTION())) };
        if empty::hero_button(ui, Pos2::new(cx, y), id.with("ask"), label, None, Lead::None, tint, true, 0.0).clicked() {
            ask = Some(hero);
        }
    }
    if !chips.is_empty() {
        let cy = y + HERO_H / 2.0 + sp::L + 17.0;
        let ws: Vec<f32> = chips.iter().map(|c| empty::side_width(&p, c.name(), None)).collect();
        let mut x = cx - (ws.iter().sum::<f32>() + sp::S * (ws.len() - 1) as f32) / 2.0;
        for (&perm, w) in chips.iter().zip(ws) {
            let done = ok(app, perm);
            let (r, _) = empty::side_button(ui, Pos2::new(x, cy), id.with(("chip", perm.name())), perm.name(), None, true, |p, g| {
                if done {
                    icon(p, Icon::Check, g, GREEN());
                } else {
                    icon(p, perm.icon(), g, TEXT_DIM());
                }
            });
            if !done && r.on_hover_text(format!("{} {}", perm.ask_label(), perm.name())).clicked() {
                ask = Some(perm);
            }
            x += w + sp::S;
        }
    }
    if let Some(perm) = ask {
        app.perm_watch.ask(perm, now);
    }
}

/// Over a locked widget: a lock in the corner; a click asks for the permission.
pub fn widget_cover(app: &mut App, ui: &mut Ui, r: Rect, perm: Perm, id: egui::Id) {
    let p = ui.painter().clone();
    let c = Pos2::new(r.right() - 16.0, r.top() + 16.0);
    p.circle_filled(c, 11.0, RAISED());
    icon(&p, Icon::Lock, Rect::from_center_size(c, Vec2::splat(12.0)), TEXT_SOFT());
    let resp = ui.interact(r, id.with("perm-lock"), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text(format!("Needs {}", perm.name())).clicked() {
        app.perm_watch.ask(perm, ui.ctx().input(|i| i.time));
    }
}
