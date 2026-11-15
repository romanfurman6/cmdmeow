//! The editor's marks and how they're burned into the final image. Pure
//! pixel work, no UI: pixel effects first (blur, eraser), then shapes and text
//! drawn with anti-aliasing, then the crop. Coordinates are image pixels.
//! Also the pixel readers behind the color picker and the ruler.

use super::settings::BlurStyle;
use egui::{Pos2, Rect, Vec2};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

pub type Rgba8 = [u8; 4];

/// Which ends of an arrow get a head.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Heads {
    /// One head, at the end the drag finished on.
    #[default]
    End,
    Both,
    /// A plain line.
    None,
}

impl Heads {
    pub const ALL: [Heads; 3] = [Heads::End, Heads::Both, Heads::None];
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Arrow { a: Pos2, b: Pos2, color: Rgba8, width: f32, heads: Heads },
    Rect { r: Rect, color: Rgba8, width: f32 },
    /// An ellipse outline inside `r`.
    Ellipse { r: Rect, color: Rgba8, width: f32 },
    /// A freehand line through `pts`.
    Pen { pts: Vec<Pos2>, color: Rgba8, width: f32 },
    /// A label: text on a filled rounded box of `color`.
    Text { at: Pos2, text: String, color: Rgba8, size: f32 },
    Highlight { r: Rect, color: Rgba8 },
    /// `boxes` empty = the whole `r`; otherwise only these (the text OCR found).
    Blur { r: Rect, style: BlurStyle, boxes: Vec<Rect> },
    Erase { r: Rect, boxes: Vec<Rect> },
    Step { at: Pos2, n: u32, color: Rgba8, size: f32 },
}

impl Mark {
    /// The area it covers, for hit tests and the selection outline.
    pub fn bounds(&self) -> Rect {
        match self {
            Mark::Arrow { a, b, width, .. } => Rect::from_two_pos(*a, *b).expand(*width * 2.0),
            Mark::Rect { r, width, .. } | Mark::Ellipse { r, width, .. } => r.expand(*width / 2.0),
            Mark::Pen { pts, width, .. } => {
                let mut b = Rect::NOTHING;
                for q in pts {
                    b.extend_with(*q);
                }
                b.expand(*width / 2.0)
            }
            Mark::Highlight { r, .. } | Mark::Blur { r, .. } | Mark::Erase { r, .. } => *r,
            Mark::Text { at, text, size, .. } => {
                let s = label_size(text, *size);
                Rect::from_min_size(*at, s)
            }
            Mark::Step { at, size, .. } => Rect::from_center_size(*at, Vec2::splat(*size)),
        }
    }

    /// Distance-based hit test for selecting a mark.
    pub fn hit(&self, p: Pos2, slack: f32) -> bool {
        match self {
            Mark::Arrow { a, b, width, .. } => segment_distance(p, *a, *b) <= width + slack,
            Mark::Rect { r, width, .. } => {
                let outer = r.expand(width / 2.0 + slack);
                let inner = r.shrink(width / 2.0 + slack);
                outer.contains(p) && (!inner.is_positive() || !inner.contains(p))
            }
            Mark::Ellipse { r, width, .. } => {
                // distance to the nearest point of the outline (sampled
                // finely enough for any on-screen size)
                let c = r.center();
                let (a, b) = (r.width().abs() / 2.0, r.height().abs() / 2.0);
                if a < 1.0 || b < 1.0 {
                    return self.bounds().expand(slack).contains(p);
                }
                let n = ((a.max(b) * 0.5) as usize).clamp(64, 720);
                let near = (0..n)
                    .map(|i| {
                        let th = i as f32 / n as f32 * std::f32::consts::TAU;
                        (c + Vec2::new(a * th.cos(), b * th.sin()) - p).length()
                    })
                    .fold(f32::MAX, f32::min);
                near <= width / 2.0 + slack
            }
            Mark::Pen { pts, width, .. } => match pts.len() {
                0 => false,
                1 => (p - pts[0]).length() <= width + slack,
                _ => pts.windows(2).any(|w| segment_distance(p, w[0], w[1]) <= width / 2.0 + slack),
            },
            _ => self.bounds().expand(slack).contains(p),
        }
    }

    /// Resize handles, in image px: two ends for an arrow, eight around a
    /// box (corners and edge midpoints, clockwise from the top left). Marks
    /// that only move have none.
    pub fn handles(&self) -> Vec<Pos2> {
        match self {
            Mark::Arrow { a, b, .. } => vec![*a, *b],
            Mark::Rect { r, .. } | Mark::Ellipse { r, .. } | Mark::Highlight { r, .. } => box_handles(*r).to_vec(),
            Mark::Blur { r, boxes, .. } | Mark::Erase { r, boxes } if boxes.is_empty() => box_handles(*r).to_vec(),
            _ => Vec::new(),
        }
    }

    /// Move handle `k` (from [`Mark::handles`]) to `to`.
    pub fn drag_handle(&mut self, k: usize, to: Pos2) {
        match self {
            Mark::Arrow { a, b, .. } => {
                if k == 0 {
                    *a = to;
                } else {
                    *b = to;
                }
            }
            Mark::Rect { r, .. } | Mark::Ellipse { r, .. } | Mark::Highlight { r, .. } | Mark::Blur { r, .. } | Mark::Erase { r, .. } => *r = resize_box(*r, k, to),
            _ => {}
        }
    }

    pub fn translate(&mut self, d: Vec2) {
        match self {
            Mark::Arrow { a, b, .. } => {
                *a += d;
                *b += d;
            }
            Mark::Rect { r, .. } | Mark::Ellipse { r, .. } | Mark::Highlight { r, .. } => *r = r.translate(d),
            Mark::Pen { pts, .. } => {
                for q in pts {
                    *q += d;
                }
            }
            Mark::Blur { r, boxes, .. } | Mark::Erase { r, boxes } => {
                *r = r.translate(d);
                for b in boxes {
                    *b = b.translate(d);
                }
            }
            Mark::Text { at, .. } | Mark::Step { at, .. } => *at += d,
        }
    }

