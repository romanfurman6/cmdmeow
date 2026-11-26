//! AI coding assistants on this Mac, behind one `Provider` trait: plan and
//! limit windows and local token counts (per model and project) from session
//! logs. Each provider fills the same `UsageInfo`, and
//! whatever it can't tell stays empty, so one UI shows any of them.

pub mod account;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;
pub mod history;
pub mod pace;
pub mod price;
pub mod settings;
pub mod alerts;
pub mod status;
pub mod statusline;

use crate::kit::claude_code::Availability;
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Utc};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// One limit window, e.g. "Current session" at 19%, resets 9:10pm.
#[derive(Clone, Debug, Default)]
pub struct Limit {
    pub label: String,
    pub pct: f32,
    pub resets: String,
    /// When the window resets (unix time), if known.
    pub resets_at: Option<i64>,
    /// How long the window is, in seconds (0 when unknown).
    pub window_secs: i64,
}

/// How the person pays for the assistant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Billing {
    #[default]
    Unknown,
    /// A plan with usage limits (Claude Pro/Max, ChatGPT Plus/Pro).
    Subscription,
    /// Pay per token with an API key.
    Api,
}

/// A block of extra lines about where usage came from.
#[derive(Clone, Debug, Default)]
pub struct Contrib {
    pub title: String,
    pub lines: Vec<String>,
}

/// Everything one provider reported.
#[derive(Clone, Debug, Default)]
pub struct UsageInfo {
    /// The provider's display name, e.g. "Claude".
    pub provider: &'static str,
    /// A check the person waits on is running: the first one after launch,
    /// or a Refresh. Quiet checks in the background leave it off.
    pub loading: bool,
    /// The first check (account and local logs) is done. Until then the
    /// page says it's checking; after it, a refresh never swaps the layout.
    pub scanned: bool,
    pub installed: bool,
    pub logged_in: bool,
    /// The provider said plainly that nobody is signed in.
    pub signed_out: bool,
    pub plan: String,
    /// Limit windows. Providers name the 5-hour one "…session…" and the
    /// weekly ones "…week…", so `session()` and `weekly()` find them.
    pub limits: Vec<Limit>,
    pub contrib: Vec<Contrib>,
    pub note: String,
    pub error: Option<String>,
    pub fetched: Option<DateTime<Local>>,
    pub local: LocalTokens,
    pub billing: Billing,
    /// Extra plan detail that can tell the price, e.g. a rate-limit tier
    /// like "default_claude_max_20x".
    pub tier: String,
    /// Where the limits came from: "API" (the provider's account), "CLI"
    /// or "Local" (session logs). Not shown; kept for the logs and tests.
    pub source: &'static str,
    /// The last check failed: the limits shown are the last ones known, as
    /// of `fetched`.
    pub stale: bool,
    /// An outage or slowdown the provider's status page reports right now:
    /// "Degraded" or "Outage".
    pub incident: Option<String>,
    /// The open incidents behind it, for the badge's hover.
    pub incidents: Vec<status::Incident>,
    /// The account's name when the provider has more than one ("Work").
    pub account: String,
    /// The account's config folder, as "~/.claude-work".
    pub folder: String,
    /// The provider's other accounts (Claude), each with its own data.
    pub accounts: Vec<UsageInfo>,
}

impl UsageInfo {
    pub fn new(provider: &'static str) -> Self {
        UsageInfo { provider, ..Default::default() }
    }
    /// "Claude", or "Claude · Work" for one of several accounts.
    pub fn display_name(&self) -> String {
        if self.account.is_empty() { self.provider.to_string() } else { format!("{} · {}", self.provider, self.account) }
    }
    /// This and its other accounts, each on its own.
    pub fn each_account(&self) -> Vec<UsageInfo> {
        let mut v = vec![UsageInfo { accounts: Vec::new(), ..self.clone() }];
        v.extend(self.accounts.iter().cloned());
        v
    }
    /// The assistant is missing or signed out (known only after the first check).
    pub fn unavailable(&self) -> Option<Availability> {
        if !self.scanned {
            None
        } else if !self.installed {
            Some(Availability::NotInstalled)
        } else if self.signed_out && self.limits.is_empty() {
            Some(Availability::SignedOut)
        } else {
            None
        }
    }
    pub fn session(&self) -> Option<&Limit> {
        self.limits.iter().find(|l| l.label.to_lowercase().contains("session"))
    }
    pub fn weekly(&self) -> Option<&Limit> {
        self.limits
            .iter()
            .find(|l| l.label.to_lowercase().contains("all models"))
            .or_else(|| self.limits.iter().find(|l| l.label.to_lowercase().contains("week")))
    }
    /// "Claude Max", or just the provider name when the plan is unknown.
    /// Monthly price of the plan in dollars, when known.
    pub fn plan_usd(&self) -> Option<f64> {
        settings::get().plan_usd.get(self.provider).copied().or_else(|| price::plan_usd(self.provider, &self.tier)).or_else(|| price::plan_usd(self.provider, &self.plan))
    }
    /// Pays per token (API key), so dollars matter more than limits.
    pub fn is_api(&self) -> bool {
        self.billing == Billing::Api
    }
    pub fn plan_title(&self) -> String {
        if self.plan.is_empty() {
            return self.provider.into();
        }
        let mut p = self.plan.clone();
        if let Some(f) = p.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        format!("{} {p}", self.provider)
    }
}

