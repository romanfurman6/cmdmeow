//! Finding an app's files outside its bundle: its folders in ~/Library and
//! /Library (support data, caches, preferences, containers, saved state,
//! logs, web data, plug-ins), launchd jobs that run it, and dot-folders in
//! the home folder. Also the data left behind by apps that are gone
//! ("orphans").
//!
//! Matching is deliberately strict. A file belongs to an app when it is
//! named by the app's bundle id (or an id below it), carries that id inside
//! (a plug-in's Info.plist, a sandbox container's metadata, a launchd job's
//! AssociatedBundleIdentifiers), is one of the app groups the app is signed
//! for, or, in the folders apps commonly name after themselves, has exactly
//! one of the app's names. Anything that matches two installed apps, or that
//! the shared protection rules keep, is never offered.

use crate::kit::fs::is_protected;
use crate::kit::plist;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Where an item lives; drives its label and whether a reset removes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    App,
    Support,
    Caches,
    Preferences,
    Containers,
    GroupContainers,
    SavedState,
    Logs,
    WebData,
    /// Plug-ins, extensions, preference panes, audio drivers.
    Extension,
    Launch,
    /// Files an installer package or Homebrew put in place.
    Installed,
    /// Anywhere else (a vendor folder, a dot-folder, a Spotlight find).
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "App",
            Kind::Support => "Application Support",
            Kind::Caches => "Caches",
            Kind::Preferences => "Preferences",
            Kind::Containers => "Containers",
            Kind::GroupContainers => "Group Containers",
            Kind::SavedState => "Saved State",
            Kind::Logs => "Logs",
            Kind::WebData => "Web data",
            Kind::Extension => "Plug-in",
            Kind::Launch => "Launch agent",
            Kind::Installed => "Installed files",
            Kind::Other => "Other",
        }
    }

    /// Part of the app itself rather than its data: a reset keeps it.
    pub fn is_part(self) -> bool {
        matches!(self, Kind::App | Kind::Launch | Kind::Extension | Kind::Installed)
    }
}

/// One folder that apps write into.
struct Place {
    rel: &'static str,
    kind: Kind,
    /// Apps also name entries here after themselves ("Application Support/Slack").
    by_name: bool,
    /// Entries may be data of apps that are gone.
    orphans: bool,
    /// Also look one level into vendor folders ("Application Support/JetBrains/PyCharm2024.2").
    nested: bool,
}

const fn place(rel: &'static str, kind: Kind, by_name: bool, orphans: bool, nested: bool) -> Place {
    Place { rel, kind, by_name, orphans, nested }
}

/// Under ~/Library.
const USER_PLACES: &[Place] = &[
    place("Application Support", Kind::Support, true, true, true),
    place("Caches", Kind::Caches, true, true, true),
    place("Preferences", Kind::Preferences, false, true, false),
    place("Preferences/ByHost", Kind::Preferences, false, true, false),
    place("Containers", Kind::Containers, false, true, false),
    place("Group Containers", Kind::GroupContainers, false, false, false),
    place("Saved Application State", Kind::SavedState, false, true, false),
    place("Logs", Kind::Logs, true, true, true),
    place("HTTPStorages", Kind::WebData, false, true, false),
    place("WebKit", Kind::WebData, false, true, false),
    place("Cookies", Kind::WebData, false, true, false),
    place("Application Scripts", Kind::Containers, false, true, false),
    place("PreferencePanes", Kind::Extension, true, false, false),
    place("Internet Plug-Ins", Kind::Extension, true, false, false),
    place("Services", Kind::Extension, true, false, false),
    place("QuickLook", Kind::Extension, true, false, false),
    place("Screen Savers", Kind::Extension, true, false, false),
];

/// Under the home folder itself.
const HOME_PLACES: &[Place] = &[place(".config", Kind::Other, true, false, false)];

/// Under "/" (moving these needs the admin password unless they are ours).
const SYSTEM_PLACES: &[Place] = &[
    place("Library/Application Support", Kind::Support, true, false, true),
    place("Library/Caches", Kind::Caches, true, false, true),
    place("Library/Preferences", Kind::Preferences, false, false, false),
    place("Library/Logs", Kind::Logs, true, false, true),
    place("Library/Extensions", Kind::Extension, true, false, false),
    place("Library/PreferencePanes", Kind::Extension, true, false, false),
    place("Library/Internet Plug-Ins", Kind::Extension, true, false, false),
    place("Library/Audio/Plug-Ins/HAL", Kind::Extension, true, false, false),
    place("Library/Audio/Plug-Ins/Components", Kind::Extension, true, false, false),
    place("Library/QuickLook", Kind::Extension, true, false, false),
    place("Library/Spotlight", Kind::Extension, true, false, false),
    place("Library/Screen Savers", Kind::Extension, true, false, false),
];

/// launchd folders: (path under the root, needs admin).
const LAUNCH_USER: &str = "LaunchAgents";
const LAUNCH_SYSTEM: &[(&str, bool)] = &[("Library/LaunchAgents", false), ("Library/LaunchDaemons", true)];
const HELPERS_SYSTEM: &str = "Library/PrivilegedHelperTools";

