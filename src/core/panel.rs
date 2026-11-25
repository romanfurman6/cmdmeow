//! The menu-bar panel (design A, "Control Center"): a title line with Quit
//! (asks first, in place) and Open, the user's widgets on a
//! 4-column grid of square cells (scrolling when they don't fit), an "Edit widgets…" link
//! that opens the widget editor in the full window (see `editor.rs`), and
//! the sheet that opens beside the panel.

use super::layout;
use super::shell::App;
use super::tool::{self, Cx, Size, GRID_COLUMNS};
use crate::kit::glass::*;
use crate::kit::mechanics as m;
use egui::{Align2, Id, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2};

/// Settings › General "Keep panel open": the panel stays up when focus
/// leaves it and while a capture runs, so it can be screenshotted. Off by
/// default.
pub const KEEP_OPEN_KEY: &str = "panel.keep_open";
static KEEP_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn keep_open() -> bool {
    KEEP_OPEN.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_keep_open(on: bool) {
    KEEP_OPEN.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Height of the title line above the widgets.
pub const HEADER: f32 = 60.0;
/// Space below the widgets: a gap plus the "Edit widgets…" link.
pub const FOOTER: f32 = 40.0;
/// Canvas only: which update notice to draw (`CMDMEOW_BANNER=now|a|b|c`).
/// now = the 44 pt card at the bottom; a = a 36 pt tinted strip under the
/// header; b = a pill in the header; c = a 28 pt line under the header.
fn variant() -> char {
    static V: std::sync::OnceLock<char> = std::sync::OnceLock::new();
    *V.get_or_init(|| match std::env::var("CMDMEOW_BANNER").as_deref() {
        Ok("now") => '0',
        Ok("b") => 'b',
        Ok("c") => 'c',
        _ => 'a',
    })
}

/// Height of the strip (variant a) or line (variant c).
fn notice_h() -> f32 {
    match variant() {
        'a' => 36.0,
        'c' => 28.0,
        _ => sp::ROW_H,
    }
}
/// Side padding of the widget grid.
pub const PAD: f32 = 12.0;
/// Gap between grid cells.
pub const GAP: f32 = 8.0;

/// Height the update banner takes (0 when there's nothing to say).
pub fn banner_h() -> f32 {
    if !crate::kit::update::banner() {
        return 0.0;
    }
    match variant() {
        // the pill lives in the header
        'b' => 0.0,
        // the notice plus the grid gap under it
        _ => notice_h() + GAP,
    }
}

/// Space the notice takes above the widgets (0 when it's below them or in the header).
fn top_h() -> f32 {
    if variant() == '0' {
        0.0
    } else {
        banner_h()
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
    let lock = w.owner.and_then(|i| super::gate::missing(app, i).or_else(|| super::gate::widget_missing(app, i, w.id)));
    draw_at(app, ui, w.owner, w.id, w.size, r, lock, cx);
}

/// Draw widget `id` of `owner` (`None`: the shell's) at `size` in `r`.
/// `lock`: a required permission is missing, so it shows dimmed and off.
#[allow(clippy::too_many_arguments)]
pub fn draw_at(app: &mut App, ui: &mut Ui, owner: Option<usize>, id: &'static str, size: Size, r: Rect, lock: Option<crate::kit::perms::Perm>, cx: &mut Cx) {
    // W12: inside a widget only the card hovers; a pressed card takes what's on it down
    let was = crate::kit::glass::set_in_widget(true);
    draw_inner(app, ui, owner, id, size, r, lock, cx);
    crate::kit::tile::finish_press(ui.ctx());
    crate::kit::glass::set_in_widget(was);
}

#[allow(clippy::too_many_arguments)]
fn draw_inner(app: &mut App, ui: &mut Ui, owner: Option<usize>, id: &'static str, size: Size, r: Rect, lock: Option<crate::kit::perms::Perm>, cx: &mut Cx) {
    match owner {
        Some(i) => {
            let salt = (app.tools[i].id(), id, size.label());
            if let Some(perm) = lock {
                // W10: the widget's name and what it needs, readable; only the glyph is dimmed
                let b = app.tools[i].badge();
                let title = app.tools[i].widgets().into_iter().find(|w| w.id == id).map(|w| w.title).unwrap_or_else(|| app.tools[i].name());
                let mut child = ui.new_child(UiBuilder::new().max_rect(r).id_salt((salt, "locked")));
                let lid = child.scope_id();
                if crate::kit::tile::locked(&mut child, r, lid, b.icon, title, perm.name()).on_hover_text(t!("app.needs", perm = perm.name())).clicked() {
                    app.perm_watch.ask(perm, ui.ctx().input(|i| i.time));
                }
                return;
            }
            let mut child = ui.new_child(UiBuilder::new().max_rect(r).id_salt(salt));
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
    let area = Rect::from_min_max(Pos2::new(rect.left() + PAD, rect.top() + HEADER + top_h()), Pos2::new(rect.right() - PAD, rect.bottom() - FOOTER - (banner_h() - top_h())));
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
        let w = rect.width() - 2.0 * PAD;
        match variant() {
            '0' => update_banner(ui, Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.bottom() - FOOTER - sp::ROW_H), Vec2::new(w, sp::ROW_H))),
            v => update_notice(ui, Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.top() + HEADER), Vec2::new(w, notice_h())), v == 'c'),
        }
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

/// How long Quit takes to turn into its confirm, s.
const ARM: f32 = 0.18;
/// An armed Quit folds back after this long without the pointer on it, s.
const ARM_IDLE: f32 = 3.0;
/// An armed Quit takes a click only after this long, so a double click on
/// ✕ doesn't quit, s.
const ARM_GUARD: f32 = 0.35;

/// Title and status from the first tool that offers one, then two round
/// buttons on the right: Quit (✕) and Open (▢) at the corner. Quit asks
/// first, right in the header: a click arms it, a second click quits. Esc,
/// a click elsewhere, hiding the panel or 3 s without the pointer on it
/// put it back.
fn header(app: &mut App, ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let size = BTN_H_ROW;
    let top = rect.top() + 31.0 - size / 2.0;
    let at = |k: f32| Rect::from_min_size(Pos2::new(rect.right() - 14.0 - size * (k + 1.0) - sp::S * k, top), Vec2::splat(size));
    let open_r = at(0.0);
    let quit_r = at(1.0);
    let id = Id::new("panel-quit");
    // armed since (None = at rest)
    let mut armed = ctx.data(|d| d.get_temp::<std::time::Instant>(id));
    if app.demo && std::env::var("CMDMEOW_PANEL_CONFIRM").is_ok() {
        armed = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    }
    let disarm = |ctx: &egui::Context| ctx.data_mut(|d| d.remove::<std::time::Instant>(id));
    if let Some(since) = armed {
        let idle = ctx.data(|d| d.get_temp::<std::time::Instant>(id.with("idle"))).unwrap_or(since);
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let hid = app.last_hide() > since;
        let idle_out = !app.demo && idle.elapsed().as_secs_f32() > ARM_IDLE;
        if hid || esc || idle_out {
            quit_log(&format!("disarm: hid {hid} esc {esc} idle {idle_out} after {} ms", since.elapsed().as_millis()));
            disarm(&ctx);
            armed = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
    let mut t = ctx.animate_bool_with_time(id.with("t"), armed.is_some(), ARM);
    if app.demo && armed.is_some() {
        // a still picture shows it fully out
        t = 1.0;
    }
    if t > 0.0 && t < 1.0 {
        ctx.request_repaint();
    }
    let x0 = rect.left() + 16.0;
    let quit_w = button_width(&p, ts!("app.quit"), BTN_DANGER).max(56.0);
    let mut clicked_quit = false;
    let mut hot = Rect::NOTHING;
    {
        // ✕ grows left into a red "Quit" pill; Open stays at the corner
        let w = size + (quit_w - size) * t;
        let r = Rect::from_min_size(Pos2::new(quit_r.right() - w, top), Vec2::new(w, size));
        // variant b: the update pill sits left of ✕; an armed Quit covers it
        let mut title_right = r.left();
        if variant() == 'b' && crate::kit::update::banner() && t < 0.01 {
            title_right = update_pill(ui, quit_r.left() - sp::S, top).left();
        }
        title_line(app, &p, x0, rect.top(), title_right - x0 - 10.0);
        if t < 0.01 {
            if round_button_in(ui, r, Icon::Close, ts!("app.quit_app")).clicked() {
                quit_log("arm");
                ctx.data_mut(|d| {
                    d.insert_temp(id, std::time::Instant::now());
                    d.remove::<std::time::Instant>(id.with("idle"));
                });
                ctx.request_repaint();
            }
        } else {
            let k = crate::kit::key::press(ui, r, id.with("key"), true, Some(crate::kit::sound::Click::Danger));
            // the fill turns red as it grows; the ✕ gives way to the word
            let mut look = crate::kit::key::Look::danger().radius(size / 2.0);
            let q = crate::kit::key::Look::quiet(0.08, 0.20);
            look.fill = lerp_color(q.fill, look.fill, t);
            look.fill_hover = lerp_color(q.fill_hover, look.fill_hover, t);
            look.base = lerp_color(q.base, look.base, t);
            look.glow = look.glow.map(|g| g.gamma_multiply(t));
            let face = crate::kit::key::paint(&p, r, &look, &k);
            if t < 0.5 {
                icon(&p, Icon::Close, Rect::from_center_size(face.center(), Vec2::splat(size * 0.46)), TEXT_SOFT().gamma_multiply(1.0 - t * 2.0));
            } else {
                let col = on_color(DANGER());
                p.text(face.center(), Align2::CENTER_CENTER, ts!("app.quit"), bold(13.0), col.gamma_multiply((t - 0.5) * 2.0));
            }
            if k.resp.hovered() {
                ctx.data_mut(|d| d.insert_temp(id.with("idle"), std::time::Instant::now()));
            }
            clicked_quit = k.resp.clicked();
            hot = r;
        }
        if round_button_in(ui, open_r, Icon::Window, ts!("app.open_app")).clicked() {
            disarm(&ctx);
            open_app(app, &ctx);
        }
    }
    if let Some(since) = armed {
        if clicked_quit && since.elapsed().as_secs_f32() > ARM_GUARD {
            quit_log("quit");
            disarm(&ctx);
            if !app.demo {
                app.quit(&ctx);
            }
        } else if !clicked_quit && ctx.input(|i| i.pointer.any_click()) && since.elapsed().as_secs_f32() > ARM_GUARD {
            // a click anywhere else puts it back
            if ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| !hot.contains(pos)) {
                quit_log("disarm: click elsewhere");
                disarm(&ctx);
            }
        }
    }
}

/// CMDMEOW_TRAY_LOG=1: say when Quit arms and why it folds back.
fn quit_log(what: &str) {
    if std::env::var_os("CMDMEOW_TRAY_LOG").is_some() {
        eprintln!("[panel-quit] {what}");
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
        p.text(origin.center_top() + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, ts!("app.no_widgets"), bold(15.0), TEXT_DIM());
        p.text(origin.center_top() + Vec2::new(0.0, 64.0), Align2::CENTER_CENTER, ts!("app.use_edit"), font(13.0), TEXT_FAINT());
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
    // the kit segmented look (B rules): quiet track, raised thumb
    p.rect_filled(rect, R_SMALL, white(15));
    let inner = rect.shrink(2.0);
    let w = inner.width() / Size::ALL.len() as f32;
    let mut out = None;
    for (i, z) in Size::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(inner.left() + w * i as f32, inner.top()), Vec2::new(w, inner.height())).shrink2(Vec2::new(1.0, 0.0));
        let ok = offered.contains(&z);
        let (k, face) = crate::kit::glass::segment(ui, &p, r, id.with(i), z == cur, ok);
        let resp = k.resp;
        let col = if z == cur { TEXT() } else if ok { lerp_color(TEXT_DIM(), TEXT(), k.hover) } else { white(60) };
        let g = p.text(face.center(), Align2::CENTER_CENTER, z.label(), font(12.0), col);
        if !ok {
            p.line_segment([g.left_center(), g.right_center()], Stroke::new(1.0, white(60)));
        }
        let resp = resp.on_hover_text(if ok { z.name() } else { ts!("app.not_offered") });
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
        State::Available(m) => (format!("Cmd+Meow {}", m.version), ACTION()),
        State::Downloading(v, _) | State::Installing(v) => (format!("Cmd+Meow {v}"), ACTION()),
        State::Restarting => ("Cmd+Meow".into(), ACTION()),
        State::Failed(..) => (ts!("app.update_failed").into(), RED()),
        State::Idle => return,
    };
    icon(&p, if matches!(state, State::Failed(..)) { Icon::Info } else { Icon::Down }, icon_r, accent);
    let mut text_right = right;
    match &state {
        State::Available(_) => {
            let label = ts!("app.update");
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
            bar(&p, b, f, ACTION());
            text_right = b.left() - sp::M;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        State::Failed(_, why) => {
            let close = Rect::from_min_size(Pos2::new(right - btn_h, r.center().y - btn_h / 2.0), Vec2::splat(btn_h));
            if icon_button(ui, close, Icon::Close, ts!("app.hide")).clicked() {
                update::dismiss();
            }
            let label = ts!("app.download");
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

/// Variant a (strip) / c (line): the update under the header, smaller than a
/// widget and not drawn like one. A: an `ACTION` 12% tint, R_BUTTON, no edge
/// or base, ↓ + "Cmd+Meow 0.2.1" + a 28 pt Primary "Update". C: no fill, ↓
/// + the version in dim, an ACCENT "Update" link at the right.
fn update_notice(ui: &mut Ui, r: Rect, line: bool) {
    use crate::kit::update::{self, State};
    let p = ui.painter().clone();
    let state = update::state();
    let failed = matches!(state, State::Failed(..));
    let accent = if failed { RED() } else { ACTION() };
    let title: String = match &state {
        State::Available(m) => format!("Cmd+Meow {}", m.version),
        State::Downloading(v, _) | State::Installing(v) => format!("Cmd+Meow {v}"),
        State::Restarting => "Cmd+Meow".into(),
        State::Failed(..) => ts!("app.update_failed").into(),
        State::Idle => return,
    };
    let inset = if line { 0.0 } else { 4.0 };
    if !line {
        p.rect_filled(r, R_BUTTON, with_alpha(accent, 0.12));
    }
    let ic = Rect::from_center_size(Pos2::new(r.left() + if line { 8.0 } else { sp::M + 7.0 }, r.center().y), Vec2::splat(15.0));
    icon(&p, if failed { Icon::Info } else { Icon::Down }, ic, if line { ACCENT() } else { accent });
    let x = ic.right() + sp::S;
    let right = r.right() - inset - if line { 4.0 } else { 0.0 };
    let btn_h = sp::CONTROL_H;
    let mut text_right = right;
    match &state {
        State::Available(_) => {
            let label = ts!("app.update");
            if line {
                let lr = link(ui, Pos2::new(right, r.center().y), Align2::RIGHT_CENTER, label, 12.0);
                if lr.clicked() {
                    update::install();
                }
                text_right = lr.rect.left() - sp::S;
            } else {
                let bw = button_width(&p, label, BTN_PRIMARY);
                let b = Rect::from_min_size(Pos2::new(right - bw, r.center().y - btn_h / 2.0), Vec2::new(bw, btn_h));
                if pill_button(ui, b, label, BTN_PRIMARY, true).clicked() {
                    update::install();
                }
                text_right = b.left() - sp::S;
            }
        }
        State::Downloading(..) | State::Installing(_) => {
            let f = if let State::Downloading(_, f) = &state { *f } else { 1.0 };
            let w = 72.0;
            let b = Rect::from_min_size(Pos2::new(right - w - if line { 0.0 } else { 8.0 }, r.center().y - 2.0), Vec2::new(w, 4.0));
            bar(&p, b, f, ACTION());
            text_right = b.left() - sp::M;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        State::Failed(_, why) => {
            let cs = if line { 20.0 } else { btn_h };
            let close = Rect::from_min_size(Pos2::new(right - cs, r.center().y - cs / 2.0), Vec2::splat(cs));
            if icon_button(ui, close, Icon::Close, ts!("app.hide")).clicked() {
                update::dismiss();
            }
            let label = ts!("app.download");
            if line {
                let lr = link(ui, Pos2::new(close.left() - sp::S, r.center().y), Align2::RIGHT_CENTER, label, 12.0).on_hover_text(why);
                if lr.clicked() {
                    update::open_releases();
                }
                text_right = lr.rect.left() - sp::S;
            } else {
                let bw = button_width(&p, label, BTN_SECONDARY);
                let b = Rect::from_min_size(Pos2::new(close.left() - sp::S - bw, close.top()), Vec2::new(bw, btn_h));
                if pill_button(ui, b, label, BTN_SECONDARY, true).on_hover_text(why).clicked() {
                    update::open_releases();
                }
                text_right = b.left() - sp::S;
            }
            ui.interact(Rect::from_min_max(r.min, Pos2::new(text_right, r.bottom())), ui.scope_id().with("update-why"), Sense::hover()).on_hover_text(why);
        }
        _ => {}
    }
    let (f, col) = if line { (font(12.0), TEXT_DIM()) } else { (bold(13.0), TEXT()) };
    text_fit(&p, Pos2::new(x, r.center().y), Align2::LEFT_CENTER, &title, f, col, text_right - x);
}

/// Variant b: the update as a 28 pt pill in the header, its right edge at
/// `right`. Available: Primary "↓ 0.2.1"; downloading: a busy pill with the
/// percent; failed: Secondary "ⓘ Download" (the release page, reason in the
/// tooltip). Returns its rect.
fn update_pill(ui: &mut Ui, right: f32, top: f32) -> Rect {
    use crate::kit::update::{self, State};
    let p = ui.painter().clone();
    let state = update::state();
    let h = BTN_H_ROW;
    let place = |w: f32| Rect::from_min_size(Pos2::new(right - w, top), Vec2::new(w, h));
    match &state {
        State::Available(m) => {
            let r = place(icon_pill_width(&p, &m.version, BTN_PRIMARY));
            if icon_pill(ui, r, Icon::Down, &m.version, BTN_PRIMARY, true).on_hover_text(ts!("app.update")).clicked() {
                update::install();
            }
            r
        }
        State::Downloading(..) | State::Installing(_) | State::Restarting => {
            let f = if let State::Downloading(_, f) = &state { *f } else { 1.0 };
            let label = format!("{}%", (f * 100.0).round() as i32);
            let r = place(icon_pill_width(&p, "100%", BTN_SECONDARY));
            busy_pill(ui, r, &label, ACTION());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            r
        }
        State::Failed(_, why) => {
            let label = ts!("app.download");
            let r = place(icon_pill_width(&p, label, BTN_SECONDARY));
            if icon_pill(ui, r, Icon::Info, label, BTN_SECONDARY, true).on_hover_text(why).clicked() {
                update::open_releases();
                update::dismiss();
            }
            r
        }
        State::Idle => place(0.0),
    }
}

/// "Edit widgets…", centred under the widgets.
fn footer(app: &mut App, ui: &mut Ui, rect: Rect) {
    let p = ui.painter().clone();
    let g = p.layout_no_wrap(ts!("app.edit_widgets").into(), font(12.0), TEXT_FAINT());
    let c = Pos2::new(rect.center().x, rect.bottom() - FOOTER / 2.0 - 2.0);
    let r = Rect::from_center_size(c, g.size() + Vec2::new(16.0, 8.0));
    let resp = ui.interact(r, ui.scope_id().with("edit-widgets"), Sense::click());
    p.galley(c - g.size() / 2.0, g, if resp.hovered() { TEXT_DIM() } else { TEXT_FAINT() });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text(ts!("app.edit_widgets_tip")).clicked() {
        let ctx = ui.ctx().clone();
        app.open_settings(&ctx, false);
    }
}
