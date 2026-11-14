//! A tool whose required macOS permission is missing stays off: its page and
//! widgets show dimmed and don't react, and the page gets an Allow row on top
//! (a click on a widget asks too). Statuses come from the shared
//! [`super::onboard::PermWatch`], read in the background; nothing here asks
//! macOS on its own.

use super::onboard::{self, Row};
use super::shell::App;
use crate::kit::glass::*;
use crate::kit::perms::{Perm, Status};
use egui::{Pos2, Rect, Sense, Ui, Vec2};

/// Height the Allow row takes at the top of a page.
pub const BAR_H: f32 = 46.0 + 24.0;

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

/// The Allow row at the top of a locked page. Returns the rect left for the page.
pub fn page_bar(app: &mut App, ui: &mut Ui, content: Rect, perm: Perm) -> Rect {
    let now = ui.ctx().input(|i| i.time);
    let bar = Rect::from_min_size(content.min + Vec2::new(24.0, 14.0), Vec2::new(content.width() - 48.0, 46.0));
    onboard::perm_list(app, ui, bar, &[Row { perm, optional: false, tools: Vec::new(), locked: true }], 10.0, now);
    Rect::from_min_max(Pos2::new(content.left(), content.top() + BAR_H), content.max)
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
