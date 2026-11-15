//! Which sessions of the main AI agent are busy. Each agent writes its
//! session as it works, so a session written in the last couple of minutes
//! is working. Where the log shows a tool call still waiting for its result
//! (a long build, tests), that counts as working too, even though the log
//! stays quiet while it runs.
//!
//! - Claude Code: `~/.claude/projects/<project>/<session>.jsonl` (subagents
//!   in a folder named after the session). Open tool calls: yes.
//! - Codex: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, the folder in
//!   its `cwd`. Open tool calls: yes.
//! - OpenCode: messages in `opencode.db` (through `sqlite3`) or, in older
//!   versions, `storage/message/<session>/*.json`. A reply without a
//!   `completed` time is still running.
//! - Gemini CLI: `~/.gemini/tmp/<project>/chats/*.json`, rewritten as the
//!   chat goes. Writes only: a long tool call looks quiet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Sessions older than this aren't listed at all.
const RECENT_SECS: i64 = 24 * 3600;
/// A tool call silent for longer than this is taken as abandoned (Claude
/// quit or crashed mid-call).
pub const TOOL_CAP_SECS: i64 = 60 * 60;
/// How much of a log's end is read to find an open tool call.
const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// The project's folder name (where Claude Code ran).
    pub project: String,
    /// Unix seconds of the last write to the session's logs.
    pub last_write: i64,
    /// A tool call is under way (no result logged yet).
    pub tool_running: bool,
}

impl Session {
    /// When the session last showed it was busy: now while a tool runs
    /// (up to [`TOOL_CAP_SECS`]), else its last write.
    pub fn active_at(&self, now: i64) -> i64 {
        if self.tool_running && now - self.last_write <= TOOL_CAP_SECS {
            now
        } else {
            self.last_write
        }
    }
    pub fn working(&self, now: i64) -> bool {
        now - self.active_at(now) <= super::engine::WORKING_SECS
    }
}

/// An AI agent whose work can keep the Mac awake. Saved by its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
    Gemini,
    OpenCode,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::Claude, Source::Codex, Source::Gemini, Source::OpenCode];

    /// By provider id ("claude", "codex", …), as `ai::settings` keeps it.
    pub fn of(id: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn id(self) -> &'static str {
        match self {
            Source::Claude => "claude",
            Source::Codex => "codex",
            Source::Gemini => "gemini",
            Source::OpenCode => "opencode",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Source::Claude => crate::kit::ai::claude::NAME,
            Source::Codex => crate::kit::ai::codex::NAME,
            Source::Gemini => crate::kit::ai::gemini::NAME,
            Source::OpenCode => crate::kit::ai::opencode::NAME,
        }
    }

    /// The folder its sessions are written to.
    pub fn root(self) -> PathBuf {
        match self {
            Source::Claude => crate::kit::claude_code::projects_dir(),
            Source::Codex => crate::kit::ai::codex::home().join("sessions"),
            Source::Gemini => crate::kit::ai::gemini::home().join("tmp"),
            Source::OpenCode => crate::kit::ai::opencode::home(),
        }
    }

    /// The main agent picked in setup (Settings › AI Usage), when it has
    /// been used on this Mac. None: nothing to watch, so the trigger hides.
    pub fn main() -> Option<Source> {
        Source::of(&crate::kit::ai::settings::get().main).filter(|s| s.root().is_dir())
    }
}

/// Remembers project names so each log's first lines are read only once.
#[derive(Default)]
pub struct Scanner {
    names: HashMap<PathBuf, String>,
    /// Whether a log ends in an open tool call, by (log, mtime).
    open: HashMap<PathBuf, (i64, bool)>,
    /// OpenCode: the database's mtimes at the last read, and its messages.
    db: Option<((i64, i64), Vec<Msg>)>,
}

impl Scanner {
    /// `source`'s sessions written to in the last day, newest first.
    pub fn scan(&mut self, source: Source, now: i64) -> Vec<Session> {
        let root = source.root();
        let mut out = match source {
            Source::Claude => self.scan_claude(&root, now),
            Source::Codex => self.scan_codex(&root, now),
            Source::Gemini => self.scan_gemini(&root, now),
            Source::OpenCode => self.scan_opencode(&root, now),
        };
        out.sort_by_key(|s| std::cmp::Reverse(s.last_write));
        out
    }

