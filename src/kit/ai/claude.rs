//! Claude Code as a `Provider`. Plan limits come from Claude's account,
//! signed in with Claude Code's own token (read from the Keychain once the
//! person allows it in setup or Settings), and from the Claude CLI (`claude auth status`,
//! `claude -p /usage`, which spends no tokens) when that doesn't work.
//! Local activity comes from counting usage fields in Claude Code's logs.

use crate::kit::sync::LockExt;
use super::account::{self, ApiError, ClaudeCreds, KeychainRead};
use super::{balanced, find, recent_jsonl, Billing, Contrib, Limit, LocalTokens, LogCache, ParseCtx, Provider, Shared, UsageEntry, UsageInfo};
use crate::kit::claude_code::{self as cc, claude_bin, claude_cmd};
use chrono::{DateTime, Datelike, Local};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const NAME: &str = "Claude";

#[derive(Default)]
pub struct ClaudeCode {
    cache: LogCache,
    /// Claude Code's sign-in, kept while it works.
    creds: Option<ClaudeCreds>,
    /// When the sign-in was last looked for (the Keychain is asked at most
    /// every few minutes).
    creds_at: Option<Instant>,
}

/// How often to look for Claude Code's sign-in again when there was none
/// (or it had run out: Claude Code renews it when it next runs).
const CREDS_RETRY: Duration = Duration::from_secs(10 * 60);

impl ClaudeCode {
    /// Claude Code's sign-in: the one in hand while it's fresh, else read
    /// again from its credentials file or the Keychain.
    fn creds(&mut self) -> Option<ClaudeCreds> {
        let now = chrono::Utc::now().timestamp();
        if let Some(c) = self.creds.as_ref().filter(|c| c.fresh(now)) {
            return Some(c.clone());
        }
        self.creds = None;
        // just connected from setup or Settings
        if let Some(c) = HANDED.lock_ok().take().filter(|c| c.fresh(now)) {
            self.creds_at = Some(Instant::now());
            self.creds = Some(c.clone());
            return Some(c);
        }
        if self.creds_at.is_some_and(|t| t.elapsed() < CREDS_RETRY) {
            return None;
        }
        self.creds_at = Some(Instant::now());
        let c = account::claude_creds_file().filter(|c| c.fresh(now)).or_else(read_keychain).filter(|c| c.fresh(now))?;
        self.creds = Some(c.clone());
        Some(c)
    }

    /// Limits from Claude's account. False when there's no usable sign-in
    /// or the call failed, so the CLI runs instead.
    fn fetch_api(&mut self, s: &Shared) -> bool {
        let Some(c) = self.creds() else { return false };
        match account::claude_usage(&c.token) {
            Ok(limits) => {
                cc::set_signed_in(Some(true));
                let mut g = s.lock_ok();
                g.installed = true;
                g.logged_in = true;
                g.signed_out = false;
                if !c.plan.is_empty() {
                    g.plan = c.plan;
                }
                if !c.tier.is_empty() {
                    g.tier = c.tier;
                }
                g.billing = Billing::Subscription;
                g.limits = limits;
                g.error = None;
                g.stale = false;
                g.source = "API";
                g.fetched = Some(Local::now());
                true
            }
            Err(e) => {
                eprintln!("[ai] claude usage: {e:?}");
                if e == ApiError::Auth {
                    // refused: read the sign-in again next time
                    self.creds = None;
                    self.creds_at = None;
                }
                false
            }
        }
    }
}

/// Claude Code's sign-in read by [`connect_keychain`], waiting for the
/// provider to pick it up (so macOS doesn't ask twice in a row).
static HANDED: std::sync::Mutex<Option<ClaudeCreds>> = std::sync::Mutex::new(None);

/// Claude Code's sign-in from the Keychain, read again once the person let
/// us (setup's permission row or Settings › AI Usage): never before that,
/// and only while a window is open, in case macOS asks again (someone is
/// there to answer). A Deny is remembered until they connect again.
fn read_keychain() -> Option<ClaudeCreds> {
    if !may_read(&super::settings::get().claude_keychain, crate::kit::activity::visible()) {
        return None;
    }
    match account::read_claude_keychain() {
        KeychainRead::Found(b) => account::parse_claude_creds(&b),
        KeychainRead::Denied => {
            super::settings::update(true, |s| s.claude_keychain = "denied".into());
            None
        }
        KeychainRead::Missing | KeychainRead::Failed => None,
    }
}

