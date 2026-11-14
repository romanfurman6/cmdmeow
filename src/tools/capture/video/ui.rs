//! The viewer's window: title bar with Save, the picture, the transport
//! row and the timeline (frames, trim handles, cuts, selection, playhead).

use super::speech::{self, Model};
use super::{scene, Drag, JobKind, Out, Tab, Viewer, SPEEDS};
use crate::kit::glass::*;
use crate::tools::capture::icons::{self, Ico};
use crate::tools::capture::record::timeline::clock_label;
use crate::tools::capture::Capture;
use egui::{Align2, Color32, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};

const HEAD_H: f32 = 46.0;
const TRANSPORT_H: f32 = 52.0;
const TIMELINE_H: f32 = 136.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 18.0;
/// Trim handles.
const TRIM: Color32 = Color32::from_rgb(0xFF, 0xC4, 0x2E);

/// The viewer window, when one is open.
pub fn show(c: &mut Capture, ctx: &egui::Context) {
    let Some(mut v) = c.video.take() else { return };
    v.tick();
    let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1440.0, 900.0));
    let size = Vec2::new(1280.0f32.min(screen.x * 0.92), 800.0f32.min(screen.y * 0.9));
    let pos = Pos2::new((screen.x - size.x) / 2.0, ((screen.y - size.y) / 2.0).max(30.0));
    let first = !v.opened;
    let builder = ViewportBuilder::default()
        .with_title(crate::tools::capture::windows::plain("capture-video"))
        .with_inner_size(size)
        .with_position(pos)
        .with_min_inner_size([980.0, 600.0])
        .with_transparent(true)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_has_shadow(true)
        .with_active(true);
    let mut closed = false;
    let upload = c.uploads.progress(&v.path);
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of("capture-video"), builder, |ui, _| {
        let vctx = ui.ctx().clone();
        if first {
            v.opened = true;
            vctx.send_viewport_cmd(ViewportCommand::Focus);
            crate::kit::native::activate();
        }
        if vctx.input(|i| i.viewport().close_requested()) {
            if v.dirty() && !v.asking {
                vctx.send_viewport_cmd(ViewportCommand::CancelClose);
                v.asking = true;
            } else {
                closed = true;
                return;
            }
        }
        let full = ui.max_rect();
        if draw(&mut v, ui, full, upload) {
            closed = true;
        }
    });
    for o in std::mem::take(&mut v.out) {
        match o {
            Out::CopyText(t) => c.copy_text(&t),
            Out::Saved { path, secs, copy } => {
                if !copy {
                    if let Some(id) = c.store.video_entry(&path).map(|s| s.id) {
                        c.store.set_video(id, path.clone(), secs as f32);
                    }
                    // shared before: upload the edit under the same link
                    if crate::tools::capture::share::link_for(&path).is_some() {
                        c.share(path);
                    }
                }
                c.files_at = None;
            }
            Out::Share(path) => c.copy_link(path),
        }
    }
    if closed {
        v.cancel_job();
        v.cancel_save();
        if let Some(f) = v.voice.take() {
            let _ = std::fs::remove_file(f);
        }
    } else {
        c.video = Some(v);
    }
}

/// The whole window; true to close.
fn draw(v: &mut Viewer, ui: &mut Ui, full: Rect, upload: Option<f32>) -> bool {
    let p = ui.painter().clone();
    p.rect_filled(full, 12.0, BG());
    let head = Rect::from_min_size(full.min, Vec2::new(full.width(), HEAD_H));
    let mut close = header(v, ui, head, upload);
    let tl = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - TIMELINE_H), full.max);
    let tr = Rect::from_min_max(Pos2::new(full.left(), tl.top() - TRANSPORT_H), Pos2::new(full.right(), tl.top()));
    let side = Rect::from_min_max(Pos2::new(full.right() - PANEL_W, head.bottom()), Pos2::new(full.right(), tr.top()));
    let pic = Rect::from_min_max(Pos2::new(full.left(), head.bottom()), Pos2::new(side.left(), tr.top()));
    picture(v, ui, pic);
    panel(v, ui, side);
    hairline(&p, tr.left_top(), tr.right_top());
    transport(v, ui, tr);
    timeline(v, ui, tl);
    if keys(v, ui) {
        close = true;
    }
    // frames while playing, and for a moment after a seek; progress a few
    // times a second (the spinner and bar move on their own)
    if v.playing() || v.drag.is_some() || v.kick.elapsed().as_secs_f32() < 0.6 {
        ui.ctx().request_repaint();
    } else if v.busy() || v.saving {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    if let Some((m, at)) = &v.status {
        if at.elapsed().as_secs_f32() < 2.5 {
            let g = p.layout_no_wrap(m.clone(), font(13.0), TEXT());
            let r = Rect::from_center_size(Pos2::new(pic.center().x, pic.bottom() - 30.0), g.size() + Vec2::new(28.0, 16.0));
            p.rect_filled(r, r.height() / 2.0, POPOVER());
            p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
            p.galley(r.center() - g.size() / 2.0, g, TEXT());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            v.status = None;
        }
    }
    close
}