    fn scan_claude(&mut self, root: &Path, now: i64) -> Vec<Session> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(root) else { return out };
        for e in rd.flatten() {
            let folder = e.path();
            if !folder.is_dir() {
                continue;
            }
            // session id → (newest write, a log to read the project from)
            let mut by_session: HashMap<String, (i64, PathBuf)> = HashMap::new();
            let Ok(files) = std::fs::read_dir(&folder) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
                let newest = if p.is_dir() { newest_log(&p, 2) } else if is_log(&p) { mtime(&p).map(|t| (t, p.clone())) } else { None };
                if let Some((t, log)) = newest {
                    let slot = by_session.entry(stem).or_insert((t, log.clone()));
                    if t > slot.0 {
                        *slot = (t, log);
                    }
                }
            }
            for (_, (t, log)) in by_session {
                if now - t > RECENT_SECS {
                    continue;
                }
                let project = self.name(&folder, &log);
                let tool_running = now - t <= TOOL_CAP_SECS && self.tool_open(&log, t);
                out.push(Session { project, last_write: t, tool_running });
            }
        }
        out
    }

    /// One rollout log per session; the project is the log's `cwd`.
    fn scan_codex(&mut self, root: &Path, now: i64) -> Vec<Session> {
        let mut out = Vec::new();
        for log in crate::kit::ai::recent_jsonl(root, 3) {
            let Some(t) = mtime(&log) else { continue };
            if now - t > RECENT_SECS {
                continue;
            }
            let project = match self.names.get(&log) {
                Some(n) => n.clone(),
                None => match crate::kit::claude_code::first_cwd(&log).and_then(|c| c.file_name().map(|s| s.to_string_lossy().into_owned())) {
                    Some(n) => {
                        self.names.insert(log.clone(), n.clone());
                        n
                    }
                    None => Source::Codex.name().into(),
                },
            };
            let tool_running = now - t <= TOOL_CAP_SECS && self.cached_open(&log, t, codex_open_call);
            out.push(Session { project, last_write: t, tool_running });
        }
        out
    }

    /// `tmp/<project>/chats/<chat>.json`: each chat is a session, named
    /// after the folder Gemini wrote down for the project.
    fn scan_gemini(&mut self, root: &Path, now: i64) -> Vec<Session> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(root) else { return out };
        for e in rd.flatten() {
            let dir = e.path();
            let Ok(chats) = std::fs::read_dir(dir.join("chats")) else { continue };
            for c in chats.flatten() {
                let p = c.path();
                if !p.extension().is_some_and(|x| x == "json" || x == "jsonl") {
                    continue;
                }
                let Some(t) = mtime(&p) else { continue };
                if now - t > RECENT_SECS {
                    continue;
                }
                let project = std::fs::read_to_string(dir.join(".project_root"))
                    .ok()
                    .and_then(|r| Path::new(r.trim()).file_name().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| Source::Gemini.name().into());
                out.push(Session { project, last_write: t, tool_running: false });
            }
        }
        out
    }

    /// Messages grouped by session. The database is read again only when
    /// it (or its write-ahead log) changed.
    fn scan_opencode(&mut self, root: &Path, now: i64) -> Vec<Session> {
        let db = root.join("opencode.db");
        let mut msgs = Vec::new();
        if let Some(mt) = mtime(&db) {
            let key = (mt, mtime(&root.join("opencode.db-wal")).unwrap_or(0));
            if self.db.as_ref().map(|d| d.0) != Some(key) {
                // a failed read (locked, no sqlite3) is tried again next scan
                self.db = read_opencode_db(&db, now).map(|m| (key, m));
            }
            msgs.extend(self.db.as_ref().map(|d| d.1.clone()).unwrap_or_default());
        }
        // older versions: storage/message/<session>/<message>.json
        for f in crate::kit::ai::recent_files(&root.join("storage/message"), 1, "json") {
            if mtime(&f).is_some_and(|t| now - t <= RECENT_SECS) {
                let v: serde_json::Value = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                let session = f.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                msgs.extend(Msg::from_json(&v, &session, 0));
            }
        }
        opencode_sessions(&msgs, now)
    }

    fn tool_open(&mut self, log: &Path, mtime: i64) -> bool {
        self.cached_open(log, mtime, open_tool_call)
    }

    fn cached_open(&mut self, log: &Path, mtime: i64, decide: fn(&[u8]) -> bool) -> bool {
        if let Some((m, v)) = self.open.get(log) {
            if *m == mtime {
                return *v;
            }
        }
        let v = decide(&tail(log));
        self.open.insert(log.to_path_buf(), (mtime, v));
        v
    }

    fn name(&mut self, folder: &Path, log: &Path) -> String {
        if let Some(n) = self.names.get(folder) {
            return n.clone();
        }
        // only a name read from a log is kept; the folder's guess is retried
        match crate::kit::claude_code::first_cwd(log).and_then(|c| c.file_name().map(|s| s.to_string_lossy().into_owned())) {
            Some(n) => {
                self.names.insert(folder.to_path_buf(), n.clone());
                n
            }
            None => fallback_name(folder),
        }
    }
}

