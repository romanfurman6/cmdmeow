//! The recorder's windows: the picker over the screen, the 3-2-1, the frame
//! around a recorded area, the control bar with its settings, and the card
//! after saving. None of them end up in the video (the recording leaves our
//! windows out, except the camera bubble).

use super::super::icons::{self, Ico};
use super::super::windows::{floating, plain};
use super::super::Capture;
use super::sys::{self, Media};
use super::{timeline, Chosen, Confirm, Phase, Source};
use crate::core::tool::Cx;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};
use std::time::Instant;

const DIM: Color32 = Color32::from_rgba_premultiplied(5, 5, 7, 140);
/// Cards and bars over the screen: the theme's popover, so the text on
/// them (TEXT and co.) reads in light mode too.
#[allow(non_snake_case)]
fn CARD() -> Color32 { POPOVER() }
#[allow(non_snake_case)]
fn ROWS() -> Color32 { white(12) }
/// A floating bar: the popover, slightly see-through.
fn bar_fill(a: u8) -> Color32 { with_alpha(POPOVER(), a as f32 / 255.0) }
const LIVE: Color32 = Color32::from_rgb(0xFF, 0x69, 0x61);
const PAUSED: Color32 = Color32::from_rgb(0xFF, 0xB3, 0x40);

/// Everything the recorder shows, once per pass.
pub fn show(c: &mut Capture, ctx: &egui::Context, cx: &mut Cx) {
    picker(c, ctx, cx);
    countdown(c, ctx);
    frame(c, ctx);
    bar(c, ctx);
    camera_controls(c, ctx);
    bar_settings(c, ctx);
    saved(c, ctx);
}

fn tex(ctx: &egui::Context, slot: &mut Option<egui::TextureHandle>, img: &image::RgbaImage) -> egui::TextureId {
    if slot.is_none() {
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        *slot = Some(ctx.load_texture("capture-recording", ci, egui::TextureOptions::LINEAR));
    }
    slot.as_ref().unwrap().id()
}

/// A round icon button; `on` gives it a quiet fill.
fn icon_button(ui: &mut Ui, r: Rect, id: egui::Id, ico: Ico, tip: &str, color: Color32, on: bool) -> bool {
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { white(36) } else if on { white(26) } else { Color32::TRANSPARENT };
    p.circle_filled(r.center(), r.width() / 2.0, fill);
    icons::draw(p, ico, Rect::from_center_size(r.center(), Vec2::splat(r.width() * 0.47)), color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip).clicked()
}

/// The camera and microphone lists, for the picker's menus.
struct Devs {
    menu: Option<Media>,
    cams: Vec<sys::Device>,
    mics: Vec<sys::Device>,
    cam: Option<String>,
    mic: Option<String>,
    /// The mic's level now, 0..1, while it's listened to.
    level: Option<f32>,
}

const METER_ON: Color32 = Color32::from_rgb(0x34, 0xC7, 0x59);

/// Five bars, lit up to `level`, in `r` (bottom aligned).
fn meter_bars(p: &egui::Painter, r: Rect, level: f32) {
    let w = (r.width() - 4.0 * 2.0) / 5.0;
    for i in 0..5 {
        let h = r.height() * (0.4 + 0.15 * i as f32);
        let x = r.left() + i as f32 * (w + 2.0);
        let bar = Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + w, r.bottom()));
        let lit = level >= (i as f32 + 1.0) / 6.0;
        p.rect_filled(bar, 1.0, if lit { METER_ON } else { white(40) });
    }
}

impl Devs {
    fn of(&self, m: Media) -> (&[sys::Device], Option<&str>) {
        match m {
            Media::Camera => (&self.cams, self.cam.as_deref()),
            Media::Mic => (&self.mics, self.mic.as_deref()),
        }
    }
}

const MENU_ROW: f32 = 30.0;

/// A list of devices under `at` (top left), the one in use ticked. Picks
/// push `Do::Use`; a click anywhere else closes it. Returns its rect.
fn device_menu(ui: &mut Ui, p: &egui::Painter, at: Pos2, w: f32, m: Media, devs: &Devs, todo: &mut Vec<Do>) -> Rect {
    let (list, cur) = devs.of(m);
    let rows = list.len().max(1);
    let r = Rect::from_min_size(at, Vec2::new(w, rows as f32 * MENU_ROW + 8.0));
    p.rect_filled(r.translate(Vec2::new(0.0, 4.0)), 10.0, Color32::from_black_alpha(70));
    p.rect_filled(r, 10.0, CARD());
    p.rect_stroke(r, 10.0, Stroke::new(1.0, white(36)), StrokeKind::Inside);
    let id = ui.scope_id().with(("dev-menu", m == Media::Mic));
    // swallow clicks on the card itself
    let _ = ui.interact(r, id, Sense::click());
    if list.is_empty() {
        let none = if m == Media::Mic { "No microphone found" } else { "No camera found" };
        p.text(Pos2::new(r.left() + 14.0, r.top() + 4.0 + MENU_ROW / 2.0), Align2::LEFT_CENTER, none, font(12.0), TEXT_DIM());
    }
    for (i, dev) in list.iter().enumerate() {
        let rr = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.top() + 4.0 + i as f32 * MENU_ROW), Vec2::new(w - 8.0, MENU_ROW));
        let resp = ui.interact(rr, id.with(i), Sense::click());
        if resp.hovered() {
            p.rect_filled(rr, 6.0, white(26));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if cur == Some(dev.id.as_str()) {
            p.text(Pos2::new(rr.left() + 12.0, rr.center().y), Align2::CENTER_CENTER, "✓", bold(12.0), TEXT());
        }
        text_fit(p, Pos2::new(rr.left() + 26.0, rr.center().y), Align2::LEFT_CENTER, &dev.name, font(12.0), TEXT(), rr.width() - 34.0);
        if resp.clicked() {
            todo.push(Do::Use(m, dev.id.clone()));
        }
    }
    if ui.input(|i| i.pointer.any_click()) && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|q| r.contains(q)) && !todo.iter().any(|t| matches!(t, Do::Menu(_))) {
        todo.push(Do::Menu(None));
    }
    r
}

/// A small ▾ after a device name, so it reads as a menu.
fn chevron(p: &egui::Painter, c: Pos2, color: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-3.5, -1.5), c + Vec2::new(3.5, -1.5), c + Vec2::new(0.0, 2.5)], color, Stroke::NONE));
}

fn small_switch(ui: &mut Ui, r: Rect, id: egui::Id, on: bool) -> bool {
    let resp = ui.interact(r, id, Sense::click());
    let t = ui.ctx().animate_bool(id.with("t"), on);
    let p = ui.painter();
    p.rect_filled(r, r.height() / 2.0, lerp_color(white(41), BLUE(), t));
    let d = r.height() - 4.0;
    let x = r.left() + 2.0 + d / 2.0 + (r.width() - 4.0 - d) * t;
    p.circle_filled(Pos2::new(x, r.center().y), d / 2.0, Color32::WHITE);
    resp.clicked()
}

