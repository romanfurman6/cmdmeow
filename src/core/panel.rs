//! The menu-bar panel (design A, "Control Center"): a title line with the
//! window and settings buttons, the user's widgets on a 4-column grid of
//! square cells (scrolling when they don't fit), an "Edit widgets…" link
//! that opens the widget editor in the full window (see `editor.rs`), and
//! the sheet that opens beside the panel.

use super::layout;
use super::shell::App;
use super::tool::{self, Cx, Size, GRID_COLUMNS};
use crate::kit::glass::*;
use crate::kit::mechanics as m;
use egui::{Align2, Id, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2};

/// Height of the title line above the widgets.
pub const HEADER: f32 = 60.0;
/// Space below the widgets: a gap plus the "Edit widgets…" link.
pub const FOOTER: f32 = 40.0;
/// The update banner: one 44 pt row plus the gap above it.
pub const BANNER: f32 = 52.0;
/// Side padding of the widget grid.
pub const PAD: f32 = 12.0;
/// Gap between grid cells.
pub const GAP: f32 = 8.0;

/// Height the update banner takes (0 when there's nothing to say).
pub fn banner_h() -> f32 {
    if crate::kit::update::banner() {
        BANNER
    } else {
        0.0
    }
}

pub fn inner_width() -> f32 {
    m::PANEL_WIDTH - 2.0 * PAD
}

/// Side of one square grid cell for a grid `width` wide.
pub fn cell(width: f32) -> f32 {
    ((width - GAP * (GRID_COLUMNS as f32 - 1.0)) / GRID_COLUMNS as f32).max(1.0)
}

/// Pixel size of a widget of `size` on a grid `width` wide.
pub fn size_px(size: Size, width: f32) -> Vec2 {
    let c = cell(width);
    let (r, k) = size.dims();
    Vec2::new(k as f32 * c + (k as f32 - 1.0) * GAP, r as f32 * c + (r as f32 - 1.0) * GAP)
}

/// A shown widget and where it goes, relative to the grid's origin.
pub struct Placed {
    /// Index in `app.widgets`.
    pub entry: usize,
    /// Owner tool index; `None` = the shell.
    pub owner: Option<usize>,
    pub id: &'static str,
    pub size: Size,
    pub rect: Rect,
}

