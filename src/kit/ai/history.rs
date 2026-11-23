//! What the app saw over time, kept in `<data>/ai/history/<provider>.json`:
//! limit percentages (for the week's chart and for alerts) and daily
//! totals, which outlive the session logs (Claude Code deletes its logs
//! after 30 days).

use super::{Day, Limit, LocalTokens};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sample {
    pub ts: i64,
    pub label: String,
    pub pct: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SavedDay {
    /// "2026-11-12"
    pub date: String,
    pub tokens: u64,
    pub usd: f64,
    pub requests: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct History {
    pub samples: Vec<Sample>,
    pub days: Vec<SavedDay>,
    /// The `COUNTING` the days were saved under.
    pub counting: u32,
}

/// Goes up when replies are counted differently, so days saved under the
/// old rules give way to what the logs say now.
const COUNTING: u32 = 1;

/// Samples and days older than this are dropped.
const KEEP_DAYS: i64 = 45;
/// A limit that didn't change is sampled again after this long, so the
/// chart has points through quiet hours.
const RESAMPLE_SECS: i64 = 30 * 60;

impl History {
    /// Add a sample for each limit that changed (or wasn't sampled lately).
    /// Returns whether anything was added.
    pub fn record(&mut self, limits: &[Limit], now: i64) -> bool {
        let mut added = false;
        for l in limits {
            let last = self.samples.iter().rev().find(|s| s.label == l.label);
            let due = match last {
                Some(s) => (s.pct - l.pct).abs() >= 0.5 || now - s.ts >= RESAMPLE_SECS,
                None => true,
            };
            if due {
                self.samples.push(Sample { ts: now, label: l.label.clone(), pct: l.pct });
                added = true;
            }
        }
        let cut = now - KEEP_DAYS * 86_400;
        self.samples.retain(|s| s.ts >= cut);
        added
    }

    /// The samples of `l` in its current window, as (unix time, percent).
    pub fn window(&self, l: &Limit) -> Vec<(i64, f32)> {
        let Some(end) = l.resets_at else { return Vec::new() };
        let start = end - l.window_secs;
        self.samples.iter().filter(|s| s.label == l.label && s.ts >= start && s.ts <= end).map(|s| (s.ts, s.pct)).collect()
    }

    /// Fill days the logs no longer cover from what was saved, and save
    /// what the logs say now. A past day keeps the larger of the two counts
    /// (its logs only get deleted); today is what the logs say, so it reads
    /// the same everywhere and a count that was too high comes back down.
    pub fn merge_days(&mut self, local: &mut LocalTokens, today: NaiveDate) {
        let key = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
        let mut by_date: HashMap<String, SavedDay> = self.days.drain(..).map(|d| (d.date.clone(), d)).collect();
        // days saved under other counting rules stay only where the logs are gone
        let same_rules = self.counting == COUNTING;
        self.counting = COUNTING;
        for d in local.days.iter_mut() {
            let past = d.date != today;
            match by_date.get(&key(d.date)) {
                Some(s) if past && s.tokens > d.tokens && (same_rules || d.tokens == 0) => {
                    *d = Day { date: d.date, tokens: s.tokens, usd: s.usd, requests: s.requests };
                }
                _ => {
                    if d.tokens > 0 {
                        by_date.insert(key(d.date), SavedDay { date: key(d.date), tokens: d.tokens, usd: d.usd, requests: d.requests });
                    } else {
                        by_date.remove(&key(d.date));
                    }
                }
            }
        }
        let cut = key(today - chrono::Days::new(KEEP_DAYS as u64));
        // ISO dates sort as text
        self.days = by_date.into_values().filter(|d| d.date >= cut).collect();
        self.days.sort_by(|a, b| a.date.cmp(&b.date));
        // the totals follow the days
        let n = local.days.len();
        let tail = |k: usize| local.days[n.saturating_sub(k)..].iter().map(|d| d.usd).sum::<f64>();
        if n > 0 {
            local.usd_7d = local.usd_7d.max(tail(7));
            local.usd_30d = local.usd_30d.max(tail(30));
            local.usd_month = local.usd_month.max(local.month().iter().map(|d| d.usd).sum());
            local.daily = local.days[n.saturating_sub(7)..].iter().map(|d| d.tokens).collect();
        }
    }
}

fn path(provider: &str) -> PathBuf {
    crate::kit::fs::data_path("ai").join("history").join(format!("{provider}.json"))
}

static CACHE: Mutex<Option<HashMap<String, History>>> = Mutex::new(None);

/// Read and change the history of `provider` (an id like "claude"),
/// saving it afterwards when `f` says it changed.
pub fn with<R>(provider: &str, f: impl FnOnce(&mut History) -> (R, bool)) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    let h = map.entry(provider.to_string()).or_insert_with(|| crate::kit::fs::load_json(&path(provider)));
    let (r, changed) = f(h);
    if changed {
        crate::kit::fs::save_json_later(&path(provider), &*h, false);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim(label: &str, pct: f32, end: i64) -> Limit {
        Limit { label: label.into(), pct, resets_at: Some(end), window_secs: 1000, ..Default::default() }
    }

    #[test]
    fn samples_only_changes_or_after_a_while() {
        let mut h = History::default();
        assert!(h.record(&[lim("week", 10.0, 2000)], 1500));
        assert!(!h.record(&[lim("week", 10.2, 2000)], 1510));
        assert!(h.record(&[lim("week", 11.0, 2000)], 1520));
        assert!(h.record(&[lim("week", 11.0, 2000)], 1520 + RESAMPLE_SECS));
        // only this window's samples
        let w = h.window(&lim("week", 11.0, 2000 + RESAMPLE_SECS));
        assert_eq!(w.last(), Some(&(1520 + RESAMPLE_SECS, 11.0)));
        assert!(w.iter().all(|(t, _)| *t >= 1000 + RESAMPLE_SECS));
        // old samples go
        assert!(h.record(&[lim("week", 50.0, 0)], 100 * 86_400));
        assert_eq!(h.samples.len(), 1);
    }

    #[test]
    fn days_outlive_the_logs() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let d = |n: u64| today - chrono::Days::new(n);
        let mut h = History::default();
        let mut local = LocalTokens { days: (0..3).rev().map(|n| Day { date: d(n), tokens: 10 * (n + 1), usd: 1.0, requests: 1 }).collect(), ..Default::default() };
        h.merge_days(&mut local, today);
        assert_eq!(h.days.len(), 3);
        // the logs of two days ago were deleted since
        let mut later = LocalTokens { days: (0..3).rev().map(|n| Day { date: d(n), tokens: if n == 2 { 0 } else { 10 * (n + 1) }, ..Default::default() }).collect(), ..Default::default() };
        h.merge_days(&mut later, today);
        assert_eq!(later.days[0].tokens, 30);
        assert_eq!(later.days[0].usd, 1.0);
        assert!(later.usd_30d >= 1.0);
    }

    #[test]
    fn today_is_what_the_logs_say() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let days = |yesterday: u64, now: u64| LocalTokens {
            days: vec![Day { date: today - chrono::Days::new(1), tokens: yesterday, usd: 1.0, requests: 1 }, Day { date: today, tokens: now, usd: 1.0, requests: 1 }],
            ..Default::default()
        };
        let mut h = History::default();
        h.merge_days(&mut days(50, 900), today);
        // a session counted earlier today is gone from the logs: today
        // follows them, yesterday keeps what was saved
        let mut later = days(20, 300);
        h.merge_days(&mut later, today);
        assert_eq!(later.days.iter().map(|d| d.tokens).collect::<Vec<_>>(), vec![50, 300]);
        assert_eq!(h.days.last().map(|d| d.tokens), Some(300));
        // nothing left today: nothing saved for it either
        h.merge_days(&mut days(20, 0), today);
        assert_eq!(h.days.len(), 1);
    }

    #[test]
    fn days_counted_under_old_rules_give_way_to_the_logs() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let d = |n: u64| today - chrono::Days::new(n);
        let saved = |n: u64, tokens: u64| SavedDay { date: d(n).format("%Y-%m-%d").to_string(), tokens, usd: 1.0, requests: 1 };
        // saved before the counting changed
        let mut h = History { days: vec![saved(2, 700), saved(1, 700)], ..Default::default() };
        // the logs still have yesterday, not the day before
        let mut local = LocalTokens { days: vec![Day { date: d(2), ..Default::default() }, Day { date: d(1), tokens: 400, usd: 1.0, requests: 1 }, Day { date: today, ..Default::default() }], ..Default::default() };
        h.merge_days(&mut local, today);
        assert_eq!(local.days.iter().map(|d| d.tokens).collect::<Vec<_>>(), vec![700, 400, 0]);
        // from then on a past day keeps the larger count again
        let mut later = LocalTokens { days: vec![Day { date: d(1), tokens: 100, usd: 1.0, requests: 1 }], ..Default::default() };
        h.merge_days(&mut later, today);
        assert_eq!(later.days[0].tokens, 400);
    }
}
