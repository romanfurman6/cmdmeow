//! Uninstaller: removes apps together with the files they leave in
//! ~/Library, cleans up after apps that are already gone, resets an app to a
//! fresh start, and turns off what starts with the Mac. Everything goes to the
//! Trash and Undo puts it back.
//!
//! Drop an app on its page or its widget, or just start dragging an app out of
//! /Applications: a small drop zone appears at the top of the screen.
//!
//! Page routes: "" (Apps), "leftovers", "app:<path>" (select an app).

use crate::kit::sync::LockExt;
pub(crate) mod apps;
mod extra;
mod leftovers;
mod mac;
mod remove;
mod rules;
mod ui;

use crate::core::tool::{Badge, Cx, Tool, ToolEnv, ToolId, View};
use crate::kit::fs::dir_size;
use crate::kit::glass::Icon;
use apps::Source;
use egui::{Color32, Rect, Ui};
use leftovers::{Index, Item, Kind, Owner, Roots};
use remove::{Report, Target};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ID: ToolId = "uninstaller";
#[allow(non_snake_case)]
pub fn TINT() -> Color32 { crate::kit::glass::tint::CLEANUP() }

/// Apps unused this long count as unused.
pub const UNUSED_DAYS: i64 = 90;
/// "Large" filter.
pub const LARGE: u64 = 1_000_000_000;
/// Leftovers touched more recently than this are still in use.
const ORPHAN_MIN_AGE: i64 = 7 * 86400;
/// How often the app folders are checked for apps that came or went.
const WATCH_EVERY: Duration = Duration::from_secs(8);
/// With folder events on, the look that catches anything they missed.
const WATCH_FALLBACK: Duration = Duration::from_secs(300);
/// How long Undo stays offered.
pub const UNDO_FOR: Duration = Duration::from_secs(60);

/// An installed app and everything found for it.
#[derive(Clone, Debug)]
pub struct AppRow {
    pub path: PathBuf,
    pub name: String,
    pub id: String,
    pub version: String,
    pub source: Source,
    pub locked: Option<&'static str>,
    pub last_used: Option<i64>,
    /// The bundle first, then its files.
    pub items: Vec<Item>,
    /// Sizes are known.
    pub sized: bool,
    /// Other names it goes by.
    pub alt: Vec<String>,
    /// Its signature: team and app groups (read with the files).
    pub sign: extra::Sign,
    /// Installer packages that put it in place.
    pub pkgs: Vec<String>,
}

impl AppRow {
    /// A copy for drawing a list row: everything but the files.
    pub fn without_items(&self) -> AppRow {
        AppRow {
            path: self.path.clone(),
            name: self.name.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
            source: self.source,
            locked: self.locked,
            last_used: self.last_used,
            items: Vec::new(),
            sized: self.sized,
            alt: self.alt.clone(),
            sign: self.sign.clone(),
            pkgs: self.pkgs.clone(),
        }
    }

    pub fn total(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }
    pub fn app_bytes(&self) -> u64 {
        self.items.iter().filter(|i| i.kind == Kind::App).map(|i| i.bytes).sum()
    }
    pub fn owner(&self) -> Owner {
        Owner { id: self.id.clone(), name: self.name.clone(), path: self.path.clone(), alt: self.alt.clone(), groups: self.sign.groups.clone(), team: self.sign.team.clone() }
    }
    pub fn target(&self, reset: bool) -> Target {
        Target { name: self.name.clone(), id: self.id.clone(), app: Some(self.path.clone()), items: self.items.clone(), reset, pkgs: self.pkgs.clone() }
    }
}

/// Data of an app that's gone.
#[derive(Clone, Debug)]
pub struct Orphan {
    pub id: String,
    pub name: String,
    pub items: Vec<Item>,
    /// Newest change among its items.
    pub touched: Option<i64>,
    pub selected: bool,
}