/// Folders of ~/Library and /Library that macOS or everyone uses. Anything
/// else at the top of a Library is a vendor's own folder ("/Library/Objective-See").
pub const STANDARD_LIBRARY: &[&str] = &[
    "Accessibility", "Accounts", "Application Scripts", "Application Support", "Apple", "AppleMediaServices", "Assistant", "Assistants", "Audio", "Autosave Information", "Biome", "Bluetooth", "Caches", "Calendars", "CallServices", "CloudStorage", "ColorPickers", "ColorSync", "Colors", "Components", "Compositions", "Containers", "ContainerManager", "Contextual Menu Items", "Cookies", "CoreAnalytics", "CoreData", "CoreFollowUp", "CoreMediaIO", "Daemon Containers", "DataDeliveryServices", "DataVault", "Developer", "DirectoryServices", "Dictionaries", "DoNotDisturb", "Documentation", "DriverExtensions", "DuetExpertCenter", "Extensions", "Favorites", "FileProvider", "Filesystems", "Finance", "FontCollections", "Fonts", "Frameworks", "GameKit", "GPUBundles", "Graphics", "Group Containers", "HomeKit", "HTTPStorages", "IdentityServices", "Image Capture", "Input Methods", "IntelligencePlatform", "Internet Plug-Ins", "Java", "KernelCollections", "Keyboard", "Keyboard Layouts", "KeyboardServices", "Keychains", "LanguageModeling", "LaunchAgents", "LaunchDaemons", "LockdownMode", "Logs", "Mail", "Managed Preferences", "Messages", "Metadata", "Mobile Documents", "Modem Scripts", "MonitorPanels", "Movie", "Music", "OSAnalytics", "PDF Services", "Passes", "Perl", "Personas", "Photos", "PreferencePanes", "Preferences", "Printers", "PrivilegedHelperTools", "Python", "QuickLook", "Receipts", "Reminders", "ResponseKit", "Ruby", "Safari", "SafariSafeBrowsing", "Sandbox", "Saved Application State", "Screen Savers", "ScreenRecordings", "ScriptingAdditions", "Scripts", "Security", "Services", "Sharing", "Shortcuts", "Sounds", "Speech", "Spelling", "Spotlight", "StartupItems", "Staged Extensions", "Suggestions", "SyncedPreferences", "SystemExtensions", "SystemMigration", "Translation", "Trial", "UnifiedAssetFramework", "Updates", "User Pictures", "Video", "WebKit", "WebServer", "Weather", "Widgets", "com.apple.AppleMediaServices",
];

/// Folder names that are vendors or shared homes, never one app's.
const SHARED_NAMES: &[&str] = &[
    "apple", "google", "microsoft", "adobe", "jetbrains", "mozilla", "oracle", "java", "crashreporter", "com.apple", "cloudkit", "icloud", "caches", "logs", "temp", "tmp", "default", "app", "data", "electron", "node", "python",
    "homebrew", "setapp", "steam", "unity", "autodesk", "zoom", "code", "chrome", "helper", "updater", "update", "plugins", "config", "cache", "settings", "support",
];

/// Most children read from one vendor folder.
const NESTED_MAX: usize = 200;

/// The places to look in. Tests point these at temp folders.
#[derive(Clone, Debug)]
pub struct Roots {
    /// ~/Library
    pub user_library: PathBuf,
    /// "/" (holds Library/LaunchDaemons and so on)
    pub system: PathBuf,
}

