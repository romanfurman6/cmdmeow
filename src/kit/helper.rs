//! The privileged helper's client side: the protocol, asking it, and
//! installing or removing it. The helper itself (a small root daemon that
//! writes fan speeds and the charging switch to the SMC) is Fan Control's
//! `serve`; this side lives in the kit so any tool can talk to it.
//!
//! Installed once with the admin password (`osascript … with administrator
//! privileges`): this binary is copied to `/Library/PrivilegedHelperTools`
//! and started by launchd as `--fan-helper <uid>`. It listens on a Unix
//! socket only that user (and root) can open, one JSON line in, one out.
//!
//! Safety lives in the helper, so it holds even if the app dies:
//! - a `set` is a lease: without a new one within [`LEASE_SECS`] (app quit,
//!   crashed or the Mac slept) the fans go back to Auto;
//! - any sensor at 95 °C or more gives the fans back to Auto;
//! - on start and on SIGTERM the fans go back to Auto;
//! - every second the speeds are written again, so a reset by macOS (after
//!   sleep, on M3 and later) doesn't stick.
//!
//! It also holds the battery's charge limit ([`charge`]): the policy is kept
//! in [`CHARGE_FILE`] so it holds while the app is closed and after a
//! restart, and charging is switched back on when the helper stops.

use super::charge::{Policy, Status};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

// The helper keeps the ids from the app's first name (CleanMeBro), so a
// helper that is already installed keeps working after the rename.
pub const LABEL: &str = "com.romanfurman6.cleanmebro.fanhelper";
pub const SOCKET: &str = "/var/run/com.romanfurman6.cleanmebro.fan.sock";
pub const BIN: &str = "/Library/PrivilegedHelperTools/com.romanfurman6.cleanmebro.fanhelper";
pub const PLIST: &str = "/Library/LaunchDaemons/com.romanfurman6.cleanmebro.fanhelper.plist";
/// The charge policy the helper holds (root only).
pub const CHARGE_FILE: &str = "/var/db/com.romanfurman6.cmdmeow.charge.json";
/// Bump when the helper's behaviour or protocol changes: the app then
/// offers to update it.
pub const VERSION: u32 = 3;
/// How long a `set` holds without a fresh one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const LEASE_SECS: u64 = 8;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    Hello,
    /// Hold each fan at this rpm (`None` = Auto for that fan).
    Set { rpm: Vec<Option<f32>> },
    Auto,
    /// Hold this charge policy (limit 100 = off).
    Charge { policy: Policy },
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Reply {
    pub version: u32,
    /// Some fan is under the helper's control now.
    pub manual: bool,
    /// The helper refused or reverted: "hot" (a sensor at 95 °C).
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub charge: Status,
}

// ---------------------------------------------------------------- client

/// Send one request to the helper. `None` if it isn't running.
pub fn ask(req: &Request) -> Option<Reply> {
    let mut s = UnixStream::connect(SOCKET).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
    let mut line = serde_json::to_string(req).ok()?;
    line.push('\n');
    s.write_all(line.as_bytes()).ok()?;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).ok()?;
    serde_json::from_str(out.trim()).ok()
}