    /// Pixel effects are applied under every shape.
    pub fn is_effect(&self) -> bool {
        matches!(self, Mark::Blur { .. } | Mark::Erase { .. })
    }
}

/// `r` with handle `k` (from [`box_handles`]) moved to `to`.
pub fn resize_box(r: Rect, k: usize, to: Pos2) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    match k {
        0 => (x0, y0) = (to.x, to.y),
        1 => y0 = to.y,
        2 => (x1, y0) = (to.x, to.y),
        3 => x1 = to.x,
        4 => (x1, y1) = (to.x, to.y),
        5 => y1 = to.y,
        6 => (x0, y1) = (to.x, to.y),
        _ => x0 = to.x,
    }
    Rect::from_two_pos(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

/// Corners and edge midpoints, clockwise from the top left.
pub fn box_handles(r: Rect) -> [Pos2; 8] {
    let c = r.center();
    [r.left_top(), Pos2::new(c.x, r.top()), r.right_top(), Pos2::new(r.right(), c.y), r.right_bottom(), Pos2::new(c.x, r.bottom()), r.left_bottom(), Pos2::new(r.left(), c.y)]
}

/// Everything the editor knows about one capture.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub marks: Vec<Mark>,
    pub crop: Option<Rect>,
    /// The backdrop around the picture, if on.
    pub frame: Option<Frame>,
    /// Image pixels per point (2 on Retina): sizes the frame's corners and shadow.
    pub ppp: f32,
}

impl Default for Doc {
    fn default() -> Doc {
        Doc { marks: Vec::new(), crop: None, frame: None, ppp: 1.0 }
    }
}

// ---------------------------------------------------------------- frame

/// Backdrops for the frame: name, top-left color, bottom-right color.
pub const BACKDROPS: [(&str, Rgba8, Rgba8); 6] = [
    ("Moss", [0x3A, 0x6B, 0x2F, 255], [0x0B, 0x17, 0x0A, 255]),
    ("Lime", [0xE2, 0xFA, 0x9C, 255], [0x7D, 0xB5, 0x19, 255]),
    ("Apricot", [0xFC, 0xD9, 0xA6, 255], [0xE0, 0x72, 0x2E, 255]),
    ("Sky", [0xB4, 0xEE, 0xF6, 255], [0x1F, 0x7A, 0xB0, 255]),
    ("Lavender", [0xF3, 0xEF, 0xFF, 255], [0x86, 0x70, 0xDC, 255]),
    ("Graphite", [0x55, 0x5B, 0x61, 255], [0x12, 0x15, 0x17, 255]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ratio {
    /// Just the padding around the picture.
    Auto,
    Wide,
    Classic,
    Square,
}

impl Ratio {
    pub const ALL: [Ratio; 4] = [Ratio::Auto, Ratio::Wide, Ratio::Classic, Ratio::Square];

    pub fn label(self) -> &'static str {
        match self {
            Ratio::Auto => "Auto",
            Ratio::Wide => "16:9",
            Ratio::Classic => "4:3",
            Ratio::Square => "1:1",
        }
    }

    fn value(self) -> Option<f32> {
        match self {
            Ratio::Auto => None,
            Ratio::Wide => Some(16.0 / 9.0),
            Ratio::Classic => Some(4.0 / 3.0),
            Ratio::Square => Some(1.0),
        }
    }
}

/// The picture on a gradient backdrop with padding, round corners and a
/// soft shadow. The editor remembers the last one used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    /// Index into [`BACKDROPS`].
    pub bg: usize,
    /// Padding: 0 small, 1 medium, 2 large.
    pub pad: usize,
    pub round: bool,
    pub shadow: bool,
    pub ratio: Ratio,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame { bg: 0, pad: 1, round: true, shadow: true, ratio: Ratio::Auto }
    }
}

/// Padding per size, as a share of the picture's longer side.
const FRAME_PAD: [f32; 3] = [0.04, 0.08, 0.13];

/// Where things go in a framed picture, in image px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGeom {
    /// The whole framed image.
    pub size: Vec2,
    /// The picture inside it.
    pub inner: Rect,
    pub radius: f32,
    /// Shadow blur and how far down it falls; None without a shadow.
    pub shadow: Option<(f32, f32)>,
}

pub fn frame_geom(w: f32, h: f32, f: &Frame, ppp: f32) -> FrameGeom {
    let ppp = ppp.max(1.0);
    let pad = (FRAME_PAD[f.pad.min(2)] * w.max(h)).max(8.0 * ppp).round();
    let (mut ow, mut oh) = (w + 2.0 * pad, h + 2.0 * pad);
    if let Some(r) = f.ratio.value() {
        if ow / oh < r {
            ow = (oh * r).round();
        } else {
            oh = (ow / r).round();
        }
    }
    let inner = Rect::from_min_size(Pos2::new(((ow - w) / 2.0).round(), ((oh - h) / 2.0).round()), Vec2::new(w, h));
    let radius = if f.round { (12.0 * ppp).min(w.min(h) / 4.0) } else { 0.0 };
    let shadow = f.shadow.then(|| ((24.0 * ppp).min(pad * 0.9), (8.0 * ppp).min(pad * 0.4)));
    FrameGeom { size: Vec2::new(ow, oh), inner, radius, shadow }
}