/// Whether the provider may read the Keychain on its own: only once the
/// person allowed it, and only with a window open.
fn may_read(state: &str, visible: bool) -> bool {
    state == "ok" && visible
}

/// What connecting to Claude Code's sign-in gave.
#[derive(Debug, PartialEq)]
pub enum Connect {
    Ok,
    Denied,
    /// No sign-in in the Keychain (Claude Code is signed out).
    Missing,
    Failed,
}

/// The person asked to use Claude Code's sign-in: read the Keychain now
/// (macOS shows its Allow / Always Allow / Deny prompt) and remember the
/// answer. Blocks while the prompt is up; run it off the ui thread.
pub fn connect_keychain() -> Connect {
    match account::read_claude_keychain() {
        KeychainRead::Found(b) => {
            super::settings::update(true, |s| s.claude_keychain = "ok".into());
            *HANDED.lock_ok() = account::parse_claude_creds(&b);
            Connect::Ok
        }
        KeychainRead::Denied => {
            super::settings::update(true, |s| s.claude_keychain = "denied".into());
            Connect::Denied
        }
        KeychainRead::Missing => Connect::Missing,
        KeychainRead::Failed => Connect::Failed,
    }
}

impl Provider for ClaudeCode {
    fn name(&self) -> &'static str {
        NAME
    }
    fn enabled(&self) -> bool {
        cc::tool_on()
    }
    fn fetch_account(&mut self, s: &Shared) {
        if !self.fetch_api(s) {
            fetch_cli(s);
        }
    }
    fn local_tokens(&mut self) -> LocalTokens {
        // project folders hold the session logs one level down
        // <project>/<session>.jsonl, and subagents' own logs in
        // <project>/<session>/subagents/, which count toward the limits too;
        // a reply copied into two folders counts once (`LogCache::entries`)
        let files = cc::log_dirs().iter().flat_map(|d| recent_jsonl(d, 3)).collect();
        self.cache.update(files, usage_entry)
    }
    fn data_dir(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".claude"))
    }
    fn demo(&self) -> UsageInfo {
        demo_info()
    }
}

fn fetch_cli(s: &Shared) {
    let Some(bin) = claude_bin() else {
        cc::set_signed_in(None);
        let mut g = s.lock_ok();
        g.installed = false;
        g.signed_out = false;
        g.limits.clear();
        g.error = Some(format!("Claude Code isn't installed on this Mac. {}", cc::Availability::NotInstalled.how()));
        return;
    };
    // plan + login
    let status = claude_cmd(&bin).args(["auth", "status", "--json"]).output();
    let status: Value = status.ok().and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or(Value::Null);
    let known = status["loggedIn"].as_bool();
    let plan = status["subscriptionType"].as_str().unwrap_or("").to_string();
    let tier = ["rateLimitTier", "subscriptionTier", "tier"].iter().find_map(|k| status[*k].as_str()).unwrap_or("").to_string();
    let logged_in = known.unwrap_or(false);
    cc::set_signed_in(known);
    // limits (a local slash command: no model call, no tokens)
    let usage = claude_cmd(&bin)
        .args(["-p", "/usage", "--output-format", "json", "--no-session-persistence"])
        .output();
    let mut g = s.lock_ok();
    g.installed = true;
    g.logged_in = logged_in;
    g.signed_out = known == Some(false);
    if !plan.is_empty() {
        g.plan = plan;
    }
    if !tier.is_empty() {
        g.tier = tier;
    }
    g.source = "CLI";
    match usage {
        Ok(o) => {
            let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
            let text = v["result"].as_str().unwrap_or("").to_string();
            let (limits, contrib, note) = parse_usage(&text);
            if limits.is_empty() {
                g.error = Some(if logged_in { "Claude didn't report limits right now.".into() } else { format!("Sign in to Claude Code to see your limits. {}", cc::Availability::SignedOut.how()) });
                if g.signed_out {
                    g.limits.clear();
                }
            } else {
                g.error = None;
                g.stale = false;
                g.limits = limits;
                g.contrib = contrib;
                g.note = note;
                g.fetched = Some(Local::now());
            }
            g.billing = billing(&status, &g.note);
            // limits from before stay up, marked as old, until a check works again
            g.stale = g.error.is_some() && !g.limits.is_empty();
        }
        Err(e) => {
            g.error = Some(format!("Couldn't run Claude: {e}"));
            g.stale = !g.limits.is_empty();
        }
    }
}

