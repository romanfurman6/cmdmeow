//! Disk Map: measure a folder tree in the background, then browse it as a
//! drill-down treemap; selected items go to the Trash after review.

use crate::kit::sync::LockExt;
use std::os::unix::fs::MetadataExt;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub items: u64,
    pub children: Vec<Node>,
    pub other: u64, // small files folded together ("Other items")
}

/// A top-level folder while mapping: its live byte count, and whether it's
/// finished.
pub struct LiveChild {
    pub name: String,
    pub path: PathBuf,
    pub bytes: Arc<AtomicU64>,
    pub done: bool,
    /// The folder's finished map, so it can be browsed while the rest is
    /// still being measured.
    pub node: Option<Arc<Node>>,
}

/// One row of the live list while mapping (cheap to copy: no subtree).
#[derive(Clone, Debug)]
pub struct LiveRow {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub done: bool,
    pub node: Option<Arc<Node>>,
}

#[derive(Default)]
pub struct Scan {
    pub running: AtomicBool,
    pub stop: AtomicBool,
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub current: Mutex<String>,
    pub result: Mutex<Option<Node>>,
    /// When the map in `result` was made (Unix seconds).
    pub when: Mutex<Option<i64>>,
    /// The saved map is being read back from disk.
    pub loading: AtomicBool,
    /// Reading the saved map was already tried this run.
    pub loaded: AtomicBool,
    /// The root's folders while mapping, so the map can grow live.
    pub live: Mutex<Option<(PathBuf, Vec<LiveChild>)>>,
    /// The file-system event id the map in `result` is current up to, so a
    /// later update re-reads only the folders that changed since.
    pub event: Mutex<Option<u64>>,
    /// An update of the shown map is running (it stays on screen meanwhile).
    pub updating: AtomicBool,
    /// That update is an automatic one: nothing shows it's running.
    pub quiet: AtomicBool,
    /// The map in `result` was stopped before it was finished.
    pub partial: AtomicBool,
    /// Counts maps made from scratch or read back from disk (not updates
    /// or removals): AI Check checks once per map.
    pub maps: AtomicU64,
}

pub type Shared = Arc<Scan>;

/// What a folder at `p` is called on the map ("/" is the startup disk).
pub fn display_name(p: &Path) -> String {
    p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| "Macintosh HD".into())
}

/// The root's folders so far while mapping, with what's been counted in
/// each. Only sizes are copied, never the finished trees, so this stays cheap
/// however big the disk is.
pub fn partial(s: &Scan) -> Option<(PathBuf, Vec<LiveRow>)> {
    let g = s.live.lock_ok();
    let (root, kids) = g.as_ref()?;
    let rows = kids
        .iter()
        .map(|k| LiveRow { name: k.name.clone(), path: k.path.clone(), bytes: k.bytes.load(Ordering::Relaxed), done: k.done, node: k.node.clone() })
        .filter(|r| r.bytes > 0)
        .collect();
    Some((root.clone(), rows))
}

/// Folders macOS guards behind their own permission prompt (Music and TV
/// media libraries, Contacts, Calendars, Reminders). Reading inside them
/// pops a system dialog even with Full Disk Access, so the map never
/// enters them.
pub fn asks_permission(p: &Path) -> bool {
    if p.extension().is_some_and(|e| e == "musiclibrary" || e == "tvlibrary") {
        return true;
    }
    let Some(home) = dirs::home_dir() else { return false };
    ["Music/Music", "Music/iTunes", "Movies/TV", "Library/Calendars", "Library/Reminders", "Library/Application Support/AddressBook", "Library/Application Support/CallHistoryDB"]
        .iter()
        .any(|x| p == home.join(x))
}

/// Run the current thread just below the UI, so mapping never makes the
/// window stutter.
fn below_ui() {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INITIATED, 0);
    }
}

/// Folders that the system needs; they can be browsed but not selected.
pub fn is_essential(p: &Path) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let fixed = ["/System", "/usr", "/bin", "/sbin", "/private", "/Library", "/Applications", "/opt", "/cores", "/dev", "/etc", "/var", "/tmp", "/Volumes", "/Users"];
    if fixed.iter().any(|f| p == Path::new(f)) {
        return true;
    }
    // a whole volume
    if p.parent() == Some(Path::new("/Volumes")) {
        return true;
    }
    if p.starts_with("/System") || p.starts_with("/usr") || p.starts_with("/bin") || p.starts_with("/sbin") || p.starts_with("/private/var/db") {
        return true;
    }
    p == home || p == home.join("Library") || p == home.join("Library/Application Support") || p == home.join("Library/Containers") || p == home.join("Library/Preferences") || p == home.join("Library/Keychains")
}

