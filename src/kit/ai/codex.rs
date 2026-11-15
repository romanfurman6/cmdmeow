//! OpenAI Codex CLI as a `Provider`. Everything comes from its own files, no
//! CLI calls and no network: sign-in from `~/.codex/auth.json`, and both the
//! limit windows and token counts from the session logs in
//! `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, whose `token_count`
//! events carry the tokens of each turn and the plan's rate limits as of
//! that turn.

use crate::kit::sync::LockExt;
use super::{find, recent_jsonl, Billing, Limit, LocalTokens, LogCache, ParseCtx, Provider, Shared, UsageEntry, UsageInfo};
use crate::kit::claude_code as cc;
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const NAME: &str = "Codex";

#[derive(Default)]
pub struct Codex {
    cache: LogCache,
}

/// Where Codex keeps its data (`$CODEX_HOME`, else `~/.codex`).
pub fn home() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}

/// The `codex` command, if it's installed somewhere usual.
pub fn bin() -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs_: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs_.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs_.extend([".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin"].map(|d| home.join(d)));
    dirs_.into_iter().map(|d| d.join("codex")).find(|p| p.is_file())
}

impl Provider for Codex {
    fn name(&self) -> &'static str {
        NAME
    }
    // one switch and one refresh button for the whole tool
    fn enabled(&self) -> bool {
        cc::tool_on()
    }
    fn fetch_account(&mut self, s: &Shared) {
        let home = home();
        let installed = bin().is_some() || home.join("sessions").is_dir();
        let logged_in = home.join("auth.json").is_file();
        let auth = std::fs::read(home.join("auth.json")).unwrap_or_default();
        let billing = billing(&auth);
        // the account's own limits first: fresh, not only as of the last request
        let live = if installed && billing == Billing::Subscription { super::account::codex_usage(&auth).map_err(|e| eprintln!("[ai] codex usage: {e:?}")).ok() } else { None };
        let latest = if installed && live.is_none() { latest_limits(&home.join("sessions")) } else { None };
        let mut g = s.lock_ok();
        g.installed = installed;
        g.logged_in = logged_in;
        g.signed_out = installed && !logged_in;
        g.billing = billing;
        g.source = if live.is_some() { "API" } else { "Local" };
        if let Some((plan, limits)) = live {
            g.error = None;
            g.stale = false;
            if !plan.is_empty() {
                g.plan = plan;
            }
            g.limits = limits;
            g.note.clear();
            g.fetched = Some(Local::now());
            return;
        }
        if !installed {
            g.limits.clear();
            // checked: keeps the setup screen up while the next check runs
            g.fetched = Some(Local::now());
            g.error = Some("Codex isn't installed on this Mac. Install it with npm i -g @openai/codex or brew install codex, then run codex once to sign in.".into());
            return;
        }
        match latest {
            Some(l) => {
                g.error = None;
                if !l.plan.is_empty() {
                    g.plan = l.plan;
                }
                g.limits = l.limits;
                g.fetched = Some(l.at);
                g.note = "Codex reports its limits with each request, so these are as of your last one.".into();
            }
            None => {
                g.limits.clear();
                g.fetched = Some(Local::now());
                g.error = Some(if logged_in { "Codex shows your limits here after your next request.".into() } else { "Sign in to Codex to see your limits: run codex in Terminal.".into() });
            }
        }
    }
    fn local_tokens(&mut self) -> LocalTokens {
        // archived chats move to `archived_sessions/`; a rollout found in
        // both places counts once (`LogCache::entries`)
        let home = home();
        let mut files = recent_jsonl(&home.join("sessions"), 3);
        files.extend(recent_jsonl(&home.join("archived_sessions"), 1));
        self.cache.update(files, usage_entry)
    }
    fn data_dir(&self) -> Option<PathBuf> {
        Some(home())
    }
    fn demo(&self) -> UsageInfo {
        demo_info()
    }
}