/// Parse the text of `/usage`.
pub fn parse_usage(text: &str) -> (Vec<Limit>, Vec<Contrib>, String) {
    let mut limits = Vec::new();
    let mut contrib: Vec<Contrib> = Vec::new();
    let mut note = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim();
        if let Some(idx) = t.find("% used") {
            if let Some((label, rest)) = t[..idx].rsplit_once(':') {
                let pct = rest.trim().parse::<f32>().unwrap_or(0.0);
                let resets = t[idx..].split("resets").nth(1).map(|s| s.trim().to_string()).unwrap_or_default();
                let label = label.trim().to_string();
                let low = label.to_lowercase();
                let window_secs = if low.contains("session") { 5 * 3600 } else if low.contains("week") { 7 * 86_400 } else { 0 };
                let resets_at = parse_reset(&resets, Local::now());
                limits.push(Limit { label, pct, resets, resets_at, window_secs });
                continue;
            }
        }
        if t.starts_with("Last ") && t.contains('·') {
            contrib.push(Contrib { title: t.to_string(), lines: vec![] });
            continue;
        }
        if line.starts_with("  ") && !t.is_empty() {
            if let Some(c) = contrib.last_mut() {
                c.lines.push(t.to_string());
                continue;
            }
        }
        if t.starts_with("You are currently") {
            note = t.to_string();
        }
    }
    (limits, contrib, note)
}

/// Subscription or API key, from `claude auth status --json` and the first
/// line of `/usage` ("You are currently using your subscription…").
pub fn billing(status: &Value, note: &str) -> Billing {
    let note = note.to_lowercase();
    if status["subscriptionType"].as_str().is_some_and(|s| !s.is_empty()) || note.contains("subscription") {
        return Billing::Subscription;
    }
    let says_key = ["authMethod", "apiKeySource", "apiProvider", "source"].iter().filter_map(|k| status[*k].as_str()).any(|v| {
        let v = v.to_lowercase();
        v.contains("api") || v.contains("key") || v.contains("bedrock") || v.contains("vertex")
    });
    if says_key || note.contains("api key") || note.contains("api usage") {
        Billing::Api
    } else {
        Billing::Unknown
    }
}

/// When a `/usage` reset text ("9:10pm", "Nov 8 at 5am", either maybe
/// followed by " (Europe/Madrid)") falls, as unix time. A bare time already
/// past today means tomorrow.
pub fn parse_reset(text: &str, now: DateTime<Local>) -> Option<i64> {
    let t = text.split(" (").next().unwrap_or(text).trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let (date, time) = match t.split_once(" at ") {
        Some((d, tm)) => (Some(d.trim().to_string()), tm.trim().to_string()),
        None => (None, t.clone()),
    };
    let time = time.replace(' ', "");
    let pm = time.ends_with("pm");
    let am = time.ends_with("am");
    if !pm && !am {
        return None;
    }
    let hm = &time[..time.len() - 2];
    let (h, m) = match hm.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (hm.parse::<u32>().ok()?, 0),
    };
    if !(1..=12).contains(&h) || m > 59 {
        return None;
    }
    let h24 = (h % 12) + if pm { 12 } else { 0 };
    let day = match date {
        Some(d) => {
            // "nov 8": this year, or next year when that's long past
            let parsed = chrono::NaiveDate::parse_from_str(&format!("{d} {}", now.year()), "%b %d %Y").ok()?;
            if parsed < now.date_naive() - chrono::Days::new(30) {
                chrono::NaiveDate::parse_from_str(&format!("{d} {}", now.year() + 1), "%b %d %Y").ok()?
            } else {
                parsed
            }
        }
        None => now.date_naive(),
    };
    let at = day.and_hms_opt(h24, m, 0)?.and_local_timezone(Local).earliest()?;
    let at = if date_is_bare(&t) && at < now { at + chrono::Duration::days(1) } else { at };
    Some(at.timestamp())
}

