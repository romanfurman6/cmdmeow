//! Drawing for Displays: the panel widget, the window page, and the level
//! that shows after a brightness or volume key.

use super::engine::{Cmd, Screen};
use super::{mac, Displays, ID, OSD_SECS};
use crate::core::tool::{Cx, Size};
use crate::kit::glass::*;
use crate::kit::tile::{self, PAD};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportId};

const OSD_TITLE: &str = "CmdMeow Level";
const KNOB: f32 = 7.0;

fn pct(v: f32) -> String {
    format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round())
}

/// A level slider across `r`. Drag, click or scroll; returns the new level.
fn slider(ui: &mut Ui, r: Rect, id: egui::Id, value: f32, color: Color32) -> Option<f32> {
    let resp = ui.interact(r.expand2(Vec2::new(0.0, 6.0)), id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (x0, x1) = (r.left() + KNOB, r.right() - KNOB);
    let v = value.clamp(0.0, 1.0);
    let x = x0 + (x1 - x0) * v;
    let p = ui.painter();
    let track = Rect::from_min_max(Pos2::new(r.left(), r.center().y - 3.0), Pos2::new(r.right(), r.center().y + 3.0));
    p.rect_filled(track, 3.0, with_alpha(TEXT(), 0.12));
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.max.y)), 3.0, color);
    let lift = ui.ctx().animate_bool(id.with("hover"), resp.hovered() || resp.dragged());
    let c = Pos2::new(x, r.center().y);
    p.circle_filled(c + Vec2::new(0.0, 1.0), KNOB + 1.0, Color32::from_black_alpha(40));
    p.circle_filled(c, KNOB + lift, white(250));
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        return Some(((pos.x - x0) / (x1 - x0)).clamp(0.0, 1.0));
    }
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x);
        if dy.abs() > 0.1 {
            return Some((v + dy / 400.0).clamp(0.0, 1.0));
        }
    }
    None
}

/// One control in a list: a display's brightness or its speakers.
#[derive(Clone, Copy, PartialEq)]
enum Ctl {
    Brightness,
    Volume,
}

fn rows(screens: &[Screen], max: usize) -> Vec<(Screen, Ctl)> {
    let mut out: Vec<(Screen, Ctl)> = screens.iter().filter(|s| s.adjustable()).map(|s| (s.clone(), Ctl::Brightness)).collect();
    out.extend(screens.iter().filter(|s| s.adjustable() && s.volume.is_some()).map(|s| (s.clone(), Ctl::Volume)));
    out.truncate(max);
    out
}

/// The glyph of a row; the speaker one is the mute button.
fn row_icon(s: &Screen, c: Ctl) -> Icon {
    match c {
        Ctl::Brightness => Icon::Sun,
        Ctl::Volume if s.muted => Icon::SpeakerOff,
        Ctl::Volume => Icon::Speaker,
    }
}

fn row_level(s: &Screen, c: Ctl) -> f32 {
    match c {
        Ctl::Brightness => s.brightness,
        Ctl::Volume if s.muted => 0.0,
        Ctl::Volume => s.volume.unwrap_or(0.0),
    }
}

/// A slider row: glyph (mute button for volume), slider, percent.
fn control(d: &mut Displays, ui: &mut Ui, r: Rect, id: egui::Id, s: &Screen, c: Ctl, color: Color32) {
    let ir = Rect::from_center_size(Pos2::new(r.left() + 9.0, r.center().y), Vec2::splat(16.0));
    let p = ui.painter().clone();
    let glyph = row_icon(s, c);
    if c == Ctl::Volume {
        let resp = ui.interact(ir.expand(4.0), id.with("mute"), Sense::click());
        let col = if resp.hovered() { TEXT() } else { TEXT_DIM() };
        icon(&p, glyph, ir, col);
        if resp.on_hover_text(if s.muted { "Unmute" } else { "Mute" }).clicked() {
            d.set(Cmd::Mute(s.id, !s.muted));
        }
    } else {
        icon(&p, glyph, ir, TEXT_DIM());
    }
    let level = row_level(s, c);
    p.text(Pos2::new(r.right(), r.center().y), Align2::RIGHT_CENTER, pct(level), mono(12.0), TEXT_SOFT());
    let sr = Rect::from_min_max(Pos2::new(r.left() + 28.0, r.top()), Pos2::new(r.right() - 44.0, r.bottom()));
    if let Some(v) = slider(ui, sr, id, level, color) {
        match c {
            Ctl::Brightness => d.set(Cmd::Brightness(s.id, v)),
            Ctl::Volume => {
                if s.muted {
                    d.set(Cmd::Mute(s.id, false));
                }
                d.set(Cmd::Volume(s.id, v));
            }
        }
    }
}