/// Lay out the shown widgets for a grid `width` wide: the widgets and the
/// height used.
pub fn place(app: &App, width: f32) -> (Vec<Placed>, f32) {
    let items: Vec<(usize, Option<usize>, &'static str, Size)> = app
        .widgets
        .iter()
        .enumerate()
        .filter(|(_, e)| e.on)
        .filter_map(|(entry, e)| {
            let (owner, spec) = app.widget_spec(&e.key)?;
            if owner.is_some_and(|i| !app.tool_on(i)) {
                return None;
            }
            Some((entry, owner, spec.id, spec.fit(e.size)))
        })
        .collect();
    let sizes: Vec<(u8, f32)> = items.iter().map(|it| (it.3.cols(), size_px(it.3, width).y)).collect();
    let (rects, total) = layout::flow(&sizes, width, GAP);
    let placed = items.into_iter().zip(rects).map(|((entry, owner, id, size), rect)| Placed { entry, owner, id, size, rect }).collect();
    (placed, total)
}

/// Draw one placed widget at `r`.
pub fn draw_widget(app: &mut App, ui: &mut Ui, w: &Placed, r: Rect, cx: &mut Cx) {
    let lock = w.owner.and_then(|i| super::gate::missing(app, i));
    draw_at(app, ui, w.owner, w.id, w.size, r, lock, cx);
}

/// Draw widget `id` of `owner` (`None`: the shell's) at `size` in `r`.
/// `lock`: a required permission is missing, so it shows dimmed and off.
#[allow(clippy::too_many_arguments)]
pub fn draw_at(app: &mut App, ui: &mut Ui, owner: Option<usize>, id: &'static str, size: Size, r: Rect, lock: Option<crate::kit::perms::Perm>, cx: &mut Cx) {
    match owner {
        Some(i) => {
            let salt = (app.tools[i].id(), id, size.label());
            let mut child = ui.new_child(UiBuilder::new().max_rect(r).id_salt(salt));
            if lock.is_some() {
                child.disable();
                child.multiply_opacity(0.45);
            }
            // screenshots only: CMB_DEMO_LOADING shows every tool that can load as loading
            let demo = {
                static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                *ON.get_or_init(|| std::env::var_os("CMB_DEMO_LOADING").is_some())
            } && matches!(app.tools[i].id(), "system" | "startup" | "fan");
            let tid = app.tools[i].id();
            let drawn = if tool::is_broken(tid) {
                false
            } else if lock.is_none() && (demo || app.tools[i].widget_loading(id)) {
                let b = app.tools[i].badge();
                let title = app.tools[i].widgets().into_iter().find(|w| w.id == id).map(|w| w.title).unwrap_or_else(|| app.tools[i].name());
                let lid = child.scope_id().with("loading");
                crate::kit::tile::loading(&mut child, r, lid, b.icon, b.color, title);
                true
            } else {
                let t = &mut app.tools[i];
                tool::guard(tid, || t.widget_ui(id, size, &mut child, r, cx)).is_some()
            };
            if !drawn {
                // the tool stopped after an error: say so where its widget was
                let mut over = ui.new_child(UiBuilder::new().max_rect(r).id_salt((salt, "stopped")));
                let title = app.tools[i].widgets().into_iter().find(|w| w.id == id).map(|w| w.title).unwrap_or_else(|| app.tools[i].name());
                let sid = over.scope_id();
                if crate::kit::tile::stopped(&mut over, r, sid, app.tools[i].badge().icon, title).clicked() {
                    crate::kit::perms::relaunch();
                    cx.quit();
                }
            }
            if let Some(perm) = lock {
                super::gate::widget_cover(app, ui, r, perm, ui.scope_id().with(salt));
            }
        }
        None => {}
    }
}

/// Height the widget area needs.
pub fn content_height(app: &App) -> f32 {
    place(app, inner_width()).1.max(120.0)
}

pub fn panel(app: &mut App, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    super::gate::watch(app, &ctx);
    header(app, ui, rect);
    let area = Rect::from_min_max(Pos2::new(rect.left() + PAD, rect.top() + HEADER), Pos2::new(rect.right() - PAD, rect.bottom() - FOOTER - banner_h()));
    let need = content_height(app);
    if need > area.height() + 0.5 {
        // the bar runs in the right padding, beside the widgets
        let scroll_r = Rect::from_min_max(area.min, Pos2::new(area.right() + PAD - 2.0, area.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(scroll_r).layout(egui::Layout::top_down(egui::Align::Min)));
        // a slim bar, so even hovered it stays clear of the widgets
        child.spacing_mut().scroll.floating_allocated_width = 0.0;
        child.set_clip_rect(area.expand2(Vec2::new(PAD, 8.0)));
        ScrollArea::vertical().id_salt("panel-widgets").auto_shrink([false, false]).show(&mut child, |ui| {
            let (origin, _) = ui.allocate_exact_size(Vec2::new(area.width(), need), Sense::hover());
            widgets(app, ui, origin, cx);
        });
    } else {
        widgets(app, ui, Rect::from_min_size(area.min, Vec2::new(area.width(), need)), cx);
    }
    if banner_h() > 0.0 {
        let r = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.bottom() - FOOTER - sp::ROW_H), Vec2::new(rect.width() - 2.0 * PAD, sp::ROW_H));
        update_banner(ui, r);
    }
    footer(app, ui, rect);
}

