//! What the folder listing alone can't tell: the app's signature (team and
//! app groups), the installer packages and Homebrew cask that put it in
//! place, a Spotlight look for files named after it elsewhere in the home
//! folder, and the processes still running from inside it.
//!
//! The parsing is plain functions (tested anywhere); asking macOS
//! (`codesign`, `pkgutil`, `mdfind`, `ps`) only happens on macOS.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::apps::Cask;
use super::leftovers::{id_of, kind_of, under_id, Item, Owner, Roots};
use crate::kit::fs::is_protected;
use crate::kit::plist;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- signature

/// Team id and app groups from an app's code signature.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sign {
    pub team: String,
    pub groups: Vec<String>,
}

/// From `codesign -dv --entitlements - --xml`: the entitlements plist on
/// stdout, "TeamIdentifier=…" among the lines on stderr.
pub fn parse_codesign(stdout: &str, stderr: &str) -> Sign {
    let team = stderr.lines().find_map(|l| l.trim().strip_prefix("TeamIdentifier=")).map(str::trim).filter(|t| !t.is_empty() && *t != "not set").unwrap_or("").to_string();
    let groups = plist::parse(stdout)
        .and_then(|v| v.get("com.apple.security.application-groups").map(|g| g.strings().into_iter().map(String::from).collect()))
        .unwrap_or_default();
    Sign { team, groups }
}

/// The signature of `app`, read once per version of the app.
#[cfg(target_os = "macos")]
pub fn signing(app: &Path) -> Sign {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<PathBuf, (Option<std::time::SystemTime>, Sign)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let stamp = app.join("Contents/Info.plist").metadata().and_then(|m| m.modified()).ok();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((s, sign)) = cache.lock().ok().and_then(|c| c.get(app).cloned()) {
        if s == stamp {
            return sign;
        }
    }
    let sign = match std::process::Command::new("/usr/bin/codesign").args(["-dv", "--entitlements", "-", "--xml"]).arg(app).output() {
        Ok(o) => parse_codesign(&String::from_utf8_lossy(&o.stdout), &String::from_utf8_lossy(&o.stderr)),
        Err(_) => Sign::default(),
    };
    if let Ok(mut c) = cache.lock() {
        c.insert(app.to_path_buf(), (stamp, sign.clone()));
    }
    sign
}

#[cfg(not(target_os = "macos"))]
pub fn signing(_app: &Path) -> Sign {
    Sign::default()
}

// ---------------------------------------------------------------- packages

/// Where installer receipts live, under the system root.
const RECEIPTS: &str = "private/var/db/receipts";

/// Where a package's files count as the app's. Each file there is offered
/// as the folder or file right below one of these.
const PKG_PARENTS: &[&str] = &[
    "Applications",
    "Library/Application Support",
    "Library/Audio/Plug-Ins/Components",
    "Library/Audio/Plug-Ins/HAL",
    "Library/Caches",
    "Library/Extensions",
    "Library/Frameworks",
    "Library/Internet Plug-Ins",
    "Library/LaunchAgents",
    "Library/LaunchDaemons",
    "Library/Logs",
    "Library/PreferencePanes",
    "Library/Preferences",
    "Library/PrivilegedHelperTools",
    "Library/QuickLook",
    "Library/Screen Savers",
    "Library/Spotlight",
    "usr/local/bin",
    "usr/local/etc",
    "usr/local/lib",
    "usr/local/libexec",
    "usr/local/sbin",
    "usr/local/share",
];

/// Ids of the installer packages this Mac has receipts for (not Apple's).
pub fn receipts(system: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(system.join(RECEIPTS))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".plist").map(String::from))
        .filter(|id| !super::leftovers::is_system_id(id))
        .collect();
    out.sort();
    out
}

/// Packages that may have installed `app`: named by its id, its vendor, or
/// its name. Each is confirmed later by its file list.
pub fn pkg_candidates(app: &Owner, receipts: &[String]) -> Vec<String> {
    let id = app.id.to_lowercase();
    let parts: Vec<&str> = id.split('.').collect();
    let vendor = (parts.len() >= 3).then(|| format!("{}.{}.", parts[0], parts[1]));
    let name: String = app.name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    receipts
        .iter()
        .filter(|r| {
            let r = r.to_lowercase();
            under_id(&r, &id) || under_id(&id, &r) || vendor.as_ref().is_some_and(|v| r.starts_with(v.as_str())) || (name.chars().count() >= 4 && r.replace(['-', '_'], "").contains(&name))
        })
        .take(8)
        .cloned()
        .collect()
}