impl Roots {
    pub fn real() -> Self {
        Roots { user_library: crate::kit::fs::home().join("Library"), system: PathBuf::from("/") }
    }
    /// The home folder.
    pub fn home(&self) -> PathBuf {
        self.user_library.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// An app as far as matching cares.
#[derive(Clone, Debug, Default)]
pub struct Owner {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// Other names it goes by (Info.plist name and display name).
    pub alt: Vec<String>,
    /// App groups it is signed for ("group.com.foo", "TEAMID.com.foo").
    pub groups: Vec<String>,
    /// Developer team id from its signature.
    pub team: String,
}

impl Owner {
    /// The names a folder may carry for this app, lowercased: its names, and
    /// each without a trailing version ("Bartender 6" → "bartender").
    pub fn name_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in std::iter::once(&self.name).chain(self.alt.iter()) {
            let t = n.trim().to_lowercase();
            for k in [strip_version(&t), t.clone()] {
                if k.chars().count() >= 3 && !SHARED_NAMES.contains(&k.as_str()) && !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }
}

/// "bartender 6" → "bartender", "firefox 120.0" → "firefox".
fn strip_version(s: &str) -> String {
    let mut words: Vec<&str> = s.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|w| w.trim_start_matches('v').chars().all(|c| c.is_ascii_digit() || c == '.') && w.chars().any(|c| c.is_ascii_digit())) {
        words.pop();
    }
    words.join(" ")
}

/// Letters and digits only, lowercased ("Visual Studio Code" → "visualstudiocode").
fn compact(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A file or folder that belongs to an app.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub path: PathBuf,
    pub kind: Kind,
    pub bytes: u64,
    /// Lives outside the home folder; moving it needs the admin password.
    pub admin: bool,
    pub selected: bool,
}

#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    /// File name as on disk.
    name: String,
    /// The name without a bundle extension (".prefPane", ".plugin"…).
    stem: String,
    kind: Kind,
    by_name: bool,
    orphans: bool,
    admin: bool,
    /// Found inside a vendor folder: version digits at the end are ignored.
    nested: bool,
    /// Bundle ids the entry names inside itself.
    ids: Vec<String>,
    /// For launchd jobs: label and every string of the plist.
    job: Option<(String, Vec<String>)>,
}

/// Bundle extensions whose Info.plist names the owner.
const BUNDLE_EXTS: &[&str] = &["prefPane", "plugin", "kext", "driver", "component", "qlgenerator", "mdimporter", "saver", "bundle", "appex", "app", "workflow", "vst", "vst3"];

/// One listing of every place, reused for all apps of a scan.
#[derive(Clone, Debug, Default)]
pub struct Index {
    entries: Vec<Entry>,
}

impl Index {
    pub fn build(roots: &Roots) -> Index {
        let mut entries = Vec::new();
        let home = roots.home();
        let groups: [(&Path, &[Place], bool); 3] = [(&roots.user_library, USER_PLACES, false), (&home, HOME_PLACES, false), (&roots.system, SYSTEM_PLACES, true)];
        for (base, places, system) in groups {
            if base.as_os_str().is_empty() {
                continue;
            }
            for pl in places {
                let dir = base.join(pl.rel);
                for (path, name) in list(&dir) {
                    if pl.kind == Kind::Preferences && path.is_dir() {
                        continue;
                    }
                    let admin = system && super::remove::needs_root(&path);
                    let ids = inner_ids(&path, &name, pl.kind);
                    if pl.nested && path.is_dir() && !path.is_symlink() && !looks_like_bundle_id(&name) && !name.to_lowercase().starts_with("com.apple") && !name.eq_ignore_ascii_case("apple") {
                        nested(&mut entries, &path, pl.kind, system);
                    }
                    entries.push(Entry { stem: bundle_stem(&name), path, name, kind: pl.kind, by_name: pl.by_name, orphans: pl.orphans, admin, nested: false, ids, job: None });
                }
            }
        }
        // vendors' own folders at the top of each Library
        for (lib, system) in [(roots.user_library.clone(), false), (roots.system.join("Library"), true)] {
            for (path, name) in list(&lib) {
                if !path.is_dir() || path.is_symlink() || STANDARD_LIBRARY.iter().any(|s| s.eq_ignore_ascii_case(&name)) || name.to_lowercase().starts_with("com.apple") {
                    continue;
                }
                nested(&mut entries, &path, Kind::Other, system);
                let admin = system && super::remove::needs_root(&path);
                entries.push(Entry { stem: name.clone(), path, name, kind: Kind::Other, by_name: true, orphans: false, admin, nested: false, ids: Vec::new(), job: None });
            }
        }
        let mut jobs = |dir: PathBuf, admin: bool| {
            for (path, name) in list(&dir) {
                if !name.ends_with(".plist") {
                    continue;
                }
                let v = plist::read(&path);
                let label = v.as_ref().and_then(|v| v.get_str("Label")).unwrap_or(name.trim_end_matches(".plist")).to_string();
                let strings = v.as_ref().map(|v| v.strings().into_iter().map(String::from).collect()).unwrap_or_default();
                let ids = v.as_ref().and_then(|v| v.get("AssociatedBundleIdentifiers")).map(|a| a.strings().into_iter().map(String::from).collect()).unwrap_or_default();
                entries.push(Entry { stem: name.clone(), path, name, kind: Kind::Launch, by_name: false, orphans: false, admin, nested: false, ids, job: Some((label, strings)) });
            }
        };
        jobs(roots.user_library.join(LAUNCH_USER), false);
        for (rel, admin) in LAUNCH_SYSTEM {
            jobs(roots.system.join(rel), *admin);
        }
        for (path, name) in list(&roots.system.join(HELPERS_SYSTEM)) {
            entries.push(Entry { stem: name.clone(), path, name, kind: Kind::Launch, by_name: false, orphans: false, admin: true, nested: false, ids: Vec::new(), job: None });
        }
        Index { entries }
    }

    /// Everything that belongs to `app` and to no other app in `all`
    /// (sizes not filled in).
    pub fn for_app(&self, app: &Owner, all: &[Owner]) -> Vec<Item> {
        // what the app carries inside (extensions, helpers) owns data too;
        // matched by id only, never by name
        let inner: Vec<Owner> = if app.path.as_os_str().is_empty() { Vec::new() } else { super::apps::embedded_ids(&app.path, &app.id).into_iter().map(|id| Owner { id, ..Default::default() }).collect() };
        let mut out = Vec::new();
        for e in &self.entries {
            if !belongs(e, app, all) && !inner.iter().any(|o| belongs(e, o, all)) {
                continue;
            }
            if all.iter().any(|o| o.id != app.id && o.path != app.path && belongs(e, o, all)) {
                continue;
            }
            if is_protected(&e.path).is_some() {
                continue;
            }
            out.push(Item { path: e.path.clone(), kind: e.kind, bytes: 0, admin: e.admin, selected: true });
        }
        tidy(&mut out);
        out
    }

    /// Whether `p` is something the index knows to belong to an installed
    /// app other than `app`.
    pub fn claimed_by_other(&self, p: &Path, app: &Owner, all: &[Owner]) -> bool {
        self.entries.iter().filter(|e| e.path == p || p.starts_with(&e.path)).any(|e| all.iter().any(|o| o.id != app.id && o.path != app.path && belongs(e, o, all)))
    }

    /// launchd jobs with this label.
    pub fn jobs_labelled(&self, label: &str) -> Vec<Item> {
        self.entries
            .iter()
            .filter(|e| e.job.as_ref().is_some_and(|(l, _)| l.eq_ignore_ascii_case(label)) && is_protected(&e.path).is_none())
            .map(|e| Item { path: e.path.clone(), kind: Kind::Launch, bytes: 0, admin: e.admin, selected: true })
            .collect()
    }