/// The title line: the first enabled tool that offers one, or the app name.
pub fn title_line(app: &App, p: &egui::Painter, x0: f32, top: f32, width: f32) {
    match app.tools.iter().enumerate().filter(|(i, _)| app.tool_on(*i)).find_map(|(_, t)| t.panel_header()) {
        Some(h) => {
            // one line: "MacBook Pro  • All good"
            let cy = top + 31.0;
            let t = text_fit(p, Pos2::new(x0, cy), Align2::LEFT_CENTER, &h.title, ty::lead(), TEXT(), width * 0.65);
            let dx = t.right() + 10.0;
            // a healthy Mac shows just its name; a dot and word only when something needs a look
            if h.status.is_empty() {
                return;
            }
            p.circle_filled(Pos2::new(dx + 3.0, cy + 1.0), 3.0, h.dot);
            text_fit(p, Pos2::new(dx + 11.0, cy + 1.0), Align2::LEFT_CENTER, &h.status, ty::label(), ty::caption_color(), x0 + width - dx - 11.0);
        }
        None => {
            text_fit(p, Pos2::new(x0, top + 31.0), Align2::LEFT_CENTER, "Cmd+Meow", bold(15.0), TEXT(), width);
        }
    }
}

/// Title and status from the first tool that offers one, then the one
/// button that opens the full app (tools, Settings, Quit).
fn header(app: &mut App, ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let win = Rect::from_min_size(Pos2::new(rect.right() - 14.0 - 30.0, rect.top() + 16.0), Vec2::splat(30.0));
    let x0 = rect.left() + 16.0;
    title_line(app, &p, x0, rect.top(), win.left() - x0 - 10.0);
    if icon_button(ui, win, Icon::Window, "Open Cmd+Meow").clicked() {
        open_app(app, &ctx);
    }
}

/// The full app on the last tool page; with no page tools on, on Settings.
fn open_app(app: &mut App, ctx: &egui::Context) {
    let pages = app.page_tools();
    if pages.is_empty() {
        app.open_settings(ctx, false);
    } else {
        let page = if app.tools.get(app.page).is_some_and(|t| t.has_page()) { app.page } else { pages[0] };
        app.open_window(ctx, page);
    }
}

fn widgets(app: &mut App, ui: &mut Ui, origin: Rect, cx: &mut Cx) {
    let (placed, _) = place(app, origin.width());
    if placed.is_empty() {
        let p = ui.painter();
        p.text(origin.center_top() + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, "No widgets shown", bold(15.0), TEXT_DIM());
        p.text(origin.center_top() + Vec2::new(0.0, 64.0), Align2::CENTER_CENTER, "Use Edit widgets… to pick some.", font(13.0), TEXT_FAINT());
        return;
    }
    for w in &placed {
        draw_widget(app, ui, w, w.rect.translate(origin.min.to_vec2()), cx);
    }
}

/// Five size chips; sizes the widget doesn't offer are struck out. Returns
/// a newly picked size.
pub fn size_chips(ui: &mut Ui, rect: Rect, id: Id, offered: &[Size], cur: Size) -> Option<Size> {
    let p = ui.painter().clone();
    p.rect_filled(rect, 9.0, white(18));
    let inner = rect.shrink(2.0);
    let w = inner.width() / Size::ALL.len() as f32;
    let mut out = None;
    for (i, z) in Size::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(inner.left() + w * i as f32, inner.top()), Vec2::new(w, inner.height())).shrink2(Vec2::new(1.0, 0.0));
        let ok = offered.contains(&z);
        let resp = ui.interact(r, id.with(i), if ok { Sense::click() } else { Sense::hover() });
        if z == cur {
            p.rect_filled(r, 7.0, BLUE());
        } else if ok && resp.hovered() {
            p.rect_filled(r, 7.0, white(20));
        }
        let col = if z == cur { ON_ACTION } else if ok { TEXT_SOFT() } else { white(60) };
        let g = p.text(r.center(), Align2::CENTER_CENTER, z.label(), if z == cur { bold(12.0) } else { font(12.0) }, col);
        if !ok {
            p.line_segment([g.left_center(), g.right_center()], Stroke::new(1.0, white(60)));
        }
        let resp = resp.on_hover_text(if ok { z.name() } else { "Not offered for this widget" });
        if ok && resp.clicked() && z != cur {
            out = Some(z);
        }
        if ok && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    out
}