pub type Shared = Arc<Mutex<UsageInfo>>;

/// One AI coding assistant. Calls run on the tool's background thread.
pub trait Provider: Send {
    fn name(&self) -> &'static str;
    /// Turned on in Settings. Off: no CLI calls and no log reading.
    fn enabled(&self) -> bool {
        true
    }
    /// Plan, sign-in and limit windows, written into `s`. May run a CLI, so
    /// it locks `s` only to write the result.
    fn fetch_account(&mut self, s: &Shared);
    /// Token counts from local session logs.
    fn local_tokens(&mut self) -> LocalTokens;
    /// Every log there is, older than `LOG_DAYS` too, read once to fill the
    /// saved history (`History::backfilled`). None: nothing older to read.
    fn old_tokens(&mut self) -> Option<LocalTokens> {
        None
    }
    /// The folder where the assistant keeps its data; settings use it to
    /// tell whether the assistant is on this Mac.
    fn data_dir(&self) -> Option<PathBuf> {
        None
    }
    /// Sample data for screenshots and demos.
    fn demo(&self) -> UsageInfo;
}

// ---------------------------------------------------------------- providers

/// What the UI says about a provider when it's missing or signed out.
pub struct Meta {
    pub id: &'static str,
    pub name: &'static str,
    /// The command people type, e.g. "claude".
    pub cli: &'static str,
    pub install_url: &'static str,
    /// Reports plan limits (otherwise tokens only).
    pub limits: bool,
    /// Has a status page with a Statuspage-style `/api/v2/status.json`.
    pub status_url: &'static str,
}

impl Meta {
    /// How to install the CLI and sign in, then come back.
    pub fn install_steps(&self) -> [&'static str; 3] {
        let [a, b] = match self.id {
            "claude" => [ts!("ai.steps.claude.install"), ts!("ai.steps.claude.first_run")],
            "codex" => [ts!("ai.steps.codex.install"), ts!("ai.steps.codex.first_run")],
            "gemini" => [ts!("ai.steps.gemini.install"), ts!("ai.steps.gemini.first_run")],
            _ => [ts!("ai.steps.opencode.install"), ts!("ai.steps.opencode.first_run")],
        };
        [a, b, ts!("ai.steps.come_back")]
    }

    /// How to sign in to an installed CLI, then come back.
    pub fn sign_in_steps(&self) -> [&'static str; 3] {
        let [a, b] = match self.id {
            "claude" => [ts!("ai.steps.claude.open"), ts!("ai.steps.claude.sign_in")],
            "codex" => [ts!("ai.steps.codex.open"), ts!("ai.steps.codex.sign_in")],
            "gemini" => [ts!("ai.steps.gemini.open"), ts!("ai.steps.gemini.sign_in")],
            _ => [ts!("ai.steps.opencode.open"), ts!("ai.steps.opencode.sign_in")],
        };
        [a, b, ts!("ai.steps.come_back")]
    }
}

pub const PROVIDERS: &[Meta] = &[
    Meta {
        id: "claude",
        name: claude::NAME,
        cli: "claude",
        install_url: "https://claude.com/claude-code",
        limits: true,
        status_url: "https://status.anthropic.com/api/v2/status.json",
    },
    Meta {
        id: "codex",
        name: codex::NAME,
        cli: "codex",
        install_url: "https://developers.openai.com/codex/cli",
        limits: true,
        status_url: "https://status.openai.com/api/v2/status.json",
    },
    Meta {
        id: "gemini",
        name: gemini::NAME,
        cli: "gemini",
        install_url: "https://github.com/google-gemini/gemini-cli",
        limits: false,
        status_url: "",
    },
    Meta {
        id: "opencode",
        name: opencode::NAME,
        cli: "opencode",
        install_url: "https://opencode.ai",
        limits: false,
        status_url: "",
    },
];

