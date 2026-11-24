//! Capture's own floating windows: the preview after a capture, pinned
//! captures, the editor, the countdown, short messages, the Screen Recording
//! card and the full-screen color picker / ruler. All are egui viewports
//! titled "plain-…" so the shell puts no glass behind them.

use super::editor::{self, Editor, Request};
use super::grab::Grab;
use super::icons::{self, Ico};
use super::{mac, render, Capture, Measure, Purpose};
use crate::core::tool::Cx;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, ViewportBuilder, ViewportCommand, ViewportId};
use image::RgbaImage;
use std::time::Instant;

pub struct Countdown {
    pub until: Instant,
    pub total: u32,
    pub grab: Grab,
    pub purpose: Purpose,
}

pub struct Preview {
    id: Option<u64>,
    img: RgbaImage,
    /// The picture at the preview's size, for its texture (a full-size
    /// texture of a 5K capture is ~60 MB to convert and upload).
    small: Option<egui::ColorImage>,
    note: String,
    /// Names its window; `born` moves while the mouse is over it.
    key: Instant,
    born: Instant,
    tex: Option<egui::TextureHandle>,
    frames: u32,
}

impl Preview {
    pub fn new(id: Option<u64>, img: RgbaImage, small: Option<egui::ColorImage>, note: String) -> Preview {
        Preview { id, img, small, note, key: Instant::now(), born: Instant::now(), tex: None, frames: 0 }
    }
}

pub struct Pin {
    n: u64,
    id: Option<u64>,
    img: RgbaImage,
    tex: Option<egui::TextureHandle>,
    /// Size in points at scale 1 (true size).
    base: Vec2,
    scale: f32,
    offset: f32,
    /// Where and how big it opened; later moves and resizes are the window's own.
    start: Option<(Pos2, Vec2)>,
    frames: u32,
    /// A resize drag in progress: which handle, the scale and pointer at its start.
    resize: Option<(Grip, f32, Pos2)>,
    /// Closed while it had the keyboard: drawn empty until the keyboard is
    /// back with the app the user came from, then removed.
    closing: Option<Instant>,
}

/// Where a pin is being resized from. The top-left corner stays put, so a
/// resize only changes the window's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Right,
    Bottom,
    Corner,
}

pub const PIN_MIN_SCALE: f32 = 0.1;
pub const PIN_MAX_SCALE: f32 = 4.0;

/// The pin scale after dragging `grip` by `delta` points, keeping the
/// picture's aspect ratio. `base` is the true size, `start` the scale when
/// the drag began. The corner follows whichever side moved further.
pub fn drag_scale(base: Vec2, start: f32, grip: Grip, delta: Vec2) -> f32 {
    let w = (base.x * start + delta.x) / base.x.max(1.0);
    let h = (base.y * start + delta.y) / base.y.max(1.0);
    let s = match grip {
        Grip::Right => w,
        Grip::Bottom => h,
        Grip::Corner => if (w - start).abs() >= (h - start).abs() { w } else { h },
    };
    s.clamp(PIN_MIN_SCALE, PIN_MAX_SCALE)
}

impl Pin {
    pub fn new(n: u64, id: Option<u64>, img: RgbaImage, ppp: f32, offset: f32) -> Pin {
        let base = Vec2::new(img.width() as f32, img.height() as f32) / ppp.max(1.0);
        Pin { n, id, img, tex: None, base, scale: 1.0, offset, start: None, frames: 0, resize: None, closing: None }
    }
}

pub struct EditorWin {
    pub ed: Editor,
    opened: bool,
    /// The file the Share button is uploading.
    pub share: Option<std::path::PathBuf>,
    /// The editor's revision when its link was last made or copied.
    shared_at: Option<usize>,
}

impl EditorWin {
    pub fn new(ed: Editor) -> EditorWin {
        EditorWin { ed, opened: false, share: None, shared_at: None }
    }
}

pub struct Overlay {
    mode: Measure,
    img: RgbaImage,
    tex: Option<egui::TextureHandle>,
    drag: Option<(Pos2, Pos2)>,
    frames: u32,
}

impl Overlay {
    pub fn new(mode: Measure, img: RgbaImage) -> Overlay {
        Overlay { mode, img, tex: None, drag: None, frames: 0 }
    }
}

fn texture(ctx: &egui::Context, slot: &mut Option<egui::TextureHandle>, name: &str, img: &RgbaImage) -> egui::TextureId {
    if slot.is_none() {
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        *slot = Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR));
    }
    slot.as_ref().unwrap().id()
}

/// A window title that keeps the shell's glass away.
pub(super) fn plain(name: &str) -> String {
    format!("{}{name}", crate::kit::native::PLAIN_WINDOW)
}

pub(super) fn floating(title: &str, pos: Pos2, size: Vec2) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(true)
        .with_position(pos)
        .with_inner_size(size)
}

fn screen_size(ctx: &egui::Context) -> Vec2 {
    ctx.input(|i| i.viewport().monitor_size).filter(|s| s.x > 100.0).unwrap_or(Vec2::new(1512.0, 982.0))
}

/// Everything Capture shows outside the shell, once per ui pass.
pub fn show(c: &mut Capture, ctx: &egui::Context, cx: &mut Cx) {
    let screen = screen_size(ctx);
    countdown(c, ctx, screen);
    permission(c, ctx, screen, cx);
    keychain(c, ctx, screen, cx);
    preview(c, ctx, screen);
    pins(c, ctx, screen);
    editor_window(c, ctx, screen);
    overlay(c, ctx, screen);
    hud(c, ctx, screen);
}