impl Orphan {
    pub fn bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }
    pub fn target(&self) -> Target {
        Target { name: self.name.clone(), id: self.id.clone(), app: None, items: self.items.clone(), reset: false, pkgs: Vec::new() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// Reading the app folders.
    Listing,
    /// Apps are listed; sizes and files come in.
    Sizing { done: usize, total: usize },
    Ready,
}

/// An app that left /Applications while Cmd+Meow watched, with what it
/// left behind.
#[derive(Clone, Debug)]
pub struct Notice {
    pub target: Target,
    pub at: Instant,
}

/// What the drop zone and the widget show.
#[derive(Clone, Debug, Default)]
pub enum Drop {
    #[default]
    Idle,
    /// Finding the files of a dropped app.
    Reading(PathBuf),
    Confirm(Target),
    /// It can't be removed (with why).
    Refused(String, String),
}

/// What's dragged on screen right now, seen through the drag pasteboard.
#[derive(Clone, Debug, Default)]
pub struct Drag {
    /// Apps being dragged while the mouse is down.
    pub apps: Vec<PathBuf>,
    /// Keep showing the drop zone until then (drop can land just after release).
    pub until: Option<Instant>,
}

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    pub apps: Vec<AppRow>,
    pub orphans: Vec<Orphan>,
    /// A removal is running: its progress line.
    pub working: Option<String>,
    /// The last removal, for Undo and the summary.
    pub last: Option<(Report, Instant)>,
    pub notice: Option<Notice>,
    pub drop: Drop,
    pub drag: Drag,
    pub settings: Settings,
    /// Rescan when the current scan ends.
    pub again: bool,
    /// Bundle ids we removed ourselves (the watcher stays quiet about those).
    pub removed_by_us: HashSet<String>,
    /// The tool is off in Settings: the background watchers idle.
    pub off: bool,
    /// Scan for apps was clicked. Until then the page shows its start screen
    /// and scans only list the apps (enough to notice one being thrown away),
    /// leaving measuring them for later.
    pub page_seen: bool,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Offer to remove what an app left when it is thrown away elsewhere.
    pub watch_removed: bool,
    /// Show the drop zone while an app is dragged.
    pub drop_zone: bool,
    /// Names of apps seen installed, by bundle id (names for leftovers).
    pub known: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { watch_removed: true, drop_zone: true, known: BTreeMap::new() }
    }
}

fn settings_path() -> PathBuf {
    crate::kit::fs::data_path("uninstaller").join("settings.json")
}

impl Settings {
    fn load() -> Settings {
        crate::kit::fs::load_json(&settings_path())
    }
    pub fn save(&self) {
        crate::kit::fs::save_json(&settings_path(), self, true);
    }
}

pub type Shared = Arc<Mutex<State>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Apps,
    Leftovers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Unused,
    Large,
    Source(Source),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Size,
    Name,
    LastUsed,
}

/// The confirm sheet before anything moves.
pub struct Review {
    pub targets: Vec<Target>,
    pub open: HashSet<usize>,
}

pub struct Uninstaller {
    pub st: Shared,
    pub roots: Roots,
    pub app_roots: Vec<PathBuf>,
    pub demo: bool,
    ctx: egui::Context,
    pub tab: Tab,
    pub search: String,
    pub filter: Filter,
    pub sort: Sort,
    pub selected: Option<PathBuf>,
    /// Apps ticked for a batch.
    pub checked: Vec<PathBuf>,
    pub review: Option<Review>,
    pub icons: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Demo clock (screenshots).
    pub demo_now: Option<i64>,
    /// Where the current drop/confirm came from, so only that place shows it.
    pub origin: Origin,
    /// Bundle ids of open apps, refreshed at most every 2 s.
    pub running: Option<(Vec<String>, Instant)>,
    /// The widget asked for the window page.
    pub want_page: bool,
    /// The drop zone keeps showing the result until then.
    pub done_until: Option<Instant>,
    on: bool,
    drag_watch: Option<DragWatch>,
    /// AI Check beside the leftovers.
    pub ai: crate::kit::aicheck::Check,
}

