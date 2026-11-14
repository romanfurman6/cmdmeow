//! Startup: what starts with the Mac (apps opened at login, launchd agents
//! and daemons), when each runs and whether it runs now. Turn any of them
//! off without removing anything, or turn off every suggested one (updaters,
//! agents of removed apps, known extras) at once, with Undo. A note pops up
//! when an app adds itself to startup or changes what it runs, with a
//! Turn off button. What each item runs is checked: who signed it, a fake
//! Apple name, code handed inline to a shell, temp or hidden folders.
//! Cron jobs, system extensions and the full Background Items list too.
//!
//! Page routes: "" (All), "suggested", "flagged", "off".

use crate::kit::sync::LockExt;
mod extra;
mod items;
mod sign;
mod ui;

use crate::core::tool::{Action, Badge, Cx, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec};
use crate::kit::glass::{tint, Icon};
use egui::{Rect, Ui};
use extra::{BtmItem, CronJob, SysExt};
use items::{Job, LoginApp, Scope, When};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ID: ToolId = "startup";
/// Our notifications' button handler.
const NOTE: &str = "startup";
const SUMMARY: &str = "summary";
/// How long Undo stays offered after a batch.
pub const UNDO_FOR: Duration = Duration::from_secs(60);
/// How often the launchd folders are read again.
const POLL: Duration = Duration::from_secs(20);
/// The same while nothing is on screen and folder events are on.
const HIDDEN_POLL: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Suggested,
    /// Something about what it runs looks wrong.
    Flagged,
    Off,
}

/// A job plist as last seen: what it ran, and its names for the Removed list.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Seen {
    pub fp: String,
    pub name: String,
    pub label: String,
}

/// A startup item that went away (its plist was deleted).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Gone {
    pub name: String,
    pub label: String,
    /// Unix seconds.
    pub at: u64,
}

/// How long a removed item stays listed.
const GONE_FOR: u64 = 7 * 24 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The key a trusted job is kept under.
pub fn trust_key(j: &Job) -> String {
    format!("{:?}|{}", j.scope, j.label)
}

/// One thing switched by a batch, as it was before.
#[derive(Clone, Debug)]
pub enum Change {
    Job(Job),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Login apps turned off here (macOS forgets them, so we keep them to turn back on).
    pub login_off: Vec<(String, PathBuf, bool)>,
    /// Job plists seen before (old format, read once into `seen`).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub known: BTreeSet<PathBuf>,
    /// Job plists seen before, with what they ran: a new one or a changed
    /// one is announced, a deleted one listed under Removed.
    pub seen: BTreeMap<PathBuf, Seen>,
    /// Items that went away lately.
    pub gone: Vec<Gone>,
    /// Jobs the person trusts, with what they ran then (a change drops it).
    pub trusted: BTreeMap<String, String>,
    /// Cron lines seen before.
    pub cron_seen: BTreeSet<String>,
    pub cron_seeded: bool,
    /// `known` has been filled once (the first look announces nothing).
    pub seeded: bool,
    /// Notify when an app adds itself to startup.
    pub notify_new: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { login_off: Vec::new(), known: BTreeSet::new(), seen: BTreeMap::new(), gone: Vec::new(), trusted: BTreeMap::new(), cron_seen: BTreeSet::new(), cron_seeded: false, seeded: false, notify_new: true }
    }
}

fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("startup")
}

impl Settings {
    fn load() -> Settings {
        if let Some(mut s) = std::fs::read_to_string(data_dir().join("settings.json")).ok().and_then(|s| serde_json::from_str::<Settings>(&s).ok()) {
            s.migrate();
            return s;
        }
        // first run: keep the login apps turned off in the Uninstaller's old Login items tab
        let mut s = Settings::default();
        let old = data_dir().with_file_name("uninstaller").join("settings.json");
        if let Some(v) = std::fs::read_to_string(old).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            if let Some(off) = v.get("login_off").and_then(|o| serde_json::from_value(o.clone()).ok()) {
                s.login_off = off;
            }
        }
        s
    }
    /// The old list of plists becomes `seen` (what they ran isn't known yet,
    /// so the first look fills it in without calling anything changed).
    fn migrate(&mut self) {
        if self.seen.is_empty() && !self.known.is_empty() {
            self.seen = std::mem::take(&mut self.known).into_iter().map(|p| (p, Seen::default())).collect();
        }
        self.known.clear();
    }
    pub fn save(&self) {
        let dir = data_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(dir.join("settings.json"), s);
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    pub jobs: Vec<Job>,
    pub jobs_loaded: bool,
    pub apps: Vec<LoginApp>,
    pub apps_loaded: bool,
    pub apps_error: Option<String>,
    pub settings: Settings,
    /// Labels that appeared while the app runs.
    pub new_labels: HashSet<String>,
    /// Labels whose job now runs something else.
    pub changed_labels: HashSet<String>,
    /// Plists we moved to the Trash ourselves (not listed as removed).
    pub trashed: HashSet<PathBuf>,
    pub cron: Vec<CronJob>,
    pub new_cron: HashSet<String>,
    pub sysext: Vec<SysExt>,
    /// The Background Items list, once read (it takes the admin password).
    pub btm: Option<Vec<BtmItem>>,
    pub btm_busy: bool,
    /// Turn off asked from a notification's button: "job|<scope>|<label>" or "cron|<line>".
    pub pending: Vec<String>,
    /// The last batch, for Undo.
    pub last: Option<(Vec<Change>, Instant)>,
    /// Something is being switched: its line.
    pub busy: Option<String>,
}

