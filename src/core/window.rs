//! The full window: traffic lights, a sidebar with one entry per tool page,
//! and the selected tool's page.

use super::shell::App;
use super::tool::Cx;
use crate::kit::glass::{self, *};
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, UiBuilder, Vec2, ViewportCommand};

/// Tools that are planned but not built yet: shown dashed and disabled so
/// it's clear where they will live. An installed tool with the same name hides
/// its placeholder.
const PLANNED: [(Icon, &str); 4] = [(Icon::Crop, "Screenshot"), (Icon::Clipboard, "Clipboard"), (Icon::Fan, "Fan Control"), (Icon::Cup, "Keep Awake")];

/// `draw_lights`: paint traffic lights ourselves (screenshots; the real
/// window has native ones).
pub fn window_ui(app: &mut App, outer: &mut Ui, full: Rect, draw_lights: bool, cx: &mut Cx) {
    let ui = &mut *outer;
    let ctx = ui.ctx().clone();
    let side = Rect::from_min_size(full.min, Vec2::new(SIDEBAR_W, full.height()));
    // drag the window from the top strip of the page
    let drag = Rect::from_min_size(full.min + Vec2::new(SIDEBAR_W, 0.0), Vec2::new(full.width() - SIDEBAR_W, 16.0));
    let dr = ui.interact(drag, ui.scope_id().with("drag"), Sense::click_and_drag());
    if dr.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let p = ui.painter().clone();
    p.rect_filled(side, egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }, glass::SIDEBAR());
    hairline(&p, side.right_top(), side.right_bottom());
    if draw_lights {
        traffic_lights(ui, &p, full);
    }
    // one entry per tool page, grouped by section
    let mut pages = app.page_tools();
    // group by section in the sidebar's order (tools keep their order inside)
    pages.sort_by_key(|&i| section_rank(app.tools[i].section()));
    if !pages.contains(&app.page) {
        if let Some(&first) = pages.first() {
            app.page = first;
        }
    }
    let x = side.left() + 8.0;
    let w = side.width() - 16.0;
    // app mark under the traffic lights
    let logo_y = full.top() + 44.0;
    if let Some(tex) = crate::kit::brand::app_icon(&ctx) {
        let r = Rect::from_min_size(Pos2::new(side.left() + 16.0, logo_y), Vec2::splat(22.0));
        p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    p.text(Pos2::new(side.left() + 46.0, logo_y + 11.0), Align2::LEFT_CENTER, "Cmd+Meow", bold(13.0), TEXT());
    let planned: Vec<_> = PLANNED.iter().filter(|(_, n)| !app.tools.iter().any(|t| t.name() == *n)).collect();
    // the list sits between the logo and Settings: on a short window the gaps
    // tighten first, then it scrolls (clipped) instead of running under Settings
    let settings_top = full.bottom() - 12.0 - 30.0 - 6.0 - SIDEBAR_ROW_H;
    let list = Rect::from_min_max(Pos2::new(side.left(), logo_y + 22.0 + 4.0), Pos2::new(side.right(), settings_top - 8.0));
    let n_sections = {
        let mut v: Vec<&str> = pages.iter().map(|&i| app.tools[i].section()).collect();
        v.dedup();
        v.len() + usize::from(!planned.is_empty())
    };
    let n_rows = pages.len() + planned.len();
    let need = |gap: f32, head: f32, step: f32| n_sections as f32 * (gap + head) + n_rows as f32 * step;
    let (gap, head, step) = if need(12.0, 18.0, SIDEBAR_ROW_H + 2.0) <= list.height() { (12.0, 18.0, SIDEBAR_ROW_H + 2.0) } else { (6.0, 16.0, SIDEBAR_ROW_H) };
    let overflow = (need(gap, head, step) + 4.0 - list.height()).max(0.0);
    let scroll_id = ui.scope_id().with("sidebar-scroll");
    let mut off: f32 = ctx.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if overflow > 0.0 && ui.rect_contains_pointer(list) {
        off -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    off = off.clamp(0.0, overflow);
    ctx.data_mut(|d| d.insert_temp(scroll_id, off));
    let mut lu = ui.new_child(UiBuilder::new().max_rect(list).id_salt("sidebar-list"));
    lu.set_clip_rect(list.intersect(ui.clip_rect()));
    let ui = &mut lu;
    let p = ui.painter().clone();
    let mut y = list.top() - off;
    let mut section = "";
    for &i in &pages {
        if app.tools[i].section() != section {
            section = app.tools[i].section();
            y += gap;
            group_header(&p, Pos2::new(x + 8.0, y), section);
            y += head;
        }
        // a required permission is missing: a lock where the shortcut sits
        let locked = super::gate::missing(app, i).is_some();
        let t = &app.tools[i];
        let busy = t.busy();
        let (label, badge, meta, id) = (t.name(), t.badge(), if busy || locked { String::new() } else { no_zero(t.sidebar_meta()) }, t.id());
        let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, SIDEBAR_ROW_H));
        if sidebar_item(ui, r, ui.scope_id().with(("nav", id)), badge.icon, badge.color, label, &meta, app.page == i && !app.settings_open, false, SIDEBAR_INDENT).clicked() {
            app.page = i;
            app.settings_open = false;
            app.drag = None;
        }
        if locked && !busy {
            icon(&p, Icon::Lock, Rect::from_center_size(Pos2::new(r.right() - 14.0, r.center().y), Vec2::splat(12.0)), TEXT_FAINT());
        }
        if busy {
            // working in the background: a small spinner where the count sits
            let now = ctx.input(|i| i.time);
            spinner(&p, Pos2::new(r.right() - 14.0, r.center().y), 5.0, now, TEXT_DIM());
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        y += step;
    }
    if !planned.is_empty() {
        y += gap;
        section_header(&p, Pos2::new(x + 8.0, y), Align2::LEFT_TOP, "Planned tools");
        y += head;
        for (ic, label) in planned {
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, SIDEBAR_ROW_H));
            sidebar_item(ui, r, ui.scope_id().with(("planned", *label)), *ic, Color32::TRANSPARENT, label, "", false, true, 0.0);
            y += step;
        }
    }
    let _ = y;
    // back to the whole window for Settings and the page
    let ui = &mut *outer;
    let p = ui.painter().clone();
    // settings: widgets and tools
    let r = Rect::from_min_size(Pos2::new(x, full.bottom() - 12.0 - 30.0 - 6.0 - SIDEBAR_ROW_H), Vec2::new(w, SIDEBAR_ROW_H));
    if sidebar_item(ui, r, ui.scope_id().with("nav-settings"), Icon::Gear, tint::NEUTRAL(), "Settings", "", app.settings_open, false, 0.0).clicked() && !app.settings_open {
        app.settings_open = true;
        app.settings_tab = super::editor::TAB_GENERAL;
    }
    // back to the menu bar panel
    let r = Rect::from_min_size(Pos2::new(x, full.bottom() - 12.0 - 30.0), Vec2::new(w, 30.0));
    let resp = ui.interact(r, ui.scope_id().with("to-panel"), Sense::click());
    let hov = ctx.animate_bool(ui.scope_id().with("to-panel-h"), resp.hovered());
    if hov > 0.0 {
        p.rect_filled(r, R_SMALL, white((12.0 * hov) as u8));
    }
    icon(&p, Icon::Monitor, Rect::from_min_size(Pos2::new(r.left() + 10.0, r.center().y - 8.0), Vec2::splat(16.0)), TEXT_DIM());
    p.text(Pos2::new(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, "Menu bar panel", font(13.0), TEXT_DIM());
    if resp.on_hover_text("Show the menu bar panel").clicked() {
        app.open_panel(&ctx);
    }
    let content = Rect::from_min_max(Pos2::new(side.right() + 1.0, full.top()), full.max);
    if app.settings_open {
        let mut child = ui.new_child(UiBuilder::new().max_rect(content).id_salt("settings"));
        super::editor::settings_page(app, &mut child, content);
        return;
    }
    if !app.tools.get(app.page).is_some_and(|t| t.has_page()) {
        p.text(content.center(), Align2::CENTER_CENTER, "No tools with a page are installed", bold(15.0), TEXT_DIM());
        return;
    }
    // a required permission is missing: Allow row on top, the page dimmed and off
    super::gate::watch(app, &ctx);
    let lock = super::gate::missing(app, app.page);
    if lock.is_some() {
        super::gate::perm_page(app, ui, content, app.page);
        return;
    }
    let tool = &mut app.tools[app.page];
    let tid = tool.id();
    let mut child = ui.new_child(UiBuilder::new().max_rect(content).id_salt(("page", tid)));
    if super::tool::guard(tid, || tool.page_ui(&mut child, content, cx)).is_none() {
        stopped_page(app, ui, content, cx);
    }
}