/// The provider the person picked, as an index into `PROVIDERS`.
static SELECTED: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Index into `PROVIDERS` of the main provider (Claude until one is picked).
pub fn selected() -> usize {
    let i = SELECTED.load(Ordering::Relaxed);
    if i != usize::MAX {
        return i;
    }
    let i = index_of(&settings::get().provider);
    SELECTED.store(i, Ordering::Relaxed);
    i
}

pub fn index_of(id: &str) -> usize {
    if id == "all" {
        return ALL;
    }
    PROVIDERS.iter().position(|m| m.id == id).unwrap_or(0)
}

/// The "provider" index that shows every provider at once.
pub const ALL: usize = PROVIDERS.len();

/// Every provider is shown at once.
pub fn shows_all() -> bool {
    selected() == ALL
}

pub fn selected_meta() -> &'static Meta {
    PROVIDERS.get(selected()).unwrap_or(&PROVIDERS[0])
}

/// The meta for a provider by display name (Claude when unknown).
pub fn meta(name: &str) -> &'static Meta {
    PROVIDERS.iter().find(|m| m.name == name).unwrap_or(&PROVIDERS[0])
}

/// Make `i` the main provider and save it (unless `save` is false, for
/// demos). Nothing is checked again: the loop keeps every provider's data.
pub fn select(i: usize, save: bool) {
    let i = i.min(ALL);
    if SELECTED.swap(i, Ordering::Relaxed) == i || !save {
        return;
    }
    settings::update(true, |s| s.provider = PROVIDERS.get(i).map(|m| m.id).unwrap_or("all").into());
}

/// A fresh provider for index `i`.
pub fn make(i: usize) -> Box<dyn Provider> {
    match PROVIDERS.get(i).map(|m| m.id) {
        Some("codex") => Box::new(codex::Codex::default()),
        Some("gemini") => Box::new(gemini::Gemini::default()),
        Some("opencode") => Box::new(opencode::OpenCode::default()),
        _ => Box::new(claude::ClaudeCode::default()),
    }
}

// ---------------------------------------------------------------- session logs

/// One reply in a session log: its tokens by kind, the model that wrote it
/// and the project it was for (both as `intern`ed names).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct UsageEntry {
    pub ts: i64,
    /// Fresh input (not read from the cache).
    pub input: u64,
    pub output: u64,
    /// Written to the 5-minute prompt cache.
    pub cache_write: u64,
    /// Written to the 1-hour prompt cache.
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub model: u32,
    pub project: u32,
}

impl UsageEntry {
    /// Tokens that count toward limits: everything but cache reads.
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_write_1h
    }
    /// What this reply costs at API prices, if the model is known.
    pub fn usd(&self) -> Option<f64> {
        let p = price::of(&name(self.model))?;
        Some(price::cost(&p, self.input, self.output, self.cache_write, self.cache_write_1h, self.cache_read))
    }
}

static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A small id for a model or project name, so entries stay `Copy`. 0 is "".
pub fn intern(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let mut v = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = v.iter().position(|x| x == s) {
        return i as u32 + 1;
    }
    v.push(s.to_string());
    v.len() as u32
}

/// The name behind an `intern` id.
pub fn name(id: u32) -> String {
    if id == 0 {
        return String::new();
    }
    NAMES.lock().unwrap_or_else(|e| e.into_inner()).get(id as usize - 1).cloned().unwrap_or_default()
}

/// What a parser remembers while reading one log: message ids seen (so a
/// reply logged over several lines counts once), and the model and project
/// named by earlier lines.
#[derive(Default)]
pub struct ParseCtx {
    pub ids: HashSet<Vec<u8>>,
    /// Message id → where its entry is in the log's list, and its size.
    pub seen: HashMap<Vec<u8>, (usize, u64)>,
    /// Where the next new entry goes in the log's list.
    pub at: usize,
    /// Set by a parser: its entry replaces this earlier one (the same
    /// message logged again, further along).
    pub amend: Option<usize>,
    pub model: u32,
    pub project: u32,
}

pub type Parser = fn(&[u8], &mut ParseCtx) -> Option<UsageEntry>;