fn date_is_bare(t: &str) -> bool {
    !t.contains(" at ")
}

/// The usage in one log line (input + output + cache-write tokens, output
/// tokens), unless its message was counted already. Cache reads don't count.
fn usage_entry(line: &[u8], ctx: &mut ParseCtx) -> Option<UsageEntry> {
    // the folder the session works in, named by its lines
    if ctx.project == 0 {
        if let Some(ci) = find(line, b"\"cwd\":\"") {
            let rest = &line[ci + 7..];
            if let Some(e) = rest.iter().position(|&b| b == b'"') {
                ctx.project = super::intern(&String::from_utf8_lossy(&rest[..e]).replace("\\\\", "\\"));
            }
        }
    }
    let ui = find(line, b"\"usage\":{")?;
    let ti = find(line, b"\"timestamp\":\"")?;
    let ts_bytes = &line[ti + 13..];
    let te = ts_bytes.iter().position(|&b| b == b'"')?;
    let ts = std::str::from_utf8(&ts_bytes[..te]).ok().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp())?;
    // one reply can be logged in several lines; count each message id once
    if let Some(mi) = find(line, b"\"id\":\"msg_") {
        let rest = &line[mi + 6..];
        if let Some(e) = rest.iter().position(|&b| b == b'"') {
            if !ctx.ids.insert(rest[..e].to_vec()) {
                return None;
            }
        }
    }
    let obj = balanced(&line[ui + 8..]);
    let v: Value = serde_json::from_slice(obj).unwrap_or(Value::Null);
    let g = |k: &str| v[k].as_u64().unwrap_or(0);
    let write = g("cache_creation_input_tokens");
    let write_1h = v["cache_creation"]["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0).min(write);
    let model = find(line, b"\"model\":\"").and_then(|mi| {
        let rest = &line[mi + 9..];
        rest.iter().position(|&b| b == b'"').map(|e| super::intern(&String::from_utf8_lossy(&rest[..e])))
    });
    Some(UsageEntry {
        ts,
        input: g("input_tokens"),
        output: g("output_tokens"),
        cache_write: write - write_1h,
        cache_write_1h: write_1h,
        cache_read: g("cache_read_input_tokens"),
        model: model.unwrap_or(0),
        project: ctx.project,
    })
}

pub fn demo_info() -> UsageInfo {
    let (limits, contrib, note) = parse_usage(
        "You are currently using your subscription to power your Claude Code usage\n\n\
         Current session: 19% used · resets 9:10pm\n\
         Current week (all models): 5% used · resets Nov 8 at 5am\n\
         Current week (Opus): 0% used · resets Nov 8 at 5am\n\n\
         Last 24h · 637 requests · 8 sessions\n  95% of your usage was at >150k context\n  51% of your usage came from sessions active for 8+ hours\n",
    );
    // resets relative to now, so pace and forecast look like a real week
    let now = chrono::Utc::now().timestamp();
    let mut limits = limits;
    for (l, (pct, left)) in limits.iter_mut().zip([(19.0, 10_200), (62.0, 314_000), (4.0, 314_000)]) {
        l.pct = pct;
        l.resets_at = Some(now + left);
        l.resets = super::codex::reset_text(now + left);
    }
    // CMDMEOW_DEMO_LONG=1: the longest texts the widgets meet, for checking
    // nothing gets cut ("Resets tomorrow 23:40", "Out tomorrow 23:40")
    if std::env::var_os("CMDMEOW_DEMO_LONG").is_some() {
        let late = (Local::now() + chrono::Duration::days(1)).date_naive().and_hms_opt(23, 40, 0).and_then(|t| t.and_local_timezone(Local).single()).map(|t| t.timestamp()).unwrap_or(now);
        if let Some(s) = limits.first_mut() {
            (s.pct, s.window_secs, s.resets_at, s.resets) = (2.0, late - now + 3600, Some(late), super::codex::reset_text(late));
        }
        // week: ahead of pace, runs out tomorrow 23:40, resets two days later
        let end = late + 2 * 86_400;
        let start = end - 7 * 86_400;
        let pct = (100.0 * (now - start) as f64 / (late - start) as f64) as f32;
        for l in limits.iter_mut().skip(1) {
            (l.pct, l.resets_at, l.resets) = (pct.min(99.0), Some(end), super::codex::reset_text(end));
        }
    }
    UsageInfo {
        provider: NAME,
        loading: false,
        installed: true,
        logged_in: true,
        signed_out: false,
        plan: "max".into(),
        limits,
        contrib,
        note,
        error: None,
        fetched: Some(Local::now()),
        local: demo_local(),
        data_bytes: 5_400_000_000,
        billing: Billing::Subscription,
        source: "API",
        ..Default::default()
    }
}

