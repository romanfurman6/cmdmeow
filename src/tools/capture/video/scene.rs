//! How a video is framed when it's saved: the picture on a gradient
//! backdrop with padding, round corners and a soft shadow (the screenshot
//! editor's frame). Pure: the layout maths and the still parts of the frame,
//! drawn once per save on the CPU and tested on any platform. The macOS side
//! ([`super::compose`]) puts every frame of the video into them on the GPU;
//! the viewer draws the same layout live while the frame is chosen.
//!
//! Pixels are BGRA, premultiplied (what the video side reads and writes).
//! tiny-skia thinks in RGBA, so every color we paint goes in with red and
//! blue swapped ([`bgra`]); pictures from the video stay as they are.

use crate::tools::capture::render::{self, Frame, Rgba8};
use serde::{Deserialize, Serialize};
use tiny_skia::{BlendMode, FillRule, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint, SpreadMode, Transform};

/// How a video is saved. A framed video keeps it in its project.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Style {
    pub frame: Option<Frame>,
}

// ---------------------------------------------------------------- layout

/// Where the picture goes in the saved video, in output pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub out: (u32, u32),
    /// The recorded picture inside the output, on whole pixels.
    pub screen: egui::Rect,
    pub radius: f32,
    /// Shadow blur and how far down it falls.
    pub shadow: Option<(f32, f32)>,
}

/// Even sizes: H.264 wants them.
fn even(v: f32) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// The output keeps the recording's size: with a frame, the picture
/// shrinks inside it instead of the video growing.
pub fn layout(src: (u32, u32), frame: Option<&Frame>) -> Layout {
    let (w, h) = (src.0 as f32, src.1 as f32);
    let Some(f) = frame else {
        return Layout { out: (even(w), even(h)), screen: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h)), radius: 0.0, shadow: None };
    };
    let ppp = (w.max(h) / 1440.0).max(1.0);
    let g = render::frame_geom(w, h, f, ppp);
    let k = w.max(h) / g.size.x.max(g.size.y);
    let out = (even(g.size.x * k), even(g.size.y * k));
    let sx = out.0 as f32 / g.size.x;
    let sy = out.1 as f32 / g.size.y;
    // whole pixels: the picture's straight edges stay crisp
    let px = |x: f32, y: f32| egui::pos2(x.round(), y.round());
    let screen = egui::Rect::from_min_max(px(g.inner.min.x * sx, g.inner.min.y * sy), px(g.inner.max.x * sx, g.inner.max.y * sy));
    Layout { out, screen, radius: g.radius * k, shadow: g.shadow.map(|(b, d)| (b * k, d * k)) }
}

/// The backdrop's colors at its top-left and bottom-right corners.
pub fn backdrop_colors(f: &Frame) -> (Rgba8, Rgba8) {
    let (_, c0, c1) = render::BACKDROPS[f.bg.min(render::BACKDROPS.len() - 1)];
    (c0, c1)
}

// ---------------------------------------------------------------- still parts

/// A color for a BGRA pixmap (tiny-skia reads it as RGBA).
pub fn bgra(c: Rgba8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c[2], c[1], c[0], c[3])
}

/// The parts of a frame that never move, output-sized, BGRA premultiplied.
/// `under` is the backdrop with the picture's shadow; `over` is the same
/// with a hole the picture shows through, its round corners cut by the
/// backdrop. A frame of video is `over` on (the picture on `under`).
pub struct Chrome {
    pub under: Pixmap,
    pub over: Pixmap,
}

pub fn chrome(f: &Frame, lay: &Layout) -> Option<Chrome> {
    let (ow, oh) = lay.out;
    let (c0, c1) = backdrop_colors(f);
    let mut under = Pixmap::new(ow, oh)?;
    let grad = tiny_skia::LinearGradient::new(
        tiny_skia::Point::from_xy(0.0, 0.0),
        tiny_skia::Point::from_xy(ow as f32, oh as f32),
        vec![tiny_skia::GradientStop::new(0.0, bgra(c0)), tiny_skia::GradientStop::new(1.0, bgra(c1))],
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    under.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, ow as f32, oh as f32)?, &Paint { shader: grad, ..Default::default() }, Transform::identity(), None);
    if let Some((blur, dy)) = lay.shadow {
        render::soft_shadow(&mut under, lay.screen.translate(egui::vec2(0.0, dy)), lay.radius, blur);
    }
    let mut over = under.clone();
    let hole = Paint { anti_alias: true, blend_mode: BlendMode::Clear, ..Default::default() };
    over.fill_path(&rounded(lay.screen, lay.radius)?, &hole, FillRule::Winding, Transform::identity(), None);
    Some(Chrome { under, over })
}