/// The page of a tool that stopped after an error: its glyph, its name and
/// "Stopped", and a button that restarts the app.
fn stopped_page(app: &App, ui: &mut Ui, content: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let t = &app.tools[app.page];
    let title = format!("{} stopped", t.name());
    let what = format!("{} ran into an error, so Cmd+Meow turned it off to keep your other tools running. Restart Cmd+Meow to bring it back.", t.name());
    let text_w = (content.width() - 2.0 * sp::PAGE_EDGE).min(380.0);
    let mut job = egui::text::LayoutJob::default();
    job.append(&what, 0.0, egui::TextFormat { font_id: ty::body(), color: TEXT_DIM(), line_height: Some(20.0), ..Default::default() });
    job.wrap.max_width = text_w;
    job.halign = egui::Align::Center;
    let desc = p.layout_job(job);

    // badge, title, description, button: one centred column
    let badge = 64.0;
    let btn_h = sp::HEADER_CONTROL_H;
    let title_h = p.layout_no_wrap(title.clone(), ty::lead(), TEXT()).size().y;
    let total = badge + sp::XL + title_h + sp::M + desc.size().y + sp::XL + btn_h;
    let x = content.center().x;
    let mut y = content.center().y - total / 2.0;

    let bc = Pos2::new(x, y + badge / 2.0);
    p.circle_filled(bc, badge / 2.0, with_alpha(AMBER(), 0.14));
    icon(&p, t.badge().icon, Rect::from_center_size(bc, Vec2::splat(28.0)), AMBER());
    y += badge + sp::XL;

    p.text(Pos2::new(x, y), Align2::CENTER_TOP, &title, ty::lead(), TEXT());
    y += title_h + sp::M;

    p.galley(Pos2::new(x, y), desc.clone(), TEXT_DIM());
    y += desc.size().y + sp::XL;

    let label = "Restart Cmd+Meow";
    let bw = button_width(&p, label, BTN_PRIMARY);
    let r = Rect::from_min_size(Pos2::new(x - bw / 2.0, y), Vec2::new(bw, btn_h));
    if pill_button(ui, r, label, BTN_PRIMARY, true).clicked() {
        crate::kit::perms::relaunch();
        cx.quit();
    }
}

