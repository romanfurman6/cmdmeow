//! The app's logs, for Settings › About: where they are, a run log for
//! release builds, and one zip a person can attach to a bug report.
//!
//! `~/Library/Logs/CmdMeow` holds `crash.log` (panics, see
//! [`super::sync::install_panic_log`]), `cmdmeow.log` (stderr of a release
//! build, see [`start_run_log`]) and, for the dev build, `dev.log` and
//! `crash-dev.log` (scripts/dev-install.sh).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The run log starts over (the old one kept as `cmdmeow.1.log`) past this.
const RUN_LOG_MAX: u64 = 1 << 20;
/// macOS crash reports for the app older than this stay out of the zip.
const REPORT_DAYS: u64 = 30;

/// `~/Library/Logs/CmdMeow`.
pub fn dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("Library/Logs/CmdMeow")
}

/// Release builds: stderr (the `[update]`, `[rec]`… lines) goes to
/// `cmdmeow.log`. Launched from a terminal, or the dev build (whose script
/// already sends it to `dev.log`), stderr stays where it is.
pub fn start_run_log() {
    if super::fs::is_dev() || unsafe { libc::isatty(2) } == 1 {
        return;
    }
    let dir = dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("cmdmeow.log");
    rotate(&path, RUN_LOG_MAX);
    let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    use std::os::fd::AsRawFd;
    // fd 2 now writes to the file; `f` can close, the dup stays
    unsafe {
        libc::dup2(f.as_raw_fd(), 2);
    }
    eprintln!("\n{} Cmd+Meow {} started", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"));
}

/// Past `max` bytes, `path` becomes `<stem>.1.log` (replacing the old one)
/// so the next write starts a new file.
fn rotate(path: &Path, max: u64) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > max) {
        let _ = std::fs::rename(path, path.with_extension("1.log"));
    }
}

/// Show the logs folder in Finder (made first, so it always opens).
pub fn reveal() {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = Command::new("/usr/bin/open").arg(&d).spawn();
}

/// The app's own reports in `~/Library/Logs/DiagnosticReports` (hangs,
/// crashes macOS caught) from the last [`REPORT_DAYS`] days.
fn system_reports() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(home.join("Library/Logs/DiagnosticReports")) else { return Vec::new() };
    let max_age = std::time::Duration::from_secs(REPORT_DAYS * 86_400);
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| is_ours(p))
        .filter(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a < max_age))
        .collect()
}

/// "cmdmeow-2026-11-25-101112.ips", "CmdMeow Dev_2026….hang": the binary is
/// called cmdmeow in both builds.
fn is_ours(p: &Path) -> bool {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).is_some_and(|n| n.starts_with("cmdmeow") && [".ips", ".crash", ".hang", ".diag", ".spin"].iter().any(|e| n.ends_with(e)))
}

fn sh(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// What goes on top of the zip: the app, macOS and Mac it ran on.
fn about_text() -> String {
    format!(
        "{}\nmacOS {} ({})\n{} · {}\n{}\n",
        crate::core::editor::version_label(),
        sh("/usr/bin/sw_vers", &["-productVersion"]),
        sh("/usr/bin/sw_vers", &["-buildVersion"]),
        sh("/usr/sbin/sysctl", &["-n", "hw.model"]),
        sh("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z"),
    )
}

/// Put the logs, the app's macOS reports and an about.txt in one zip on the
/// Desktop and show it in Finder. Returns the zip.
/// Returns `Err` at once while another export runs (a double click).
pub fn export() -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("already saving".into());
    }
    let r = export_now();
    BUSY.store(false, Ordering::SeqCst);
    r
}

fn export_now() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("no home folder")?;
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let name = format!("CmdMeow-logs-{stamp}");
    let tmp = std::env::temp_dir().join(&name);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("reports")).map_err(|e| e.to_string())?;
    std::fs::write(tmp.join("about.txt"), about_text()).map_err(|e| e.to_string())?;
    if let Ok(rd) = std::fs::read_dir(dir()) {
        for p in rd.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
            if let Some(n) = p.file_name() {
                let _ = std::fs::copy(&p, tmp.join(n));
            }
        }
    }
    for p in system_reports() {
        if let Some(n) = p.file_name() {
            let _ = std::fs::copy(&p, tmp.join("reports").join(n));
        }
    }
    let desktop = home.join("Desktop");
    let zip = desktop.join(format!("{name}.zip"));
    let out = Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(&tmp).arg(&zip).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let _ = Command::new("/usr/bin/open").arg("-R").arg(&zip).spawn();
    Ok(zip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_log_moves_aside_a_short_one_stays() {
        let d = std::env::temp_dir().join(format!("cmdmeow-logs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("cmdmeow.log");
        std::fs::write(&p, "short").unwrap();
        rotate(&p, 10);
        assert!(p.exists());
        std::fs::write(&p, "a line longer than ten").unwrap();
        rotate(&p, 10);
        assert!(!p.exists());
        assert!(d.join("cmdmeow.1.log").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_the_apps_own_reports_go_in_the_zip() {
        for n in ["cmdmeow-2026-11-25-101112.ips", "CmdMeow Dev_2026-11-25-101112_Mac.hang", "cmdmeow.crash"] {
            assert!(is_ours(Path::new(n)), "{n}");
        }
        for n in ["Safari-2026-11-25.ips", "cmdmeow.txt", "kernel.panic"] {
            assert!(!is_ours(Path::new(n)), "{n}");
        }
    }
}
