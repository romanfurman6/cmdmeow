//! Cleanup: find junk by the original's rules (System Junk, Xcode Junk, app
//! caches, AI Clutter), then let Claude judge each PATH (never contents) as
//! safe / ask / keep. Anything not clearly safe waits for the person's OK.

use crate::kit::sync::LockExt;
use crate::kit::claude_code;
use crate::kit::fs::{home, is_protected, move_to_trash};
use crate::kit::mechanics as m;
use crate::kit::fs::dir_size;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    AiTools,
    AppCaches,
    Developer,
    Projects,
    Installers,
    Captures,
    Logs,
    Trash,
}

impl Group {
    pub const COUNT: usize = 8;
    pub const ALL: [Group; Group::COUNT] = [Group::AiTools, Group::AppCaches, Group::Developer, Group::Projects, Group::Installers, Group::Captures, Group::Logs, Group::Trash];
    pub fn title(self) -> &'static str {
        match self {
            Group::AiTools => "AI Tools",
            Group::AppCaches => "App Caches",
            Group::Developer => "Developer",
            Group::Projects => "Projects",
            Group::Installers => "Installers",
            Group::Captures => "Captures",
            Group::Logs => "Logs",
            Group::Trash => "Trash",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Group::AiTools => "Caches, logs and old versions of Claude, Codex, Gemini, Cursor",
            Group::AppCaches => "Files apps rebuild when needed",
            Group::Developer => "Xcode build data, simulator caches, package downloads",
            Group::Projects => "node_modules, target and other build output of idle projects",
            Group::Installers => "Old installers in Downloads",
            Group::Captures => "Screenshots, recordings and uploads older than 30 days",
            Group::Logs => "Logs from apps and macOS",
            Group::Trash => "Items in the Trash",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Safe,
    Ask,
    Keep,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Safe => "Safe",
            Verdict::Ask => "Needs your OK",
            Verdict::Keep => "Kept",
        }
    }
    fn rank(self) -> u8 {
        match self {
            Verdict::Safe => 0,
            Verdict::Ask => 1,
            Verdict::Keep => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Rule,
    Ai,
    You,
}

/// How an item is removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Its file or folder (to the Trash or deleted).
    Files,
    /// `simctl delete` a simulator whose runtime is gone.
    SimDevice(String),
    /// `simctl runtime delete` an old simulator runtime.
    SimRuntime(String),
    /// `brew cleanup`: old versions and downloads.
    BrewCleanup,
    /// An old upload: deleted from the person's storage, then forgotten
    /// here (`local` is the file that was shared).
    Upload { key: String, local: PathBuf },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub group: Group,
    pub title: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub age_days: f64,
    pub kind: &'static str, // Caches, Logs, Temp Files, Stale Versions, Build Data...
    pub verdict: Verdict,
    pub reason: String,
    pub rule_reason: String,
    /// What the built-in rules said, before Claude or the person.
    pub rule_verdict: Verdict,
    pub by: Who,
    pub selected: bool,
    pub hard_keep: bool,
    pub permanent: bool, // emptying the Trash can't be undone
    pub app_running: bool,
    pub action: Action,
}

impl Item {
    /// Rebuilt by its app or tool when needed, and nothing in it is the
    /// person's: deleted outright even in "Move to Trash" mode, because in
    /// the Trash it would still take the space this clean promised to free.
    /// Anything the rules or Claude weren't sure of goes to the Trash.
    pub fn rebuilds(&self) -> bool {
        self.verdict == Verdict::Safe && self.rule_verdict == Verdict::Safe && self.by != Who::You && matches!(self.kind, "Caches" | "Logs" | "Temp Files" | "Build Data")
    }
    /// Removed by a tool, not by moving files: can't go to the Trash.
    pub fn by_command(&self) -> bool {
        self.action != Action::Files
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Intro,
    Scanning,
    Judging,
    Results,
    Cleaning,
    Done,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub phase: Phase,
    pub current: String,
    pub progress: f32,
    pub items: Vec<Item>,
    pub ai_status: String,
    /// This scan asks Claude after looking (step 2 of 2).
    pub with_ai: bool,
    pub ai_used: bool,
    pub ai_error: Option<String>,
    pub cleaned_bytes: u64,
    /// The part of `cleaned_bytes` that went to the Trash (space comes back
    /// once it's emptied).
    pub trashed_bytes: u64,
    pub cleaned_count: usize,
    /// While cleaning: bytes of everything planned, and of what was gone
    /// through so far (removed or skipped), for the loader's "x of y".
    pub clean_total: u64,
    pub clean_done: u64,
    /// The clean was stopped before the end.
    pub stopped: bool,
    /// Group and size of each item removed (they leave `items`).
    pub cleaned: Vec<(Group, u64)>,
    pub failed: Vec<(String, String)>,
    pub log: Vec<String>,
    /// Set to stop this run (Stop, Skip, a new scan); each run has its own.
    pub cancel: Arc<AtomicBool>,
    /// Which scan this is (counts up per run): AI Check checks once per scan.
    pub run: u64,
    /// The agent checking this scan's paths, if any.
    pub agent: Option<super::agent::Agent>,
    /// Found, but hidden: the person chose to keep them before.
    pub hidden_kept: usize,
    /// While scanning: per group (in [`Group::ALL`] order) what has been
    /// measured so far, so the totals grow live.
    pub live: [GroupLive; Group::COUNT],
    /// While scanning: every place to look at has been listed (only measuring is left).
    pub listed: bool,
    /// When this scan started (the bar creeps from it until real progress comes).
    pub started: Option<std::time::Instant>,
}

/// One group's live progress while scanning.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupLive {
    /// Bytes found so far (items big enough to show, not kept).
    pub bytes: u64,
    /// Items found so far.
    pub items: usize,
    /// Places to measure, and how many are measured.
    pub total: usize,
    pub done: usize,
}

/// Finding what to look at is quick; measuring it is the long part.
const FIND: f32 = 0.1;

/// Where the scan bar creeps to while the scan is still listing places
/// (Homebrew, simulators): it never sits still, and real progress takes over
/// from it without going back.
const CREEP_TO: f32 = 0.06;
/// Seconds for the creep to cover ~63% of the way (5.5% after 10 s).
const CREEP_TAU: f32 = 4.0;

/// The creep `secs` into a scan: eases out towards [`CREEP_TO`], in steps
/// of 0.1% (the bar's layer eases between steps by itself, so a new value
/// every pass isn't needed).
fn creep(secs: f32) -> f32 {
    let c = CREEP_TO * (1.0 - (-secs.max(0.0) / CREEP_TAU).exp());
    (c * 1000.0).floor() / 1000.0
}

impl State {
    /// The progress to show: while scanning, never below the creep. Both only
    /// grow, so the bar never steps back when the real count passes it.
    pub fn shown_progress(&self) -> f32 {
        match (&self.phase, self.started) {
            (Phase::Scanning, Some(t0)) if self.progress >= 0.0 => self.progress.max(creep(t0.elapsed().as_secs_f32())),
            _ => self.progress,
        }
    }
    pub fn total(&self) -> u64 {
        self.items.iter().filter(|i| i.verdict != Verdict::Keep).map(|i| i.bytes).sum()
    }
    pub fn selected_bytes(&self) -> u64 {
        self.items.iter().filter(|i| i.selected).map(|i| i.bytes).sum()
    }
    pub fn pending_questions(&self) -> usize {
        self.items.iter().filter(|i| i.verdict == Verdict::Ask && i.by != Who::You).count()
    }
    pub fn group_bytes(&self, g: Group) -> u64 {
        self.items.iter().filter(|i| i.group == g && i.verdict != Verdict::Keep).map(|i| i.bytes).sum()
    }
    /// Claude junk safe to clean (for the panel's recommendation).
    pub fn claude_safe_bytes(&self) -> u64 {
        self.items.iter().filter(|i| i.group == Group::AiTools && i.verdict == Verdict::Safe).map(|i| i.bytes).sum()
    }
}

pub type Shared = Arc<Mutex<State>>;

// ---------------------------------------------------------------- scanning

type ScanStep = (&'static str, fn(&mut Ctx));

pub(super) struct Ctx {
    pub(super) items: Vec<Item>,
    /// Paths the person chose to keep in an earlier scan: not offered again.
    kept: HashSet<PathBuf>,
    hidden_kept: usize,
    pub(super) running_ids: HashSet<String>,
    running_names: HashSet<String>,
    /// Homebrew's own cleanup was offered: its download cache isn't offered again.
    brew: bool,
    shared: Shared,
    egui: egui::Context,
    cancel: Arc<AtomicBool>,
}

impl Ctx {
    pub(super) fn stopped(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    fn status(&self, s: &str, progress: f32) {
        let mut g = self.shared.lock_ok();
        g.current = s.to_string();
        g.progress = progress;
        self.egui.request_repaint();
    }
    /// Offer an item removed by a tool (simctl, brew): its size comes from the
    /// tool and it lives outside the home folder.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn add_action(&mut self, group: Group, title: &str, path: PathBuf, kind: &'static str, verdict: Verdict, reason: &str, action: Action, bytes: u64) {
        if self.kept.contains(&path) {
            self.hidden_kept += 1;
            return;
        }
        self.items.push(Item {
            group,
            title: title.to_string(),
            path,
            bytes,
            age_days: 0.0,
            kind,
            verdict,
            reason: reason.to_string(),
            rule_reason: reason.to_string(),
            rule_verdict: verdict,
            by: Who::Rule,
            selected: false,
            hard_keep: false,
            permanent: true,
            app_running: false,
            action,
        });
    }

    pub(super) fn add(&mut self, group: Group, title: &str, path: PathBuf, kind: &'static str, verdict: Verdict, reason: &str) {
        // The app's own cache and logs (also under its first name, CleanMeBro): a few MB, and in use while it runs
        if path.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_lowercase();
            n == "cmdmeow" || n == "cleanmebro" || n.starts_with("com.romanfurman6.cmdmeow") || n.starts_with("com.romanfurman6.cleanmebro")
        }) {
            return;
        }
        if self.kept.iter().any(|k| path.starts_with(k)) {
            self.hidden_kept += 1;
            return;
        }
        // missing, or a link: removing a link frees nothing
        if !path.symlink_metadata().is_ok_and(|m| !m.file_type().is_symlink()) {
            return;
        }
        // measured later, all at once and in parallel, once nested and
        // repeated paths are dropped (see `measure`)
        let age_days = age_days(&path);
        let mut it = Item {
            group,
            title: title.to_string(),
            path: path.clone(),
            bytes: 0,
            age_days,
            kind,
            verdict,
            reason: reason.to_string(),
            rule_reason: reason.to_string(),
            rule_verdict: verdict,
            by: Who::Rule,
            selected: false,
            hard_keep: false,
            permanent: false,
            app_running: false,
            action: Action::Files,
        };
        if let Some(why) = guard(group, &path) {
            it.verdict = Verdict::Keep;
            it.reason = why.into();
            it.hard_keep = true;
        }
        if verdict == Verdict::Safe && kind == "Caches" && app_open(&self.running_ids, &self.running_names, &path, title) {
            let name = path.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            it.verdict = Verdict::Ask;
            it.rule_verdict = Verdict::Ask;
            it.app_running = true;
            let app = match title_app(title) {
                a if a != title => a.to_string(),
                _ if title.contains(' ') && title.len() > 18 => pretty(&name),
                _ => title.to_string(),
            };
            it.reason = format!("{} is open right now — quit it first or allow anyway", if app.is_empty() { title.to_string() } else { app });
        }
        self.items.push(it);
    }
}