/// `img` on its frame.
pub fn framed(img: &RgbaImage, f: &Frame, ppp: f32) -> RgbaImage {
    use tiny_skia::{Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, Pattern, Pixmap, Point, SpreadMode, Transform};
    let (w, h) = img.dimensions();
    let g = frame_geom(w as f32, h as f32, f, ppp);
    let (ow, oh) = (g.size.x as u32, g.size.y as u32);
    let Some(mut pm) = Pixmap::new(ow, oh) else { return img.clone() };
    let t = Transform::identity();
    let (_, c0, c1) = BACKDROPS[f.bg.min(BACKDROPS.len() - 1)];
    let rgba = |c: Rgba8| Color::from_rgba8(c[0], c[1], c[2], c[3]);
    if let Some(shader) = LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(ow as f32, oh as f32), vec![GradientStop::new(0.0, rgba(c0)), GradientStop::new(1.0, rgba(c1))], SpreadMode::Pad, t) {
        let paint = Paint { shader, anti_alias: false, ..Default::default() };
        if let Some(r) = tiny_skia::Rect::from_xywh(0.0, 0.0, ow as f32, oh as f32) {
            pm.fill_rect(r, &paint, t, None);
        }
    }
    let r = g.inner;
    if let Some((blur, dy)) = g.shadow {
        soft_shadow(&mut pm, r.translate(Vec2::new(0.0, dy)), g.radius, blur);
    }
    let src = to_pixmap(img);
    if let (Some(src), Some(path)) = (src, rounded(r, g.radius)) {
        let shader = Pattern::new(src.as_ref(), SpreadMode::Pad, FilterQuality::Nearest, 1.0, Transform::from_translate(r.min.x, r.min.y));
        let paint = Paint { shader, anti_alias: true, ..Default::default() };
        pm.fill_path(&path, &paint, FillRule::Winding, t, None);
    }
    let mut out = RgbaImage::new(ow, oh);
    from_pixmap(&pm, &mut out);
    out
}

/// A soft dark shadow of the rounded rect `r` drawn onto `pm`: drawn at a
/// quarter size, blurred, then scaled up (soft and cheap).
pub fn soft_shadow(pm: &mut tiny_skia::Pixmap, r: Rect, radius: f32, blur: f32) {
    use tiny_skia::{FillRule, FilterQuality, Paint, Pixmap, PixmapPaint, Transform};
    let k = 4.0;
    let (ow, oh) = (pm.width(), pm.height());
    let Some(mut sm) = Pixmap::new((ow as f32 / k).ceil() as u32 + 1, (oh as f32 / k).ceil() as u32 + 1) else { return };
    let sr = Rect::from_min_size(Pos2::new(r.min.x / k, r.min.y / k), r.size() / k);
    if let Some(path) = rounded(sr, radius / k) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 120);
        paint.anti_alias = true;
        sm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
    let (sw, sh) = (sm.width() as usize, sm.height() as usize);
    blur_alpha(sm.data_mut(), sw, sh, (blur / k / 2.0).round().max(1.0) as usize);
    let pp = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
    pm.draw_pixmap(0, 0, sm.as_ref(), &pp, Transform::from_scale(k, k), None);
}

/// Three box-blur passes over a black-only premultiplied buffer's alpha.
fn blur_alpha(data: &mut [u8], w: usize, h: usize, radius: usize) {
    let mut a: Vec<f32> = data.chunks_exact(4).map(|p| p[3] as f32).collect();
    let mut line = vec![0f32; w.max(h)];
    for _ in 0..3 {
        for pass in 0..2 {
            let (outer, inner) = if pass == 0 { (h, w) } else { (w, h) };
            let at = |o: usize, i: usize| if pass == 0 { o * w + i } else { i * w + o };
            let n = (2 * radius + 1) as f32;
            for o in 0..outer {
                // running sum with clamped edges
                let mut sum = 0f32;
                for d in 0..=2 * radius {
                    sum += a[at(o, d.saturating_sub(radius).min(inner - 1))];
                }
                for i in 0..inner {
                    line[i] = sum / n;
                    let out = a[at(o, i.saturating_sub(radius))];
                    let inn = a[at(o, (i + radius + 1).min(inner - 1))];
                    sum += inn - out;
                }
                for i in 0..inner {
                    a[at(o, i)] = line[i];
                }
            }
        }
    }
    for (p, v) in data.chunks_exact_mut(4).zip(a) {
        p.copy_from_slice(&[0, 0, 0, v.round().clamp(0.0, 255.0) as u8]);
    }
}

fn to_pixmap(img: &RgbaImage) -> Option<tiny_skia::Pixmap> {
    let mut pm = tiny_skia::Pixmap::new(img.width(), img.height())?;
    // straight → premultiplied
    for (dst, src) in pm.data_mut().chunks_exact_mut(4).zip(img.as_raw().chunks_exact(4)) {
        let a = src[3] as u32;
        dst[0] = (src[0] as u32 * a / 255) as u8;
        dst[1] = (src[1] as u32 * a / 255) as u8;
        dst[2] = (src[2] as u32 * a / 255) as u8;
        dst[3] = src[3];
    }
    Some(pm)
}

fn from_pixmap(pm: &tiny_skia::Pixmap, img: &mut RgbaImage) {
    // premultiplied → straight
    for (dst, src) in img.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
        let a = src[3] as u32;
        if a == 0 {
            dst.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            dst[0] = (src[0] as u32 * 255 / a).min(255) as u8;
            dst[1] = (src[1] as u32 * 255 / a).min(255) as u8;
            dst[2] = (src[2] as u32 * 255 / a).min(255) as u8;
            dst[3] = src[3];
        }
    }
}

impl Doc {
    /// The next number for the Steps tool.
    pub fn next_step(&self) -> u32 {
        self.marks.iter().filter_map(|m| if let Mark::Step { n, .. } = m { Some(*n) } else { None }).max().unwrap_or(0) + 1
    }
}