fn header(v: &mut Viewer, ui: &mut Ui, r: Rect, upload: Option<f32>) -> bool {
    let p = ui.painter().clone();
    p.rect_filled(r, egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, FOOTER());
    hairline(&p, r.left_bottom(), r.right_bottom());
    let drag = ui.interact(Rect::from_min_max(r.min + Vec2::new(90.0, 0.0), r.max - Vec2::new(470.0, 0.0)), ui.scope_id().with("drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let y = r.center().y;
    let t = p.text(Pos2::new(r.left() + 90.0, y), Align2::LEFT_CENTER, v.title(), bold(13.0), TEXT());
    let meta = if v.edited() { format!("{}  →  {}", clock_label(v.duration()), clock_label(v.edl.length())) } else { clock_label(v.duration()) };
    p.text(Pos2::new(t.right() + 8.0, y), Align2::LEFT_CENTER, meta, font(12.0), TEXT_FAINT());

    let mut close = false;
    let mut x = r.right() - 14.0;
    let mut button = |ui: &mut Ui, label: &str, kind: u8, on: bool| {
        let w = button_width(ui.painter(), label, kind);
        let br = Rect::from_min_max(Pos2::new(x - w, y - 14.0), Pos2::new(x, y + 14.0));
        x = br.left() - 8.0;
        pill_button(ui, br, label, kind, on).clicked()
    };
    if v.asking {
        if button(ui, "Save", BTN_PRIMARY, !v.saving) {
            v.asking = false;
            v.save(false);
        }
        if button(ui, "Discard", BTN_DANGER, true) {
            close = true;
        }
        if button(ui, "Keep editing", BTN_SECONDARY, true) {
            v.asking = false;
        }
        p.text(Pos2::new(x - 4.0, y), Align2::RIGHT_CENTER, "Close without saving?", font(12.0), AMBER());
        return close;
    }
    let can = !v.saving && !v.busy();
    let saving = match &v.rendering {
        Some(pr) => format!("Saving {}%", pr.done.load(std::sync::atomic::Ordering::Relaxed) / 10),
        None if v.saving => "Saving".into(),
        None => "Save  ⌘S".into(),
    };
    if button(ui, &saving, BTN_PRIMARY, v.dirty() && can) {
        v.save(false);
    }
    if button(ui, "Save copy", BTN_SECONDARY, v.player.is_some() && can) {
        v.save(true);
    }
    // Share uploads the saved file; unsaved edits go up after Save
    let shared = crate::tools::capture::share::link_for(&v.path).is_some();
    let label = match upload {
        Some(f) => format!("Uploading {:.0}%", f * 100.0),
        None if shared => "Copy link".into(),
        None => "Share".into(),
    };
    if button(ui, &label, BTN_SECONDARY, upload.is_none() && !v.saving) {
        v.out.push(Out::Share(v.path.clone()));
    }
    if v.dirty() {
        let what = match (v.edited(), v.voice_on && v.voice.is_some()) {
            (true, true) => "Unsaved cuts and voice",
            (true, false) => "Unsaved cuts",
            (false, true) => "Unsaved voice cleanup",
            _ => "Unsaved frame",
        };
        chip(&p, Pos2::new(x - 2.0, y), Align2::RIGHT_CENTER, what, CHIP_ASK());
    }
    close
}

fn picture(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    use crate::tools::capture::editor::{c32, gradient_rect};
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, Color32::from_rgb(0x0E, 0x0E, 0x10));
    if let Some(e) = &v.error {
        p.text(r.center(), Align2::CENTER_CENTER, format!("Can't play this video: {e}"), font(13.0), TEXT_DIM());
        return;
    }
    if let Some(f) = v.player.as_ref().and_then(|pl| pl.frame()) {
        let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
        match &mut v.tex {
            Some(t) if t.size() == [f.w, f.h] => t.set(img, egui::TextureOptions::LINEAR),
            slot => *slot = Some(ui.ctx().load_texture("capture-video-frame", img, egui::TextureOptions::LINEAR)),
        }
    }
    // the frame being chosen, laid out as Save renders it
    let lay = scene::layout((v.size.0.max(2), v.size.1.max(2)), v.look.frame.as_ref());
    let area = r.shrink(sp::L);
    let out = Vec2::new(lay.out.0 as f32, lay.out.1 as f32);
    let s = (area.width() / out.x).min(area.height() / out.y);
    let outer = Rect::from_center_size(area.center(), out * s);
    let fit = Rect::from_min_size(outer.min + lay.screen.min.to_vec2() * s, lay.screen.size() * s);
    let radius = lay.radius * s;
    if let Some(f) = &v.look.frame {
        let (c0, c1) = scene::backdrop_colors(f);
        gradient_rect(&p, outer, c32(c0), c32(c1));
        if let Some((blur, dy)) = lay.shadow {
            let sh = egui::epaint::Shadow { offset: [0, (dy * s).round().min(127.0) as i8], blur: (blur * s).round().min(255.0) as u8, spread: 0, color: Color32::from_black_alpha(110) };
            p.add(sh.as_shape(fit, radius));
        }
    }
    match &v.tex {
        Some(t) => {
            p.add(egui::epaint::RectShape::filled(fit, radius, Color32::WHITE).with_texture(t.id(), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))));
        }
        None => {
            p.rect_filled(fit, radius.max(6.0), Color32::from_rgb(0x1A, 0x1A, 0x1E));
        }
    }
    // a cut piece shows dimmed, so it's clear it won't be in the file
    if v.player.is_some() && !v.edl.kept(v.time()) && v.time() < v.duration() - 1e-3 {
        p.rect_filled(outer, 0.0, Color32::from_black_alpha(150));
        p.text(fit.center(), Align2::CENTER_CENTER, "Cut", bold(15.0), TEXT_SOFT());
    }
    if let Some(text) = v.caption() {
        let wrap = (fit.width() * 0.8).max(120.0);
        let g = p.layout(text.to_string(), bold(15.0), Color32::WHITE, wrap);
        let box_ = Rect::from_center_size(Pos2::new(fit.center().x, fit.bottom() - 24.0 - g.size().y / 2.0), g.size() + Vec2::new(20.0, 10.0));
        p.rect_filled(box_, 6.0, Color32::from_black_alpha(170));
        p.galley(box_.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    let resp = ui.interact(outer, ui.scope_id().with("pic"), Sense::click());
    if resp.clicked() {
        v.toggle_play();
    }
    if !v.playing() && resp.hovered() {
        p.circle_filled(fit.center(), 30.0, Color32::from_black_alpha(140));
        icons::draw(&p, Ico::Play, Rect::from_center_size(fit.center() + Vec2::new(2.0, 0.0), Vec2::splat(24.0)), Color32::WHITE);
    }
}

fn plain_icon(ui: &mut Ui, r: Rect, ico: Ico, tip: &str, on: bool) -> bool {
    let resp = ui.interact(r, ui.scope_id().with(("vi", tip)), if on { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if on && resp.hovered() {
        p.rect_filled(r, 7.0, white(20));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    icons::draw(p, ico, Rect::from_center_size(r.center(), Vec2::splat(15.0)), if on { TEXT() } else { TEXT_MUTED() });
    resp.on_hover_text(tip).clicked() && on
}

fn transport(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let y = r.center().y;
    // play / pause
    let pr = Rect::from_center_size(Pos2::new(r.left() + PAD + 17.0, y), Vec2::splat(34.0));
    let presp = ui.interact(pr, ui.scope_id().with("play"), Sense::click());
    p.circle_filled(pr.center(), 17.0, white(if presp.hovered() { 40 } else { 26 }));
    let playing = v.playing();
    icons::draw(&p, if playing { Ico::Pause } else { Ico::Play }, Rect::from_center_size(pr.center() + Vec2::new(if playing { 0.0 } else { 1.5 }, 0.0), Vec2::splat(15.0)), TEXT());
    if presp.on_hover_text(if playing { "Pause (Space)" } else { "Play (Space)" }).clicked() {
        v.toggle_play();
    }
    // time in the edited video
    let now = v.edl.to_edited(v.time()).unwrap_or_else(|| v.edl.to_edited(v.edl.playable(v.time()).unwrap_or(v.edl.end())).unwrap_or(v.edl.length()));
    let tr = p.text(Pos2::new(pr.right() + 14.0, y), Align2::LEFT_CENTER, clock_label(now), mono(13.0), TEXT());
    p.text(Pos2::new(tr.right() + 4.0, y), Align2::LEFT_CENTER, format!("/ {}", clock_label(v.edl.length())), mono(13.0), TEXT_FAINT());
    // speed
    let labels: Vec<String> = SPEEDS.iter().map(|s| format!("{s}×")).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let sr = Rect::from_min_size(Pos2::new(tr.right() + 76.0, y - 14.0), Vec2::new(150.0, 28.0));
    if let Some(k) = segmented(ui, sr, ui.scope_id().with("speed"), &refs, v.speed) {
        v.set_speed(k);
    }

    // right: Cut, Keep, undo / redo
    let mut x = r.right() - PAD;
    let has_sel = v.sel.is_some();
    let cut_w = 150.0;
    let cr = Rect::from_min_max(Pos2::new(x - cut_w, y - 15.0), Pos2::new(x, y + 15.0));
    if pill_button(ui, cr, "", BTN_PRIMARY, has_sel).on_hover_text("Cut the chosen stretch out (⌫)").clicked() {
        v.cut_selection();
    }
    let col = if has_sel { Color32::WHITE } else { with_alpha(Color32::WHITE, 0.5) };
    icons::draw(&p, Ico::Scissors, Rect::from_center_size(Pos2::new(cr.left() + 22.0, y), Vec2::splat(14.0)), col);
    p.text(Pos2::new(cr.left() + 36.0, y), Align2::LEFT_CENTER, "Cut selection", bold(13.0), col);
    x = cr.left() - 8.0;
    let kw = button_width(&p, "Keep only this", BTN_SECONDARY);
    let kr = Rect::from_min_max(Pos2::new(x - kw, y - 15.0), Pos2::new(x, y + 15.0));
    if pill_button(ui, kr, "Keep only this", BTN_SECONDARY, has_sel).on_hover_text("Trim to the chosen stretch").clicked() {
        v.keep_selection();
    }
    x = kr.left() - 16.0;
    let rr = Rect::from_min_size(Pos2::new(x - 30.0, y - 15.0), Vec2::splat(30.0));
    if plain_icon(ui, rr, Ico::Redo, "Redo (⇧⌘Z)", !v.redo.is_empty()) {
        v.redo();
    }
    let ur = rr.translate(Vec2::new(-34.0, 0.0));
    if plain_icon(ui, ur, Ico::Undo, "Undo (⌘Z)", !v.undo.is_empty()) {
        v.undo();
    }
}

/// A short label for a ruler tick.
fn tick_label(t: f64) -> String {
    let s = t.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Seconds between ruler ticks so they're at least `min_px` apart.
pub fn tick_step(duration: f64, width: f32, min_px: f32) -> f64 {
    let per_px = duration / width.max(1.0) as f64;
    [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0].into_iter().find(|s| s / per_px >= min_px as f64).unwrap_or(3600.0)
}

fn timeline(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(r, egui::CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }, FOOTER());
    hairline(&p, r.left_top(), r.right_top());
    let d = v.duration().max(0.001);
    let full_lane = Rect::from_min_max(Pos2::new(r.left() + PAD + 8.0, r.top() + 30.0), Pos2::new(r.right() - PAD - 8.0, r.bottom() - 22.0));
    let lane = Rect::from_min_max(full_lane.min, Pos2::new(full_lane.right(), full_lane.bottom() - 24.0));
    let wave = Rect::from_min_max(Pos2::new(full_lane.left(), lane.bottom() + 4.0), full_lane.max);
    let x_of = |t: f64| lane.left() + (t / d) as f32 * lane.width();
    let t_of = |x: f32| (((x - lane.left()) / lane.width()) as f64 * d).clamp(0.0, d);

    // ruler
    let step = tick_step(d, lane.width(), 70.0);
    let mut t = 0.0;
    while t <= d + 1e-6 {
        let x = x_of(t);
        p.line_segment([Pos2::new(x, r.top() + 20.0), Pos2::new(x, r.top() + 25.0)], Stroke::new(1.0, white(60)));
        p.text(Pos2::new(x, r.top() + 12.0), Align2::CENTER_CENTER, tick_label(t), font(11.0), TEXT_FAINT());
        t += step;
    }

    // frames
    if v.strip.is_empty() && !v.strip_imgs.is_empty() {
        let ctx = ui.ctx().clone();
        v.strip = v.strip_imgs.iter().enumerate().map(|(k, img)| ctx.load_texture(format!("capture-video-strip-{k}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR)).collect();
    }
    p.rect_filled(lane, 6.0, Color32::from_rgb(0x26, 0x26, 0x2B));
    if !v.strip.is_empty() {
        let n = v.strip.len();
        let sw = lane.width() / n as f32;
        let clip = p.with_clip_rect(lane);
        for (k, tex) in v.strip.iter().enumerate() {
            let cell = Rect::from_min_size(Pos2::new(lane.left() + sw * k as f32, lane.top()), Vec2::new(sw + 0.5, lane.height()));
            // crop the middle of the frame to the cell's shape
            let [iw, ih] = tex.size();
            let (ia, ca) = (iw as f32 / ih.max(1) as f32, cell.width() / cell.height());
            let uv = if ia > ca {
                let f = ca / ia;
                Rect::from_min_max(Pos2::new(0.5 - f / 2.0, 0.0), Pos2::new(0.5 + f / 2.0, 1.0))
            } else {
                let f = ia / ca;
                Rect::from_min_max(Pos2::new(0.0, 0.5 - f / 2.0), Pos2::new(1.0, 0.5 + f / 2.0))
            };
            clip.image(tex.id(), cell, uv, Color32::WHITE);
        }
    }

    // the voice
    p.rect_filled(wave, 4.0, Color32::from_rgb(0x1E, 0x24, 0x20));
    let peaks = if v.voice_on && !v.clean_peaks.is_empty() { &v.clean_peaks } else { &v.peaks };
    if !peaks.is_empty() {
        let n = peaks.len();
        let step = (n as f32 / wave.width()).max(1.0);
        let mut x = 0.0f32;
        while x < wave.width() {
            let k = ((x / wave.width()) * n as f32) as usize;
            let hi = peaks[k..((k as f32 + step) as usize).min(n).max(k + 1)].iter().fold(0.0f32, |a, b| a.max(*b));
            let h = (hi.sqrt() * (wave.height() - 4.0)).max(1.0);
            let cx = wave.left() + x;
            p.line_segment([Pos2::new(cx, wave.center().y - h / 2.0), Pos2::new(cx, wave.center().y + h / 2.0)], Stroke::new(1.0, with_alpha(GREEN(), 0.75)));
            x += 2.0;
        }
    }

    // cut pieces: darkened and striped
    for (a, b) in v.edl.removed() {
        let cr = Rect::from_min_max(Pos2::new(x_of(a), lane.top()), Pos2::new(x_of(b), lane.bottom()));
        p.rect_filled(cr, 0.0, Color32::from_black_alpha(185));
        p.rect_filled(Rect::from_min_max(Pos2::new(cr.left(), wave.top()), Pos2::new(cr.right(), wave.bottom())), 0.0, Color32::from_black_alpha(170));
        let clip = p.with_clip_rect(cr.intersect(lane));
        let mut sx = cr.left() - lane.height();
        while sx < cr.right() {
            clip.line_segment([Pos2::new(sx, lane.bottom()), Pos2::new(sx + lane.height(), lane.top())], Stroke::new(1.0, white(28)));
            sx += 8.0;
        }
    }

    // what's kept: a yellow frame with handles at both ends
    let (ks, ke) = (x_of(v.edl.start()), x_of(v.edl.end()));
    let kept = Rect::from_min_max(Pos2::new(ks, lane.top()), Pos2::new(ke, lane.bottom()));
    p.rect_stroke(kept, 4.0, Stroke::new(2.0, TRIM), StrokeKind::Outside);
    for (x, left) in [(ks, true), (ke, false)] {
        let hr = if left { Rect::from_min_max(Pos2::new(x - 10.0, lane.top() - 2.0), Pos2::new(x, lane.bottom() + 2.0)) } else { Rect::from_min_max(Pos2::new(x, lane.top() - 2.0), Pos2::new(x + 10.0, lane.bottom() + 2.0)) };
        let cr = if left { egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 } } else { egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 } };
        p.rect_filled(hr, cr, TRIM);
        p.line_segment([Pos2::new(hr.center().x, hr.center().y - 7.0), Pos2::new(hr.center().x, hr.center().y + 7.0)], Stroke::new(1.5, Color32::from_black_alpha(140)));
    }

    // selection
    if let Some((a, b)) = v.sel {
        let sr = Rect::from_min_max(Pos2::new(x_of(a), lane.top()), Pos2::new(x_of(b), lane.bottom()));
        p.rect_filled(sr, 0.0, with_alpha(ACCENT(), 0.28));
        p.rect_stroke(sr, 0.0, Stroke::new(1.5, ACCENT()), StrokeKind::Inside);
        p.text(Pos2::new(sr.center().x, wave.bottom() + 11.0), Align2::CENTER_CENTER, format!("{:.1} s selected", b - a), font(11.0), ACCENT());
    }

    // playhead
    let px = x_of(v.time());
    p.line_segment([Pos2::new(px, r.top() + 22.0), Pos2::new(px, wave.bottom() + 2.0)], Stroke::new(2.0, Color32::WHITE));
    p.circle_filled(Pos2::new(px, r.top() + 22.0), 5.0, Color32::WHITE);

    // input
    let hit = Rect::from_min_max(Pos2::new(lane.left() - 12.0, r.top() + 4.0), Pos2::new(lane.right() + 12.0, wave.bottom() + 4.0));
    let resp = ui.interact(hit, ui.scope_id().with("timeline"), Sense::click_and_drag());
    let near = |x: f32, at: f32| (x - at).abs() <= 10.0;
    if let Some(pos) = resp.hover_pos() {
        let on_handle = pos.y >= lane.top() - 4.0 && (near(pos.x, ks - 5.0) || near(pos.x, ke + 5.0));
        ui.ctx().set_cursor_icon(if on_handle { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::Text });
    }
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            v.drag = Some(if pos.y < lane.top() - 4.0 {
                Drag::Scrub
            } else if near(pos.x, ks - 5.0) {
                v.begin_trim();
                Drag::Trim { end: false }
            } else if near(pos.x, ke + 5.0) {
                v.begin_trim();
                Drag::Trim { end: true }
            } else {
                Drag::Select(t_of(pos.x))
            });
            if let Some(pl) = &v.player {
                pl.pause();
            }
        }
    }
    if let (Some(drag), Some(pos)) = (v.drag, resp.interact_pointer_pos()) {
        let t = t_of(pos.x);
        match drag {
            Drag::Scrub => v.seek(t),
            Drag::Trim { end } => {
                v.trim_to(end, t);
                v.seek(if end { v.edl.end() } else { v.edl.start() });
            }
            Drag::Select(a) => {
                v.sel = (t - a).abs().ge(&0.05).then(|| (a.min(t), a.max(t)));
                v.seek(t);
            }
        }
    }
    if resp.drag_stopped() {
        if let Some(Drag::Trim { .. }) = v.drag {
            v.end_trim();
        }
        v.drag = None;
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            v.sel = None;
            v.seek(t_of(pos.x));
        }
    }
}

// ---------------------------------------------------------------- side panel

fn panel(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, SIDEBAR());
    hairline(&p, r.left_top(), r.left_bottom());
    let tabs = Rect::from_min_size(r.min + Vec2::new(14.0, 12.0), Vec2::new(r.width() - 28.0, 28.0));
    let all = [Tab::Transcript, Tab::Audio, Tab::Look];
    let k = all.iter().position(|t| *t == v.tab).unwrap_or(0);
    if let Some(i) = segmented(ui, tabs, ui.scope_id().with("tabs"), &["Transcript", "Voice", "Frame"], k) {
        v.tab = all[i];
    }
    let body = Rect::from_min_max(Pos2::new(r.left() + 14.0, tabs.bottom() + 12.0), Pos2::new(r.right() - 14.0, r.bottom() - 10.0));
    match v.tab {
        Tab::Transcript => transcript(v, ui, body),
        Tab::Audio => voice_tab(v, ui, body),
        Tab::Look => frame_tab(v, ui, body),
    }
}

/// The frame: none or a backdrop, then padding, shape, corners and shadow.
/// Any pick puts a frame on, starting from the last one used.
fn frame_tab(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    use crate::tools::capture::editor::{c32, gradient_disc};
    use crate::tools::capture::render::{Ratio, BACKDROPS};
    let p = ui.painter_at(r);
    let id = ui.scope_id().with("frame");
    let cur = v.look.frame;
    let mut next = cur;
    let mut f = cur.unwrap_or(v.prefs.frame);

    // the backdrops; the first takes the frame off
    let n = BACKDROPS.len() + 1;
    let step = r.width() / n as f32;
    let rad = (step / 2.0 - 6.0).min(13.0);
    let cy = r.top() + rad + 4.0;
    for k in 0..n {
        let c = Pos2::new(r.left() + step * (k as f32 + 0.5), cy);
        let name = if k == 0 { "None" } else { BACKDROPS[k - 1].0 };
        let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(rad * 2.0 + 8.0)), id.with(("bg", k)), Sense::click()).on_hover_text(name);
        let on = match cur {
            None => k == 0,
            Some(x) => x.bg + 1 == k,
        };
        if on {
            stroke_circle(&p, c, rad + 3.5, Stroke::new(2.0, BLUE()));
        }
        if k == 0 {
            stroke_circle(&p, c, rad - 0.5, Stroke::new(1.5, TEXT_SOFT()));
            let d = Vec2::splat((rad - 0.5) * std::f32::consts::FRAC_1_SQRT_2);
            p.line_segment([c - d, c + d], Stroke::new(1.5, TEXT_SOFT()));
        } else {
            let (_, c0, c1) = BACKDROPS[k - 1];
            gradient_disc(&p, c, rad, c32(c0), c32(c1));
            stroke_circle(&p, c, rad, Stroke::new(1.0, white(64)));
        }
        if resp.clicked() {
            if k == 0 {
                next = None;
            } else {
                f.bg = k - 1;
                next = Some(f);
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    // its options, one settings row each
    let mut y = cy + rad + 4.0 + sp::M;
    let row_h = sp::SETTINGS_ROW_H;
    let ratios: Vec<&str> = Ratio::ALL.iter().map(|q| q.label()).collect();
    for k in 0..4 {
        let row = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), row_h));
        if k > 0 {
            hairline(&p, row.left_top(), row.right_top());
        }
        let label = ["Padding", "Shape", "Round corners", "Shadow"][k];
        p.text(Pos2::new(row.left(), row.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT());
        let seg_at = |labels: &[&str]| {
            let w = labels.iter().map(|l| p.layout_no_wrap(l.to_string(), font(12.0), TEXT()).size().x + 2.0 * sp::M).fold(0.0f32, f32::max) * labels.len() as f32 + 4.0;
            Rect::from_min_max(Pos2::new(row.right() - w, row.center().y - sp::CONTROL_H / 2.0), Pos2::new(row.right(), row.center().y + sp::CONTROL_H / 2.0))
        };
        let sw = Rect::from_min_size(Pos2::new(row.right() - 30.0, row.center().y - 9.0), Vec2::new(30.0, 18.0));
        match k {
            0 => {
                let labels = ["S", "M", "L"];
                if let Some(i) = segmented(ui, seg_at(&labels), id.with("pad"), &labels, f.pad.min(2)) {
                    f.pad = i;
                    next = Some(f);
                }
            }
            1 => {
                if let Some(i) = segmented(ui, seg_at(&ratios), id.with("ratio"), &ratios, Ratio::ALL.iter().position(|q| *q == f.ratio).unwrap_or(0)) {
                    f.ratio = Ratio::ALL[i];
                    next = Some(f);
                }
            }
            2 => {
                if switch(ui, sw, id.with("round"), f.round && cur.is_some(), true).clicked() {
                    f.round = !f.round || cur.is_none();
                    next = Some(f);
                }
            }
            _ => {
                if switch(ui, sw, id.with("shadow"), f.shadow && cur.is_some(), true).clicked() {
                    f.shadow = !f.shadow || cur.is_none();
                    next = Some(f);
                }
            }
        }
        y = row.bottom();
    }
    if next != cur {
        v.set_frame(next);
    }
}