/// The limits in the newest session log.
struct Latest {
    plan: String,
    limits: Vec<Limit>,
    at: DateTime<Local>,
}

fn latest_limits(sessions: &Path) -> Option<Latest> {
    let mut files = recent_jsonl(sessions, 3);
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    // newest first; an old log may still hold limits when the newest has none yet
    files.iter().rev().take(5).find_map(|p| limits_in(&tail(p, 1 << 20)))
}

/// The last `max` bytes of a file, from the first whole line.
fn tail(p: &Path, max: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(p) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    if start > 0 {
        let cut = buf.iter().position(|&b| b == b'\n').map(|i| i + 1).unwrap_or(buf.len());
        buf.drain(..cut);
    }
    buf
}

/// The last `rate_limits` snapshot in a log.
fn limits_in(log: &[u8]) -> Option<Latest> {
    log.split(|&b| b == b'\n').rev().filter(|l| find(l, b"\"rate_limits\":{").is_some()).find_map(|line| {
        let v: Value = serde_json::from_slice(line).ok()?;
        let rl = &v["payload"]["rate_limits"];
        let at = v["timestamp"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok())?.with_timezone(&Local);
        let limits: Vec<Limit> = [&rl["primary"], &rl["secondary"]].into_iter().filter_map(|w| window(w, at)).collect();
        (!limits.is_empty()).then(|| Latest { plan: rl["plan_type"].as_str().unwrap_or("").to_string(), limits, at })
    })
}

/// One limit window, named the way the UI expects ("…session…", "…week…").
fn window(w: &Value, at: DateTime<Local>) -> Option<Limit> {
    let pct = w["used_percent"].as_f64()? as f32;
    let minutes = w["window_minutes"].as_i64().unwrap_or(0);
    let label = window_label(minutes);
    // newer logs say when it resets, older ones how long until then
    let resets = w["resets_at"].as_i64().or_else(|| w["resets_in_seconds"].as_i64().map(|s| at.timestamp() + s));
    let now = Local::now().timestamp();
    match resets {
        // the window rolled over since that request: nothing used in it yet
        Some(r) if r <= now => Some(Limit { label, pct: 0.0, resets: String::new(), resets_at: None, window_secs: minutes * 60 }),
        Some(r) => Some(Limit { label, pct, resets: reset_text(r), resets_at: Some(r), window_secs: minutes * 60 }),
        None => Some(Limit { label, pct, resets: String::new(), resets_at: None, window_secs: minutes * 60 }),
    }
}

/// The name of a limit window `minutes` long, the way the UI expects it.
pub fn window_label(minutes: i64) -> String {
    match minutes {
        m if (240..=360).contains(&m) => "Current session".to_string(),
        m if (9_000..=11_000).contains(&m) => "Current week (all models)".to_string(),
        m if (40_000..=46_000).contains(&m) => "Current month".to_string(),
        m if m >= 60 => format!("Last {}h", m / 60),
        _ => "Limit".to_string(),
    }
}

/// "9:10pm" today, "Nov 8 at 5pm" on another day, like Claude's `/usage`.
pub fn reset_text(ts: i64) -> String {
    let Some(t) = Local.timestamp_opt(ts, 0).single() else { return String::new() };
    let time = if t.format("%M").to_string() == "00" { t.format("%-I%P").to_string() } else { t.format("%-I:%M%P").to_string() };
    if t.date_naive() == Local::now().date_naive() {
        time
    } else {
        // another day: the hour is enough
        format!("{} at {}", t.format("%b %-d"), t.format("%-I%P"))
    }
}

/// Subscription (signed in with ChatGPT) or API key, from `auth.json`.
pub fn billing(auth: &[u8]) -> Billing {
    let v: Value = serde_json::from_slice(auth).unwrap_or(Value::Null);
    let mode = v["auth_mode"].as_str().unwrap_or("").to_lowercase();
    if mode.contains("chatgpt") || (mode.is_empty() && v["tokens"].is_object()) {
        Billing::Subscription
    } else if mode.contains("api") || v["OPENAI_API_KEY"].as_str().is_some_and(|k| !k.is_empty()) {
        Billing::Api
    } else {
        Billing::Unknown
    }
}