// ---------------------------------------------------------------- picker

enum Do {
    Cancel,
    Source(Source),
    Display,
    Start(Chosen),
    Camera,
    Mic,
    System,
    /// Open (or close) a device list.
    Menu(Option<Media>),
    Use(Media, String),
    Settings,
}

fn picker(c: &mut Capture, ctx: &egui::Context, cx: &mut Cx) {
    let Phase::Pick(pick) = &c.rec.phase else { return };
    let Some(d) = c.rec.displays.get(pick.display).cloned() else {
        c.rec.cancel();
        return;
    };
    let title = &plain("capture-rec-picker");
    let lift = pick.frames < 5;
    let first = pick.frames == 0;
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_position(d.frame.min)
        .with_inner_size(d.frame.size())
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(true);
    let mut todo: Vec<Do> = Vec::new();
    let s = c.settings.rec.clone();
    let cam_name = c.rec.device_name(Media::Camera, &s);
    let mic_name = c.rec.device_name(Media::Mic, &s);
    let devs = Devs { menu: c.rec.menu, cams: c.rec.cams.clone(), mics: c.rec.mics.clone(), cam: c.rec.device_id(Media::Camera, &s), mic: c.rec.device_id(Media::Mic, &s), level: c.rec.meter.as_ref().map(|m| m.level()) };
    let many = c.rec.displays.len() > 1;
    let shown = crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), builder, |ui, _| {
        let vctx = ui.ctx().clone();
        if lift {
            sys::lift(title, Some(d.id));
            vctx.request_repaint();
        }
        if first {
            vctx.send_viewport_cmd(ViewportCommand::Focus);
            crate::kit::native::activate();
        }
        let Phase::Pick(pick) = &mut c.rec.phase else { return };
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let to_local = |g: Rect| g.translate(-d.frame.min.to_vec2() + r.min.to_vec2());
        let to_global = |l: Pos2| l - r.min.to_vec2() + d.frame.min.to_vec2();
        match pick.source {
            Source::Display => {
                p.rect_filled(r, 0.0, DIM);
                p.rect_stroke(r, 0.0, Stroke::new(3.0, BLUE()), StrokeKind::Inside);
                let card = Rect::from_center_size(r.center(), Vec2::new(440.0, 464.0));
                display_card(ui, &p, card, &d, &s, many, &cam_name, &mic_name, &devs, &mut todo);
                if devs.menu.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    todo.push(Do::Start(super::chose_display(&d, &s)));
                }
            }
            Source::Window => {
                let hover = ui.input(|i| i.pointer.hover_pos()).map(to_global);
                let win = hover.and_then(|h| sys::window_at(&pick.windows, h)).cloned();
                match &win {
                    Some(w) => {
                        let wr = to_local(w.frame).intersect(r);
                        dim_around(&p, r, wr);
                        p.rect_stroke(wr, 6.0, Stroke::new(3.0, BLUE()), StrokeKind::Outside);
                        let label = format!("{} · {} × {}", w.label(), w.frame.width().round(), w.frame.height().round());
                        let g = p.layout_no_wrap(label, bold(12.0), TEXT());
                        let lr = Rect::from_center_size(wr.center() - Vec2::new(0.0, 14.0), g.size() + Vec2::new(20.0, 10.0));
                        p.rect_filled(lr, lr.height() / 2.0, CARD());
                        p.galley(lr.center() - g.size() / 2.0, g, TEXT());
                        let hr = Rect::from_center_size(wr.center() + Vec2::new(0.0, 16.0), Vec2::new(110.0, 22.0));
                        p.rect_filled(hr, 11.0, BLUE());
                        p.text(hr.center(), Align2::CENTER_CENTER, "Click to record", bold(11.0), ON_ACTION);
                    }
                    None => {
                        p.rect_filled(r, 0.0, DIM);
                    }
                }
                let resp = ui.interact(r, ui.scope_id().with("win-pick"), Sense::click());
                vctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                let bar = top_bar(ui, &p, r, pick.source, &s, &devs, &mut todo);
                if resp.clicked() && devs.menu.is_none() && !resp.interact_pointer_pos().is_some_and(|q| bar.contains(q)) {
                    if let Some(w) = win {
                        todo.push(Do::Start(super::chose_window(&d, &w, &s)));
                    }
                }
            }
            Source::Area => {
                let resp = ui.interact(r, ui.scope_id().with("area-pick"), Sense::click_and_drag());
                vctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                if resp.drag_started() {
                    if let Some(q) = resp.interact_pointer_pos() {
                        pick.drag = Some((q, q));
                    }
                }
                if let (Some((a, _)), Some(q)) = (pick.drag, resp.interact_pointer_pos()) {
                    if resp.dragged() {
                        pick.drag = Some((a, q));
                    }
                }
                if resp.drag_stopped() {
                    if let Some((a, b)) = pick.drag.take() {
                        let lr = Rect::from_two_pos(a, b);
                        if lr.width() >= 16.0 && lr.height() >= 16.0 {
                            pick.area = Some(Rect::from_two_pos(to_global(a), to_global(b)));
                        }
                    }
                }
                let shown_area = pick.drag.map(|(a, b)| Rect::from_two_pos(a, b)).or_else(|| pick.area.filter(|g| g.intersects(d.frame)).map(to_local));
                match shown_area {
                    Some(ar) => {
                        dim_around(&p, r, ar);
                        p.rect_stroke(ar, 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Outside);
                        let size = format!("{} × {}", ar.width().round(), ar.height().round());
                        tag(&p, Pos2::new(ar.left(), ar.top() - 26.0).max(r.min + Vec2::new(4.0, 4.0)), &size);
                        if pick.drag.is_none() {
                            if let Some(ch) = pick.area.and_then(|g| super::chose_area(&d, g, &s)) {
                                let below = ar.bottom() + 12.0 + 40.0 < r.bottom();
                                let ty = if below { ar.bottom() + 12.0 } else { ar.bottom() - 52.0 };
                                let tr = Rect::from_min_size(Pos2::new(ar.center().x - 60.0, ty), Vec2::new(120.0, 40.0));
                                p.rect_filled(tr, 20.0, CARD());
                                p.rect_stroke(tr, 20.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
                                let sb = tr.shrink(5.0);
                                let sr = ui.interact(sb, ui.scope_id().with("area-start"), Sense::click());
                                p.rect_filled(sb, sb.height() / 2.0, if sr.hovered() { lerp_color(BLUE(), Color32::WHITE, 0.15) } else { BLUE() });
                                p.text(sb.center(), Align2::CENTER_CENTER, "Start  Enter", bold(13.0), ON_ACTION);
                                if sr.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    todo.push(Do::Start(ch));
                                }
                            }
                        }
                    }
                    None => {
                        p.rect_filled(r, 0.0, DIM);
                        let g = p.layout_no_wrap("Drag over what to record".into(), bold(15.0), TEXT());
                        p.galley(r.center() - g.size() / 2.0, g, TEXT());
                    }
                }
                let _ = top_bar(ui, &p, r, pick.source, &s, &devs, &mut todo);
            }
        }
        if devs.menu.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            todo.push(Do::Menu(None));
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || vctx.input(|i| i.viewport().close_requested()) {
            todo.push(Do::Cancel);
        }
        // the mouse repaints on its own; only the mic meter moves by itself
        if devs.level.is_some() {
            vctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let _ = (&mut todo, &cam_name, &mic_name);
    });
    if shown.is_some() {
        if let Phase::Pick(pick) = &mut c.rec.phase {
            pick.frames += 1;
        }
    }
    for t in todo {
        match t {
            Do::Cancel => c.rec.cancel(),
            Do::Source(src) => c.rec.set_source(src),
            Do::Display => c.rec.next_display(),
            Do::Start(ch) => {
                let s = c.settings.rec.clone();
                c.rec.choose(ch, &s);
            }
            Do::Camera => c.rec.toggle_camera(&mut c.settings.rec),
            Do::Mic => c.rec.toggle_mic(&mut c.settings.rec),
            Do::System => {
                c.settings.rec.system_audio = !c.settings.rec.system_audio;
                c.save_settings();
            }
            Do::Menu(m) => {
                if m.is_some() {
                    // fresh lists, in case something was plugged in
                    c.rec.mics = sys::devices(Media::Mic);
                    c.rec.cams = sys::devices(Media::Camera);
                }
                c.rec.menu = m;
            }
            Do::Use(m, id) => c.rec.use_device(m, id, &mut c.settings.rec),
            Do::Settings => {
                c.rec.cancel();
                c.vtab = super::super::VTab::Settings;
                cx.open_page(super::super::VIDEO_ID, "settings");
            }
        }
    }

    // the big card for a whole display
    #[allow(clippy::too_many_arguments)]
    fn display_card(ui: &mut Ui, p: &egui::Painter, card: Rect, d: &sys::Display, s: &crate::tools::capture::settings::RecSettings, many: bool, cam: &str, mic: &str, devs: &Devs, todo: &mut Vec<Do>) {
        p.rect_filled(card, 16.0, CARD());
        p.rect_stroke(card, 16.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let id = ui.scope_id().with("rec-card");
        let x0 = card.left() + 16.0;
        let x1 = card.right() - 16.0;
        let mut y = card.top() + 16.0;
        if let Some(i) = segmented(ui, Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 34.0)), id.with("src"), &["Display", "Window", "Area"], 0) {
            todo.push(Do::Source(Source::ALL[i]));
        }
        y += 34.0 + 16.0;
        icons::draw(p, Ico::Screen, Rect::from_center_size(Pos2::new(card.center().x, y + 18.0), Vec2::splat(36.0)), TEXT_SOFT());
        y += 42.0;
        let name = if many { format!("{}  ⌄", d.name) } else { d.name.clone() };
        let nr = Rect::from_center_size(Pos2::new(card.center().x, y + 12.0), Vec2::new(x1 - x0, 28.0));
        let nresp = ui.interact(nr, id.with("name"), if many { Sense::click() } else { Sense::hover() });
        text_fit(p, nr.center(), Align2::CENTER_CENTER, &name, bold(22.0), TEXT(), nr.width());
        if many && nresp.on_hover_text("Record another display").clicked() {
            todo.push(Do::Display);
        }
        y += 28.0;
        let (w, h) = d.pixels();
        p.text(Pos2::new(card.center().x, y + 8.0), Align2::CENTER_CENTER, format!("{w} × {h} · {} fps", s.fps.min(d.fps)), font(12.0), TEXT_DIM());
        y += 28.0;

        let start = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 56.0));
        let sr = ui.interact(start, id.with("start"), Sense::click());
        // the action colour with dark text, like every primary button
        p.rect_filled(start, 12.0, if sr.hovered() { lerp_color(BLUE(), Color32::WHITE, 0.15) } else { BLUE() });
        let dot = Pos2::new(start.left() + 30.0, start.center().y);
        p.circle_filled(dot, 13.0, ON_ACTION);
        p.circle_filled(dot, 6.0, LIVE);
        p.text(Pos2::new(dot.x + 25.0, start.center().y), Align2::LEFT_CENTER, "Start recording", bold(15.0), ON_ACTION);
        let kr = Rect::from_center_size(Pos2::new(start.right() - 36.0, start.center().y), Vec2::new(44.0, 22.0));
        p.rect_filled(kr, 6.0, with_alpha(ON_ACTION, 0.14));
        p.text(kr.center(), Align2::CENTER_CENTER, "Enter", bold(11.0), ON_ACTION);
        if sr.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if sr.clicked() {
            todo.push(Do::Start(super::chose_display(d, s)));
        }
        y += 56.0 + 14.0;

        const ROW: f32 = 56.0;
        let rows = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 3.0 * ROW));
        p.rect_filled(rows, 12.0, ROWS());
        let list: [(Ico, &str, String, bool, Option<Media>); 3] = [
            (Ico::Camera, "Camera", cam.to_string(), s.camera, Some(Media::Camera)),
            (Ico::Mic, "Microphone", mic.to_string(), s.mic, Some(Media::Mic)),
            (Ico::Speaker, "System audio", "Everything you hear".into(), s.system_audio, None),
        ];
        for (k, (ico, label, dev, on, media)) in list.into_iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(x0, y + k as f32 * ROW), Vec2::new(x1 - x0, ROW));
            if k > 0 {
                p.line_segment([Pos2::new(rr.left() + 12.0, rr.top()), Pos2::new(rr.right() - 12.0, rr.top())], Stroke::new(1.0, white(15)));
            }
            icons::draw(p, ico, Rect::from_center_size(Pos2::new(rr.left() + 22.0, rr.center().y), Vec2::splat(18.0)), TEXT_SOFT());
            let (ly1, ly2) = two_lines_y(&p, rr.center().y, &font(13.0), &font(11.0));
            p.text(Pos2::new(rr.left() + 44.0, ly1), Align2::LEFT_CENTER, label, font(13.0), TEXT());
            let dr = text_fit(p, Pos2::new(rr.left() + 44.0, ly2), Align2::LEFT_CENTER, &dev, font(11.0), TEXT_DIM(), rr.width() - 170.0);
            if let Some(m) = media {
                chevron(p, Pos2::new(dr.right() + 9.0, dr.center().y), TEXT_DIM());
                let hit = dr.with_max_x(dr.right() + 16.0).expand2(Vec2::new(5.0, 2.0));
                let dresp = ui.interact(hit, id.with(("dev", k)), Sense::click());
                if dresp.hovered() || devs.menu == Some(m) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    p.rect_filled(hit, 5.0, white(20));
                }
                if dresp.clicked() {
                    todo.push(Do::Menu(if devs.menu == Some(m) { None } else { Some(m) }));
                }
            }
            let sw = Rect::from_min_size(Pos2::new(rr.right() - 52.0, rr.center().y - 11.0), Vec2::new(38.0, 22.0));
            if let (Some(Media::Mic), Some(l)) = (media, devs.level) {
                let mr = Rect::from_min_size(Pos2::new(sw.left() - 42.0, rr.center().y - 8.0), Vec2::new(28.0, 16.0));
                meter_bars(p, mr, l);
                ui.interact(mr, id.with("meter"), Sense::hover()).on_hover_text("Microphone level: say something to test it");
            }
            if small_switch(ui, sw, id.with(("sw", k)), on) {
                todo.push(match k {
                    0 => Do::Camera,
                    1 => Do::Mic,
                    _ => Do::System,
                });
            }
        }
        y += 3.0 * ROW + 14.0;
        p.text(Pos2::new(x0, y + 14.0), Align2::LEFT_CENTER, "Esc to cancel", font(12.0), TEXT_FAINT());
        let gr = Rect::from_min_size(Pos2::new(x1 - 28.0, y), Vec2::splat(28.0));
        if icon_button(ui, gr, id.with("gear"), Ico::Gear, "Recording settings", TEXT_SOFT(), true) {
            todo.push(Do::Settings);
        }
        if let Some(m) = devs.menu {
            let k = if m == Media::Camera { 0.0 } else { 1.0 };
            let top = rows.top() + k * ROW + ROW - 2.0;
            device_menu(ui, p, Pos2::new(x0 + 36.0, top), x1 - x0 - 72.0, m, devs, todo);
        }
    }

    // the compact bar for Window and Area; returns its rect
    fn top_bar(ui: &mut Ui, p: &egui::Painter, r: Rect, src: Source, s: &crate::tools::capture::settings::RecSettings, devs: &Devs, todo: &mut Vec<Do>) -> Rect {
        let br = Rect::from_center_size(Pos2::new(r.center().x, r.top() + 64.0), Vec2::new(500.0, 48.0));
        p.rect_filled(br, 24.0, CARD());
        p.rect_stroke(br, 24.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let id = ui.scope_id().with("rec-top");
        let cur = Source::ALL.iter().position(|x| *x == src).unwrap_or(0);
        if let Some(i) = segmented(ui, Rect::from_min_size(Pos2::new(br.left() + 8.0, br.top() + 8.0), Vec2::new(240.0, 32.0)), id.with("src"), &["Display", "Window", "Area"], cur) {
            todo.push(Do::Source(Source::ALL[i]));
        }
        let mut x = br.left() + 262.0;
        for (k, (on, ico_on, ico_off, tip)) in [(s.camera, Ico::Camera, Ico::Camera, "Camera"), (s.mic, Ico::Mic, Ico::MicOff, "Microphone"), (s.system_audio, Ico::Speaker, Ico::Speaker, "System audio")].into_iter().enumerate() {
            let b = Rect::from_min_size(Pos2::new(x, br.top() + 8.0), Vec2::splat(32.0));
            let color = if on { ON_ACTION } else { TEXT_FAINT() };
            if on {
                p.circle_filled(b.center(), 16.0, BLUE());
            }
            if k == 1 {
                if let Some(l) = devs.level {
                    let lr = Rect::from_min_size(Pos2::new(b.left() + 6.0, b.bottom() + 1.0), Vec2::new(20.0, 3.0));
                    p.rect_filled(lr, 1.5, white(40));
                    p.rect_filled(Rect::from_min_size(lr.min, Vec2::new(lr.width() * l, lr.height())), 1.5, METER_ON);
                }
            }
            if icon_button(ui, b, id.with(("t", k)), if on { ico_on } else { ico_off }, &format!("{tip}: {}", if on { "on" } else { "off" }), color, false) {
                todo.push(match k {
                    0 => Do::Camera,
                    1 => Do::Mic,
                    _ => Do::System,
                });
            }
            x += 34.0;
            if k < 2 {
                // the device list
                let m = if k == 0 { Media::Camera } else { Media::Mic };
                let cr = Rect::from_min_size(Pos2::new(x, br.top() + 12.0), Vec2::new(16.0, 24.0));
                let resp = ui.interact(cr, id.with(("menu", k)), Sense::click());
                if resp.hovered() || devs.menu == Some(m) {
                    p.rect_filled(cr, 5.0, white(30));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                chevron(p, cr.center(), TEXT_SOFT());
                let tip = if k == 0 { "Choose the camera" } else { "Choose the microphone" };
                if resp.on_hover_text(tip).clicked() {
                    todo.push(Do::Menu(if devs.menu == Some(m) { None } else { Some(m) }));
                }
                if devs.menu == Some(m) {
                    device_menu(ui, p, Pos2::new(b.left() - 8.0, br.bottom() + 8.0), 260.0, m, devs, todo);
                }
                x += 22.0;
            } else {
                x += 4.0;
            }
        }
        p.text(Pos2::new(br.right() - 16.0, br.center().y), Align2::RIGHT_CENTER, "Esc", font(12.0), TEXT_FAINT());
        br
    }
}

/// Dim `r` everywhere but `hole`.
fn dim_around(p: &egui::Painter, r: Rect, hole: Rect) {
    let h = hole.intersect(r);
    p.rect_filled(Rect::from_min_max(r.min, Pos2::new(r.right(), h.top())), 0.0, DIM);
    p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), h.bottom()), r.max), 0.0, DIM);
    p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), h.top()), Pos2::new(h.left(), h.bottom())), 0.0, DIM);
    p.rect_filled(Rect::from_min_max(Pos2::new(h.right(), h.top()), Pos2::new(r.right(), h.bottom())), 0.0, DIM);
}