/// A job's progress: label, bar, Stop.
fn job_row(v: &mut Viewer, ui: &mut Ui, r: Rect, label: &str) -> f32 {
    let Some(j) = &v.job else { return r.top() };
    let p = ui.painter().clone();
    let frac = j.progress.fraction();
    let detail = match j.kind {
        JobKind::Download(_) => {
            let (d, t) = (j.progress.done.load(std::sync::atomic::Ordering::Relaxed), j.progress.total.load(std::sync::atomic::Ordering::Relaxed));
            format!("{} of {}", human_bytes(d), human_bytes(t))
        }
        _ => format!("{:.0}%", frac * 100.0),
    };
    // the arc and the bar move on Core Animation: smooth with few frames
    crate::kit::spin::arc(&p, "video-job", Pos2::new(r.left() + 8.0, r.top() + 10.0), 6.0, ACCENT());
    p.text(Pos2::new(r.left() + 22.0, r.top() + 10.0), Align2::LEFT_CENTER, label, font(12.0), TEXT());
    p.text(Pos2::new(r.right(), r.top() + 10.0), Align2::RIGHT_CENTER, detail, font(12.0), TEXT_FAINT());
    crate::kit::spin::bar(&p, "video-job-bar", Rect::from_min_size(Pos2::new(r.left(), r.top() + 26.0), Vec2::new(r.width(), 6.0)), frac, ACCENT());
    let stop = Rect::from_min_size(Pos2::new(r.left(), r.top() + 42.0), Vec2::new(70.0, 26.0));
    if pill_button(ui, stop, "Stop", BTN_SECONDARY, true).clicked() {
        v.cancel_job();
    }
    stop.bottom() + 12.0
}

