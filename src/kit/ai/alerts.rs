//! When to notify: a limit (or an API budget) passed a threshold, a limit
//! that was getting full reset, or a weekly limit is about to reset with
//! much of it unused. Each alert fires once per window, remembered by key.

use super::{pace, settings::Settings, Limit, UsageInfo};
use chrono::{Local, TimeZone};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// What alerts already fired and the limits seen last time, per provider.
#[derive(Default, Debug)]
pub struct Memory {
    pub sent: HashSet<String>,
    /// (label, percent, resets_at) at the last check.
    pub last: Vec<(String, f32, Option<i64>)>,
}

/// Short limit name for a sentence: "session", "week", "week · Opus".
fn short(label: &str) -> String {
    let low = label.to_lowercase();
    if low.contains("session") {
        "session".into()
    } else if let (true, Some(a), Some(b)) = (low.contains("week"), label.find('('), label.rfind(')')) {
        let inner = &label[a + 1..b];
        if inner.eq_ignore_ascii_case("all models") {
            "week".into()
        } else {
            format!("week · {inner}")
        }
    } else if low.contains("week") {
        "week".into()
    } else {
        label.to_string()
    }
}

/// "21:10" today, "tomorrow 05:00", "Sat 14:00" later.
pub fn when(ts: i64, now: i64) -> String {
    let (Some(t), Some(n)) = (Local.timestamp_opt(ts, 0).single(), Local.timestamp_opt(now, 0).single()) else { return String::new() };
    let days = (t.date_naive() - n.date_naive()).num_days();
    match days {
        0 => t.format("%H:%M").to_string(),
        1 => format!("tomorrow {}", t.format("%H:%M")),
        2..=6 => t.format("%a %H:%M").to_string(),
        _ => t.format("%b %-d %H:%M").to_string(),
    }
}

/// The alerts due for `info` at `now`, updating `mem`.
pub fn check(info: &UsageInfo, s: &Settings, mem: &mut Memory, now: i64) -> Vec<Alert> {
    let mut out = Vec::new();
    let name = info.provider;
    let mut thresholds = s.notify_at.clone();
    thresholds.sort_unstable();
    for l in &info.limits {
        let window = l.resets_at.unwrap_or(0);
        // the highest threshold newly passed (one alert, not three)
        if let Some(t) = thresholds.iter().rev().find(|t| l.pct >= **t as f32) {
            let key = format!("{name}|{}|{window}|{t}", l.label);
            if mem.sent.insert(key) {
                // lower thresholds of this window count as sent too
                for lower in thresholds.iter().filter(|x| *x < t) {
                    mem.sent.insert(format!("{name}|{}|{window}|{lower}", l.label));
                }
                let mut body = l.resets_at.map(|r| format!("Resets {}", when(r, now))).unwrap_or_default();
                if pace::of_limit(l, now).is_some_and(|p| p.state == pace::State::Ahead) {
                    body.push_str(if body.is_empty() { "Ahead of pace" } else { " · ahead of pace" });
                }
                out.push(Alert { title: format!("{name} {} at {t}%", short(&l.label)), body });
            }
        }
        // a limit that was getting full started over
        if s.notify_reset {
            let first = thresholds.first().copied().unwrap_or(80) as f32;
            let was_full = mem.last.iter().any(|(lab, was, at)| *lab == l.label && *was >= first && (l.pct + 30.0 < *was || (at.is_some() && l.resets_at.is_some() && *at != l.resets_at && l.pct < *was)));
            if was_full {
                out.push(Alert { title: format!("{name} {} reset", short(&l.label)), body: format!("Back to {:.0}%", l.pct) });
            }
        }
        // a week about to reset with much of it left
        let whole_week = l.window_secs >= 6 * 86_400 && (!l.label.contains('(') || l.label.to_lowercase().contains("all models"));
        if s.notify_unused && whole_week {
            if let Some(r) = l.resets_at {
                let left = r - now;
                if left > 0 && left <= 86_400 && l.pct <= 50.0 && mem.sent.insert(format!("{name}|{}|{r}|unused", l.label)) {
                    out.push(Alert { title: format!("{name} {}: {:.0}% unused", short(&l.label), 100.0 - l.pct), body: format!("Resets {}", when(r, now)) });
                }
            }
        }
    }
    // API keys: the month's budget
    if info.is_api() {
        if let Some(budget) = s.budgets.get(name).copied().filter(|b| *b > 0.0) {
            let pct = (info.local.usd_month / budget * 100.0) as f32;
            let month = Local.timestamp_opt(now, 0).single().map(|t| t.format("%Y-%m").to_string()).unwrap_or_default();
            if let Some(t) = thresholds.iter().rev().find(|t| pct >= **t as f32) {
                if mem.sent.insert(format!("{name}|budget|{month}|{t}")) {
                    for lower in thresholds.iter().filter(|x| *x < t) {
                        mem.sent.insert(format!("{name}|budget|{month}|{lower}"));
                    }
                    out.push(Alert { title: format!("{name} budget at {t}%"), body: format!("${:.0} of ${:.0} this month", info.local.usd_month, budget) });
                }
            }
        }
    }
    mem.last = info.limits.iter().map(|l| (l.label.clone(), l.pct, l.resets_at)).collect();
    // forget windows long gone
    if mem.sent.len() > 500 {
        mem.sent.clear();
    }
    out
}

