//! The Cmd+Meow mark as vector paths: a keycap with cat ears and a ⌘ cut
//! out of it. Drawn with tiny-skia at any size; the menu bar uses it as a
//! template image, so only the alpha channel matters and macOS tints it.
//!
//! Coordinates are in a 1024 box traced from the brand glyph artwork.

use tiny_skia::{BlendMode, FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Keycap with ears, as one closed outline.
fn outline() -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    // top edge between the ears
    pb.move_to(330.0, 200.0);
    pb.line_to(692.0, 200.0);
    // right ear: inner edge up to the tip, outer edge down
    pb.cubic_to(760.0, 110.0, 830.0, 44.0, 862.0, 44.0);
    pb.cubic_to(905.0, 44.0, 940.0, 200.0, 940.0, 300.0);
    // right side of the keycap
    pb.cubic_to(955.0, 350.0, 958.0, 400.0, 958.0, 450.0);
    pb.line_to(958.0, 840.0);
    pb.cubic_to(958.0, 930.0, 895.0, 992.0, 805.0, 992.0);
    // bottom
    pb.line_to(218.0, 992.0);
    pb.cubic_to(128.0, 992.0, 65.0, 930.0, 65.0, 840.0);
    // left side
    pb.line_to(65.0, 450.0);
    pb.cubic_to(65.0, 400.0, 68.0, 350.0, 84.0, 300.0);
    // left ear: outer edge up to the tip, inner edge down
    pb.cubic_to(84.0, 200.0, 118.0, 44.0, 161.0, 44.0);
    pb.cubic_to(193.0, 44.0, 263.0, 110.0, 330.0, 200.0);
    pb.close();
    pb.finish()
}

/// Circular arc around (cx, cy) from angle `a0` to `a1` (radians, y down),
/// in cubic pieces of at most 90°.
fn arc(pb: &mut PathBuilder, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
    let n = ((a1 - a0).abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (a1 - a0) / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan() * r;
    let p = |a: f32| (cx + r * a.cos(), cy + r * a.sin());
    let (x, y) = p(a0);
    pb.move_to(x, y);
    for i in 0..n {
        let (s, e) = (a0 + step * i as f32, a0 + step * (i + 1) as f32);
        let (sx, sy) = p(s);
        let (ex, ey) = p(e);
        pb.cubic_to(sx - k * s.sin(), sy + k * s.cos(), ex + k * e.sin(), ey - k * e.cos(), ex, ey);
    }
}

/// The ⌘ as a centerline: four 3/4 loops joined by two bars each way.
fn command() -> Option<tiny_skia::Path> {
    use std::f32::consts::PI;
    let (l, r, t, b, rad) = (347.5, 670.5, 433.5, 757.5, 87.5);
    let mut pb = PathBuilder::new();
    arc(&mut pb, l, t, rad, 0.0, -1.5 * PI); // top left: east, over the top, to south
    arc(&mut pb, r, t, rad, PI, 2.5 * PI); // top right: west, over the top, to south
    arc(&mut pb, l, b, rad, -0.5 * PI, -2.0 * PI); // bottom left: north, round the left, to east
    arc(&mut pb, r, b, rad, 1.5 * PI, 3.0 * PI); // bottom right: north, round the right, to west
    for (x0, y0, x1, y1) in [(l + rad, t, l + rad, b), (r - rad, t, r - rad, b), (l, t + rad, r, t + rad), (l, b - rad, r, b - rad)] {
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
    }
    pb.finish()
}

/// The mark as straight RGBA (black, alpha = coverage) in a `px` square.
pub fn glyph_rgba(px: u32) -> Vec<u8> {
    let Some(mut pm) = Pixmap::new(px, px) else { return vec![0; (px * px * 4) as usize] };
    let scale = px as f32 / 1024.0;
    let t = Transform::from_scale(scale, scale).pre_translate(512.0 - 511.5, 512.0 - 518.0);
    let mut ink = Paint::default();
    ink.set_color_rgba8(0, 0, 0, 255);
    ink.anti_alias = true;
    if let Some(p) = outline() {
        pm.fill_path(&p, &ink, FillRule::Winding, t, None);
    }
    let mut cut = Paint::default();
    cut.blend_mode = BlendMode::Clear;
    cut.anti_alias = true;
    let stroke = Stroke { width: 59.0, line_cap: LineCap::Butt, ..Stroke::default() };
    if let Some(p) = command() {
        pm.stroke_path(&p, &cut, &stroke, t, None);
    }
    // black stays black after un-premultiplying, so the bytes are already straight RGBA
    pm.take()
}

// ---------------------------------------------------------------- the mascot

/// The keycap without its ears (the mark's outline, ears cut off along
/// their base lines), so the ears can move on their own.
fn body() -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(330.0, 200.0);
    pb.line_to(692.0, 200.0);
    pb.line_to(940.0, 300.0);
    pb.cubic_to(955.0, 350.0, 958.0, 400.0, 958.0, 450.0);
    pb.line_to(958.0, 840.0);
    pb.cubic_to(958.0, 930.0, 895.0, 992.0, 805.0, 992.0);
    pb.line_to(218.0, 992.0);
    pb.cubic_to(128.0, 992.0, 65.0, 930.0, 65.0, 840.0);
    pb.line_to(65.0, 450.0);
    pb.cubic_to(65.0, 400.0, 68.0, 350.0, 84.0, 300.0);
    pb.close();
    pb.finish()
}