/// The tokens of one turn: fresh input, cached input (read from the cache)
/// and output. Codex can repeat a count, so each running total counts once.
/// The model comes from the turn's `turn_context` line, the project from the
/// session's `session_meta`.
fn usage_entry(line: &[u8], ctx: &mut ParseCtx) -> Option<UsageEntry> {
    if find(line, b"\"session_meta\"").is_some() || find(line, b"\"turn_context\"").is_some() {
        let v: Value = serde_json::from_slice(line).ok()?;
        let p = &v["payload"];
        if let Some(cwd) = p["cwd"].as_str() {
            ctx.project = super::intern(cwd);
        }
        if let Some(m) = p["model"].as_str() {
            ctx.model = super::intern(m);
        }
        return None;
    }
    find(line, b"\"token_count\"")?;
    find(line, b"\"last_token_usage\"")?;
    let v: Value = serde_json::from_slice(line).ok()?;
    let info = &v["payload"]["info"];
    let total = info["total_token_usage"]["total_tokens"].as_i64()?;
    if !ctx.ids.insert(total.to_le_bytes().to_vec()) {
        return None;
    }
    let ts = v["timestamp"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok())?.timestamp();
    let last = &info["last_token_usage"];
    let g = |k: &str| last[k].as_i64().unwrap_or(0).max(0) as u64;
    let cached = g("cached_input_tokens").min(g("input_tokens"));
    Some(UsageEntry { ts, input: g("input_tokens") - cached, output: g("output_tokens"), cache_read: cached, model: ctx.model, project: ctx.project, ..Default::default() })
}