/// A new version: its number and Update; while it installs, a bar; when it
/// failed, Download (the release page) and a close button.
fn update_banner(ui: &mut Ui, r: Rect) {
    use crate::kit::update::{self, State};
    let p = ui.painter().clone();
    // the same card as the widgets above, text on their text column
    crate::kit::tile::paint_card(&p, r, 0.0, false);
    let icon_r = Rect::from_min_size(Pos2::new(r.left() + crate::kit::tile::PAD, r.center().y - 9.0), Vec2::splat(18.0));
    let x = icon_r.right() + sp::S;
    let btn_h = sp::CONTROL_H;
    let right = r.right() - sp::M;
    let state = update::state();
    let (title, accent) = match &state {
        State::Available(m) => (format!("Cmd+Meow {}", m.version), BLUE()),
        State::Downloading(v, _) | State::Installing(v) => (format!("Cmd+Meow {v}"), BLUE()),
        State::Restarting => ("Cmd+Meow".into(), BLUE()),
        State::Failed(..) => ("Update failed".into(), RED()),
        State::Idle => return,
    };
    icon(&p, if matches!(state, State::Failed(..)) { Icon::Info } else { Icon::Down }, icon_r, accent);
    let mut text_right = right;
    match &state {
        State::Available(_) => {
            let label = "Update";
            let bw = button_width(&p, label, BTN_PRIMARY);
            let b = Rect::from_min_size(Pos2::new(right - bw, r.center().y - btn_h / 2.0), Vec2::new(bw, btn_h));
            if pill_button(ui, b, label, BTN_PRIMARY, true).clicked() {
                update::install();
            }
            text_right = b.left() - sp::S;
        }
        State::Downloading(..) | State::Installing(_) => {
            let f = if let State::Downloading(_, f) = &state { *f } else { 1.0 };
            let w = 96.0;
            let b = Rect::from_min_size(Pos2::new(right - w, r.center().y - 2.0), Vec2::new(w, 4.0));
            bar(&p, b, f, BLUE());
            text_right = b.left() - sp::M;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        State::Failed(_, why) => {
            let close = Rect::from_min_size(Pos2::new(right - btn_h, r.center().y - btn_h / 2.0), Vec2::splat(btn_h));
            if icon_button(ui, close, Icon::Close, "Hide").clicked() {
                update::dismiss();
            }
            let label = "Download";
            let bw = button_width(&p, label, BTN_SECONDARY);
            let b = Rect::from_min_size(Pos2::new(close.left() - sp::S - bw, close.top()), Vec2::new(bw, btn_h));
            if pill_button(ui, b, label, BTN_SECONDARY, true).on_hover_text(why).clicked() {
                update::open_releases();
            }
            text_right = b.left() - sp::S;
            // why it failed, on the row itself
            ui.interact(Rect::from_min_max(r.min, Pos2::new(text_right, r.bottom())), ui.scope_id().with("update-why"), Sense::hover()).on_hover_text(why);
        }
        _ => {}
    }
    text_fit(&p, Pos2::new(x, r.center().y), Align2::LEFT_CENTER, &title, ty::title(), TEXT(), text_right - x);
}

/// "Edit widgets…", centred under the widgets.
fn footer(app: &mut App, ui: &mut Ui, rect: Rect) {
    let p = ui.painter().clone();
    let g = p.layout_no_wrap("Edit widgets…".into(), font(12.0), TEXT_FAINT());
    let c = Pos2::new(rect.center().x, rect.bottom() - FOOTER / 2.0 - 2.0);
    let r = Rect::from_center_size(c, g.size() + Vec2::new(16.0, 8.0));
    let resp = ui.interact(r, ui.scope_id().with("edit-widgets"), Sense::click());
    p.galley(c - g.size() / 2.0, g, if resp.hovered() { TEXT_DIM() } else { TEXT_FAINT() });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text("Edit the panel's widgets in the Cmd+Meow window").clicked() {
        let ctx = ui.ctx().clone();
        app.open_settings(&ctx, false);
    }
}

/// The sheet beside the panel; the tool draws all of it, header included.
pub fn detail(app: &mut App, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let Some((i, d)) = app.detail.clone() else { return };
    let tid = app.tools[i].id();
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).id_salt(("detail", tid, &d)));
    let t = &mut app.tools[i];
    if tool::guard(tid, || t.detail_ui(&d, &mut child, rect, cx)).is_none() {
        app.detail = None;
    }
}