// ---------------------------------------------------------------- countdown

fn countdown(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    let Some(cd) = &c.countdown else { return };
    let left = cd.until.saturating_duration_since(Instant::now()).as_secs_f32();
    let total = cd.total as f32;
    let what = match (cd.grab, cd.purpose) {
        (Grab::Area, _) => ts!("cap.cd.area"),
        (Grab::Window, _) => ts!("cap.cd.window"),
        (Grab::Screen, _) => ts!("cap.cd.screen"),
    };
    let size = Vec2::new(200.0, 200.0);
    let pos = Pos2::new((screen.x - size.x) / 2.0, (screen.y - size.y) / 2.0);
    let title = &plain("capture-countdown");
    let mut cancel = false;
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(false), |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter();
        let card = Rect::from_center_size(r.center(), Vec2::splat(180.0));
        p.rect_filled(card, 24.0, Color32::from_rgba_unmultiplied(28, 28, 31, 235));
        p.rect_stroke(card, 24.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let c0 = card.center() - Vec2::new(0.0, 16.0);
        crate::kit::glass::stroke_circle(&p, c0, 44.0, Stroke::new(4.0, white(30)));
        let frac = (left / total.max(0.1)).clamp(0.0, 1.0);
        let a0 = -std::f32::consts::FRAC_PI_2;
        p.add(egui::epaint::PathShape::line(arc_points(c0, 44.0, a0, a0 + frac * std::f32::consts::TAU, 64), egui::epaint::PathStroke::new(4.0, tint::CAPTURE())));
        p.text(c0, Align2::CENTER_CENTER, format!("{}", left.ceil().max(1.0) as u32), bold(40.0), TEXT());
        p.text(Pos2::new(card.center().x, card.bottom() - 40.0), Align2::CENTER_CENTER, what, font(12.0), TEXT_SOFT());
        p.text(Pos2::new(card.center().x, card.bottom() - 22.0), Align2::CENTER_CENTER, ts!("cap.click_cancel"), font(11.0), TEXT_FAINT());
        if ui.interact(card, ui.scope_id().with("cancel"), Sense::click()).clicked() {
            cancel = true;
        }
    });
    if cancel {
        c.countdown = None;
    }
}

// ---------------------------------------------------------------- permission

/// The Keychain card: before macOS asks for the sharing key, what it is and
/// what is allowed. A floating card like the Screen Recording one, so it
/// shows wherever Share was pressed (editor, preview, the app window).
/// Hidden while macOS's own dialog is up.
fn keychain(c: &mut Capture, ctx: &egui::Context, screen: Vec2, cx: &mut Cx) {
    let Some(k) = c.keychain.as_ref() else { return };
    if k.busy {
        return;
    }
    let denied = k.denied;
    let app = crate::kit::perms::app_name();
    let size = Vec2::new(440.0, 252.0);
    let pos = Pos2::new((screen.x - size.x) / 2.0, screen.y * 0.3);
    let title = &plain("capture-keychain");
    #[derive(Clone, Copy)]
    enum Choice {
        Cancel,
        Go,
        NewKey,
    }
    let mut choice = None;
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(true), |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(r, 14.0, POPOVER());
        p.rect_stroke(r, 14.0, Stroke::new(1.0, POPOVER_EDGE()), StrokeKind::Inside);
        let x = r.left() + 16.0;
        let w = r.width() - 32.0;
        let chip = Rect::from_min_size(Pos2::new(x, r.top() + 20.0), Vec2::splat(42.0));
        tool_chip(&p, chip, Icon::Lock, tint::NEUTRAL());
        p.text(Pos2::new(chip.right() + 12.0, chip.top() + 3.0), Align2::LEFT_TOP, ts!("cap.kc.title"), bold(15.0), TEXT());
        p.text(Pos2::new(chip.right() + 12.0, chip.bottom() - 3.0), Align2::LEFT_BOTTOM, t!("cap.kc.sub", app = app), font(13.0), TEXT_DIM());
        let body = t!("cap.kc.body", app = app);
        let mut y = chip.bottom() + 14.0;
        y += paragraph(&p, Pos2::new(x, y), &body, font(13.0), TEXT_SOFT(), w) + 12.0;
        hairline(&p, Pos2::new(r.left(), y), Pos2::new(r.right(), y));
        for (k, (glyph, line, col)) in [(Icon::Check, ts!("cap.kc.only"), GREEN()), (Icon::Close, ts!("cap.kc.none"), TEXT_FAINT())].into_iter().enumerate() {
            let cy = y + 16.0 + k as f32 * 32.0;
            icon(&p, glyph, Rect::from_center_size(Pos2::new(x + 8.0, cy), Vec2::splat(14.0)), col);
            p.text(Pos2::new(x + 26.0, cy), Align2::LEFT_CENTER, line, font(13.0), if k == 0 { TEXT() } else { TEXT_DIM() });
        }
        y += 64.0;
        hairline(&p, Pos2::new(r.left(), y), Pos2::new(r.right(), y));
        let by = r.bottom() - 16.0 - 32.0;
        let (left, back, go) = if denied { (ts!("cap.kc.denied"), ts!("cap.kc.enter_again"), ts!("cap.try_again")) } else { (ts!("cap.kc.always"), ts!("cap.not_now"), ts!("cap.continue")) };
        let gw = button_width(&p, go, BTN_PRIMARY);
        let gr = Rect::from_min_size(Pos2::new(r.right() - 16.0 - gw, by), Vec2::new(gw, 32.0));
        let bw = button_width(&p, back, BTN_SECONDARY);
        let br = Rect::from_min_size(Pos2::new(gr.left() - 8.0 - bw, by), Vec2::new(bw, 32.0));
        p.text(Pos2::new(x, br.center().y), Align2::LEFT_CENTER, left, font(12.0), if denied { AMBER() } else { TEXT_FAINT() });
        if pill_button(ui, br, back, BTN_SECONDARY, true).clicked() {
            choice = Some(if denied { Choice::NewKey } else { Choice::Cancel });
        }
        if pill_button(ui, gr, go, BTN_PRIMARY, true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            choice = Some(Choice::Go);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || ui.ctx().input(|i| i.viewport().close_requested()) {
            choice = Some(Choice::Cancel);
        }
    });
    match choice {
        Some(Choice::Go) => c.keychain_continue(),
        Some(Choice::Cancel) => c.keychain_drop(),
        Some(Choice::NewKey) => {
            c.keychain_drop();
            c.focus_secret = true;
            cx.open_page(super::SHARING_ID, "");
        }
        None => {}
    }
}