/// What one append-only session log held so far. Logs only grow, so a
/// longer log is read on from where the last read stopped.
#[derive(Default)]
pub struct LogTail {
    /// Modification time at the last read.
    pub mtime: Option<SystemTime>,
    /// Bytes read so far (whole lines only).
    pub read: u64,
    pub entries: Vec<UsageEntry>,
    pub ctx: ParseCtx,
}

/// Parsed logs by path.
#[derive(Default)]
pub struct LogCache(HashMap<PathBuf, LogTail>);

/// How far back local logs are read: a month, for costs and the month's spend.
pub const LOG_DAYS: i64 = 32;

impl LogCache {
    /// Bring the cache up to date with `files` (logs touched in the last
    /// `LOG_DAYS`), reading each line of new bytes with `parse`, and drop
    /// logs that are gone. Then sum the entries.
    pub fn update(&mut self, files: Vec<PathBuf>, parse: Parser) -> LocalTokens {
        let mut seen = HashSet::new();
        for p in files {
            let Ok(md) = std::fs::metadata(&p) else { continue };
            let Ok(mt) = md.modified() else { continue };
            seen.insert(p.clone());
            let log = self.0.entry(p.clone()).or_default();
            if log.mtime != Some(mt) || log.read != md.len() {
                read_on(&p, md.len(), log, parse);
                log.mtime = Some(mt);
            }
        }
        self.0.retain(|k, _| seen.contains(k));
        sum(self.entries())
    }
    /// Every entry once: a resumed or forked session starts its log with a
    /// copy of the old one, so the same reply in two files counts once.
    pub fn entries(&self) -> Vec<UsageEntry> {
        let mut once = HashSet::new();
        self.0.values().flat_map(|l| l.entries.iter().copied()).filter(|e| once.insert((e.ts, e.input, e.output, e.cache_write, e.cache_write_1h, e.cache_read))).collect()
    }
}

/// Logs that are rewritten whole (JSON files), parsed again when they
/// change.
#[derive(Default)]
pub struct FileCache(HashMap<PathBuf, (Option<SystemTime>, u64, Vec<UsageEntry>)>);

impl FileCache {
    /// Bring the cache up to date with `files`, parsing changed ones with
    /// `parse`, and return every entry.
    pub fn update(&mut self, files: Vec<PathBuf>, parse: fn(&Path, &[u8]) -> Vec<UsageEntry>) -> Vec<UsageEntry> {
        let mut seen = HashSet::new();
        for p in files {
            let Ok(md) = std::fs::metadata(&p) else { continue };
            let mt = md.modified().ok();
            seen.insert(p.clone());
            let e = self.0.entry(p.clone()).or_default();
            if e.0 != mt || e.1 != md.len() || mt.is_none() {
                let bytes = std::fs::read(&p).unwrap_or_default();
                *e = (mt, md.len(), parse(&p, &bytes));
            }
        }
        self.0.retain(|k, _| seen.contains(k));
        self.0.values().flat_map(|v| v.2.iter().copied()).collect()
    }
}

/// `.jsonl` files under `dir` (down to `depth` folder levels) changed in the last `LOG_DAYS`.
pub fn recent_jsonl(dir: &Path, depth: usize) -> Vec<PathBuf> {
    recent_files(dir, depth, "jsonl")
}

/// Every `.jsonl` file under `dir` (down to `depth` folder levels), however old.
pub fn all_jsonl(dir: &Path, depth: usize) -> Vec<PathBuf> {
    files_since(dir, depth, "jsonl", 0)
}

/// Files with extension `ext` under `dir` (down to `depth` levels) changed in the last `LOG_DAYS`.
pub fn recent_files(dir: &Path, depth: usize, ext: &str) -> Vec<PathBuf> {
    files_since(dir, depth, ext, Utc::now().timestamp() - LOG_DAYS * 86_400)
}

/// Files with extension `ext` under `dir` (down to `depth` levels) changed at `since` (unix time) or later.
fn files_since(dir: &Path, depth: usize, ext: &str, since: i64) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(d: &Path, depth: usize, since: i64, ext: &str, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, since, ext, out);
                }
                continue;
            }
            if p.extension().map(|x| x != ext).unwrap_or(true) {
                continue;
            }
            let mts = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            if mts >= since {
                out.push(p);
            }
        }
    }
    walk(dir, depth, since, ext, &mut out);
    out
}