/// The finished image: effects, shapes, the crop, then the frame.
pub fn render(base: &RgbaImage, doc: &Doc) -> RgbaImage {
    let mut img = apply_effects(base, &doc.marks);
    draw_shapes(&mut img, &doc.marks);
    let img = match doc.crop.and_then(|c| clamp_rect(c, img.width(), img.height())) {
        Some((x, y, w, h)) => image::imageops::crop_imm(&img, x, y, w, h).to_image(),
        None => img,
    };
    match &doc.frame {
        Some(f) => framed(&img, f, doc.ppp),
        None => img,
    }
}

/// Only the pixel effects (Blur, Eraser): what the editor shows under its
/// live shapes.
pub fn apply_effects(base: &RgbaImage, marks: &[Mark]) -> RgbaImage {
    let mut img = base.clone();
    for m in marks {
        match m {
            Mark::Blur { r, style, boxes } => {
                for b in targets(*r, boxes) {
                    match style {
                        BlurStyle::Pixelate => pixelate(&mut img, b),
                        BlurStyle::Blur => box_blur(&mut img, b),
                        BlurStyle::Solid => fill(&mut img, b, [28, 28, 31, 255]),
                    }
                }
            }
            Mark::Erase { r, boxes } => {
                for b in targets(*r, boxes) {
                    let c = surrounding_color(&img, b);
                    fill(&mut img, b, c);
                }
            }
            _ => {}
        }
    }
    img
}

fn targets(r: Rect, boxes: &[Rect]) -> Vec<Rect> {
    if boxes.is_empty() {
        vec![r]
    } else {
        // a little air around each word so no edge pixels survive
        boxes.iter().map(|b| b.expand(2.0)).collect()
    }
}