fn demo_local() -> LocalTokens {
    let home = dirs::home_dir().unwrap_or_default();
    let p = |rel: &str| home.join(rel).display().to_string();
    super::demo_local(&[("claude-opus-4-6", 6), ("claude-sonnet-4-6", 3), ("claude-haiku-4-5", 1)], &[&p("Projects/cmdmeow"), &p("Projects/site"), &p("work/api-gateway"), &p("notes")], 3.0)
}

#[cfg(test)]
mod tests {
    use super::super::{read_on, LogTail};
    use super::*;
    use std::path::Path;

    #[test]
    fn keychain_only_after_consent() {
        // never before the person pressed Allow / Connect, never after a Deny
        assert!(!may_read("", true));
        assert!(!may_read("denied", true));
        // allowed, but nobody there to answer a prompt
        assert!(!may_read("ok", false));
        assert!(may_read("ok", true));
    }

    #[test]
    fn usage_text() {
        let info = demo_info();
        assert_eq!(info.limits.len(), 3);
        let s = info.session().unwrap();
        assert_eq!(s.pct, 19.0);
        assert!(s.resets_at.is_some());
        let (l, _, _) = parse_usage("Current session: 19% used · resets 9:10pm");
        assert_eq!((l[0].pct, l[0].resets.as_str(), l[0].window_secs), (19.0, "9:10pm", 5 * 3600));
        assert_eq!(info.weekly().unwrap().label, "Current week (all models)");
        assert_eq!(info.contrib.len(), 1);
        assert_eq!(info.contrib[0].lines.len(), 2);
        assert!(info.note.starts_with("You are currently"));
        assert_eq!(info.plan_title(), "Claude Max");
        let (l, c, n) = parse_usage("");
        assert!(l.is_empty() && c.is_empty() && n.is_empty());
    }