/// Read on in a log of `len` bytes from where `log` stopped, adding an
/// entry for each line `parse` accepts. A log that got shorter is read
/// again from the start.
pub fn read_on(p: &Path, len: u64, log: &mut LogTail, parse: Parser) {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    if len < log.read {
        *log = LogTail::default();
    }
    let Ok(mut f) = std::fs::File::open(p) else { return };
    if f.seek(SeekFrom::Start(log.read)).is_err() {
        return;
    }
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut line = Vec::new();
    loop {
        line.clear();
        match r.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // a line still being written is read next time, whole
        if line.last() != Some(&b'\n') {
            break;
        }
        log.read += line.len() as u64;
        log.ctx.at = log.entries.len();
        if let Some(e) = parse(&line, &mut log.ctx) {
            match log.ctx.amend.take() {
                Some(i) if i < log.entries.len() => log.entries[i] = e,
                _ => log.entries.push(e),
            }
        }
    }
}

/// One local day.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub tokens: u64,
    /// At API prices, for the replies whose model has a known price.
    pub usd: f64,
    pub requests: u64,
}

/// Tokens and dollars of one model or one project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Share {
    pub name: String,
    pub tokens: u64,
    pub usd: f64,
}

/// Tokens counted from local session logs.
#[derive(Clone, Debug, Default)]
pub struct LocalTokens {
    pub tokens_5h: u64,
    pub tokens_today: u64,
    pub tokens_7d: u64,
    pub output_7d: u64,
    pub requests_today: u64,
    pub daily: Vec<u64>, // 7 buckets, oldest first
    /// The last `DAYS` days, oldest first; the last is today.
    pub days: Vec<Day>,
    pub usd_today: f64,
    pub usd_7d: f64,
    pub usd_30d: f64,
    /// This calendar month so far.
    pub usd_month: f64,
    /// Tokens of the last 30 days from models with no known price (their
    /// cost is missing from the dollar totals).
    pub unpriced_30d: u64,
    /// The last 30 days by model and by project, most expensive first.
    pub models: Vec<Share>,
    pub projects: Vec<Share>,
    /// Every day with usage, oldest first: what the logs still have, filled
    /// in from the app's own history (`history::merge_days`) for the days
    /// they no longer cover. Model and project names are kept as logged.
    pub archive: Vec<DayRecord>,
    /// The last 30 days by kind of token (Totals, Cost by type).
    pub kinds_30d: Kinds,
}

/// Tokens and their cost at API prices by kind. Cache writes hold both the
/// 5-minute and the 1-hour cache.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kinds {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub usd_input: f64,
    pub usd_output: f64,
    pub usd_cache_write: f64,
    pub usd_cache_read: f64,
    /// What the cache reads saved: their cost as fresh input less what
    /// they cost.
    pub savings: f64,
    pub requests: u64,
}

impl Kinds {
    /// Tokens that count toward limits (all but cache reads), as
    /// [`UsageEntry::tokens`].
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_write
    }
    fn add(&mut self, e: &UsageEntry) {
        self.requests += 1;
        self.input += e.input;
        self.output += e.output;
        self.cache_write += e.cache_write + e.cache_write_1h;
        self.cache_read += e.cache_read;
        if let Some(p) = price::of(&name(e.model)) {
            self.usd_input += e.input as f64 * p.input / 1e6;
            self.usd_output += e.output as f64 * p.output / 1e6;
            self.usd_cache_write += (e.cache_write as f64 * p.cache_write + e.cache_write_1h as f64 * p.cache_write_1h) / 1e6;
            self.usd_cache_read += e.cache_read as f64 * p.cache_read / 1e6;
            self.savings += e.cache_read as f64 * (p.input - p.cache_read).max(0.0) / 1e6;
        }
    }
    /// Another provider's kinds added in.
    pub fn plus(mut self, o: &Kinds) -> Kinds {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
        self.usd_input += o.usd_input;
        self.usd_output += o.usd_output;
        self.usd_cache_write += o.usd_cache_write;
        self.usd_cache_read += o.usd_cache_read;
        self.savings += o.savings;
        self.requests += o.requests;
        self
    }
}

/// One day with usage and what it went to. `models` and `projects` hold the
/// names as logged (a model id, a folder path), most expensive first;
/// [`model_label`] and [`project_label`] name them for the screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayRecord {
    pub date: NaiveDate,
    pub tokens: u64,
    pub usd: f64,
    pub requests: u64,
    pub models: Vec<Share>,
    pub projects: Vec<Share>,
}

/// A model as a list shows it: "Opus 4.5" for "claude-opus-4-5-20251101".
pub fn model_label(raw: &str) -> String {
    if raw.is_empty() { ts!("ai.other_model").into() } else { price::display_name(raw) }
}