/// Sidebar order of the tool sections; unknown sections go last.
fn section_rank(section: &str) -> usize {
    ["Clean", "Monitor", "Create"].iter().position(|s| *s == section).unwrap_or(3)
}

fn traffic_lights(ui: &mut Ui, p: &egui::Painter, full: Rect) {
    let ctx = ui.ctx().clone();
    let lights = [(Color32::from_rgb(255, 95, 87), 0), (Color32::from_rgb(254, 188, 46), 1), (Color32::from_rgb(40, 200, 64), 2)];
    let any_hover = ui.rect_contains_pointer(Rect::from_min_size(full.min + Vec2::new(10.0, 8.0), Vec2::new(70.0, 24.0)));
    for (c, k) in lights {
        let center = full.min + Vec2::new(22.0 + k as f32 * 20.0, 20.0);
        let r = Rect::from_center_size(center, Vec2::splat(13.0));
        let resp = ui.interact(r, ui.scope_id().with(("light", k)), Sense::click());
        p.circle_filled(center, 6.2, c);
        crate::kit::glass::stroke_circle(&p, center, 6.2, egui::Stroke::new(0.6, Color32::from_black_alpha(60)));
        if any_hover {
            let s = egui::Stroke::new(1.2, Color32::from_black_alpha(150));
            match k {
                0 => {
                    p.line_segment([center + Vec2::new(-2.5, -2.5), center + Vec2::new(2.5, 2.5)], s);
                    p.line_segment([center + Vec2::new(-2.5, 2.5), center + Vec2::new(2.5, -2.5)], s);
                }
                1 => {
                    p.line_segment([center + Vec2::new(-3.0, 0.0), center + Vec2::new(3.0, 0.0)], s);
                }
                _ => {
                    p.line_segment([center + Vec2::new(-2.5, 2.5), center + Vec2::new(2.5, -2.5)], s);
                }
            }
        }
        if resp.clicked() {
            match k {
                0 => ctx.send_viewport_cmd(ViewportCommand::Close),
                1 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                _ => {}
            }
        }
    }
}

/// A count of nothing says nothing: "0" (or "0 B") isn't shown in the sidebar.
fn no_zero(meta: String) -> String {
    let first = meta.split_whitespace().next().unwrap_or("");
    if !first.is_empty() && first.chars().all(|c| c == '0' || c == '.' || c == ',') && meta.split_whitespace().count() <= 2 {
        String::new()
    } else {
        meta
    }
}

#[cfg(test)]
mod meta_tests {
    #[test]
    fn zero_counts_are_hidden() {
        assert_eq!(super::no_zero("0".into()), "");
        assert_eq!(super::no_zero("0 B".into()), "");
        assert_eq!(super::no_zero("11".into()), "11");
        assert_eq!(super::no_zero("Set up".into()), "Set up");
        assert_eq!(super::no_zero("10.5 GB".into()), "10.5 GB");
    }
}