/// What package `files` (relative to its install `location`) put outside
/// `app`, if the package did install `app`: the top item of each, right
/// below one of `PKG_PARENTS`. None when the package isn't the app's.
pub fn pkg_items(app: &Path, location: &str, files: &[String], system: &Path) -> Option<Vec<PathBuf>> {
    let app_rel = app.strip_prefix(system).ok()?;
    let base = PathBuf::from(location.trim_matches('/'));
    let rels: Vec<PathBuf> = files.iter().map(|f| base.join(f.trim_start_matches("./").trim_start_matches('/'))).collect();
    if !rels.iter().any(|r| r == app_rel) {
        return None;
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for r in &rels {
        if r.starts_with(app_rel) {
            continue;
        }
        let Some(parent) = PKG_PARENTS.iter().filter(|p| r.starts_with(p)).max_by_key(|p| p.len()) else { continue };
        let Some(first) = r.strip_prefix(parent).ok().and_then(|rest| rest.components().next()) else { continue };
        let top = Path::new(parent).join(first);
        // another app the package put in /Applications: only bundles
        if *parent == "Applications" && top.extension().is_none_or(|e| e != "app") {
            continue;
        }
        let abs = system.join(&top);
        if !out.contains(&abs) {
            out.push(abs);
        }
    }
    out.sort();
    Some(out)
}

/// Install location and file list of a package, from `pkgutil` (asked
/// once per install of the package).
#[cfg(target_os = "macos")]
pub fn pkg_payload(id: &str) -> Option<(String, Vec<String>)> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Payload = Option<(String, Vec<String>)>;
    type Cache = Mutex<HashMap<String, (Option<std::time::SystemTime>, Payload)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let stamp = Path::new("/").join(RECEIPTS).join(format!("{id}.plist")).metadata().and_then(|m| m.modified()).ok();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((s, p)) = cache.lock().ok().and_then(|c| c.get(id).cloned()) {
        if s == stamp {
            return p;
        }
    }
    let p = pkg_payload_now(id);
    if let Ok(mut c) = cache.lock() {
        c.insert(id.to_string(), (stamp, p.clone()));
    }
    p
}

#[cfg(target_os = "macos")]
fn pkg_payload_now(id: &str) -> Option<(String, Vec<String>)> {
    let info = run_timeout(std::process::Command::new("/usr/sbin/pkgutil").args(["--pkg-info", id]), 10)?;
    let location = info.lines().find_map(|l| l.strip_prefix("location:")).map(|s| s.trim().to_string()).unwrap_or_default();
    let files = run_timeout(std::process::Command::new("/usr/sbin/pkgutil").args(["--files", id]), 20)?;
    Some((location, files.lines().map(String::from).filter(|l| !l.is_empty()).collect()))
}

#[cfg(not(target_os = "macos"))]
pub fn pkg_payload(_id: &str) -> Option<(String, Vec<String>)> {
    None
}

// ---------------------------------------------------------------- Homebrew

/// What a cask says to remove when it is uninstalled or zapped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaskPlan {
    /// Files and folders that exist now.
    pub paths: Vec<PathBuf>,
    /// launchd job labels.
    pub labels: Vec<String>,
    /// Installer package ids.
    pub pkgs: Vec<String>,
}

/// The cask that installed the app with this file name ("Zoom.app").
pub fn cask_for<'a>(app_file: &str, casks: &'a [Cask]) -> Option<&'a Cask> {
    casks.iter().find(|c| c.apps.iter().any(|a| a == app_file))
}

