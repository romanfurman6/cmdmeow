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
}
