//! Google's Gemini CLI as a `Provider`. Its chats are saved as
//! `~/.gemini/tmp/<project>/chats/*.json` (or `.jsonl`), where each reply
//! from the model carries its token counts and model. Daily request quotas
//! come from Google's account when Gemini CLI is signed in with Google on a
//! plan that still serves it (`account::gemini_usage`); otherwise tokens only.

use super::account::ApiError;
use super::{intern, recent_files, FileCache, LocalTokens, Provider, Shared, UsageEntry, UsageInfo};
use crate::kit::sync::LockExt;
use chrono::{DateTime, Local};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const NAME: &str = "Gemini";

#[derive(Default)]
pub struct Gemini {
    cache: FileCache,
    /// After a failed quota call, the next one waits an hour: most Google
    /// sign-ins (personal accounts, API keys) get no quotas this way.
    quota_after: Option<std::time::Instant>,
}

/// Where Gemini CLI keeps its data (`$GEMINI_DATA_DIR`, else `~/.gemini`).
pub fn home() -> PathBuf {
    std::env::var_os("GEMINI_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".gemini"))
}

impl Provider for Gemini {
    fn name(&self) -> &'static str {
        NAME
    }
    fn fetch_account(&mut self, s: &Shared) {
        let home = home();
        let installed = home.join("tmp").is_dir();
        let now = std::time::Instant::now();
        let due = installed && self.quota_after.is_none_or(|t| now >= t);
        let live = due.then(|| super::account::gemini_usage(&home));
        if let Some(Err(e)) = &live {
            eprintln!("[ai] gemini quota: {e:?}");
            // no Google sign-in that gets quotas: ask again in an hour; a
            // passing failure (offline, a 5xx) only skips one round
            let wait = if *e == ApiError::Auth { 3600 } else { 240 };
            self.quota_after = Some(now + std::time::Duration::from_secs(wait));
        }
        let mut g = s.lock_ok();
        g.installed = installed;
        g.logged_in = installed;
        g.fetched = Some(Local::now());
        g.error = (!installed).then(|| "Gemini CLI hasn't been used on this Mac.".to_string());
        match live {
            Some(Ok((plan, limits))) => {
                g.source = "API";
                g.plan = plan;
                g.limits = limits;
                g.stale = false;
                g.billing = super::Billing::Subscription;
            }
            // the last quotas known stay up, marked as old
            Some(Err(ApiError::Other(_))) | None if !g.limits.is_empty() && installed => g.stale = true,
            _ => {
                g.source = "Local";
                g.limits.clear();
                g.stale = false;
            }
        }
    }
    fn local_tokens(&mut self) -> LocalTokens {
        let tmp = home().join("tmp");
        let mut files = recent_files(&tmp, 3, "json");
        files.extend(recent_files(&tmp, 3, "jsonl"));
        files.retain(|p| p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "chats"));
        super::sum(self.cache.update(files, parse))
    }
    fn data_dir(&self) -> Option<PathBuf> {
        Some(home())
    }
    fn demo(&self) -> UsageInfo {
        let home = dirs::home_dir().unwrap_or_default();
        UsageInfo {
            provider: NAME,
            installed: true,
            logged_in: true,
            source: "Local",
            fetched: Some(Local::now()),
            local: super::demo_local(&[("gemini-2.5-pro", 3), ("gemini-2.5-flash", 2)], &[&home.join("Projects/site").display().to_string()], 0.8),
            ..Default::default()
        }
    }
}

/// The replies in one chat file: a JSON session (`{"messages": [...]}`) or
/// JSON lines, one record each.
fn parse(path: &Path, bytes: &[u8]) -> Vec<UsageEntry> {
    let project = project_of(path);
    let mut out = Vec::new();
    let mut add = |rec: &Value| {
        if let Some(e) = entry(rec, project) {
            out.push(e);
        }
    };
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v) => match v["messages"].as_array() {
            Some(ms) => ms.iter().for_each(&mut add),
            None => add(&v),
        },
        Err(_) => {
            for line in bytes.split(|&b| b == b'\n') {
                if let Ok(v) = serde_json::from_slice::<Value>(line) {
                    add(&v);
                }
            }
        }
    }
    out
}

/// One model reply: input includes the cached part, output adds thinking.
fn entry(rec: &Value, project: u32) -> Option<UsageEntry> {
    if rec["type"].as_str() != Some("gemini") {
        return None;
    }
    let t = &rec["tokens"];
    let n = |k: &str| t[k].as_u64().unwrap_or(0);
    let input = n("input");
    if input == 0 && n("output") == 0 {
        return None;
    }
    let ts = rec["timestamp"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok())?.timestamp();
    let cached = n("cached").min(input);
    Some(UsageEntry { ts, input: input - cached, output: n("output") + n("thoughts"), cache_read: cached, model: intern(rec["model"].as_str().unwrap_or("")), project, ..Default::default() })
}

/// The project folder of `~/.gemini/tmp/<project>/chats/<file>`, when Gemini
/// wrote it down (`.project_root`).
fn project_of(chat: &Path) -> u32 {
    let Some(dir) = chat.parent().and_then(|c| c.parent()) else { return 0 };
    std::fs::read_to_string(dir.join(".project_root")).ok().map(|s| intern(s.trim())).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_from_a_session_and_from_lines() {
        let session = br#"{"sessionId":"s","messages":[
            {"type":"user","timestamp":"2026-11-10T10:00:00Z","content":"hi"},
            {"type":"gemini","timestamp":"2026-11-10T10:00:05Z","model":"gemini-2.5-pro","tokens":{"input":1000,"output":200,"cached":400,"thoughts":50,"tool":0,"total":1250}}
        ]}"#;
        let v = parse(Path::new("/x/chats/s.json"), session);
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].input, v[0].output, v[0].cache_read), (600, 250, 400));
        assert_eq!(super::super::name(v[0].model), "gemini-2.5-pro");
        let lines = b"{\"type\":\"gemini\",\"timestamp\":\"2026-11-10T10:00:05Z\",\"tokens\":{\"input\":10,\"output\":2}}\n{\"type\":\"user\"}\n";
        assert_eq!(parse(Path::new("/x/chats/s.jsonl"), lines).len(), 1);
        assert!(parse(Path::new("/x/chats/s.json"), b"garbage").is_empty());
    }
}