fn permission(c: &mut Capture, ctx: &egui::Context, screen: Vec2, cx: &mut Cx) {
    if c.perm_card.is_none() {
        return;
    }
    // allowed in System Settings, but macOS only applies it to a new process
    let allowed = crate::kit::perms::Perm::Screen.status() == crate::kit::perms::Status::Allowed;
    // a restart already happened and capture still can't see the screen:
    // another one won't help, the switch in Settings has to be flipped again
    let restart = allowed && !crate::kit::perms::relaunched();
    let (title_text, body, action) = if restart {
        (ts!("cap.perm.restart_title").to_string(), ts!("cap.perm.restart_body").to_string(), ts!("cap.perm.restart"))
    } else if allowed {
        (ts!("cap.perm.off_title").to_string(), t!("cap.perm.off_body", app = crate::kit::perms::app_name()).into_owned(), ts!("cap.perm.open"))
    } else {
        (ts!("cap.needs_screen").to_string(), t!("cap.perm.on_body", app = crate::kit::perms::app_name()).into_owned(), ts!("cap.perm.open"))
    };
    let size = Vec2::new(400.0, 152.0);
    let pos = Pos2::new((screen.x - size.x) / 2.0, screen.y * 0.3);
    let title = &plain("capture-permission");
    let mut choice = None;
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), floating(title, pos, size).with_active(true), |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        // a permission card (docs/ui-rules.md P9)
        p.rect_filled(r, 14.0, POPOVER());
        p.rect_stroke(r, 14.0, Stroke::new(1.0, POPOVER_EDGE()), StrokeKind::Inside);
        tool_chip(&p, Rect::from_min_size(r.min + Vec2::new(16.0, 16.0), Vec2::splat(42.0)), Icon::Crop, tint::CAPTURE());
        p.text(r.min + Vec2::new(70.0, 18.0), Align2::LEFT_TOP, &title_text, bold(15.0), TEXT());
        paragraph(&p, r.min + Vec2::new(70.0, 42.0), &body, font(13.0), TEXT_DIM(), r.width() - 86.0);
        let y = r.bottom() - 16.0 - 32.0;
        let ow = button_width(&p, action, BTN_PRIMARY);
        let or = Rect::from_min_size(Pos2::new(r.right() - 16.0 - ow, y), Vec2::new(ow, 32.0));
        let nw = button_width(&p, ts!("cap.not_now"), BTN_SECONDARY);
        let nr = Rect::from_min_size(Pos2::new(or.left() - 8.0 - nw, y), Vec2::new(nw, 32.0));
        if pill_button(ui, nr, ts!("cap.not_now"), BTN_SECONDARY, true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(false);
        }
        if pill_button(ui, or, action, BTN_PRIMARY, true).clicked() {
            choice = Some(true);
        }
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            choice = Some(false);
        }
    });
    match choice {
        Some(true) if restart => {
            c.perm_card = None;
            crate::kit::perms::relaunch();
            cx.push(crate::core::tool::Action::Quit);
        }
        Some(true) => {
            mac::ask_to_record_screen();
            mac::open_privacy_settings();
            c.perm_card = None;
        }
        Some(false) => {
            c.perm_card = None;
            c.perm_skip = true;
        }
        None => {}
    }
}

// ---------------------------------------------------------------- preview

#[derive(Clone, Copy, PartialEq)]
enum PreviewAct {
    Copy,
    Save,
    Edit,
    Pin,
    Text,
    Close,
}