/// A rect clamped to the image, as integer x, y, w, h. None if empty.
pub fn clamp_rect(r: Rect, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = r.min.x.min(r.max.x).floor().clamp(0.0, w as f32) as u32;
    let y0 = r.min.y.min(r.max.y).floor().clamp(0.0, h as f32) as u32;
    let x1 = r.min.x.max(r.max.x).ceil().clamp(0.0, w as f32) as u32;
    let y1 = r.min.y.max(r.max.y).ceil().clamp(0.0, h as f32) as u32;
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

fn fill(img: &mut RgbaImage, r: Rect, c: Rgba8) {
    let Some((x, y, w, h)) = clamp_rect(r, img.width(), img.height()) else { return };
    for yy in y..y + h {
        for xx in x..x + w {
            img.put_pixel(xx, yy, Rgba(c));
        }
    }
}

/// Big square blocks, each the average of what it covers.
fn pixelate(img: &mut RgbaImage, r: Rect) {
    let Some((x, y, w, h)) = clamp_rect(r, img.width(), img.height()) else { return };
    let block = (w.min(h) / 3).clamp(6, 24);
    let mut by = y;
    while by < y + h {
        let bh = block.min(y + h - by);
        let mut bx = x;
        while bx < x + w {
            let bw = block.min(x + w - bx);
            let mut sum = [0u64; 4];
            for yy in by..by + bh {
                for xx in bx..bx + bw {
                    let p = img.get_pixel(xx, yy).0;
                    for k in 0..4 {
                        sum[k] += p[k] as u64;
                    }
                }
            }
            let n = (bw * bh) as u64;
            let avg = Rgba([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, (sum[3] / n) as u8]);
            for yy in by..by + bh {
                for xx in bx..bx + bw {
                    img.put_pixel(xx, yy, avg);
                }
            }
            bx += bw;
        }
        by += bh;
    }
}

/// Three box-blur passes inside the rect (close to a Gaussian).
fn box_blur(img: &mut RgbaImage, r: Rect) {
    let Some((x, y, w, h)) = clamp_rect(r, img.width(), img.height()) else { return };
    let radius = (w.min(h) / 4).clamp(4, 16) as i64;
    let mut buf: Vec<[f32; 4]> = Vec::with_capacity((w * h) as usize);
    for yy in y..y + h {
        for xx in x..x + w {
            let p = img.get_pixel(xx, yy).0;
            buf.push([p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]);
        }
    }
    let (w, h) = (w as i64, h as i64);
    for _ in 0..3 {
        // horizontal then vertical running averages, edges clamped
        for pass in 0..2 {
            let (outer, inner) = if pass == 0 { (h, w) } else { (w, h) };
            let at = |o: i64, i: i64| if pass == 0 { (o * w + i) as usize } else { (i * w + o) as usize };
            let mut line = vec![[0f32; 4]; inner as usize];
            for o in 0..outer {
                for i in 0..inner {
                    let mut s = [0f32; 4];
                    for d in -radius..=radius {
                        let p = buf[at(o, (i + d).clamp(0, inner - 1))];
                        for k in 0..4 {
                            s[k] += p[k];
                        }
                    }
                    let n = (2 * radius + 1) as f32;
                    line[i as usize] = [s[0] / n, s[1] / n, s[2] / n, s[3] / n];
                }
                for i in 0..inner {
                    buf[at(o, i)] = line[i as usize];
                }
            }
        }
    }
    for yy in 0..h {
        for xx in 0..w {
            let p = buf[(yy * w + xx) as usize];
            img.put_pixel(x + xx as u32, y + yy as u32, Rgba([p[0] as u8, p[1] as u8, p[2] as u8, p[3] as u8]));
        }
    }
}

/// The background around a rect: the per-channel median of the ring of
/// pixels just outside it. That's what the Eraser fills with.
pub fn surrounding_color(img: &RgbaImage, r: Rect) -> Rgba8 {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let x0 = r.min.x.floor() as i64 - 1;
    let y0 = r.min.y.floor() as i64 - 1;
    let x1 = r.max.x.ceil() as i64;
    let y1 = r.max.y.ceil() as i64;
    let mut ring: Vec<Rgba8> = Vec::new();
    let mut take = |x: i64, y: i64| {
        if x >= 0 && y >= 0 && x < w && y < h {
            ring.push(img.get_pixel(x as u32, y as u32).0);
        }
    };
    for x in x0..=x1 {
        take(x, y0);
        take(x, y1);
    }
    for y in y0 + 1..y1 {
        take(x0, y);
        take(x1, y);
    }
    if ring.is_empty() {
        return [255, 255, 255, 255];
    }
    let mut out = [0u8; 4];
    for (k, o) in out.iter_mut().enumerate() {
        let mut ch: Vec<u8> = ring.iter().map(|p| p[k]).collect();
        ch.sort_unstable();
        *o = ch[ch.len() / 2];
    }
    out
}

// ---------------------------------------------------------------- shapes

/// The arrow head's three corners for a shaft from `a` to `b`.
pub fn arrow_head(a: Pos2, b: Pos2, width: f32) -> [Pos2; 3] {
    let d = (b - a).normalized();
    let d = if d.is_finite() { d } else { Vec2::X };
    let n = Vec2::new(-d.y, d.x);
    let len = (width * 4.5).max(10.0);
    let base = b - d * len;
    [b, base + n * len * 0.55, base - n * len * 0.55]
}

/// Where the shaft stops so its round cap hides under the head.
pub fn shaft_end(a: Pos2, b: Pos2, width: f32) -> Pos2 {
    let d = (b - a).normalized();
    if !d.is_finite() {
        return b;
    }
    b - d * (width * 4.5).max(10.0) * 0.6
}

/// Padding of a text label's box, as a fraction of its font size.
const LABEL_PAD: f32 = 0.35;

/// Size of a text label box (text plus padding) at `size` px.
pub fn label_size(text: &str, size: f32) -> Vec2 {
    let (tw, th) = measure(text, size);
    Vec2::new(tw + 2.0 * LABEL_PAD * size, th + LABEL_PAD * size)
}

/// White on dark colors, near-black on light ones.
pub fn ink_on(c: Rgba8) -> Rgba8 {
    let l = 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
    if l > 160.0 {
        [28, 28, 31, 255]
    } else {
        [255, 255, 255, 255]
    }
}

fn draw_shapes(img: &mut RgbaImage, marks: &[Mark]) {
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Stroke, Transform};
    let (w, h) = img.dimensions();
    if !marks.iter().any(|m| !m.is_effect()) || w == 0 || h == 0 {
        return;
    }
    let Some(mut pm) = to_pixmap(img) else { return };
    let paint = |c: Rgba8| {
        let mut p = Paint::default();
        p.set_color_rgba8(c[0], c[1], c[2], c[3]);
        p.anti_alias = true;
        p
    };
    let stroke = |width: f32| Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    let t = Transform::identity();
    for m in marks {
        match m {
            Mark::Arrow { a, b, color, width, heads } => {
                let end = if *heads == Heads::None { *b } else { shaft_end(*a, *b, *width) };
                let start = if *heads == Heads::Both { shaft_end(*b, *a, *width) } else { *a };
                let mut pb = PathBuilder::new();
                pb.move_to(start.x, start.y);
                pb.line_to(end.x, end.y);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint(*color), &stroke(*width), t, None);
                }
                let tips: &[(Pos2, Pos2)] = match heads {
                    Heads::End => &[(*a, *b)],
                    Heads::Both => &[(*a, *b), (*b, *a)],
                    Heads::None => &[],
                };
                for (from, to) in tips {
                    let [p0, p1, p2] = arrow_head(*from, *to, *width);
                    let mut pb = PathBuilder::new();
                    pb.move_to(p0.x, p0.y);
                    pb.line_to(p1.x, p1.y);
                    pb.line_to(p2.x, p2.y);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pm.fill_path(&path, &paint(*color), FillRule::Winding, t, None);
                        pm.stroke_path(&path, &paint(*color), &stroke(width * 0.6), t, None);
                    }
                }
            }
            Mark::Rect { r, color, width } => {
                if let Some(path) = rounded(*r, width * 1.5) {
                    pm.stroke_path(&path, &paint(*color), &stroke(*width), t, None);
                }
            }
            Mark::Ellipse { r, color, width } => {
                let r = Rect::from_two_pos(r.min, r.max);
                if let Some(path) = tiny_skia::Rect::from_xywh(r.min.x, r.min.y, r.width(), r.height()).and_then(PathBuilder::from_oval) {
                    pm.stroke_path(&path, &paint(*color), &stroke(*width), t, None);
                }
            }
            Mark::Pen { pts, color, width } => {
                let mut pb = PathBuilder::new();
                if let Some(first) = pts.first() {
                    pb.move_to(first.x, first.y);
                    if pts.len() == 1 {
                        pb.line_to(first.x + 0.01, first.y);
                    }
                    // the same segments the editor draws and hits (dense samples
                    // with round joins look smooth)
                    for q in pts.iter().skip(1) {
                        pb.line_to(q.x, q.y);
                    }
                }
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint(*color), &stroke(*width), t, None);
                }
            }
            Mark::Highlight { r, color } => {
                if let Some(path) = rounded(*r, 2.0) {
                    pm.fill_path(&path, &paint([color[0], color[1], color[2], 110]), FillRule::Winding, t, None);
                }
            }
            Mark::Step { at, n, color, size } => {
                let mut pb = PathBuilder::new();
                pb.push_circle(at.x, at.y, size / 2.0);
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint(*color), FillRule::Winding, t, None);
                }
                let label = n.to_string();
                let (tw, th) = measure(&label, size * 0.55);
                glyphs(&mut pm, &label, Pos2::new(at.x - tw / 2.0, at.y - th / 2.0), size * 0.55, ink_on(*color));
            }
            Mark::Text { at, text, color, size } => {
                let s = label_size(text, *size);
                if let Some(path) = rounded(Rect::from_min_size(*at, s), size * 0.3) {
                    pm.fill_path(&path, &paint(*color), FillRule::Winding, t, None);
                }
                glyphs(&mut pm, text, *at + Vec2::new(LABEL_PAD * size, LABEL_PAD * size / 2.0), *size, ink_on(*color));
            }
            Mark::Blur { .. } | Mark::Erase { .. } => {}
        }
    }
    from_pixmap(&pm, img);
}