const KEEP_FILES_OVER: u64 = 8_000_000;

/// Walk `root` on background threads. Big trees are split across the CPU
/// cores: a folder's subfolders go to free threads while any are left.
pub fn start(shared: Shared, root: PathBuf, ctx: egui::Context) {
    if shared.running.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        below_ui();
        full(&shared, &root, &ctx);
    });
}

/// A whole map from scratch, on this thread (`running` is already set).
fn full(shared: &Shared, root: &Path, ctx: &egui::Context) {
    shared.stop.store(false, Ordering::SeqCst);
    shared.files.store(0, Ordering::SeqCst);
    shared.bytes.store(0, Ordering::SeqCst);
    *shared.result.lock_ok() = None;
    *shared.live.lock_ok() = None;
    let event = super::history::current();
    let w = Walker::new(root, shared, ctx);
    let node = w.root(root);
    let now = chrono::Utc::now().timestamp();
    let stopped = shared.stop.load(Ordering::SeqCst);
    // a stopped map is incomplete: no update may build on it
    let event = if stopped { None } else { event };
    // nor replace the last finished map on disk
    if root == Path::new("/") && !stopped {
        save(&node, now, event);
    }
    shared.partial.store(stopped, Ordering::SeqCst);
    *shared.when.lock_ok() = Some(now);
    *shared.event.lock_ok() = event;
    *shared.result.lock_ok() = Some(node);
    shared.maps.fetch_add(1, Ordering::SeqCst);
    *shared.live.lock_ok() = None;
    shared.running.store(false, Ordering::SeqCst);
    ctx.request_repaint();
    if !stopped {
        crate::kit::sound::play(crate::kit::sound::Cue::Ready);
    }
}

/// How old a map may be and still be updated from the change history;
/// older ones are mapped again from scratch.
const UPDATE_MAX_AGE: i64 = 3 * 86400;
/// More changed folders than this: mapping again is quicker.
pub(super) const UPDATE_LIMIT: usize = 50_000;

/// Bring the shown map up to date: only the folders that changed since it
/// was made are read again (from the system's file-change history). Falls
/// back to a whole new map when there's no usable history. `quiet` (an
/// automatic update while the page is open): no progress, no sound, and no
/// new map when the history can't be used; nothing is saved when nothing
/// on the map changed.
pub fn update(shared: Shared, ctx: egui::Context, quiet: bool) {
    if shared.running.swap(true, Ordering::SeqCst) {
        return;
    }
    // the map stays on screen while it's brought up to date
    shared.quiet.store(quiet, Ordering::SeqCst);
    // cleared once, now: a Stop while the history is read still counts
    shared.stop.store(false, Ordering::SeqCst);
    shared.updating.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        below_ui();
        let old = shared.result.lock_ok().clone();
        let since = *shared.event.lock_ok();
        let when = shared.when.lock_ok().unwrap_or(0);
        let fresh = chrono::Utc::now().timestamp() - when < UPDATE_MAX_AGE;
        let root = old.as_ref().map(|n| n.path.clone()).unwrap_or_else(|| PathBuf::from("/"));
        let plan = match (old, since) {
            (Some(old), Some(since)) if fresh => {
                ctx.request_repaint();
                let now_event = super::history::current();
                super::history::since(since, &watch_roots(&root), std::time::Duration::from_secs(10)).filter(|c| c.len() <= UPDATE_LIMIT).map(|c| (old, c, now_event))
            }
            _ => None,
        };
        if shared.stop.load(Ordering::SeqCst) {
            shared.updating.store(false, Ordering::SeqCst);
            shared.running.store(false, Ordering::SeqCst);
            ctx.request_repaint();
            return;
        }
        let Some((old, changes, now_event)) = plan else {
            shared.updating.store(false, Ordering::SeqCst);
            // Update clicked while this ran quietly: it turned `quiet` off
            // and expects a new map
            if quiet && shared.quiet.load(Ordering::SeqCst) {
                shared.running.store(false, Ordering::SeqCst);
            } else {
                full(&shared, &root, &ctx);
            }
            return;
        };
        let changes = relevant(changes, &root);
        if changes.is_empty() {
            // nothing on the map changed (only the app's own files, or
            // places outside it): just remember how far the history was read
            *shared.event.lock_ok() = now_event;
            shared.updating.store(false, Ordering::SeqCst);
            shared.running.store(false, Ordering::SeqCst);
            ctx.request_repaint();
            return;
        }
        if !quiet {
            shared.files.store(0, Ordering::SeqCst);
            shared.bytes.store(0, Ordering::SeqCst);
        }
        let w = Walker::new(&root, &shared, &ctx);
        let node = w.refresh(old, 0, &changes);
        let stopped = shared.stop.load(Ordering::SeqCst);
        if let (Some(node), false) = (node, stopped) {
            let now = chrono::Utc::now().timestamp();
            if root == Path::new("/") {
                save(&node, now, now_event);
            }
            *shared.when.lock_ok() = Some(now);
            *shared.event.lock_ok() = now_event;
            *shared.result.lock_ok() = Some(node);
            shared.partial.store(false, Ordering::SeqCst);
        }
        shared.updating.store(false, Ordering::SeqCst);
        shared.running.store(false, Ordering::SeqCst);
        ctx.request_repaint();
        if !stopped && !quiet {
            crate::kit::sound::play(crate::kit::sound::Cue::Ready);
        }
    });
}

