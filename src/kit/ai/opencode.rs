//! OpenCode as a `Provider`: tokens only. OpenCode keeps its messages in a
//! SQLite database (`~/.local/share/opencode/opencode.db`, table `message`,
//! read through macOS's own `sqlite3`), and older versions in JSON files
//! under `storage/message/`. Each assistant message carries its model,
//! token counts and the folder it worked in.

use super::{intern, recent_files, FileCache, LocalTokens, Provider, Shared, UsageEntry, UsageInfo, LOG_DAYS};
use crate::kit::sync::LockExt;
use chrono::Local;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const NAME: &str = "OpenCode";

#[derive(Default)]
pub struct OpenCode {
    files: FileCache,
    /// The database's mtime at the last read, and what it held.
    db: Option<(SystemTime, Vec<UsageEntry>)>,
}

/// Where OpenCode keeps its data.
pub fn home() -> PathBuf {
    if let Some(d) = std::env::var_os("OPENCODE_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"));
    base.join("opencode")
}

impl Provider for OpenCode {
    fn name(&self) -> &'static str {
        NAME
    }
    fn fetch_account(&mut self, s: &Shared) {
        let installed = home().is_dir();
        let mut g = s.lock_ok();
        g.installed = installed;
        g.logged_in = installed;
        g.source = "Local";
        g.fetched = Some(Local::now());
        g.error = (!installed).then(|| "OpenCode hasn't been used on this Mac.".to_string());
    }
    fn local_tokens(&mut self) -> LocalTokens {
        let home = home();
        let mut all = self.files.update(recent_files(&home.join("storage/message"), 2, "json"), |_, b| serde_json::from_slice::<Value>(b).ok().and_then(|v| entry(&v)).into_iter().collect());
        let db = home.join("opencode.db");
        if let Some(mt) = std::fs::metadata(&db).ok().and_then(|m| m.modified().ok()) {
            if self.db.as_ref().map(|d| d.0) != Some(mt) {
                self.db = Some((mt, read_db(&db)));
            }
        }
        if let Some((_, v)) = &self.db {
            // the same message in both places counts once
            let mut seen: std::collections::HashSet<(i64, u64, u64)> = all.iter().map(|e| (e.ts, e.input, e.output)).collect();
            all.extend(v.iter().copied().filter(|e| seen.insert((e.ts, e.input, e.output))));
        }
        super::sum(all)
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
            local: super::demo_local(&[("claude-sonnet-4-6", 1)], &[&home.join("notes").display().to_string()], 0.2),
            ..Default::default()
        }
    }
}

/// The last month's messages in the database, through `sqlite3`.
fn read_db(db: &Path) -> Vec<UsageEntry> {
    let since_ms = (chrono::Utc::now().timestamp() - LOG_DAYS * 86_400) * 1000;
    let sql = format!("SELECT data FROM message WHERE time_created >= {since_ms}");
    let Ok(out) = std::process::Command::new("/usr/bin/sqlite3").args(["-readonly", "-json"]).arg(db).arg(sql).output() else { return Vec::new() };
    rows(&out.stdout)
}

/// Entries from `sqlite3 -json` output: `[{"data": "<message JSON>"}, …]`.
fn rows(json: &[u8]) -> Vec<UsageEntry> {
    let v: Value = serde_json::from_slice(json).unwrap_or(Value::Null);
    v.as_array().into_iter().flatten().filter_map(|r| r["data"].as_str()).filter_map(|d| serde_json::from_str::<Value>(d).ok()).filter_map(|m| entry(&m)).collect()
}

/// One assistant message.
fn entry(m: &Value) -> Option<UsageEntry> {
    if m["role"].as_str() != Some("assistant") {
        return None;
    }
    let t = &m["tokens"];
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let (input, output) = (n(&t["input"]), n(&t["output"]) + n(&t["reasoning"]));
    if input == 0 && output == 0 {
        return None;
    }
    let ts = m["time"]["created"].as_i64()? / 1000;
    let cwd = m["path"]["cwd"].as_str().or_else(|| m["path"]["root"].as_str()).unwrap_or("");
    Some(UsageEntry { ts, input, output, cache_write: n(&t["cache"]["write"]), cache_read: n(&t["cache"]["read"]), model: intern(m["modelID"].as_str().unwrap_or("")), project: intern(cwd), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_messages_from_the_database() {
        let msg = r#"{"role":"assistant","modelID":"claude-sonnet-4-6","time":{"created":1762768800000},"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":1000,"write":50}},"path":{"cwd":"/w/app"}}"#;
        let user = r#"{"role":"user","time":{"created":1762768800000}}"#;
        let out = serde_json::to_vec(&serde_json::json!([{ "data": msg }, { "data": user }, { "data": "not json" }])).unwrap();
        let v = rows(&out);
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].ts, v[0].input, v[0].output, v[0].cache_write, v[0].cache_read), (1762768800, 100, 25, 50, 1000));
        assert_eq!(super::super::name(v[0].project), "/w/app");
        assert!(rows(b"").is_empty());
    }
}