fn rounded(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let r = Rect::from_two_pos(r.min, r.max);
    let k = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let mut pb = tiny_skia::PathBuilder::new();
    let (x0, y0, x1, y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    pb.move_to(x0 + k, y0);
    pb.line_to(x1 - k, y0);
    pb.quad_to(x1, y0, x1, y0 + k);
    pb.line_to(x1, y1 - k);
    pb.quad_to(x1, y1, x1 - k, y1);
    pb.line_to(x0 + k, y1);
    pb.quad_to(x0, y1, x0, y1 - k);
    pb.line_to(x0, y0 + k);
    pb.quad_to(x0, y0, x0 + k, y0);
    pb.close();
    pb.finish()
}

// ---------------------------------------------------------------- text

/// A bold system font for labels in exported images.
fn label_font() -> Option<&'static ab_glyph::FontVec> {
    static FONT: std::sync::OnceLock<Option<ab_glyph::FontVec>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let candidates: [(&str, u32); 5] = [
            ("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 0),
            ("/System/Library/Fonts/Helvetica.ttc", 1),
            ("/System/Library/Fonts/SFNS.ttf", 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 0),
            ("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 0),
        ];
        candidates.iter().find_map(|(path, index)| {
            let bytes = std::fs::read(path).ok()?;
            ab_glyph::FontVec::try_from_vec_and_index(bytes, *index).ok()
        })
    })
    .as_ref()
}

/// Width and line height of `text` at `size` px. A rough estimate when no
/// font is available.
pub fn measure(text: &str, size: f32) -> (f32, f32) {
    use ab_glyph::{Font, ScaleFont};
    let Some(font) = label_font() else {
        return (text.chars().count() as f32 * size * 0.6, size * 1.2);
    };
    let sf = font.as_scaled(size);
    let mut w: f32 = 0.0;
    let mut prev = None;
    let mut widest: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        lines += 1;
        w = 0.0;
        for ch in line.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                w += sf.kern(p, id);
            }
            w += sf.h_advance(id);
            prev = Some(id);
        }
        widest = widest.max(w);
        prev = None;
    }
    let _ = w;
    (widest, sf.height() * lines.max(1) as f32)
}

/// Blend `text` into a premultiplied pixmap, top-left at `pos`.
fn glyphs(pm: &mut tiny_skia::Pixmap, text: &str, pos: Pos2, size: f32, color: Rgba8) {
    let (w, h) = (pm.width(), pm.height());
    glyphs_into(pm.data_mut(), w, h, text, pos, size, color);
}

/// Blend `text` into premultiplied 4-byte pixels (`w`×`h`, tight rows),
/// top-left at `pos`. The color's first three bytes go in as they are.
pub fn glyphs_into(data: &mut [u8], w: u32, h: u32, text: &str, pos: Pos2, size: f32, color: Rgba8) {
    use ab_glyph::{point, Font, ScaleFont};
    let Some(font) = label_font() else { return };
    let sf = font.as_scaled(size);
    let (w, h) = (w as i64, h as i64);
    let mut y = pos.y + sf.ascent();
    for line in text.split('\n') {
        let mut x = pos.x;
        let mut prev = None;
        for ch in line.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            let g = id.with_scale_and_position(size, point(x, y));
            x += sf.h_advance(id);
            prev = Some(id);
            let Some(outline) = font.outline_glyph(g) else { continue };
            let bb = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px = bb.min.x as i64 + gx as i64;
                let py = bb.min.y as i64 + gy as i64;
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let a = cov.clamp(0.0, 1.0) * color[3] as f32 / 255.0;
                let i = ((py * w + px) * 4) as usize;
                for k in 0..3 {
                    let s = color[k] as f32 * a;
                    data[i + k] = (s + data[i + k] as f32 * (1.0 - a)).round() as u8;
                }
                data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)).round() as u8;
            });
        }
        y += sf.height();
    }
}

// ---------------------------------------------------------------- picker and ruler

pub fn hex(c: Rgba8) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

pub fn pixel(img: &RgbaImage, x: i64, y: i64) -> Option<Rgba8> {
    (x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height()).then(|| img.get_pixel(x as u32, y as u32).0)
}

fn close(a: Rgba8, b: Rgba8, tol: i32) -> bool {
    (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= tol)
}