pub fn demo_info() -> UsageInfo {
    let now = Local::now();
    let limits = vec![
        Limit { label: "Current session".into(), pct: 71.0, resets: reset_text(now.timestamp() + 3600 + 600), resets_at: Some(now.timestamp() + 3600 + 600), window_secs: 5 * 3600 },
        Limit { label: "Current week (all models)".into(), pct: 34.0, resets: reset_text(now.timestamp() + 4 * 86_400), resets_at: Some(now.timestamp() + 4 * 86_400), window_secs: 7 * 86_400 },
    ];
    UsageInfo {
        provider: NAME,
        installed: true,
        logged_in: true,
        plan: "plus".into(),
        limits,
        fetched: Some(now),
        local: {
            let home = dirs::home_dir().unwrap_or_default();
            super::demo_local(&[("gpt-5.1-codex", 4), ("gpt-5-mini", 1)], &[&home.join("Projects/cmdmeow").display().to_string(), &home.join("work/api-gateway").display().to_string()], 1.6)
        },
        billing: Billing::Subscription,
        source: "API",
        data_bytes: 1_300_000_000,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{read_on, LogTail};
    use super::*;

    fn token_line(ts: &str, total: i64, input: i64, cached: i64, out: i64, limits: &str) -> String {
        format!(
            "{{\"timestamp\":\"{ts}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1,\"reasoning_output_tokens\":0,\"total_tokens\":{total}}},\"last_token_usage\":{{\"input_tokens\":{input},\"cached_input_tokens\":{cached},\"output_tokens\":{out},\"reasoning_output_tokens\":0,\"total_tokens\":0}},\"model_context_window\":272000}},\"rate_limits\":{limits}}}}}\n"
        )
    }

    #[test]
    fn tokens_count_once_per_total() {
        let dir = std::env::temp_dir().join(format!("cmb-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rollout.jsonl");
        let body = "{\"timestamp\":\"2026-11-10T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/tmp/p\"}}\n".to_string()
            + &token_line("2026-11-10T10:00:01Z", 100, 1000, 800, 50, "null")
            + &token_line("2026-11-10T10:00:02Z", 100, 1000, 800, 50, "null")
            + &token_line("2026-11-10T10:01:00Z", 300, 500, 0, 20, "null");
        std::fs::write(&f, &body).unwrap();
        let mut log = LogTail::default();
        read_on(&f, body.len() as u64, &mut log, usage_entry);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].tokens(), 250);
        assert_eq!(log.entries[0].output, 50);
        assert_eq!(log.entries[0].cache_read, 800);
        assert_eq!(log.entries[1].tokens(), 520);
        assert_eq!(super::super::name(log.entries[0].project), "/tmp/p");
    }

    #[test]
    fn limits_from_last_snapshot() {
        let future = Local::now().timestamp() + 3600;
        let past = Local::now().timestamp() - 60;
        let rl = |p: f64| format!("{{\"primary\":{{\"used_percent\":{p},\"window_minutes\":300,\"resets_at\":{future}}},\"secondary\":{{\"used_percent\":30.0,\"window_minutes\":10080,\"resets_at\":{past}}},\"plan_type\":\"pro\"}}");
        let log = token_line("2026-11-10T10:00:01Z", 1, 1, 0, 1, &rl(10.0)) + &token_line("2026-11-10T10:05:00Z", 2, 1, 0, 1, &rl(25.0)) + &token_line("2026-11-10T10:06:00Z", 3, 1, 0, 1, "null");
        let l = limits_in(log.as_bytes()).unwrap();
        assert_eq!(l.plan, "pro");
        assert_eq!(l.limits.len(), 2);
        assert_eq!(l.limits[0].label, "Current session");
        assert_eq!(l.limits[0].pct, 25.0);
        assert_eq!(l.limits[0].resets_at, Some(future));
        assert_eq!(l.limits[0].window_secs, 300 * 60);
        assert!(!l.limits[0].resets.is_empty());
        // the weekly window reset since: shown as unused
        assert_eq!(l.limits[1].label, "Current week (all models)");
        assert_eq!(l.limits[1].pct, 0.0);
        let mut info = demo_info();
        info.limits = l.limits;
        assert_eq!(info.session().unwrap().pct, 25.0);
        assert_eq!(info.weekly().unwrap().pct, 0.0);
    }

    #[test]
    fn model_from_turn_context() {
        let mut ctx = ParseCtx::default();
        assert!(usage_entry(b"{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"/w\",\"model\":\"gpt-5.1-codex\"}}", &mut ctx).is_none());
        let line = token_line("2026-11-10T10:00:01Z", 7, 1_000_000, 0, 1_000_000, "null");
        let e = usage_entry(line.as_bytes(), &mut ctx).unwrap();
        assert_eq!(super::super::name(e.model), "gpt-5.1-codex");
        assert!((e.usd().unwrap() - 11.25).abs() < 1e-9);
    }

    #[test]
    fn chatgpt_or_api_key() {
        assert_eq!(billing(b"{\"auth_mode\":\"chatgpt\",\"tokens\":{}}"), Billing::Subscription);
        assert_eq!(billing(b"{\"OPENAI_API_KEY\":null,\"tokens\":{\"id_token\":\"x\"}}"), Billing::Subscription);
        assert_eq!(billing(b"{\"OPENAI_API_KEY\":\"sk-x\"}"), Billing::Api);
        assert_eq!(billing(b""), Billing::Unknown);
    }

    #[test]
    fn old_logs_say_seconds_until_reset() {
        let w: Value = serde_json::from_str("{\"used_percent\":5.5,\"window_minutes\":299,\"resets_in_seconds\":600}").unwrap();
        let l = window(&w, Local::now()).unwrap();
        assert_eq!(l.pct, 5.5);
        assert!(!l.resets.is_empty());
        assert!(limits_in(b"{\"rate_limits\":{}}").is_none());
    }
}