/// Which surface started a removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Origin {
    Zone,
    #[default]
    Page,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl Uninstaller {
    pub fn new(env: &ToolEnv) -> Self {
        let mut u = Uninstaller {
            st: Arc::new(Mutex::new(State::default())),
            roots: Roots::real(),
            app_roots: apps::app_roots(),
            demo: env.demo,
            ctx: env.ctx.clone(),
            tab: Tab::Apps,
            search: String::new(),
            filter: Filter::All,
            sort: Sort::Name,
            selected: None,
            checked: Vec::new(),
            review: None,
            icons: HashMap::new(),
            demo_now: None,
            origin: Origin::Page,
            running: None,
            want_page: false,
            done_until: None,
            on: true,
            ai: crate::kit::aicheck::Check::new(ID, env.demo),
            drag_watch: None,
        };
        if env.demo {
            ui::demo::fill(&mut u, "uninstaller");
            return u;
        }
        u.st.lock_ok().settings = Settings::load();
        watch(u.st.clone(), u.app_roots.clone(), u.roots.clone(), env.ctx.clone());
        u.drag_watch = watch_drag(u.st.clone(), u.app_roots.clone(), env.ctx.clone());
        u.sync_drag_hook();
        u
    }

    /// Have the global mouse monitor only while it is needed: the tool on
    /// and the drop zone wanted. Main thread (set_on, tick).
    pub fn sync_drag_hook(&mut self) {
        let want = self.on && self.st.lock_ok().settings.drop_zone;
        let Some(w) = self.drag_watch.as_mut() else { return };
        if want == w.hook.is_some() {
            return;
        }
        w.hook = if want {
            let tx = w.tx.clone();
            mac::on_mouse_drag(move || {
                let _ = tx.try_send(());
            })
        } else {
            None
        };
        w.hooked.store(w.hook.is_some(), std::sync::atomic::Ordering::Relaxed);
        eprintln!("[uninstaller] drop zone {}", if !want { "off" } else if w.hook.is_some() { "on, mouse monitor set" } else { "on, no mouse monitor (polling)" });
        w.wake();
    }

    pub fn now(&self) -> i64 {
        self.demo_now.unwrap_or_else(now)
    }

    /// Start a full scan (unless one runs; then once more after it).
    pub fn rescan(&self) {
        if self.demo {
            return;
        }
        start_scan(self.st.clone(), self.app_roots.clone(), self.roots.clone(), self.ctx.clone());
    }

    /// Move `targets` to the Trash in the background.
    pub fn run(&self, targets: Vec<Target>) {
        if targets.is_empty() {
            return;
        }
        let mut g = self.st.lock_ok();
        if g.working.is_some() {
            return;
        }
        if self.demo {
            let moved = targets.iter().flat_map(|t| t.items.iter().filter(|i| i.selected)).map(|i| remove::Moved { from: i.path.clone(), to: i.path.clone(), admin: false, bytes: i.bytes, owner: None }).collect();
            g.last = Some((Report { moved, names: targets.iter().map(|t| t.name.clone()).collect(), reset: targets.iter().all(|t| t.reset), apps: targets.iter().filter(|t| !t.reset && t.app.is_some()).map(|t| t.name.clone()).collect(), ..Default::default() }, Instant::now()));
            return;
        }
        g.working = Some("Starting…".into());
        for t in targets.iter().filter(|t| !t.reset) {
            g.removed_by_us.insert(t.id.clone());
        }
        drop(g);
        let (st, roots, app_roots, ctx) = (self.st.clone(), self.roots.clone(), self.app_roots.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("uninstaller-remove".into())
            .spawn(move || {
                let st2 = st.clone();
                let c2 = ctx.clone();
                let rep = remove::run(&targets, &roots, &app_roots, move |line| {
                    st2.lock_ok().working = Some(line);
                    c2.request_repaint();
                });
                crate::kit::sound::play(if rep.failed.is_empty() { crate::kit::sound::Cue::Done } else { crate::kit::sound::Cue::Oops });
                {
                    let mut g = st.lock_ok();
                    // the lists lose what was moved right away (the rescan
                    // below takes a while), so the page matches the summary
                    drop_moved(&mut g, &rep);
                    g.working = None;
                    g.last = Some((rep, Instant::now()));
                    g.drop = Drop::Idle;
                }
                ctx.request_repaint();
                start_scan(st, app_roots, roots, ctx);
            })
            .ok();
    }

    /// Put the last removal back.
    pub fn undo(&self) {
        let Some((rep, _)) = self.st.lock_ok().last.take() else { return };
        if self.demo {
            return;
        }
        let (st, roots, app_roots, ctx) = (self.st.clone(), self.roots.clone(), self.app_roots.clone(), self.ctx.clone());
        st.lock_ok().working = Some("Putting things back…".into());
        std::thread::Builder::new()
            .name("uninstaller-undo".into())
            .spawn(move || {
                let (n, failed) = remove::undo(&rep);
                {
                    let mut g = st.lock_ok();
                    g.working = None;
                    if !failed.is_empty() {
                        let mut r = Report { names: vec![format!("{n} put back")], ..Default::default() };
                        r.failed = failed;
                        g.last = Some((r, Instant::now()));
                    }
                }
                ctx.request_repaint();
                start_scan(st, app_roots, roots, ctx);
            })
            .ok();
    }

    /// An app was dropped (page, widget or drop zone): find its files, then confirm.
    pub fn dropped(&self, path: PathBuf) {
        let path = path.ancestors().find(|a| a.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf).unwrap_or(path);
        let mut g = self.st.lock_ok();
        if !path.extension().is_some_and(|e| e == "app") {
            g.drop = Drop::Refused(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "Not an app. Drop an app from Applications.".into());
            return;
        }
        if let Some(row) = g.apps.iter().find(|a| a.path == path && a.sized) {
            g.drop = match row.locked {
                Some(why) => Drop::Refused(row.name.clone(), format!("{why}, so it can't be removed.")),
                None => Drop::Confirm(row.target(false)),
            };
            return;
        }
        g.drop = Drop::Reading(path.clone());
        drop(g);
        let (st, roots, app_roots, ctx) = (self.st.clone(), self.roots.clone(), self.app_roots.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("uninstaller-drop".into())
            .spawn(move || {
                let others: Vec<Owner> = st.lock_ok().apps.iter().map(|a| a.owner()).collect();
                let res = read_app(&path, &roots, &others, &app_roots);
                let mut g = st.lock_ok();
                if matches!(&g.drop, Drop::Reading(p) if *p == path) {
                    g.drop = match res {
                        Some(row) => match row.locked {
                            Some(why) => Drop::Refused(row.name, format!("{why}, so it can't be removed.")),
                            None => Drop::Confirm(row.target(false)),
                        },
                        None => Drop::Refused(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "Couldn't read this app.".into()),
                    };
                }
                ctx.request_repaint();
            })
            .ok();
    }

    /// Visible apps for the current search, filter and sort.
    pub fn visible(&self) -> Vec<usize> {
        let g = self.st.lock_ok();
        let q = self.search.trim().to_lowercase();
        let now = self.now();
        let mut idx: Vec<usize> = (0..g.apps.len())
            .filter(|&i| {
                let a = &g.apps[i];
                (q.is_empty() || a.name.to_lowercase().contains(&q) || a.id.to_lowercase().contains(&q))
                    && match self.filter {
                        Filter::All => true,
                        Filter::Unused => a.locked.is_none() && a.last_used.is_some_and(|t| now - t > UNUSED_DAYS * 86400),
                        Filter::Large => a.total() >= LARGE,
                        Filter::Source(s) => a.source == s,
                    }
            })
            .collect();
        let apps = &g.apps;
        // keys worked out once per app, not once per comparison: this runs
        // several times every frame
        match self.sort {
            Sort::Size => idx.sort_by_cached_key(|&a| (std::cmp::Reverse(apps[a].total()), apps[a].name.to_lowercase())),
            Sort::Name => idx.sort_by_cached_key(|&a| apps[a].name.to_lowercase()),
            Sort::LastUsed => idx.sort_by(|&a, &b| apps[a].last_used.unwrap_or(i64::MAX).cmp(&apps[b].last_used.unwrap_or(i64::MAX))),
        }
        // apps that can't be removed go last
        idx.sort_by_key(|&i| apps[i].locked.is_some());
        idx
    }
}

/// Read one bundle: info, files, sizes.
fn read_app(path: &Path, roots: &Roots, others: &[Owner], app_roots: &[PathBuf]) -> Option<AppRow> {
    let info = apps::read_info(path)?;
    let casks = apps::casks(&roots.system);
    let source = apps::source(path, &info.id, &apps::cask_apps(&casks));
    let mut row = AppRow {
        path: path.to_path_buf(),
        name: info.name,
        id: info.id.clone(),
        version: info.version,
        source,
        locked: apps::locked(path, &info.id, source),
        last_used: apps::last_used(&[path.to_path_buf()]).into_iter().next().flatten(),
        items: Vec::new(),
        sized: false,
        alt: info.alt,
        sign: extra::signing(path),
        pkgs: Vec::new(),
    };
    let index = Index::build(roots);
    let mut all = others.to_vec();
    if !all.iter().any(|o| o.path == row.path) {
        all.push(row.owner());
    }
    let found = Found { casks, receipts: extra::receipts(&roots.system), spotlight: extra::spotlight(&[row.owner()], roots) };
    fill_items(&mut row, &index, &all, roots, app_roots, &found);
    Some(row)
}

/// What one scan learns once for all apps.
struct Found {
    casks: Vec<apps::Cask>,
    receipts: Vec<String>,
    /// Spotlight's finds named after any app.
    spotlight: Vec<PathBuf>,
}

/// Everything that belongs to `row`: its folders, the rules' extras, what
/// its Homebrew cask and installer packages put in place, and (unticked)
/// Spotlight's finds. Only what removal may reach is offered.
fn fill_items(row: &mut AppRow, index: &Index, all: &[Owner], roots: &Roots, app_roots: &[PathBuf], found: &Found) {
    let owner = row.owner();
    let home = roots.home();
    let mut extra_items: Vec<Item> = index.for_app(&owner, all);
    rules::apply(rules::RULES, &owner, &mut extra_items, roots);
    let free = |p: &Path, have: &[Item]| p != row.path && !p.starts_with(&row.path) && p.symlink_metadata().is_ok() && crate::kit::fs::is_protected(p).is_none() && !index.claimed_by_other(p, &owner, all) && !have.iter().any(|i| p.starts_with(&i.path));
    let admin_for = |p: &Path| !p.starts_with(&home) && remove::needs_root(p);
    // Homebrew: the cask's record, and what its uninstall and zap stanzas name
    let mut pkg_ids = extra::pkg_candidates(&owner, &found.receipts);
    let file = row.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(cask) = extra::cask_for(&file, &found.casks) {
        extra_items.push(Item { path: cask.dir.clone(), kind: Kind::Installed, bytes: 0, admin: admin_for(&cask.dir), selected: true });
        if let Some(json) = extra::cask_json(cask) {
            let plan = extra::cask_plan(&json, roots);
            for p in plan.paths {
                if free(&p, &extra_items) {
                    // zap lists can name what a vendor's apps share: ticked
                    // only when named after this app
                    let selected = p.file_name().is_some_and(|n| leftovers::names_app(&n.to_string_lossy(), &owner));
                    extra_items.push(Item { kind: leftovers::kind_of(&p, roots), admin: admin_for(&p), path: p, bytes: 0, selected });
                }
            }
            for l in &plan.labels {
                for it in index.jobs_labelled(l) {
                    if !extra_items.iter().any(|i| i.path == it.path) {
                        extra_items.push(it);
                    }
                }
            }
            pkg_ids.extend(plan.pkgs);
        }
    }
    // installer packages that did install this app
    pkg_ids.sort();
    pkg_ids.dedup();
    let mut pkgs = Vec::new();
    for id in pkg_ids {
        let Some((location, files)) = extra::pkg_payload(&id) else { continue };
        let Some(paths) = extra::pkg_items(&row.path, &location, &files, &roots.system) else { continue };
        pkgs.push(id);
        for p in paths {
            if !free(&p, &extra_items) {
                continue;
            }
            let kind = match leftovers::kind_of(&p, roots) {
                Kind::Other => Kind::Installed,
                k => k,
            };
            let named = p.file_name().is_some_and(|n| leftovers::names_app(&n.to_string_lossy(), &owner));
            // a package may drop files into a vendor folder its other apps
            // share ("Application Support/Microsoft"): data folders only
            // when named after the app
            if matches!(kind, Kind::Support | Kind::Caches | Kind::Logs | Kind::Preferences) && !named {
                continue;
            }
            // command-line tools, frameworks and extra apps may be shared
            // with other installs: offered, not ticked
            let shared = p.starts_with(roots.system.join("usr/local")) || p.starts_with(roots.system.join("Library/Frameworks")) || p.starts_with(roots.system.join("Applications"));
            extra_items.push(Item { kind, admin: admin_for(&p), path: p, bytes: 0, selected: named || !shared });
        }
    }
    // Spotlight: named after the app, somewhere else in the home folder
    let picks: Vec<PathBuf> = extra::spotlight_pick(&found.spotlight, &owner, roots, &extra_items).into_iter().filter(|p| free(p, &extra_items)).collect();
    extra_items.extend(extra::spotlight_items(picks, roots));
    extra_items.retain(|i| remove::movable(&i.path, roots, app_roots));
    leftovers::tidy(&mut extra_items);
    let mut items = vec![Item { path: row.path.clone(), kind: Kind::App, bytes: dir_size(&row.path), admin: remove::needs_root(&row.path), selected: true }];
    for mut it in extra_items {
        it.bytes = dir_size(&it.path);
        items.push(it);
    }
    row.items = items;
    row.pkgs = pkgs;
    row.sized = true;
}

/// Take what a removal moved to the Trash out of the apps and leftovers
/// lists (an app whose bundle went, an orphan with nothing left).
fn drop_moved(g: &mut State, rep: &Report) {
    let moved: HashSet<&Path> = rep.moved.iter().map(|m| m.from.as_path()).collect();
    if moved.is_empty() {
        return;
    }
    let gone = |p: &Path| p.ancestors().any(|a| moved.contains(a));
    g.apps.retain(|a| !gone(&a.path));
    for a in g.apps.iter_mut() {
        a.items.retain(|i| !gone(&i.path));
    }
    for o in g.orphans.iter_mut() {
        o.items.retain(|i| !gone(&i.path));
    }
    g.orphans.retain(|o| !o.items.is_empty());
}

fn start_scan(st: Shared, app_roots: Vec<PathBuf>, roots: Roots, ctx: egui::Context) {
    {
        let mut g = st.lock_ok();
        if matches!(g.phase, Phase::Listing | Phase::Sizing { .. }) {
            g.again = true;
            return;
        }
        g.phase = Phase::Listing;
    }
    std::thread::Builder::new()
        .name("uninstaller-scan".into())
        .spawn(move || loop {
            let full = scan(&st, &app_roots, &roots, &ctx);
            let mut g = st.lock_ok();
            if g.again {
                g.again = false;
                g.phase = Phase::Listing;
                continue;
            }
            // listed only: the page measures everything when it opens
            g.phase = if full { Phase::Ready } else { Phase::Idle };
            ctx.request_repaint();
            break;
        })
        .ok();
}

/// One scan: list apps, then (once the page was seen) fill in files and
/// sizes app by app (the list shows right away and fills in), then find
/// orphans. Returns whether it went that far.
fn scan(st: &Shared, app_roots: &[PathBuf], roots: &Roots, ctx: &egui::Context) -> bool {
    let full = st.lock_ok().page_seen;
    let bundles = apps::find_bundles(app_roots);
    let casks = apps::casks(&roots.system);
    let cask_names = apps::cask_apps(&casks);
    let used = apps::last_used(&bundles);
    let mut rows: Vec<AppRow> = Vec::new();
    for (k, b) in bundles.iter().enumerate() {
        let Some(info) = apps::read_info(b) else { continue };
        let source = apps::source(b, &info.id, &cask_names);
        rows.push(AppRow { path: b.clone(), name: info.name, id: info.id.clone(), version: info.version, source, locked: apps::locked(b, &info.id, source), last_used: used[k], items: Vec::new(), sized: false, alt: info.alt, sign: Default::default(), pkgs: Vec::new() });
    }
    {
        let mut g = st.lock_ok();
        // keep what an earlier scan knew until it is measured again
        for r in rows.iter_mut() {
            if let Some(old) = g.apps.iter().find(|a| a.path == r.path && a.id == r.id) {
                r.items = old.items.clone();
                r.sign = old.sign.clone();
                r.pkgs = old.pkgs.clone();
            }
        }
        for r in &rows {
            g.settings.known.insert(r.id.clone(), r.name.clone());
        }
        g.settings.save();
        g.apps = rows.clone();
        if !full {
            return false;
        }
        g.phase = Phase::Sizing { done: 0, total: rows.len() };
    }
    ctx.request_repaint();
    // signatures (team, app groups) before matching: groups decide whose a
    // group container is
    for r in rows.iter_mut() {
        r.sign = extra::signing(&r.path);
    }
    let index = Index::build(roots);
    let owners: Vec<Owner> = rows.iter().map(|r| r.owner()).collect();
    let found = Found { casks, receipts: extra::receipts(&roots.system), spotlight: extra::spotlight(&owners, roots) };
    // small apps first so most of the list is complete quickly
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| rows[i].items.first().map(|x| x.bytes).unwrap_or(0));
    for (n, i) in order.into_iter().enumerate() {
        let mut row = rows[i].clone();
        fill_items(&mut row, &index, &owners, roots, app_roots, &found);
        let mut g = st.lock_ok();
        if let Some(slot) = g.apps.iter_mut().find(|a| a.path == row.path) {
            // keep the user's ticks on files
            for it in row.items.iter_mut() {
                if let Some(old) = slot.items.iter().find(|o| o.path == it.path) {
                    it.selected = old.selected;
                }
            }
            *slot = row;
        }
        g.phase = Phase::Sizing { done: n + 1, total: rows.len() };
        drop(g);
        ctx.request_repaint();
    }
    // orphans
    let mut known: HashSet<String> = rows.iter().map(|r| r.id.clone()).collect();
    known.extend(apps::all_installed_ids());
    #[cfg(target_os = "macos")]
    known.extend(crate::kit::native::running_apps().0);
    let names = st.lock_ok().settings.known.clone();
    let mut orphans: Vec<Orphan> = index
        .orphans(&known, now(), ORPHAN_MIN_AGE)
        .into_iter()
        .map(|(id, mut items)| {
            for it in items.iter_mut() {
                it.bytes = dir_size(&it.path);
            }
            let touched = items.iter().filter_map(|i| mtime(&i.path)).max();
            Orphan { name: names.get(&id).cloned().unwrap_or_else(|| pretty_name(&id)), id, items, touched, selected: true }
        })
        .collect();
    orphans.sort_by_key(|o| std::cmp::Reverse(o.bytes()));
    let mut g = st.lock_ok();
    for o in orphans.iter_mut() {
        if let Some(old) = g.orphans.iter().find(|x| x.id == o.id) {
            o.selected = old.selected;
        }
    }
    g.orphans = orphans;
    true
}

fn mtime(p: &Path) -> Option<i64> {
    let m = p.symlink_metadata().ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// "com.hnc.Discord" → "Discord", "us.zoom.xos" → "Zoom xos".
pub fn pretty_name(id: &str) -> String {
    let parts: Vec<&str> = id.split('.').collect();
    let pick = if parts.len() >= 3 { &parts[parts.len() - 1] } else { parts.last().unwrap_or(&id) };
    let mut c = pick.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => id.to_string(),
    }
}

/// Watch the app folders: when an app disappears that we didn't remove,
/// offer its leftovers; when anything changes, scan again.
fn watch(st: Shared, app_roots: Vec<PathBuf>, roots: Roots, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("uninstaller-watch".into())
        .spawn(move || {
            crate::kit::activity::low_priority();
            // the scan reads apps all over the home folder (macOS asks about
            // Downloads and other apps' data): not before first-run setup is
            // finished and Full Disk Access is granted (the page is locked till then)
            let fda = || crate::kit::perms::Perm::FullDisk.status() == crate::kit::perms::Status::Allowed;
            while !crate::core::onboard::is_done() || !fda() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            start_scan(st.clone(), app_roots.clone(), roots.clone(), ctx.clone());
            static CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();
            // app folders change rarely: wait for macOS to say so, with a slow
            // look now and then in case an event was missed
            let hooked = crate::kit::fswatch::watch(&app_roots, 2.0, &CHANGED);
            let mut last = apps::find_bundles(&app_roots);
            loop {
                crate::kit::activity::nap(if hooked { WATCH_FALLBACK } else { WATCH_EVERY }, Some(&CHANGED));
                let now_b = apps::find_bundles(&app_roots);
                if now_b == last {
                    continue;
                }
                let gone: Vec<PathBuf> = last.iter().filter(|p| !now_b.contains(p)).cloned().collect();
                last = now_b;
                let (watching, rows, ours) = {
                    let g = st.lock_ok();
                    (g.settings.watch_removed, g.apps.clone(), g.removed_by_us.clone())
                };
                if watching {
                    for p in gone {
                        let Some(row) = rows.iter().find(|r| r.path == p) else { continue };
                        // moved elsewhere (renamed, other folder) or removed by us
                        if ours.contains(&row.id) || rows.iter().any(|r| r.id == row.id && r.path != p && r.path.exists()) {
                            continue;
                        }
                        let index = Index::build(&roots);
                        let others: Vec<Owner> = rows.iter().filter(|r| r.path != p).map(|r| r.owner()).collect();
                        let mut all = others.clone();
                        all.push(row.owner());
                        let mut items: Vec<Item> = index.for_app(&row.owner(), &all);
                        for it in items.iter_mut() {
                            it.bytes = dir_size(&it.path);
                        }
                        if items.iter().map(|i| i.bytes).sum::<u64>() < 100_000 {
                            continue;
                        }
                        st.lock_ok().notice = Some(Notice { target: Target { name: row.name.clone(), id: row.id.clone(), app: None, items, reset: false, pkgs: Vec::new() }, at: Instant::now() });
                        ctx.request_repaint();
                    }
                }
                start_scan(st.clone(), app_roots.clone(), roots.clone(), ctx.clone());
            }
        })
        .ok();
}

/// The drag watcher's wake-up line and the global mouse monitor that pulls
/// it. The monitor exists only while the tool is on and the drop zone is
/// wanted ([`Uninstaller::sync_drag_hook`]); otherwise nothing watches the mouse.
pub struct DragWatch {
    tx: std::sync::mpsc::SyncSender<()>,
    hook: Option<mac::DragHook>,
    hooked: Arc<std::sync::atomic::AtomicBool>,
}

impl DragWatch {
    fn wake(&self) {
        // a full queue already holds a wake-up
        let _ = self.tx.try_send(());
    }
}

/// Notice an app being dragged out of an app folder (any drag, anywhere) by
/// watching the drag pasteboard, so the drop zone can appear. A global mouse
/// monitor wakes the watcher only while the mouse is dragging; between drags,
/// and while the zone is off, it sleeps. Without the monitor it falls back
/// to polling while the zone is on.
fn watch_drag(st: Shared, app_roots: Vec<PathBuf>, ctx: egui::Context) -> Option<DragWatch> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    let hooked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watch = DragWatch { tx, hook: None, hooked: hooked.clone() };
    std::thread::Builder::new()
        .name("uninstaller-drag".into())
        .spawn(move || {
            let mut last = mac::drag_change_count();
            loop {
                let on = {
                    let g = st.lock_ok();
                    g.settings.drop_zone && !g.off
                };
                // keep looking while the button is held: the drag pasteboard
                // can fill a moment after the first drag event
                let busy = !st.lock_ok().drag.apps.is_empty() || mac::mouse_down();
                if !busy && (!on || hooked.load(std::sync::atomic::Ordering::Relaxed)) {
                    // nothing dragged (or the zone is off): sleep until the
                    // mouse drags somewhere or the zone is turned back on
                    if rx.recv().is_err() {
                        return;
                    }
                } else {
                    // a drag in progress, or no monitor: look again shortly
                    std::thread::sleep(Duration::from_millis(150));
                    while rx.try_recv().is_ok() {}
                }
                let n = mac::drag_change_count();
                let down = mac::mouse_down();
                let mut g = st.lock_ok();
                if n != last && on {
                    last = n;
                    if down {
                        let apps: Vec<PathBuf> = mac::drag_files().into_iter().filter(|p| p.extension().is_some_and(|e| e == "app") && app_roots.iter().any(|r| p.starts_with(r))).collect();
                        if !apps.is_empty() {
                            eprintln!("[uninstaller] app dragged: {}", apps.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
                            g.drag = Drag { apps, until: None };
                            ctx.request_repaint();
                        }
                    }
                } else if n != last {
                    last = n;
                }
                if !g.drag.apps.is_empty() && !down && g.drag.until.is_none() {
                    // the drop arrives just after the button goes up
                    g.drag.until = Some(Instant::now() + Duration::from_millis(900));
                    ctx.request_repaint();
                }
                if g.drag.until.is_some_and(|u| Instant::now() > u) {
                    g.drag = Drag::default();
                    ctx.request_repaint();
                }
            }
        })
        .ok()?;
    Some(watch)
}

impl Tool for Uninstaller {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Uninstaller"
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::AppRemove, color: TINT() }
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::FullDisk)]
    }

    fn section(&self) -> &'static str {
        "Clean"
    }
    fn about(&self) -> &'static str {
        "Remove apps with everything they leave behind"
    }
    fn busy(&self) -> bool {
        let working = {
            let g = self.st.lock_ok();
            g.working.is_some() || matches!(g.phase, Phase::Listing | Phase::Sizing { .. })
        };
        working || self.ai.phase() == crate::kit::aicheck::Phase::Running
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        match route {
            "leftovers" => self.tab = Tab::Leftovers,
            r if r.starts_with("app:") => {
                self.tab = Tab::Apps;
                self.filter = Filter::All;
                self.search.clear();
                self.selected = Some(PathBuf::from(&r[4..]));
            }
            _ => self.tab = Tab::Apps,
        }
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::page(self, ui, rect, cx);
    }
    fn set_on(&mut self, on: bool) {
        self.on = on;
        self.st.lock_ok().off = !on;
        if !on {
            let mut g = self.st.lock_ok();
            g.drag = Drag::default();
            g.notice = None;
        }
        self.sync_drag_hook();
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        // the drop zone switch may have changed in Settings
        self.sync_drag_hook();
        let g = self.st.lock_ok();
        let busy = g.working.is_some() || matches!(g.phase, Phase::Listing | Phase::Sizing { .. });
        // our own floating windows need frames while the shell is hidden
        let windows = self.on && (!g.drag.apps.is_empty() || g.notice.is_some() || !matches!(g.drop, Drop::Idle) || g.last.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(8)));
        if busy && (windows || crate::kit::activity::visible()) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        windows
    }
    fn windows(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        ui::windows(self, ctx, cx);
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        ui::demo::fill(self, view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_items_leave_the_lists_at_once() {
        let item = |p: &str| super::leftovers::Item { path: PathBuf::from(p), kind: super::leftovers::Kind::Caches, bytes: 1, admin: false, selected: true };
        let mut g = State::default();
        g.orphans = vec![
            Orphan { id: "a".into(), name: "A".into(), items: vec![item("/u/Library/Caches/a")], touched: None, selected: true },
            Orphan { id: "b".into(), name: "B".into(), items: vec![item("/u/Library/Caches/b"), item("/u/Library/Logs/b/x.log")], touched: None, selected: true },
        ];
        let moved = |p: &str| remove::Moved { from: PathBuf::from(p), to: PathBuf::from("/u/.Trash/x"), admin: false, bytes: 1, owner: None };
        let rep = Report { moved: vec![moved("/u/Library/Caches/a"), moved("/u/Library/Logs/b")], ..Default::default() };
        drop_moved(&mut g, &rep);
        assert_eq!(g.orphans.len(), 1);
        assert_eq!(g.orphans[0].items.len(), 1);
        assert_eq!(g.orphans[0].items[0].path, PathBuf::from("/u/Library/Caches/b"));
    }

    #[test]
    fn pretty_names() {
        assert_eq!(pretty_name("com.hnc.Discord"), "Discord");
        assert_eq!(pretty_name("com.parallels.desktop.console"), "Console");
        assert_eq!(pretty_name("x"), "X");
    }

    #[test]
    fn filters_and_sorting() {
        let env = ToolEnv { ctx: egui::Context::default(), demo: true };
        let mut u = Uninstaller::new(&env);
        // opens by name, so rows don't jump while sizes come in
        assert!(matches!(u.sort, Sort::Name));
        u.sort = Sort::Size;
        let all = u.visible();
        assert!(all.len() > 5);
        {
            let g = u.st.lock_ok();
            // biggest first, locked ones last
            let sizes: Vec<u64> = all.iter().filter(|&&i| g.apps[i].locked.is_none()).map(|&i| g.apps[i].total()).collect();
            assert!(sizes.windows(2).all(|w| w[0] >= w[1]));
            assert!(g.apps[*all.last().unwrap()].locked.is_some());
        }
        u.filter = Filter::Unused;
        let unused = u.visible();
        {
            let g = u.st.lock_ok();
            assert!(!unused.is_empty());
            assert!(unused.iter().all(|&i| g.apps[i].last_used.is_some_and(|t| u.now() - t > UNUSED_DAYS * 86400)));
        }
        u.filter = Filter::All;
        u.search = "dock".into();
        let found = u.visible();
        assert_eq!(found.len(), 1);
    }
}
