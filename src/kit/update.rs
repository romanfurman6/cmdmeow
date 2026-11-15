//! In-app updates. Each GitHub Release carries a small `latest.json` (made by
//! release.yml); the app reads the latest one at launch and every few hours.
//! When it names a newer version, the panel shows a banner, and its button
//! downloads that release's CmdMeow.zip, checks it (sha256 from the manifest,
//! a valid code signature from the same Apple team as the running app, the
//! same bundle id and the promised version), puts it in place of the running
//! app and relaunches.
//!
//! A file the app downloads itself carries no quarantine flag, so the new
//! version opens without Gatekeeper's "Open Anyway", notarized or not.
//! The dev build never checks on its own (`CMDMEOW_UPDATE_URL` makes it).

use super::sync::LockExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The newest non-prerelease Release's manifest (a stable GitHub redirect,
/// no API call, so no rate limit).
pub const MANIFEST_URL: &str = "https://github.com/romanfurman6/cmdmeow/releases/latest/download/latest.json";
/// Where the banner sends you when an update can't install itself.
pub const RELEASES_URL: &str = "https://github.com/romanfurman6/cmdmeow/releases/latest";
const FIRST_CHECK: Duration = Duration::from_secs(15);
const EVERY: Duration = Duration::from_secs(6 * 3600);
/// Staging folder, next to the app so the final swap is a same-volume rename.
const STAGING: &str = ".cmdmeow-update";

/// What `latest.json` says.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// CmdMeow.zip of exactly this version (a versioned URL, so its sha256
    /// stays right after a newer release).
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    /// A newer version is out.
    Available(Manifest),
    /// Downloading it: 0..1.
    Downloading(String, f32),
    /// Checking and swapping the app.
    Installing(String),
    /// In place; the app quits and the new one opens.
    Restarting,
    /// The update didn't go through; the banner offers the download page.
    Failed(String, String),
}

struct Shared {
    state: State,
    busy: bool,
    last_check: Option<Instant>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { state: State::Idle, busy: false, last_check: None });
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn state() -> State {
    SHARED.lock_ok().state.clone()
}

fn set(s: State) {
    SHARED.lock_ok().state = s;
}

/// The banner shows for every state but Idle.
pub fn banner() -> bool {
    !matches!(state(), State::Idle)
}

fn manifest_url() -> Option<String> {
    if let Ok(u) = std::env::var("CMDMEOW_UPDATE_URL") {
        return Some(u);
    }
    if super::fs::is_dev() {
        return None;
    }
    Some(MANIFEST_URL.into())
}

/// Called every frame (and on the hidden heartbeat): checks when it's due.
pub fn tick() {
    let started = *STARTED.get_or_init(Instant::now);
    let Some(url) = manifest_url() else { return };
    {
        let mut s = SHARED.lock_ok();
        // a test manifest (CMDMEOW_UPDATE_URL) is checked right away
        let first = if url == MANIFEST_URL { FIRST_CHECK } else { Duration::from_secs(2) };
        if s.busy || started.elapsed() < first || s.last_check.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        // an offered update stays offered; a failed one is checked again
        if matches!(s.state, State::Downloading(..) | State::Installing(_) | State::Restarting) {
            return;
        }
        s.busy = true;
        s.last_check = Some(Instant::now());
    }
    let _ = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let found = fetch_manifest(&url);
        let mut s = SHARED.lock_ok();
        s.busy = false;
        match found {
            Ok(m) if is_newer(&m.version, env!("CARGO_PKG_VERSION")) => {
                eprintln!("[update] {} is out", m.version);
                s.state = State::Available(m);
            }
            Ok(_) => {
                if matches!(s.state, State::Failed(..) | State::Available(_)) {
                    s.state = State::Idle;
                }
            }
            Err(e) => eprintln!("[update] check failed: {e}"),
        }
    });
}

/// `body`: how long the whole body may take, so a stalled connection ends
/// in a failure (and the banner's Download) instead of a frozen bar.
fn agent(body: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(20))).timeout_recv_response(Some(Duration::from_secs(30))).timeout_recv_body(Some(body)).http_status_as_error(false).build())
}

/// `file://` URLs are for local tests only (`CMDMEOW_UPDATE_URL` set).
fn local_ok() -> bool {
    cfg!(test) || std::env::var_os("CMDMEOW_UPDATE_URL").is_some()
}

fn fetch_manifest(url: &str) -> Result<Manifest, String> {
    if let Some(path) = url.strip_prefix("file://") {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        return serde_json::from_str(&text).map_err(|e| e.to_string());
    }
    let resp = agent(Duration::from_secs(30)).get(url).header("User-Agent", "Cmd+Meow").call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("manifest answered {status}"));
    }
    let text = resp.into_body().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// `1.2.3` → (1, 2, 3, release?). A `-beta.1` style suffix is a prerelease,