/// The end of a log, where an open tool call would be.
fn tail(log: &Path) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(log) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    buf
}

/// Does Claude's log end asking for a tool whose result isn't logged yet?
/// The last assistant or user entry decides.
fn open_tool_call(text: &[u8]) -> bool {
    let has = |l: &[u8], pat: &[u8]| l.windows(pat.len()).any(|w| w == pat);
    for line in text.split(|b| *b == b'\n').rev() {
        if has(line, b"\"tool_result\"") {
            return false;
        }
        if has(line, b"\"type\":\"assistant\"") {
            return has(line, b"\"type\":\"tool_use\"");
        }
        if has(line, b"\"type\":\"user\"") {
            return false;
        }
    }
    false
}

/// The same for a Codex rollout: the last call, its output, a message or
/// the end of the turn decides. Token counts and reasoning don't.
fn codex_open_call(text: &[u8]) -> bool {
    let has = |l: &[u8], pat: &[u8]| l.windows(pat.len()).any(|w| w == pat);
    for line in text.split(|b| *b == b'\n').rev() {
        if has(line, b"\"type\":\"function_call_output\"") || has(line, b"\"type\":\"custom_tool_call_output\"") {
            return false;
        }
        if has(line, b"\"type\":\"function_call\"") || has(line, b"\"type\":\"custom_tool_call\"") || has(line, b"\"type\":\"local_shell_call\"") {
            return true;
        }
        if has(line, b"\"type\":\"message\"") || has(line, b"\"type\":\"task_complete\"") || has(line, b"\"type\":\"turn_aborted\"") {
            return false;
        }
    }
    false
}

/// One OpenCode message, as far as being busy goes.
#[derive(Clone, Debug, PartialEq)]
struct Msg {
    session: String,
    project: String,
    /// Unix seconds of its last change.
    at: i64,
    /// An assistant reply that hasn't finished.
    running: bool,
}

impl Msg {
    /// From a message's JSON; `session` and `updated_ms` from its row, when
    /// the JSON doesn't carry them.
    fn from_json(m: &serde_json::Value, session: &str, updated_ms: i64) -> Option<Msg> {
        let session = m["sessionID"].as_str().unwrap_or(session);
        if session.is_empty() {
            return None;
        }
        let t = &m["time"];
        let created = t["created"].as_i64()?;
        let completed = t["completed"].as_i64();
        let at = created.max(completed.unwrap_or(0)).max(updated_ms) / 1000;
        let cwd = m["path"]["cwd"].as_str().or_else(|| m["path"]["root"].as_str()).unwrap_or("");
        let project = Path::new(cwd).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let running = m["role"].as_str() == Some("assistant") && completed.is_none() && m["error"].is_null();
        Some(Msg { session: session.to_string(), project, at, running })
    }
}

/// The last day's messages in OpenCode's database, through `sqlite3`.
/// None when it couldn't be read.
fn read_opencode_db(db: &Path, now: i64) -> Option<Vec<Msg>> {
    let since_ms = (now - RECENT_SECS) * 1000;
    let sql = format!("SELECT * FROM message WHERE time_created >= {since_ms}");
    let out = std::process::Command::new("/usr/bin/sqlite3").args(["-readonly", "-json"]).arg(db).arg(sql).output().ok()?;
    out.status.success().then(|| opencode_rows(&out.stdout))
}