    /// Data of apps that are no longer installed, grouped by the id it is
    /// named after. `known` = every bundle id still on this Mac (installed
    /// anywhere, or running); `min_age_secs`: entries touched more recently
    /// are left alone, something is still using them.
    pub fn orphans(&self, known: &HashSet<String>, now: i64, min_age_secs: i64) -> BTreeMap<String, Vec<Item>> {
        let known_lc: Vec<String> = known.iter().map(|k| k.to_lowercase()).collect();
        let mut out: BTreeMap<String, Vec<Item>> = BTreeMap::new();
        for e in &self.entries {
            let id = match &e.job {
                // a job whose program is gone, named like an app id
                Some((label, strings)) => {
                    let program = strings.iter().find(|s| s.starts_with('/'));
                    if e.admin || program.is_none_or(|p| Path::new(p).exists()) {
                        continue;
                    }
                    label.clone()
                }
                // a sandbox container named by a UUID says whose it is inside
                None if e.orphans => match id_of(&e.name).or_else(|| e.ids.first().cloned()) {
                    Some(id) => id,
                    None => continue,
                },
                None => continue,
            };
            if !looks_like_bundle_id(&id) || is_system_id(&id) {
                continue;
            }
            let lc = id.to_lowercase();
            if known_lc.iter().any(|k| related(&lc, k)) {
                continue;
            }
            if is_protected(&e.path).is_some() || newest_mtime(&e.path).is_some_and(|t| now - t < min_age_secs) {
                continue;
            }
            out.entry(id).or_default().push(Item { path: e.path.clone(), kind: e.kind, bytes: 0, admin: e.admin, selected: true });
        }
        out
    }
}

/// Sort by kind, drop duplicates and anything inside another item.
pub fn tidy(items: &mut Vec<Item>) {
    items.sort_by(|a, b| a.path.cmp(&b.path));
    let mut kept: Vec<Item> = Vec::with_capacity(items.len());
    for it in items.drain(..) {
        if kept.iter().any(|k| it.path.starts_with(&k.path)) {
            continue;
        }
        kept.push(it);
    }
    kept.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.path.cmp(&b.path)));
    *items = kept;
}

/// The children of a vendor folder as entries matched by name. When the
/// vendor folder holds just one thing, that thing is the whole folder.
fn nested(entries: &mut Vec<Entry>, vendor: &Path, kind: Kind, system: bool) {
    let kids = list(vendor);
    if kids.len() > NESTED_MAX {
        return;
    }
    let alone = kids.len() == 1;
    for (path, name) in kids {
        let at = if alone { vendor.to_path_buf() } else { path };
        let admin = system && super::remove::needs_root(&at);
        entries.push(Entry { stem: bundle_stem(&name), path: at, name, kind, by_name: true, orphans: false, admin, nested: true, ids: Vec::new(), job: None });
    }
}

/// "Foo.prefPane" → "Foo"; other names as they are.
fn bundle_stem(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && BUNDLE_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)) => stem.to_string(),
        _ => name.to_string(),
    }
}

