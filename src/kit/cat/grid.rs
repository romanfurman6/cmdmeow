//! Finder's desktop grid, read from where its icons lie: how far apart its
//! columns and rows are, and the slots a tidy lines the icons up in (a
//! column from the top right, as Finder places new files). Pure.

/// What Finder told about one desktop item: its name and where it shows it
/// (Finder's `desktop position`: points down from the top left of the
/// first screen).
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

/// Finder's answer, one item a line: name, x, y (tab between). Numbers may
/// come with a decimal comma; a line that is not an item is left out.
pub fn parse(out: &str) -> Vec<Spot> {
    let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
    out.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?.to_string();
            if name.is_empty() || name == "!" {
                return None;
            }
            Some(Spot { name, x: num(it.next()?)?, y: num(it.next()?)? })
        })
        .collect()
}

/// Finder's desktop view settings (`defaults read com.apple.finder
/// DesktopViewSettings`): icon size, text size, and whether it keeps its
/// own order (then it puts icons where it likes, not where they are set).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub icon: f64,
    pub text: f64,
    pub arranged: bool,
}

impl Default for View {
    fn default() -> Self {
        View { icon: 64.0, text: 12.0, arranged: false }
    }
}

pub fn view(defaults: &str) -> View {
    let mut v = View::default();
    let field = |key: &str| {
        defaults.lines().find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(key)?.trim_start();
            let rest = rest.strip_prefix('=')?;
            Some(rest.trim().trim_end_matches(';').trim().trim_matches('"').to_string())
        })
    };
    if let Some(n) = field("iconSize").and_then(|s| s.parse().ok()) {
        v.icon = n;
    }
    if let Some(n) = field("textSize").and_then(|s| s.parse().ok()) {
        v.text = n;
    }
    if let Some(a) = field("arrangeBy") {
        v.arranged = !matches!(a.as_str(), "none" | "grid" | "");
    }
    v
}

/// The most common gap between neighbours along one axis, among those in
/// line on the other (Finder puts new files on its grid, so most desktops
/// show it). None when no two icons line up.
fn common_gap(spots: &[Spot], horizontal: bool) -> Option<f64> {
    let mut gaps: Vec<f64> = Vec::new();
    for a in spots {
        let mut best: Option<f64> = None;
        for b in spots {
            let (d, off) = if horizontal { (b.x - a.x, (b.y - a.y).abs()) } else { (b.y - a.y, (b.x - a.x).abs()) };
            if d > 20.0 && off < 3.0 {
                best = Some(best.map_or(d, |x: f64| x.min(d)));
            }
        }
        gaps.extend(best);
    }
    if gaps.is_empty() {
        return None;
    }
    // the gap seen most often, to 2 points
    let mut best = (0usize, gaps[0]);
    for g in &gaps {
        let n = gaps.iter().filter(|h| (*h - g).abs() <= 2.0).count();
        if n > best.0 || (n == best.0 && *g < best.1) {
            best = (n, *g);
        }
    }
    Some(best.1)
}

/// How far apart Finder's columns and rows are.
pub fn pitch(spots: &[Spot], v: View) -> (f64, f64) {
    let fallback = (v.icon + 40.0, v.icon + v.text * 2.0 + 22.0);
    let x = common_gap(spots, true).filter(|g| *g >= v.icon * 0.8 && *g <= v.icon * 3.0).unwrap_or(fallback.0);
    let y = common_gap(spots, false).filter(|g| *g >= v.icon * 0.8 && *g <= v.icon * 3.0).unwrap_or(fallback.1);
    (x, y)
}

/// The slots of a tidy: columns from the right edge leftwards, each from
/// the top down, `n` of them, inside `area` (x0, y0, x1, y1: the usable
/// part of the screen, in Finder's points). The first column is the one
/// Finder already uses on the right, if there is one.
pub fn slots(spots: &[Spot], pitch: (f64, f64), area: (f64, f64, f64, f64), n: usize) -> Vec<(f64, f64)> {
    let (px, py) = pitch;
    let (x0, y0, x1, y1) = area;
    let right = spots.iter().map(|s| s.x).filter(|x| *x > x1 - px * 1.2).fold(f64::NAN, f64::max);
    let col0 = if right.is_finite() { right } else { x1 - px / 2.0 - 6.0 };
    let top = spots.iter().filter(|s| (s.x - col0).abs() < 3.0).map(|s| s.y).fold(f64::NAN, f64::min);
    let row0 = if top.is_finite() && top < y0 + py * 1.5 { top } else { y0 + py / 2.0 };
    // leave the bottom right corner to the cat
    let rows = (((y1 - 170.0) - row0) / py).floor().max(0.0) as usize + 1;
    let mut out = Vec::with_capacity(n);
    let mut c = 0;
    while out.len() < n {
        let x = col0 - c as f64 * px;
        if x < x0 + px / 2.0 {
            break;
        }
        for r in 0..rows {
            if out.len() == n {
                break;
            }
            out.push((x, row0 + r as f64 * py));
        }
        c += 1;
    }
    out
}