fn error_row(v: &Viewer, p: &egui::Painter, r: Rect, y: f32) -> f32 {
    match &v.job_error {
        Some(e) => {
            let h = paragraph(p, Pos2::new(r.left(), y), e, font(12.0), RED(), r.width());
            y + h + 10.0
        }
        None => y,
    }
}

fn transcript(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let mut y = r.top();
    let model = v.prefs.model;
    let busy_here = matches!(v.job.as_ref().map(|j| j.kind), Some(JobKind::Download(_) | JobKind::Transcribe));
    if busy_here {
        let label = match v.job.as_ref().map(|j| j.kind) {
            Some(JobKind::Download(m)) => format!("Downloading the {} model", m.name().to_lowercase()),
            _ => "Transcribing on this Mac".to_string(),
        };
        y = job_row(v, ui, Rect::from_min_max(Pos2::new(r.left(), y), r.max), &label);
    }
    if v.lines.is_empty() {
        if busy_here {
            return;
        }
        y = error_row(v, &p, r, y);
        // what's needed to start: a model (downloaded once), then Transcribe
        p.text(Pos2::new(r.left(), y + 8.0), Align2::LEFT_CENTER, "Speech to text, on this Mac", bold(13.0), TEXT());
        y += 22.0;
        y += paragraph(&p, Pos2::new(r.left(), y), "Whisper runs here; the sound never leaves the Mac. The model is downloaded once.", font(12.0), TEXT_DIM(), r.width()) + 12.0;
        for m in Model::ALL {
            let row = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 50.0));
            let resp = ui.interact(row, ui.scope_id().with(("model", m.name())), Sense::click());
            let on = m == model;
            p.rect_filled(row, 8.0, if on { with_alpha(BLUE(), 0.18) } else { white(if resp.hovered() { 14 } else { 8 }) });
            if on {
                p.rect_stroke(row, 8.0, Stroke::new(1.0, with_alpha(ACCENT(), 0.7)), StrokeKind::Inside);
            }
            let size = if m.ready() { "Downloaded".to_string() } else { human_bytes(m.bytes()) };
            p.text(Pos2::new(row.left() + 12.0, row.top() + 16.0), Align2::LEFT_CENTER, m.name(), bold(13.0), TEXT());
            p.text(Pos2::new(row.right() - 12.0, row.top() + 16.0), Align2::RIGHT_CENTER, size, font(12.0), if m.ready() { GREEN() } else { TEXT_FAINT() });
            text_fit(&p, Pos2::new(row.left() + 12.0, row.top() + 34.0), Align2::LEFT_CENTER, m.about(), font(12.0), TEXT_DIM(), row.width() - 24.0);
            if resp.clicked() && !on {
                v.prefs.model = m;
                v.save_prefs();
            }
            y = row.bottom() + 8.0;
        }
        let go = Rect::from_min_size(Pos2::new(r.left(), y + 4.0), Vec2::new(r.width(), 32.0));
        let label = if v.prefs.model.ready() { "Transcribe".to_string() } else { format!("Download ({}) and transcribe", human_bytes(v.prefs.model.bytes())) };
        if pill_button(ui, go, &label, BTN_PRIMARY, !v.busy() && v.player.is_some()).clicked() {
            v.transcribe();
        }
        y = go.bottom() + 10.0;
        if v.prefs.model.ready() && link(ui, Pos2::new(r.left(), y + 8.0), Align2::LEFT_CENTER, &format!("Remove the {} model", v.prefs.model.name().to_lowercase()), 11.5).clicked() {
            v.prefs.model.remove();
        }
        return;
    }

    // the lines
    let foot = 44.0;
    let list = Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), r.bottom() - foot));
    let now = speech::line_at(&v.lines, v.time());
    let mut clicked = None;
    let shift = ui.input(|i| i.modifiers.shift);
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("video-lines").auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width();
            for (i, l) in v.lines.iter().enumerate() {
                let cut = speech::line_cut(l, &v.edl);
                let picked = v.picked.is_some_and(|(a, b)| i >= a && i <= b);
                let g = ui.painter().layout(l.text.clone(), font(13.0), if cut { TEXT_MUTED() } else { TEXT_SOFT() }, w - 54.0);
                let (row, resp) = ui.allocate_exact_size(Vec2::new(w, g.size().y + 12.0), Sense::click());
                let pp = ui.painter();
                if picked {
                    pp.rect_filled(row, 6.0, with_alpha(ACCENT(), 0.22));
                } else if Some(i) == now {
                    pp.rect_filled(row, 6.0, white(14));
                } else if resp.hovered() {
                    pp.rect_filled(row, 6.0, white(8));
                }
                pp.text(Pos2::new(row.left() + 6.0, row.top() + 6.0), Align2::LEFT_TOP, clock_label(v.edl.edited_at(l.start)), mono(11.0), if Some(i) == now { ACCENT() } else { TEXT_FAINT() });
                let at = Pos2::new(row.left() + 48.0, row.top() + 6.0);
                let rect = Rect::from_min_size(at, g.size());
                pp.galley(at, g, TEXT_SOFT());
                if cut {
                    pp.line_segment([Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)], Stroke::new(1.0, TEXT_MUTED()));
                }
                if Some(i) == now && v.playing() {
                    ui.scroll_to_rect(row, None);
                }
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
        });
    });
    if let Some(i) = clicked {
        v.pick_line(i, shift);
    }

    // under the list: captions, copy and save the text
    let by = r.bottom() - foot + sp::S;
    let cb = Rect::from_min_size(Pos2::new(r.left(), by + 6.0), Vec2::splat(16.0));
    if checkbox(ui, cb, ui.scope_id().with("captions"), v.prefs.captions, true).clicked() {
        v.prefs.captions = !v.prefs.captions;
        v.save_prefs();
    }
    p.text(Pos2::new(cb.right() + 8.0, cb.center().y), Align2::LEFT_CENTER, "Captions", font(12.0), TEXT_SOFT());
    let mut x = r.right();
    for (label, k) in [(".txt", 2), (".srt", 1), ("Copy", 0)] {
        let w = button_width(&p, label, BTN_SECONDARY) - 6.0;
        let br = Rect::from_min_max(Pos2::new(x - w, by), Pos2::new(x, by + 28.0));
        x = br.left() - 6.0;
        let tip = ["Copy the text", "Save captions (.srt) beside the video", "Save the text (.txt) beside the video"][k];
        if pill_button(ui, br, label, BTN_SECONDARY, true).on_hover_text(tip).clicked() {
            match k {
                0 => v.copy_text(),
                1 => v.export_text(true),
                _ => v.export_text(false),
            }
        }
    }
    if !v.busy() && link(ui, Pos2::new(x - 6.0, by + 14.0), Align2::RIGHT_CENTER, "Redo", 11.5).on_hover_text("Transcribe again").clicked() {
        v.lines.clear();
        v.picked = None;
    }
}

