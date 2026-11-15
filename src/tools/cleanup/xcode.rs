//! Xcode leftovers: build data named after its project, device support kept
//! per OS, archives kept per app, and simulators whose runtime is gone.

use super::engine::{Action, Ctx, Group, Verdict};
use super::run;
use crate::kit::fs::home;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn scan(c: &mut Ctx) {
    let x = home().join("Library/Developer");
    let xcode_open = c.running_ids.contains("com.apple.dt.xcode");
    // Derived Data, named after the project it was built from
    for p in super::engine::children(&x.join("Xcode/DerivedData")) {
        if !p.is_dir() || super::engine::fname(&p) == "ModuleCache.noindex" {
            continue;
        }
        let ws = workspace_of(&p);
        let name = ws.as_ref().and_then(|w| w.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| derived_data_name(&super::engine::fname(&p)));
        let gone = ws.as_ref().is_some_and(|w| !w.exists());
        let (v, why) = if xcode_open {
            (Verdict::Ask, "Xcode is open — quit it first or allow anyway".to_string())
        } else if gone {
            (Verdict::Safe, "Its project no longer exists".to_string())
        } else {
            (Verdict::Safe, "Build products and index; Xcode rebuilds them".to_string())
        };
        c.add(Group::Developer, &format!("Derived Data · {name}"), p, "Build Data", v, &why);
        if xcode_open {
            if let Some(it) = c.items.last_mut() {
                it.app_running = true;
            }
        }
    }
    c.add(Group::Developer, "Xcode Caches", x.join("Xcode/DocumentationCache"), "Caches", Verdict::Safe, "Documentation cache");
    c.add(Group::Developer, "Swift module cache", x.join("Xcode/ModuleCache.noindex"), "Caches", Verdict::Safe, "Precompiled module cache");
    c.add(Group::Developer, "SwiftUI Previews", x.join("Xcode/UserData/Previews"), "Caches", Verdict::Safe, "Preview simulator data");
    c.add(Group::Developer, "iOS Device Logs", x.join("Xcode/iOS Device Logs"), "Logs", Verdict::Safe, "Device logs collected by Xcode");
    c.add(Group::Developer, "Simulator caches", x.join("CoreSimulator/Caches"), "Caches", Verdict::Safe, "Simulator dyld caches; rebuilt on boot");
    // device support: the newest version of each OS stays on offer as a question
    for os in ["iOS DeviceSupport", "watchOS DeviceSupport", "macOS DeviceSupport", "tvOS DeviceSupport", "visionOS DeviceSupport"] {
        let label = os.replace(" DeviceSupport", "");
        let found: Vec<(PathBuf, Option<DsVersion>)> = super::engine::children(&x.join("Xcode").join(os)).into_iter().filter(|p| p.is_dir()).map(|p| (p.clone(), DsVersion::parse(&super::engine::fname(&p)))).collect();
        let newest = found.iter().filter_map(|(_, v)| v.as_ref()).max().cloned();
        for (p, v) in found {
            let (verdict, why) = match &v {
                None => (Verdict::Ask, "Couldn't tell which OS version this is for"),
                Some(v) if Some(v) == newest.as_ref() => (Verdict::Ask, "Newest device support — needed to debug on that device"),
                Some(_) => (Verdict::Safe, "Support files for an older OS version; recreated when you connect that device"),
            };
            c.add(Group::Developer, &format!("{label} {}", super::engine::fname(&p)), p, "Device Support", verdict, why);
        }
    }
    scan_archives(c, &x.join("Xcode/Archives"));
}

/// "MyApp-abcdefghijk" → "MyApp".
fn derived_data_name(folder: &str) -> String {
    match folder.rsplit_once('-') {
        Some((name, hash)) if !name.is_empty() && hash.len() >= 20 && hash.chars().all(|c| c.is_ascii_alphanumeric()) => name.to_string(),
        _ => folder.to_string(),
    }
}

/// The .xcodeproj / .xcworkspace / package a Derived Data folder was built from.
fn workspace_of(dir: &Path) -> Option<PathBuf> {
    let info = crate::kit::plist::read(&dir.join("info.plist"))?;
    let ws = info.get_str("WorkspacePath")?;
    (!ws.is_empty() && ws.starts_with('/')).then(|| PathBuf::from(ws))
}

/// A device support folder's OS version: "17.2 (21C62)", "17.2 (21C62) arm64e",
/// "iPhone16,2 17.2 (21C62)", "Watch6,1 10.2 (21S364)", "14.2".
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DsVersion {
    version: Vec<u32>,
    build: String,
}