/// A picture's see-through mask: alpha 0–255 per pixel, rows from the top,
/// `scale` pixels a point, its top left pixel at `origin` (points).
pub struct Mask {
    pub alpha: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub origin: (f64, f64),
    pub scale: f64,
}

impl Mask {
    fn at(&self, x: f64, y: f64) -> (isize, isize) {
        (((x - self.origin.0) * self.scale).floor() as isize, ((y - self.origin.1) * self.scale).floor() as isize)
    }
}

/// The rectangle (x, y, w, h: points, top left) that icon `i`'s picture and
/// label really cover on `mask`, Finder's icons alone: whatever is not
/// see-through below the top of `guess`, out to halfway to the icons beside
/// it and down to the top of the one under it, with 2 points to spare.
/// Long names wrap to two lines and wide ones run past the grid's pitch, so
/// a rectangle worked out from the text size misses part of them.
/// `mids`: the middles of the icons' pictures; `icon`: their size.
pub fn ink_box(mask: &Mask, mids: &[(f64, f64)], i: usize, icon: f64, guess: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let Some(&(mx, my)) = mids.get(i) else { return guess };
    let half = icon / 2.0;
    let (mut x0, y0, mut x1, mut y1) = (mx - icon * 1.6, guess.1, mx + icon * 1.6, my + half + icon * 1.5);
    for (k, &(ox, oy)) in mids.iter().enumerate() {
        if k == i {
            continue;
        }
        if (oy - my).abs() < icon {
            if ox > mx {
                x1 = x1.min((mx + ox) / 2.0);
            } else {
                x0 = x0.max((mx + ox) / 2.0);
            }
        } else if oy > my && (ox - mx).abs() < icon * 1.6 {
            y1 = y1.min(oy - half - 2.0);
        }
    }
    let ((px0, py0), (px1, py1)) = (mask.at(x0, y0), mask.at(x1, y1));
    let (px0, py0) = (px0.max(0) as usize, py0.max(0) as usize);
    let (px1, py1) = ((px1.max(0) as usize).min(mask.w), (py1.max(0) as usize).min(mask.h));
    // the picture's square always counts, even where it is see-through
    let (mut a0, mut b0, mut a1, mut b1) = (mx - half, my - half, mx + half, my + half);
    for py in py0..py1 {
        let row = &mask.alpha[py * mask.w..(py + 1) * mask.w];
        for (px, &al) in row.iter().enumerate().take(px1).skip(px0) {
            if al > 24 {
                let (x, y) = (mask.origin.0 + px as f64 / mask.scale, mask.origin.1 + py as f64 / mask.scale);
                a0 = a0.min(x);
                b0 = b0.min(y);
                a1 = a1.max(x + 1.0 / mask.scale);
                b1 = b1.max(y + 1.0 / mask.scale);
            }
        }
    }
    let (a0, b0, a1, b1) = ((a0 - 2.0).max(x0), (b0 - 2.0).max(y0), (a1 + 2.0).min(x1), (b1 + 2.0).min(y1));
    (a0, b0, a1 - a0, b1 - b0)
}