fn voice_tab(v: &mut Viewer, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let mut y = r.top();
    p.text(Pos2::new(r.left(), y + 8.0), Align2::LEFT_CENTER, "Clean up voice", bold(13.0), TEXT());
    y += 22.0;
    let about = if v.tracks >= 3 {
        "Takes background noise out of the microphone; system sound stays as it was. Runs on this Mac."
    } else if v.tracks > 0 {
        "Takes background noise out of the sound. Runs on this Mac. (Recordings with both the microphone and system sound clean the voice alone.)"
    } else {
        "This video has no sound."
    };
    y += paragraph(&p, Pos2::new(r.left(), y), about, font(12.0), TEXT_DIM(), r.width()) + 14.0;
    if v.tracks == 0 {
        return;
    }
    if matches!(v.job.as_ref().map(|j| j.kind), Some(JobKind::Clean)) {
        y = job_row(v, ui, Rect::from_min_max(Pos2::new(r.left(), y), r.max), "Cleaning up the voice");
    }
    y = error_row(v, &p, r, y);

    // strength
    p.text(Pos2::new(r.left(), y + 8.0), Align2::LEFT_CENTER, "Strength", font(12.0), TEXT_SOFT());
    let pct = v.prefs.strength;
    p.text(Pos2::new(r.right(), y + 8.0), Align2::RIGHT_CENTER, if pct >= 0.99 { "Full".to_string() } else { format!("{:.0} dB", super::voice::atten_db(pct)) }, font(12.0), TEXT_FAINT());
    y += 22.0;
    let track = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 20.0));
    let resp = ui.interact(track, ui.scope_id().with("strength"), Sense::click_and_drag());
    let rail = Rect::from_center_size(track.center(), Vec2::new(track.width(), 4.0));
    p.rect_filled(rail, 2.0, white(30));
    p.rect_filled(Rect::from_min_max(rail.min, Pos2::new(rail.left() + rail.width() * pct, rail.bottom())), 2.0, ACCENT());
    p.circle_filled(Pos2::new(rail.left() + rail.width() * pct, rail.center().y), 8.0, Color32::WHITE);
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        v.prefs.strength = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
    }
    if resp.drag_stopped() || resp.clicked() {
        v.save_prefs();
    }
    y = track.bottom() + 4.0;
    p.text(Pos2::new(r.left(), y + 6.0), Align2::LEFT_CENTER, "Gentle", font(11.0), TEXT_FAINT());
    p.text(Pos2::new(r.right(), y + 6.0), Align2::RIGHT_CENTER, "Strong", font(11.0), TEXT_FAINT());
    y += 22.0;
    let go = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 32.0));
    let label = if v.voice.is_some() { "Clean up again" } else { "Clean up voice" };
    if pill_button(ui, go, label, BTN_PRIMARY, !v.busy() && v.player.is_some()).clicked() {
        v.clean_voice();
    }
    y = go.bottom() + 16.0;

    // before / after
    if v.voice.is_some() {
        let seg = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), 28.0));
        if let Some(k) = segmented(ui, seg, ui.scope_id().with("ab"), &["Before", "After"], v.voice_on as usize) {
            v.set_voice_on(k == 1);
        }
        y = seg.bottom() + 12.0;
        for (label, peaks, on) in [("Before", &v.peaks, !v.voice_on), ("After", &v.clean_peaks, v.voice_on)] {
            p.text(Pos2::new(r.left(), y + 6.0), Align2::LEFT_CENTER, label, font(11.0), if on { TEXT() } else { TEXT_FAINT() });
            let w = Rect::from_min_size(Pos2::new(r.left(), y + 14.0), Vec2::new(r.width(), 36.0));
            p.rect_filled(w, 4.0, Color32::from_rgb(0x1E, 0x24, 0x20));
            if !peaks.is_empty() {
                let n = peaks.len();
                let mut x = 0.0;
                while x < w.width() {
                    let k = ((x / w.width()) * n as f32) as usize;
                    let h = (peaks[k.min(n - 1)].sqrt() * (w.height() - 4.0)).max(1.0);
                    p.line_segment([Pos2::new(w.left() + x, w.center().y - h / 2.0), Pos2::new(w.left() + x, w.center().y + h / 2.0)], Stroke::new(1.0, with_alpha(GREEN(), if on { 0.85 } else { 0.4 })));
                    x += 2.0;
                }
            }
            y = w.bottom() + 10.0;
        }
        paragraph(&p, Pos2::new(r.left(), y), "What plays is what Save writes into the video.", font(12.0), TEXT_FAINT(), r.width());
    }
}