/// Install (or update) the helper. Blocks while macOS asks for the admin
/// password. `Err` carries what to show, "" when the user cancelled.
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let uid = unsafe { libc::getuid() };
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{BIN}</string><string>--fan-helper</string><string>{uid}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    );
    let dir = std::env::temp_dir().join(format!("cmdmeow-fan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let plist_tmp = dir.join("helper.plist");
    std::fs::write(&plist_tmp, plist).map_err(|e| e.to_string())?;
    let script = install_script(&sh_quote(&exe), &sh_quote(&plist_tmp));
    let r = run_admin(&dir, &script, "Cmd+Meow wants to add its helper so it can change fan speed and charging.");
    let _ = std::fs::remove_dir_all(&dir);
    r?;
    // give launchd a moment to start it
    for _ in 0..40 {
        if ask(&Request::Hello).is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("The helper was installed but didn't start.".into())
}

/// The root script that (re)installs the helper. An update has to:
/// - wait until launchd has really let go of the old helper: `bootout`
///   returns before it's gone, and `bootstrap` right after fails with
///   "Bootstrap failed: 5: Input/output error";
/// - put the new binary at a new inode (copy, then rename): copying over
///   the old file in place keeps the kernel's cached code signature of the
///   old one, so the new helper is killed at launch and never answers;
/// - re-sign the copy on its own (ad-hoc, with the helper's id): the app's
///   signature binds the bundle's Info.plist, so a copy taken out of the
///   bundle is "not valid" to AMFI (-420) and launchd can't start it;
/// - re-enable the service, in case it was turned off.
fn install_script(exe: &str, plist: &str) -> String {
    format!(
        "set -e\n\
         launchctl bootout system/{LABEL} 2>/dev/null || true\n\
         i=0; while launchctl print system/{LABEL} >/dev/null 2>&1 && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done\n\
         mkdir -p /Library/PrivilegedHelperTools\n\
         rm -f {BIN}.new\n\
         cp {exe} {BIN}.new\n\
         chown root:wheel {BIN}.new\n\
         chmod 755 {BIN}.new\n\
         /usr/bin/codesign --force --sign - --identifier {LABEL} {BIN}.new\n\
         mv -f {BIN}.new {BIN}\n\
         cp -f {plist} {PLIST}\n\
         chown root:wheel {PLIST}\n\
         chmod 644 {PLIST}\n\
         launchctl enable system/{LABEL} 2>/dev/null || true\n\
         launchctl bootstrap system {PLIST} 2>/dev/null || {{ sleep 1; launchctl bootstrap system {PLIST}; }}\n"
    )
}

/// Stop and remove the helper (fans go back to Auto as it stops).
pub fn uninstall() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("cmdmeow-fan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = uninstall_script();
    let r = run_admin(&dir, &script, "Cmd+Meow wants to remove its fan helper.");
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// Stopping the helper switches charging back on; the policy goes too, so a
/// later install starts without a limit.
fn uninstall_script() -> String {
    format!("launchctl bootout system/{LABEL} 2>/dev/null || true\nrm -f {PLIST} {BIN} {SOCKET} {CHARGE_FILE}\n")
}

fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// Run `script` as root after the macOS admin prompt.
fn run_admin(dir: &Path, script: &str, prompt: &str) -> Result<(), String> {
    let file = dir.join("run.sh");
    std::fs::write(&file, script).map_err(|e| e.to_string())?;
    let cmd = format!("/bin/sh {}", sh_quote(&file));
    let apple = format!("do shell script \"{}\" with prompt \"{}\" with administrator privileges", cmd.replace('\\', "\\\\").replace('"', "\\\""), prompt);
    let out = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(apple).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    // -128: the user pressed Cancel
    if err.contains("-128") {
        return Err(String::new());
    }
    Err(if err.is_empty() { "The helper couldn't be installed.".into() } else { err })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trips() {
        let r = Request::Set { rpm: vec![Some(2400.0), None] };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"op":"set","rpm":[2400.0,null]}"#);
        assert_eq!(serde_json::from_str::<Request>(&s).unwrap(), r);
        assert_eq!(serde_json::from_str::<Request>(r#"{"op":"auto"}"#).unwrap(), Request::Auto);
        assert_eq!(serde_json::from_str::<Reply>(r#"{"version":1,"manual":true}"#).unwrap(), Reply { version: 1, manual: true, ..Default::default() });
        let c = Request::Charge { policy: Policy { limit: 80, sailing: true, heat: false } };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"{"op":"charge","policy":{"limit":80,"sailing":true,"heat":false}}"#);
        assert_eq!(serde_json::from_str::<Request>(&s).unwrap(), c);
    }

    #[test]
    fn uninstall_removes_the_charge_policy() {
        let s = uninstall_script();
        assert!(s.starts_with(&format!("launchctl bootout system/{LABEL}")));
        assert!(s.contains(CHARGE_FILE));
    }

    #[test]
    fn quotes_paths_for_sh() {
        assert_eq!(sh_quote(Path::new("/Applications/CmdMeow Dev.app/x")), "'/Applications/CmdMeow Dev.app/x'");
        assert_eq!(sh_quote(Path::new("/a'b")), r"'/a'\''b'");
    }

    #[test]
    fn install_replaces_the_binary_by_rename() {
        let s = install_script("'/x/cmdmeow'", "'/t/helper.plist'");
        // never copy over the running binary in place
        assert!(!s.contains(&format!("cp -f '/x/cmdmeow' {BIN}\n")));
        assert!(s.contains(&format!("mv -f {BIN}.new {BIN}")));
        // signed on its own before it goes in place
        let sign = s.find(&format!("/usr/bin/codesign --force --sign - --identifier {LABEL} {BIN}.new")).unwrap();
        assert!(sign < s.find("mv -f").unwrap());
        // waits for the old helper before bootstrapping the new one
        let wait = s.find("while launchctl print").unwrap();
        assert!(wait < s.find("launchctl bootstrap").unwrap());
        assert!(s.find("launchctl bootout").unwrap() < wait);
    }
}