/// The cask's own definition, as Homebrew saved it at install time.
pub fn cask_json(cask: &Cask) -> Option<serde_json::Value> {
    let mut found: Vec<PathBuf> = Vec::new();
    for ver in std::fs::read_dir(cask.dir.join(".metadata")).into_iter().flatten().flatten() {
        for stamp in std::fs::read_dir(ver.path()).into_iter().flatten().flatten() {
            let p = stamp.path().join("Casks").join(format!("{}.json", cask.token));
            if p.is_file() {
                found.push(p);
            }
        }
    }
    found.sort();
    let text = std::fs::read_to_string(found.last()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read the "uninstall" and "zap" stanzas of a cask definition.
pub fn cask_plan(json: &serde_json::Value, roots: &Roots) -> CaskPlan {
    use serde_json::Value;
    fn strings(v: &Value) -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().flat_map(strings).collect(),
            _ => Vec::new(),
        }
    }
    let mut plan = CaskPlan::default();
    let Some(artifacts) = json.get("artifacts").and_then(Value::as_array) else { return plan };
    for art in artifacts {
        for key in ["uninstall", "zap"] {
            let Some(stanza) = art.get(key) else { continue };
            let blocks: Vec<&Value> = match stanza {
                Value::Array(a) => a.iter().collect(),
                v => vec![v],
            };
            for b in blocks {
                for field in ["trash", "delete"] {
                    for p in b.get(field).map(strings).unwrap_or_default() {
                        for found in expand_glob(&p, roots) {
                            if !plan.paths.contains(&found) {
                                plan.paths.push(found);
                            }
                        }
                    }
                }
                plan.labels.extend(b.get("launchctl").map(strings).unwrap_or_default());
                // only plain ids; patterns could reach other packages
                plan.pkgs.extend(b.get("pkgutil").map(strings).unwrap_or_default().into_iter().filter(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))));
            }
        }
    }
    plan.labels.dedup();
    plan.pkgs.dedup();
    plan
}

/// "~/Library/Caches/com.foo*" → the files that exist. A "*" may only be
/// in the last part of the path.
fn expand_glob(p: &str, roots: &Roots) -> Vec<PathBuf> {
    let path = super::rules::expand(p.trim_end_matches('/'), roots);
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return Vec::new() };
    let Some(parent) = path.parent() else { return Vec::new() };
    if parent.to_string_lossy().contains('*') {
        return Vec::new();
    }
    if !name.contains('*') {
        return if path.symlink_metadata().is_ok() { vec![path] } else { Vec::new() };
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(parent).into_iter().flatten().flatten().filter(|e| wild(&name, &e.file_name().to_string_lossy())).map(|e| e.path()).collect();
    out.sort();
    out
}

/// Shell-style match with "*" only.
fn wild(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s.ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

// ---------------------------------------------------------------- Spotlight

/// Spotlight finds named exactly after the app (its id, or one of its
/// names) in the home folder, outside what was already found. Offered
/// unticked: they are a guess.
pub fn spotlight_pick(paths: &[PathBuf], app: &Owner, roots: &Roots, found: &[Item]) -> Vec<PathBuf> {
    let id = app.id.to_lowercase();
    let keys = app.name_keys();
    let home = roots.home();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let named = id_of(&name).is_some_and(|i| under_id(&i.to_lowercase(), &id)) || keys.contains(&name.to_lowercase());
        let elsewhere = !p.starts_with(&app.path) && !p.starts_with(home.join(".Trash")) && !found.iter().any(|f| p.starts_with(&f.path)) && p.extension().is_none_or(|e| e != "app");
        if named && elsewhere && p.starts_with(&home) && is_protected(p).is_none() && !out.iter().any(|o| p.starts_with(o)) {
            out.retain(|o| !o.starts_with(p));
            out.push(p.clone());
        }
    }
    out
}

/// Items for Spotlight finds, unticked.
pub fn spotlight_items(paths: Vec<PathBuf>, roots: &Roots) -> Vec<Item> {
    paths.into_iter().map(|p| Item { kind: kind_of(&p, roots), path: p, bytes: 0, admin: false, selected: false }).collect()
}

/// Ask Spotlight, once for all of `apps`, about files in the home folder
/// named after any of them. `spotlight_pick` sorts out whose each one is.
#[cfg(target_os = "macos")]
pub fn spotlight(apps: &[Owner], roots: &Roots) -> Vec<PathBuf> {
    let clean = |s: &str| !s.is_empty() && !s.contains(['"', '\'', '\\', '*']);
    let mut terms: Vec<String> = Vec::new();
    for app in apps {
        if clean(&app.id) {
            terms.push(format!("kMDItemFSName == \"{}*\"c", app.id));
        }
        for n in app.name_keys() {
            if clean(&n) {
                terms.push(format!("kMDItemFSName == \"{n}\"c"));
            }
        }
    }
    if terms.is_empty() {
        return Vec::new();
    }
    let out = run_timeout(std::process::Command::new("/usr/bin/mdfind").arg("-onlyin").arg(roots.home()).arg(terms.join(" || ")), 15).unwrap_or_default();
    out.lines().take(20_000).map(PathBuf::from).collect()
}

#[cfg(not(target_os = "macos"))]
pub fn spotlight(_apps: &[Owner], _roots: &Roots) -> Vec<PathBuf> {
    Vec::new()
}

// ---------------------------------------------------------------- processes