fn tag(p: &egui::Painter, at: Pos2, text: &str) {
    let g = p.layout_no_wrap(text.to_string(), bold(11.0), Color32::from_rgb(0x1C, 0x1C, 0x1F));
    let r = Rect::from_min_size(at, g.size() + Vec2::new(12.0, 6.0));
    p.rect_filled(r, 4.0, Color32::WHITE);
    p.galley(r.min + Vec2::new(6.0, 3.0), g, Color32::from_rgb(0x1C, 0x1C, 0x1F));
}

// ---------------------------------------------------------------- countdown

fn countdown(c: &mut Capture, ctx: &egui::Context) {
    let Phase::Countdown { chosen, until, total } = &c.rec.phase else { return };
    let left = until.saturating_duration_since(Instant::now()).as_secs_f32();
    let total = *total as f32;
    let size = Vec2::splat(200.0);
    let pos = chosen.rect.center() - size / 2.0;
    let title = &plain("capture-rec-countdown");
    let mut skip = false;
    let mut cancel = false;
    let mut native_ring = false;
    // no system shadow: macOS would redo it on every frame of the ring
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(true).with_has_shadow(false), |ui, _| {
        sys::lift(title, None);
        let r = ui.max_rect();
        let p = ui.painter();
        let c0 = r.center();
        for (k, a) in [(84.0, 10u8), (78.0, 22), (74.0, 40)] {
            p.circle_filled(c0 + Vec2::new(0.0, 3.0), k, Color32::from_black_alpha(a));
        }
        p.circle_filled(c0, 70.0, bar_fill(235));
        crate::kit::glass::stroke_circle(&p, c0, 66.0, Stroke::new(2.0, Color32::from_black_alpha(90)));
        let frac = (left / total.max(0.1)).clamp(0.0, 1.0);
        // macOS animates the ring itself, smooth whatever our frame rate;
        // elsewhere (and before its window is up) egui draws it
        let [r0, g0, b0, _] = BLUE().to_array();
        native_ring = sys::countdown_ring(title, 66.0, 5.0, [r0, g0, b0], frac as f64, left as f64);
        if !native_ring {
            let a0 = -std::f32::consts::FRAC_PI_2;
            p.add(egui::epaint::PathShape::line(arc_points(c0, 66.0, a0, a0 + frac * std::f32::consts::TAU, 64), egui::epaint::PathStroke::new(5.0, BLUE())));
        }
        p.text(c0, Align2::CENTER_CENTER, format!("{}", left.ceil().max(1.0) as u32), bold(52.0), TEXT());
        if ui.interact(r, ui.scope_id().with("skip"), Sense::click()).on_hover_text("Click to start now · Esc to cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Space)) {
            skip = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
    });
    if native_ring {
        // only the number changes: wake up for the next whole second
        let next = (left - left.floor()).max(0.005);
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(next.min(0.1)));
    } else {
        // egui's ring: every display frame (vsync paces it)
        ctx.request_repaint();
    }
    if cancel {
        c.rec.cancel();
    } else if skip {
        if let Phase::Countdown { chosen, .. } = std::mem::replace(&mut c.rec.phase, Phase::Idle) {
            let s = c.settings.rec.clone();
            c.rec.launch(chosen, &s);
        }
    }
}