/// Messages from `sqlite3 -json` rows: `[{"session_id": …, "data": "<JSON>"}, …]`.
fn opencode_rows(json: &[u8]) -> Vec<Msg> {
    let v: serde_json::Value = serde_json::from_slice(json).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let data: serde_json::Value = serde_json::from_str(r["data"].as_str()?).ok()?;
            let session = r["session_id"].as_str().or_else(|| r["sessionID"].as_str()).unwrap_or("");
            Msg::from_json(&data, session, r["time_updated"].as_i64().unwrap_or(0))
        })
        .collect()
}

/// One session per OpenCode session id: its newest change, and whether its
/// newest reply is still running.
fn opencode_sessions(msgs: &[Msg], now: i64) -> Vec<Session> {
    let mut by: HashMap<&str, (i64, bool, &str)> = HashMap::new();
    for m in msgs {
        let slot = by.entry(&m.session).or_insert((i64::MIN, false, ""));
        if m.at >= slot.0 {
            *slot = (m.at, m.running, if m.project.is_empty() { slot.2 } else { &m.project });
        } else if slot.2.is_empty() {
            slot.2 = &m.project;
        }
    }
    by.into_values()
        .filter(|(t, ..)| now - t <= RECENT_SECS)
        .map(|(t, running, project)| Session {
            project: if project.is_empty() { Source::OpenCode.name().into() } else { project.into() },
            last_write: t,
            tool_running: running && now - t <= TOOL_CAP_SECS,
        })
        .collect()
}

/// The last dash-separated part of the encoded folder name.
fn fallback_name(folder: &Path) -> String {
    let enc = folder.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    enc.rsplit('-').find(|s| !s.is_empty()).unwrap_or(&enc).to_string()
}

fn is_log(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "jsonl")
}