/// Where the change history for a map of `root` is read: the startup disk
/// is also reached through its Data volume.
pub fn watch_roots(root: &Path) -> Vec<PathBuf> {
    if root == Path::new("/") {
        vec![PathBuf::from("/"), PathBuf::from("/System/Volumes/Data")]
    } else {
        vec![root.to_path_buf()]
    }
}

/// The changes that touch the map of `root`: inside it, and not the app's
/// own data (saving the map would otherwise count as a change of the map).
fn relevant(changes: Vec<Change>, root: &Path) -> Vec<Change> {
    let own = crate::kit::fs::data_path("");
    changes.into_iter().filter(|c| c.path.starts_with(root) && !c.path.starts_with(&own)).collect()
}

/// One changed folder from the history. `deep`: everything under it may have
/// changed (the system lost track of the details), so it's read again whole.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: PathBuf,
    pub deep: bool,
}

/// Folders shown as one item and never opened on the map: app and document
/// packages, and folders of countless tiny files nobody browses
/// (dependencies, version control, bytecode caches). They're still measured
/// in full; only their insides aren't kept.
pub fn is_atomic(p: &Path) -> bool {
    const EXT: &[&str] = &["app", "bundle", "framework", "plugin", "appex", "xpc", "kext", "photoslibrary", "photolibrary", "aplibrary", "migratedphotolibrary", "imovielibrary", "fcpbundle", "logicx", "band", "xcarchive", "dSYM", "sparsebundle", "pkg", "mpkg", "rtfd", "pages", "numbers", "key", "playground", "xcodeproj", "xcworkspace"];
    const NAMES: &[&str] = &["node_modules", ".git", ".svn", ".hg", "__pycache__", ".venv", ".tox", ".gradle", ".pnpm-store", ".yarn", "bower_components"];
    if p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXT.iter().any(|x| x.eq_ignore_ascii_case(e))) {
        return true;
    }
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| NAMES.contains(&n))
}

// ---------------------------------------------------------------- saved map

