//! Pieces the AI usage page, card and sheet share: colours, pace marks,
//! money and freshness labels, and the interactive day chart.

use crate::kit::ai::{alerts, pace, Day, DayRecord, Limit, UsageInfo};
use chrono::{Datelike, NaiveDate};
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
        Some(at) => t!("ai.runs_out", when = alerts::when(at, now)).into_owned(),
        None => l.resets_at.map(|r| t!("ai.resets", when = alerts::when(r, now)).into_owned()).unwrap_or_else(|| reset_short(&l.resets)),
    }
}

/// The current rate runs the limit out before it resets (the forecast is red).
pub fn runs_out(l: &Limit) -> bool {
    pace::of_limit(l, now()).and_then(|p| p.out_at).is_some()
}

/// "21:10" / "Mon 05:00": when a limit resets.
pub fn reset_when(l: &Limit) -> String {
    l.resets_at.map(|r| alerts::when(r, now())).unwrap_or_else(|| l.resets.split(" (").next().unwrap_or("").to_string())
}

fn reset_short(s: &str) -> String {
    let s = s.split(" (").next().unwrap_or(s);
    if s.is_empty() { String::new() } else { t!("ai.resets", when = s).into_owned() }
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
        return vec![(ts!("ai.not_started").into(), TEXT_DIM())];
    }
    let mut out = Vec::new();
    if let Some(at) = pace::of_limit(l, now()).and_then(|p| p.out_at) {
        out.push((t!("ai.runs_out_2", when = alerts::when(at, now())).into_owned(), RED()));
    }
    if !reset.is_empty() {
        out.push((t!("ai.resets_2", when = reset).into_owned(), TEXT_DIM()));
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

/// "Updated 14:20" when the limits were last checked, or "Couldn't update · 13:52" (amber)
/// when the last check failed and these are the last ones known.
pub fn fresh(i: &UsageInfo) -> (String, Color32) {
    let Some(f) = i.fetched else { return (String::new(), TEXT_FAINT()) };
    let t = f.format("%H:%M");
    if i.stale {
        (t!("ai.couldnt_update", time = t).into_owned(), AMBER())
    } else {
        (t!("ai.updated", time = t).into_owned(), TEXT_FAINT())
    }
}

/// A small chip for an outage: "Degraded" (amber) or "Outage" (red).
pub fn incident_chip(p: &egui::Painter, pos: Pos2, align: Align2, i: &UsageInfo) -> Option<Rect> {
    let s = i.incident.as_deref()?;
    let name = crate::kit::ai::meta(i.provider).name;
    let (label, c) = if s == "Outage" { (t!("ai.is_down", name = name).into_owned(), RED()) } else { (t!("ai.has_problems", name = name).into_owned(), AMBER()) };
    Some(chip(p, pos, align, &label, (with_alpha(c, 0.16), c)))
}

/// The caption's top under a stat's number in the day chart's readout.
const STAT_CAP_Y: f32 = 20.0;

/// What the chart shows per day.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Tokens,
    Usd,
}

/// How far back the usage chart looks (SP18): days for 30d, weeks for
/// 90d, months for Year and All.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Period {
    #[default]
    D30,
    D90,
    Year,
    All,
}

impl Period {
    pub const EVERY: [Period; 4] = [Period::D30, Period::D90, Period::Year, Period::All];