// ---------------------------------------------------------------- widget

pub fn widget(d: &mut Displays, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let id = ui.scope_id().with("w-displays");
    if tile::card(ui, rect, id, false).clicked() {
        cx.open_page(ID, "");
    }
    let screens = d.screens();
    let color = tint::DISPLAYS();
    let p = ui.painter().clone();
    let ours: Vec<&Screen> = screens.iter().filter(|s| s.adjustable()).collect();
    if ours.is_empty() {
        tile::empty_note(&p, rect, "No external display");
        return;
    }
    let inner = rect.shrink(PAD);
    if size == Size::S14 {
        // one row: every monitor together
        let first = ours[0].clone();
        let title = if ours.len() == 1 { first.name.clone() } else { format!("{} displays", ours.len()) };
        text_fit(&p, Pos2::new(inner.left(), inner.top() + 8.0), Align2::LEFT_CENTER, &title, ty::title(), TEXT(), inner.width() - 40.0);
        let speakers = ours.iter().find(|s| s.volume.is_some()).map(|s| (*s).clone());
        let right = if speakers.is_some() { inner.right() - 30.0 } else { inner.right() };
        let row = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - 22.0), Pos2::new(right, inner.bottom()));
        let ir = Rect::from_center_size(Pos2::new(row.left() + 9.0, row.center().y), Vec2::splat(16.0));
        icon(&p, Icon::Sun, ir, TEXT_DIM());
        p.text(Pos2::new(inner.right(), inner.top() + 8.0), Align2::RIGHT_CENTER, pct(first.brightness), mono(12.0), TEXT_SOFT());
        let sr = Rect::from_min_max(Pos2::new(row.left() + 28.0, row.top()), row.max);
        if let Some(v) = slider(ui, sr, id.with("all"), first.brightness, color) {
            for s in &ours {
                d.set(Cmd::Brightness(s.id, v));
            }
        }
        if let Some(s) = speakers {
            let br = Rect::from_center_size(Pos2::new(inner.right() - 10.0, row.center().y), Vec2::splat(22.0));
            let resp = ui.interact(br, id.with("mute"), Sense::click());
            icon(&p, row_icon(&s, Ctl::Volume), br.shrink(3.0), if resp.hovered() { TEXT() } else { TEXT_DIM() });
            if resp.on_hover_text(if s.muted { "Unmute" } else { "Mute" }).clicked() {
                d.set(Cmd::Mute(s.id, !s.muted));
            }
        }
        return;
    }
    // Medium: a row per control, the display's name over its slider
    let list = rows(&screens, 4);
    let h = inner.height() / list.len().max(3) as f32;
    for (i, (s, c)) in list.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(inner.left(), inner.top() + h * i as f32), Vec2::new(inner.width(), h));
        text_fit(&p, Pos2::new(r.left() + 28.0, r.top() + 7.0), Align2::LEFT_CENTER, &s.name, ty::label(), TEXT_SOFT(), r.width() - 72.0);
        let cr = Rect::from_min_max(Pos2::new(r.left(), r.top() + 14.0), Pos2::new(r.right(), (r.top() + 36.0).min(r.bottom())));
        control(d, ui, cr, id.with(("row", s.id, i)), s, *c, color);
    }
}

// ---------------------------------------------------------------- page