    #[test]
    fn a_resumed_session_copying_old_replies_counts_them_once() {
        let dir = std::env::temp_dir().join(format!("cmb-resume-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("p/s1/subagents")).unwrap();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let line = |id: &str, out: u64| format!("{{\"timestamp\":\"{now}\",\"message\":{{\"id\":\"{id}\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":{out}}}}}}}\n");
        std::fs::write(dir.join("p/s1.jsonl"), line("msg_a", 1) + &line("msg_b", 2)).unwrap();
        // the resumed session starts with a copy of s1, then goes on
        std::fs::write(dir.join("p/s2.jsonl"), line("msg_a", 1) + &line("msg_b", 2) + &line("msg_c", 3)).unwrap();
        // a subagent's own replies count too
        std::fs::write(dir.join("p/s1/subagents/agent-x.jsonl"), line("msg_d", 4)).unwrap();
        let mut cache = LogCache::default();
        let t = cache.update(recent_jsonl(&dir, 3), usage_entry);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(t.output_7d, 1 + 2 + 3 + 4);
    }

    #[test]
    fn usage_lines_counted_once_per_message() {
        let dir = std::env::temp_dir().join(format!("cmb-usage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.jsonl");
        let line = |id: &str| format!(
            "{{\"cwd\":\"/tmp/proj\",\"timestamp\":\"2026-11-08T10:00:00Z\",\"message\":{{\"id\":\"{id}\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":5,\"cache_creation_input_tokens\":100,\"cache_read_input_tokens\":9999}}}}}}\n"
        );
        let body = line("msg_1") + &line("msg_1") + &line("msg_2") + "{\"type\":\"user\"}\n";
        // a half-written last line waits for the next read
        let partial = line("msg_3");
        std::fs::write(&f, body.clone() + &partial[..40]).unwrap();
        let mut log = LogTail::default();
        let len = |f: &Path| std::fs::metadata(f).unwrap().len();
        read_on(&f, len(&f), &mut log, usage_entry);
        let cwd = crate::kit::claude_code::first_cwd(&f);
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.read, body.len() as u64);
        // cache reads are not counted
        assert_eq!(log.entries[0].tokens(), 115);
        assert_eq!(log.entries[0].output, 5);
        assert_eq!(log.entries[0].cache_read, 9999);
        assert_eq!(super::super::name(log.entries[0].project), "/tmp/proj");
        assert_eq!(cwd, Some(PathBuf::from("/tmp/proj")));
        // the log grows: only the new lines are read, repeats still count once
        std::fs::write(&f, body.clone() + &partial + &line("msg_2")).unwrap();
        read_on(&f, len(&f), &mut log, usage_entry);
        assert_eq!(log.entries.len(), 3);
        // the log was replaced by a shorter one: read again from the start
        std::fs::write(&f, line("msg_9")).unwrap();
        read_on(&f, len(&f), &mut log, usage_entry);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(log.entries.len(), 1);
    }

    #[test]
    fn model_cache_kinds_and_cost() {
        let line = b"{\"cwd\":\"/w/app\",\"timestamp\":\"2026-11-08T10:00:00Z\",\"message\":{\"model\":\"claude-sonnet-4-6\",\"id\":\"msg_x\",\"usage\":{\"input_tokens\":1000000,\"output_tokens\":1000000,\"cache_creation_input_tokens\":3000000,\"cache_read_input_tokens\":1000000,\"cache_creation\":{\"ephemeral_5m_input_tokens\":2000000,\"ephemeral_1h_input_tokens\":1000000}}}}";
        let mut ctx = ParseCtx::default();
        let e = usage_entry(line, &mut ctx).unwrap();
        assert_eq!(super::super::name(e.model), "claude-sonnet-4-6");
        assert_eq!(super::super::name(e.project), "/w/app");
        assert_eq!((e.cache_write, e.cache_write_1h), (2_000_000, 1_000_000));
        // 3 + 15 + 2 × 3.75 + 6 + 0.3
        assert!((e.usd().unwrap() - 31.8).abs() < 1e-6);
    }

    #[test]
    fn reset_times() {
        use chrono::TimeZone;
        let now = Local.with_ymd_and_hms(2026, 11, 12, 15, 0, 0).unwrap();
        let at = |t: &str| parse_reset(t, now).map(|ts| Local.timestamp_opt(ts, 0).unwrap().format("%Y-%m-%d %H:%M").to_string());
        assert_eq!(at("9:10pm").as_deref(), Some("2026-11-12 21:10"));
        assert_eq!(at("9:10pm (Europe/Madrid)").as_deref(), Some("2026-11-12 21:10"));
        // already past today: tomorrow
        assert_eq!(at("5am").as_deref(), Some("2026-11-13 05:00"));
        assert_eq!(at("Nov 18 at 5am").as_deref(), Some("2026-11-18 05:00"));
        assert_eq!(at("Jan 2 at 12pm").as_deref(), Some("2027-01-02 12:00"));
        assert_eq!(at(""), None);
        assert_eq!(at("soon"), None);
        let info = demo_info();
        assert_eq!(info.session().unwrap().window_secs, 5 * 3600);
        assert_eq!(info.weekly().unwrap().window_secs, 7 * 86_400);
    }

    #[test]
    fn subscription_or_api_key() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(billing(&v("{\"subscriptionType\":\"max\"}"), ""), Billing::Subscription);
        assert_eq!(billing(&Value::Null, "You are currently using your subscription to power your Claude Code usage"), Billing::Subscription);
        assert_eq!(billing(&v("{\"loggedIn\":true,\"apiKeySource\":\"ANTHROPIC_API_KEY\"}"), ""), Billing::Api);
        assert_eq!(billing(&Value::Null, ""), Billing::Unknown);
    }
}