    /// The switch's label: "30d".
    pub fn short(self) -> &'static str {
        match self {
            Period::D30 => ts!("ai.period.d30"),
            Period::D90 => ts!("ai.period.d90"),
            Period::Year => ts!("ai.period.year"),
            Period::All => ts!("ai.period.all"),
        }
    }

    /// The lists' caption: "90 days".
    pub fn name(self) -> &'static str {
        match self {
            Period::D30 => ts!("ai.days_30"),
            Period::D90 => ts!("ai.days_90"),
            Period::Year => ts!("ai.period.year"),
            Period::All => ts!("ai.all_time"),
        }
    }

    pub fn grain(self) -> Grain {
        match self {
            Period::D30 => Grain::Day,
            Period::D90 => Grain::Week,
            Period::Year | Period::All => Grain::Month,
        }
    }

    /// The first day the period covers, given `first`, the first day with
    /// usage (for All).
    pub fn start(self, today: NaiveDate, first: Option<NaiveDate>) -> NaiveDate {
        let month1 = |d: NaiveDate| d.with_day(1).unwrap_or(d);
        match self {
            Period::D30 => today - chrono::Days::new(29),
            Period::D90 => week_start(today) - chrono::Days::new(7 * 12),
            Period::Year => month1(today) - chrono::Months::new(11),
            // never less than a year, so a short history isn't one wide bar
            Period::All => month1(first.unwrap_or(today).min(today)).min(month1(today) - chrono::Months::new(11)),
        }
    }
}

/// What one bar holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grain {
    Day,
    Week,
    Month,
}

/// "Oct 2026".
fn month_year(d: &NaiveDate) -> String {
    t!("ai.month_year", mon = crate::kit::date::mon(d), year = d.year()).into_owned()
}

/// The Monday of `d`'s week.
fn week_start(d: NaiveDate) -> NaiveDate {
    d - chrono::Days::new(d.weekday().num_days_from_monday() as u64)
}

/// `archive` (every day with usage, oldest first) as the bars of `period`
/// up to `today`, empty buckets included; each bar's date is its first day.
pub fn buckets(archive: &[DayRecord], period: Period, today: NaiveDate) -> Vec<Day> {
    let start = period.start(today, archive.first().map(|r| r.date));
    let open = |d: NaiveDate| match period.grain() {
        Grain::Day => d,
        Grain::Week => week_start(d),
        Grain::Month => d.with_day(1).unwrap_or(d),
    };
    let next = |d: NaiveDate| match period.grain() {
        Grain::Day => d + chrono::Days::new(1),
        Grain::Week => d + chrono::Days::new(7),
        Grain::Month => d + chrono::Months::new(1),
    };
    let mut out: Vec<Day> = Vec::new();
    let mut b = open(start);
    while b <= today {
        out.push(Day { date: b, ..Default::default() });
        b = next(b);
    }
    for r in archive.iter().filter(|r| r.date >= start && r.date <= today) {
        let k = open(r.date);
        if let Some(d) = out.iter_mut().rev().find(|d| d.date == k) {
            d.tokens += r.tokens;
            d.usd += r.usd;
            d.requests += r.requests;
        }
    }
    out
}

/// The days of `archive` that `period` covers.
pub fn period_days(archive: &[DayRecord], period: Period, today: NaiveDate) -> impl Iterator<Item = &DayRecord> {
    let start = period.start(today, archive.first().map(|r| r.date));
    archive.iter().filter(move |r| r.date >= start && r.date <= today)
}