/// Where the last map of the disk is kept between runs.
fn saved_path() -> PathBuf {
    crate::kit::fs::data_path("diskmap.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    when: i64,
    root: Node,
    #[serde(default)]
    event: Option<u64>,
}

/// Keep `root` as the last map (written to a temp file, then moved in place).
pub fn save(root: &Node, when: i64, event: Option<u64>) {
    let path = saved_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let tmp = path.with_extension("json.tmp");
    let ok = std::fs::File::create(&tmp).ok().is_some_and(|f| {
        #[derive(serde::Serialize)]
        struct SavedRef<'a> {
            when: i64,
            root: &'a Node,
            event: Option<u64>,
        }
        serde_json::to_writer(std::io::BufWriter::new(f), &SavedRef { when, root, event }).is_ok()
    });
    if ok {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Save the current map in the background (after items were removed from it).
pub fn save_later(shared: &Shared) {
    let s = shared.clone();
    std::thread::spawn(move || {
        let node = s.result.lock_ok().clone();
        let when = *s.when.lock_ok();
        let event = *s.event.lock_ok();
        if let (Some(node), Some(when)) = (node, when) {
            if node.path == Path::new("/") {
                save(&node, when, event);
            }
        }
    });
}

/// Read the last map back, once per run, in the background. Returns false
/// when there's no saved map (so the caller can start one).
pub fn load(shared: &Shared, ctx: &egui::Context) -> bool {
    if shared.loaded.swap(true, Ordering::SeqCst) {
        return true;
    }
    let path = saved_path();
    if !path.exists() {
        return false;
    }
    shared.loading.store(true, Ordering::SeqCst);
    let (s, ctx) = (shared.clone(), ctx.clone());
    std::thread::spawn(move || {
        below_ui();
        let saved: Option<Saved> = std::fs::File::open(&path).ok().and_then(|f| serde_json::from_reader(std::io::BufReader::new(f)).ok());
        if let Some(saved) = saved {
            let mut r = s.result.lock_ok();
            if r.is_none() && !s.running.load(Ordering::SeqCst) {
                *r = Some(saved.root);
                s.maps.fetch_add(1, Ordering::SeqCst);
                *s.when.lock_ok() = Some(saved.when);
                *s.event.lock_ok() = saved.event;
            }
        }
        s.loading.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
    true
}

/// Volumes and folders a walk stays inside, and the threads it may use.
struct Walker<'a> {
    s: &'a Scan,
    ctx: &'a egui::Context,
    /// Devices to stay on: the root's, plus the Data volume when mapping "/"
    /// (macOS reaches /Users, /Applications… through firmlinks onto it).
    devs: Vec<u64>,
    /// Folders never entered (the Data volume's own mount point, so nothing
    /// is counted twice).
    skip: Vec<PathBuf>,
    /// Extra threads still free.
    free: AtomicU64,
    last: Mutex<std::time::Instant>,
    /// Files with several hard links already counted: (device, file id).
    links: Mutex<HashSet<(u64, u64)>>,
    /// APFS clone families already counted in full.
    clones: Mutex<HashSet<u64>>,
}

type Files = Vec<(PathBuf, u64, String)>;

impl<'a> Walker<'a> {
    fn new(root: &Path, s: &'a Scan, ctx: &'a egui::Context) -> Self {
        let dev_of = |p: &str| Path::new(p).symlink_metadata().map(|m| m.dev()).ok();
        let mut devs: Vec<u64> = root.symlink_metadata().map(|m| m.dev()).into_iter().collect();
        let mut skip = Vec::new();
        if root == Path::new("/") {
            devs.extend(dev_of("/System/Volumes/Data"));
            skip.push(PathBuf::from("/System/Volumes"));
            skip.push(PathBuf::from("/Volumes"));
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) as u64;
        // leave a core for the window
        Walker { s, ctx, devs, skip, free: AtomicU64::new(cores.saturating_sub(2).max(1)), last: Mutex::new(std::time::Instant::now()), links: Default::default(), clones: Default::default() }
    }

    /// Take a free thread if there is one.
    fn try_thread(&self) -> bool {
        let mut n = self.free.load(Ordering::Acquire);
        while n > 0 {
            match self.free.compare_exchange_weak(n, n - 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true,
                Err(now) => n = now,
            }
        }
        false
    }

    /// Bytes a file takes that no file counted before shares: a second hard
    /// link takes nothing, and of an APFS clone family only the first file
    /// counts in full, the rest just what they don't share.
    fn charge(&self, dev: u64, e: &crate::kit::fs::Listed) -> u64 {
        if e.links > 1 && !self.links.lock_ok().insert((dev, e.id)) {
            return 0;
        }
        match (e.private, e.clone) {
            (Some(own), Some(family)) if own < e.bytes => {
                if self.clones.lock_ok().insert(family) {
                    e.bytes
                } else {
                    own
                }
            }
            _ => e.bytes,
        }
    }

    /// The root: its folders are published live and each one counts into
    /// its own total while it's walked.
    fn root(&self, root: &Path) -> Node {
        let mut node = Node { name: display_name(root), path: root.to_path_buf(), ..Default::default() };
        let (dirs, files) = self.list(root).unwrap_or_default();
        self.add_files(&mut node, files, None, true);
        let live: Vec<LiveChild> = dirs
            .iter()
            .map(|d| LiveChild { name: d.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default(), path: d.clone(), bytes: Arc::new(AtomicU64::new(0)), done: false, node: None })
            .collect();
        let counters: Vec<Arc<AtomicU64>> = live.iter().map(|l| l.bytes.clone()).collect();
        *self.s.live.lock_ok() = Some((root.to_path_buf(), live));
        let next = AtomicU64::new(0);
        // every core pulls the next top-level folder; inside one, free threads help
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(1).max(1).min(dirs.len().max(1));
        std::thread::scope(|sc| {
            for _ in 0..workers {
                sc.spawn(|| loop {
                    below_ui();
                    let k = next.fetch_add(1, Ordering::SeqCst) as usize;
                    if k >= dirs.len() {
                        break;
                    }
                    let child = self.walk(&dirs[k], 1, Some(&counters[k]), true);
                    counters[k].store(child.as_ref().map(|c| c.bytes).unwrap_or(0), Ordering::Relaxed);
                    if let Some((_, live)) = self.s.live.lock_ok().as_mut() {
                        live[k].done = true;
                        live[k].node = child.map(Arc::new);
                    }
                    self.ctx.request_repaint();
                });
            }
        });
        // the finished folders back out of the live list (the window may
        // still hold one for a moment: then it's copied)
        let done: Vec<Arc<Node>> = self.s.live.lock_ok().as_mut().map(|(_, live)| live.iter_mut().filter_map(|l| l.node.take()).collect()).unwrap_or_default();
        for child in done.into_iter().map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone())) {
            node.bytes += child.bytes;
            node.items += child.items + 1;
            node.children.push(child);
        }
        node.children.sort_by_key(|c| std::cmp::Reverse(c.bytes));
        node
    }

    /// Subfolders and (path, bytes, name) files of `p`. `None` when it can't
    /// be read or is on another volume (network mounts, backups, the Data
    /// volume's own mount point).
    fn list(&self, p: &Path) -> Option<(Vec<PathBuf>, Files)> {
        let (dev, entries) = crate::kit::fs::list_dir(p)?;
        if !self.devs.is_empty() && !self.devs.contains(&dev) {
            return None;
        }
        let (mut dirs, mut files) = (Vec::new(), Vec::new());
        for e in &entries {
            let cp = p.join(&e.name);
            if asks_permission(&cp) || self.skip.contains(&cp) {
                continue; // never touch folders behind a permission prompt
            }
            if e.dir {
                dirs.push(cp);
            } else {
                files.push((cp, self.charge(dev, e), e.name.to_string_lossy().into_owned()));
            }
        }
        Some((dirs, files))
    }

    /// Count `files` into `node`; big ones get their own entry when `keep`.
    fn add_files(&self, node: &mut Node, files: Files, top: Option<&AtomicU64>, keep: bool) {
        let mut sum = 0;
        self.s.files.fetch_add(files.len() as u64, Ordering::Relaxed);
        for (path, b, name) in files {
            node.bytes += b;
            node.items += 1;
            sum += b;
            if keep && b >= KEEP_FILES_OVER {
                node.children.push(Node { name, path, bytes: b, items: 1, children: vec![], other: 0 });
            } else {
                node.other += b;
            }
        }
        if let Some(t) = top {
            t.fetch_add(sum, Ordering::Relaxed);
        }
        self.s.bytes.fetch_add(sum, Ordering::Relaxed);
    }

    fn progress(&self, p: &Path) {
        if let Ok(mut last) = self.last.try_lock() {
            if last.elapsed().as_millis() > 120 {
                *self.s.current.lock_ok() = p.display().to_string();
                self.ctx.request_repaint();
                *last = std::time::Instant::now();
            }
        }
    }

    /// Measure the folder at `p`. `keep`: build its subtree for browsing;
    /// otherwise (inside a package or a dependency folder) only sizes are
    /// added up. `None` when it can't be read or is on another volume.
    fn walk(&self, p: &Path, depth: usize, top: Option<&AtomicU64>, keep: bool) -> Option<Node> {
        let name = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
        let mut node = Node { name, path: p.to_path_buf(), ..Default::default() };
        if self.s.stop.load(Ordering::Relaxed) {
            return Some(node);
        }
        self.progress(p);
        let (dirs, files) = self.list(p)?;
        let keep = keep && !is_atomic(p);
        self.add_files(&mut node, files, top, keep);
        let kids: Vec<Node> = std::thread::scope(|sc| {
            let mut handles = Vec::new();
            let mut done = Vec::new();
            for d in &dirs {
                if self.try_thread() {
                    handles.push(sc.spawn(move || {
                        below_ui();
                        let n = self.walk(d, depth + 1, top, keep);
                        self.free.fetch_add(1, Ordering::AcqRel);
                        n
                    }));
                } else {
                    done.extend(self.walk(d, depth + 1, top, keep));
                }
            }
            done.extend(handles.into_iter().filter_map(|h| h.join().ok().flatten()));
            done
        });
        for child in kids {
            node.bytes += child.bytes;
            node.items += child.items + 1;
            if keep {
                node.children.push(child);
            } else {
                node.other += child.bytes;
            }
        }
        finish(&mut node, depth);
        Some(node)
    }

    /// `old` brought up to date with `changes` (all of them at or under its
    /// path): untouched folders are kept as they are, changed ones read
    /// again. `None` when the folder is gone.
    fn refresh(&self, old: Node, depth: usize, changes: &[Change]) -> Option<Node> {
        if changes.is_empty() || self.s.stop.load(Ordering::Relaxed) {
            return Some(old);
        }
        let p = old.path.clone();
        if is_atomic(&p) || changes.iter().any(|c| c.deep && c.path == p) {
            return self.walk(&p, depth, None, true);
        }
        self.progress(&p);
        let under = |q: &Path| changes.iter().filter(|c| c.path.starts_with(q)).cloned().collect::<Vec<_>>();
        // read this folder again when it changed itself, or a change is in
        // something the map folded away (small files, tiny folders)
        let relist = changes.iter().any(|c| c.path == p || !old.children.iter().any(|k| c.path.starts_with(&k.path)));
        let mut node = Node { name: old.name.clone(), path: p.clone(), ..Default::default() };
        if relist {
            let (dirs, files) = self.list(&p)?;
            self.add_files(&mut node, files, None, true);
            let mut known: std::collections::HashMap<PathBuf, Node> = old.children.into_iter().map(|k| (k.path.clone(), k)).collect();
            for d in dirs {
                let child = match known.remove(&d) {
                    // a known folder; anything that looked like a file is read whole
                    Some(k) if k.items > 1 || !k.children.is_empty() => self.refresh(k, depth + 1, &under(&d)),
                    _ => self.walk(&d, depth + 1, None, true),
                };
                if let Some(c) = child {
                    node.bytes += c.bytes;
                    node.items += c.items + 1;
                    node.children.push(c);
                }
            }
        } else {
            let kids_bytes: u64 = old.children.iter().map(|k| k.bytes).sum();
            let kids_items: u64 = old.children.iter().map(|k| k.items + 1).sum();
            node.bytes = old.bytes.saturating_sub(kids_bytes);
            node.items = old.items.saturating_sub(kids_items);
            node.other = old.other;
            for k in old.children {
                let sub = under(&k.path);
                if let Some(c) = self.refresh(k, depth + 1, &sub) {
                    node.bytes += c.bytes;
                    node.items += c.items + 1;
                    node.children.push(c);
                }
            }
        }
        finish(&mut node, depth);
        Some(node)
    }
}

/// Biggest first; deep down, tiny folders fold into "other" so memory stays
/// bounded however big the disk is.
fn finish(node: &mut Node, depth: usize) {
    node.children.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    if depth > 3 {
        let mut kept = Vec::new();
        for c in node.children.drain(..) {
            if c.bytes >= 1_000_000 || kept.len() < 40 {
                kept.push(c);
            } else {
                node.other += c.bytes;
            }
        }
        node.children = kept;
    }
}

/// Find the node at `path` inside `root`.
pub fn find<'a>(root: &'a Node, path: &Path) -> Option<&'a Node> {
    if root.path == path {
        return Some(root);
    }
    root.children.iter().filter(|c| path.starts_with(&c.path)).find_map(|c| find(c, path))
}