// ---------------------------------------------------------------- live

/// The red frame and dim around a recorded area (clicks pass through).
fn frame(c: &mut Capture, ctx: &egui::Context) {
    let (chosen, starting) = match &c.rec.phase {
        Phase::Live(l) => (&l.chosen, false),
        Phase::Starting { chosen } => (chosen, true),
        _ => return,
    };
    if !matches!(chosen.target, super::engine::Target::Area { .. }) {
        return;
    }
    let d = chosen.display.frame;
    let area = chosen.rect;
    let title = &plain("capture-rec-frame");
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, d.min, d.size()).with_has_shadow(false).with_active(false).with_mouse_passthrough(true), |ui, _| {
        sys::lift(title, None);
        let r = ui.max_rect();
        let p = ui.painter();
        let a = area.translate(-d.min.to_vec2() + r.min.to_vec2());
        dim_around(p, r, a);
        p.rect_stroke(a, 3.0, Stroke::new(2.0, if starting { white(120) } else { with_alpha(LIVE, 0.9) }), StrokeKind::Outside);
    });
}

const BAR_W: f32 = 372.0;
const BAR_CONFIRM_W: f32 = 430.0;
const BAR_H: f32 = 56.0;

/// Where the bar goes: under the area when there's room, else at the
/// bottom of the display.
pub fn bar_spot(ch: &Chosen, w: f32) -> Pos2 {
    let d = ch.display.frame;
    let below = ch.rect.bottom() + 14.0;
    let y = if matches!(ch.target, super::engine::Target::Area { .. }) && below + BAR_H < d.bottom() - 8.0 {
        below
    } else if matches!(ch.target, super::engine::Target::Area { .. }) {
        ch.rect.bottom() - BAR_H - 14.0
    } else {
        d.bottom() - BAR_H - 70.0
    };
    let cx = if matches!(ch.target, super::engine::Target::Display(_)) { d.center().x } else { ch.rect.center().x };
    Pos2::new((cx - w / 2.0).clamp(d.left() + 8.0, d.right() - w - 8.0), y)
}