/// One ear, the same curves as [`outline`]. It reaches a little into the
/// keycap below its base line, so no seam shows where the two meet.
fn ear(right: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    if right {
        pb.move_to(692.0, 200.0);
        pb.cubic_to(760.0, 110.0, 830.0, 44.0, 862.0, 44.0);
        pb.cubic_to(905.0, 44.0, 940.0, 200.0, 940.0, 300.0);
        pb.line_to(915.0, 330.0);
        pb.line_to(692.0, 250.0);
    } else {
        pb.move_to(84.0, 300.0);
        pb.cubic_to(84.0, 200.0, 118.0, 44.0, 161.0, 44.0);
        pb.cubic_to(193.0, 44.0, 263.0, 110.0, 330.0, 200.0);
        pb.line_to(330.0, 250.0);
        pb.line_to(110.0, 330.0);
    }
    pb.close();
    pb.finish()
}

/// Where each ear turns: the middle of its base line (1024 box).
const EAR_PIVOT: [(f32, f32); 2] = [(207.0, 250.0), (816.0, 250.0)];

/// The mascot in three white layers (alpha = coverage) in a `px` square:
/// the keycap with the ⌘ cut out, the left ear, the right ear. Drawn on top
/// of each other they make the mark.
pub fn mascot_rgba(px: u32) -> [Vec<u8>; 3] {
    let scale = px as f32 / 1024.0;
    let t = Transform::from_scale(scale, scale).pre_translate(512.0 - 511.5, 512.0 - 518.0);
    let mut ink = Paint::default();
    ink.set_color_rgba8(255, 255, 255, 255);
    ink.anti_alias = true;
    let layer = |path: Option<tiny_skia::Path>, cut: bool| -> Vec<u8> {
        let Some(mut pm) = Pixmap::new(px, px) else { return vec![0; (px * px * 4) as usize] };
        if let Some(p) = path {
            pm.fill_path(&p, &ink, FillRule::Winding, t, None);
        }
        if cut {
            let clear = Paint { blend_mode: BlendMode::Clear, anti_alias: true, ..Paint::default() };
            let stroke = Stroke { width: 59.0, line_cap: LineCap::Butt, ..Stroke::default() };
            if let Some(p) = command() {
                pm.stroke_path(&p, &clear, &stroke, t, None);
            }
        }
        // premultiplied white → straight white with the coverage as alpha
        let mut v = pm.take();
        for c in v.chunks_exact_mut(4) {
            if c[3] > 0 {
                c[..3].fill(255);
            }
        }
        v
    };
    [layer(body(), true), layer(ear(false), false), layer(ear(true), false)]
}

/// The mascot's three layers as textures, made once per context.
fn mascot_textures(ctx: &egui::Context) -> [egui::TextureHandle; 3] {
    let id = egui::Id::NULL.with("cmdmeow-mascot");
    if let Some(t) = ctx.data(|d| d.get_temp::<[egui::TextureHandle; 3]>(id)) {
        return t;
    }
    let n = 256;
    let [b, l, r] = mascot_rgba(n);
    let load = |name: &str, px: &[u8]| ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied([n as usize, n as usize], px), egui::TextureOptions::LINEAR);
    let t = [load("cmdmeow-mascot-body", &b), load("cmdmeow-mascot-ear-l", &l), load("cmdmeow-mascot-ear-r", &r)];
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// The mascot (the mark, ears that move) filling `rect`, tinted `col`.
/// `ears` tilt the left and right ear outward by that many radians (negative
/// folds them in). A squashed `rect` squashes the whole cat. A see-through
/// `col` draws the plain mark, ears still.
pub fn draw_mascot(p: &egui::Painter, rect: egui::Rect, col: egui::Color32, ears: (f32, f32)) {
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));
    // see-through: the layers' overlap would show brighter, so draw the
    // mark in one piece (ears still)
    if col.a() < 255 {
        p.image(glyph(p.ctx()).id(), rect, uv, col);
        return;
    }
    let [body, ear_l, ear_r] = mascot_textures(p.ctx());
    // the layers are drawn with the mark's own small offset (see glyph_rgba)
    let at = |(x, y): (f32, f32)| rect.min + egui::Vec2::new((x + 0.5) / 1024.0 * rect.width(), (y - 6.0) / 1024.0 * rect.height());
    for (tex, pivot, turn) in [(&ear_l, EAR_PIVOT[0], -ears.0), (&ear_r, EAR_PIVOT[1], ears.1)] {
        let mut m = egui::Mesh::with_texture(tex.id());
        m.add_rect_with_uv(rect, uv, col);
        if turn != 0.0 {
            m.rotate(egui::emath::Rot2::from_angle(turn), at(pivot));
        }
        p.add(m);
    }
    let mut m = egui::Mesh::with_texture(body.id());
    m.add_rect_with_uv(rect, uv, col);
    p.add(m);
}

