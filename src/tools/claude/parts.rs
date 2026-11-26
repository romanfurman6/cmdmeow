//! Pieces the AI usage page, card and sheet share: colours, pace marks,
//! money and freshness labels, and the interactive day chart.

use crate::kit::ai::{alerts, pace, Day, DayRecord, Limit, UsageInfo};
use chrono::{Datelike, NaiveDate};
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect};

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
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

/// "21:10" / "Mon 05:00": when a limit resets.
pub fn reset_when(l: &Limit) -> String {
    l.resets_at.map(|r| alerts::when(r, now())).unwrap_or_else(|| l.resets.split(" (").next().unwrap_or("").to_string())
}

fn reset_short(s: &str) -> String {
    let s = s.split(" (").next().unwrap_or(s);
    if s.is_empty() { String::new() } else { t!("ai.resets", when = s).into_owned() }
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

/// Statuspage's incident state in words: "Investigating".
pub fn incident_state(s: &str) -> Option<String> {
    Some(match s {
        "investigating" => ts!("ai.inc.investigating"),
        "identified" => ts!("ai.inc.identified"),
        "monitoring" => ts!("ai.inc.monitoring"),
        "" => return None,
        other => return Some(other.to_owned()),
    }
    .to_owned())
}

/// When an incident started (T14, 24 h): "since 13:05" today, "since
/// Yesterday 22:10", "since Mon 22:10" within the week, else "since Nov 3".
pub fn incident_since(t: chrono::DateTime<chrono::Local>) -> String {
    let today = chrono::Local::now().date_naive();
    let d = t.date_naive();
    let when = if d == today {
        t.format("%H:%M").to_string()
    } else if today.pred_opt() == Some(d) {
        t!("ai.inc.yesterday", time = t.format("%H:%M")).into_owned()
    } else if (today - d).num_days() < 7 {
        t.format("%a %H:%M").to_string()
    } else {
        t.format("%b %-d").to_string()
    };
    t!("ai.inc.since", when = when).into_owned()
}

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
    use super::{buckets, period_days, project_names, Period};
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
    fn project_names_are_folders_never_cut_paths() {
        let home = "/Users/a";
        let paths = ["/Users/a/Projects/pdf-editor", "/Users/a/Library/Application Support/Claude/scratch-workspaces/1f3a9c2e", "/Users/a/work/site", "/Users/a/Projects/site", "Home"];
        assert_eq!(project_names(&paths, home), ["pdf-editor", "scratch-workspaces/1f3a9c2e", "work/site", "Projects/site", "Home"]);
    }
}