/// Bundle ids an entry names inside itself: a plug-in's Info.plist, or the
/// metadata of a sandbox container named by a UUID.
fn inner_ids(path: &Path, name: &str, kind: Kind) -> Vec<String> {
    let mut out = Vec::new();
    if kind == Kind::Containers && is_uuid(name) {
        if let Some(id) = plist::read(&path.join(".com.apple.containermanagerd.metadata.plist")).and_then(|v| v.get_str("MCMMetadataIdentifier").map(String::from)) {
            out.push(id);
        }
    }
    if kind == Kind::Extension && bundle_stem(name) != name {
        for info in ["Contents/Info.plist", "Info.plist"] {
            if let Some(id) = plist::read(&path.join(info)).and_then(|v| v.get_str("CFBundleIdentifier").map(|s| s.trim().to_string())) {
                out.push(id);
                break;
            }
        }
    }
    out
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5 && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Whether index entry `e` is named for, or run by, `app`.
fn belongs(e: &Entry, app: &Owner, all: &[Owner]) -> bool {
    if app.id.is_empty() {
        return false;
    }
    let id = app.id.to_lowercase();
    if e.ids.iter().any(|x| under_id(&x.to_lowercase(), &id)) {
        return true;
    }
    if let Some((label, strings)) = &e.job {
        let inside = format!("{}/", app.path.display());
        return under_id(&label.to_lowercase(), &id) || (!app.path.as_os_str().is_empty() && strings.iter().any(|s| s.starts_with(&inside) || *s == app.path.display().to_string()));
    }
    let stem = id_of(&e.name).unwrap_or_else(|| e.name.clone()).to_lowercase();
    if e.kind == Kind::Launch {
        // privileged helper tools are named by id
        return under_id(&stem, &id);
    }
    if under_id(&stem, &id) || under_id(&bundle_stem(&e.name).to_lowercase(), &id) {
        return true;
    }
    if e.kind == Kind::GroupContainers {
        // the groups the app is signed for
        if app.groups.iter().any(|g| g.eq_ignore_ascii_case(&stem)) {
            return true;
        }
        // "TEAMID.com.foo.app", "group.com.foo.app", or the vendor's "group.com.foo"
        if stem.ends_with(&format!(".{id}")) {
            return true;
        }
        // "TEAMID.anything" of a team with no other app here
        let team = app.team.to_lowercase();
        if !team.is_empty() && stem.starts_with(&format!("{team}.")) && !all.iter().any(|o| o.path != app.path && o.id.to_lowercase() != id && o.team.eq_ignore_ascii_case(&team)) {
            return true;
        }
        if let Some(vendor) = vendor_of(&id) {
            let others = all.iter().any(|o| o.id.to_lowercase() != id && o.id.to_lowercase().starts_with(&format!("{vendor}.")));
            if !others && (stem == format!("group.{vendor}") || stem.ends_with(&format!(".group.{vendor}"))) {
                return true;
            }
        }
        return false;
    }
    e.by_name && name_matches(&e.stem, e.nested, app)
}

/// Whether a folder called `name` is named after `app`: exactly one of its
/// names, ignoring case, spaces and punctuation; inside a vendor folder also
/// ignoring a version at the end ("PyCharm2024.2").
fn name_matches(name: &str, nested: bool, app: &Owner) -> bool {
    let n = name.trim().to_lowercase();
    if n.chars().count() < 3 || SHARED_NAMES.contains(&n.as_str()) {
        return false;
    }
    let mut c = compact(&n);
    if nested {
        c = c.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
    }
    app.name_keys().iter().any(|k| *k == n || (c.chars().count() >= 4 && compact(k) == c))
}

/// Whether a file called `name` is named after `app`: its id (or one below
/// it, or "TEAMID.<id>"), or one of its names.
pub fn names_app(name: &str, app: &Owner) -> bool {
    let id = app.id.to_lowercase();
    let stem = id_of(name).unwrap_or_else(|| bundle_stem(name)).to_lowercase();
    (!id.is_empty() && (under_id(&stem, &id) || stem.ends_with(&format!(".{id}")))) || name_matches(&bundle_stem(name), true, app)
}

/// `name` is `id` or something below it ("com.foo.app.helper").
pub fn under_id(name: &str, id: &str) -> bool {
    name == id || (name.starts_with(id) && name.as_bytes().get(id.len()) == Some(&b'.'))
}

/// Either id is the other or below it.
fn related(a: &str, b: &str) -> bool {
    under_id(a, b) || under_id(b, a)
}

/// "com.foo" of "com.foo.app", when that still says who the vendor is.
fn vendor_of(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.split('.').collect();
    (parts.len() >= 3).then(|| parts[..parts.len() - 1].join("."))
}

/// Which kind of place `p` is in, judged by its path.
pub fn kind_of(p: &Path, roots: &Roots) -> Kind {
    let rel = p.strip_prefix(&roots.user_library).ok().or_else(|| p.strip_prefix(roots.system.join("Library")).ok());
    let Some(first) = rel.and_then(|r| r.components().next()).map(|c| c.as_os_str().to_string_lossy().into_owned()) else {
        return if p.components().any(|c| c.as_os_str() == "Caskroom") { Kind::Installed } else { Kind::Other };
    };
    match first.as_str() {
        "Application Support" => Kind::Support,
        "Caches" => Kind::Caches,
        "Preferences" => Kind::Preferences,
        "Containers" | "Application Scripts" => Kind::Containers,
        "Group Containers" => Kind::GroupContainers,
        "Saved Application State" => Kind::SavedState,
        "Logs" => Kind::Logs,
        "HTTPStorages" | "WebKit" | "Cookies" => Kind::WebData,
        "LaunchAgents" | "LaunchDaemons" | "PrivilegedHelperTools" => Kind::Launch,
        "PreferencePanes" | "Internet Plug-Ins" | "Extensions" | "Audio" | "QuickLook" | "Spotlight" | "Screen Savers" | "Services" => Kind::Extension,
        _ => Kind::Other,
    }
}

/// The id an entry is named by: its name without ".plist", ".savedState",
/// ".binarycookies", and the machine id ByHost files carry.
pub fn id_of(name: &str) -> Option<String> {
    let mut s = name;
    for suf in [".plist", ".savedState", ".binarycookies"] {
        if let Some(t) = s.strip_suffix(suf) {
            s = t;
            break;
        }
    }
    // ByHost: "com.foo.app.0123ABCD-...-....plist"
    if let Some((head, tail)) = s.rsplit_once('.') {
        if tail.len() >= 12 && tail.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            s = head;
        }
    }
    looks_like_bundle_id(s).then(|| s.to_string())
}

/// Reverse-DNS with at least three parts and a domain-like first part.
pub fn looks_like_bundle_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 3 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let tld = parts[0];
    if !(2..=6).contains(&tld.len()) || !tld.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// macOS's own ids: never orphans.
pub fn is_system_id(id: &str) -> bool {
    let l = id.to_lowercase();
    l.starts_with("com.apple.") || l.starts_with("group.com.apple.") || l.starts_with("systemgroup.")
}

/// Newest modification time in a folder (its top two levels), or of a file.
fn newest_mtime(p: &Path) -> Option<i64> {
    fn mtime(p: &Path) -> Option<i64> {
        let m = p.symlink_metadata().ok()?.modified().ok()?;
        Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
    }
    let mut best = mtime(p);
    if p.is_dir() {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten().take(500) {
            best = best.max(mtime(&e.path()));
            if e.path().is_dir() {
                for f in std::fs::read_dir(e.path()).into_iter().flatten().flatten().take(200) {
                    best = best.max(mtime(&f.path()));
                }
            }
        }
    }
    best
}