/// Remove nodes whose paths were deleted, fixing sizes on the way up.
pub fn prune(node: &mut Node, removed: &[PathBuf]) -> u64 {
    let mut freed = 0;
    node.children.retain(|c| {
        if removed.contains(&c.path) {
            freed += c.bytes;
            false
        } else {
            true
        }
    });
    for c in node.children.iter_mut() {
        freed += prune(c, removed);
    }
    node.bytes = node.bytes.saturating_sub(freed);
    freed
}

// ---------------------------------------------------------------- treemap

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, bytes: u64, children: Vec<Node>) -> Node {
        Node { name: path.rsplit('/').next().unwrap().into(), path: PathBuf::from(path), bytes, children, ..Default::default() }
    }

    fn tree() -> Node {
        node("/r", 100, vec![node("/r/a", 60, vec![node("/r/a/x", 50, vec![]), node("/r/a/y", 10, vec![])]), node("/r/ab", 40, vec![])])
    }

    #[test]
    fn find_by_path() {
        let t = tree();
        assert_eq!(find(&t, Path::new("/r/a/y")).unwrap().bytes, 10);
        assert_eq!(find(&t, Path::new("/r/ab")).unwrap().bytes, 40);
        assert!(find(&t, Path::new("/r/zz")).is_none());
    }

    #[test]
    fn prune_fixes_sizes_up_the_tree() {
        let mut t = tree();
        let freed = prune(&mut t, &[PathBuf::from("/r/a/x"), PathBuf::from("/r/ab")]);
        assert_eq!(freed, 90);
        assert_eq!(t.bytes, 10);
        assert_eq!(find(&t, Path::new("/r/a")).unwrap().bytes, 10);
        assert!(find(&t, Path::new("/r/a/x")).is_none());
    }

    #[test]
    fn permission_folders_are_skipped() {
        let h = dirs::home_dir().unwrap();
        assert!(asks_permission(&h.join("Music/Music")));
        assert!(asks_permission(Path::new("/Volumes/X/Old.musiclibrary")));
        assert!(asks_permission(&h.join("Library/Calendars")));
        assert!(!asks_permission(&h.join("Music")));
        assert!(!asks_permission(&h.join("Downloads")));
    }

    #[test]
    fn saved_map_round_trips() {
        let t = tree();
        let json = serde_json::to_string(&Saved { when: 7, root: t.clone(), event: Some(9) }).unwrap();
        let back: Saved = serde_json::from_str(&json).unwrap();
        assert_eq!(back.when, 7);
        assert_eq!(back.event, Some(9));
        // maps saved before updates existed still open
        let old: Saved = serde_json::from_str(r#"{"when":1,"root":{"name":"r","path":"/r","bytes":0,"items":0,"children":[],"other":0}}"#).unwrap();
        assert_eq!(old.event, None);
        assert_eq!(find(&back.root, Path::new("/r/a/x")).unwrap().bytes, 50);
    }

    #[test]
    fn essential_paths() {
        assert!(is_essential(Path::new("/System/Library")));
        assert!(is_essential(Path::new("/Applications")));
        assert!(!is_essential(Path::new("/Applications/Foo.app")));
        let h = dirs::home_dir().unwrap();
        assert!(is_essential(&h));
        assert!(is_essential(&h.join("Library")));
        assert!(!is_essential(&h.join("Downloads/big.iso")));
    }

    fn map_now(base: &Path) -> Node {
        let shared: Shared = Default::default();
        start(shared.clone(), base.to_path_buf(), egui::Context::default());
        let t0 = std::time::Instant::now();
        while shared.running.load(Ordering::SeqCst) && t0.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let n = shared.result.lock_ok().take().unwrap();
        n
    }

    fn same(a: &Node, b: &Node) {
        assert_eq!(a.path, b.path);
        assert_eq!(a.bytes, b.bytes, "bytes of {:?}", a.path);
        assert_eq!(a.items, b.items, "items of {:?}", a.path);
        let mut x: Vec<_> = a.children.iter().map(|c| c.path.clone()).collect();
        let mut y: Vec<_> = b.children.iter().map(|c| c.path.clone()).collect();
        x.sort();
        y.sort();
        assert_eq!(x, y, "children of {:?}", a.path);
        for c in &a.children {
            same(c, b.children.iter().find(|d| d.path == c.path).unwrap());
        }
    }

    #[test]
    fn update_reads_only_changed_folders_and_matches_a_new_map() {
        let base = std::env::temp_dir().join(format!("cmb-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for a in 0..4 {
            for b in 0..3 {
                let d = base.join(format!("t{a}/s{b}"));
                std::fs::create_dir_all(&d).unwrap();
                for c in 0..3 {
                    std::fs::write(d.join(format!("f{c}")), vec![1u8; 20_000]).unwrap();
                }
            }
        }
        let old = map_now(&base);
        // a file added deep down, a folder removed, a new folder with a big file
        std::fs::write(base.join("t1/s2/new"), vec![2u8; 50_000]).unwrap();
        std::fs::remove_dir_all(base.join("t3/s0")).unwrap();
        std::fs::create_dir_all(base.join("t0/fresh")).unwrap();
        std::fs::write(base.join("t0/fresh/big"), vec![3u8; 9_000_000]).unwrap();
        // an untouched folder changes too, but isn't in the history: kept as it was
        let changes = vec![Change { path: base.join("t0"), deep: false }, Change { path: base.join("t1/s2"), deep: false }, Change { path: base.join("t3"), deep: false }];
        let shared: Shared = Default::default();
        let ctx = egui::Context::default();
        let w = Walker::new(&base, &shared, &ctx);
        let updated = w.refresh(old, 0, &changes).unwrap();
        let fresh = map_now(&base);
        same(&updated, &fresh);
        assert!(find(&updated, &base.join("t0/fresh/big")).is_some());
        // a folder read whole
        std::fs::write(base.join("t2/s1/deep"), vec![4u8; 30_000]).unwrap();
        let w = Walker::new(&base, &shared, &ctx);
        let updated = w.refresh(updated, 0, &[Change { path: base.join("t2"), deep: true }]).unwrap();
        same(&updated, &map_now(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_changes_on_the_map_count() {
        let own = crate::kit::fs::data_path("diskmap.json");
        let c = |p: &Path| Change { path: p.to_path_buf(), deep: false };
        let got = relevant(vec![c(Path::new("/Users/me/a")), c(Path::new("/Volumes/X/b")), c(own.parent().unwrap())], Path::new("/Users/me"));
        assert_eq!(got, vec![c(Path::new("/Users/me/a"))]);
        assert_eq!(watch_roots(Path::new("/")).len(), 2);
        assert_eq!(watch_roots(Path::new("/Users/me")), vec![PathBuf::from("/Users/me")]);
    }

    #[test]
    fn packages_are_one_item() {
        assert!(is_atomic(Path::new("/Applications/Xcode.app")));
        assert!(is_atomic(Path::new("/p/web/node_modules")));
        assert!(is_atomic(Path::new("/p/web/.git")));
        assert!(!is_atomic(Path::new("/Users/me/Downloads")));
        let base = std::env::temp_dir().join(format!("cmb-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Big.app/Contents/Resources")).unwrap();
        std::fs::write(base.join("Big.app/Contents/Resources/blob"), vec![1u8; 9_000_000]).unwrap();
        let root = map_now(&base);
        let app = find(&root, &base.join("Big.app")).unwrap();
        assert!(app.children.is_empty());
        assert!(app.bytes >= 9_000_000);
        assert_eq!(app.items, 3);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hard_links_count_once() {
        let base = std::env::temp_dir().join(format!("cmb-hl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a")).unwrap();
        std::fs::create_dir_all(base.join("b")).unwrap();
        std::fs::write(base.join("a/f"), vec![1u8; 2_000_000]).unwrap();
        let one = map_now(&base).bytes;
        std::fs::hard_link(base.join("a/f"), base.join("b/f")).unwrap();
        assert_eq!(map_now(&base).bytes, one);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parallel_walk_counts_every_file_once() {
        let base = std::env::temp_dir().join(format!("cmb-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // 6 top folders x 5 subfolders x 4 files of 10 kB, plus one big file
        for a in 0..6 {
            for b in 0..5 {
                let d = base.join(format!("t{a}/s{b}"));
                std::fs::create_dir_all(&d).unwrap();
                for c in 0..4 {
                    std::fs::write(d.join(format!("f{c}")), vec![1u8; 10_000]).unwrap();
                }
            }
        }
        std::fs::write(base.join("t0/big"), vec![1u8; 9_000_000]).unwrap();
        let shared: Shared = Default::default();
        start(shared.clone(), base.clone(), egui::Context::default());
        let t0 = std::time::Instant::now();
        while shared.running.load(Ordering::SeqCst) && t0.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let root = shared.result.lock_ok().take().unwrap();
        assert_eq!(shared.files.load(Ordering::SeqCst), 6 * 5 * 4 + 1);
        assert_eq!(root.children.len(), 6);
        assert_eq!(root.children[0].name, "t0"); // biggest first
        assert!(root.children[0].children.iter().any(|c| c.name == "big"));
        let sum: u64 = root.children.iter().map(|c| c.bytes).sum();
        assert_eq!(root.bytes, sum);
        assert_eq!(shared.bytes.load(Ordering::SeqCst), root.bytes);
        assert!(shared.live.lock_ok().is_none());
        // every folder's subtree came back from the live list
        assert!(root.children.iter().all(|c| c.children.len() >= 5));
        let _ = std::fs::remove_dir_all(&base);
    }

}