pub fn page(d: &mut Displays, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let id = ui.scope_id().with("displays-page");
    let color = tint::DISPLAYS();
    let x0 = r.left() + 24.0;
    let x1 = r.right() - 24.0;
    // a title like every other page
    page_header(&p, r, "Displays", "", TEXT_DIM());
    let mut y = r.top() + PAGE_HEADER_H + 20.0;
    let screens = d.screens();
    let ours: Vec<Screen> = screens.iter().filter(|s| s.adjustable()).cloned().collect();
    if ours.is_empty() {
        empty(ui, &p, r, color);
        return;
    }
    // cards no wider than CARD_W; side by side when there's room
    const CARD_W: f32 = 520.0;
    const MIN_W: f32 = 360.0;
    const GAP: f32 = 12.0;
    let avail = x1 - x0;
    let cols = (((avail + GAP) / (MIN_W + GAP)).floor() as usize).clamp(1, ours.len().max(1));
    let cw = ((avail - GAP * (cols - 1) as f32) / cols as f32).min(CARD_W);
    let card_h = |s: &Screen| 52.0 + 40.0 * (1 + s.volume.is_some() as usize) as f32;
    for row in ours.chunks(cols) {
        let rh = row.iter().map(card_h).fold(0.0, f32::max);
        for (k, s) in row.iter().enumerate() {
            let cx = x0 + k as f32 * (cw + GAP);
            let cr = Rect::from_min_size(Pos2::new(cx, y), Vec2::new(cw, rh));
            glass_card(&p, cr, R_CARD, 0.0, false);
            let head = Pos2::new(cr.left() + 18.0, cr.top() + 26.0);
            icon(&p, Icon::Display, Rect::from_center_size(head + Vec2::new(9.0, 0.0), Vec2::splat(18.0)), lift(color));
            text_fit(&p, head + Vec2::new(30.0, 0.0), Align2::LEFT_CENTER, &s.name, ty::lead(), TEXT(), cr.width() - 66.0);
            let mut ry = cr.top() + 52.0;
            for c in [Ctl::Brightness, Ctl::Volume] {
                if c == Ctl::Volume && s.volume.is_none() {
                    continue;
                }
                let row = Rect::from_min_max(Pos2::new(cr.left() + 18.0, ry), Pos2::new(cr.right() - 18.0, ry + 28.0));
                control(d, ui, row, id.with(("ctl", s.id, c == Ctl::Volume)), s, c, color);
                ry += 40.0;
            }
        }
        y += rh + GAP;
    }
    // the keys card as wide as the cards above it
    let shown = cols.min(ours.len()).max(1) as f32;
    let x1 = (x0 + shown * cw + (shown - 1.0) * GAP).min(x1).max(x0 + cw.min(avail));
    // the keys
    let kr = Rect::from_min_max(Pos2::new(x0, y + 4.0), Pos2::new(x1, y + 56.0));
    glass_card(&p, kr, R_CARD, 0.0, false);
    p.text(Pos2::new(kr.left() + 18.0, kr.center().y), Align2::LEFT_CENTER, "Use keyboard keys for these displays", ty::body(), TEXT());
    let mut st = d.settings();
    let sw = Rect::from_min_size(Pos2::new(kr.right() - 18.0 - 38.0, kr.center().y - 11.0), Vec2::new(38.0, 22.0));
    if switch(ui, sw, id.with("keys"), st.keys, true).clicked() {
        st.keys = !st.keys;
        d.set_settings(st);
    }
}

/// No external display: what the page is for and how to get one here.
fn empty(ui: &mut Ui, p: &egui::Painter, r: Rect, color: Color32) {
    let c = Pos2::new(r.center().x, r.top() + r.height() * 0.38);
    p.circle_filled(c, 34.0, white(12));
    icon(p, Icon::Display, Rect::from_center_size(c, Vec2::splat(30.0)), lift(color));
    p.text(c + Vec2::new(0.0, 56.0), Align2::CENTER_CENTER, "No external display", bold(15.0), TEXT());
    let w = (r.width() - 48.0).min(380.0);
    let h = paragraph_centered(p, c + Vec2::new(0.0, 80.0), "Connect a monitor over USB-C, DisplayPort or HDMI. Its brightness and speaker volume show up here, and your Mac's keys control them.", font(13.0), TEXT_DIM(), w);
    let label = "Display Settings";
    let bw = button_width(p, label, BTN_SECONDARY);
    let br = Rect::from_center_size(Pos2::new(c.x, c.y + 80.0 + h + 30.0), Vec2::new(bw, 30.0));
    if pill_button(ui, br, label, BTN_SECONDARY, true).clicked() {
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Displays-Settings.extension").spawn();
    }
}

/// `text` wrapped to `w` and centred under `top`; returns its height.
fn paragraph_centered(p: &egui::Painter, top: Pos2, text: &str, f: egui::FontId, col: Color32, w: f32) -> f32 {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), f, col, w);
    job.halign = egui::Align::Center;
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(Pos2::new(top.x, top.y), g, col);
    h
}

// ---------------------------------------------------------------- the level after a key

/// Where the level shows on a display: bottom middle, like macOS's own.
pub fn osd_pos(frame: Rect, size: Vec2) -> Pos2 {
    Pos2::new(frame.center().x - size.x / 2.0, frame.bottom() - 120.0 - size.y)
}

