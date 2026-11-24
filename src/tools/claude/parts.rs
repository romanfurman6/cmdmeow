//! Pieces the AI usage page, card and sheet share: colours, pace marks,
//! money and freshness labels, and the interactive day chart.

use crate::kit::ai::{alerts, pace, Day, Limit, UsageInfo};
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

/// Bar colour for a usage percentage: the tool's tint, amber from 70%,
/// red from 90%.
pub fn usage_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        RED()
    } else if pct >= 70.0 {
        AMBER()
    } else {
        tint::CLAUDE()
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Colour of a limit: red when ahead of pace or nearly full.
pub fn limit_color(l: &Limit) -> Color32 {
    match pace::of_limit(l, now()) {
        Some(p) if p.state == pace::State::Ahead && l.pct >= 20.0 => RED(),
        _ => usage_color(l.pct),
    }
}

/// "Runs out Sat 14:00" when the current rate runs out before the reset, else
/// when it resets.
pub fn forecast(l: &Limit) -> String {
    let now = now();
    match pace::of_limit(l, now).and_then(|p| p.out_at) {
        Some(at) => format!("Runs out {}", alerts::when(at, now)),
        None => l.resets_at.map(|r| format!("Resets {}", alerts::when(r, now))).unwrap_or_else(|| reset_short(&l.resets)),
    }
}

/// "21:10" / "Mon 05:00": when a limit resets.
pub fn reset_when(l: &Limit) -> String {
    l.resets_at.map(|r| alerts::when(r, now())).unwrap_or_else(|| l.resets.split(" (").next().unwrap_or("").to_string())
}

fn reset_short(s: &str) -> String {
    let s = s.split(" (").next().unwrap_or(s);
    if s.is_empty() { String::new() } else { format!("Resets {s}") }
}

/// The faint arc of a limit's ring (SP2 `stat_ring_paced`): where even use
/// would be by now, as a solid colour mixed over the card so its caps don't
/// double up. None when the window's start isn't known.
pub fn pace_arc(l: &Limit) -> Option<(f32, Color32)> {
    let pc = pace::of_limit(l, now())?;
    let base = {
        let [r_, g, b, a] = CARD().to_srgba_unmultiplied();
        lerp_color(BG(), Color32::from_rgb(r_, g, b), a as f32 / 255.0)
    };
    Some((pc.expected / 100.0, lerp_color(base, TEXT(), if is_light() { 0.18 } else { 0.22 })))
}

/// What a limit's card says beside its ring, each as a word over its time:
/// when it runs out at this rate ("Runs out" / "Fri 19:10", red), then
/// when it resets ("Resets" / "Sun 08:52"). A session that hasn't started
/// (0% and no reset yet) says so.
pub fn limit_lines(l: &Limit) -> Vec<(String, Color32)> {
    let reset = reset_when(l);
    if l.pct <= 0.0 && reset.is_empty() {
        return vec![("Not started".into(), TEXT_DIM())];
    }
    let mut out = Vec::new();
    if let Some(at) = pace::of_limit(l, now()).and_then(|p| p.out_at) {
        out.push((format!("Runs out\n{}", alerts::when(at, now())), RED()));
    }
    if !reset.is_empty() {
        out.push((format!("Resets\n{reset}"), TEXT_DIM()));
    }
    out
}

/// A thin bar with the even-use tick.
pub fn pace_bar(p: &egui::Painter, r: Rect, l: &Limit) {
    bar(p, r, (l.pct / 100.0).max(0.01), limit_color(l));
    if let Some(pc) = pace::of_limit(l, now()) {
        let x = r.left() + r.width() * pc.expected / 100.0;
        p.line_segment([Pos2::new(x, r.top() - 3.0), Pos2::new(x, r.bottom() + 3.0)], Stroke::new(1.5, TEXT()));
    }
}

/// "$38.40", "$664".
pub fn usd(v: f64) -> String {
    if v >= 100.0 { format!("${v:.0}") } else { format!("${v:.2}") }
}

/// "14:20" when the limits were last checked, or "As of 13:52" (amber)
/// when the last check failed and these are the last ones known.
pub fn fresh(i: &UsageInfo) -> (String, Color32) {
    let Some(f) = i.fetched else { return (String::new(), TEXT_FAINT()) };
    let t = f.format("%H:%M");
    if i.stale {
        (format!("As of {t}"), AMBER())
    } else {
        (format!("Updated {t}"), TEXT_FAINT())
    }
}

/// A small chip for an outage: "Degraded" (amber) or "Outage" (red).
pub fn incident_chip(p: &egui::Painter, pos: Pos2, align: Align2, i: &UsageInfo) -> Option<Rect> {
    let s = i.incident.as_deref()?;
    let c = if s == "Outage" { RED() } else { AMBER() };
    Some(chip(p, pos, align, s, (with_alpha(c, 0.16), c)))
}

/// The caption's top under a stat's number in the day chart's readout.
const STAT_CAP_Y: f32 = 20.0;

/// What the chart shows per day.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Tokens,
    Usd,
}