/// (path, file name) of each entry of `dir`, hidden ones skipped.
fn list(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, String)> = rd
        .flatten()
        .map(|e| (e.path(), e.file_name().to_string_lossy().into_owned()))
        .filter(|(_, n)| !n.starts_with('.'))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct Fake {
        pub root: PathBuf,
        pub roots: Roots,
    }

    impl Fake {
        pub fn new(tag: &str) -> Fake {
            let root = std::env::temp_dir().join(format!("cmb-uninst-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let roots = Roots { user_library: root.join("home/Library"), system: root.join("sys") };
            std::fs::create_dir_all(&roots.user_library).unwrap();
            Fake { root, roots }
        }
        pub fn file(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            p
        }
        pub fn lib(&self, rel: &str) -> PathBuf {
            self.file(&format!("home/Library/{rel}"), "x")
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn owner(id: &str, name: &str, path: &str) -> Owner {
        Owner { id: id.into(), name: name.into(), path: PathBuf::from(path), ..Default::default() }
    }

    fn rels(items: &[Item], f: &Fake) -> Vec<String> {
        items.iter().map(|i| i.path.strip_prefix(&f.root).unwrap().display().to_string()).collect()
    }

    #[test]
    fn ids_from_names() {
        assert_eq!(id_of("com.docker.docker.plist").as_deref(), Some("com.docker.docker"));
        assert_eq!(id_of("com.docker.docker.savedState").as_deref(), Some("com.docker.docker"));
        assert_eq!(id_of("com.foo.bar.0A1B2C3D-1111-2222-3333-444455556666.plist").as_deref(), Some("com.foo.bar"));
        assert_eq!(id_of("Slack"), None);
        assert_eq!(id_of("com.foo"), None);
        assert!(!looks_like_bundle_id("Com.Foo.bar"));
        assert!(looks_like_bundle_id("us.zoom.xos"));
        assert!(!looks_like_bundle_id("com..x"));
        assert!(is_system_id("com.apple.Safari"));
    }

    #[test]
    fn finds_an_apps_files_and_nothing_else() {
        let f = Fake::new("match");
        for rel in [
            "Application Support/Slack/x",
            "Application Support/com.tinyspeck.slackmacgap/x",
            "Caches/com.tinyspeck.slackmacgap/x",
            "Caches/com.tinyspeck.slackmacgap.ShipIt/x",
            "Preferences/com.tinyspeck.slackmacgap.plist",
            "Preferences/ByHost/com.tinyspeck.slackmacgap.0A1B2C3D-1111-2222-3333-444455556666.plist",
            "Containers/com.tinyspeck.slackmacgap/x",
            "Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap/x",
            "Saved Application State/com.tinyspeck.slackmacgap.savedState/x",
            "HTTPStorages/com.tinyspeck.slackmacgap/x",
            "Logs/Slack/x",
            // not Slack's
            "Caches/com.tinyspeck.slackmacgapX/x",
            "Caches/com.other.app/x",
            "Preferences/Slack/x",
            "Application Support/Google/x",
        ] {
            f.lib(rel);
        }
        f.file("home/Library/LaunchAgents/com.tinyspeck.updater.plist", "<plist><dict><key>Label</key><string>com.tinyspeck.updater</string><key>ProgramArguments</key><array><string>/Applications/Slack.app/Contents/Helper</string></array></dict></plist>");
        f.file("sys/Library/LaunchDaemons/com.tinyspeck.slackmacgap.helper.plist", "<plist><dict><key>Label</key><string>com.tinyspeck.slackmacgap.helper</string></dict></plist>");
        f.file("sys/Library/LaunchDaemons/com.other.plist", "<plist><dict><key>Label</key><string>com.other.daemon</string></dict></plist>");
        let idx = Index::build(&f.roots);
        let slack = owner("com.tinyspeck.slackmacgap", "Slack", "/Applications/Slack.app");
        let all = vec![slack.clone(), owner("com.other.app", "Other", "/Applications/Other.app")];
        let got = idx.for_app(&slack, &all);
        let r = rels(&got, &f);
        let want = [
            "home/Library/Application Support/Slack",
            "home/Library/Application Support/com.tinyspeck.slackmacgap",
            "home/Library/Caches/com.tinyspeck.slackmacgap",
            "home/Library/Caches/com.tinyspeck.slackmacgap.ShipIt",
            "home/Library/Preferences/com.tinyspeck.slackmacgap.plist",
            "home/Library/Preferences/ByHost/com.tinyspeck.slackmacgap.0A1B2C3D-1111-2222-3333-444455556666.plist",
            "home/Library/Containers/com.tinyspeck.slackmacgap",
            "home/Library/Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap",
            "home/Library/Saved Application State/com.tinyspeck.slackmacgap.savedState",
            "home/Library/HTTPStorages/com.tinyspeck.slackmacgap",
            "home/Library/Logs/Slack",
            "home/Library/LaunchAgents/com.tinyspeck.updater.plist",
            "sys/Library/LaunchDaemons/com.tinyspeck.slackmacgap.helper.plist",
        ];
        for w in want {
            assert!(r.iter().any(|x| x == w), "missing {w} in {r:#?}");
        }
        assert_eq!(r.len(), want.len(), "{r:#?}");
        let admin: Vec<_> = got.iter().filter(|i| i.admin).collect();
        assert_eq!(admin.len(), 1);
        assert!(got.iter().all(|i| i.selected));
    }

    #[test]
    fn data_of_extensions_and_helpers_inside_the_app_counts() {
        let f = Fake::new("embedded");
        let info = |id: &str| format!("<plist><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>");
        let app = f.root.join("apps/1Password.app");
        f.file("apps/1Password.app/Contents/Info.plist", &info("com.1password.1password"));
        f.file("apps/1Password.app/Contents/PlugIns/Safari.appex/Contents/Info.plist", &info("com.1password.safari"));
        f.file("apps/1Password.app/Contents/Library/LoginItems/Helper.app/Contents/Info.plist", &info("com.1password.browser-support"));
        // a stock helper from another vendor is never taken as the app's
        f.file("apps/1Password.app/Contents/Frameworks/Electron Helper.app/Contents/Info.plist", &info("com.github.electron.helper"));
        f.lib("Containers/com.1password.safari/x");
        f.lib("Application Support/com.1password.browser-support/x");
        f.lib("Caches/com.github.electron.helper/x");
        let idx = Index::build(&f.roots);
        let me = owner("com.1password.1password", "1Password", &app.to_string_lossy());
        let got = rels(&idx.for_app(&me, std::slice::from_ref(&me)), &f);
        assert!(got.iter().any(|p| p.ends_with("Containers/com.1password.safari")), "{got:?}");
        assert!(got.iter().any(|p| p.ends_with("Application Support/com.1password.browser-support")), "{got:?}");
        assert!(!got.iter().any(|p| p.contains("electron")), "{got:?}");
        assert_eq!(super::super::apps::embedded_ids(&app, "com.1password.1password"), vec!["com.1password.browser-support".to_string(), "com.1password.safari".to_string()]);
    }

    #[test]
    fn shared_names_and_vendor_groups_are_careful() {
        let f = Fake::new("shared");
        f.lib("Application Support/Notes/x");
        f.lib("Group Containers/group.com.docker/x");
        f.lib("Group Containers/group.com.jetbrains/x");
        let idx = Index::build(&f.roots);
        // two installed apps are both called "Notes": the folder is no one's
        let a = owner("com.a.notes", "Notes", "/Applications/Notes.app");
        let b = owner("com.b.notes", "Notes", "/Users/x/Applications/Notes.app");
        assert!(idx.for_app(&a, &[a.clone(), b.clone()]).is_empty());
        // the vendor group goes with the vendor's only app
        let docker = owner("com.docker.docker", "Docker", "/Applications/Docker.app");
        assert_eq!(idx.for_app(&docker, std::slice::from_ref(&docker)).len(), 1);
        // two apps of one vendor share it: keep
        let idea = owner("com.jetbrains.intellij", "IntelliJ IDEA", "/Applications/IntelliJ IDEA.app");
        let pyc = owner("com.jetbrains.pycharm", "PyCharm", "/Applications/PyCharm.app");
        assert!(idx.for_app(&idea, &[idea.clone(), pyc]).is_empty());
    }

    #[test]
    fn orphans_skip_known_recent_and_system() {
        let f = Fake::new("orphans");
        for rel in [
            "Caches/com.gone.app/x",
            "Preferences/com.gone.app.plist",
            "Application Support/com.gone.app/x",
            "Caches/com.installed.app/x",
            "Caches/com.installed.app.helper/x",
            "Caches/com.apple.Safari/x",
            "Caches/Homebrew/x",
            "Group Containers/ABC.com.gone.app/x",
        ] {
            f.lib(rel);
        }
        f.file("home/Library/LaunchAgents/com.gone.agent.plist", "<plist><dict><key>Label</key><string>com.gone.agent</string><key>Program</key><string>/nonexistent/gone/agent</string></dict></plist>");
        f.file("home/Library/LaunchAgents/com.alive.agent.plist", "<plist><dict><key>Label</key><string>com.alive.agent</string><key>Program</key><string>/bin/sh</string></dict></plist>");
        let idx = Index::build(&f.roots);
        let known: HashSet<String> = ["com.installed.app".to_string()].into();
        let now = chrono::Utc::now().timestamp();
        // everything was just written: too fresh
        assert!(idx.orphans(&known, now, 7 * 86400).is_empty());
        let later = now + 30 * 86400;
        let o = idx.orphans(&known, later, 7 * 86400);
        let keys: Vec<&String> = o.keys().collect();
        assert_eq!(keys, vec!["com.gone.agent", "com.gone.app"]);
        assert_eq!(o["com.gone.app"].len(), 3);
    }

    fn plist_of(pairs: &[(&str, &str)]) -> String {
        let body: String = pairs.iter().map(|(k, v)| format!("<key>{k}</key><string>{v}</string>")).collect();
        format!("<plist><dict>{body}</dict></plist>")
    }

    #[test]
    fn system_folders_and_plug_ins() {
        let f = Fake::new("system");
        f.file("sys/Library/Application Support/ClearVPN/x", "x");
        f.file("sys/Library/Application Support/Other/x", "x");
        f.file("sys/Library/Preferences/com.macpaw.clearvpn.plist", "x");
        f.file("sys/Library/Caches/com.macpaw.clearvpn/x", "x");
        f.file("sys/Library/Extensions/CVPNTunnel.kext/Contents/Info.plist", &plist_of(&[("CFBundleIdentifier", "com.macpaw.clearvpn.tunnel")]));
        f.file("sys/Library/PreferencePanes/ClearVPN.prefPane/Contents/Info.plist", &plist_of(&[("CFBundleIdentifier", "com.someone.else")]));
        f.file("sys/Library/Audio/Plug-Ins/HAL/Unrelated.driver/Contents/Info.plist", &plist_of(&[("CFBundleIdentifier", "com.unrelated.driver")]));
        let idx = Index::build(&f.roots);
        let app = owner("com.macpaw.clearvpn", "ClearVPN", "/Applications/ClearVPN.app");
        let got = idx.for_app(&app, std::slice::from_ref(&app));
        let r = rels(&got, &f);
        for w in [
            "sys/Library/Application Support/ClearVPN",
            "sys/Library/Preferences/com.macpaw.clearvpn.plist",
            "sys/Library/Caches/com.macpaw.clearvpn",
            "sys/Library/Extensions/CVPNTunnel.kext",
            "sys/Library/PreferencePanes/ClearVPN.prefPane",
        ] {
            assert!(r.iter().any(|x| x == w), "missing {w} in {r:#?}");
        }
        assert_eq!(r.len(), 5, "{r:#?}");
        assert!(got.iter().any(|i| i.kind == Kind::Extension));
        assert!(got.iter().all(|i| !i.kind.is_part() || i.kind == Kind::Extension));
    }

    #[test]
    fn uuid_containers_say_whose_they_are() {
        let f = Fake::new("uuid");
        let uuid = "0A1B2C3D-1111-2222-3333-444455556666";
        f.file(&format!("home/Library/Containers/{uuid}/.com.apple.containermanagerd.metadata.plist"), &plist_of(&[("MCMMetadataIdentifier", "com.foo.widget")]));
        f.file(&format!("home/Library/Containers/{uuid}/Data/x"), "x");
        let idx = Index::build(&f.roots);
        let foo = owner("com.foo.widget", "Widget", "/Applications/Widget.app");
        let r = rels(&idx.for_app(&foo, std::slice::from_ref(&foo)), &f);
        assert_eq!(r, vec![format!("home/Library/Containers/{uuid}")]);
        // and once the app is gone it is an orphan named by that id
        let later = chrono::Utc::now().timestamp() + 30 * 86400;
        let o = idx.orphans(&HashSet::new(), later, 7 * 86400);
        assert!(o.contains_key("com.foo.widget"), "{o:?}");
    }

    #[test]
    fn signed_groups_and_team() {
        let f = Fake::new("groups");
        f.lib("Group Containers/group.net.sharedname/x");
        f.lib("Group Containers/ABCDE12345.mystuff/x");
        f.lib("Group Containers/ZZZZZ99999.office/x");
        let idx = Index::build(&f.roots);
        let mut a = owner("com.vendor.alpha", "Alpha", "/Applications/Alpha.app");
        a.groups = vec!["group.net.sharedname".into()];
        a.team = "ABCDE12345".into();
        let r = rels(&idx.for_app(&a, std::slice::from_ref(&a)), &f);
        assert_eq!(r, vec!["home/Library/Group Containers/ABCDE12345.mystuff", "home/Library/Group Containers/group.net.sharedname"]);
        // a second app of the same team: the team's folder is shared, keep it
        let mut b = owner("com.vendor.beta", "Beta", "/Applications/Beta.app");
        b.team = "ABCDE12345".into();
        let r = rels(&idx.for_app(&a, &[a.clone(), b]), &f);
        assert_eq!(r, vec!["home/Library/Group Containers/group.net.sharedname"]);
    }

    #[test]
    fn vendor_folders_and_name_variants() {
        let f = Fake::new("vendor");
        f.lib("Application Support/JetBrains/PyCharm2024.2/x");
        f.lib("Application Support/JetBrains/IntelliJIdea2024.2/x");
        f.lib("Application Support/Bartender/x");
        f.lib("Caches/Bartender 6/x");
        f.file("sys/Library/Objective-See/LuLu/x", "x");
        let idx = Index::build(&f.roots);
        let pyc = owner("com.jetbrains.pycharm", "PyCharm", "/Applications/PyCharm.app");
        assert_eq!(rels(&idx.for_app(&pyc, std::slice::from_ref(&pyc)), &f), vec!["home/Library/Application Support/JetBrains/PyCharm2024.2"]);
        let bart = owner("com.surteesstudios.Bartender", "Bartender 6", "/Applications/Bartender 6.app");
        assert_eq!(rels(&idx.for_app(&bart, std::slice::from_ref(&bart)), &f), vec!["home/Library/Application Support/Bartender", "home/Library/Caches/Bartender 6"]);
        // the vendor folder holds only this app: the whole folder goes
        let lulu = owner("com.objective-see.lulu.app", "LuLu", "/Applications/LuLu.app");
        assert_eq!(rels(&idx.for_app(&lulu, std::slice::from_ref(&lulu)), &f), vec!["sys/Library/Objective-See"]);
        assert_eq!(strip_version("firefox 120.0"), "firefox");
        assert_eq!(strip_version("1password 7"), "1password");
        assert_eq!(strip_version("cinema 4d"), "cinema 4d");
        assert!(names_app("PyCharm2024.2", &pyc));
        assert!(names_app("com.jetbrains.pycharm.plist", &pyc));
        assert!(!names_app("JetBrains", &pyc));
        assert!(!names_app("Microsoft", &owner("com.microsoft.Word", "Microsoft Word", "/Applications/Microsoft Word.app")));
    }

    #[test]
    fn launch_jobs_naming_the_app() {
        let f = Fake::new("assoc");
        f.file("sys/Library/LaunchDaemons/com.vendor.daemon.plist", "<plist><dict><key>Label</key><string>com.vendor.daemon</string><key>AssociatedBundleIdentifiers</key><array><string>com.vendor.app</string></array><key>Program</key><string>/Library/PrivilegedHelperTools/x</string></dict></plist>");
        f.file("sys/Library/LaunchDaemons/com.vendor.other.plist", "<plist><dict><key>Label</key><string>com.vendor.other</string><key>AssociatedBundleIdentifiers</key><string>com.vendor.second</string></dict></plist>");
        let idx = Index::build(&f.roots);
        let app = owner("com.vendor.app", "App", "/Applications/App.app");
        assert_eq!(rels(&idx.for_app(&app, std::slice::from_ref(&app)), &f), vec!["sys/Library/LaunchDaemons/com.vendor.daemon.plist"]);
        assert_eq!(idx.jobs_labelled("com.vendor.other").len(), 1);
    }

    #[test]
    fn kinds_by_path() {
        let f = Fake::new("kinds");
        assert_eq!(kind_of(&f.roots.user_library.join("Caches/x"), &f.roots), Kind::Caches);
        assert_eq!(kind_of(&f.roots.system.join("Library/LaunchDaemons/x.plist"), &f.roots), Kind::Launch);
        assert_eq!(kind_of(&f.roots.system.join("Library/Audio/Plug-Ins/HAL/x.driver"), &f.roots), Kind::Extension);
        assert_eq!(kind_of(Path::new("/opt/homebrew/Caskroom/zoom"), &f.roots), Kind::Installed);
        assert_eq!(kind_of(&f.roots.home().join(".docker"), &f.roots), Kind::Other);
    }
}