/// How opaque the level is `t` seconds after the key: it fades at the end.
pub fn osd_alpha(t: f32) -> f32 {
    const FADE: f32 = 0.3;
    if t >= OSD_SECS {
        0.0
    } else {
        ((OSD_SECS - t) / FADE).clamp(0.0, 1.0)
    }
}

pub fn osd(d: &mut Displays, ctx: &egui::Context) {
    let (o, frame) = {
        let mut v = d.view.lock().unwrap_or_else(|e| e.into_inner());
        let Some(o) = v.osd else { return };
        let t = o.at.elapsed().as_secs_f32();
        if t >= OSD_SECS {
            v.osd = None;
            return;
        }
        let Some(frame) = v.screen(o.id).map(|s| s.frame) else { return };
        (o, frame)
    };
    let a = osd_alpha(o.at.elapsed().as_secs_f32());
    let size = Vec2::new(250.0, 46.0);
    let builder = ViewportBuilder::default()
        .with_title(OSD_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false)
        .with_mouse_passthrough(true)
        .with_position(osd_pos(frame, size))
        .with_inner_size(size);
    crate::kit::viewport::show(ctx, ViewportId::from_hash_of(OSD_TITLE), builder, |ui, _| {
        mac::float(OSD_TITLE);
        let r = ui.max_rect().shrink(1.0);
        let p = ui.painter().clone();
        let bg = Color32::from_rgba_unmultiplied(24, 24, 26, (242.0 * a) as u8);
        p.rect_filled(r, r.height() / 2.0, bg);
        p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, white((30.0 * a) as u8)), StrokeKind::Inside);
        let cy = r.center().y;
        let glyph = match (o.volume, o.muted) {
            (false, _) => Icon::Sun,
            (true, false) => Icon::Speaker,
            (true, true) => Icon::SpeakerOff,
        };
        let fg = white((235.0 * a) as u8);
        icon(&p, glyph, Rect::from_center_size(Pos2::new(r.left() + 24.0, cy), Vec2::splat(18.0)), fg);
        let level = if o.muted { 0.0 } else { o.level };
        p.text(Pos2::new(r.right() - 18.0, cy), Align2::RIGHT_CENTER, pct(level), mono(12.0), fg);
        let bar = Rect::from_min_max(Pos2::new(r.left() + 46.0, cy - 3.0), Pos2::new(r.right() - 62.0, cy + 3.0));
        p.rect_filled(bar, 3.0, white((50.0 * a) as u8));
        p.rect_filled(Rect::from_min_max(bar.min, Pos2::new(bar.left() + bar.width() * level.clamp(0.0, 1.0), bar.max.y)), 3.0, fg);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::displays::engine::How;

    fn s(id: u32, how: How, volume: Option<f32>) -> Screen {
        Screen::sample(id, "x", how == How::Native, how, Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)), 0.5, volume)
    }

    #[test]
    fn rows_list_brightness_first_then_speakers() {
        let list = rows(&[s(1, How::Native, None), s(2, How::Ddc, Some(0.2)), s(3, How::Dim, None)], 4);
        let got: Vec<(u32, bool)> = list.iter().map(|(s, c)| (s.id, *c == Ctl::Volume)).collect();
        assert_eq!(got, vec![(2, false), (3, false), (2, true)]);
        assert_eq!(rows(&[s(2, How::Ddc, Some(0.2)), s(3, How::Ddc, Some(0.1)), s(4, How::Ddc, Some(0.1))], 4).len(), 4);
    }

    #[test]
    fn muted_rows_read_zero() {
        let mut x = s(2, How::Ddc, Some(0.4));
        assert_eq!(row_level(&x, Ctl::Volume), 0.4);
        x.muted = true;
        assert_eq!(row_level(&x, Ctl::Volume), 0.0);
        assert_eq!(row_icon(&x, Ctl::Volume), Icon::SpeakerOff);
    }

    #[test]
    fn the_level_sits_low_in_the_middle_and_fades() {
        let f = Rect::from_min_size(Pos2::new(1000.0, 0.0), Vec2::new(1000.0, 800.0));
        let p = osd_pos(f, Vec2::new(200.0, 40.0));
        assert_eq!(p, Pos2::new(1400.0, 640.0));
        assert_eq!(osd_alpha(0.0), 1.0);
        assert!(osd_alpha(OSD_SECS - 0.1) < 1.0);
        assert_eq!(osd_alpha(OSD_SECS), 0.0);
    }

    #[test]
    fn percent_is_whole() {
        assert_eq!(pct(0.456), "46%");
        assert_eq!(pct(2.0), "100%");
    }
}