fn mtime(p: &Path) -> Option<i64> {
    let t = p.metadata().ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// The newest `.jsonl` under `dir`, `depth` folders deep at most.
fn newest_log(dir: &Path, depth: u32) -> Option<(i64, PathBuf)> {
    let mut best: Option<(i64, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let found = if p.is_dir() {
            if depth > 0 { newest_log(&p, depth - 1) } else { None }
        } else if is_log(&p) {
            mtime(&p).map(|t| (t, p))
        } else {
            None
        };
        if let Some(f) = found {
            if best.as_ref().is_none_or(|b| f.0 > b.0) {
                best = Some(f);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sessions_and_subagent_logs() {
        let root = std::env::temp_dir().join(format!("cmb-awake-sessions-{}", std::process::id()));
        let proj = root.join("-Users-me-work-cmdmeow");
        std::fs::create_dir_all(proj.join("abc/subagents")).unwrap();
        std::fs::write(proj.join("abc.jsonl"), "{\"cwd\":\"/Users/me/work/cmdmeow\"}\n").unwrap();
        std::fs::write(proj.join("abc/subagents/agent-1.jsonl"), "{}\n").unwrap();
        std::fs::write(proj.join("def.jsonl"), "{}\n").unwrap();
        std::fs::write(proj.join("notes.txt"), "x").unwrap();
        let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let mut s = Scanner::default();
        let found = s.scan_claude(&root, now);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|x| x.project == "cmdmeow"));
        assert!(s.scan_claude(&root, now + RECENT_SECS + 60).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_open_tool_call_counts_as_working() {
        let a_tool = br#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash"}]}}"#;
        let its_result = br#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#;
        let text = br#"{"type":"assistant","message":{"content":[{"type":"text","text":"done"}]}}"#;
        let log = |ls: &[&[u8]]| ls.join(&b'\n');
        assert!(open_tool_call(&log(&[text, a_tool, b""])));
        assert!(!open_tool_call(&log(&[a_tool, its_result])));
        assert!(!open_tool_call(&log(&[a_tool, its_result, text])));
        assert!(!open_tool_call(b""));
        let s = Session { project: "x".into(), last_write: 1000, tool_running: true };
        assert!(s.working(1000 + 1800), "a long build keeps it working");
        assert!(!s.working(1000 + TOOL_CAP_SECS + 200), "an abandoned call stops counting");
        let quiet = Session { tool_running: false, ..s };
        assert!(!quiet.working(1000 + 1800));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cmb-awake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    }

    #[test]
    fn codex_open_calls() {
        let call = br#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}","call_id":"c1"}}"#;
        let output = br#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#;
        let tokens = br#"{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":null}}"#;
        let reasoning = br#"{"timestamp":"t","type":"response_item","payload":{"type":"reasoning","summary":[]}}"#;
        let answer = br#"{"timestamp":"t","type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#;
        let patch = br#"{"timestamp":"t","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c2"}}"#;
        let log = |ls: &[&[u8]]| ls.join(&b'\n');
        assert!(codex_open_call(&log(&[reasoning, call, tokens, b""])), "a call, then only token counts");
        assert!(!codex_open_call(&log(&[call, output, tokens])));
        assert!(!codex_open_call(&log(&[call, output, answer])));
        assert!(codex_open_call(&log(&[answer, patch])));
        assert!(!codex_open_call(b""));
    }

    #[test]
    fn codex_sessions_are_named_after_their_folder() {
        let root = tmp("codex");
        let day = root.join("2026/11/13");
        std::fs::create_dir_all(&day).unwrap();
        let meta = r#"{"timestamp":"t","type":"session_meta","payload":{"id":"x","cwd":"/Users/me/work/api-server","originator":"codex_cli_rs"}}"#;
        let call = r#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1"}}"#;
        std::fs::write(day.join("rollout-a.jsonl"), format!("{meta}\n{call}\n")).unwrap();
        std::fs::write(day.join("rollout-b.jsonl"), "{}\n").unwrap();
        let mut s = Scanner::default();
        let mut found = s.scan_codex(&root, unix_now());
        found.sort_by(|a, b| a.project.cmp(&b.project));
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].project.as_str(), found[0].tool_running), ("Codex", false));
        assert_eq!((found[1].project.as_str(), found[1].tool_running), ("api-server", true));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gemini_chats_are_sessions() {
        let root = tmp("gemini");
        let proj = root.join("9f2c");
        std::fs::create_dir_all(proj.join("chats")).unwrap();
        std::fs::write(proj.join(".project_root"), "/Users/me/notes\n").unwrap();
        std::fs::write(proj.join("chats/session-1.json"), "{}").unwrap();
        std::fs::write(proj.join("chats/readme.txt"), "x").unwrap();
        let found = Scanner::default().scan_gemini(&root, unix_now());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].project, "notes");
        assert!(found[0].working(unix_now()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opencode_running_reply_keeps_its_session_working() {
        let now = 1_800_000_000;
        let ms = |t: i64| t * 1000;
        let row = |sid: &str, data: String| serde_json::json!({ "id": "m", "session_id": sid, "time_updated": 0, "data": data });
        let done = serde_json::json!({ "role": "assistant", "time": { "created": ms(now - 4000), "completed": ms(now - 3900) }, "path": { "cwd": "/w/site" } }).to_string();
        let asked = serde_json::json!({ "role": "user", "time": { "created": ms(now - 600) } }).to_string();
        let running = serde_json::json!({ "role": "assistant", "time": { "created": ms(now - 590) }, "path": { "cwd": "/w/app" } }).to_string();
        let rows = serde_json::Value::Array(vec![row("s1", done), row("s2", asked), row("s2", running)]);
        let msgs = opencode_rows(rows.to_string().as_bytes());
        assert_eq!(msgs.len(), 3);
        let mut found = opencode_sessions(&msgs, now);
        found.sort_by(|a, b| a.project.cmp(&b.project));
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].project, "app");
        assert!(found[0].working(now), "a reply without a completed time is still running");
        assert_eq!(found[1].project, "site");
        assert!(!found[1].working(now));
        assert!(opencode_rows(b"garbage").is_empty());
    }

    #[test]
    fn sources_follow_provider_ids() {
        assert_eq!(Source::of("codex"), Some(Source::Codex));
        assert_eq!(Source::of("opencode").map(Source::name), Some("OpenCode"));
        assert_eq!(Source::of("cursor"), None);
        assert!(Source::ALL.iter().all(|s| Source::of(s.id()) == Some(*s)));
        // every agent a person can pick in setup can be watched
        assert!(crate::kit::ai::PROVIDERS.iter().all(|m| Source::of(m.id).is_some()));
    }

    #[test]
    fn name_from_folder_when_log_has_no_cwd() {
        assert_eq!(fallback_name(Path::new("/x/-Users-me-api-server")), "server");
        assert_eq!(fallback_name(Path::new("/x/-Users-me-app-")), "app");
    }
}