/// A project as a list keeps it: the logged path, or a word for none / home.
pub fn project_label(raw: &str) -> String {
    match raw {
        "" => ts!("ai.unknown_folder").into(),
        "~" => ts!("ai.home_folder").into(),
        _ => raw.to_string(),
    }
}

/// Models or projects of `days` added up under their labels, most expensive
/// first; `label` is [`model_label`] or [`project_label`].
pub fn shares_of<'a>(days: impl IntoIterator<Item = &'a DayRecord>, models: bool) -> Vec<Share> {
    let label: fn(&str) -> String = if models { model_label } else { project_label };
    let mut by: HashMap<String, Share> = HashMap::new();
    for d in days {
        for s in if models { &d.models } else { &d.projects } {
            let name = label(&s.name);
            let e = by.entry(name.clone()).or_insert_with(|| Share { name, ..Default::default() });
            e.tokens += s.tokens;
            e.usd += s.usd;
        }
    }
    let mut v: Vec<Share> = by.into_values().filter(|s| s.tokens > 0).collect();
    v.sort_by(|a, b| b.usd.total_cmp(&a.usd).then(b.tokens.cmp(&a.tokens)).then(a.name.cmp(&b.name)));
    v
}

/// Days kept in `LocalTokens::days`: enough for the 30-day totals and a
/// whole calendar month.
pub const DAYS: usize = 31;

impl LocalTokens {
    /// This calendar month's days, from the 1st to today.
    pub fn month(&self) -> &[Day] {
        let Some(last) = self.days.last() else { return &[] };
        let start = self.days.iter().position(|d| d.date.month() == last.date.month() && d.date.year() == last.date.year()).unwrap_or(0);
        &self.days[start..]
    }
}

/// Sum entries into 5-hour, today, 7-day, 30-day and per-day totals, and
/// by model and project.
pub fn sum(entries: impl IntoIterator<Item = UsageEntry>) -> LocalTokens {
    sum_at(entries, Local::now())
}

pub fn sum_at(entries: impl IntoIterator<Item = UsageEntry>, now: DateTime<Local>) -> LocalTokens {
    let today = now.date_naive();
    let midnight = |d: NaiveDate| d.and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(Local).earliest()).map(|t| t.timestamp()).unwrap_or(0);
    let today_start = midnight(today);
    let first = today - chrono::Days::new(DAYS as u64 - 1);
    let first_start = midnight(first);
    let now_ts = now.timestamp();
    let mut l = LocalTokens { daily: vec![0; 7], ..Default::default() };
    l.days = (0..DAYS).map(|i| Day { date: first + chrono::Days::new(i as u64), ..Default::default() }).collect();
    let mut models: HashMap<u32, Share> = HashMap::new();
    let mut projects: HashMap<u32, Share> = HashMap::new();
    // every day the logs have, with its models and projects by id
    type ByDay = std::collections::BTreeMap<NaiveDate, (Day, HashMap<u32, Share>, HashMap<u32, Share>)>;
    let mut records: ByDay = Default::default();
    for e in entries {
        if e.ts > now_ts + 3600 {
            continue;
        }
        let tokens = e.tokens();
        let usd = e.usd();
        // the local day it fell on
        let date = Local.timestamp_opt(e.ts, 0).single().map(|t| t.date_naive()).unwrap_or(today);
        let rec = records.entry(date).or_insert_with(|| (Day { date, ..Default::default() }, HashMap::new(), HashMap::new()));
        rec.0.tokens += tokens;
        rec.0.requests += 1;
        rec.0.usd += usd.unwrap_or(0.0);
        for (map, key) in [(&mut rec.1, e.model), (&mut rec.2, e.project)] {
            let s = map.entry(key).or_default();
            s.tokens += tokens;
            s.usd += usd.unwrap_or(0.0);
        }
        if e.ts < first_start {
            continue;
        }
        let idx = (date - first).num_days().clamp(0, DAYS as i64 - 1) as usize;
        let d = &mut l.days[idx];
        d.tokens += tokens;
        d.requests += 1;
        d.usd += usd.unwrap_or(0.0);
        let days_ago = DAYS - 1 - idx;
        if days_ago < 7 {
            l.tokens_7d += tokens;
            l.output_7d += e.output;
            l.usd_7d += usd.unwrap_or(0.0);
        }
        if days_ago < 30 {
            l.kinds_30d.add(&e);
            l.usd_30d += usd.unwrap_or(0.0);
            if usd.is_none() {
                l.unpriced_30d += tokens;
            }
            for (map, key) in [(&mut models, e.model), (&mut projects, e.project)] {
                let s = map.entry(key).or_default();
                s.tokens += tokens;
                s.usd += usd.unwrap_or(0.0);
            }
        }
        if e.ts >= now_ts - 5 * 3600 {
            l.tokens_5h += tokens;
        }
        if e.ts >= today_start {
            l.tokens_today += tokens;
            l.requests_today += 1;
            l.usd_today += usd.unwrap_or(0.0);
        }
    }
    l.usd_month = l.days.iter().filter(|d| d.date.month() == today.month() && d.date.year() == today.year()).map(|d| d.usd).sum();
    l.daily = l.days[DAYS - 7..].iter().map(|d| d.tokens).collect();
    let rank = |m: HashMap<u32, Share>, named: fn(&str) -> String| {
        let mut v: Vec<Share> = m.into_iter().filter(|(_, s)| s.tokens > 0).map(|(k, s)| Share { name: named(&name(k)), ..s }).collect();
        v.sort_by(|a, b| b.usd.total_cmp(&a.usd).then(b.tokens.cmp(&a.tokens)));
        v
    };
    l.models = merge_same(rank(models, model_label));
    l.projects = rank(projects, project_label);
    l.archive = records
        .into_values()
        .filter(|(d, _, _)| d.tokens > 0)
        .map(|(d, m, p)| DayRecord { date: d.date, tokens: d.tokens, usd: d.usd, requests: d.requests, models: rank(m, name_of), projects: rank(p, name_of) })
        .collect();
    l
}