/// Why `p` must never be removed by `group`'s rules, if so. Everything
/// Cleanup offers lives in the home folder; build output of code projects is
/// judged by its own rules (it sits in folders that are otherwise personal).
pub(super) fn guard(group: Group, p: &Path) -> Option<&'static str> {
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !p.starts_with(home()) {
        return Some("Outside your home folder");
    }
    if group == Group::Projects {
        return super::projects::still_build_output(p);
    }
    if group == Group::Captures {
        return super::captures::guard(p);
    }
    is_protected(p)
}

/// The app named in an item's title: "Slack · Cache" and "Slack (sandboxed)"
/// are Slack's.
fn title_app(title: &str) -> &str {
    let t = title.split(" · ").next().unwrap_or(title);
    t.strip_suffix(" (sandboxed)").unwrap_or(t)
}

/// The app a cache belongs to is running: by the item's title, or by the
/// app's own folder, which is the cache itself (Caches/<bundle id>) or sits
/// above it (Containers/<bundle id>/Data/Library/Caches, Application
/// Support/<app>/Cache).
fn app_open(ids: &HashSet<String>, names: &HashSet<String>, path: &Path, title: &str) -> bool {
    let low = |s: &std::ffi::OsStr| s.to_string_lossy().to_lowercase();
    let name = path.file_name().map(low).unwrap_or_default();
    if ids.contains(&name) || names.contains(&title.to_lowercase()) || names.contains(&title_app(title).to_lowercase()) {
        return true;
    }
    let parts: Vec<String> = path.components().map(|c| low(c.as_os_str())).collect();
    parts.windows(2).any(|w| match w[0].as_str() {
        // an app's helpers and extensions keep containers of their own
        "containers" => ids.contains(&w[1]) || ids.iter().any(|id| w[1].strip_prefix(id.as_str()).is_some_and(|rest| rest.starts_with('.'))),
        "application support" => names.contains(&w[1]) || ids.contains(&w[1]),
        _ => false,
    })
}

/// Drop repeated and nested paths (the outer one stays), then measure what's
/// left on all cores and drop items too small to matter. Sizing is nearly all
/// of a scan's time, so nothing is measured twice.
/// `brew_pending`: Homebrew's answer is still to come (one more Developer place).
/// Drops repeated and nested items, so nothing is listed or counted twice.
/// The same folder can come under two spellings: "~/Code" and "~/code" are one
/// folder on a case-insensitive disk, so files are also matched by what they are on disk.
fn drop_repeats(items: &mut Vec<Item>) {
    use std::os::unix::fs::MetadataExt;
    let mut seen = HashSet::new();
    items.retain(|it| seen.insert(it.path.clone()));
    let mut ids = HashSet::new();
    items.retain(|it| {
        if it.action != Action::Files {
            return true;
        }
        match it.path.symlink_metadata() {
            Ok(md) => ids.insert((md.dev(), md.ino())),
            Err(_) => true,
        }
    });
    let all: HashSet<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
    items.retain(|it| !it.path.ancestors().skip(1).any(|a| all.contains(a)));
}

fn measure(items: &mut Vec<Item>, shared: &Shared, ctx: &egui::Context, from: f32, cancel: &AtomicBool, brew_pending: bool) {
    drop_repeats(items);
    let n = items.len();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let sizes: Vec<std::sync::atomic::AtomicU64> = (0..n).map(|_| std::sync::atomic::AtomicU64::new(0)).collect();
    let gi = |g: Group| Group::ALL.iter().position(|x| *x == g).unwrap_or(0);
    let weights = group_weights(last_scan().map(|l| l.groups).as_deref());
    {
        let mut g = shared.lock_ok();
        g.live = Default::default();
        for it in items.iter() {
            g.live[gi(it.group)].total += 1;
        }
        if brew_pending {
            g.live[gi(Group::Developer)].total += 1;
        }
        g.listed = true;
    }
    ctx.request_repaint();
    let workers = std::thread::available_parallelism().map(|c| c.get()).unwrap_or(4).saturating_sub(1).clamp(1, 8);
    std::thread::scope(|s| {
        for _ in 0..workers.min(n.max(1)) {
            s.spawn(|| {
                crate::kit::activity::low_priority();
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if k >= n || cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    // a tool's item came with its size
                    let size = if items[k].action == Action::Files { dir_size(&items[k].path) } else { items[k].bytes };
                    sizes[k].store(size, std::sync::atomic::Ordering::Relaxed);
                    let mut g = shared.lock_ok();
                    // a stopped run no longer owns the shared state
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let l = &mut g.live[gi(items[k].group)];
                    l.done += 1;
                    if size >= m::MIN_ITEM_BYTES && items[k].verdict != Verdict::Keep {
                        l.bytes += size;
                        l.items += 1;
                    }
                    g.current = items[k].path.display().to_string();
                    let p = from + (1.0 - from) * live_progress(&g.live, &weights);
                    g.progress = g.progress.max(p);
                    drop(g);
                    ctx.request_repaint();
                }
            });
        }
    });
    for (it, s) in items.iter_mut().zip(sizes.iter()) {
        it.bytes = s.load(std::sync::atomic::Ordering::Relaxed);
    }
    items.retain(|it| it.bytes >= m::MIN_ITEM_BYTES);
}

/// How much each group weighs in the progress: what the last scan found in
/// it (a big group takes longer to measure), at least 1 GB; equal without one.
fn group_weights(last: Option<&[u64]>) -> [f32; Group::COUNT] {
    let mut w = [1.0; Group::COUNT];
    if let Some(l) = last.filter(|l| l.len() == Group::COUNT) {
        for (x, b) in w.iter_mut().zip(l) {
            *x = (*b).max(1_000_000_000) as f32 / 1e9;
        }
    }
    w
}

/// Measuring progress, 0..1, as the group rows show it. A count of places
/// alone runs ahead: the small ones finish in seconds while a few big folders
/// take minutes, so a group counts half until its last place is measured.
fn live_progress(live: &[GroupLive], weights: &[f32; Group::COUNT]) -> f32 {
    let (mut got, mut all) = (0.0, 0.0);
    for (l, w) in live.iter().zip(weights) {
        if l.total == 0 {
            continue;
        }
        all += w;
        got += w * if l.done >= l.total { 1.0 } else { 0.5 * l.done as f32 / l.total as f32 };
    }
    if all > 0.0 {
        got / all
    } else {
        0.0
    }
}

fn age_days(p: &Path) -> f64 {
    p.symlink_metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs_f64() / 86_400.0)
        .unwrap_or(0.0)
}

pub(super) fn children(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

pub(super) fn fname(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Friendly app name from a bundle id or folder name.
pub fn pretty(name: &str) -> String {
    let known = [
        ("com.spotify.client", "Spotify"),
        ("com.google.Chrome", "Google Chrome"),
        ("com.apple.Safari", "Safari"),
        ("com.microsoft.VSCode", "Visual Studio Code"),
        ("com.tinyspeck.slackmacgap", "Slack"),
        ("com.hnc.Discord", "Discord"),
        ("com.apple.dt.Xcode", "Xcode"),
        ("org.mozilla.firefox", "Firefox"),
        ("com.brave.Browser", "Brave"),
        ("us.zoom.xos", "Zoom"),
        ("com.anthropic.claudefordesktop", "Claude"),
    ];
    for (id, n) in known {
        if name.eq_ignore_ascii_case(id) {
            return n.into();
        }
    }
    if name.matches('.').count() >= 2 && !name.contains(' ') {
        let parts: Vec<&str> = name.split('.').collect();
        let generic = ["macos", "mac", "osx", "desktop", "app", "client", "helper", "agent", "main"];
        let mut last = parts[parts.len() - 1];
        if generic.contains(&last.to_lowercase().as_str()) && parts.len() >= 3 {
            last = parts[parts.len() - 2];
        }
        let s = last.replace(['-', '_'], " ");
        let mut chars = s.chars();
        return chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default();
    }
    name.to_string()
}

fn running_apps() -> (HashSet<String>, HashSet<String>) {
    let (ids, names) = crate::kit::native::running_apps();
    let ids: HashSet<String> = ids.into_iter().map(|s| s.to_lowercase()).collect();
    let names: HashSet<String> = names.into_iter().map(|s| s.to_lowercase()).collect();
    (ids, names)
}

const CHROMIUM_CACHES: [&str; 9] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnWebGPUCache",
    "DawnGraphiteCache",
    "GraphiteDawnCache",
    "Service Worker/CacheStorage",
    "Shared Dictionary",
];

/// Scan, then (with `agent`) have that agent check the paths. A run still
/// going is stopped first.
pub fn start_scan(shared: Shared, ctx: egui::Context, agent: Option<super::agent::Agent>) {
    let use_ai = agent.is_some();
    let cancel = begin_run(&shared, agent);
    std::thread::spawn(move || {
        let (running_ids, running_names) = running_apps();
        // `brew cleanup --dry-run` takes ~10 s: it runs alongside the rest
        // and its answer joins once everything else is measured
        let brew_tool = brew_path();
        let brew_job = brew_tool.clone().map(|b| std::thread::spawn(move || brew_dry_run(&b)));
        let mut c = Ctx { items: Vec::new(), kept: kept().into_iter().collect(), hidden_kept: 0, running_ids, running_names, brew: brew_tool.is_some(), shared: shared.clone(), egui: ctx.clone(), cancel: cancel.clone() };
        let steps: [ScanStep; 12] = [
            ("Claude", scan_claude),
            ("AI tools", scan_ai_tools),
            ("User caches", scan_user_caches),
            ("App caches", scan_app_caches),
            ("Logs", scan_logs),
            ("Xcode", super::xcode::scan),
            ("Simulators", super::xcode::scan_simulators),
            ("Developer caches", scan_developer),
            ("Projects", super::projects::scan),
            ("Installers", scan_installers),
            ("Captures", super::captures::scan),
            ("Trash", scan_trash),
        ];
        for (i, (_, f)) in steps.iter().enumerate() {
            if c.stopped() {
                break;
            }
            c.status("", FIND * i as f32 / steps.len() as f32);
            f(&mut c);
        }
        if !c.stopped() {
            measure(&mut c.items, &shared, &ctx, FIND, &cancel, brew_job.is_some());
        }
        if let (Some(job), Some(tool)) = (brew_job, brew_tool) {
            join_brew(&mut c, job, &tool, &shared, &ctx, &cancel);
        }
        c.items.sort_by(|a, b| a.group.cmp(&b.group).then(b.bytes.cmp(&a.bytes)));
        for it in c.items.iter_mut() {
            it.selected = it.verdict == Verdict::Safe && !it.permanent;
        }
        {
            // stopped: Stop already took the page back to the start
            let mut g = shared.lock_ok();
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            g.items = std::mem::take(&mut c.items);
            g.hidden_kept = c.hidden_kept;
            g.phase = if use_ai { Phase::Judging } else { Phase::Results };
            if !use_ai {
                record_scan(&g);
            }
        }
        ctx.request_repaint();
        if let Some(agent) = agent {
            judge(&shared, &ctx, agent, &cancel);
            let mut g = shared.lock_ok();
            // skipped: the results are already showing
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            finish_results(&mut g);
        }
        ctx.request_repaint();
        crate::kit::sound::play(crate::kit::sound::Cue::Ready);
    });
}

/// Stop any run still going and set up a fresh scan; returns its token.
fn begin_run(shared: &Shared, agent: Option<super::agent::Agent>) -> Arc<AtomicBool> {
    let mut g = shared.lock_ok();
    g.cancel.store(true, Ordering::SeqCst);
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, Ordering::SeqCst) + 1;
    *g = State { phase: Phase::Scanning, with_ai: agent.is_some(), agent, run, started: Some(std::time::Instant::now()), ..Default::default() };
    g.cancel.clone()
}