impl DsVersion {
    pub fn parse(name: &str) -> Option<DsVersion> {
        let toks: Vec<&str> = name.split_whitespace().collect();
        let num = |t: &str| -> Option<Vec<u32>> {
            if t.is_empty() || !t.starts_with(|c: char| c.is_ascii_digit()) || !t.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return None;
            }
            t.split('.').map(|x| x.parse().ok()).collect()
        };
        // the version comes first, or second after a device model
        let (vi, version) = match toks.first().and_then(|t| num(t)) {
            Some(v) => (0, v),
            None => (1, toks.get(1).and_then(|t| num(t))?),
        };
        let build = toks.get(vi + 1).filter(|t| t.starts_with('(')).map(|t| t.trim_matches(|c| c == '(' || c == ')').to_string()).unwrap_or_default();
        Some(DsVersion { version, build: build_key(&build) })
    }
}

/// Apple builds compare by number, letter, number ("21C62" < "21C101").
fn build_key(b: &str) -> String {
    let digits_first: String = b.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &b[digits_first.len()..];
    let letters: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let tail = &rest[letters.len()..];
    let tail_num: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    format!("{:0>4}{}{:0>8}{}", digits_first, letters, tail_num, &tail[tail_num.len()..])
}

// ---- archives: grouped by app, the newest of each app stays

struct Archive {
    path: PathBuf,
    app: String,
    label: String,
    when: std::time::SystemTime,
}

fn read_archive(p: &Path) -> Archive {
    let info = crate::kit::plist::read(&p.join("Info.plist"));
    let props = info.as_ref().and_then(|i| i.get("ApplicationProperties"));
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // "MyApp 12.11.2026, 10.21" → "MyApp"
    let from_name = stem.split_once(' ').map(|(a, _)| a.to_string()).unwrap_or(stem.clone());
    let name = info.as_ref().and_then(|i| i.get_str("Name")).map(str::to_string).unwrap_or(from_name);
    let app = props.and_then(|p| p.get_str("CFBundleIdentifier")).map(str::to_string).unwrap_or_else(|| name.clone());
    let ver = props.and_then(|p| p.get_str("CFBundleShortVersionString"));
    let build = props.and_then(|p| p.get_str("CFBundleVersion"));
    let label = match (ver, build) {
        (Some(v), Some(b)) => format!("{name} {v} ({b})"),
        (Some(v), None) => format!("{name} {v}"),
        _ => stem,
    };
    let when = p.symlink_metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    Archive { path: p.to_path_buf(), app, label, when }
}

fn scan_archives(c: &mut Ctx, root: &Path) {
    for (a, v, why) in archive_plan(root) {
        c.add(Group::Developer, &format!("Archive · {}", a.label), a.path, "Archives", v, why);
    }
}

fn archive_plan(root: &Path) -> Vec<(Archive, Verdict, &'static str)> {
    let mut by_app: BTreeMap<String, Vec<Archive>> = BTreeMap::new();
    for day in super::engine::children(root) {
        for a in super::engine::children(&day) {
            if a.extension().is_some_and(|e| e == "xcarchive") {
                let ar = read_archive(&a);
                by_app.entry(ar.app.clone()).or_default().push(ar);
            }
        }
    }
    let mut out = Vec::new();
    for (_, mut list) in by_app {
        list.sort_by_key(|a| std::cmp::Reverse(a.when));
        for (i, a) in list.into_iter().enumerate() {
            let (v, why) = if i == 0 { (Verdict::Keep, "Latest archive of this app — keeps the debug symbols of what you shipped") } else { (Verdict::Ask, "Older archive; the latest one of this app is kept") };
            out.push((a, v, why));
        }
    }
    out
}

// ---- simulators (through simctl, only when Xcode's tools are installed)

fn xcrun() -> Option<PathBuf> {
    // `xcrun` without developer tools pops up an install prompt: check first
    let dev = std::process::Command::new("/usr/bin/xcode-select").arg("-p").output().ok()?;
    if !dev.status.success() || !home().join("Library/Developer/CoreSimulator").is_dir() {
        return None;
    }
    run::find_tool(&["/usr/bin/xcrun"])
}

