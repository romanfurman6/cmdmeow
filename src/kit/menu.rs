//! One menu look for every "⋯" and right-click menu (MN1–MN6 in
//! `docs/ui-rules.md`). Open one with [`popup`] (under its button) or
//! [`context`] (at the pointer), then add rows with [`item`], [`choice`],
//! [`sub`], [`note`] and [`separator`]. Never `ui.button` in a menu.
use crate::kit::glass::*;
use egui::containers::menu::{MenuConfig, SubMenu};
use egui::{Align2, CornerRadius, Margin, Popup, Pos2, Rect, RectAlign, Response, Sense, Stroke, Style, Ui, Vec2};

/// Row height (B2: 28 inside cards and rows).
pub const ROW_H: f32 = 28.0;
/// Padding above and below the rows; rows run edge to edge (H2).
pub const PAD: i8 = 4;
/// Text inset from the menu edge (`sp::M`).
pub const INSET: f32 = 12.0;
/// Between a menu and its button, and between a menu and its submenu.
pub const GAP: f32 = 4.0;
/// Narrowest menu.
pub const MIN_W: f32 = 168.0;

/// The menu frame: radius 10 (P1), 4 above and below, rows touching.
pub fn style(s: &mut Style) {
    egui::containers::menu::menu_style(s);
    s.spacing.menu_margin = Margin { left: 0, right: 0, top: PAD, bottom: PAD };
    s.visuals.menu_corner_radius = CornerRadius::same(R_BUTTON as u8);
    s.spacing.item_spacing.y = 0.0;
}

fn config() -> MenuConfig {
    MenuConfig::new().style(style).submenu_gap(GAP)
}

/// A menu that opens `GAP` under the button `resp`, right edges lined up
/// (MN6), flipping above when there's no room.
pub fn popup(resp: &Response) -> Popup<'_> {
    Popup::menu(resp).style(style).gap(GAP).align(RectAlign::BOTTOM_END).align_alternatives(&[RectAlign::TOP_END, RectAlign::BOTTOM_START, RectAlign::TOP_START])
}

/// A right-click menu at the pointer.
pub fn context(resp: &Response) -> Popup<'_> {
    Popup::context_menu(resp).style(style)
}

/// Paint one row: hover fill, picked fill and check, label, submenu chevron.
pub fn paint_row(p: &egui::Painter, r: Rect, label: &str, hover: f32, on: bool, has_sub: bool, enabled: bool) {
    if on {
        p.rect_filled(r, 0.0, with_alpha(ACTION(), 0.16));
    } else if hover > 0.0 && enabled {
        p.rect_filled(r, 0.0, white((ROW_HOVER as f32 * hover) as u8));
    }
    let fg = if !enabled { with_alpha(TEXT(), crate::kit::key::disabled_alpha()) } else if on { ACCENT() } else { TEXT() };
    let right = if on || has_sub { r.right() - INSET - 16.0 } else { r.right() - INSET };
    text_fit(p, Pos2::new(r.left() + INSET, r.center().y), Align2::LEFT_CENTER, label, font(13.0), fg, right - r.left() - INSET);
    if on {
        let c = Pos2::new(r.right() - INSET - 5.0, r.center().y);
        let s = Stroke::new(1.6, ACCENT());
        p.line_segment([c + Vec2::new(-5.0, 0.0), c + Vec2::new(-1.5, 3.5)], s);
        p.line_segment([c + Vec2::new(-1.5, 3.5), c + Vec2::new(5.0, -3.5)], s);
    }
    if has_sub {
        let c = Pos2::new(r.right() - INSET - 2.0, r.center().y);
        let s = Stroke::new(1.5, TEXT_DIM());
        p.line_segment([c + Vec2::new(-2.5, -4.0), c + Vec2::new(1.5, 0.0)], s);
        p.line_segment([c + Vec2::new(1.5, 0.0), c + Vec2::new(-2.5, 4.0)], s);
    }
}


/// The width every row of this menu takes: the widest row's own width,
/// measured as the rows are added (last pass's widest, so all rows match).
/// A menu is as wide as its widest item, never as wide as the window.
fn width(ui: &Ui, own: f32) -> f32 {
    let id = ui.scope_id().with("kit-menu-w");
    let pass = ui.ctx().cumulative_pass_nr();
    let (mut at, mut cur, mut last) = ui.data(|d| d.get_temp::<(u64, f32, f32)>(id)).unwrap_or((pass, 0.0, 0.0));
    if at != pass {
        last = cur;
        cur = 0.0;
        at = pass;
    }
    cur = cur.max(own.max(MIN_W));
    ui.data_mut(|d| d.insert_temp(id, (at, cur, last)));
    if ui.is_sizing_pass() { cur } else { cur.max(last) }
}

fn label_w(ui: &Ui, label: &str) -> f32 {
    ui.painter().layout_no_wrap(label.to_string(), font(13.0), TEXT()).size().x
}

fn row(ui: &mut Ui, label: &str, on: bool, has_sub: bool, enabled: bool) -> Response {
    let w = width(ui, label_w(ui, label) + 2.0 * INSET + if on || has_sub { 24.0 } else { 0.0 });
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), sense);
    // rows highlight only: no press, no sound (B10)
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.08);
    paint_row(ui.painter(), r, label, h, on, has_sub, enabled);
    resp
}

fn done(ui: &mut Ui, resp: Response) -> Response {
    if resp.clicked() {
        ui.close();
    }
    resp
}

/// A plain row; closes the menu when clicked.
pub fn item(ui: &mut Ui, label: &str) -> Response {
    let r = row(ui, label, false, false, true);
    done(ui, r)
}

/// A destructive row (Move to Trash…): `DANGER` text (B6).
pub fn danger(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let w = width(ui, label_w(ui, label) + 2.0 * INSET);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.08);
    if h > 0.0 && enabled {
        ui.painter().rect_filled(r, 0.0, white((ROW_HOVER as f32 * h) as u8));
    }
    let fg = if enabled { DANGER() } else { with_alpha(DANGER(), crate::kit::key::disabled_alpha()) };
    ui.painter().text(Pos2::new(r.left() + INSET, r.center().y), Align2::LEFT_CENTER, label, font(13.0), fg);
    done(ui, resp)
}

/// One of a set of choices: the picked one is lime 16% with a check (H3).
pub fn choice(ui: &mut Ui, label: &str, on: bool) -> Response {
    let r = row(ui, label, on, false, true);
    done(ui, r)
}

/// A row that opens a submenu `GAP` to its right, top aligned (MN5).
pub fn sub<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let r = row(ui, label, false, true, true);
    SubMenu::new().config(config()).show(ui, &r, |ui| {
        content(ui)
    }).map(|i| i.inner)
}

/// A line of information at the top of a menu (state, signature): caption
/// 11 dim, wraps, never clickable. `mono` for commands and paths.
pub fn note(ui: &mut Ui, text: &str, mono: bool) {
    // notes wrap at 320 so a long command never makes the menu wide
    let f = if mono { egui::FontId::monospace(11.0) } else { font(11.0) };
    let g = ui.painter().layout(text.to_string(), f, TEXT_DIM(), 320.0 - 2.0 * INSET);
    let w = width(ui, g.size().x + 2.0 * INSET);
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, g.size().y + 12.0), Sense::hover());
    ui.painter().galley(Pos2::new(r.left() + INSET, r.top() + 6.0), g, TEXT_DIM());
}

/// A hairline between groups of rows, edge to edge with 4 above and below.
pub fn separator(ui: &mut Ui) {
    let w = width(ui, MIN_W);
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 9.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, white(HAIRLINE)));
}