fn preview(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    let Some(pv) = &mut c.preview else { return };
    let w = 264.0;
    let (iw, ih) = pv.img.dimensions();
    let th = ((w - 16.0) * ih as f32 / iw.max(1) as f32).clamp(70.0, 170.0);
    let size = Vec2::new(w, 8.0 + 26.0 + th + 8.0 + 40.0 + 8.0);
    let pos = Pos2::new(20.0, screen.y - size.y - 20.0);
    let title = &plain("capture-preview");
    let vid = ViewportId::from_hash_of(("capture-preview", pv.key));
    let hide_after = c.settings.preview_secs;
    let mut act = None;
    // the native window may appear a frame late: lift it for a few frames
    let lift = pv.frames < 5;
    let shown = crate::kit::viewport::show(ctx, vid, floating(title, pos, size).with_active(false), |ui, _| {
        if lift {
            mac::float_window(title, false);
        }
        let vctx = ui.ctx().clone();
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(r, 14.0, Color32::from_rgba_unmultiplied(28, 28, 31, 245));
        p.rect_stroke(r, 14.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let hovered = ui.rect_contains_pointer(r);
        // header
        let hy = r.top() + 8.0 + 13.0;
        icons::draw(&p, Ico::Check, Rect::from_center_size(Pos2::new(r.left() + 18.0, hy), Vec2::splat(13.0)), GREEN());
        p.text(Pos2::new(r.left() + 30.0, hy), Align2::LEFT_CENTER, &pv.note, bold(12.0), TEXT());
        if hovered {
            let cr = Rect::from_center_size(Pos2::new(r.right() - 18.0, hy), Vec2::splat(20.0));
            let b = ui.interact(cr, ui.scope_id().with("close"), Sense::click());
            p.circle_filled(cr.center(), 10.0, white(if b.hovered() { 40 } else { 22 }));
            icons::draw(&p, Ico::Close, cr.shrink(5.0), TEXT_SOFT());
            if b.on_hover_text(ts!("cap.close")).clicked() {
                act = Some(PreviewAct::Close);
            }
        } else {
            p.text(Pos2::new(r.right() - 12.0, hy), Align2::RIGHT_CENTER, format!("{iw}×{ih}"), font(11.0), TEXT_FAINT());
        }
        // picture: drag moves the preview, click opens the editor
        let tr = Rect::from_min_size(Pos2::new(r.left() + 8.0, r.top() + 34.0), Vec2::new(w - 16.0, th));
        if pv.tex.is_none() {
            let ci = pv.small.take().unwrap_or_else(|| super::scaled_image(&pv.img, super::PREVIEW_PX));
            pv.tex = Some(vctx.load_texture("capture-preview", ci, egui::TextureOptions::LINEAR));
        }
        let tex = pv.tex.as_ref().map(|t| t.id()).unwrap();
        super::ui::thumb(&p, tr, Some(tex), (iw, ih), 8.0);
        let tresp = ui.interact(tr, ui.scope_id().with("pic"), Sense::click_and_drag());
        p.rect_stroke(tr, 8.0, Stroke::new(1.0, if tresp.hovered() { ACCENT() } else { white(26) }), StrokeKind::Inside);
        if tresp.drag_started() {
            vctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if tresp.on_hover_text(ts!("cap.click_to_edit")).clicked() {
            act = Some(PreviewAct::Edit);
        }
        // actions
        let items = [(Ico::Copy, ts!("cap.copy"), PreviewAct::Copy), (Ico::Save, ts!("cap.save"), PreviewAct::Save), (Ico::Edit, ts!("cap.edit"), PreviewAct::Edit), (Ico::Pin, ts!("cap.pin"), PreviewAct::Pin), (Ico::ScanText, ts!("cap.text"), PreviewAct::Text)];
        let ay = tr.bottom() + 8.0;
        let bw = (w - 16.0 - 4.0 * 4.0) / 5.0;
        for (k, (ico, label, a)) in items.iter().enumerate() {
            let br = Rect::from_min_size(Pos2::new(r.left() + 8.0 + k as f32 * (bw + 4.0), ay), Vec2::new(bw, 40.0));
            let b = ui.interact(br, ui.scope_id().with(("act", k)), Sense::click());
            p.rect_filled(br, 8.0, white(if b.hovered() { 34 } else { 16 }));
            icons::draw(&p, *ico, Rect::from_center_size(Pos2::new(br.center().x, br.top() + 14.0), Vec2::splat(15.0)), TEXT());
            p.text(Pos2::new(br.center().x, br.top() + 31.0), Align2::CENTER_CENTER, *label, font(11.0), TEXT_SOFT());
            if b.clicked() {
                act = Some(*a);
            }
        }
        if hovered {
            pv.born = Instant::now();
        }
    });
    if shown.is_some() {
        pv.frames += 1;
    }
    let expired = hide_after.is_some_and(|s| pv.born.elapsed().as_secs_f32() > s as f32);
    if act.is_none() && !expired {
        return;
    }
    // the preview closes on any action: take its picture instead of copying it
    let Some(Preview { id, img, .. }) = c.preview.take() else { return };
    match act {
        Some(PreviewAct::Copy) => match id.and_then(|id| c.store.png(id)) {
            Some(png) => {
                c.copy_png(png);
                c.hud(ts!("cap.copied"));
            }
            None => {
                c.copy_image(img);
                c.hud(ts!("cap.copied"));
            }
        },
        Some(PreviewAct::Save) => c.save_image_later(id, img),
        Some(PreviewAct::Edit) => c.open_editor(id, img),
        Some(PreviewAct::Pin) => c.pin(id, img),
        Some(PreviewAct::Text) => c.spawn_ocr(img),
        Some(PreviewAct::Close) | None => {}
    }
}

pub fn short_path(p: &std::path::Path) -> String {
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let s = p.display().to_string();
    if !home.is_empty() && s.starts_with(&home) {
        s.replacen(&home, "~", 1)
    } else {
        s
    }
}

// ---------------------------------------------------------------- pins

/// A small capture window (selection overlay, countdown, preview, pin, HUD,
/// recorder bits) that doesn't count as the user picking our app: using it
/// takes the keyboard, closing a pin should give it back. The editor and
/// the video viewer are real windows.
fn passing_window(title: &str) -> bool {
    let Some(name) = title.strip_prefix(crate::kit::native::PLAIN_WINDOW) else { return false };
    name.starts_with("capture-") && !["capture-editor", "capture-video"].contains(&name)
}

/// Who gets the keyboard when a pin closes, updated once per pass: the app
/// in front while we're in the background; kept while one of our passing
/// windows (or none) has the keyboard; dropped once a real window of ours
/// (the app, the editor) has it, since closing a pin then returns there.
pub fn track_return(prev: Option<i32>, active: bool, front_other: Option<i32>, key: Option<&str>) -> Option<i32> {
    if !active {
        return front_other.or(prev);
    }
    match key {
        Some(t) if !passing_window(t) => None,
        _ => prev,
    }
}

/// How long a closed pin waits for the other app to take the keyboard.
const PIN_HAND_BACK: std::time::Duration = std::time::Duration::from_millis(500);

fn pins(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    use crate::kit::native;
    let active = native::app_active();
    // tracked on every pass: a fresh screenshot goes overlay → preview → pin
    // without our app ever leaving the front
    c.pin_return = track_return(c.pin_return, active, native::front_other_app(), native::key_window_title().as_deref());
    let mut close = Vec::new();
    let mut edit = Vec::new();
    for (k, pin) in c.pins.iter_mut().enumerate() {
        let (pos, size) = *pin.start.get_or_insert_with(|| {
            // open at most 60% of the screen, centered
            pin.scale = (screen.x * 0.6 / pin.base.x).min(screen.y * 0.6 / pin.base.y).min(1.0);
            let size = pin.base * pin.scale;
            (Pos2::new((screen.x - size.x) / 2.0 + pin.offset, (screen.y - size.y) / 2.0 + pin.offset), size)
        });
        let title = plain(&format!("capture-pin-{}", pin.n));
        let lift = pin.frames < 5;
        let vid = ViewportId::from_hash_of(("capture-pin", pin.n));
        if let Some(at) = pin.closing {
            // closing the window that has the keyboard makes macOS hand it to
            // our next window, the app window, and bring that up (from behind
            // other apps or another Space). So the pin stays, empty and
            // click-through, until the app the user came from has it
            if !active || at.elapsed() > PIN_HAND_BACK {
                close.push(k);
                continue;
            }
            let builder = floating(&title, pos, size).with_has_shadow(false).with_mouse_passthrough(true);
            crate::kit::viewport::show(ctx, vid, builder, |ui, _| {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            });
            continue;
        }
        let builder = floating(&title, pos, size).with_has_shadow(true);
        let shown = crate::kit::viewport::show(ctx, vid, builder, |ui, _| {
            let vctx = ui.ctx().clone();
            if lift {
                mac::float_window(&title, false);
            }
            let r = ui.max_rect();
            let p = ui.painter().clone();
            let tex = texture(&vctx, &mut pin.tex, &title, &pin.img);
            p.image(tex, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let resp = ui.interact(r, ui.scope_id().with("pin"), Sense::click_and_drag());
            let hovered = ui.rect_contains_pointer(r);
            // resize handles (added after the body, so they win where they
            // overlap); the grab zone reaches 12 pt in from the edge (C16)
            let band = crate::kit::glass::overlay::GRAB;
            let corner = 2.0 * band;
            let handles = [
                (Grip::Corner, Rect::from_min_max(r.max - Vec2::splat(corner), r.max), egui::CursorIcon::ResizeNwSe),
                (Grip::Right, Rect::from_min_max(Pos2::new(r.right() - band, r.top() + 36.0), Pos2::new(r.right(), r.bottom() - corner)), egui::CursorIcon::ResizeHorizontal),
                (Grip::Bottom, Rect::from_min_max(Pos2::new(r.left(), r.bottom() - band), Pos2::new(r.right() - corner, r.bottom())), egui::CursorIcon::ResizeVertical),
            ];
            let mut resized = None;
            for (grip, hr, cursor) in handles {
                let h = ui.interact(hr, ui.scope_id().with(("pin-grip", grip as u8)), Sense::drag());
                if h.hovered() || h.dragged() {
                    vctx.set_cursor_icon(cursor);
                }
                if h.drag_started() {
                    if let Some(at) = h.interact_pointer_pos() {
                        pin.resize = Some((grip, pin.scale, at));
                    }
                }
                if h.dragged() {
                    if let (Some((g, s0, at)), Some(now)) = (pin.resize, h.interact_pointer_pos()) {
                        if g == grip {
                            resized = Some(drag_scale(pin.base, s0, g, now - at));
                        }
                    }
                }
                if h.drag_stopped() {
                    pin.resize = None;
                }
            }
            // the edge on hover is an on-picture overlay: white over a dark band (C16)
            if hovered || pin.resize.is_some() {
                crate::kit::glass::overlay::frame(&p, r, 0.0, StrokeKind::Inside);
            } else {
                p.rect_stroke(r, 0.0, Stroke::new(1.0, white(40)), StrokeKind::Inside);
            }
            if resp.drag_started() && pin.resize.is_none() {
                vctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                edit.push(k);
            }
            if hovered || pin.resize.is_some() {
                // scroll wheel or trackpad pinch zooms; edge/corner drags resize
                let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                let mut scale = resized.unwrap_or(pin.scale);
                if scroll.abs() > 0.1 {
                    scale *= 1.0 + scroll * 0.003;
                }
                if (pinch - 1.0).abs() > 1e-4 {
                    scale *= pinch;
                }
                let scale = scale.clamp(PIN_MIN_SCALE, PIN_MAX_SCALE);
                if (scale - pin.scale).abs() > 1e-4 {
                    pin.scale = scale;
                    vctx.send_viewport_cmd(ViewportCommand::InnerSize(pin.base * pin.scale));
                }
                // corner grip: white over a dark band, like every on-picture line (C16)
                let c = r.max - Vec2::splat(5.0);
                for k in 1..=3 {
                    let d = 4.0 * k as f32;
                    let seg = [c - Vec2::new(d, 0.0), c - Vec2::new(0.0, d)];
                    p.line_segment(seg, Stroke::new(3.0, crate::kit::glass::overlay::ring(0.5)));
                    p.line_segment(seg, Stroke::new(1.5, crate::kit::glass::overlay::LINE));
                }
                let cr = Rect::from_center_size(Pos2::new(r.right() - 16.0, r.top() + 16.0), Vec2::splat(22.0));
                let b = ui.interact(cr, ui.scope_id().with("close"), Sense::click());
                p.circle_filled(cr.center(), 11.0, Color32::from_black_alpha(if b.hovered() { 220 } else { 160 }));
                icons::draw(&p, Ico::Close, cr.shrink(6.0), Color32::WHITE);
                if b.on_hover_text(ts!("cap.unpin")).clicked() {
                    close.push(k);
                }
                let label = format!("{:.0}%", pin.scale * 100.0);
                let g = p.layout_no_wrap(label, font(11.0), Color32::WHITE);
                let tr = Rect::from_min_size(r.left_top() + Vec2::new(8.0, 8.0), g.size() + Vec2::new(12.0, 6.0));
                p.rect_filled(tr, 6.0, Color32::from_black_alpha(160));
                p.galley(tr.min + Vec2::new(6.0, 3.0), g, Color32::WHITE);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) || vctx.input(|i| i.viewport().close_requested()) {
                close.push(k);
            }
        });
        if shown.is_some() {
            pin.frames += 1;
        }
    }
    for k in edit {
        if let Some(pin) = c.pins.get(k) {
            let (id, img) = (pin.id, pin.img.clone());
            c.open_editor(id, img);
        }
    }
    close.sort();
    close.dedup();
    // a pin closed while it has the keyboard, after a click from another app:
    // that app gets the keyboard back first (see `closing`)
    let key = native::key_window_title();
    if let Some(pid) = c.pin_return.filter(|_| active) {
        let mut handed = false;
        close.retain(|&k| {
            let pin = &mut c.pins[k];
            if pin.closing.is_some() || key.as_deref().is_some_and(|t| t != plain(&format!("capture-pin-{}", pin.n))) {
                return true;
            }
            handed = handed || native::give_focus_to(pid);
            if !handed {
                return true;
            }
            pin.closing = Some(Instant::now());
            ctx.request_repaint();
            false
        });
        if handed {
            c.pin_return = None;
        }
    }
    for k in close.into_iter().rev() {
        let pin = c.pins.remove(k);
        if let Some(id) = pin.id {
            if !c.pins.iter().any(|p| p.id == Some(id)) {
                c.store.set_pinned(id, false);
            }
        }
    }
}

// ---------------------------------------------------------------- editor

fn editor_window(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    let Some(mut ew) = c.editor.take() else { return };
    let (iw, ih) = ew.ed.image_size();
    let chrome = Vec2::new(48.0, 42.0 + 46.0 + 36.0 + 32.0 + 48.0);
    let want = Vec2::new(iw as f32, ih as f32) / c.ppp.max(1.0) + chrome;
    let size = Vec2::new(want.x.clamp(860.0, screen.x * 0.92), want.y.clamp(520.0, screen.y * 0.9));
    let pos = Pos2::new((screen.x - size.x) / 2.0, ((screen.y - size.y) / 2.0).max(30.0));
    let title = &plain("capture-editor");
    let first = !ew.opened;
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_position(pos)
        .with_min_inner_size([super::editor::EDITOR_MIN_WIDTH, 460.0])
        .with_transparent(true)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_has_shadow(true)
        .with_active(true);
    let mut closed = false;
    ew.ed.share_progress = ew.share.as_ref().and_then(|p| c.uploads.progress(p));
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of("capture-editor"), builder, |ui, _| {
        let vctx = ui.ctx().clone();
        if first {
            ew.opened = true;
            // a new picture reuses the editor's window; if it was minimized
            // it stayed in the Dock and the capture looked lost (2026-11-18)
            vctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            vctx.send_viewport_cmd(ViewportCommand::Focus);
            crate::kit::native::activate();
        }
        if vctx.input(|i| i.viewport().close_requested()) {
            closed = true;
            return;
        }
        let full = ui.max_rect();
        ew.ed.ui(ui, full, false);
    });
    for r in std::mem::take(&mut ew.ed.requests) {
        match r {
            // Copy and Save finish the job: the editor closes and a toast confirms
            // encoding and writing happen on a thread; a failure shows as a toast
            Request::Copy => {
                c.copy_image(ew.ed.rendered());
                c.hud(ts!("cap.copied_clip"));
                closed = true;
            }
            Request::Save => {
                let img = ew.ed.rendered();
                let id = c.commit_rendered(&mut ew.ed, &img);
                c.save_image_later(id, img);
                closed = true;
            }
            Request::Pin => {
                let img = ew.ed.rendered();
                let id = c.commit_rendered(&mut ew.ed, &img);
                c.pin(id, img);
                ew.ed.flash(ts!("cap.pinned_on_screen"));
            }
            Request::Share => {
                if ew.share.is_some() {
                    continue;
                }
                // the history copy (with the marks) goes up; the editor stays
                let img = ew.ed.rendered();
                let path = match c.commit_rendered(&mut ew.ed, &img) {
                    Some(id) => Some(c.store.path(id)),
                    None => c.save_image(None, &img),
                };
                let Some(path) = path else { continue };
                let rev = ew.ed.revision();
                // a link that still shows these marks: just copy it
                let fresh = ew.shared_at == Some(rev) || (ew.shared_at.is_none() && !ew.ed.edited);
                match super::share::link_for(&path).filter(|_| fresh) {
                    Some(link) => {
                        c.copy_known_link(&link);
                        ew.ed.flash(ts!("cap.link_copied"));
                    }
                    None => match c.try_share(path.clone()) {
                        Ok(()) => ew.share = Some(path),
                        Err(e) => ew.ed.flash(e),
                    },
                }
                ew.shared_at = Some(rev);
            }
            Request::CopyText(t) => c.copy_text(&t),
            Request::CopyPart(img) => c.copy_image(img),
            Request::Close => closed = true,
        }
    }
    if closed {
        c.commit_edit(&mut ew.ed);
        let s = &mut c.settings;
        if (s.blur, s.cover, s.keep_breaks, s.frame) != (ew.ed.blur, ew.ed.cover, ew.ed.keep_breaks, ew.ed.frame_style) {
            s.blur = ew.ed.blur;
            s.cover = ew.ed.cover;
            s.keep_breaks = ew.ed.keep_breaks;
            s.frame = ew.ed.frame_style;
            c.save_settings();
        }
    } else {
        c.editor = Some(ew);
    }
}

// ---------------------------------------------------------------- picker / ruler

fn overlay(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    let Some(ov) = &mut c.overlay else { return };
    let title = &plain("capture-overlay");
    let first = ov.frames == 0;
    let lift = ov.frames < 5;
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_position(Pos2::ZERO)
        .with_inner_size(screen)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_active(true);
    let mut done: Option<Option<String>> = None;
    let shown = crate::kit::viewport::show(ctx, ViewportId::from_hash_of(title), builder, |ui, _| {
        let vctx = ui.ctx().clone();
        if lift {
            mac::float_window(title, true);
        }
        if first {
            vctx.send_viewport_cmd(ViewportCommand::Focus);
            crate::kit::native::activate();
        }
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let tex = texture(&vctx, &mut ov.tex, "capture-overlay", &ov.img);
        p.image(tex, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // image pixels per point on this screen
        let k = ov.img.width() as f32 / r.width().max(1.0);
        let to_img = |s: Pos2| Pos2::new((s.x - r.left()) * k, (s.y - r.top()) * k);
        let to_screen = |q: Pos2| Pos2::new(r.left() + q.x / k, r.top() + q.y / k);
        let resp = ui.interact(r, ui.scope_id().with("overlay"), Sense::click_and_drag());
        vctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let hint = match ov.mode {
            Measure::Color => ts!("cap.hint.color"),
            Measure::Ruler => ts!("cap.hint.ruler"),
        };
        let g = p.layout_no_wrap(hint.to_string(), font(12.0), TEXT());
        let hr = Rect::from_center_size(Pos2::new(r.center().x, r.top() + 44.0), g.size() + Vec2::new(28.0, 14.0));
        p.rect_filled(hr, hr.height() / 2.0, Color32::from_rgba_unmultiplied(28, 28, 31, 235));
        p.galley(hr.center() - g.size() / 2.0, g, TEXT());
        let hover = resp.hover_pos();
        match ov.mode {
            Measure::Color => {
                if let Some(h) = hover {
                    let q = to_img(h);
                    if let Some(px) = render::pixel(&ov.img, q.x as i64, q.y as i64) {
                        editor::loupe(&p, &ov.img, q, h, px);
                        if resp.clicked() {
                            done = Some(Some(render::hex(px)));
                        }
                    }
                }
            }
            Measure::Ruler => {
                let st = Stroke::new(1.5, Color32::from_rgb(255, 214, 10));
                if resp.drag_started() {
                    if let Some(s) = resp.interact_pointer_pos() {
                        ov.drag = Some((s, s));
                    }
                }
                if let (Some((a, _)), Some(s)) = (ov.drag, resp.interact_pointer_pos()) {
                    if resp.dragged() {
                        ov.drag = Some((a, s));
                    }
                }
                if let Some((a, b)) = ov.drag {
                    let br = Rect::from_two_pos(a, b);
                    p.rect_filled(br, 0.0, Color32::from_rgba_unmultiplied(255, 214, 10, 30));
                    p.rect_stroke(br, 0.0, st, StrokeKind::Middle);
                    let label = format!("{:.0} × {:.0} pt", br.width(), br.height());
                    editor::tag(&p, br.right_bottom() + Vec2::new(8.0, 8.0), &label);
                    if resp.drag_stopped() {
                        done = Some(Some(format!("{:.0} × {:.0}", br.width(), br.height())));
                    }
                } else if let Some(h) = hover {
                    let q = to_img(h);
                    if let Some((x0, x1, y0, y1)) = render::edge_span(&ov.img, q.x as i64, q.y as i64, 10) {
                        let a = to_screen(Pos2::new(x0 as f32, q.y));
                        let b = to_screen(Pos2::new(x1 as f32 + 1.0, q.y));
                        let c2 = to_screen(Pos2::new(q.x, y0 as f32));
                        let d = to_screen(Pos2::new(q.x, y1 as f32 + 1.0));
                        p.line_segment([a, b], st);
                        p.line_segment([c2, d], st);
                        let (wp, hp) = (b.x - a.x, d.y - c2.y);
                        editor::tag(&p, h + Vec2::new(14.0, 14.0), &format!("{wp:.0} × {hp:.0} pt"));
                        if resp.clicked() {
                            done = Some(Some(format!("{wp:.0} × {hp:.0}")));
                        }
                    }
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || vctx.input(|i| i.viewport().close_requested()) {
            done = Some(None);
        }
        // frames while the window settles; after that the mouse and keys
        // bring the next one (a still mouse needs none)
        if lift {
            vctx.request_repaint();
        }
    });
    if shown.is_some() {
        ov.frames += 1;
    }
    if let Some(copied) = done {
        c.overlay = None;
        if let Some(t) = copied {
            c.copy_text(&t);
            c.hud(t!("cap.copied_x", text = t).into_owned());
        }
    }
}

// ---------------------------------------------------------------- messages

fn hud(c: &mut Capture, ctx: &egui::Context, screen: Vec2) {
    let Some((msg, at)) = &c.hud else { return };
    if at.elapsed().as_secs_f32() > 2.2 {
        c.hud = None;
        return;
    }
    let msg = msg.clone();
    let w = (msg.chars().count() as f32 * 7.4 + 56.0).clamp(160.0, 520.0);
    let size = Vec2::new(w, 40.0);
    let pos = Pos2::new((screen.x - size.x) / 2.0, 44.0);
    let title = &plain("capture-hud");
    let vid = ViewportId::from_hash_of(("capture-hud", at));
    crate::kit::viewport::show(ctx, vid, floating(title, pos, size).with_active(false).with_mouse_passthrough(true), |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(r, r.height() / 2.0, Color32::from_rgba_unmultiplied(28, 28, 31, 240));
        p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        text_fit(p, r.center(), Align2::CENTER_CENTER, &msg, font(13.0), TEXT(), r.width() - 24.0);
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_pin_returns_to_the_app_the_click_came_from() {
        let pin = plain("capture-pin-3");
        // in the background: remember who is in front
        assert_eq!(track_return(None, false, Some(42), None), Some(42));
        // a click on the pin makes us active: keep it
        assert_eq!(track_return(Some(42), true, None, Some(&pin)), Some(42));
        assert_eq!(track_return(Some(42), true, None, None), Some(42));
        assert_eq!(track_return(Some(42), true, None, Some(&plain("capture-preview"))), Some(42));
        // a fresh screenshot: selection overlay, then preview, then pin
        assert_eq!(track_return(Some(42), true, None, Some(&plain("capture-overlay"))), Some(42));
        assert_eq!(track_return(Some(42), true, None, Some(&plain("capture-rec-bar"))), Some(42));
        assert_eq!(track_return(Some(42), true, None, Some(&plain("capture-video"))), None);
        assert_eq!(track_return(Some(42), true, None, Some(&plain("clipboard-hud"))), None);
        // the user went to the app window or the editor: closing returns there
        assert_eq!(track_return(Some(42), true, None, Some("CmdMeow")), None);
        assert_eq!(track_return(Some(42), true, None, Some(&plain("capture-editor"))), None);
        // in the background again, with another app in front
        assert_eq!(track_return(None, false, Some(7), None), Some(7));
    }

    #[test]
    fn pin_drag_keeps_aspect_and_clamps() {
        let base = Vec2::new(400.0, 200.0);
        assert!((drag_scale(base, 1.0, Grip::Right, Vec2::new(200.0, 0.0)) - 1.5).abs() < 1e-5);
        assert!((drag_scale(base, 1.0, Grip::Bottom, Vec2::new(0.0, -100.0)) - 0.5).abs() < 1e-5);
        // the corner follows the side that moved further, relative to its length
        assert!((drag_scale(base, 1.0, Grip::Corner, Vec2::new(40.0, 60.0)) - 1.3).abs() < 1e-5);
        assert_eq!(drag_scale(base, 1.0, Grip::Right, Vec2::new(-1000.0, 0.0)), PIN_MIN_SCALE);
        assert_eq!(drag_scale(base, 1.0, Grip::Corner, Vec2::new(5000.0, 0.0)), PIN_MAX_SCALE);
    }
}