/// Bars per day with a hover readout above them: $, tokens and requests of
/// the day under the pointer (today when none), each a number over a
/// caption, the date under the dollars. `pace` draws a
/// dashed line at that many dollars a day (a budget's even share).
pub fn day_chart(ui: &mut Ui, r: Rect, days: &[Day], m: Measure, pace: Option<f64>, salt: &str) {
    let p = ui.painter().clone();
    if days.is_empty() {
        return;
    }
    let head = STAT_CAP_Y + 14.0;
    let foot = 16.0;
    let plot = Rect::from_min_max(Pos2::new(r.left(), r.top() + head + sp::S), Pos2::new(r.right(), r.bottom() - foot));
    let val = |d: &Day| match m {
        Measure::Tokens => d.tokens as f64,
        Measure::Usd => d.usd,
    };
    let top = days.iter().map(val).fold(pace.unwrap_or(0.0), f64::max).max(1e-9);
    let n = days.len() as f32;
    let gap = if n > 14.0 { 3.0 } else { 6.0 };
    let bw = ((plot.width() - gap * (n - 1.0)) / n).max(1.0);
    let resp = ui.interact(plot.expand2(Vec2::new(0.0, head)), ui.scope_id().with(("ai-chart", salt)), Sense::hover());
    let hover = resp.hover_pos().map(|q| (((q.x - plot.left()) / (bw + gap)) as usize).min(days.len() - 1));
    let sel = hover.unwrap_or(days.len() - 1);
    for (i, d) in days.iter().enumerate() {
        let h = (plot.height() * (val(d) / top) as f32).max(2.0);
        let x = plot.left() + i as f32 * (bw + gap);
        let br = Rect::from_min_max(Pos2::new(x, plot.bottom() - h), Pos2::new(x + bw, plot.bottom()));
        let over = pace.is_some_and(|pc| val(d) > pc);
        let col = if i == sel {
            tint::CLAUDE()
        } else if over {
            with_alpha(RED(), 0.55)
        } else {
            with_alpha(tint::CLAUDE(), 0.28)
        };
        p.rect_filled(br, 2.0, col);
    }
    if let Some(pc) = pace {
        let y = plot.bottom() - plot.height() * (pc / top) as f32;
        p.extend(Shape::dashed_line(&[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, TEXT_DIM()), 4.0, 3.0));
    }
    // readout
    let d = &days[sel];
    let today = days.last().map(|l| l.date) == Some(d.date);
    let date = if today { "Today".to_string() } else { d.date.format("%a %b %-d").to_string() };
    // three stats, number over caption (Roman 2026-11-24, variant A): the
    // date names the dollars, so no line of mixed sizes sits over the bars
    let mut x = r.left();
    let stats = [
        (usd(d.usd), date),
        (crate::kit::ai::human_tokens(d.tokens), "tokens".to_string()),
        (if d.requests > 0 { d.requests.to_string() } else { String::new() }, "requests".to_string()),
    ];
    for (v, cap) in stats {
        if v.is_empty() {
            continue;
        }
        let vr = p.text(Pos2::new(x, r.top()), Align2::LEFT_TOP, v, ty::lead(), TEXT());
        let cr = p.text(Pos2::new(x, r.top() + STAT_CAP_Y), Align2::LEFT_TOP, cap, ty::caption(), TEXT_DIM());
        x = vr.right().max(cr.right()) + sp::XL;
    }
    // first and last day under the bars
    let lab = |d: &Day| d.date.format("%b %-d").to_string();
    p.text(Pos2::new(plot.left(), r.bottom()), Align2::LEFT_BOTTOM, lab(&days[0]), ty::caption(), ty::caption_color());
    p.text(Pos2::new(plot.right(), r.bottom()), Align2::RIGHT_BOTTOM, "Today", ty::caption(), ty::caption_color());
}


/// A project's name for a list: its folder ("pdf-editor" for
/// ~/Projects/pdf-editor), with the parent too when two folders share a
/// name. The full path is never cut with "…" (R10).
pub fn project_names(paths: &[&str], home: &str) -> Vec<String> {
    let tail = |p: &str, n: usize| -> String {
        let p = if home.is_empty() { p.to_string() } else { p.replacen(home, "~", 1) };
        let parts: Vec<&str> = p.trim_end_matches('/').split('/').filter(|x| !x.is_empty()).collect();
        if parts.len() <= 1 {
            return p;
        }
        parts[parts.len().saturating_sub(n)..].join("/")
    };
    // a folder named like an id ("1f3a9c2e") says nothing alone: its parent too
    let id_like = |p: &str| {
        let last = p.trim_end_matches('/').rsplit('/').next().unwrap_or("");
        last.len() >= 8 && last.chars().any(|c| c.is_ascii_digit()) && last.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    };
    let short: Vec<String> = paths.iter().map(|p| tail(p, if id_like(p) { 2 } else { 1 })).collect();
    paths.iter().zip(&short).map(|(p, s)| if short.iter().filter(|x| *x == s).count() > 1 { tail(p, 2) } else { s.clone() }).collect()
}