/// Which pixels of the picture `(px, py, w, h)` (pixels of `mask`, top
/// left) are icon `i`'s own: nearer its picture and the label under it
/// than any other icon's. A messy desk has names that run into the
/// neighbours' squares; without this a lifted icon takes those pieces
/// along, and its patch of wallpaper covers them up.
/// Each icon is a line down its middle, from the top of its picture to the
/// foot of a two-line label; `mids`: the pictures' middles; `icon`: their size.
pub fn owned(mask: &Mask, mids: &[(f64, f64)], i: usize, icon: f64, (px, py, w, h): (usize, usize, usize, usize)) -> Vec<bool> {
    let Some(&(mx, my)) = mids.get(i) else { return vec![true; w * h] };
    let (top, foot) = (-icon / 2.0, icon / 2.0 + icon * 1.5);
    let far = icon * 4.0;
    let near: Vec<(f64, f64)> = mids.iter().enumerate().filter(|&(k, &(ox, oy))| k != i && (ox - mx).abs() < far && (oy - my).abs() < far).map(|(_, &o)| o).collect();
    let d = |(x, y): (f64, f64), (ox, oy): (f64, f64)| -> f64 {
        let dy = y - oy;
        let out = if dy < top { top - dy } else if dy > foot { dy - foot } else { 0.0 };
        (x - ox).hypot(out)
    };
    let mut own = Vec::with_capacity(w * h);
    for row in 0..h {
        for col in 0..w {
            let p = (mask.origin.0 + (px + col) as f64 / mask.scale, mask.origin.1 + (py + row) as f64 / mask.scale);
            let mine = d(p, (mx, my));
            own.push(near.iter().all(|&o| mine <= d(p, o)));
        }
    }
    own
}