/// The app icon at 64 px, for the window sidebar.
const APP_ICON_64: &[u8] = include_bytes!("../../assets/icon/AppIcon-64.png");

/// The app icon as a texture (loaded once per context).
pub fn app_icon(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::NULL.with("cmdmeow-app-icon");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let img = image::load_from_memory(APP_ICON_64).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let t = ctx.load_texture("cmdmeow-app-icon", ci, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// The mark as a white texture (tint it when drawing), loaded once per context.
pub fn glyph(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::NULL.with("cmdmeow-glyph");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let n = 64;
    let mut px = glyph_rgba(n);
    for c in px.chunks_exact_mut(4) {
        c[..3].fill(255);
    }
    let ci = egui::ColorImage::from_rgba_unmultiplied([n as usize, n as usize], &px);
    let t = ctx.load_texture("cmdmeow-glyph", ci, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &[u8], n: u32, x: f32, y: f32) -> u8 {
        let (x, y) = ((x / 1024.0 * n as f32) as u32, (y / 1024.0 * n as f32) as u32);
        px[((y * n + x) * 4 + 3) as usize]
    }

    #[test]
    fn mark_has_body_ears_and_a_cut_out_command_sign() {
        let n = 256;
        let px = glyph_rgba(n);
        assert_eq!(px.len(), (n * n * 4) as usize);
        assert_eq!(alpha(&px, n, 150.0, 600.0), 255, "keycap body");
        assert_eq!(alpha(&px, n, 165.0, 90.0), 255, "left ear");
        assert_eq!(alpha(&px, n, 850.0, 90.0), 255, "right ear");
        assert_eq!(alpha(&px, n, 512.0, 100.0), 0, "gap between the ears");
        assert_eq!(alpha(&px, n, 435.0, 600.0), 0, "left bar of the ⌘");
        assert_eq!(alpha(&px, n, 300.0, 350.0), 0, "top-left loop");
        assert_eq!(alpha(&px, n, 512.0, 600.0), 255, "the ⌘'s middle square stays filled");
        assert_eq!(alpha(&px, n, 348.0, 434.0), 255, "inside a loop stays filled");
        assert_eq!(alpha(&px, n, 10.0, 10.0), 0, "corner is clear");
    }

    #[test]
    fn mascot_layers_add_up_to_the_mark() {
        let n = 256;
        let mark = glyph_rgba(n);
        let [body, ear_l, ear_r] = mascot_rgba(n);
        // over-compositing the three layers gives the mark's coverage
        let mut worst = 0i32;
        for i in 0..(n * n) as usize {
            let a = |v: &[u8]| v[i * 4 + 3] as f32 / 255.0;
            let mut cov = a(&ear_l);
            cov += a(&ear_r) * (1.0 - cov);
            cov += a(&body) * (1.0 - cov);
            worst = worst.max(((cov * 255.0).round() as i32 - mark[i * 4 + 3] as i32).abs());
        }
        assert!(worst <= 40, "layers differ from the mark by {worst}/255 somewhere");
        assert_eq!(alpha(&body, n, 150.0, 600.0), 255, "keycap body");
        assert_eq!(alpha(&body, n, 165.0, 90.0), 0, "no ears on the body layer");
        assert_eq!(alpha(&ear_l, n, 165.0, 90.0), 255, "left ear");
        assert_eq!(alpha(&ear_r, n, 850.0, 90.0), 255, "right ear");
        assert_eq!(alpha(&ear_l, n, 850.0, 90.0), 0, "left layer has only the left ear");
        assert_eq!(alpha(&body, n, 435.0, 600.0), 0, "⌘ cut out of the body");
        assert_eq!(body[(128 * n as usize + 40) * 4], 255, "white ink");
    }

    #[test]
    fn app_icon_png_decodes() {
        let img = image::load_from_memory(APP_ICON_64).unwrap();
        assert_eq!((img.width(), img.height()), (64, 64));
    }
}