/// The limit closest to running out, for the menu bar: (provider, limit).
pub fn closest<'a>(infos: &'a [UsageInfo]) -> Option<(&'a UsageInfo, &'a Limit)> {
    infos.iter().flat_map(|i| i.limits.iter().map(move |l| (i, l))).max_by(|a, b| a.1.pct.total_cmp(&b.1.pct))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(limits: Vec<Limit>) -> UsageInfo {
        UsageInfo { provider: "Claude", limits, ..Default::default() }
    }
    fn lim(label: &str, pct: f32, resets_at: i64, window: i64) -> Limit {
        Limit { label: label.into(), pct, resets_at: Some(resets_at), window_secs: window, ..Default::default() }
    }

    #[test]
    fn thresholds_fire_once_per_window() {
        let s = Settings::default();
        let mut m = Memory::default();
        let now = 1_000_000;
        assert!(check(&info(vec![lim("Current session", 50.0, now + 3600, 18_000)]), &s, &mut m, now).is_empty());
        let a = check(&info(vec![lim("Current session", 82.0, now + 3600, 18_000)]), &s, &mut m, now);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "Claude session at 80%");
        assert!(check(&info(vec![lim("Current session", 85.0, now + 3600, 18_000)]), &s, &mut m, now).is_empty());
        // straight past both: one alert, for 95
        let mut m2 = Memory::default();
        let a = check(&info(vec![lim("Current session", 97.0, now + 3600, 18_000)]), &s, &mut m2, now);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "Claude session at 95%");
    }

    #[test]
    fn reset_after_being_full() {
        let s = Settings::default();
        let mut m = Memory::default();
        let now = 1_000_000;
        check(&info(vec![lim("Current session", 90.0, now + 600, 18_000)]), &s, &mut m, now);
        let a = check(&info(vec![lim("Current session", 2.0, now + 18_000, 18_000)]), &s, &mut m, now + 700);
        assert!(a.iter().any(|a| a.title == "Claude session reset" && a.body == "Back to 2%"));
    }

    #[test]
    fn unused_week_a_day_before_reset() {
        let s = Settings::default();
        let mut m = Memory::default();
        let now = 1_000_000;
        let a = check(&info(vec![lim("Current week (all models)", 42.0, now + 20 * 3600, 7 * 86_400)]), &s, &mut m, now);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "Claude week: 58% unused");
        assert!(check(&info(vec![lim("Current week (all models)", 43.0, now + 20 * 3600, 7 * 86_400)]), &s, &mut m, now + 60).is_empty());
        // turned off
        let off = Settings { notify_unused: false, ..Settings::default() };
        assert!(check(&info(vec![lim("Current week (all models)", 42.0, now + 20 * 3600, 7 * 86_400)]), &off, &mut Memory::default(), now).is_empty());
    }

    #[test]
    fn api_budget() {
        let mut s = Settings::default();
        s.budgets.insert("Claude".into(), 500.0);
        let mut i = info(vec![]);
        i.billing = super::super::Billing::Api;
        i.local.usd_month = 410.0;
        let a = check(&i, &s, &mut Memory::default(), 1_000_000);
        assert_eq!(a[0].title, "Claude budget at 80%");
        assert_eq!(a[0].body, "$410 of $500 this month");
    }

    #[test]
    fn closest_to_a_limit() {
        let a = info(vec![lim("Current session", 19.0, 0, 0), lim("Current week (all models)", 62.0, 0, 0)]);
        let b = UsageInfo { provider: "Codex", limits: vec![lim("Current session", 71.0, 0, 0)], ..Default::default() };
        let all = [a, b];
        let (i, l) = closest(&all).unwrap();
        assert_eq!((i.provider, l.pct), ("Codex", 71.0));
    }
}