fn name_of(raw: &str) -> String {
    raw.to_string()
}

/// A month of made-up replies for screenshots: `per_day` million tokens a
/// day on average, spread over `models` (by weight) and `projects`.
pub fn demo_local(models: &[(&str, u32)], projects: &[&str], per_day: f64) -> LocalTokens {
    let now = Local::now();
    let mut seed: u64 = 0x2545F4914F6CDD1D;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 10_000) as f64 / 10_000.0
    };
    let total_w: u32 = models.iter().map(|m| m.1).sum::<u32>().max(1);
    let mids: Vec<(u32, u32)> = models.iter().map(|(m, w)| (intern(m), *w)).collect();
    let pids: Vec<u32> = projects.iter().map(|p| intern(p)).collect();
    let mut out = Vec::new();
    // over a year, so Year and All have months to show
    for day in 0..420_i64 {
        // quieter weekends, busier lately
        let date = now - chrono::Duration::days(day);
        let weekend = matches!(date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
        let growth = 1.0 - 0.7 * (day as f64 / 420.0);
        let mult = if weekend { 0.35 } else { 1.0 } * (0.55 + rnd() * 0.9) * growth;
        let replies = 40;
        let hours = if day == 0 { (now.timestamp() % 86_400) / 3600 + 1 } else { 12 };
        for k in 0..replies {
            let ts = date.timestamp() - (k as i64 * hours * 3600 / replies as i64);
            let mut pick = (rnd() * total_w as f64) as u32;
            let model = mids.iter().find(|(_, w)| { let hit = pick < *w; pick = pick.saturating_sub(*w); hit }).map(|m| m.0).unwrap_or(0);
            let project = pids.get(((rnd() * rnd()) * pids.len() as f64) as usize).copied().unwrap_or(0);
            let t = per_day * 1e6 * mult / replies as f64;
            out.push(UsageEntry { ts, input: (t * 0.05) as u64, output: (t * 0.08) as u64, cache_write: (t * 0.87) as u64, cache_write_1h: 0, cache_read: (t * 9.0) as u64, model, project });
        }
    }
    sum_at(out, now)
}

/// Shares that ended up with the same display name, added together.
fn merge_same(v: Vec<Share>) -> Vec<Share> {
    let mut out: Vec<Share> = Vec::new();
    for s in v {
        match out.iter_mut().find(|o| o.name == s.name) {
            Some(o) => {
                o.tokens += s.tokens;
                o.usd += s.usd;
            }
            None => out.push(s),
        }
    }
    out
}

// ---------------------------------------------------------------- formatting