/// Keyboard: Space, arrows, ⌫, ⌘Z / ⇧⌘Z, ⌘S, Esc. True to close.
fn keys(v: &mut Viewer, ui: &mut Ui) -> bool {
    let (space, left, right, del, undo, redo, save, esc, shift) = ui.input(|i| {
        let cmd = i.modifiers.command;
        (
            i.key_pressed(Key::Space),
            i.key_pressed(Key::ArrowLeft),
            i.key_pressed(Key::ArrowRight),
            i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete),
            cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
            cmd && i.modifiers.shift && i.key_pressed(Key::Z),
            cmd && i.key_pressed(Key::S),
            i.key_pressed(Key::Escape),
            i.modifiers.shift,
        )
    });
    let jump = if shift { 5.0 } else { 1.0 };
    if space {
        v.toggle_play();
    }
    if left {
        v.step(-jump);
    }
    if right {
        v.step(jump);
    }
    if del {
        v.cut_selection();
    }
    if undo {
        v.undo();
    }
    if redo {
        v.redo();
    }
    if save {
        v.save(false);
    }
    if esc {
        if v.sel.is_some() {
            v.sel = None;
        } else if v.asking {
            v.asking = false;
        } else if v.dirty() {
            v.asking = true;
        } else {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_ticks_stay_apart() {
        assert_eq!(tick_step(60.0, 900.0, 70.0), 5.0);
        assert_eq!(tick_step(3600.0, 900.0, 70.0), 300.0);
        assert_eq!(tick_step(5.0, 900.0, 70.0), 1.0);
    }
}