fn bar(c: &mut Capture, ctx: &egui::Context) {
    let Phase::Live(live) = &mut c.rec.phase else { return };
    let w = if live.confirm.is_some() { BAR_CONFIRM_W } else { BAR_W };
    let pos = live.bar_at.unwrap_or_else(|| bar_spot(&live.chosen, w));
    let title = &plain("capture-rec-bar");
    enum Do {
        Stop,
        Mute,
        Pause,
        Ask(Confirm),
        Sure(Confirm),
        Keep,
        Settings,
    }
    let mut todo: Option<Do> = None;
    let length = live.session.length();
    let (paused, muted, confirm, has_mic, settings_open) = (live.paused, live.mic_muted, live.confirm, c.settings.rec.mic, live.settings_open);
    let mut moved: Option<Pos2> = None;
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, Vec2::new(w, BAR_H)).with_active(false), |ui, _| {
        sys::lift(title, None);
        let vctx = ui.ctx().clone();
        if let Some(or) = vctx.input(|i| i.viewport().outer_rect) {
            moved = Some(or.min);
        }
        let r = ui.max_rect().shrink(2.0);
        let p = ui.painter().clone();
        p.rect_filled(r, r.height() / 2.0, bar_fill(248));
        p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white(32)), StrokeKind::Inside);
        let id = ui.scope_id().with("rec-bar");
        let color = if paused { PAUSED } else { LIVE };
        // stop + timer
        let sr = Rect::from_min_size(Pos2::new(r.left() + 7.0, r.top() + 7.0), Vec2::new(104.0, r.height() - 14.0));
        let sresp = ui.interact(sr, id.with("stop"), Sense::click());
        p.rect_filled(sr, sr.height() / 2.0, with_alpha(color, if sresp.hovered() { 0.28 } else { 0.16 }));
        let dot = Pos2::new(sr.left() + 20.0, sr.center().y);
        crate::kit::glass::stroke_circle(&p, dot, 10.0, Stroke::new(2.0, color));
        p.rect_filled(Rect::from_center_size(dot, Vec2::splat(8.0)), 2.0, color);
        p.text(Pos2::new(dot.x + 18.0, sr.center().y), Align2::LEFT_CENTER, timeline::clock_label(length), bold(15.0), color);
        if sresp.hovered() {
            vctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if sresp.on_hover_text("Stop and save").clicked() {
            todo = Some(Do::Stop);
        }
        let mut x = sr.right() + 8.0;
        p.line_segment([Pos2::new(x, r.center().y - 12.0), Pos2::new(x, r.center().y + 12.0)], Stroke::new(1.0, white(30)));
        x += 8.0;
        let b = |x: f32| Rect::from_min_size(Pos2::new(x, r.center().y - 19.0), Vec2::splat(38.0));
        match confirm {
            Some(what) => {
                let q = if what == Confirm::Delete { format!("Delete {}?", timeline::clock_label(length)) } else { "Start over?".into() };
                p.text(Pos2::new(x + 4.0, r.center().y), Align2::LEFT_CENTER, q, bold(13.0), TEXT());
                let dr = Rect::from_min_size(Pos2::new(r.right() - 8.0 - 74.0, r.center().y - 15.0), Vec2::new(74.0, 30.0));
                let kr = Rect::from_min_size(Pos2::new(dr.left() - 8.0 - 76.0, dr.top()), Vec2::new(76.0, 30.0));
                if pill_button(ui, kr, "Keep", BTN_SECONDARY, true).clicked() {
                    todo = Some(Do::Keep);
                }
                if pill_button(ui, dr, if what == Confirm::Delete { "Delete" } else { "Restart" }, BTN_DANGER, true).clicked() {
                    todo = Some(Do::Sure(what));
                }
            }
            None => {
                let mic_tip = if !has_mic { "Microphone is off for this recording" } else if muted { "Unmute the microphone" } else { "Mute the microphone" };
                if icon_button(ui, b(x), id.with("mic"), if has_mic && !muted { Ico::Mic } else { Ico::MicOff }, mic_tip, if has_mic { TEXT_SOFT() } else { TEXT_MUTED() }, false) && has_mic {
                    todo = Some(Do::Mute);
                }
                x += 42.0;
                if icon_button(ui, b(x), id.with("pause"), if paused { Ico::Play } else { Ico::Pause }, if paused { "Resume" } else { "Pause" }, if paused { PAUSED } else { TEXT_SOFT() }, paused) {
                    todo = Some(Do::Pause);
                }
                x += 42.0;
                if icon_button(ui, b(x), id.with("restart"), Ico::Restart, "Start over", TEXT_SOFT(), false) {
                    todo = Some(Do::Ask(Confirm::Restart));
                }
                x += 42.0;
                if icon_button(ui, b(x), id.with("delete"), Ico::Trash, "Delete this recording", TEXT_SOFT(), false) {
                    todo = Some(Do::Ask(Confirm::Delete));
                }
                x += 42.0;
                if icon_button(ui, b(x), id.with("gear"), Ico::Gear, "Settings", TEXT_SOFT(), settings_open) {
                    todo = Some(Do::Settings);
                }
                x += 42.0;
                // grip: drag the bar anywhere
                let gr = Rect::from_min_max(Pos2::new(x, r.top() + 8.0), Pos2::new(r.right() - 8.0, r.bottom() - 8.0));
                let gresp = ui.interact(gr, id.with("grip"), Sense::drag());
                for (i, j) in [(0.0, -6.0), (6.0, -6.0), (0.0, 0.0), (6.0, 0.0), (0.0, 6.0), (6.0, 6.0)] {
                    p.circle_filled(Pos2::new(gr.center().x - 3.0 + i, gr.center().y + j), 1.6, TEXT_FAINT());
                }
                if gresp.hovered() {
                    vctx.set_cursor_icon(egui::CursorIcon::Grab);
                }
                if gresp.drag_started() {
                    vctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
            }
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    if let (Some(m), Phase::Live(live)) = (moved, &mut c.rec.phase) {
        if live.bar_at.is_some() || m != Pos2::ZERO {
            live.bar_at = Some(m);
        }
    }
    let s = c.settings.rec.clone();
    match todo {
        Some(Do::Stop) => c.rec.stop(),
        Some(Do::Mute) => c.rec.toggle_mute(),
        Some(Do::Pause) => c.rec.toggle_pause(),
        Some(Do::Ask(what)) => c.rec.ask_or(what, &s),
        Some(Do::Sure(what)) => c.rec.discard(what == Confirm::Restart, &s),
        Some(Do::Keep) => {
            if let Phase::Live(l) = &mut c.rec.phase {
                l.confirm = None;
            }
        }
        Some(Do::Settings) => {
            if let Phase::Live(l) = &mut c.rec.phase {
                l.settings_open = !l.settings_open;
            }
        }
        None => {}
    }
}

/// While recording: smaller, bigger and close, over the camera bubble when
/// the mouse is on it. Our own window, so it isn't in the video.
fn camera_controls(c: &mut Capture, ctx: &egui::Context) {
    use crate::tools::capture::settings::CameraSize;
    if !matches!(c.rec.phase, Phase::Live(_)) {
        return;
    }
    let Some(cam) = &c.rec.camera else { return };
    if !cam.visible() {
        return;
    }
    // look for the mouse often enough to feel immediate
    ctx.request_repaint_after(std::time::Duration::from_millis(120));
    let bubble = cam.frame();
    let size = Vec2::new(112.0, 34.0);
    let pos = Pos2::new(bubble.center().x - size.x / 2.0, bubble.bottom() - size.y - 14.0);
    let pill = Rect::from_min_size(pos, size);
    let over = sys::mouse().is_some_and(|m| bubble.expand(4.0).contains(m) || pill.contains(m));
    if !over {
        return;
    }
    let cur = c.settings.rec.camera_size;
    let sizes = [CameraSize::S, CameraSize::M, CameraSize::L];
    let k = sizes.iter().position(|&x| x == cur).unwrap_or(1);
    let mut todo: Option<i32> = None;
    let title = &plain("capture-rec-cam-ctl");
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(false).with_has_shadow(false), |ui, _| {
        sys::lift(title, None);
        let r = ui.max_rect().shrink(1.0);
        let p = ui.painter().clone();
        p.rect_filled(r, r.height() / 2.0, bar_fill(230));
        let id = ui.scope_id().with("cam-ctl");
        let b = |i: f32| Rect::from_center_size(Pos2::new(r.left() + 20.0 + i * 36.0, r.center().y), Vec2::splat(28.0));
        let glyph = |ui: &mut Ui, rect: Rect, key: &str, tip: &str, on: bool, draw: &dyn Fn(&egui::Painter, Pos2, Color32)| {
            let resp = ui.interact(rect, id.with(key), Sense::click());
            if resp.hovered() && on {
                p.circle_filled(rect.center(), 13.0, white(30));
            }
            draw(&p, rect.center(), if on { TEXT() } else { TEXT_MUTED() });
            on && resp.on_hover_text(tip).clicked()
        };
        let line = |p: &egui::Painter, a: Pos2, b: Pos2, col: Color32| {
            p.line_segment([a, b], Stroke::new(2.0, col));
        };
        if glyph(ui, b(0.0), "smaller", "Smaller", k > 0, &|p, c0, col| line(p, c0 - Vec2::new(6.0, 0.0), c0 + Vec2::new(6.0, 0.0), col)) {
            todo = Some(-1);
        }
        if glyph(ui, b(1.0), "bigger", "Bigger", k + 1 < sizes.len(), &|p, c0, col| {
            line(p, c0 - Vec2::new(6.0, 0.0), c0 + Vec2::new(6.0, 0.0), col);
            line(p, c0 - Vec2::new(0.0, 6.0), c0 + Vec2::new(0.0, 6.0), col);
        }) {
            todo = Some(1);
        }
        if glyph(ui, b(2.0), "close", "Hide the camera (bring it back in the bar's settings)", true, &|p, c0, col| {
            line(p, c0 - Vec2::splat(5.0), c0 + Vec2::splat(5.0), col);
            line(p, c0 + Vec2::new(-5.0, 5.0), c0 + Vec2::new(5.0, -5.0), col);
        }) {
            todo = Some(0);
        }
    });
    match todo {
        Some(0) => {
            if let Some(cam) = &c.rec.camera {
                cam.set_visible(false);
            }
        }
        Some(step) => {
            let next = sizes[(k as i32 + step).clamp(0, sizes.len() as i32 - 1) as usize];
            if let Some(cam) = &c.rec.camera {
                cam.set_diameter(next.points());
            }
            c.settings.rec.camera_size = next;
            c.save_settings();
        }
        None => {}
    }
}

/// The bar's settings, just above it.
fn bar_settings(c: &mut Capture, ctx: &egui::Context) {
    let Phase::Live(live) = &c.rec.phase else { return };
    if !live.settings_open {
        return;
    }
    // the system draws click highlights only on newer macOS
    let clicks = super::engine::clicks_supported();
    let size = Vec2::new(300.0, if clicks { 288.0 } else { 254.0 });
    let bar = live.bar_at.unwrap_or_else(|| bar_spot(&live.chosen, BAR_W));
    // the bubble can be hidden and shown again only if the recording
    // started with it (the video keeps that one window)
    let cam_shown = c.rec.camera.as_ref().map(|k| k.visible());
    let mut cam_toggle = false;
    let pos = Pos2::new(bar.x + BAR_W - size.x, bar.y - size.y - 8.0);
    let title = &plain("capture-rec-settings");
    let mut s = c.settings.rec.clone();
    let mut close = false;
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(true), |ui, _| {
        sys::lift(title, None);
        let r = ui.max_rect().shrink(2.0);
        let p = ui.painter().clone();
        p.rect_filled(r, 12.0, CARD());
        p.rect_stroke(r, 12.0, Stroke::new(1.0, white(32)), StrokeKind::Inside);
        let id = ui.scope_id().with("rec-settings");
        let x0 = r.left() + 14.0;
        let x1 = r.right() - 14.0;
        let mut y = r.top() + 12.0;
        p.text(Pos2::new(x0, y + 6.0), Align2::LEFT_CENTER, "Recording", bold(11.0), TEXT_FAINT());
        y += 22.0;
        let row = |p: &egui::Painter, y: f32, label: &str| {
            p.text(Pos2::new(x0, y + 15.0), Align2::LEFT_CENTER, label, font(13.0), TEXT());
        };
        let seg = |ui: &mut Ui, y: f32, w: f32, key: &str, labels: &[&str], cur: usize| segmented(ui, Rect::from_min_size(Pos2::new(x1 - w, y + 3.0), Vec2::new(w, 24.0)), id.with(key), labels, cur);
        row(&p, y, "Frame rate");
        if let Some(i) = seg(ui, y, 100.0, "fps", &["30", "60"], if s.fps == 30 { 0 } else { 1 }) {
            s.fps = crate::tools::capture::settings::RECORD_FPS[i];
        }
        y += 34.0;
        use crate::tools::capture::settings::{CameraSize, RecQuality};
        row(&p, y, "Quality");
        if let Some(i) = seg(ui, y, 130.0, "q", &["Smaller", "High"], if s.quality == RecQuality::Smaller { 0 } else { 1 }) {
            s.quality = if i == 0 { RecQuality::Smaller } else { RecQuality::High };
        }
        y += 34.0;
        row(&p, y, "Camera");
        match cam_shown {
            Some(on) => {
                if small_switch(ui, Rect::from_min_size(Pos2::new(x1 - 36.0, y + 5.0), Vec2::new(36.0, 20.0)), id.with("camera"), on) {
                    cam_toggle = true;
                }
            }
            None => {
                p.text(Pos2::new(x1, y + 15.0), Align2::RIGHT_CENTER, "Turn on before recording", font(12.0), TEXT_FAINT());
            }
        }
        y += 34.0;
        row(&p, y, "Camera size");
        let cur = match s.camera_size {
            CameraSize::S => 0,
            CameraSize::M => 1,
            CameraSize::L => 2,
        };
        if let Some(i) = seg(ui, y, 120.0, "cam", &["S", "M", "L"], cur) {
            s.camera_size = [CameraSize::S, CameraSize::M, CameraSize::L][i];
        }
        y += 34.0;
        row(&p, y, "Countdown");
        let cur = match s.countdown {
            0 => 0,
            5 => 2,
            _ => 1,
        };
        if let Some(i) = seg(ui, y, 130.0, "cd", &["Off", "3 s", "5 s"], cur) {
            s.countdown = [0, 3, 5][i];
        }
        y += 34.0;
        row(&p, y, "Show cursor");
        if small_switch(ui, Rect::from_min_size(Pos2::new(x1 - 36.0, y + 5.0), Vec2::new(36.0, 20.0)), id.with("cursor"), s.cursor) {
            s.cursor = !s.cursor;
        }
        if clicks {
            y += 34.0;
            row(&p, y, "Highlight clicks");
            if small_switch(ui, Rect::from_min_size(Pos2::new(x1 - 36.0, y + 5.0), Vec2::new(36.0, 20.0)), id.with("clicks"), s.clicks) {
                s.clicks = !s.clicks;
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    if cam_toggle {
        if let Some(cam) = &c.rec.camera {
            cam.set_visible(!cam.visible());
        }
    }
    if s != c.settings.rec {
        if s.camera_size != c.settings.rec.camera_size {
            if let Some(cam) = &c.rec.camera {
                cam.set_diameter(s.camera_size.points());
            }
        }
        c.settings.rec = s;
        c.save_settings();
    }
    if close {
        if let Phase::Live(l) = &mut c.rec.phase {
            l.settings_open = false;
        }
    }
}

// ---------------------------------------------------------------- saved

fn saved(c: &mut Capture, ctx: &egui::Context) {
    let Some(sv) = &mut c.rec.saved else { return };
    let size = Vec2::new(300.0, 236.0);
    let pos = Pos2::new(sv.display.right() - size.x - 20.0, sv.display.bottom() - size.y - 20.0);
    let title = &plain("capture-rec-saved");
    let vid = ViewportId::from_hash_of(("capture-rec-saved", sv.key));
    #[derive(PartialEq)]
    enum Do {
        Open,
        Link,
        Reveal,
        Copy,
        Delete,
        Close,
    }
    let mut todo = None;
    let mut hovered = false;
    let uploading = c.uploads.progress(&sv.path);
    let shared = uploading.is_none() && crate::tools::capture::share::link_for(&sv.path).is_some();
    let link_label = match uploading {
        Some(f) => format!("{:.0}%", f * 100.0),
        None if shared => "Copy link".to_string(),
        None => "Share".to_string(),
    };
    crate::kit::viewport::show(ctx, vid, floating(title, pos, size).with_active(false), |ui, _| {
        sys::lift(title, None);
        let vctx = ui.ctx().clone();
        let r = ui.max_rect().shrink(2.0);
        hovered = ui.rect_contains_pointer(r);
        let p = ui.painter().clone();
        p.rect_filled(r, 12.0, CARD());
        p.rect_stroke(r, 12.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let id = ui.scope_id().with("rec-saved");
        let pic = Rect::from_min_size(r.min, Vec2::new(r.width(), 128.0));
        p.rect_filled(pic, egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, white(20));
        if let Some(img) = &sv.thumb {
            let t = tex(&vctx, &mut sv.tex, img);
            super::super::ui::thumb(&p, pic.shrink(1.0), Some(t), img.dimensions(), 11.0);
        }
        let play = Rect::from_center_size(pic.center(), Vec2::splat(52.0));
        let presp = ui.interact(pic, id.with("play"), Sense::click());
        p.circle_filled(play.center(), 26.0, Color32::from_rgba_unmultiplied(28, 28, 31, if presp.hovered() { 230 } else { 200 }));
        icons::draw(&p, Ico::Play, play.shrink(16.0), Color32::WHITE);
        if presp.on_hover_text("Play and edit").clicked() {
            todo = Some(Do::Open);
        }
        let len = timeline::clock_label(sv.secs);
        // over the picture: white on dark in both themes
        let g = p.layout_no_wrap(len, bold(11.0), Color32::WHITE);
        let lr = Rect::from_min_size(pic.right_bottom() - g.size() - Vec2::new(18.0, 14.0), g.size() + Vec2::new(10.0, 6.0));
        p.rect_filled(lr, 4.0, Color32::from_rgba_unmultiplied(28, 28, 31, 220));
        p.galley(lr.min + Vec2::new(5.0, 3.0), g, Color32::WHITE);
        let close = Rect::from_min_size(Pos2::new(pic.right() - 30.0, pic.top() + 8.0), Vec2::splat(22.0));
        if icon_button(ui, close, id.with("close"), Ico::Close, "Close", TEXT(), true) {
            todo = Some(Do::Close);
        }
        let y = pic.bottom() + 14.0;
        icons::draw(&p, Ico::Check, Rect::from_center_size(Pos2::new(r.left() + 18.0, y), Vec2::splat(13.0)), GREEN());
        p.text(Pos2::new(r.left() + 30.0, y), Align2::LEFT_CENTER, format!("Saved · {}", megabytes(sv.bytes)), font(12.0), TEXT());
        p.text(Pos2::new(r.right() - 12.0, y), Align2::RIGHT_CENTER, format!("{}×{}", sv.size.0, sv.size.1), font(12.0), TEXT_FAINT());
        let acts = [(Ico::Scissors, "Edit", Do::Open), (Ico::Link, link_label.as_str(), Do::Link), (Ico::Folder, "Finder", Do::Reveal), (Ico::Copy, "Copy", Do::Copy), (Ico::Trash, "Delete", Do::Delete)];
        let bw = (r.width() - 16.0 - 16.0) / 5.0;
        for (k, (ico, label, act)) in acts.into_iter().enumerate() {
            let br = Rect::from_min_size(Pos2::new(r.left() + 8.0 + k as f32 * (bw + 4.0), y + 14.0), Vec2::new(bw, 44.0));
            let resp = ui.interact(br, id.with(("act", k)), Sense::click());
            p.rect_filled(br, 8.0, white(if resp.hovered() { 26 } else { 15 }));
            let col = if act == Do::Delete { RED() } else { TEXT() };
            icons::draw(&p, ico, Rect::from_center_size(Pos2::new(br.center().x, br.top() + 15.0), Vec2::splat(15.0)), col);
            p.text(Pos2::new(br.center().x, br.top() + 33.0), Align2::CENTER_CENTER, label, font(11.0), col);
            if resp.hovered() {
                vctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                todo = Some(act);
            }
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    let Some(sv) = &mut c.rec.saved else { return };
    if hovered {
        sv.born = Instant::now();
    }
    let path = sv.path.clone();
    let history = sv.history;
    match todo {
        Some(Do::Open) => {
            c.rec.saved = None;
            c.open_video(path);
        }
        Some(Do::Link) => c.copy_link(path),
        Some(Do::Reveal) => sys::reveal(&path),
        Some(Do::Copy) => {
            if sys::copy_file(&path) {
                c.hud("Copied the video, paste it into any chat");
            } else {
                c.hud("Couldn't copy");
            }
        }
        Some(Do::Delete) => {
            c.rec.saved = None;
            match crate::kit::native::trash_item(&path) {
                Ok(()) => {
                    if let Some(id) = history {
                        c.store.remove(id);
                        c.forget_dropped_thumbs();
                    }
                    c.files_at = None;
                    c.hud("Moved to Trash");
                }
                Err(e) => c.hud(format!("Couldn't move it to the Trash: {e}")),
            }
        }
        Some(Do::Close) => c.rec.saved = None,
        None => {}
    }
}

pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::capture::record::sys::Display;

    fn chosen(target: super::super::engine::Target, rect: Rect) -> Chosen {
        let display = Display { id: 1, name: String::new(), frame: Rect::from_min_size(Pos2::ZERO, Vec2::new(1512.0, 982.0)), scale: 2.0, fps: 120 };
        Chosen { target, display, rect, size: (100, 100), fps: 60 }
    }

    #[test]
    fn the_bar_sits_under_an_area_or_inside_it_when_there_is_no_room() {
        use super::super::engine::Target;
        let a = Rect::from_min_size(Pos2::new(200.0, 100.0), Vec2::new(600.0, 400.0));
        let at = bar_spot(&chosen(Target::Area { display: 1, rect: [0.0; 4] }, a), BAR_W);
        assert_eq!(at, Pos2::new(500.0 - BAR_W / 2.0, 514.0));
        let low = Rect::from_min_size(Pos2::new(200.0, 500.0), Vec2::new(600.0, 470.0));
        let at = bar_spot(&chosen(Target::Area { display: 1, rect: [0.0; 4] }, low), BAR_W);
        assert!(at.y + BAR_H <= low.bottom());
        let d = chosen(Target::Display(1), Rect::from_min_size(Pos2::ZERO, Vec2::new(1512.0, 982.0)));
        assert_eq!(bar_spot(&d, BAR_W).x, 756.0 - BAR_W / 2.0);
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(megabytes(2_400_000), "2.4 MB");
        assert_eq!(megabytes(24_000_000), "24 MB");
        assert_eq!(megabytes(2_400_000_000), "2.4 GB");
    }
}