impl State {
    pub fn trusted(&self, j: &Job) -> bool {
        self.settings.trusted.get(&trust_key(j)).is_some_and(|fp| *fp == j.fp)
    }
    /// Suggested to turn off: likely not needed, still on, not trusted.
    pub fn is_suggested(&self, j: &Job) -> bool {
        j.suggested() && !self.trusted(j)
    }
    /// What looks wrong about a job (nothing once it's trusted).
    pub fn flags(&self, j: &Job) -> Vec<sign::Flag> {
        if self.trusted(j) { Vec::new() } else { j.flags() }
    }
    pub fn suggested(&self) -> Vec<Job> {
        self.jobs.iter().filter(|j| self.is_suggested(j)).cloned().collect()
    }
    /// Jobs with something that looks wrong; `strong`: only red flags.
    pub fn flagged(&self, strong: bool) -> Vec<Job> {
        self.jobs.iter().filter(|j| self.flags(j).iter().any(|f| !strong || f.strong())).cloned().collect()
    }
    pub fn total(&self) -> usize {
        self.jobs.len() + self.apps.len() + self.cron.len() + self.sysext.len()
    }
    pub fn running(&self) -> usize {
        self.jobs.iter().filter(|j| j.pid.is_some()).count()
    }
    /// The last batch while Undo is still offered.
    pub fn undoable(&self) -> Option<usize> {
        self.last.as_ref().filter(|(_, t)| t.elapsed() < UNDO_FOR).map(|(c, _)| c.len())
    }
}

type Shared = Arc<Mutex<State>>;

pub struct Startup {
    st: Shared,
    ctx: egui::Context,
    pub demo: bool,
    user_library: PathBuf,
    /// App icons by path (None: couldn't be read).
    pub icons: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
    pub filter: Filter,
    pub search: String,
    apps_requested: bool,
    /// System Events access, read at most once a second while the page shows.
    pub events: (crate::kit::perms::Status, Option<std::time::Instant>),
    /// AI Check beside the list.
    pub ai: crate::kit::aicheck::Check,
}

impl Startup {
    pub fn new(env: &ToolEnv) -> Self {
        let user_library = crate::kit::fs::home().join("Library");
        let st: Shared = Arc::new(Mutex::new(State::default()));
        let me = Startup { st: st.clone(), ctx: env.ctx.clone(), demo: env.demo, user_library: user_library.clone(), filter: Filter::All, search: String::new(), apps_requested: false, icons: Default::default(), events: (crate::kit::perms::Status::NotAsked, None), ai: crate::kit::aicheck::Check::new(ID, env.demo) };
        if env.demo {
            demo(&mut st.lock_ok(), false);
            return me;
        }
        st.lock_ok().settings = Settings::load();
        {
            let (st, ctx) = (st.clone(), env.ctx.clone());
            crate::kit::notify::on_action(NOTE, move |payload| {
                st.lock_ok().pending.push(payload);
                ctx.request_repaint();
            });
        }
        let ctx = env.ctx.clone();
        std::thread::Builder::new()
            .name("startup-watch".into())
            .spawn(move || {
                crate::kit::activity::low_priority();
                watch(st, user_library, ctx)
            })
            .ok();
        me
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock_ok()
    }

    /// Whether System Events lets us read the login apps (never asks).
    pub fn events_allowed(&mut self) -> bool {
        use crate::kit::perms::{Perm, Status};
        if self.demo {
            return true;
        }
        if self.events.1.is_none_or(|t| t.elapsed().as_secs_f32() > 1.0) {
            self.events = (Perm::SystemEvents.status(), Some(std::time::Instant::now()));
        }
        self.events.0 == Status::Allowed
    }