/// Where icon's own pixels (`own`, the rectangle `(x0, y0, w, h)` of
/// `mask`) are drawn, `r` pixels around: what its wallpaper patch covers.
pub fn drawn(mask: &Mask, own: &[bool], (x0, y0, w, h): (usize, usize, usize, usize), r: usize) -> Vec<bool> {
    let ink: Vec<bool> = (0..w * h).map(|k| own[k] && mask.alpha[(y0 + k / w) * mask.w + x0 + k % w] > 8).collect();
    // grow by r: along the rows, then down the columns
    let mut rows = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if ink[y * w + x] {
                for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                    rows[y * w + xx] = true;
                }
            }
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if rows[y * w + x] {
                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                    out[yy * w + x] = true;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, x: f64, y: f64) -> Spot {
        Spot { name: name.into(), x, y }
    }

    #[test]
    fn reads_finders_answer_with_any_decimal_mark() {
        let out = "a.png\t100\t50\nb c.pdf\t1388,5\t140,0\n!\tsome error\n\nnope\n";
        assert_eq!(parse(out), vec![s("a.png", 100.0, 50.0), s("b c.pdf", 1388.5, 140.0)]);
    }

    #[test]
    fn reads_the_view_settings() {
        let d = "{\n    GroupBy = None;\n    IconViewSettings =     {\n        arrangeBy = none;\n        gridSpacing = 54;\n        iconSize = 48;\n        textSize = 11;\n    };\n}";
        assert_eq!(view(d), View { icon: 48.0, text: 11.0, arranged: false });
        assert!(view("arrangeBy = name;").arranged);
        assert!(!view("arrangeBy = grid;").arranged);
        assert_eq!(view(""), View::default());
    }

    #[test]
    fn finds_the_grid_from_icons_in_line() {
        let spots = vec![s("a", 1450.0, 60.0), s("b", 1450.0, 150.0), s("c", 1450.0, 240.0), s("d", 1350.0, 60.0), s("e", 600.0, 333.0)];
        assert_eq!(pitch(&spots, View::default()), (100.0, 90.0));
        // nothing in line: Finder's usual spacing for the icon size
        assert_eq!(pitch(&[s("a", 10.0, 10.0)], View::default()), (104.0, 110.0));
    }

    #[test]
    fn slots_start_in_finders_right_column_and_leave_the_corner() {
        let spots = vec![s("a", 1450.0, 60.0), s("b", 1450.0, 150.0), s("e", 600.0, 333.0)];
        let out = slots(&spots, (100.0, 90.0), (0.0, 25.0, 1512.0, 900.0), 12);
        assert_eq!(out.len(), 12);
        assert_eq!(out[0], (1450.0, 60.0));
        assert_eq!(out[1], (1450.0, 150.0));
        assert!(out.iter().all(|(_, y)| *y <= 900.0 - 170.0 + 0.1));
        let cols: std::collections::BTreeSet<i64> = out.iter().map(|(x, _)| *x as i64).collect();
        assert_eq!(cols.len(), 2);
        // an empty desktop: a column at the right edge
        let out = slots(&[], (100.0, 90.0), (0.0, 25.0, 1512.0, 900.0), 3);
        assert_eq!(out, vec![(1456.0, 70.0), (1456.0, 160.0), (1456.0, 250.0)]);
    }

    /// A 1 pt = 1 px mask with the given filled rectangles (x0, y0, x1, y1).
    fn mask(w: usize, h: usize, fills: &[(usize, usize, usize, usize)]) -> Mask {
        let mut alpha = vec![0u8; w * h];
        for &(x0, y0, x1, y1) in fills {
            for y in y0..y1 {
                for x in x0..x1 {
                    alpha[y * w + x] = 255;
                }
            }
        }
        Mask { alpha, w, h, origin: (0.0, 0.0), scale: 1.0 }
    }

    #[test]
    fn the_ink_box_takes_in_a_long_two_line_name() {
        // a 64 icon at (200, 100), its name 150 wide on two lines under it
        let m = mask(600, 400, &[(168, 68, 232, 132), (125, 136, 275, 168)]);
        let guess = (151.0, 62.0, 98.0, 115.0);
        let b = ink_box(&m, &[(200.0, 100.0)], 0, 64.0, guess);
        assert!(b.0 <= 123.0 && b.0 + b.2 >= 277.0, "{b:?}");
        assert!(b.1 + b.3 >= 170.0, "{b:?}");
    }

    #[test]
    fn the_ink_box_stops_halfway_to_a_neighbour() {
        // two icons 100 apart in a row, labels wide enough to meet
        let m = mask(600, 400, &[(168, 68, 232, 132), (268, 68, 332, 132), (140, 136, 249, 150), (251, 136, 360, 150)]);
        let b = ink_box(&m, &[(200.0, 100.0), (300.0, 100.0)], 0, 64.0, (151.0, 62.0, 98.0, 115.0));
        assert!(b.0 + b.2 <= 250.0 + 1e-9, "{b:?}");
        assert!(b.0 <= 138.0, "{b:?}");
        // and above the icon under it
        let m = mask(600, 400, &[(168, 68, 232, 132), (168, 168, 232, 232)]);
        let b = ink_box(&m, &[(200.0, 100.0), (200.0, 200.0)], 0, 64.0, (151.0, 62.0, 98.0, 115.0));
        assert!(b.1 + b.3 <= 166.0 + 1e-9, "{b:?}");
    }

    #[test]
    fn a_neighbours_name_running_into_the_box_is_not_ours() {
        // icon 0 at (200, 100); icon 1 sits lower right and its label runs
        // under icon 0's picture
        let m = mask(600, 400, &[]);
        let mids = [(200.0, 100.0), (270.0, 160.0)];
        let own = owned(&m, &mids, 0, 64.0, (130, 60, 140, 160));
        let at = |x: usize, y: usize| own[(y - 60) * 140 + (x - 130)];
        // our picture and our label straight under it
        assert!(at(200, 100) && at(200, 150) && at(180, 170));
        // the neighbour's label, under its own picture, is not ours
        assert!(!at(262, 215) && !at(255, 200));
        // with no neighbour everything is ours
        assert!(owned(&m, &mids[..1], 0, 64.0, (130, 60, 140, 160)).iter().all(|&b| b));
    }

    #[test]
    fn the_patch_covers_our_ink_and_a_little_around_it_only() {
        let m = mask(40, 40, &[(10, 10, 12, 12), (30, 10, 32, 12)]);
        // the right half is someone else's
        let own: Vec<bool> = (0..40 * 40).map(|k| k % 40 < 25).collect();
        let d = drawn(&m, &own, (0, 0, 40, 40), 2);
        assert!(d[11 * 40 + 11] && d[8 * 40 + 8] && d[11 * 40 + 13]);
        assert!(!d[11 * 40 + 14] && !d[20 * 40 + 11]);
        // the neighbour's ink is not covered
        assert!(!d[11 * 40 + 31]);
    }

    #[test]
    fn an_empty_mask_still_covers_the_picture() {
        let m = mask(600, 400, &[]);
        let b = ink_box(&m, &[(200.0, 100.0)], 0, 64.0, (151.0, 62.0, 98.0, 115.0));
        assert!(b.0 <= 168.0 && b.1 <= 68.0 && b.0 + b.2 >= 232.0 && b.1 + b.3 >= 132.0, "{b:?}");
    }
}
