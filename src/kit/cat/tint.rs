//! The desktop cat in the app's theme (TH10): its lime fur, stripes and
//! shading take the theme's action colour, its outline the theme's ink.
//! The pictures stay the bought pack's; only their colours move, once per
//! theme switch. Eyes, nose, mouth and tongue keep theirs. Pure maths.

use crate::kit::glass::{self, Theme};

/// The pack's own lime, as lightness and saturation (HLS).
const BASE_L: f64 = 0.643;
const BASE_S: f64 = 0.824;
/// Its outline's lightness.
const INK_L: f64 = 0.049;

/// Where the cat's colours go: fur and outline as HLS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    fur: [f64; 3],
    ink: Option<[f64; 3]>,
}

/// The tint for theme `t`; `None` draws the pictures as they are (Dark Classic).
pub fn for_theme(t: Theme) -> Option<Tint> {
    match t {
        Theme::Dark => None,
        // glass::ACTION() of each theme
        Theme::Light => Some(Tint { fur: hls(0xA8D83A), ink: None }),
        Theme::Miami => Some(Tint { fur: hls(0xFF5FA2), ink: Some(hls(0x1A0B2E)) }),
    }
}

/// The tint for the theme being drawn.
pub fn current() -> (Theme, Option<Tint>) {
    let t = glass::theme();
    (t, for_theme(t))
}

fn hls(rgb: u32) -> [f64; 3] {
    let c = |s: u32| f64::from((rgb >> s) & 0xff) / 255.0;
    to_hls(c(16), c(8), c(0))
}

fn to_hls(r: f64, g: f64, b: f64) -> [f64; 3] {
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    let l = (hi + lo) / 2.0;
    if hi - lo < 1e-9 {
        return [0.0, l, 0.0];
    }
    let d = hi - lo;
    let s = if l <= 0.5 { d / (hi + lo) } else { d / (2.0 - hi - lo) };
    let h = if hi == r {
        (g - b) / d
    } else if hi == g {
        2.0 + (b - r) / d
    } else {
        4.0 + (r - g) / d
    };
    [(h / 6.0).rem_euclid(1.0), l, s]
}

fn from_hls([h, l, s]: [f64; 3]) -> [f64; 3] {
    if s <= 0.0 {
        return [l, l, l];
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;
    let v = |h: f64| {
        let h = h.rem_euclid(1.0);
        if h < 1.0 / 6.0 {
            m1 + (m2 - m1) * h * 6.0
        } else if h < 0.5 {
            m2
        } else if h < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - h) * 6.0
        } else {
            m1
        }
    };
    [v(h + 1.0 / 3.0), v(h), v(h - 1.0 / 3.0)]
}

/// One straight (not premultiplied) colour moved into the tint; colours
/// that are not the cat's lime family come back as they are.
pub fn color(rgb: [u8; 3], t: &Tint) -> [u8; 3] {
    let f = |v: u8| f64::from(v) / 255.0;
    let [h, l, s] = to_hls(f(rgb[0]), f(rgb[1]), f(rgb[2]));
    if !(0.15..=0.40).contains(&h) || s <= 0.04 {
        return rgb;
    }
    let out = if l < 0.15 {
        // the outline
        match t.ink {
            Some([ih, il, is]) => [ih, (il * l / INK_L).min(1.0), is],
            None => return rgb,
        }
    } else if l > 0.88 && s < 0.6 {
        // the faintly green whites of the eyes: same light, the fur's hue
        [t.fur[0], l, s]
    } else {
        let [th, tl, ts] = t.fur;
        let l = if l > BASE_L { tl + (l - BASE_L) * (1.0 - tl) / (1.0 - BASE_L) } else { tl * l / BASE_L };
        [th, l, (ts * s / BASE_S).min(1.0)]
    };
    let c = from_hls(out);
    c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Premultiplied RGBA pixels (alpha last) moved into the tint, in place.
pub fn pixels(px: &mut [u8], t: &Tint) {
    let mut seen: std::collections::HashMap<[u8; 4], [u8; 3]> = std::collections::HashMap::new();
    for p in px.chunks_exact_mut(4) {
        let a = p[3];
        if a == 0 {
            continue;
        }
        let key = [p[0], p[1], p[2], a];
        let out = *seen.entry(key).or_insert_with(|| {
            let un = |v: u8| ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
            let c = color([un(p[0]), un(p[1]), un(p[2])], t);
            let pre = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
            [pre(c[0]), pre(c[1]), pre(c[2])]
        });
        p[..3].copy_from_slice(&out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(rgb: [u8; 3]) -> u32 {
        (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
    }

    #[test]
    fn dark_meow_keeps_the_drawn_lime() {
        assert!(for_theme(Theme::Dark).is_none());
    }

    #[test]
    fn the_fur_takes_the_theme_colour() {
        let miami = for_theme(Theme::Miami).unwrap();
        // the pack's lime lands on the theme's action pink (rounding aside)
        let c = color([0xC7, 0xEF, 0x59], &miami);
        let want = [0xFF, 0x5F, 0xA2];
        assert!(c.iter().zip(want).all(|(a, b)| (i16::from(*a) - b as i16).abs() <= 3), "{:06x}", hex(c));
        let light = for_theme(Theme::Light).unwrap();
        let c = color([0xC7, 0xEF, 0x59], &light);
        assert!(c.iter().zip([0xA8, 0xD8, 0x3A]).all(|(a, b)| (i16::from(*a) - b as i16).abs() <= 3), "{:06x}", hex(c));
    }

    #[test]
    fn stripes_stay_darker_and_highlights_lighter() {
        let t = for_theme(Theme::Miami).unwrap();
        let l = |rgb| to_hls(f64::from(color(rgb, &t)[0]) / 255.0, f64::from(color(rgb, &t)[1]) / 255.0, f64::from(color(rgb, &t)[2]) / 255.0)[1];
        let fur = l([0xC7, 0xEF, 0x59]);
        assert!(l([0x94, 0xBC, 0x31]) < fur);
        assert!(l([0xDF, 0xF8, 0x93]) > fur);
    }

    #[test]
    fn the_outline_follows_the_ink_only_where_the_theme_has_one() {
        let miami = for_theme(Theme::Miami).unwrap();
        let c = color([0x0A, 0x10, 0x09], &miami);
        assert!(c.iter().zip([0x1A, 0x0B, 0x2E]).all(|(a, b)| (i16::from(*a) - b as i16).abs() <= 2), "{:06x}", hex(c));
        let light = for_theme(Theme::Light).unwrap();
        assert_eq!(color([0x0A, 0x10, 0x09], &light), [0x0A, 0x10, 0x09]);
    }

    #[test]
    fn eyes_mouth_and_tongue_keep_their_colours() {
        let t = for_theme(Theme::Miami).unwrap();
        for c in [[0xFF, 0xFF, 0xFF], [0, 0, 0], [0xC8, 0x37, 0x37], [0x00, 0xC5, 0xF7]] {
            assert_eq!(color(c, &t), c);
        }
    }

    #[test]
    fn premultiplied_edges_keep_their_alpha() {
        let t = for_theme(Theme::Miami).unwrap();
        // half-covered lime edge pixel
        let mut px = [0x64, 0x78, 0x2D, 128];
        pixels(&mut px, &t);
        assert_eq!(px[3], 128);
        assert!(px[0] > px[1], "pink, not lime: {px:?}");
    }
}