/// Show the results as they stand: select what's safe, remember the scan.
fn finish_results(g: &mut State) {
    for it in g.items.iter_mut() {
        it.selected = it.verdict == Verdict::Safe && !it.permanent;
    }
    g.phase = Phase::Results;
    // tests don't touch the person's saved scan
    if !cfg!(test) {
        record_scan(g);
    }
}

/// Stop a scan: back to the start screen at once (the scan thread notices
/// and quits without touching the page again).
pub fn stop_scan(shared: &Shared) {
    let mut g = shared.lock_ok();
    g.cancel.store(true, Ordering::SeqCst);
    *g = State::default();
}

/// Stop a clean after the item in hand (what's done stays done).
pub fn stop_clean(shared: &Shared) {
    let g = shared.lock_ok();
    if g.phase == Phase::Cleaning {
        g.cancel.store(true, Ordering::SeqCst);
    }
}

/// Stop the AI check: the results show now, judged by the built-in rules
/// (and whatever the agent already answered).
pub fn skip_ai(shared: &Shared) {
    let mut g = shared.lock_ok();
    if g.phase != Phase::Judging {
        return;
    }
    g.cancel.store(true, Ordering::SeqCst);
    let name = g.agent.map(|a| a.name()).unwrap_or("AI");
    if !g.ai_used {
        g.ai_error = Some(format!("{name} check skipped; the built-in rules were used."));
    }
    finish_results(&mut g);
}

// ---- Claude (catalog read from the original's AIToolScanning, plus project rules)

fn scan_claude(c: &mut Ctx) {
    let h = home();
    let cl = h.join(".claude");
    let claude_running = c.running_names.iter().any(|n| n == "claude") || process_running("claude");
    // AIToolScanning sub_7118: Claude Code catalog
    c.add(Group::AiTools, "Claude Code debug logs", cl.join("debug"), "Logs", Verdict::Safe, "Debug logs; Claude Code writes new ones as needed");
    c.add(Group::AiTools, "Claude Code cache", cl.join("cache"), "Caches", Verdict::Safe, "Cache; rebuilt automatically");
    c.add(Group::AiTools, "Claude Code telemetry", cl.join("telemetry"), "Caches", Verdict::Safe, "Usage telemetry waiting to be sent");
    c.add(Group::AiTools, "Stats cache", cl.join("stats-cache.json"), "Caches", Verdict::Safe, "Cached statistics; recalculated");
    c.add(Group::AiTools, "MCP sign-in cache", cl.join("mcp-needs-auth-cache.json"), "Caches", Verdict::Safe, "Cached MCP sign-in state");
    c.add(Group::AiTools, "Plugin install counts", cl.join("plugins/install-counts-cache.json"), "Caches", Verdict::Safe, "Cached plugin popularity counts");
    // shell snapshots: only the old ones (a running session uses the newest)
    let snaps = cl.join("shell-snapshots");
    for f in children(&snaps) {
        let age = age_days(&f);
        if age >= m::SHELL_SNAPSHOT_MIN_AGE_DAYS {
            c.add(Group::AiTools, "Old shell snapshot", f, "Temp Files", Verdict::Safe, "Temporary shell snapshot from a finished session");
        }
    }
    // stale versions: keep the one ~/.local/bin/claude points to (ActiveVersionResolver)
    let versions = h.join(".local/share/claude/versions");
    let active = std::fs::canonicalize(h.join(".local/bin/claude")).ok();
    for v in children(&versions) {
        let is_active = active.as_ref().map(|a| a.starts_with(&v) || a == &v).unwrap_or(false);
        if !is_active {
            c.add(Group::AiTools, &format!("Claude Code {}", fname(&v)), v, "Stale Versions", Verdict::Safe, "Old Claude Code version; the one you run is kept");
        }
    }
    c.add(Group::AiTools, "Claude CLI logs", h.join("Library/Caches/claude-cli-nodejs"), "Logs", Verdict::Safe, "Claude Code's own error/MCP logs");
    // AIToolScanning sub_7308: Claude Desktop
    c.add(Group::AiTools, "Claude Desktop logs", h.join("Library/Logs/Claude"), "Logs", Verdict::Safe, "Claude Desktop logs");
    c.add(Group::AiTools, "Claude Desktop cache", h.join("Library/Caches/com.anthropic.claudefordesktop"), "Caches", Verdict::Safe, "App cache; rebuilt automatically");
    c.add(Group::AiTools, "Claude updater cache", h.join("Library/Caches/com.anthropic.claudefordesktop.ShipIt"), "Caches", Verdict::Safe, "Leftover update download");
    let desk = h.join("Library/Application Support/Claude");
    let desktop_running = c.running_ids.contains("com.anthropic.claudefordesktop");
    for sub in CHROMIUM_CACHES {
        let p = desk.join(sub);
        let (v, r) = if desktop_running { (Verdict::Ask, "Claude Desktop is open — its web cache is in use") } else { (Verdict::Safe, "Web view cache; rebuilt automatically") };
        c.add(Group::AiTools, &format!("Claude Desktop {}", sub), p, "Caches", v, r);
    }
    c.add(Group::AiTools, "Claude Desktop VM image", desk.join("vm_bundles"), "Downloads", Verdict::Ask, "Sandbox VM image for agent mode; downloaded again if removed");
    c.add(Group::AiTools, "Claude Desktop Code VM", desk.join("claude-code-vm"), "Downloads", Verdict::Ask, "Runtime for Claude Code inside the desktop app; re-downloaded if removed");
    // projects: the ones you use are kept, old ones are offered for review
    for p in claude_code::scan_projects() {
        let idle = p.days_idle();
        let name = p.display_name();
        if p.in_use() {
            c.add(Group::AiTools, &format!("Project · {name}"), p.folder.clone(), "Projects", Verdict::Keep, &format!("You used this project {} — kept", ago(idle)));
            if let Some(it) = c.items.last_mut() {
                if it.path == p.folder {
                    it.hard_keep = true;
                }
            }
        } else if !p.cwd_exists {
            c.add(Group::AiTools, &format!("Project · {name}"), p.folder.clone(), "Old Sessions", Verdict::Ask, &format!("Chat history for a folder that no longer exists; last used {}", ago(idle)));
        } else {
            c.add(Group::AiTools, &format!("Project · {name}"), p.folder.clone(), "Old Sessions", Verdict::Ask, &format!("Chat history not used for {} days (the project folder itself is never touched)", idle as u64));
        }
    }
    // old file-history snapshots and uploads
    for (sub, label, kind) in [("file-history", "Edit history", "Old Sessions"), ("uploads", "Uploaded files", "Temp Files")] {
        for f in children(&cl.join(sub)) {
            if age_days(&f) > m::CLAUDE_OLD_DATA_DAYS as f64 {
                c.add(Group::AiTools, &format!("{label} · {}", short_id(&fname(&f))), f, kind, Verdict::Ask, &format!("{label} older than {} days", m::CLAUDE_OLD_DATA_DAYS));
            }
        }
    }
    if claude_running {
        // a live session may hold files in these folders
        for it in c.items.iter_mut().filter(|i| i.group == Group::AiTools && i.kind == "Temp Files" && i.verdict == Verdict::Safe) {
            if it.age_days < 1.0 {
                it.verdict = Verdict::Ask;
                it.reason = "Claude is running — this may belong to an open session".into();
            }
        }
    }
}

fn short_id(s: &str) -> String {
    if s.chars().count() > 14 {
        format!("{}…", s.chars().take(12).collect::<String>())
    } else {
        s.to_string()
    }
}

fn ago(days: f64) -> String {
    if days < 1.0 {
        "today".into()
    } else if days < 2.0 {
        "yesterday".into()
    } else {
        format!("{} days ago", days as u64)
    }
}