/// Models or projects as name · bar · value lines (W5 columns shared in the
/// card, the same lines as the widgets and Mac Health), 22 pt each, as many
/// as fit. The bar is the share of dollars when shown (a model without a
/// price is 0), else of tokens. A long name fades rather than pushing the
/// bars out.
pub fn share_lines(p: &egui::Painter, r: Rect, rows: &[crate::kit::ai::Share], show_usd: bool, home: &str) {
    use crate::kit::ai::human_tokens;
    let usd_bars = show_usd && rows.iter().any(|s| s.usd > 0.0);
    let measure = |s: &crate::kit::ai::Share| if usd_bars { s.usd } else { s.tokens as f64 };
    let top = rows.iter().map(measure).fold(0.0, f64::max).max(1e-9);
    let n = rows.len().min((r.height() / sp::LEGEND_H).floor().max(0.0) as usize);
    let paths: Vec<&str> = rows.iter().take(n).map(|s| s.name.as_str()).collect();
    let names = if paths.iter().any(|p| p.starts_with('/')) { project_names(&paths, home) } else { paths.iter().map(|p| p.to_string()).collect() };
    let toks: Vec<String> = rows.iter().take(n).map(|s| human_tokens(s.tokens)).collect();
    let line = |k: usize| Rect::from_min_size(Pos2::new(r.left(), r.top() + k as f32 * sp::LEGEND_H), Vec2::new(r.width(), sp::LEGEND_H));
    let frac = |s: &crate::kit::ai::Share| Some((((measure(s) / top) as f32).max(0.01), tint::CLAUDE()));
    if show_usd {
        // tokens dim, dollars bright, each in its own column (variant A)
        let dollars: Vec<String> = rows.iter().take(n).map(|s| usd(s.usd)).collect();
        let lines: Vec<(&str, &str, &str)> = (0..n).map(|k| (names[k].as_str(), toks[k].as_str(), dollars[k].as_str())).collect();
        let (lw, dw, vw) = crate::kit::tile::bar_cols_pair(p, &lines);
        let cols = (lw.min(r.width() * 0.4), dw, vw);
        for (k, s) in rows.iter().take(n).enumerate() {
            crate::kit::tile::bar_line_pair(p, line(k), &names[k], frac(s), (&toks[k], &dollars[k]), cols);
        }
    } else {
        let values: Vec<String> = toks.iter().map(|t| format!("{t} tok")).collect();
        let (lw, vw) = crate::kit::tile::bar_cols(p, &names.iter().zip(&values).map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>());
        let cols = (lw.min(r.width() * 0.4), vw);
        for (k, s) in rows.iter().take(n).enumerate() {
            crate::kit::tile::bar_line(p, line(k), &names[k], frac(s), &values[k], cols);
        }
    }
}

/// Daily token bars, oldest first; the last (today) solid, the rest faded.
pub fn day_bars(p: &egui::Painter, r: Rect, daily: &[u64], gap: f32) {
    let maxd = daily.iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = daily.len().max(1) as f32;
    let bw = (r.width() - gap * (n - 1.0)) / n;
    for (i, v) in daily.iter().enumerate() {
        let h = (r.height() * (*v as f32 / maxd)).max(2.0);
        let x = r.left() + i as f32 * (bw + gap);
        let br = Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + bw, r.bottom()));
        let last = i + 1 == daily.len();
        p.rect_filled(br, 2.0, if last { tint::CLAUDE() } else { white(46) });
    }
}

#[cfg(test)]
mod tests {
    use super::{limit_lines, now, project_names};
    use crate::kit::ai::Limit;

    #[test]
    fn limit_lines_say_not_started_runs_out_and_resets() {
        // a session that hasn't started: 0% and nothing to reset
        let idle = Limit { label: "Current session".into(), ..Default::default() };
        assert_eq!(limit_lines(&idle).iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Not started"]);
        // a week at 90% two days in: runs out before it resets
        let t = now();
        let week = Limit { label: "Current week".into(), pct: 90.0, resets_at: Some(t + 5 * 86_400), window_secs: 7 * 86_400, ..Default::default() };
        let lines: Vec<String> = limit_lines(&week).into_iter().map(|l| l.0).collect();
        assert!(lines[0].starts_with("Runs out\n"), "{lines:?}");
        assert!(lines.last().is_some_and(|l| l.starts_with("Resets\n")), "{lines:?}");
        // no "…" and no pace words (T9)
        assert!(lines.iter().all(|l| !l.contains('…') && !l.contains("pace")));
    }

    #[test]
    fn project_names_are_folders_never_cut_paths() {
        let home = "/Users/a";
        let paths = ["/Users/a/Projects/pdf-editor", "/Users/a/Library/Application Support/Claude/scratch-workspaces/1f3a9c2e", "/Users/a/work/site", "/Users/a/Projects/site", "Home"];
        assert_eq!(project_names(&paths, home), ["pdf-editor", "scratch-workspaces/1f3a9c2e", "work/site", "Projects/site", "Home"]);
    }
}