pub(super) fn scan_simulators(c: &mut Ctx) {
    let Some(xcrun) = xcrun() else { return };
    if let Some(o) = run::output(std::process::Command::new(&xcrun).args(["simctl", "list", "devices", "-j"]), Duration::from_secs(20)) {
        for d in unavailable_devices(&o.stdout) {
            let dir = home().join("Library/Developer/CoreSimulator/Devices").join(&d.udid);
            let why = if d.why.is_empty() { "Its runtime is gone, so it can't start".to_string() } else { format!("Can't start: {}", d.why) };
            c.add(Group::Developer, &format!("Simulator · {}", d.name), dir, "Simulators", Verdict::Safe, &why);
            if let Some(it) = c.items.last_mut().filter(|i| i.kind == "Simulators") {
                it.action = Action::SimDevice(d.udid.clone());
            }
        }
    }
    if let Some(o) = run::output(std::process::Command::new(&xcrun).args(["simctl", "runtime", "list", "-j"]), Duration::from_secs(20)) {
        for r in old_runtimes(&o.stdout) {
            let path = PathBuf::from(format!("/Library/Developer/CoreSimulator/Volumes/{}", r.id));
            c.add_action(Group::Developer, &format!("Simulator runtime · {} {}", r.platform, r.version), path, "Simulators", Verdict::Ask, &format!("Older {} runtime; Xcode downloads it again if a project needs it", r.platform), Action::SimRuntime(r.id.clone()), r.bytes);
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct SimDevice {
    pub udid: String,
    pub name: String,
    pub why: String,
}

/// Devices `simctl list devices -j` marks unavailable.
pub fn unavailable_devices(json: &[u8]) -> Vec<SimDevice> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(json) else { return vec![] };
    let mut out = Vec::new();
    for (_, list) in v["devices"].as_object().into_iter().flatten() {
        for d in list.as_array().into_iter().flatten() {
            let (Some(udid), Some(false)) = (d["udid"].as_str(), d["isAvailable"].as_bool()) else { continue };
            // a udid is only ever hex and dashes: it becomes a folder name
            if udid.is_empty() || !udid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                continue;
            }
            out.push(SimDevice { udid: udid.into(), name: d["name"].as_str().unwrap_or("Simulator").into(), why: d["availabilityError"].as_str().unwrap_or("").into() });
        }
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Runtime {
    pub id: String,
    pub platform: String,
    pub version: String,
    pub bytes: u64,
}

/// Deletable runtimes from `simctl runtime list -j`, except the newest of each
/// platform.
pub fn old_runtimes(json: &[u8]) -> Vec<Runtime> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(json) else { return vec![] };
    let mut all: Vec<(Vec<u32>, Runtime, bool)> = Vec::new();
    for (key, r) in v.as_object().into_iter().flatten() {
        let id = r["identifier"].as_str().unwrap_or(key).to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            continue;
        }
        let version = r["version"].as_str().unwrap_or("").to_string();
        let platform = platform_name(r["platformIdentifier"].as_str().unwrap_or(""));
        let key: Vec<u32> = version.split('.').filter_map(|x| x.parse().ok()).collect();
        let deletable = r["deletable"].as_bool().unwrap_or(false) && r["state"].as_str().is_none_or(|s| s == "Ready");
        all.push((key, Runtime { id, platform, version, bytes: r["sizeBytes"].as_u64().unwrap_or(0) }, deletable));
    }
    let mut newest: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (k, r, _) in &all {
        let e = newest.entry(r.platform.clone()).or_default();
        if k > e {
            *e = k.clone();
        }
    }
    let mut out: Vec<Runtime> = all.into_iter().filter(|(k, r, del)| *del && newest.get(&r.platform) != Some(k)).map(|(_, r, _)| r).collect();
    out.sort_by(|a, b| a.platform.cmp(&b.platform).then(a.version.cmp(&b.version)));
    out
}

fn platform_name(id: &str) -> String {
    let id = id.to_lowercase();
    for (k, n) in [("iphone", "iOS"), ("watch", "watchOS"), ("appletv", "tvOS"), ("xr", "visionOS"), ("vision", "visionOS")] {
        if id.contains(k) {
            return n.into();
        }
    }
    "Simulator".into()
}

/// Remove a simulator device or runtime through simctl.
pub(super) fn remove(action: &Action) -> Result<(), String> {
    let xcrun = run::find_tool(&["/usr/bin/xcrun"]).ok_or("Xcode's tools aren't installed")?;
    let args: Vec<&str> = match action {
        Action::SimDevice(u) => vec!["simctl", "delete", u],
        Action::SimRuntime(id) => vec!["simctl", "runtime", "delete", id],
        _ => return Err("not a simulator".into()),
    };
    let o = run::output(std::process::Command::new(xcrun).args(&args), Duration::from_secs(120)).ok_or("simctl didn't answer")?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().lines().last().unwrap_or("simctl failed").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_support_versions() {
        let v = |s| DsVersion::parse(s).map(|d| d.version);
        assert_eq!(v("17.2 (21C62)"), Some(vec![17, 2]));
        assert_eq!(v("17.2 (21C62) arm64e"), Some(vec![17, 2]));
        assert_eq!(v("iPhone16,2 17.2 (21C62)"), Some(vec![17, 2]));
        // the device model's digits must not be read as the version
        assert_eq!(v("Watch2,7 5.2.1 (16U113)"), Some(vec![5, 2, 1]));
        assert_eq!(v("14.2"), Some(vec![14, 2]));
        assert_eq!(v("Something odd"), None);
        let mut l = ["Watch2,7 5.2.1 (16U113)", "iPhone16,2 17.2 (21C62)", "9.3", "17.10 (21A)"].map(|s| DsVersion::parse(s).unwrap());
        l.sort();
        assert_eq!(l.last().unwrap().version, vec![17, 10]);
        // same version: the later build wins
        assert!(DsVersion::parse("17.2 (21C101)").unwrap() > DsVersion::parse("17.2 (21C62)").unwrap());
    }

    #[test]
    fn derived_data_names() {
        assert_eq!(derived_data_name("MyApp-abcdefghijklmnopqrstuvwxyz"), "MyApp");
        assert_eq!(derived_data_name("my-app-abcdefghijklmnopqrstuvwxyz"), "my-app");
        assert_eq!(derived_data_name("NoHash"), "NoHash");
        assert_eq!(derived_data_name("short-abc"), "short-abc");
    }

    #[test]
    fn derived_data_project_from_info_plist() {
        let base = std::env::temp_dir().join(format!("cmdmeow-dd-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>LastAccessedDate</key><date>2026-01-01T00:00:00Z</date><key>WorkspacePath</key><string>/Users/me/gone/Foo.xcodeproj</string></dict></plist>").unwrap();
        assert_eq!(workspace_of(&base), Some(PathBuf::from("/Users/me/gone/Foo.xcodeproj")));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn unavailable_simulators() {
        let j = br#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-15-0":[
            {"udid":"AAAA-1111","name":"iPhone 13","isAvailable":false,"availabilityError":"runtime profile not found"},
            {"udid":"BBBB-2222","name":"iPhone 15","isAvailable":true},
            {"udid":"../../etc","name":"Evil","isAvailable":false}]}}"#;
        let d = unavailable_devices(j);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].udid, "AAAA-1111");
        assert_eq!(d[0].why, "runtime profile not found");
        assert!(unavailable_devices(b"not json").is_empty());
    }

    #[test]
    fn old_runtimes_keep_the_newest_per_platform() {
        let j = br#"{
          "A1": {"identifier":"A1","version":"17.0","platformIdentifier":"com.apple.platform.iphonesimulator","sizeBytes":7000000000,"deletable":true,"state":"Ready"},
          "A2": {"identifier":"A2","version":"17.5","platformIdentifier":"com.apple.platform.iphonesimulator","sizeBytes":8000000000,"deletable":true,"state":"Ready"},
          "A3": {"identifier":"A3","version":"16.4","platformIdentifier":"com.apple.platform.iphonesimulator","deletable":false},
          "W1": {"identifier":"W1","version":"10.0","platformIdentifier":"com.apple.platform.watchsimulator","sizeBytes":3000000000,"deletable":true,"state":"Ready"}
        }"#;
        let r = old_runtimes(j);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].id, "A1");
        assert_eq!(r[0].platform, "iOS");
        assert_eq!(r[0].bytes, 7_000_000_000);
    }

    #[test]
    fn archives_keep_the_newest_per_app() {
        let base = std::env::temp_dir().join(format!("cmdmeow-arch-{}", std::process::id()));
        let mk = |day: &str, name: &str, secs_ago: u64| {
            let p = base.join(day).join(name);
            std::fs::create_dir_all(&p).unwrap();
            let t = std::time::SystemTime::now() - Duration::from_secs(secs_ago);
            std::fs::File::open(&p).unwrap().set_modified(t).unwrap();
        };
        mk("2026-01-01", "Foo 01.01.2026, 10.00.xcarchive", 3000);
        mk("2026-02-01", "Foo 01.02.2026, 10.00.xcarchive", 100);
        mk("2026-02-01", "Bar 01.02.2026, 11.00.xcarchive", 200);
        let a = read_archive(&base.join("2026-01-01/Foo 01.01.2026, 10.00.xcarchive"));
        assert_eq!(a.app, "Foo");
        let plan: Vec<(String, Verdict)> = archive_plan(&base).into_iter().map(|(a, v, _)| (a.path.strip_prefix(&base).unwrap().display().to_string(), v)).collect();
        assert_eq!(plan.len(), 3);
        assert!(plan.contains(&("2026-02-01/Bar 01.02.2026, 11.00.xcarchive".into(), Verdict::Keep)));
        assert!(plan.contains(&("2026-02-01/Foo 01.02.2026, 10.00.xcarchive".into(), Verdict::Keep)));
        assert!(plan.contains(&("2026-01-01/Foo 01.01.2026, 10.00.xcarchive".into(), Verdict::Ask)));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