/// older than the same numbers without one.
fn parse(v: &str) -> Option<(u64, u64, u64, bool)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, _)) => (c, true),
        None => (v, false),
    };
    let mut it = core.split('.').map(|n| n.parse::<u64>().ok());
    let t = (it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0), !pre);
    Some(t)
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse(remote), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

/// Start installing the offered update (the banner's button).
pub fn install() {
    let m = {
        let mut s = SHARED.lock_ok();
        let State::Available(m) = s.state.clone() else { return };
        if s.busy {
            return;
        }
        s.busy = true;
        s.state = State::Downloading(m.version.clone(), 0.0);
        m
    };
    let _ = std::thread::Builder::new().name("update-install".into()).spawn(move || {
        let r = run_install(&m);
        let mut s = SHARED.lock_ok();
        s.busy = false;
        match r {
            Ok(true) => s.state = State::Restarting,
            // in place, but no relaunch: the next launch is the new version
            Ok(false) => s.state = State::Idle,
            Err(e) => {
                eprintln!("[update] {} failed: {e}", m.version);
                s.state = State::Failed(m.version.clone(), e);
            }
        }
    });
}

/// The running app's bundle, from the executable's path.
fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    exe.ancestors().find(|a| a.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Ok(true): in place and relaunching; Ok(false): in place, relaunch failed.
fn run_install(m: &Manifest) -> Result<bool, String> {
    let bundle = current_bundle().ok_or("not running from an app bundle")?;
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err("move Cmd+Meow to Applications first".into());
    }
    let dir = bundle.parent().ok_or("no folder above the app")?;
    let staging = dir.join(STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("can't write next to the app: {e}"))?;
    let r = (|| {
        let zip = staging.join("CmdMeow.zip");
        download(m, &zip)?;
        set(State::Installing(m.version.clone()));
        let out = staging.join("x");
        run("/usr/bin/ditto", &["-x", "-k", path_str(&zip)?, path_str(&out)?])?;
        let new_app = std::fs::read_dir(&out).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "app")).ok_or("no app in the zip")?;
        verify(&new_app, &bundle, &m.version)?;
        swap(&bundle, &new_app, dir)
    })();
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    r?;
    let old = dir.join(old_name(&bundle));
    match relaunch(&bundle, &staging, &old) {
        Ok(()) => Ok(true),
        Err(e) => {
            eprintln!("[update] installed, but {e}");
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_dir_all(&old);
            Ok(false)
        }
    }
}

fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str().ok_or_else(|| "path isn't UTF-8".into())
}