/// Pretty label for a limit row.
/// `l` is the limit's fixed English name (also its key in history and
/// alerts); only what this returns is shown.
pub fn short_label(l: &str) -> String {
    let low = l.to_lowercase();
    if low.contains("session") {
        ts!("ai.lim.session").into()
    } else if low.contains("all models") {
        ts!("ai.lim.week").into()
    } else if let (Some(a), Some(b)) = (l.find('('), l.rfind(')')) {
        t!("ai.lim.week_of", model = &l[a + 1..b]).into_owned()
    } else if l == "Current month" {
        ts!("ai.lim.month").into()
    } else if l == "Limit" {
        ts!("ai.lim.any").into()
    } else if let Some(h) = l.strip_prefix("Last ").and_then(|r| r.strip_suffix('h')).and_then(|n| n.parse::<i64>().ok()) {
        t!("ai.lim.last_h", h = h).into_owned()
    } else {
        l.to_string()
    }
}

pub fn human_tokens(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// Where `needle` first is in `hay`. A month of session logs is gigabytes
/// and every line is searched several times, so this is the vectorised
/// search (about 17 times the speed of comparing at every byte).
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::find(hay, needle)
}

/// The JSON object at the start of `s`, up to its closing brace.
pub fn balanced(s: &[u8]) -> &[u8] {
    let mut depth = 0i32;
    for (i, &b) in s.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &s[..=i];
                }
            }
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_until_the_first_check() {
        let mut i = UsageInfo { loading: true, ..UsageInfo::new(codex::NAME) };
        assert_eq!(i.unavailable(), None);
        i.scanned = true;
        assert_eq!(i.unavailable(), Some(Availability::NotInstalled));
        i.installed = true;
        assert_eq!(i.unavailable(), None);
    }

    #[test]
    fn kinds_split_tokens_and_dollars() {
        let e = UsageEntry { ts: 0, input: 1_000_000, output: 1_000_000, cache_write: 1_000_000, cache_write_1h: 0, cache_read: 1_000_000, model: intern("claude-sonnet-4-5"), project: 0 };
        let mut k = Kinds::default();
        k.add(&e);
        let p = price::of("claude-sonnet-4-5").unwrap();
        assert_eq!((k.input, k.output, k.cache_write, k.cache_read, k.requests), (1_000_000, 1_000_000, 1_000_000, 1_000_000, 1));
        assert_eq!(k.tokens(), e.tokens());
        let total = k.usd_input + k.usd_output + k.usd_cache_write + k.usd_cache_read;
        assert!((total - e.usd().unwrap()).abs() < 1e-9);
        assert!((k.savings - (p.input - p.cache_read)).abs() < 1e-9);
    }

    #[test]
    fn plan_titles() {
        let mut i = UsageInfo::new("Claude");
        assert_eq!(i.plan_title(), "Claude");
        i.plan = "max".into();
        assert_eq!(i.plan_title(), "Claude Max");
    }

    #[test]
    fn providers_by_id_and_name() {
        assert_eq!(PROVIDERS[index_of("codex")].name, "Codex");
        assert_eq!(index_of("nope"), 0);
        assert_eq!(meta("Codex").cli, "codex");
        assert_eq!(meta("").id, "claude");
        for (i, m) in PROVIDERS.iter().enumerate() {
            assert_eq!(make(i).name(), m.name);
        }
    }

    #[test]
    fn limit_labels() {
        assert_eq!(short_label("Current session"), "Session");
        assert_eq!(short_label("Current week (all models)"), "Week");
        assert_eq!(short_label("Current week (Opus)"), "Week · Opus");
        assert_eq!(short_label("Something else"), "Something else");
    }

    #[test]
    fn token_formatting() {
        assert_eq!(human_tokens(999), "999");
        assert_eq!(human_tokens(12_345), "12K");
        assert_eq!(human_tokens(3_400_000), "3.4M");
        assert_eq!(human_tokens(2_000_000_000), "2.0B");
    }

    #[test]
    fn balanced_object() {
        assert_eq!(balanced(b"{\"a\":{\"b\":1}},\"x\":2}"), b"{\"a\":{\"b\":1}}");
        assert_eq!(balanced(b"{open"), b"{open");
    }

    #[test]
    fn sums_by_window() {
        let now = Utc::now().timestamp();
        let e = |ts: i64, input: u64, output: u64| UsageEntry { ts, input, output, ..Default::default() };
        let l = sum([e(now - 60, 9, 1), e(now - 8 * 86_400, 90, 9)]);
        assert_eq!(l.tokens_5h, 10);
        assert_eq!(l.tokens_7d, 10);
        assert_eq!(l.output_7d, 1);
        assert_eq!(l.daily[6], l.tokens_today);
    }
}