/// The ruler: from (x, y), how far the same color runs left, right, up and
/// down before an edge. Returns the span (x0, x1, y0, y1), inclusive.
pub fn edge_span(img: &RgbaImage, x: i64, y: i64, tol: i32) -> Option<(i64, i64, i64, i64)> {
    let c = pixel(img, x, y)?;
    let run = |dx: i64, dy: i64| {
        let (mut cx, mut cy) = (x, y);
        while let Some(p) = pixel(img, cx + dx, cy + dy) {
            if !close(p, c, tol) {
                break;
            }
            cx += dx;
            cy += dy;
        }
        (cx, cy)
    };
    let (x0, _) = run(-1, 0);
    let (x1, _) = run(1, 0);
    let (_, y0) = run(0, -1);
    let (_, y1) = run(0, 1);
    Some((x0, x1, y0, y1))
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> RgbaImage {
        RgbaImage::from_pixel(120, 80, Rgba([240, 240, 240, 255]))
    }

    #[test]
    fn arrow_and_rect_paint_their_color() {
        let mut d = Doc::default();
        d.marks.push(Mark::Arrow { a: Pos2::new(10.0, 40.0), b: Pos2::new(110.0, 40.0), color: [255, 0, 0, 255], width: 4.0, heads: Heads::End });
        d.marks.push(Mark::Rect { r: Rect::from_min_max(Pos2::new(20.0, 5.0), Pos2::new(60.0, 25.0)), color: [0, 0, 255, 255], width: 4.0 });
        let out = render(&canvas(), &d);
        assert_eq!(out.get_pixel(50, 40).0, [255, 0, 0, 255], "shaft");
        assert_eq!(out.get_pixel(108, 40).0[0], 255, "head reaches the tip");
        assert_eq!(out.get_pixel(40, 5).0, [0, 0, 255, 255], "rect edge");
        assert_eq!(out.get_pixel(40, 15).0, [240, 240, 240, 255], "rect inside stays");
    }

    #[test]
    fn arrow_heads_go_on_the_chosen_ends() {
        // 6 px off the shaft, inside a head (heads are ~10 px wide each side)
        let red_near = |heads: Heads, x: u32| {
            let mut d = Doc::default();
            d.marks.push(Mark::Arrow { a: Pos2::new(10.0, 40.0), b: Pos2::new(110.0, 40.0), color: [255, 0, 0, 255], width: 4.0, heads });
            render(&canvas(), &d).get_pixel(x, 46).0 == [255, 0, 0, 255]
        };
        assert!(red_near(Heads::End, 96) && !red_near(Heads::End, 24));
        assert!(red_near(Heads::Both, 96) && red_near(Heads::Both, 24));
        assert!(!red_near(Heads::None, 96) && !red_near(Heads::None, 24));
    }

    #[test]
    fn pixelate_and_blur_hide_detail_only_inside() {
        let mut base = canvas();
        for x in 0..120 {
            for y in 0..80 {
                if (x + y) % 2 == 0 {
                    base.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        let r = Rect::from_min_max(Pos2::new(30.0, 20.0), Pos2::new(90.0, 60.0));
        for style in [BlurStyle::Pixelate, BlurStyle::Blur] {
            let out = apply_effects(&base, &[Mark::Blur { r, style, boxes: vec![] }]);
            let a = out.get_pixel(50, 30).0[0] as i32;
            let b = out.get_pixel(51, 30).0[0] as i32;
            assert!((a - b).abs() < 40, "{style:?}: checkerboard is smoothed inside ({a} vs {b})");
            assert_eq!(out.get_pixel(5, 5), base.get_pixel(5, 5), "{style:?}: outside untouched");
        }
        let solid = apply_effects(&base, &[Mark::Blur { r, style: BlurStyle::Solid, boxes: vec![] }]);
        assert_eq!(solid.get_pixel(60, 40).0, [28, 28, 31, 255]);
    }

    #[test]
    fn eraser_fills_with_the_background_and_respects_text_boxes() {
        let mut base = RgbaImage::from_pixel(100, 40, Rgba([10, 120, 200, 255]));
        // "text": dark pixels in two words
        for (x0, x1) in [(10u32, 30u32), (50, 70)] {
            for x in x0..x1 {
                for y in 15..25 {
                    base.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        let r = Rect::from_min_max(Pos2::new(5.0, 10.0), Pos2::new(40.0, 30.0));
        let out = apply_effects(&base, &[Mark::Erase { r, boxes: vec![] }]);
        assert_eq!(out.get_pixel(20, 20).0, [10, 120, 200, 255]);
        assert_eq!(out.get_pixel(60, 20).0, [0, 0, 0, 255], "outside the box stays");
        // only-text: the second word's box, nothing else
        let word = Rect::from_min_max(Pos2::new(50.0, 15.0), Pos2::new(70.0, 25.0));
        let out = apply_effects(&base, &[Mark::Erase { r: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 40.0)), boxes: vec![word] }]);
        assert_eq!(out.get_pixel(60, 20).0, [10, 120, 200, 255]);
        assert_eq!(out.get_pixel(20, 20).0, [0, 0, 0, 255], "first word wasn't in the boxes");
    }

    #[test]
    fn crop_happens_last_and_clamps() {
        let mut d = Doc::default();
        d.crop = Some(Rect::from_min_max(Pos2::new(100.0, 70.0), Pos2::new(500.0, 500.0)));
        assert_eq!(render(&canvas(), &d).dimensions(), (20, 10));
        d.crop = Some(Rect::from_min_max(Pos2::new(200.0, 200.0), Pos2::new(300.0, 300.0)));
        assert_eq!(render(&canvas(), &d).dimensions(), (120, 80), "empty crop is ignored");
    }

    #[test]
    fn labels_and_steps_draw_a_filled_box() {
        let mut d = Doc::default();
        d.marks.push(Mark::Text { at: Pos2::new(5.0, 5.0), text: "Hi".into(), color: [255, 69, 58, 255], size: 20.0 });
        d.marks.push(Mark::Step { at: Pos2::new(90.0, 60.0), n: d.next_step(), color: [255, 69, 58, 255], size: 26.0 });
        assert_eq!(d.next_step(), 2);
        let out = render(&canvas(), &d);
        assert_eq!(out.get_pixel(6, 7).0[1], 69, "label box corner area is the label color");
        assert_eq!(out.get_pixel(90, 50).0[1], 69, "step circle");
    }

    #[test]
    fn ellipse_and_pen_paint_and_hit() {
        let mut d = Doc::default();
        let r = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(110.0, 70.0));
        d.marks.push(Mark::Ellipse { r, color: [255, 0, 0, 255], width: 4.0 });
        d.marks.push(Mark::Pen { pts: vec![Pos2::new(5.0, 75.0), Pos2::new(60.0, 76.0), Pos2::new(115.0, 75.0)], color: [0, 0, 255, 255], width: 4.0 });
        let out = render(&canvas(), &d);
        assert_eq!(out.get_pixel(60, 10).0, [255, 0, 0, 255], "ellipse top");
        assert_eq!(out.get_pixel(60, 40).0, [240, 240, 240, 255], "ellipse inside stays");
        assert_eq!(out.get_pixel(60, 75).0[2], 255, "pen line");
        assert!(d.marks[0].hit(Pos2::new(10.0, 40.0), 2.0));
        assert!(!d.marks[0].hit(Pos2::new(60.0, 40.0), 2.0), "hollow inside");
        assert!(d.marks[1].hit(Pos2::new(80.0, 76.0), 2.0));
        assert!(!d.marks[1].hit(Pos2::new(80.0, 40.0), 2.0));
    }

    #[test]
    fn handles_resize_boxes_and_arrows() {
        let mut m = Mark::Rect { r: Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(50.0, 30.0)), color: [0; 4], width: 2.0 };
        assert_eq!(m.handles().len(), 8);
        m.drag_handle(4, Pos2::new(80.0, 60.0));
        assert_eq!(m.bounds(), Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(80.0, 60.0)).expand(1.0));
        // dragging past the opposite edge flips instead of going negative
        m.drag_handle(3, Pos2::new(0.0, 0.0));
        assert!(matches!(m, Mark::Rect { r, .. } if r.min.x == 0.0 && r.max.x == 10.0));
        let mut a = Mark::Arrow { a: Pos2::ZERO, b: Pos2::new(10.0, 0.0), color: [0; 4], width: 2.0, heads: Heads::End };
        a.drag_handle(1, Pos2::new(5.0, 5.0));
        assert!(matches!(a, Mark::Arrow { b, .. } if b == Pos2::new(5.0, 5.0)));
        let text_blur = Mark::Blur { r: Rect::from_min_max(Pos2::ZERO, Pos2::new(9.0, 9.0)), style: BlurStyle::Blur, boxes: vec![Rect::ZERO] };
        assert!(text_blur.handles().is_empty(), "only-text covers can't be resized");
    }

    #[test]
    fn frame_pads_rounds_and_keeps_the_picture() {
        let img = RgbaImage::from_pixel(200, 100, Rgba([255, 0, 0, 255]));
        let f = Frame { bg: 1, pad: 1, round: true, shadow: true, ratio: Ratio::Auto };
        let out = framed(&img, &f, 1.0);
        let g = frame_geom(200.0, 100.0, &f, 1.0);
        assert_eq!(out.dimensions(), (g.size.x as u32, g.size.y as u32));
        assert_eq!(g.inner.min, Pos2::new(16.0, 16.0), "8% of the longer side");
        let c = g.inner.center();
        assert_eq!(out.get_pixel(c.x as u32, c.y as u32).0, [255, 0, 0, 255], "picture in the middle");
        assert_ne!(out.get_pixel(16, 16).0, [255, 0, 0, 255], "corner is rounded off");
        assert_eq!(out.get_pixel(0, 0).0, BACKDROPS[1].1, "backdrop starts at its first color");
        // the shadow darkens the backdrop just under the picture
        let below = out.get_pixel(c.x as u32, g.inner.max.y as u32 + 4).0;
        let far = out.get_pixel(c.x as u32 + 1, out.height() - 1).0;
        assert!((below[0] as i32) < far[0] as i32 + 30, "shadow under the picture");
        for (ratio, want) in [(Ratio::Wide, 16.0 / 9.0), (Ratio::Square, 1.0), (Ratio::Classic, 4.0 / 3.0)] {
            let g = frame_geom(200.0, 100.0, &Frame { ratio, ..f }, 1.0);
            assert!((g.size.x / g.size.y - want).abs() < 0.02, "{ratio:?}");
            assert!(g.inner.min.x >= 16.0 && g.inner.min.y >= 16.0, "{ratio:?} keeps the padding");
        }
        let mut d = Doc::default();
        d.frame = Some(f);
        assert_eq!(render(&img, &d).dimensions(), out.dimensions());
    }

    #[test]
    fn hit_tests() {
        let a = Mark::Arrow { a: Pos2::new(0.0, 0.0), b: Pos2::new(100.0, 0.0), color: [0; 4], width: 4.0, heads: Heads::End };
        assert!(a.hit(Pos2::new(50.0, 3.0), 2.0));
        assert!(!a.hit(Pos2::new(50.0, 20.0), 2.0));
        let r = Mark::Rect { r: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0)), color: [0; 4], width: 4.0 };
        assert!(r.hit(Pos2::new(0.0, 50.0), 2.0));
        assert!(!r.hit(Pos2::new(50.0, 50.0), 2.0), "hollow inside");
    }

    #[test]
    fn ruler_and_picker() {
        let mut img = canvas();
        for x in 20..60 {
            for y in 10..30 {
                img.put_pixel(x, y, Rgba([11, 107, 216, 255]));
            }
        }
        assert_eq!(hex(pixel(&img, 30, 20).unwrap()), "#0B6BD8");
        assert_eq!(edge_span(&img, 30, 20, 8), Some((20, 59, 10, 29)));
        assert_eq!(pixel(&img, -1, 0), None);
        assert_eq!(surrounding_color(&img, Rect::from_min_max(Pos2::new(21.0, 11.0), Pos2::new(58.0, 28.0))), [11, 107, 216, 255]);
    }

    #[test]
    fn a_flat_ellipse_is_hit_near_its_outline() {
        let m = Mark::Ellipse { r: Rect::from_min_max(Pos2::ZERO, Pos2::new(200.0, 20.0)), color: [0; 4], width: 4.0 };
        // under 3 px from the outline, far along the ray from the center
        assert!(m.hit(Pos2::new(170.0, 3.0), 6.0));
        assert!(!m.hit(Pos2::new(100.0, 10.0), 6.0));
    }
}