/// Bars per day with a hover readout above them: $ (when `show_usd`),
/// tokens and requests of the day under the pointer (today when none), each a number over a
/// caption, the date under the dollars. `pace` draws a
/// dashed line at that many dollars a day (a budget's even share).
///
/// With `grain` Week or Month each bar is a week or a month starting at its
/// date, and the readout names it ("This week", "Week of Nov 17", "Oct 2026").
#[allow(clippy::too_many_arguments)]
pub fn day_chart(ui: &mut Ui, r: Rect, days: &[Day], m: Measure, pace: Option<f64>, show_usd: bool, salt: &str, grain: Grain) {
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
    let last = days.last().map(|l| l.date) == Some(d.date);
    let date = match (grain, last) {
        (Grain::Day, true) => ts!("ai.today").to_string(),
        (Grain::Day, false) => crate::kit::date::wmd(&d.date),
        (Grain::Week, true) => ts!("ai.this_week").to_string(),
        (Grain::Week, false) => t!("ai.week_of", date = crate::kit::date::md(&d.date)).into_owned(),
        (Grain::Month, true) => ts!("ai.this_month").to_string(),
        (Grain::Month, false) => month_year(&d.date),
    };
    // three stats, number over caption (Roman 2026-11-24, variant A): the
    // date names the dollars, so no line of mixed sizes sits over the bars
    let mut x = r.left();
    // with cost off, tokens lead and carry the date; requests always show
    // (0 on an empty day) so the readout keeps its width while hovering
    let tokens = crate::kit::ai::human_tokens(d.tokens);
    let stats = if show_usd {
        vec![(usd(d.usd), date), (tokens, ts!("ai.cap_tokens").to_string()), (d.requests.to_string(), ts!("ai.cap_requests").to_string())]
    } else {
        vec![(t!("ai.n_tok", n = tokens).into_owned(), date), (d.requests.to_string(), ts!("ai.cap_requests").to_string())]
    };
    for (v, cap) in stats {
        let vr = p.text(Pos2::new(x, r.top()), Align2::LEFT_TOP, v, ty::lead(), TEXT());
        let cr = p.text(Pos2::new(x, r.top() + STAT_CAP_Y), Align2::LEFT_TOP, cap, ty::caption(), TEXT_DIM());
        x = vr.right().max(cr.right()) + sp::XL;
    }
    // first and last bar under the bars
    let first = match grain {
        Grain::Month => month_year(&days[0].date),
        _ => crate::kit::date::md(&days[0].date),
    };
    let now = match grain {
        Grain::Day => ts!("ai.today"),
        Grain::Week => ts!("ai.this_week"),
        Grain::Month => ts!("ai.this_month"),
    };
    p.text(Pos2::new(plot.left(), r.bottom()), Align2::LEFT_BOTTOM, first, ty::caption(), ty::caption_color());
    p.text(Pos2::new(plot.right(), r.bottom()), Align2::RIGHT_BOTTOM, now, ty::caption(), ty::caption_color());
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
        let values: Vec<String> = toks.iter().map(|t| t!("ai.n_tok", n = t).into_owned()).collect();
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
    use super::{buckets, limit_lines, now, period_days, project_names, Period};
    use crate::kit::ai::{DayRecord, Limit};
    use chrono::NaiveDate;

    #[test]
    fn periods_add_up_days_into_weeks_and_months() {
        // Wed Nov 25 2026
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let day = |y, m, d, tokens| DayRecord { date: NaiveDate::from_ymd_opt(y, m, d).unwrap(), tokens, usd: tokens as f64, requests: 1, ..Default::default() };
        let archive = vec![day(2025, 3, 4, 7), day(2026, 1, 2, 5), day(2026, 1, 30, 5), day(2026, 11, 16, 2), day(2026, 11, 22, 3), day(2026, 11, 24, 4)];
        // 30 days: one bar a day, today last
        let d30 = buckets(&archive, Period::D30, today);
        assert_eq!(d30.len(), 30);
        assert_eq!(d30.last().map(|d| (d.date, d.tokens)), Some((today, 0)));
        // 90 days: 13 weeks from Monday; Nov 16 and 22 share a week
        let w = buckets(&archive, Period::D90, today);
        assert_eq!(w.len(), 13);
        assert_eq!(w.last().map(|d| (d.date, d.tokens)), Some((NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(), 4)));
        assert_eq!(w[w.len() - 2].tokens, 5);
        // a year: 12 months, Jan has two days
        let y = buckets(&archive, Period::Year, today);
        assert_eq!(y.len(), 12);
        assert_eq!(y[0].date, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(y[1].tokens, 10);
        assert_eq!(y.iter().map(|d| d.requests).sum::<u64>(), 5);
        // all: from the first month with usage
        let all = buckets(&archive, Period::All, today);
        assert_eq!(all.len(), 21);
        assert_eq!(all[0].tokens, 7);
        assert_eq!(all.iter().map(|d| d.tokens).sum::<u64>(), 26);
        assert_eq!(period_days(&archive, Period::D90, today).count(), 3);
        // a short history still shows a year
        assert_eq!(buckets(&[], Period::All, today).len(), 12);
    }

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