fn old_name(bundle: &Path) -> String {
    format!(".{}.old", bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Fetch the zip to `to`, hashing as it comes, and compare with the manifest.
fn download(m: &Manifest, to: &Path) -> Result<(), String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut done = 0u64;
    let mut on_chunk = |chunk: &[u8], total: u64| -> Result<(), String> {
        hasher.update(chunk);
        file.write_all(chunk).map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        if total > 0 {
            set(State::Downloading(m.version.clone(), (done as f32 / total as f32).min(1.0)));
        }
        Ok(())
    };
    if let Some(path) = m.url.strip_prefix("file://").filter(|_| local_ok()) {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        on_chunk(&bytes, bytes.len() as u64)?;
    } else {
        if !m.url.starts_with("https://") {
            return Err("update URL isn't https".into());
        }
        let resp = agent(Duration::from_secs(20 * 60)).get(&m.url).header("User-Agent", "Cmd+Meow").call().map_err(|e| format!("couldn't reach the download: {e}"))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(format!("the download answered {status}"));
        }
        let header_len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
        let len = header_len.unwrap_or(m.size);
        // never more than promised (plus slack), so a bad server can't fill the disk
        let cap = m.size.max(header_len.unwrap_or(0)).max(1 << 20) + (1 << 20);
        let mut got = 0u64;
        let mut body = resp.into_body();
        let mut reader = body.as_reader();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("the download broke off: {e}"))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > cap {
                return Err("the download is bigger than promised".into());
            }
            on_chunk(&buf[..n], len)?;
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    let got = hex(&hasher.finalize());
    if !got.eq_ignore_ascii_case(m.sha256.trim()) {
        return Err(format!("checksum mismatch ({got})"));
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    let all = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if o.status.success() {
        Ok(all)
    } else {
        Err(format!("{cmd} failed: {}", all.trim()))
    }
}

/// `TeamIdentifier=` from `codesign -dv`; None when unsigned or ad-hoc.
fn team_id(app: &Path) -> Option<String> {
    let out = run("/usr/bin/codesign", &["-dv", "--verbose=2", app.to_str()?]).ok()?;
    parse_team(&out)
}

fn parse_team(codesign_out: &str) -> Option<String> {
    codesign_out.lines().find_map(|l| l.strip_prefix("TeamIdentifier=")).map(str::trim).filter(|t| !t.is_empty() && *t != "not set").map(str::to_owned)
}

fn info(app: &Path, key: &str) -> Option<String> {
    super::plist::read(&app.join("Contents/Info.plist"))?.get(key)?.str().map(str::to_owned)
}

/// The new app must be validly signed by the same team as the running one,
/// be the same app, and be the version the manifest promised.
fn verify(new_app: &Path, current: &Path, version: &str) -> Result<(), String> {
    run("/usr/bin/codesign", &["--verify", "--deep", "--strict", path_str(new_app)?])?;
    let mine = team_id(current).ok_or("this copy isn't signed by a team, so updates can't be checked")?;
    let theirs = team_id(new_app).ok_or("the download isn't signed")?;
    if mine != theirs {
        return Err(format!("signed by another team ({theirs})"));
    }
    if info(new_app, "CFBundleIdentifier") != info(current, "CFBundleIdentifier") {
        return Err("a different app".into());
    }
    if info(new_app, "CFBundleShortVersionString").as_deref() != Some(version) {
        return Err("not the promised version".into());
    }
    Ok(())
}

/// Running app → `.<name>.old`, new app → its place; puts the old one back
/// if the second step fails.
fn swap(bundle: &Path, new_app: &Path, dir: &Path) -> Result<(), String> {
    let old = dir.join(old_name(bundle));
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(bundle, &old).map_err(|e| format!("can't move the app: {e}"))?;
    if let Err(e) = std::fs::rename(new_app, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(format!("can't put the new app in place: {e}"));
    }
    Ok(())
}

/// A small shell waits for this process to end, removes the leftovers and
/// opens the new app. The UI then quits (see `take_restart`).
fn relaunch(bundle: &Path, staging: &Path, old: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let script = format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; rm -rf \"$1\" \"$2\"; /usr/bin/open \"$3\"");
    Command::new("/bin/sh")
        .args(["-c", &script, "sh"])
        .arg(staging)
        .arg(old)
        .arg(bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("can't relaunch: {e}"))?;
    Ok(())
}

/// True once, when the new app is in place and this one should quit.
pub fn take_restart() -> bool {
    let mut s = SHARED.lock_ok();
    if s.state == State::Restarting {
        s.state = State::Idle;
        true
    } else {
        false
    }
}

/// Open the latest release page (the banner's fallback after a failure).
pub fn open_releases() {
    let _ = Command::new("/usr/bin/open").arg(RELEASES_URL).spawn();
}

/// Hide the banner after a failure until the next check.
pub fn dismiss() {
    let mut s = SHARED.lock_ok();
    if matches!(s.state, State::Failed(..)) {
        s.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(is_newer("0.1.2", "0.1.1"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.2.0", "0.2.0-beta.1"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(is_newer("0.2", "0.1.5"));
    }

    #[test]
    fn manifest_parses() {
        let m: Manifest = serde_json::from_str(r#"{"version":"0.1.2","url":"https://x/CmdMeow.zip","sha256":"ab","size":12,"notes":"x"}"#).unwrap();
        assert_eq!(m.version, "0.1.2");
        assert_eq!(m.size, 12);
        let m: Manifest = serde_json::from_str(r#"{"version":"0.1.2","url":"u","sha256":"ab"}"#).unwrap();
        assert_eq!(m.size, 0);
    }

    #[test]
    fn team_from_codesign() {
        let out = "Executable=/x\nIdentifier=com.romanfurman6.cmdmeow\nTeamIdentifier=T4JC44KZRB\nSealed Resources version=2";
        assert_eq!(parse_team(out).as_deref(), Some("T4JC44KZRB"));
        assert_eq!(parse_team("TeamIdentifier=not set"), None);
        assert_eq!(parse_team("Signature=adhoc"), None);
    }

    #[test]
    fn checksum_is_checked() {
        let dir = std::env::temp_dir().join(format!("cmdmeow-upd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.zip");
        std::fs::write(&src, b"hello").unwrap();
        let good = Manifest { version: "9.9.9".into(), url: format!("file://{}", src.display()), sha256: "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824".into(), size: 5 };
        assert!(download(&good, &dir.join("a.zip")).is_ok());
        let bad = Manifest { sha256: "00".into(), ..good };
        assert!(download(&bad, &dir.join("b.zip")).unwrap_err().contains("checksum"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