/// One framed frame on the CPU: `screen` (BGRA, tight rows) scaled into
/// the layout's picture, between the still parts. What the GPU does for
/// every frame; here to check it.
pub fn compose_cpu(ch: &Chrome, lay: &Layout, screen: &[u8], size: (u32, u32)) -> Option<Pixmap> {
    let mut out = ch.under.clone();
    let src = tiny_skia::PixmapRef::from_bytes(screen, size.0, size.1)?;
    let s = lay.screen;
    let tf = Transform::from_row(s.width() / size.0 as f32, 0.0, 0.0, s.height() / size.1 as f32, s.left(), s.top());
    let paint = Paint { shader: Pattern::new(src, SpreadMode::Pad, FilterQuality::Bilinear, 1.0, tf), anti_alias: true, ..Default::default() };
    out.fill_rect(tiny_skia::Rect::from_xywh(s.left(), s.top(), s.width(), s.height())?, &paint, Transform::identity(), None);
    out.draw_pixmap(0, 0, ch.over.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    Some(out)
}

fn rounded(r: egui::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let k = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    if k <= 0.0 {
        return tiny_skia::Rect::from_xywh(r.left(), r.top(), r.width(), r.height()).map(PathBuilder::from_rect);
    }
    let (x0, y0, x1, y1) = (r.left(), r.top(), r.right(), r.bottom());
    let c = k * 0.4477; // circle-ish corners with cubics
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + k, y0);
    pb.line_to(x1 - k, y0);
    pb.cubic_to(x1 - c, y0, x1, y0 + c, x1, y0 + k);
    pb.line_to(x1, y1 - k);
    pb.cubic_to(x1, y1 - c, x1 - c, y1, x1 - k, y1);
    pb.line_to(x0 + k, y1);
    pb.cubic_to(x0 + c, y1, x0, y1 - c, x0, y1 - k);
    pb.line_to(x0, y0 + k);
    pb.cubic_to(x0, y0 + c, x0 + c, y0, x0 + k, y0);
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_keep_the_size_and_fit_the_frame() {
        let plain = layout((1920, 1080), None);
        assert_eq!(plain.out, (1920, 1080));
        let f = Frame { ratio: render::Ratio::Square, ..Frame::default() };
        let l = layout((1920, 1080), Some(&f));
        assert_eq!(l.out, (1920, 1920));
        assert!(l.screen.left() > 0.0 && l.screen.right() < 1920.0);
        assert!((l.screen.width() / l.screen.height() - 16.0 / 9.0).abs() < 0.01, "the picture keeps its shape");
        assert!([l.screen.min.x, l.screen.min.y, l.screen.max.x, l.screen.max.y].iter().all(|v| v.fract() == 0.0), "on whole pixels");
        let odd = layout((1001, 667), None);
        assert_eq!((odd.out.0 % 2, odd.out.1 % 2), (0, 0));
    }

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
        c.iter().copied().cycle().take((w * h * 4) as usize).collect()
    }

    fn px(pm: &Pixmap, x: f32, y: f32) -> [u8; 4] {
        let i = ((y as u32 * pm.width() + x as u32) * 4) as usize;
        pm.data()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn the_still_parts_leave_a_round_hole_for_the_picture() {
        let f = Frame::default();
        let lay = layout((400, 300), Some(&f));
        let ch = chrome(&f, &lay).expect("chrome");
        assert_eq!((ch.under.width(), ch.under.height()), lay.out);
        let s = lay.screen;
        // the backdrop's corner, in BGRA, the same in both
        let (c0, _) = backdrop_colors(&f);
        assert_eq!(px(&ch.under, 0.0, 0.0), [c0[2], c0[1], c0[0], 255]);
        assert_eq!(px(&ch.over, 0.0, 0.0), px(&ch.under, 0.0, 0.0));
        // a hole in the middle, the backdrop in the round corner
        assert_eq!(px(&ch.over, s.center().x, s.center().y)[3], 0);
        assert_eq!(px(&ch.over, s.left() + 1.0, s.top() + 1.0)[3], 255, "the corner is cut round");
        assert_eq!(px(&ch.under, s.center().x, s.center().y)[3], 255, "under is solid");
    }

    #[test]
    fn a_framed_frame_has_the_picture_inside_and_the_backdrop_around() {
        let f = Frame::default();
        let lay = layout((200, 100), Some(&f));
        let ch = chrome(&f, &lay).unwrap();
        let screen = solid(200, 100, [10, 20, 30, 255]);
        let out = compose_cpu(&ch, &lay, &screen, (200, 100)).unwrap();
        let mid = lay.screen.center();
        assert_eq!(px(&out, mid.x, mid.y), [10, 20, 30, 255], "the picture inside");
        assert_eq!(px(&out, 0.0, 0.0), px(&ch.under, 0.0, 0.0), "the backdrop around");
        // no dark seam where the picture meets the backdrop
        let edge = px(&out, lay.screen.center().x, lay.screen.top() - 0.5);
        assert!(edge[3] == 255, "opaque at the edge");
    }

    #[test]
    fn square_corners_need_no_cut() {
        let f = Frame { round: false, shadow: false, ..Frame::default() };
        let lay = layout((200, 100), Some(&f));
        assert_eq!(lay.radius, 0.0);
        assert!(lay.shadow.is_none());
        let ch = chrome(&f, &lay).unwrap();
        let s = lay.screen;
        assert_eq!(px(&ch.over, s.left() + 1.0, s.top() + 1.0)[3], 0);
    }
}