fn process_running(name: &str) -> bool {
    Command::new("/usr/bin/pgrep").args(["-x", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

// ---- other AI tools (AIToolScanning catalog)

fn scan_ai_tools(c: &mut Ctx) {
    let h = home();
    for sub in ["codex_apps_server_info", "codex_apps_tools"] {
        c.add(Group::AiTools, "Codex cache", h.join(".codex/cache").join(sub), "Caches", Verdict::Safe, "Codex app cache");
    }
    let gem = h.join(".gemini/antigravity-browser-profile");
    for sub in ["Default/Cache", "Default/Code Cache", "Default/DawnGraphiteCache", "Default/DawnWebGPUCache", "Default/GPUCache", "Default/Service Worker/CacheStorage", "GraphiteDawnCache", "component_crx_cache", "extensions_crx_cache"] {
        c.add(Group::AiTools, "Gemini / Antigravity browser cache", gem.join(sub), "Caches", Verdict::Safe, "Browser cache of the agent profile");
    }
    let mcp = h.join(".cache/chrome-devtools-mcp/chrome-profile");
    for sub in ["Default/Cache", "Default/Code Cache", "Default/DawnCache", "Default/GPUCache", "Default/GrShaderCache", "Default/GraphiteDawnCache", "Default/Service Worker/CacheStorage", "GraphiteDawnCache", "component_crx_cache", "extensions_crx_cache"] {
        c.add(Group::AiTools, "Chrome DevTools MCP cache", mcp.join(sub), "Caches", Verdict::Safe, "Browser cache of the MCP profile");
    }
    for f in children(&h.join(".local/share/cursor-agent")) {
        if f.extension().map(|e| e == "log").unwrap_or(false) {
            c.add(Group::AiTools, "Cursor agent log", f, "Logs", Verdict::Safe, "Cursor agent log file");
        }
    }
    c.add(Group::AiTools, "OpenCode logs", h.join(".local/share/opencode/log"), "Logs", Verdict::Safe, "OpenCode logs");
    // local models are big and costly to get back: always ask
    c.add(Group::AiTools, "Ollama models", h.join(".ollama/models"), "Local Models", Verdict::Ask, "Downloaded models may still be used by your projects");
    c.add(Group::AiTools, "LM Studio models", h.join(".lmstudio/models"), "Local Models", Verdict::Ask, "Downloaded models may still be used by your projects");
    c.add(Group::AiTools, "Hugging Face cache", h.join(".cache/huggingface"), "Local Models", Verdict::Ask, "Downloaded models and datasets; large to download again");
}

// ---- System Junk: user caches (CMUserCachesScanTask)

fn scan_user_caches(c: &mut Ctx) {
    let h = home();
    let caches = h.join("Library/Caches");
    let skip = ["claude-cli-nodejs", "com.anthropic.claudefordesktop", "com.anthropic.claudefordesktop.ShipIt"];
    let keep = &super::rules::catalog().keep_caches;
    for p in children(&caches) {
        if c.stopped() {
            return;
        }
        let n = fname(&p);
        if skip.contains(&n.as_str()) || n.starts_with('.') || (n == "Homebrew" && c.brew) {
            continue;
        }
        if let Some(k) = keep.iter().find(|k| k.matches(&n)) {
            c.add(Group::AppCaches, &pretty(&n), p.clone(), "Caches", Verdict::Keep, &k.reason);
            // neither Claude nor a click turns these into junk
            if let Some(it) = c.items.last_mut().filter(|i| i.path == p) {
                it.hard_keep = true;
            }
            continue;
        }
        let (v, why) = if n.starts_with("com.apple.") {
            (Verdict::Ask, "macOS system cache — usually safe, rebuilt by macOS")
        } else if n == "Homebrew" {
            (Verdict::Safe, "Downloaded installers; Homebrew fetches them again")
        } else {
            (Verdict::Safe, "App cache; the app rebuilds it")
        };
        c.add(Group::AppCaches, &pretty(&n), p, "Caches", v, why);
    }
}
// ---- App caches: hard-coded app rules from SystemJunkScanning + Electron + sandbox

fn scan_app_caches(c: &mut Ctx) {
    let h = home();
    for r in &super::rules::catalog().app_caches {
        c.add(Group::AppCaches, &r.title, h.join(&r.path), "Caches", r.verdict(), &r.reason);
    }
    // Electron / Chromium app caches (CMElectronAppsCacheScanTask)
    let support = h.join("Library/Application Support");
    for app in children(&support) {
        if c.stopped() {
            return;
        }
        let n = fname(&app);
        if n == "Claude" || !app.is_dir() {
            continue;
        }
        for base in [app.clone(), app.join("Default")] {
            for sub in CHROMIUM_CACHES {
                let p = base.join(sub);
                if p.is_dir() {
                    c.add(Group::AppCaches, &format!("{} · {}", pretty(&n), sub), p, "Caches", Verdict::Safe, "Web view cache; rebuilt automatically");
                }
            }
        }
    }
    // sandboxed apps (CMSandboxContainersCachesScanTask)
    for ct in children(&h.join("Library/Containers")) {
        if c.stopped() {
            return;
        }
        let n = fname(&ct);
        let p = ct.join("Data/Library/Caches");
        if p.is_dir() {
            let v = if n.starts_with("com.apple.") { Verdict::Ask } else { Verdict::Safe };
            c.add(Group::AppCaches, &format!("{} (sandboxed)", pretty(&n)), p, "Caches", v, "Sandboxed app cache; rebuilt automatically");
        }
    }
}

// ---- logs (CMUserLogsScanTask, DiagnosticLogs)

fn scan_logs(c: &mut Ctx) {
    let h = home();
    for p in children(&h.join("Library/Logs")) {
        let n = fname(&p);
        if n == "Claude" || n.starts_with('.') {
            continue;
        }
        let title = if n == "DiagnosticReports" { "Crash reports".to_string() } else { format!("{} logs", pretty(&n)) };
        c.add(Group::Logs, &title, p, "Logs", Verdict::Safe, "Log files; only useful for debugging");
    }
    for ct in children(&h.join("Library/Containers")) {
        let p = ct.join("Data/Library/Logs");
        if p.is_dir() {
            c.add(Group::Logs, &format!("{} logs (sandboxed)", pretty(&fname(&ct))), p, "Logs", Verdict::Safe, "Log files of a sandboxed app");
        }
    }
}

// ---- developer caches (package managers and build tools)
fn scan_developer(c: &mut Ctx) {
    let h = home();
    for r in &super::rules::catalog().developer {
        c.add(Group::Developer, &r.title, h.join(&r.path), "Caches", r.verdict(), &r.reason);
    }
}
// ---- Homebrew: its own cleanup (old versions and downloads) when brew is there
fn brew_path() -> Option<PathBuf> {
    super::run::find_tool(&["/opt/homebrew/bin/brew", "/usr/local/bin/brew"])
}

/// What `brew cleanup` would free, or None when brew didn't answer.
fn brew_dry_run(brew: &Path) -> Option<u64> {
    let o = super::run::output(
        std::process::Command::new(brew).args(["cleanup", "--dry-run", "--prune=all"]).env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_ENV_HINTS", "1").env("HOMEBREW_NO_ANALYTICS", "1"),
        std::time::Duration::from_secs(60),
    )?;
    o.status.success().then(|| brew_freed(&String::from_utf8_lossy(&o.stdout)))
}

/// Wait for Homebrew's answer (unless the scan stops) and add it to the
/// items and the live tallies. Without an answer its download cache is
/// offered as a plain cache instead, as when brew isn't there.
fn join_brew(c: &mut Ctx, job: std::thread::JoinHandle<Option<u64>>, brew: &Path, shared: &Shared, ctx: &egui::Context, cancel: &AtomicBool) {
    while !job.is_finished() {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let freed = job.join().ok().flatten();
    let before = c.items.len();
    match freed {
        Some(bytes) if bytes >= m::MIN_ITEM_BYTES => {
            let prefix = brew.parent().and_then(|b| b.parent()).map(Path::to_path_buf).unwrap_or_default();
            c.add_action(Group::Developer, "Homebrew old versions", prefix.join("Cellar"), "Caches", Verdict::Safe, "Old versions and downloads; brew gets them again", Action::BrewCleanup, bytes);
        }
        Some(_) => {}
        None => {
            let p = home().join("Library/Caches/Homebrew");
            if p.is_dir() {
                c.add(Group::AppCaches, "Homebrew", p, "Caches", Verdict::Safe, "Downloaded installers; Homebrew fetches them again");
                if c.items.len() > before {
                    let it = c.items.last_mut().expect("just added");
                    it.bytes = dir_size(&it.path);
                    if it.bytes < m::MIN_ITEM_BYTES {
                        c.items.pop();
                    }
                }
            }
        }
    }
    let gi = |g: Group| Group::ALL.iter().position(|x| *x == g).unwrap_or(0);
    let mut g = shared.lock_ok();
    if cancel.load(Ordering::SeqCst) {
        return;
    }
    g.live[gi(Group::Developer)].done += 1;
    for it in c.items.iter().skip(before) {
        if it.verdict != Verdict::Keep {
            let l = &mut g.live[gi(it.group)];
            l.bytes += it.bytes;
            l.items += 1;
        }
    }
    let weights = group_weights(last_scan().map(|l| l.groups).as_deref());
    let p = FIND + (1.0 - FIND) * live_progress(&g.live, &weights);
    g.progress = g.progress.max(p);
    drop(g);
    ctx.request_repaint();
}
/// What `brew cleanup --dry-run` says it would free ("…free approximately 1.2GB…").
fn brew_freed(out: &str) -> u64 {
    let Some(line) = out.lines().rev().find(|l| l.contains("would free approximately")) else { return 0 };
    let Some(amount) = line.split("approximately").nth(1).and_then(|r| r.split_whitespace().next()) else { return 0 };
    let split = amount.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(amount.len());
    let (num, unit) = amount.split_at(split);
    let n: f64 = num.parse().unwrap_or(0.0);
    let mul = match unit.trim().to_uppercase().as_str() {
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (n * mul) as u64
}
fn run_brew_cleanup() -> Result<(), String> {
    let brew = super::run::find_tool(&["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]).ok_or("Homebrew isn't installed any more")?;
    let o = super::run::output(
        std::process::Command::new(brew).args(["cleanup", "--prune=all"]).env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_ENV_HINTS", "1").env("HOMEBREW_NO_ANALYTICS", "1"),
        std::time::Duration::from_secs(600),
    )
    .ok_or("brew cleanup didn't finish")?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().lines().last().unwrap_or("brew cleanup failed").to_string())
    }
}
// ---- old installers in Downloads
fn scan_installers(c: &mut Ctx) {
    let rules = &super::rules::catalog().installers;
    for (p, age) in installers(&home().join("Downloads"), &rules.extensions, rules.min_age_days as f64) {
        let name = fname(&p);
        c.add(Group::Installers, &name, p, "Installers", Verdict::Ask, &format!("Installer downloaded {} days ago; the app it installed stays", age as u64));
    }
}
/// Installer files (by extension) in `dir` and one level down, older than `min_days`.
pub(super) fn installers(dir: &Path, exts: &[String], min_days: f64) -> Vec<(PathBuf, f64)> {
    let mut out = Vec::new();
    let look = |d: &Path, out: &mut Vec<(PathBuf, f64)>| {
        for p in children(d) {
            let Ok(md) = p.symlink_metadata() else { continue };
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            // a .pkg can be a folder (bundle); both count
            if (md.is_file() || md.is_dir()) && exts.iter().any(|e| *e == ext) {
                let age = age_days(&p);
                if age >= min_days {
                    out.push((p, age));
                }
            }
        }
    };
    look(dir, &mut out);
    for sub in children(dir) {
        if sub.symlink_metadata().is_ok_and(|m| m.is_dir()) && sub.extension().is_none() {
            look(&sub, &mut out);
        }
    }
    out
}
// ---- Trash bins

fn scan_trash(c: &mut Ctx) {
    let t = home().join(".Trash");
    if std::fs::read_dir(&t).is_ok() {
        c.add(Group::Trash, "Trash", t, "Trash", Verdict::Safe, "Already deleted items; emptying can't be undone");
        if let Some(it) = c.items.last_mut() {
            if it.group == Group::Trash {
                it.permanent = true;
            }
        }
    }
}

// ---------------------------------------------------------------- AI judge

fn judge(shared: &Shared, ctx: &egui::Context, agent: super::agent::Agent, cancel: &Arc<AtomicBool>) {
    let name = agent.name();
    if !agent.ready() {
        let mut g = shared.lock_ok();
        g.ai_error = Some(format!("{}, so only the built-in rules were used.", agent.why_not()));
        return;
    }
    let h = home().display().to_string();
    let candidates: Vec<usize> = {
        let g = shared.lock_ok();
        g.items.iter().enumerate().filter(|(_, it)| !it.hard_keep && it.group != Group::Captures && it.bytes >= m::AI_MIN_BYTES).map(|(i, _)| i).collect()
    };
    // what the person kept before tells Claude what they care about
    let mut system = SYSTEM.to_string();
    let before = kept();
    if !before.is_empty() {
        system.push_str(" The person chose to KEEP these paths in earlier cleanups; judge similar paths (same app, same kind of data) as keep or ask, never safe: ");
        let list: Vec<String> = before.iter().rev().take(KEPT_IN_PROMPT).map(|p| p.display().to_string().replacen(&h, "~", 1)).collect();
        system.push_str(&list.join("; "));
        system.push('.');
    }
    let batches: Vec<Vec<usize>> = candidates.chunks(m::AI_BATCH).map(|c| c.to_vec()).collect();
    let n = batches.len().max(1);
    for (bi, batch) in batches.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let lines: String = {
            let g = shared.lock_ok();
            batch
                .iter()
                .enumerate()
                .map(|(k, &i)| {
                    let it = &g.items[i];
                    format!(
                        "{k} | {} | {} | modified {} days ago | group: {} | rule says {} ({}){}\n",
                        it.path.display().to_string().replacen(&h, "~", 1),
                        crate::kit::glass::human_bytes(it.bytes),
                        it.age_days as u64,
                        it.group.title(),
                        it.verdict.word().to_lowercase(),
                        it.rule_reason,
                        if it.app_running { " | app is open now" } else { "" }
                    )
                })
                .collect()
        };
        {
            let mut g = shared.lock_ok();
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            g.ai_status = format!("{name} is reviewing {} paths (batch {} of {})…", batch.len(), bi + 1, n);
            g.progress = bi as f32 / n as f32;
        }
        ctx.request_repaint();
        let answer = agent.ask_usage(&system, &lines, cancel).map(|(t, u)| {
            crate::kit::aicheck::log_usage("cleanup", agent, u);
            t
        });
        let mut g = shared.lock_ok();
        // stopped: the page moved on; this run writes nothing more
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let text = match answer {
            Ok(t) => t,
            Err(err) => {
                g.ai_error = Some(if err.is_empty() { format!("{name} couldn't review the paths; the built-in rules were used.") } else { format!("{name} couldn't review the paths ({err}); the built-in rules were used.") });
                return;
            }
        };
        let verdicts = parse_verdicts(&text);
        if verdicts.is_empty() {
            g.ai_error = Some(format!("{name}'s answer couldn't be read; the built-in rules were used for some items."));
            continue;
        }
        g.ai_used = true;
        for (k, v, r) in verdicts {
            let Some(&i) = batch.get(k) else { continue };
            let it = &mut g.items[i];
            let new = merge_verdict(it.verdict, v, it.app_running);
            if new != it.verdict || !r.is_empty() {
                it.by = Who::Ai;
                if !r.is_empty() {
                    it.reason = r;
                }
            }
            it.verdict = new;
        }
        drop(g);
        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------- kept by you

/// How many kept paths Claude is shown (the newest).
const KEPT_IN_PROMPT: usize = 60;

fn kept_file() -> PathBuf {
    crate::kit::fs::data_path("cleanup-kept.json")
}

/// Paths the person chose to keep, oldest first.
pub fn kept() -> Vec<PathBuf> {
    crate::kit::fs::load_json(&kept_file())
}

/// Remember that the person keeps `paths`: later scans don't offer them.
pub fn remember_kept(paths: impl IntoIterator<Item = PathBuf>) {
    let mut all = kept();
    for p in paths {
        if !all.contains(&p) {
            all.push(p);
        }
    }
    crate::kit::fs::save_json(&kept_file(), &all, true);
}

/// Offer `path` again (Undo on a Keep).
pub fn forget_kept_one(path: &std::path::Path) {
    let mut all = kept();
    all.retain(|p| p != path);
    crate::kit::fs::save_json(&kept_file(), &all, true);
}

/// Offer everything again.
pub fn forget_kept() {
    crate::kit::fs::save_json(&kept_file(), &Vec::<PathBuf>::new(), true);
}

/// Combine a rule's verdict with the AI's: the AI may always be more careful;
/// it may relax a rule's "ask" to "safe", except for items the person must
/// confirm (app open). It never relaxes "keep".
fn merge_verdict(rule: Verdict, ai: Verdict, app_running: bool) -> Verdict {
    match (rule, ai) {
        (_, ai) if ai.rank() > rule.rank() => ai,
        (Verdict::Ask, Verdict::Safe) if !app_running => Verdict::Safe,
        (cur, _) => cur,
    }
}

const SYSTEM: &str = "You are the safety judge inside a Mac cleanup app. You only see file and folder PATHS with size and age — never their contents. \
For each numbered line decide: \
\"safe\" = removing it cannot lose the person's work, settings or history; the app rebuilds or re-downloads it (caches, logs, temp files, build outputs, old versions). \
\"ask\" = probably removable but it could cost something: a big re-download, losing chat/history, an app that is open, or a purpose you are not sure about. \
\"keep\" = personal files, settings, credentials, databases, active project data, or anything that looks important. \
Claude Code chat histories of projects the person still uses must be keep; histories of deleted folders are ask. When in doubt choose ask. \
Reply ONLY with a JSON array like [{\"i\":0,\"v\":\"safe\",\"r\":\"reason in under 14 words\"}] covering every line.";

fn parse_verdicts(text: &str) -> Vec<(usize, Verdict, String)> {
    let (Some(a), Some(b)) = (text.find('['), text.rfind(']')) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[a..=b]) else { return vec![] };
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let i = e["i"].as_u64()? as usize;
                    let verdict = match e["v"].as_str()? {
                        "safe" => Verdict::Safe,
                        "keep" => Verdict::Keep,
                        _ => Verdict::Ask,
                    };
                    Some((i, verdict, e["r"].as_str().unwrap_or("").to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------- removal

/// What a clean removes, in order: the Trash is emptied first, so what this
/// clean moves there can still be put back.
fn clean_order(items: &[Item]) -> Vec<Item> {
    let mut t: Vec<Item> = items.iter().filter(|i| i.selected && i.verdict != Verdict::Keep && !i.hard_keep).cloned().collect();
    t.sort_by_key(|i| i.group != Group::Trash);
    t
}

/// How one item will be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// Moved to the Trash: can be put back.
    Trash,
    /// Deleted outright (rebuilt by its app, or the person chose "delete now").
    Deleted,
    /// The Trash itself, emptied.
    Emptied,
    /// Removed by a tool (simctl, brew).
    Command,
}

impl How {
    pub fn verb(self) -> &'static str {
        match self {
            How::Trash => "Moved to Trash",
            How::Deleted => "Deleted",
            How::Emptied => "Emptied",
            How::Command => "Removed",
        }
    }
}

/// The dry run of a clean: what goes, in order, and how. Nothing is touched.
pub fn plan_clean(items: &[Item], delete_now: bool) -> Vec<(Item, How)> {
    clean_order(items)
        .into_iter()
        .map(|it| {
            let how = if it.group == Group::Trash {
                How::Emptied
            } else if it.by_command() {
                How::Command
            } else if delete_now || it.rebuilds() {
                How::Deleted
            } else {
                How::Trash
            };
            (it, how)
        })
        .collect()
}

/// Kinds whose folders a running app may be writing into.
fn live_kind(kind: &str) -> bool {
    matches!(kind, "Caches" | "Temp Files" | "Build Data")
}

pub fn start_clean(shared: Shared, ctx: egui::Context, delete_now: bool) {
    let plan: Vec<(Item, How)> = {
        let mut g = shared.lock_ok();
        g.phase = Phase::Cleaning;
        g.cleaned_bytes = 0;
        g.cleaned_count = 0;
        g.failed.clear();
        g.log.clear();
        g.trashed_bytes = 0;
        g.cleaned.clear();
        g.stopped = false;
        g.clean_done = 0;
        g.cancel = Arc::new(AtomicBool::new(false));
        let plan = plan_clean(&g.items, delete_now);
        g.clean_total = plan.iter().map(|(it, _)| it.bytes).sum();
        plan
    };
    let cancel = shared.lock_ok().cancel.clone();
    std::thread::spawn(move || {
        let n = plan.len().max(1);
        // minutes may have passed since the scan: an app opened since then keeps its cache
        let (running_ids, running_names) = running_apps();
        // and what every process has open right now (one lsof run)
        let open = if plan.iter().any(|(it, _)| it.action == Action::Files && live_kind(it.kind)) { super::inuse::OpenFiles::snapshot() } else { None };
        let mut journal = super::journal::Entry::now();
        // bytes of the items before the one in hand
        let mut base = 0u64;
        for (k, (it, how)) in plan.iter().enumerate() {
            if k > 0 {
                base += plan[k - 1].0.bytes;
            }
            // Stop: what's left stays (a deletion stops mid-item, below)
            if cancel.load(Ordering::SeqCst) {
                shared.lock_ok().stopped = true;
                break;
            }
            {
                let mut g = shared.lock_ok();
                g.current = it.path.display().to_string();
                g.progress = k as f32 / n as f32;
                g.clean_done = base;
            }
            crate::kit::loader::test_pause();
            ctx.request_repaint();
            let skip = |why: String, shared: &Shared, journal: &mut super::journal::Entry| {
                let mut g = shared.lock_ok();
                g.log.push(format!("Skipped {}: {why}", it.path.display()));
                journal.failed.push((it.path.display().to_string(), why.clone()));
                g.failed.push((it.title.clone(), why));
            };
            if it.action == Action::Files {
                // last safety net
                if let Some(why) = guard(it.group, &it.path).filter(|_| it.group != Group::Trash) {
                    skip(why.to_string(), &shared, &mut journal);
                    continue;
                }
                if it.kind == "Caches" && !it.app_running && it.by != Who::You && app_open(&running_ids, &running_names, &it.path, &it.title) {
                    skip("its app was opened after the scan".into(), &shared, &mut journal);
                    continue;
                }
                // even with the person's OK: an open database would be written to after it's gone
                if live_kind(it.kind) {
                    if let Some(why) = super::inuse::blocked(&it.path, open.as_ref()) {
                        skip(why.to_string(), &shared, &mut journal);
                        continue;
                    }
                }
            }
            let res = match (how, &it.action) {
                (How::Emptied, _) => empty_trash(&it.path),
                (How::Command, Action::BrewCleanup) => run_brew_cleanup(),
                (How::Command, Action::Upload { key, local }) => super::captures::delete_upload(local, key),
                (How::Command, a) => super::xcode::remove(a).or_else(|e| if matches!(a, Action::SimDevice(_)) && it.path.is_dir() { remove(&it.path) } else { Err(e) }),
                (How::Trash, _) => move_to_trash(&it.path),
                (How::Deleted, _) => {
                    // file by file, so the counter moves and Stop cuts in
                    let mut last = std::time::Instant::now();
                    let mut tick = |gone: u64| {
                        shared.lock_ok().clean_done = base + gone.min(it.bytes);
                        if last.elapsed().as_millis() >= 50 {
                            last = std::time::Instant::now();
                            ctx.request_repaint();
                        }
                    };
                    remove_tracked(&it.path, &cancel, &mut tick)
                }
            };
            let mut g = shared.lock_ok();
            if let Some(Stopped(gone)) = res.as_ref().err().and_then(|e| stopped_bytes(e.clone())) {
                // stopped inside the item: what went is cleaned, the rest stays
                let gone = gone.min(it.bytes);
                g.stopped = true;
                g.cleaned_bytes += gone;
                g.cleaned.push((it.group, gone));
                g.log.push(format!("Stopped in {} ({} removed)", it.path.display(), crate::kit::glass::human_bytes(gone)));
                if let Some(x) = g.items.iter_mut().find(|x| x.path == it.path) {
                    x.bytes = x.bytes.saturating_sub(gone);
                }
                journal.items.push(super::journal::Removed { title: it.title.clone(), path: it.path.display().to_string(), bytes: gone, how: *how });
                break;
            }
            match res {
                Ok(()) => {
                    if *how == How::Trash {
                        g.trashed_bytes += it.bytes;
                    }
                    g.cleaned_bytes += it.bytes;
                    g.cleaned_count += 1;
                    g.cleaned.push((it.group, it.bytes));
                    g.log.push(format!("{}: {} ({})", how.verb(), it.path.display(), crate::kit::glass::human_bytes(it.bytes)));
                    g.items.retain(|x| x.path != it.path);
                    journal.items.push(super::journal::Removed { title: it.title.clone(), path: it.path.display().to_string(), bytes: it.bytes, how: *how });
                    if it.kind == super::captures::HISTORY {
                        crate::kit::old_captures::history_changed();
                    }
                    // an archive's day folder left empty goes too
                    if it.kind == "Archives" && matches!(how, How::Trash | How::Deleted) {
                        if let Some(day) = it.path.parent() {
                            if children(day).iter().all(|c| fname(c) == ".DS_Store") {
                                let _ = std::fs::remove_dir_all(day);
                            }
                        }
                    }
                }
                Err(e) => {
                    g.log.push(format!("Couldn't remove {}: {e}", it.path.display()));
                    journal.failed.push((it.path.display().to_string(), e.clone()));
                    g.failed.push((it.title.clone(), e));
                }
            }
        }
        let mut g = shared.lock_ok();
        g.progress = 1.0;
        g.clean_done = g.clean_total;
        g.phase = Phase::Done;
        if g.cleaned_bytes > 0 {
            record_clean(g.cleaned_bytes);
        }
        if !journal.items.is_empty() || !journal.failed.is_empty() {
            super::journal::record(journal);
        }
        ctx.request_repaint();
        crate::kit::sound::play(if g.failed.is_empty() { crate::kit::sound::Cue::Done } else { crate::kit::sound::Cue::Oops });
    });
}
/// Past cleans, oldest first: (unix seconds, bytes removed). Kept so the
/// widget can show what Cleanup did without a new scan.
/// Read once from disk, then kept in memory (the widget asks every frame).
/// What the last finished scan found per group, and when (kept on disk so
/// the start page can show it).
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LastScan {
    pub when: i64,
    /// bytes per group, in `Group::ALL` order
    pub groups: Vec<u64>,
}

pub fn last_scan() -> Option<LastScan> {
    let mut c = LAST_SCAN.lock_ok();
    if c.is_none() {
        *c = Some(std::fs::read_to_string(crate::kit::fs::data_path("cleanup").join("last-scan.json")).ok().and_then(|s| serde_json::from_str(&s).ok()));
    }
    c.clone().flatten()
}

static LAST_SCAN: Mutex<Option<Option<LastScan>>> = Mutex::new(None);

fn record_scan(g: &State) {
    let ls = LastScan { when: chrono::Utc::now().timestamp(), groups: Group::ALL.iter().map(|k| g.group_bytes(*k)).collect() };
    let dir = crate::kit::fs::data_path("cleanup");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(s) = serde_json::to_string(&ls) {
            let _ = std::fs::write(dir.join("last-scan.json"), s);
        }
    }
    *LAST_SCAN.lock_ok() = Some(Some(ls));
}

pub fn history() -> Vec<(i64, u64)> {
    let mut c = HISTORY.lock_ok();
    c.get_or_insert_with(|| std::fs::read_to_string(crate::kit::fs::data_path("cleanup").join("history.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()).clone()
}

static HISTORY: Mutex<Option<Vec<(i64, u64)>>> = Mutex::new(None);

fn record_clean(bytes: u64) {
    let mut h = history();
    h.push((chrono::Utc::now().timestamp(), bytes));
    let keep = h.len().saturating_sub(200);
    h.drain(..keep);
    let dir = crate::kit::fs::data_path("cleanup");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(s) = serde_json::to_string(&h) {
            let _ = std::fs::write(dir.join("history.json"), s);
        }
    }
    *HISTORY.lock_ok() = Some(h);
}

/// "today", "yesterday", "3 days ago".
pub fn days_ago(when: i64) -> String {
    match (chrono::Utc::now().timestamp() - when).max(0) / 86400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        d => format!("{d} days ago"),
    }
}

/// A deletion cut short by Stop, with the bytes removed until then.
struct Stopped(u64);

const STOPPED: &str = "\u{0}stopped";

/// `Err` of a cut-short [`remove_tracked`] reads "\0stopped <bytes>".
fn stopped_bytes(e: String) -> Option<Stopped> {
    e.strip_prefix(STOPPED).and_then(|n| n.trim().parse().ok()).map(Stopped)
}

/// Delete `p` file by file: `tick` gets the bytes gone so far, and `cancel`
/// stops it between files (`Err` per [`stopped_bytes`]). Anything the walk
/// couldn't take goes the usual way ([`remove`]) at the end.
fn remove_tracked(p: &Path, cancel: &AtomicBool, tick: &mut dyn FnMut(u64)) -> Result<(), String> {
    fn walk(p: &Path, cancel: &AtomicBool, gone: &mut u64, tick: &mut dyn FnMut(u64)) -> bool {
        use std::os::unix::fs::MetadataExt;
        let Ok(md) = p.symlink_metadata() else { return true };
        if md.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if !walk(&e.path(), cancel, gone, tick) {
                        return false;
                    }
                }
            }
            let _ = std::fs::remove_dir(p);
            return true;
        }
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        if std::fs::remove_file(p).is_ok() {
            *gone += md.blocks() * 512;
            tick(*gone);
        }
        true
    }
    let mut gone = 0;
    if !walk(p, cancel, &mut gone, tick) {
        return Err(format!("{STOPPED} {gone}"));
    }
    if p.symlink_metadata().is_err() {
        return Ok(());
    }
    remove(p)
}

fn remove(p: &Path) -> Result<(), String> {
    let first = remove_once(p);
    if first.is_ok() || !p.exists() {
        return Ok(());
    }
    // read-only folders (Go's module cache, npm tarballs) and locked files
    // can't be emptied as they are: make what's ours writable, then retry
    unlock_tree(p);
    remove_once(p)
}

fn remove_once(p: &Path) -> Result<(), String> {
    let md = p.symlink_metadata().map_err(|e| e.to_string())?;
    if md.is_dir() {
        std::fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(p).map_err(|e| e.to_string())
    }
}

/// Owner write/exec on every folder and no "locked" flag on anything below
/// `p`, so the tree can be removed. Never follows symlinks.
fn unlock_tree(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(md) = p.symlink_metadata() else { return };
    clear_flags(p);
    if md.file_type().is_symlink() {
        return;
    }
    if md.is_dir() {
        let mode = md.permissions().mode();
        if mode & 0o700 != 0o700 {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o700));
        }
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                unlock_tree(&e.path());
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn clear_flags(p: &Path) {
    use std::os::unix::ffi::OsStrExt;
    if let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) {
        // drop user-settable flags (uchg = Finder's "Locked"); system flags need root
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::lstat(c.as_ptr(), &mut st) == 0 && st.st_flags & libc::UF_SETTABLE != 0 {
                libc::lchflags(c.as_ptr(), st.st_flags & !libc::UF_SETTABLE);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn clear_flags(_: &Path) {}

/// Empty a folder's contents but keep the folder itself (the Trash belongs
/// to macOS and can't be removed). Names what couldn't go, if anything.
fn empty_folder(p: &Path) -> Result<(), String> {
    let mut failed: Vec<(String, String)> = Vec::new();
    for c in children(p) {
        if let Err(e) = remove(&c) {
            failed.push((fname(&c), e));
        }
    }
    match failed.len() {
        0 => Ok(()),
        1 => Err(format!("{}: {}", failed[0].0, failed[0].1)),
        n => Err(format!("{n} items, e.g. {}: {}", failed[0].0, failed[0].1)),
    }
}

/// Empty the Trash: directly first; what's left (owned by another user,
/// system-locked) goes through Finder, which asks for the password itself.
fn empty_trash(p: &Path) -> Result<(), String> {
    let direct = empty_folder(p);
    if direct.is_ok() {
        return direct;
    }
    #[cfg(target_os = "macos")]
    if crate::kit::macos::osascript("tell application \"Finder\" to empty trash").is_ok() && children(p).iter().all(|c| fname(c) == ".DS_Store") {
        return Ok(());
    }
    direct
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_bar_creeps_then_hands_over_without_going_back() {
        use super::{creep, Phase, State, CREEP_TO};
        // always moving early on, eased out, never past the cap
        assert_eq!(creep(0.0), 0.0);
        assert!(creep(0.5) > 0.0);
        let mut last = 0.0;
        for i in 1..=120 {
            let c = creep(i as f32 * 0.25);
            assert!(c >= last && c <= CREEP_TO, "{c} after {last}");
            last = c;
        }
        assert!(creep(10.0) > 0.05);
        // shown = max(real, creep): real progress below the creep doesn't pull it back
        let t0 = std::time::Instant::now() - std::time::Duration::from_secs(10);
        let mut st = State { phase: Phase::Scanning, started: Some(t0), progress: 0.015, ..Default::default() };
        assert!(st.shown_progress() > 0.05);
        st.progress = 0.3;
        assert_eq!(st.shown_progress(), 0.3);
        // judging counts its own way
        st.phase = Phase::Judging;
        st.progress = 0.0;
        assert_eq!(st.shown_progress(), 0.0);
    }

    #[test]
    fn progress_waits_for_the_last_place_of_each_group() {
        use super::{group_weights, live_progress, GroupLive, Group};
        let w = group_weights(None);
        let mut live = [GroupLive::default(); Group::COUNT];
        // three groups done, four with one big folder left out of 30
        for l in live.iter_mut().take(3) {
            *l = GroupLive { total: 10, done: 10, ..Default::default() };
        }
        for l in live.iter_mut().skip(3) {
            *l = GroupLive { total: 30, done: 29, ..Default::default() };
        }
        let p = live_progress(&live, &w);
        assert!(p < 0.75, "{p}");
        for l in live.iter_mut() {
            l.done = l.total;
        }
        assert_eq!(live_progress(&live, &w), 1.0);
        // a group that was big last time weighs more
        let w = group_weights(Some(&[50_000_000_000, 0, 0, 0, 0, 0, 0, 0]));
        let mut live = [GroupLive { total: 1, done: 1, ..Default::default() }; Group::COUNT];
        live[0] = GroupLive { total: 2, done: 1, ..Default::default() };
        assert!(live_progress(&live, &w) < 0.4);
    }

    use super::*;

    #[test]
    fn read_only_folders_in_the_trash_are_emptied() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("cmdmeow-trash-{}", std::process::id()));
        let ro = base.join("go-mod/cache@v1");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::write(ro.join("a.go"), "x").unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(base.join("go-mod"), std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::write(base.join("note.txt"), "x").unwrap();
        assert_eq!(empty_folder(&base), Ok(()));
        assert!(base.exists() && children(&base).is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn short_id_is_utf8_safe() {
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id("0123456789abcdef"), "0123456789ab…");
        // multi-byte names used to panic on a byte slice
        assert_eq!(short_id("знімок-екрана-2026"), "знімок-екран…");
    }

    #[test]
    fn a_running_app_is_seen_through_its_cache_folder() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
        let (ids, names) = (set(&["com.tinyspeck.slackmacgap", "com.apple.mail", "com.microsoft.vscode"]), set(&["slack", "mail", "code"]));
        let open = |path: &str, title: &str| app_open(&ids, &names, Path::new(path), title);
        // Caches/<bundle id>
        assert!(open("/Users/me/Library/Caches/com.tinyspeck.slackmacgap", "Slackmacgap"));
        // an Electron app: the cache is a folder inside the app's own
        assert!(open("/Users/me/Library/Application Support/Slack/Cache", "Slack · Cache"));
        assert!(open("/Users/me/Library/Application Support/Code/Service Worker/CacheStorage", "Code · Service Worker/CacheStorage"));
        // a sandboxed app, and an extension of it
        assert!(open("/Users/me/Library/Containers/com.apple.mail/Data/Library/Caches", "Mail (sandboxed)"));
        assert!(open("/Users/me/Library/Containers/com.apple.mail.MailQuickLookExtension/Data/Library/Caches", "MailQuickLookExtension (sandboxed)"));
        // apps that aren't open
        assert!(!open("/Users/me/Library/Application Support/Discord/Cache", "Discord · Cache"));
        assert!(!open("/Users/me/Library/Containers/com.apple.mailx/Data/Library/Caches", "Mailx (sandboxed)"));
        assert!(!open("/Users/me/Library/Caches/com.spotify.client", "Spotify"));
        assert_eq!(title_app("Slack · Cache"), "Slack");
        assert_eq!(title_app("Mail (sandboxed)"), "Mail");
        assert_eq!(title_app("Xcode"), "Xcode");
    }

    #[test]
    fn pretty_names() {
        assert_eq!(pretty("com.spotify.client"), "Spotify");
        assert_eq!(pretty("COM.GOOGLE.CHROME"), "Google Chrome");
        assert_eq!(pretty("com.example.my-tool"), "My tool");
        assert_eq!(pretty("com.example.Foo.helper"), "Foo");
        assert_eq!(pretty("Homebrew"), "Homebrew");
        assert_eq!(pretty("com.example.ёлка"), "Ёлка");
    }

    #[test]
    fn verdicts_from_ai_text() {
        let text = "Sure:\n[{\"i\":0,\"v\":\"safe\",\"r\":\"cache\"},{\"i\":1,\"v\":\"keep\"},{\"i\":2,\"v\":\"maybe\",\"r\":\"?\"},{\"v\":\"safe\"}]\nDone.";
        let v = parse_verdicts(text);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (0, Verdict::Safe, "cache".to_string()));
        assert_eq!(v[1], (1, Verdict::Keep, String::new()));
        // anything unknown is treated as "ask"
        assert_eq!(v[2].1, Verdict::Ask);
        assert!(parse_verdicts("no json here").is_empty());
        assert!(parse_verdicts("[not json]").is_empty());
    }

    #[test]
    fn ai_can_only_be_more_careful_except_ask_to_safe() {
        use Verdict::*;
        assert_eq!(merge_verdict(Safe, Ask, false), Ask);
        assert_eq!(merge_verdict(Safe, Keep, false), Keep);
        assert_eq!(merge_verdict(Ask, Safe, false), Safe);
        // an open app must still be confirmed
        assert_eq!(merge_verdict(Ask, Safe, true), Ask);
        // "keep" is never relaxed
        assert_eq!(merge_verdict(Keep, Safe, false), Keep);
        assert_eq!(merge_verdict(Keep, Ask, false), Keep);
    }

    fn it(group: Group, path: &str, kind: &'static str, verdict: Verdict, by: Who) -> Item {
        Item {
            group,
            title: String::new(),
            path: PathBuf::from(path),
            bytes: 0,
            age_days: 0.0,
            kind,
            verdict,
            reason: String::new(),
            rule_reason: String::new(),
            rule_verdict: verdict,
            by,
            selected: true,
            hard_keep: false,
            permanent: false,
            app_running: false,
            action: Action::Files,
        }
    }

    #[test]
    fn stop_scan_goes_back_to_the_start_and_cancels_the_run() {
        let shared: Shared = Default::default();
        let token = begin_run(&shared, None);
        shared.lock_ok().items.push(it(Group::Logs, "/tmp/a", "Logs", Verdict::Safe, Who::Rule));
        stop_scan(&shared);
        assert!(token.load(Ordering::SeqCst));
        let g = shared.lock_ok();
        assert_eq!(g.phase, Phase::Intro);
        assert!(g.items.is_empty());
        // the start screen's own token is fresh
        assert!(!g.cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn a_new_scan_cancels_the_one_before() {
        let shared: Shared = Default::default();
        let first = begin_run(&shared, None);
        let second = begin_run(&shared, Some(super::super::agent::Agent::Claude));
        assert!(first.load(Ordering::SeqCst));
        assert!(!second.load(Ordering::SeqCst));
        let g = shared.lock_ok();
        assert_eq!(g.phase, Phase::Scanning);
        assert!(g.with_ai);
    }

    #[test]
    fn skip_ai_shows_results_by_the_rules() {
        let shared: Shared = Default::default();
        let token = begin_run(&shared, Some(super::super::agent::Agent::Claude));
        {
            let mut g = shared.lock_ok();
            g.phase = Phase::Judging;
            let mut a = it(Group::Logs, "/tmp/a", "Logs", Verdict::Safe, Who::Rule);
            a.selected = false;
            let mut b = it(Group::Logs, "/tmp/b", "Logs", Verdict::Ask, Who::Rule);
            b.selected = false;
            g.items = vec![a, b];
        }
        skip_ai(&shared);
        assert!(token.load(Ordering::SeqCst));
        let g = shared.lock_ok();
        assert_eq!(g.phase, Phase::Results);
        assert!(g.ai_error.as_deref().is_some_and(|e| e.contains("skipped")));
        assert!(g.items[0].selected);
        assert!(!g.items[1].selected);
    }

    #[test]
    fn skip_ai_does_nothing_outside_the_ai_check() {
        let shared: Shared = Default::default();
        let token = begin_run(&shared, None);
        skip_ai(&shared);
        assert!(!token.load(Ordering::SeqCst));
        assert_eq!(shared.lock_ok().phase, Phase::Scanning);
    }

    #[test]
    fn trash_is_emptied_before_anything_moves_there() {
        let items = vec![
            it(Group::AiTools, "/a", "Caches", Verdict::Safe, Who::Rule),
            it(Group::Logs, "/b", "Logs", Verdict::Safe, Who::Rule),
            it(Group::Trash, "/t", "Trash", Verdict::Safe, Who::Rule),
        ];
        let order: Vec<Group> = clean_order(&items).iter().map(|i| i.group).collect();
        assert_eq!(order, vec![Group::Trash, Group::AiTools, Group::Logs]);
    }

    #[test]
    fn only_sure_rebuildable_items_skip_the_trash() {
        assert!(it(Group::AppCaches, "/c", "Caches", Verdict::Safe, Who::Rule).rebuilds());
        assert!(it(Group::Logs, "/l", "Logs", Verdict::Safe, Who::Ai).rebuilds());
        assert!(it(Group::Developer, "/d", "Build Data", Verdict::Safe, Who::Rule).rebuilds());
        // the rules weren't sure (iOS backups, Telegram media): to the Trash even when Claude relaxed it
        let mut relaxed = it(Group::AppCaches, "/m", "Caches", Verdict::Ask, Who::Rule);
        relaxed.verdict = Verdict::Safe;
        relaxed.by = Who::Ai;
        assert!(!relaxed.rebuilds());
        // the person allowed it by hand
        assert!(!it(Group::AppCaches, "/y", "Caches", Verdict::Safe, Who::You).rebuilds());
        // not a cache
        assert!(!it(Group::AiTools, "/v", "Stale Versions", Verdict::Safe, Who::Rule).rebuilds());
        assert!(!it(Group::AiTools, "/s", "Old Sessions", Verdict::Safe, Who::Rule).rebuilds());
    }

    #[test]
    fn nested_and_repeated_paths_are_measured_once() {
        let base = std::env::temp_dir().join(format!("cmb-measure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("outer/inner")).unwrap();
        std::fs::write(base.join("outer/inner/f"), vec![1u8; 200_000]).unwrap();
        std::fs::create_dir_all(base.join("tiny")).unwrap();
        std::fs::write(base.join("tiny/f"), b"x").unwrap();
        let p = |r: &str| base.join(r).display().to_string();
        let mut items = vec![
            it(Group::AppCaches, &p("outer/inner"), "Caches", Verdict::Safe, Who::Rule),
            it(Group::AppCaches, &p("outer"), "Caches", Verdict::Safe, Who::Rule),
            it(Group::Logs, &p("outer"), "Logs", Verdict::Safe, Who::Rule),
            it(Group::Logs, &p("tiny"), "Logs", Verdict::Safe, Who::Rule),
        ];
        let shared: Shared = Default::default();
        measure(&mut items, &shared, &egui::Context::default(), 0.0, &AtomicBool::new(false), false);
        assert_eq!(items.len(), 1, "inner, the repeat and the tiny one are gone");
        assert_eq!(items[0].path, base.join("outer"));
        assert_eq!(items[0].group, Group::AppCaches, "the first rule wins");
        assert!(items[0].bytes >= 200_000);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn state_totals() {
        let item = |group, bytes, verdict, selected| Item {
            group,
            title: String::new(),
            path: PathBuf::from(format!("/x/{bytes}")),
            bytes,
            age_days: 0.0,
            kind: "Caches",
            verdict,
            reason: String::new(),
            rule_reason: String::new(),
            rule_verdict: Verdict::Safe,
            by: Who::Rule,
            selected,
            hard_keep: false,
            permanent: false,
            app_running: false,
            action: Action::Files,
        };
        let s = State {
            items: vec![
                item(Group::AiTools, 10, Verdict::Safe, true),
                item(Group::AiTools, 20, Verdict::Ask, false),
                item(Group::Logs, 40, Verdict::Keep, false),
                item(Group::Logs, 80, Verdict::Safe, true),
            ],
            ..Default::default()
        };
        assert_eq!(s.total(), 110);
        assert_eq!(s.selected_bytes(), 90);
        assert_eq!(s.pending_questions(), 1);
        assert_eq!(s.group_bytes(Group::Logs), 80);
        assert_eq!(s.claude_safe_bytes(), 10);
    }

    #[test]
    fn old_captures_go_to_the_trash_and_uploads_are_deleted_from_the_storage() {
        let shot = plain(Group::Captures, "/d/Capture 2026-09-01 at 10.10.04.png", crate::tools::cleanup::captures::SCREENSHOTS, Verdict::Ask, Action::Files);
        let hist = plain(Group::Captures, "/h/1759000000000.png", crate::tools::cleanup::captures::HISTORY, Verdict::Safe, Action::Files);
        let up = plain(Group::Captures, "https://x/v/1", crate::tools::cleanup::captures::UPLOADS, Verdict::Ask, Action::Upload { key: "v/1.mp4".into(), local: PathBuf::from("/d/a.mp4") });
        let plan: Vec<How> = plan_clean(&[shot, hist, up.clone()], false).into_iter().map(|(_, h)| h).collect();
        // the person's files can come back from the Trash; an upload can't
        assert_eq!(plan, vec![How::Trash, How::Trash, How::Command]);
        assert!(up.by_command());
    }

    #[test]
    fn captures_guard_refuses_anything_not_made_by_the_app() {
        let h = home();
        assert!(guard(Group::Captures, &h.join("Desktop/Screenshot 2026-09-01 at 10.10.04.png")).is_some());
        assert!(guard(Group::Captures, &h.join("Desktop")).is_some());
        assert!(guard(Group::Captures, &h.join("Desktop/Capture 2026-09-01 at 10.10.04.png")).is_some(), "a file that isn't there");
    }

    fn plain(group: Group, path: &str, kind: &'static str, verdict: Verdict, action: Action) -> Item {
        Item {
            group,
            title: path.into(),
            path: PathBuf::from(path),
            bytes: 100,
            age_days: 0.0,
            kind,
            verdict,
            reason: String::new(),
            rule_reason: String::new(),
            rule_verdict: verdict,
            by: Who::Rule,
            selected: true,
            hard_keep: false,
            permanent: false,
            app_running: false,
            action,
        }
    }

    #[test]
    fn one_folder_under_two_spellings_is_listed_once() {
        let base = std::env::temp_dir().join(format!("cmdmeow-repeats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Code/app/.venv")).unwrap();
        std::os::unix::fs::symlink(base.join("Code"), base.join("code-link")).unwrap();
        let a = base.join("Code/app/.venv");
        let b = base.join("code-link/app/.venv");
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let mut items = vec![
            plain(Group::Projects, &s(&a), "Build Data", Verdict::Ask, Action::Files),
            plain(Group::Projects, &s(&b), "Build Data", Verdict::Ask, Action::Files),
            plain(Group::Projects, &s(&a), "Build Data", Verdict::Ask, Action::Files),
        ];
        drop_repeats(&mut items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, a);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn dry_run_says_how_each_item_goes() {
        let mut trash = plain(Group::Trash, "/t", "Trash", Verdict::Safe, Action::Files);
        trash.permanent = true;
        let mut kept = plain(Group::Logs, "/k", "Logs", Verdict::Keep, Action::Files);
        kept.selected = true;
        let mut off = plain(Group::Logs, "/off", "Logs", Verdict::Safe, Action::Files);
        off.selected = false;
        let items = vec![
            plain(Group::AppCaches, "/cache", "Caches", Verdict::Safe, Action::Files),
            plain(Group::AiTools, "/history", "Old Sessions", Verdict::Ask, Action::Files),
            plain(Group::Developer, "/sim", "Simulators", Verdict::Safe, Action::SimDevice("AB-12".into())),
            plain(Group::Developer, "/brew", "Caches", Verdict::Safe, Action::BrewCleanup),
            trash,
            kept,
            off,
        ];
        let plan: Vec<(String, How)> = plan_clean(&items, false).into_iter().map(|(i, h)| (i.path.display().to_string(), h)).collect();
        assert_eq!(
            plan,
            vec![("/t".into(), How::Emptied), ("/cache".into(), How::Deleted), ("/history".into(), How::Trash), ("/sim".into(), How::Command), ("/brew".into(), How::Command)],
            "the Trash first, kept and unselected items never"
        );
        // "delete now" deletes what would have gone to the Trash; tools stay tools
        let now: Vec<How> = plan_clean(&items, true).into_iter().map(|(_, h)| h).collect();
        assert_eq!(now, vec![How::Emptied, How::Deleted, How::Deleted, How::Command, How::Command]);
    }

    #[test]
    fn guard_keeps_cleanup_in_the_home_folder() {
        let h = home();
        assert!(guard(Group::AppCaches, Path::new("/System/Library/Caches")).is_some());
        assert!(guard(Group::AppCaches, &h.join("Library/Caches/../../Documents")).is_some());
        assert!(guard(Group::AppCaches, &h.join("Documents/x")).is_some());
        assert!(guard(Group::AppCaches, &h.join("Library/Caches/com.example.app")).is_none());
        // build output in a code folder is judged by the project rules, not refused outright
        assert_eq!(guard(Group::Projects, &h.join("Projects/nothing-here/node_modules")), Some("Not a plain build folder any more"));
    }

    #[test]
    fn brew_dry_run_size() {
        let out = "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,000 files, 60MB)\n==> This operation would free approximately 1.5GB of disk space.\n";
        assert_eq!(brew_freed(out), (1.5 * 1024.0 * 1024.0 * 1024.0) as u64);
        assert_eq!(brew_freed("==> This operation would free approximately 512KB of disk space."), 512 * 1024);
        assert_eq!(brew_freed("nothing to do"), 0);
    }

    #[test]
    fn old_installers_only() {
        let base = std::env::temp_dir().join(format!("cmdmeow-dl-{}", std::process::id()));
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::create_dir_all(base.join("Bundle.pkg")).unwrap();
        for f in ["Old.dmg", "New.dmg", "notes.txt", "sub/Deep.PKG"] {
            std::fs::write(base.join(f), "x").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 86_400);
        for f in ["Old.dmg", "sub/Deep.PKG", "Bundle.pkg"] {
            std::fs::File::open(base.join(f)).unwrap().set_modified(old).unwrap();
        }
        let exts: Vec<String> = ["dmg", "pkg"].map(String::from).to_vec();
        let mut found: Vec<String> = installers(&base, &exts, 14.0).into_iter().map(|(p, _)| p.strip_prefix(&base).unwrap().display().to_string()).collect();
        found.sort();
        assert_eq!(found, vec!["Bundle.pkg", "Old.dmg", "sub/Deep.PKG"]);
        std::fs::remove_dir_all(&base).unwrap();
    }
}

#[cfg(test)]
mod remove_tracked_tests {
    use super::*;

    fn tree(name: &str, files: usize) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cmdmeow-rt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for i in 0..files {
            let sub = d.join(format!("d{}", i % 3));
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(format!("f{i}")), vec![7u8; 5000]).unwrap();
        }
        d
    }

    #[test]
    fn deletes_file_by_file_and_counts() {
        let d = tree("all", 12);
        let mut ticks = vec![];
        assert_eq!(remove_tracked(&d, &AtomicBool::new(false), &mut |b| ticks.push(b)), Ok(()));
        assert!(!d.exists());
        assert_eq!(ticks.len(), 12);
        assert!(ticks.windows(2).all(|w| w[0] < w[1]), "the counter only goes up");
    }

    #[test]
    fn stop_cuts_in_between_files() {
        let d = tree("stop", 12);
        let cancel = AtomicBool::new(false);
        let mut n = 0;
        let res = remove_tracked(&d, &cancel, &mut |_| {
            n += 1;
            if n == 4 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        let gone = res.err().and_then(stopped_bytes).map(|Stopped(b)| b);
        assert!(gone.is_some_and(|b| b > 0), "stopped, with what went");
        assert!(d.exists(), "the rest stays");
        assert_eq!(walkdir_count(&d), 8);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn walkdir_count(p: &Path) -> usize {
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| if e.path().is_dir() { walkdir_count(&e.path()) } else { 1 }).sum()).unwrap_or(0)
    }
}