    /// Read the login apps once System Events access is there (setup or the
    /// page's Allow button asks for it; opening the page never does).
    pub fn load_apps(&mut self) {
        if self.apps_requested || self.demo || !self.events_allowed() {
            return;
        }
        self.apps_requested = true;
        let (st, ctx) = (self.st.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("startup-login".into())
            .spawn(move || {
                let apps = items::login_apps();
                let mut g = st.lock_ok();
                match apps {
                    Ok(mut a) => {
                        for (name, path, hidden) in &g.settings.login_off {
                            if !a.iter().any(|x| x.path == *path) {
                                a.push(LoginApp { name: name.clone(), path: path.clone(), hidden: *hidden, on: false });
                            }
                        }
                        g.apps = a;
                        g.apps_error = None;
                    }
                    Err(e) => g.apps_error = Some(e),
                }
                g.apps_loaded = true;
                ctx.request_repaint();
            })
            .ok();
    }

    /// Read the login apps again (after an Allow click, or an error).
    pub fn retry_apps(&mut self) {
        self.apps_requested = false;
        self.events.1 = None;
        let mut g = self.state();
        g.apps_error = None;
        g.apps_loaded = false;
    }

    /// Run `f` off the main thread (it may wait for a password), then read the jobs again.
    fn work(&self, line: String, f: impl FnOnce(&Shared) -> Result<(), String> + Send + 'static) {
        if self.state().busy.is_some() {
            return;
        }
        if self.demo {
            let _ = f(&self.st);
            return;
        }
        self.state().busy = Some(line);
        let (st, ctx, lib) = (self.st.clone(), self.ctx.clone(), self.user_library.clone());
        std::thread::Builder::new()
            .name("startup-switch".into())
            .spawn(move || {
                let res = f(&st);
                let mut raw = items::read_raw(&lib);
                items::sign_jobs(&mut raw);
                let jobs = items::shown(raw);
                let mut g = st.lock_ok();
                g.jobs = jobs;
                g.busy = None;
                if let Err(e) = res {
                    if e != "Cancelled" {
                        crate::kit::notify::send("Couldn't change startup items", &e);
                    }
                }
                ctx.request_repaint();
            })
            .ok();
    }

    pub fn toggle_job(&self, j: &Job) {
        let (j, want) = (j.clone(), !j.on);
        let demo = self.demo;
        self.work(
            format!("Turning {} {}…", if want { "on" } else { "off" }, j.name),
            move |st| {
                if !demo {
                    items::set_jobs(&[(j.clone(), want)])?;
                }
                if let Some(x) = st.lock_ok().jobs.iter_mut().find(|x| x.key() == j.key()) {
                    x.on = want;
                    if !want {
                        x.pid = None;
                    }
                }
                Ok(())
            },
        );
    }

    pub fn toggle_app(&self, a: &LoginApp) {
        let (a, want) = (a.clone(), !a.on);
        let demo = self.demo;
        self.work(
            format!("Turning {} {}…", if want { "on" } else { "off" }, a.name),
            move |st| {
                if !demo {
                    items::set_login_app(&a, want)?;
                }
                let mut g = st.lock_ok();
                if let Some(x) = g.apps.iter_mut().find(|x| x.path == a.path) {
                    x.on = want;
                }
                g.settings.login_off.retain(|(_, p, _)| *p != a.path);
                if !want {
                    g.settings.login_off.push((a.name.clone(), a.path.clone(), a.hidden));
                }
                if !demo {
                    g.settings.save();
                }
                Ok(())
            },
        );
    }

    /// Turn off every suggested job, with one password prompt for the admin ones.
    pub fn turn_off_suggested(&self) {
        let list = self.state().suggested();
        if list.is_empty() {
            return;
        }
        let demo = self.demo;
        self.work(
            format!("Turning off {}…", plural(list.len(), "item")),
            move |st| {
                if !demo {
                    let batch: Vec<(Job, bool)> = list.iter().map(|j| (j.clone(), false)).collect();
                    items::set_jobs(&batch)?;
                }
                let mut g = st.lock_ok();
                for j in &list {
                    if let Some(x) = g.jobs.iter_mut().find(|x| x.key() == j.key()) {
                        x.on = false;
                        x.pid = None;
                    }
                }
                g.last = Some((list.into_iter().map(Change::Job).collect(), Instant::now()));
                Ok(())
            },
        );
    }

    /// Put the last batch back as it was.
    pub fn undo(&self) {
        let Some((changes, _)) = self.state().last.clone() else { return };
        let demo = self.demo;
        self.work(
            "Turning them back on…".into(),
            move |st| {
                let jobs: Vec<(Job, bool)> = changes.iter().map(|Change::Job(j)| (j.clone(), j.on)).collect();
                if !demo {
                    items::set_jobs(&jobs)?;
                }
                let mut g = st.lock_ok();
                for (j, on) in &jobs {
                    if let Some(x) = g.jobs.iter_mut().find(|x| x.key() == j.key()) {
                        x.on = *on;
                        x.pid = j.pid;
                    }
                }
                g.last = None;
                Ok(())
            },
        );
    }

    /// Stop a job and move its file to the Trash.
    pub fn trash(&self, j: &Job) {
        if j.bundled {
            return;
        }
        let j = j.clone();
        let demo = self.demo;
        self.work(
            format!("Moving {} to the Trash…", j.name),
            move |st| {
                if !demo {
                    items::trash_job(&j)?;
                }
                let mut g = st.lock_ok();
                g.trashed.insert(j.plist.clone());
                g.jobs.retain(|x| x.key() != j.key());
                Ok(())
            },
        );
    }

    pub fn toggle_cron(&self, c: &CronJob) {
        let (c, want) = (c.clone(), !c.on);
        let demo = self.demo;
        self.work(
            format!("Turning {} a cron job…", if want { "on" } else { "off" }),
            move |st| {
                if !demo {
                    extra::set_cron(&c, want)?;
                }
                let fresh = if demo { None } else { Some(extra::cron_jobs()) };
                let mut g = st.lock_ok();
                match fresh {
                    Some(list) => g.cron = list,
                    None => {
                        if let Some(x) = g.cron.iter_mut().find(|x| x.line == c.line) {
                            x.on = want;
                        }
                    }
                }
                // our own switch isn't a new line
                let lines: Vec<String> = g.cron.iter().map(|x| x.line.clone()).collect();
                g.settings.cron_seen.extend(lines);
                if !demo {
                    g.settings.save();
                }
                Ok(())
            },
        );
    }

    /// Trust a job (no suggestion, no flags) until what it runs changes; or stop.
    pub fn set_trust(&self, j: &Job, on: bool) {
        let mut g = self.state();
        if on {
            g.settings.trusted.insert(trust_key(j), j.fp.clone());
        } else {
            g.settings.trusted.remove(&trust_key(j));
        }
        g.new_labels.remove(&j.label);
        g.changed_labels.remove(&j.label);
        if !self.demo {
            g.settings.save();
        }
    }

    /// Look the job's program up on VirusTotal by its hash (only the hash leaves the Mac).
    pub fn virustotal(&self, j: &Job) {
        let Some(file) = j.vt_file() else { return };
        if self.demo {
            return;
        }
        std::thread::Builder::new()
            .name("startup-vt".into())
            .spawn(move || match sign::sha256(&file) {
                Some(h) => crate::kit::macos::open(&format!("https://www.virustotal.com/gui/file/{h}")),
                None => crate::kit::notify::send("Couldn't read the file", &file.display().to_string()),
            })
            .ok();
    }

    /// Read the full Background Items list (asks for the admin password).
    pub fn load_btm(&self) {
        {
            let mut g = self.state();
            if g.btm_busy {
                return;
            }
            if self.demo {
                g.btm = Some(demo_btm());
                return;
            }
            g.btm_busy = true;
        }
        let (st, ctx) = (self.st.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("startup-btm".into())
            .spawn(move || {
                let res = extra::btm_items();
                let mut g = st.lock_ok();
                g.btm_busy = false;
                match res {
                    Ok(list) => g.btm = Some(list),
                    Err(e) if e == "Cancelled" || e.contains("-128") => {}
                    Err(e) => crate::kit::notify::send("Couldn't read background items", &e),
                }
                ctx.request_repaint();
            })
            .ok();
    }

    /// Turn off what a notification's button asked for (once nothing else runs).
    fn take_pending(&mut self) {
        let next = {
            let mut g = self.state();
            if g.busy.is_some() || g.pending.is_empty() {
                return;
            }
            g.pending.remove(0)
        };
        let g = self.state();
        if let Some(line) = next.strip_prefix("cron|") {
            let c = g.cron.iter().find(|c| c.line == line && c.on).cloned();
            drop(g);
            if let Some(c) = c {
                self.toggle_cron(&c);
            }
        } else if let Some(rest) = next.strip_prefix("job|") {
            let j = g.jobs.iter().find(|j| j.on && format!("{:?}|{}", j.scope, j.label) == rest).cloned();
            drop(g);
            if let Some(j) = j {
                self.toggle_job(&j);
            }
        }
    }

    pub fn set_notify(&self, on: bool) {
        let mut g = self.state();
        g.settings.notify_new = on;
        if !self.demo {
            g.settings.save();
        }
    }
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// What changed between two looks at the job plists.
#[derive(Debug, Default, PartialEq)]
pub struct Diff {
    pub new: Vec<PathBuf>,
    pub changed: Vec<PathBuf>,
    pub gone: Vec<Seen>,
}

/// Compare the jobs now with what was seen before (bundled jobs live in
/// their app, not in a folder: they're left out).
pub fn diff(seen: &BTreeMap<PathBuf, Seen>, jobs: &[Job]) -> (Diff, BTreeMap<PathBuf, Seen>) {
    let now: BTreeMap<PathBuf, Seen> = jobs.iter().filter(|j| !j.bundled).map(|j| (j.plist.clone(), Seen { fp: j.fp.clone(), name: j.name.clone(), label: j.label.clone() })).collect();
    let mut d = Diff::default();
    for (p, s) in &now {
        match seen.get(p) {
            None => d.new.push(p.clone()),
            // an empty fingerprint is from before they were kept: not a change
            Some(old) if !old.fp.is_empty() && old.fp != s.fp => d.changed.push(p.clone()),
            _ => {}
        }
    }
    for (p, old) in seen {
        if !now.contains_key(p) && !old.label.is_empty() {
            d.gone.push(old.clone());
        }
    }
    (d, now)
}

/// The notification for a job that's new or now runs something else.
fn announce(j: &Job, changed: bool) {
    let app = j.owner_name().unwrap_or_else(|| j.name.clone());
    let title = if changed { format!("{app} changed what starts with your Mac") } else { format!("{app} added itself to startup") };
    let body = if changed { format!("{} now runs {}", j.label, j.command()) } else { format!("{} · runs {}", j.label, j.when.label(j.scope)) };
    crate::kit::notify::send_with(&title, &body, "Turn off", NOTE, &format!("job|{:?}|{}", j.scope, j.label));
}

/// Read what starts now and then: jobs (signatures read once per program),
/// cron jobs and system extensions; announce what's new or changed.
fn watch(st: Shared, lib: PathBuf, ctx: egui::Context) {
    static CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();
    let folders: Vec<PathBuf> = items::dirs(&lib, std::path::Path::new("/")).into_iter().map(|(d, _)| d).collect();
    let hooked = crate::kit::fswatch::watch(&folders, 1.0, &CHANGED);
    loop {
        let mut raw = items::read_raw(&lib);
        if items::unsigned_yet(&raw) {
            // show the list now; signatures follow
            let mut g = st.lock_ok();
            if g.busy.is_none() {
                g.jobs = items::shown(raw.clone());
            }
            g.jobs_loaded = true;
            drop(g);
            ctx.request_repaint();
            items::sign_jobs(&mut raw);
        }
        let jobs = items::shown(raw);
        let cron = extra::cron_jobs();
        let sysext = extra::sysexts();
        let mut news: Vec<(Job, bool)> = Vec::new();
        let mut new_cron: Vec<CronJob> = Vec::new();
        {
            let mut g = st.lock_ok();
            let (d, now) = diff(&g.settings.seen, &jobs);
            let mut dirty = now != g.settings.seen;
            if g.settings.seeded {
                for (paths, changed) in [(&d.new, false), (&d.changed, true)] {
                    for p in paths {
                        let Some(j) = jobs.iter().find(|j| j.plist == *p) else { continue };
                        if changed {
                            g.changed_labels.insert(j.label.clone());
                        } else {
                            g.new_labels.insert(j.label.clone());
                        }
                        if g.settings.notify_new && j.on && !g.trusted(j) {
                            news.push((j.clone(), changed));
                        }
                    }
                }
                let at = now_secs();
                for s in d.gone {
                    let ours = g.settings.seen.iter().any(|(p, x)| *x == s && g.trashed.contains(p));
                    if !ours {
                        g.settings.gone.retain(|x| x.label != s.label);
                        g.settings.gone.push(Gone { name: s.name, label: s.label, at });
                    }
                }
            }
            let before = g.settings.gone.len();
            let cut = now_secs().saturating_sub(GONE_FOR);
            g.settings.gone.retain(|x| x.at >= cut);
            let over = g.settings.gone.len().saturating_sub(20);
            g.settings.gone.drain(..over);
            dirty |= g.settings.gone.len() != before;
            g.settings.seen = now;
            g.settings.seeded = true;
            // cron lines
            let lines: BTreeSet<String> = cron.iter().map(|c| c.line.clone()).collect();
            if g.settings.cron_seeded {
                let fresh: Vec<&CronJob> = cron.iter().filter(|c| !g.settings.cron_seen.contains(&c.line)).collect();
                for c in fresh {
                    g.new_cron.insert(c.line.clone());
                    if g.settings.notify_new && c.on {
                        new_cron.push(c.clone());
                    }
                }
            }
            dirty |= lines != g.settings.cron_seen || !g.settings.cron_seeded;
            g.settings.cron_seen = lines;
            g.settings.cron_seeded = true;
            if dirty {
                g.settings.save();
            }
            if g.busy.is_none() {
                g.jobs = jobs;
                g.cron = cron;
            }
            g.sysext = sysext;
            g.jobs_loaded = true;
        }
        for (j, changed) in news {
            announce(&j, changed);
        }
        for c in new_cron {
            crate::kit::notify::send_with("A cron job was added", &format!("Runs {} · {}", c.when, c.command), "Turn off", NOTE, &format!("cron|{}", c.line));
        }
        ctx.request_repaint();
        // a new or removed plist comes as a folder event; what's running
        // only matters on screen, so hidden it's read rarely
        let wait = if !hooked || crate::kit::activity::visible() { POLL } else { HIDDEN_POLL };
        crate::kit::activity::nap(wait, Some(&CHANGED));
    }
}

/// Sample data for screenshots. `done`: right after "Turn off 4".
pub fn demo(g: &mut State, done: bool) {
    let job = |label: &str, name: &str, prog: &str, scope: Scope, when: When, on: bool, pid: Option<u32>, orphan: bool| {
        let program = Some(PathBuf::from(prog));
        let owner = items::owner_app(std::path::Path::new(prog));
        let dir = match scope {
            Scope::User => "/Users/you/Library/LaunchAgents",
            Scope::AllUsers => "/Library/LaunchAgents",
            Scope::System => "/Library/LaunchDaemons",
        };
        let args = vec![prog.to_string()];
        let fp = items::fingerprint(program.as_deref(), &args);
        let sig = Some(if orphan { sign::Signer::Missing } else { sign::Signer::DevId { name: name.into(), team: "A1B2C3D4E5".into(), notarized: true } });
        Job { label: label.into(), plist: PathBuf::from(format!("{dir}/{label}.plist")), program, args, sig, fp, scope, on, when, pid, orphan, owner, name: name.into(), bundled: false }
    };
    // pretends to be Apple's, unsigned, from a hidden folder anyone can write to
    let mut fake = job("com.apple.audio.helperd", "Helperd", "/Users/Shared/.cache/helperd", Scope::User, When::KeepsRunning, true, Some(1377), false);
    fake.sig = Some(sign::Signer::Unsigned);
    g.jobs = vec![
        job("com.docker.helper", "Docker", "/Applications/Docker.app/Contents/Library/LaunchServices/com.docker.helper", Scope::User, When::KeepsRunning, true, Some(812), false),
        job("com.google.GoogleUpdater.wake", "Google Updater", "/Users/you/Library/Application Support/Google/GoogleUpdater/Current/GoogleUpdater.app/Contents/MacOS/GoogleUpdater", Scope::User, When::Every(3600), true, None, false),
        job("com.valvesoftware.steamclean", "Steam Cleaner", "/Users/you/Library/Application Support/Steam/Steam.AppBundle/Steam/Contents/MacOS/steamclean", Scope::User, When::AtLogin, true, None, false),
        job("com.teamviewer.teamviewer_desktop", "TeamViewer", "/Applications/TeamViewer.app/Contents/Helpers/TeamViewer_Desktop", Scope::User, When::KeepsRunning, true, None, true),
        // a long name and id, to see they get room before they get cut
        job("com.asus.display.DisplayWidgetCenter", "DisplayWidgetCenter", "/Applications/DisplayWidget Center.app/Contents/MacOS/DisplayWidget Center", Scope::User, When::AtLogin, true, None, false),
        job("com.1password.SafariShim", "1Password", "/Applications/1Password.app/Contents/Library/LaunchAgents/SafariShim", Scope::AllUsers, When::OnDemand, true, Some(904), false),
        job("com.microsoft.autoupdate.helper", "Microsoft AutoUpdate", "/Library/Application Support/Microsoft/MAU2.0/Microsoft AutoUpdate.app/Contents/MacOS/Microsoft AutoUpdate", Scope::System, When::OnDemand, true, None, false),
        job("com.docker.vmnetd", "Docker", "/Library/PrivilegedHelperTools/com.docker.vmnetd", Scope::System, When::KeepsRunning, true, Some(233), false),
        fake,
    ];
    g.cron = extra::parse_crontab("*/30 * * * * /usr/local/bin/backup-notes.sh\n#cmdmeow-off# @reboot /Users/you/bin/mount-nas\n");
    g.sysext = extra::parse_sysext("--- com.apple.system_extension.network_extension\n*\t*\tVBG97UB4TA\tcom.objective-see.lulu.extension (2.4.1/2.4.1)\tLuLu\t[activated enabled]\n");
    g.settings.gone = vec![Gone { name: "Zoom".into(), label: "us.zoom.ZoomDaemon".into(), at: now_secs() - 3600 }];
    g.apps = vec![
        LoginApp { name: "Slack".into(), path: "/Applications/Slack.app".into(), hidden: true, on: true },
        LoginApp { name: "Spotify".into(), path: "/Applications/Spotify.app".into(), hidden: false, on: false },
    ];
    g.jobs_loaded = true;
    g.apps_loaded = true;
    g.new_labels = ["com.1password.SafariShim".to_string()].into();
    if done {
        let list = g.suggested();
        for j in &list {
            if let Some(x) = g.jobs.iter_mut().find(|x| x.key() == j.key()) {
                x.on = false;
                x.pid = None;
            }
        }
        g.last = Some((list.into_iter().map(Change::Job).collect(), Instant::now()));
    }
}

fn demo_btm() -> Vec<BtmItem> {
    vec![BtmItem { uid: 501, name: "Raycast".into(), developer: "Raycast Technologies".into(), team: "SY64MV22J9".into(), kind: "app".into(), on: true, identifier: "2.com.raycast.macos".into(), bundle_id: "com.raycast.macos".into(), path: Some("/Applications/Raycast.app".into()), program: None }]
}

fn badge() -> Badge {
    Badge { icon: Icon::Power, color: tint::STARTUP() }
}

impl Tool for Startup {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Startup"
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::may(Perm::SystemEvents), Need::may(Perm::Notifications)]
    }

    fn section(&self) -> &'static str {
        "Clean"
    }
    fn about(&self) -> &'static str {
        "Login items and background apps that start with your Mac, and a switch for each"
    }
    fn busy(&self) -> bool {
        self.ai.phase() == crate::kit::aicheck::Phase::Running
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: SUMMARY, title: "Startup", about: "What starts with your Mac, and one button for what can go.", sizes: &[Size::S11, Size::S12], size: Size::S12, default_on: true }]
    }
    fn widget_loading(&self, _widget: &str) -> bool {
        !self.state().jobs_loaded
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::widget(self, size, ui, rect, cx);
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        self.filter = match route {
            "suggested" => Filter::Suggested,
            "flagged" => Filter::Flagged,
            "off" => Filter::Off,
            _ => Filter::All,
        };
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::page(self, ui, rect, cx);
    }
    fn recommendations(&self) -> Vec<Recommendation> {
        let mut out = Vec::new();
        let odd = self.state().flagged(true);
        if !odd.is_empty() {
            let names: Vec<String> = odd.iter().take(2).map(|j| j.name.clone()).collect();
            out.push(Recommendation {
                priority: 9,
                title: format!("{} look suspicious", plural(odd.len(), "startup item")),
                body: names.join(", "),
                action: "Review".into(),
                go: Action::OpenPage { tool: ID.into(), route: "flagged".into() },
            });
        }
        let list = self.state().suggested();
        if list.is_empty() {
            return out;
        }
        let names: Vec<String> = list.iter().take(2).map(|j| j.name.clone()).collect();
        let more = if list.len() > 2 { format!(" and {} more", list.len() - 2) } else { String::new() };
        out.push(Recommendation {
            priority: 8,
            title: format!("{} start with your Mac for nothing", plural(list.len(), "item")),
            body: format!("{}{more}. Turning them off removes nothing.", names.join(", ")),
            action: "Review".into(),
            go: Action::OpenPage { tool: ID.into(), route: "suggested".into() },
        });
        out
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        self.take_pending();
        let g = self.state();
        if g.busy.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else if g.undoable().is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        false
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "startup" | "startup-done" | "startup-suggested" | "startup-flagged" => {
                let mut g = self.state();
                *g = State::default();
                demo(&mut g, view == "startup-done");
                drop(g);
                self.filter = match view {
                    "startup-suggested" => Filter::Suggested,
                    "startup-flagged" => Filter::Flagged,
                    _ => Filter::All,
                };
                use crate::kit::aicheck::Check;
                self.ai.set_demo(
                    "Mostly lean; one updater you don't need",
                    vec![Check::demo_rec("off", "Turn off", "Stop Ollama updater at login", "", "Only checks for updates; Ollama still updates when you open it", "Ollama")],
                );
                Some(View::Page)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::Why;

    #[test]
    fn demo_suggests_the_right_ones() {
        let mut g = State::default();
        demo(&mut g, false);
        let s: Vec<String> = g.suggested().iter().map(|j| j.name.clone()).collect();
        assert_eq!(s, vec!["Google Updater", "Steam Cleaner", "TeamViewer", "Microsoft AutoUpdate"]);
        assert_eq!(g.jobs.iter().find(|j| j.name == "TeamViewer").unwrap().why(), Some(Why::Removed));
        let mut d = State::default();
        demo(&mut d, true);
        assert!(d.suggested().is_empty());
        assert_eq!(d.undoable(), Some(4));
        assert_eq!(plural(1, "item"), "1 item");
    }

    #[test]
    fn demo_flags_the_fake_one_and_trust_clears_it() {
        let mut g = State::default();
        demo(&mut g, false);
        let odd: Vec<String> = g.flagged(true).iter().map(|j| j.name.clone()).collect();
        assert_eq!(odd, vec!["Helperd"]);
        let fake = g.jobs.iter().find(|j| j.name == "Helperd").unwrap().clone();
        assert_eq!(fake.flags(), vec![sign::Flag::FakeApple, sign::Flag::Unsigned, sign::Flag::TempFolder, sign::Flag::HiddenFolder]);
        g.settings.trusted.insert(trust_key(&fake), fake.fp.clone());
        assert!(g.flagged(false).is_empty());
        // trust holds only while it runs the same thing
        let x = g.jobs.iter_mut().find(|j| j.name == "Helperd").unwrap();
        x.fp = "other".into();
        assert_eq!(g.flagged(true).len(), 1);
        // a trusted updater is no longer suggested
        let up = g.jobs.iter().find(|j| j.name == "Google Updater").unwrap().clone();
        g.settings.trusted.insert(trust_key(&up), up.fp.clone());
        assert!(!g.suggested().iter().any(|j| j.name == "Google Updater"));
    }

    #[test]
    fn tells_new_changed_and_removed_jobs() {
        let j = |plist: &str, label: &str, fp: &str| Job { plist: plist.into(), label: label.into(), name: label.to_uppercase(), fp: fp.into(), ..Default::default() };
        let mut seen = BTreeMap::new();
        seen.insert(PathBuf::from("/a.plist"), Seen { fp: "1".into(), name: "A".into(), label: "a".into() });
        seen.insert(PathBuf::from("/b.plist"), Seen { fp: "2".into(), name: "B".into(), label: "b".into() });
        seen.insert(PathBuf::from("/old.plist"), Seen::default());
        let mut bundled = j("/App.app/x.plist", "x", "9");
        bundled.bundled = true;
        let now = vec![j("/a.plist", "a", "1"), j("/b.plist", "b", "3"), j("/c.plist", "c", "4"), j("/old.plist", "old", "5"), bundled];
        let (d, next) = diff(&seen, &now);
        assert_eq!(d.new, vec![PathBuf::from("/c.plist")]);
        assert_eq!(d.changed, vec![PathBuf::from("/b.plist")]);
        assert!(d.gone.is_empty());
        assert_eq!(next.len(), 4);
        assert_eq!(next[&PathBuf::from("/old.plist")].fp, "5");
        let (d, _) = diff(&next, &now[..1]);
        let gone: Vec<&str> = d.gone.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(gone, vec!["b", "c", "old"]);
    }

    /// On a real Mac: `cargo test reads_this_mac -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_this_mac() {
        let lib = crate::kit::fs::home().join("Library");
        let mut raw = items::read_raw(&lib);
        let t = Instant::now();
        items::sign_jobs(&mut raw);
        println!("signed {} jobs in {:?}", raw.len(), t.elapsed());
        let hidden = raw.iter().filter(|j| j.apple()).count();
        let jobs = items::shown(raw);
        println!("{} shown, {hidden} Apple's own hidden", jobs.len());
        for j in &jobs {
            println!("{:<44} {:<9} {:?} {:?}", j.label, format!("{:?}", j.scope), j.sig.as_ref().map(|s| s.line()), j.flags());
        }
        for c in extra::cron_jobs() {
            println!("cron: {} | {} | on={} {:?}", c.when, c.command, c.on, c.flags());
        }
        for x in extra::sysexts() {
            println!("sysext: {} {} {} on={} active={} [{}]", x.name, x.bundle_id, x.kind, x.on, x.active, x.state);
        }
    }

    #[test]
    fn old_settings_move_to_seen() {
        let mut s: Settings = serde_json::from_str(r#"{"known":["/x.plist"],"seeded":true}"#).unwrap();
        s.migrate();
        assert!(s.known.is_empty() && s.seen[&PathBuf::from("/x.plist")] == Seen::default());
        assert!(!serde_json::to_string(&s).unwrap().contains("known"));
    }
}