/// (pid, uid) of processes whose program is inside `app`, from
/// `ps -axww -o pid=,uid=,comm=`.
pub fn parse_ps(out: &str, app: &Path) -> Vec<(i32, u32)> {
    let inside = format!("{}/", app.display());
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid: i32 = it.next()?.parse().ok()?;
            let uid: u32 = it.next()?.parse().ok()?;
            let rest = l.trim_start();
            let rest = rest[rest.find(char::is_whitespace)?..].trim_start();
            let comm = rest[rest.find(char::is_whitespace)?..].trim_start();
            comm.starts_with(&inside).then_some((pid, uid))
        })
        .filter(|(pid, _)| *pid > 1 && *pid as u32 != std::process::id())
        .collect()
}

/// Processes running from inside `app` (helpers, agents, the app itself).
#[cfg(target_os = "macos")]
pub fn procs_inside(app: &Path) -> Vec<(i32, u32)> {
    let out = run_timeout(std::process::Command::new("/bin/ps").args(["-axww", "-o", "pid=,uid=,comm="]), 5).unwrap_or_default();
    parse_ps(&out, app)
}

#[cfg(not(target_os = "macos"))]
pub fn procs_inside(_app: &Path) -> Vec<(i32, u32)> {
    Vec::new()
}

/// Run a command, giving up after `secs`. Its stdout, when it finished.
fn run_timeout(cmd: &mut std::process::Command, secs: u64) -> Option<String> {
    use std::io::Read;
    let mut child = cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed().as_secs() < secs => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    reader.join().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::leftovers::tests::Fake;
    use super::super::leftovers::Kind;

    #[test]
    fn codesign_output() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.application-groups</key><array><string>ABCDE12345.com.foo</string><string>group.com.foo.shared</string></array></dict></plist>"#;
        let err = "Executable=/Applications/Foo.app/Contents/MacOS/Foo\nIdentifier=com.foo.app\nTeamIdentifier=ABCDE12345\n";
        assert_eq!(parse_codesign(xml, err), Sign { team: "ABCDE12345".into(), groups: vec!["ABCDE12345.com.foo".into(), "group.com.foo.shared".into()] });
        assert_eq!(parse_codesign("", "TeamIdentifier=not set\n"), Sign::default());
    }

    #[test]
    fn packages() {
        let f = Fake::new("pkg");
        let app = f.roots.system.join("Applications/ClearVPN.app");
        let files: Vec<String> = [
            "Applications/ClearVPN.app",
            "Applications/ClearVPN.app/Contents/Info.plist",
            "Library/LaunchDaemons/com.macpaw.clearvpn.helper.plist",
            "Library/PrivilegedHelperTools/com.macpaw.clearvpn.helper",
            "Library/Application Support/ClearVPN/config/a",
            "Library/Application Support/ClearVPN/config/b",
            "usr/local/bin/clearvpn",
            "Library",
            "private/etc/hosts",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let got = pkg_items(&app, "/", &files, &f.roots.system).unwrap();
        let rel: Vec<String> = got.iter().map(|p| p.strip_prefix(&f.roots.system).unwrap().display().to_string()).collect();
        assert_eq!(rel, vec!["Library/Application Support/ClearVPN", "Library/LaunchDaemons/com.macpaw.clearvpn.helper.plist", "Library/PrivilegedHelperTools/com.macpaw.clearvpn.helper", "usr/local/bin/clearvpn"]);
        // a package that didn't install this app
        assert!(pkg_items(&app, "/", &files[2..], &f.roots.system).is_none());
        // a relative install location
        let rel_files = vec!["ClearVPN.app".to_string(), "ClearVPN.app/Contents".to_string()];
        assert_eq!(pkg_items(&app, "Applications", &rel_files, &f.roots.system), Some(vec![]));
        let me = Owner { id: "com.macpaw.clearvpn".into(), name: "ClearVPN".into(), ..Default::default() };
        let receipts = vec!["com.macpaw.clearvpn.pkg".to_string(), "com.macpaw.other".to_string(), "com.other.clearvpninstaller".to_string(), "org.nothing".to_string()];
        assert_eq!(pkg_candidates(&me, &receipts), vec!["com.macpaw.clearvpn.pkg", "com.macpaw.other", "com.other.clearvpninstaller"]);
    }

    #[test]
    fn cask_stanzas() {
        let f = Fake::new("cask");
        f.lib("Application Support/zoom.us/x");
        f.lib("Caches/us.zoom.xos/x");
        f.lib("Caches/us.zoom.other/x");
        f.lib("Preferences/us.zoom.xos.plist");
        let json: serde_json::Value = serde_json::from_str(
            r#"{"token":"zoom","artifacts":[
                {"uninstall":[{"launchctl":"us.zoom.ZoomDaemon","signal":["KILL","us.zoom.xos"],"pkgutil":["us.zoom.pkg.videomeeting","us.zoom.*"],"delete":["/Applications/zoom.us.app"]}]},
                {"pkg":["Zoom.pkg"]},
                {"zap":[{"trash":["~/Library/Application Support/zoom.us","~/Library/Caches/us.zoom.*","~/Library/Preferences/us.zoom.xos.plist","~/Library/Missing"]}]}
            ]}"#,
        )
        .unwrap();
        let plan = cask_plan(&json, &f.roots);
        let rel: Vec<String> = plan.paths.iter().map(|p| p.strip_prefix(&f.root).unwrap().display().to_string()).collect();
        assert_eq!(rel, vec!["home/Library/Application Support/zoom.us", "home/Library/Caches/us.zoom.other", "home/Library/Caches/us.zoom.xos", "home/Library/Preferences/us.zoom.xos.plist"]);
        assert_eq!(plan.labels, vec!["us.zoom.ZoomDaemon"]);
        assert_eq!(plan.pkgs, vec!["us.zoom.pkg.videomeeting"]);
        assert!(wild("us.zoom.*", "us.zoom.xos"));
        assert!(wild("a*b*c", "aXbYc"));
        assert!(!wild("a*b", "ab_c"));
        assert!(!wild("ab*ba", "aba"));
    }

    #[test]
    fn cask_definition_on_disk() {
        let f = Fake::new("caskdisk");
        f.file("sys/opt/homebrew/Caskroom/zoom/6.2.5/zoom.us.app", "");
        f.file("sys/opt/homebrew/Caskroom/zoom/.metadata/6.2.5/20261101120000.000/Casks/zoom.json", r#"{"token":"zoom","artifacts":[]}"#);
        let casks = super::super::apps::casks(&f.roots.system);
        assert_eq!(casks.len(), 1);
        let c = cask_for("zoom.us.app", &casks).unwrap();
        assert_eq!(c.token, "zoom");
        assert!(cask_json(c).is_some());
        assert!(cask_for("Other.app", &casks).is_none());
    }

    #[test]
    fn spotlight_finds() {
        let f = Fake::new("spot");
        let home = f.roots.home();
        let app = Owner { id: "com.foo.bar".into(), name: "Foobar".into(), path: PathBuf::from("/Applications/Foobar.app"), ..Default::default() };
        let found = vec![Item { path: f.roots.user_library.join("Caches/com.foo.bar"), kind: Kind::Caches, bytes: 0, admin: false, selected: true }];
        let paths = vec![
            home.join(".foobar"),
            home.join("Downloads/Foobar"),
            home.join("Downloads/Foobar/inner/com.foo.bar.plist"),
            home.join("Library/Caches/com.foo.bar/x/com.foo.bar"),
            home.join(".Trash/Foobar"),
            home.join("Downloads/Foobar.app"),
            home.join("Downloads/Foobarbaz"),
            PathBuf::from("/tmp/Foobar"),
            home.join("Library/Application Support/com.foo.bar.helper"),
        ];
        let got = spotlight_pick(&paths, &app, &f.roots, &found);
        assert_eq!(got, vec![home.join("Downloads/Foobar"), home.join("Library/Application Support/com.foo.bar.helper")]);
        assert!(spotlight_items(got, &f.roots).iter().all(|i| !i.selected));
    }

    #[test]
    fn processes_inside_an_app() {
        let ps = "  101   501 /Applications/Foo.app/Contents/MacOS/Foo\n  202     0 /Applications/Foo.app/Contents/Library/LaunchServices/com.foo.helper\n  303   501 /Applications/Foo Bar.app/Contents/MacOS/Foo Bar\n  404   501 /usr/libexec/thing\n   1     0 /sbin/launchd\n";
        assert_eq!(parse_ps(ps, Path::new("/Applications/Foo.app")), vec![(101, 501), (202, 0)]);
        assert_eq!(parse_ps(ps, Path::new("/Applications/Foo Bar.app")), vec![(303, 501)]);
    }

    #[test]
    fn commands_time_out() {
        assert_eq!(run_timeout(std::process::Command::new("echo").arg("hi"), 5).as_deref(), Some("hi\n"));
        assert!(run_timeout(std::process::Command::new("sleep").arg("5"), 1).is_none());
    }
}
