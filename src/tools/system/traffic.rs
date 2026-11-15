//! Which apps use the network right now, from `nettop` (no root, no packet
//! capture). Runs only while someone looks at the Network sheet.

use super::monitor::Shared;
use crate::kit::sync::LockExt;
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One app's traffic, in bytes per second.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppRate {
    pub name: String,
    pub down: f64,
    pub up: f64,
}

/// Apps shown at most.
pub const TOP: usize = 5;
/// Readings stop this long after the sheet was last drawn.
const LINGER: Duration = Duration::from_secs(4);
const EVERY: Duration = Duration::from_secs(2);

static WANTED_AT: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Totals per process: "name.pid" → (name, bytes in, bytes out).
pub type Totals = HashMap<String, (String, u64, u64)>;

/// Parse `nettop -P -L 1 -x -J bytes_in,bytes_out` CSV. The header names the
/// columns; the process column is the one with an empty header ("name.pid").
pub fn parse(out: &str) -> Totals {
    let mut totals = Totals::new();
    let (mut name_i, mut in_i, mut out_i) = (None, None, None);
    for line in out.lines() {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if let Some(i) = cols.iter().position(|c| *c == "bytes_in") {
            in_i = Some(i);
            out_i = cols.iter().position(|c| *c == "bytes_out");
            name_i = cols.iter().position(|c| c.is_empty()).filter(|&n| n < i);
            continue;
        }
        let (Some(ni), Some(ii), Some(oi)) = (name_i, in_i, out_i) else { continue };
        let (Some(key), Some(bi), Some(bo)) = (cols.get(ni), cols.get(ii), cols.get(oi)) else { continue };
        let (Ok(bi), Ok(bo)) = (bi.parse::<u64>(), bo.parse::<u64>()) else { continue };
        if key.is_empty() {
            continue;
        }
        totals.insert(key.to_string(), (app_name(key), bi, bo));
    }
    totals
}

/// "Google Chrome He.4120" → "Google Chrome": no pid, and helpers count
/// toward their app.
pub fn app_name(key: &str) -> String {
    let base = match key.rsplit_once('.') {
        Some((n, pid)) if !n.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) => n,
        _ => key,
    };
    let base = base.trim();
    for helper in [" Helper", " Web Content", " Networking"] {
        if let Some(i) = base.find(helper) {
            if i > 0 {
                return base[..i].trim().to_string();
            }
        }
    }
    // nettop cuts long names; a cut-off "Helper" ("… He", "… Hel") goes too
    if let Some((app, tail)) = base.rsplit_once(' ') {
        if !app.is_empty() && tail.len() < 6 && "Helper".starts_with(tail) && !tail.is_empty() {
            return app.trim().to_string();
        }
    }
    base.to_string()
}

/// Rates between two readings `secs` apart, per app, busiest first.
pub fn rates(prev: &Totals, now: &Totals, secs: f64) -> Vec<AppRate> {
    let secs = secs.max(0.1);
    let mut by_app: HashMap<&str, (f64, f64)> = HashMap::new();
    for (key, (name, bi, bo)) in now {
        // a process that's new since last time has no rate yet
        let Some((_, pi, po)) = prev.get(key) else { continue };
        let e = by_app.entry(name.as_str()).or_default();
        e.0 += bi.saturating_sub(*pi) as f64 / secs;
        e.1 += bo.saturating_sub(*po) as f64 / secs;
    }
    let mut v: Vec<AppRate> = by_app
        .into_iter()
        .filter(|(_, (d, u))| d + u >= 1.0)
        .map(|(n, (down, up))| AppRate { name: n.to_string(), down, up })
        .collect();
    v.sort_by(|a, b| (b.down + b.up).total_cmp(&(a.down + a.up)).then_with(|| a.name.cmp(&b.name)));
    v.truncate(TOP);
    v
}

fn read() -> Totals {
    let out = Command::new("nettop")
        .args(["-P", "-L", "1", "-x", "-J", "bytes_in,bytes_out"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    parse(&out)
}

/// Ask for fresh per-app readings; call each frame the Network sheet shows.
pub fn want(shared: &Shared, ctx: &egui::Context) {
    WANTED_AT.store(now_ms(), Ordering::Relaxed);
    if RUNNING.swap(true, Ordering::Relaxed) {
        return;
    }
    let (shared, ctx) = (shared.clone(), ctx.clone());
    let spawned = std::thread::Builder::new().name("app-traffic".into()).spawn(move || {
        let mut prev = read();
        let mut at = Instant::now();
        while now_ms().saturating_sub(WANTED_AT.load(Ordering::Relaxed)) < LINGER.as_millis() as u64 {
            std::thread::sleep(EVERY);
            let now = read();
            let list = rates(&prev, &now, at.elapsed().as_secs_f64());
            at = Instant::now();
            prev = now;
            shared.lock_ok().apps = list;
            ctx.request_repaint();
        }
        shared.lock_ok().apps.clear();
        RUNNING.store(false, Ordering::Relaxed);
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "time,,bytes_in,bytes_out,
12:00:00.000001,launchd.1,0,0,
12:00:00.000002,Google Chrome He.4120,1000,200,
12:00:00.000003,Google Chrome He.4121,500,100,
12:00:00.000004,Safari.900,10000,1000,
12:00:00.000005,mDNSResponder.300,50,50,
";
    const B: &str = "time,,bytes_in,bytes_out,
12:00:02.000001,launchd.1,0,0,
12:00:02.000002,Google Chrome He.4120,3000,600,
12:00:02.000003,Google Chrome He.4121,1500,300,
12:00:02.000004,Safari.900,10000,1000,
12:00:02.000005,mDNSResponder.300,52,52,
12:00:02.000006,curl.5555,999999,0,
";

    #[test]
    fn parses_by_header() {
        let t = parse(A);
        assert_eq!(t.len(), 5);
        assert_eq!(t["Safari.900"], ("Safari".to_string(), 10000, 1000));
        // columns in another order still work
        let t = parse("time,,bytes_out,bytes_in,\nx,Mail.7,5,9,\n");
        assert_eq!(t["Mail.7"], ("Mail".to_string(), 9, 5));
        assert!(parse("").is_empty());
        assert!(parse("garbage\n1,2,3\n").is_empty());
    }

    #[test]
    fn names() {
        assert_eq!(app_name("Safari.900"), "Safari");
        assert_eq!(app_name("Google Chrome He.4120"), "Google Chrome");
        assert_eq!(app_name("Slack Helper (Renderer).77"), "Slack");
        assert_eq!(app_name("com.apple.WebKit.Networking.12"), "com.apple.WebKit.Networking");
        assert_eq!(app_name("Hel.3"), "Hel");
        assert_eq!(app_name("noPid"), "noPid");
    }

    #[test]
    fn rates_per_app() {
        let r = rates(&parse(A), &parse(B), 2.0);
        // Chrome's helpers add up; Safari was idle; new curl has no rate yet
        assert_eq!(r[0], AppRate { name: "Google Chrome".into(), down: 1500.0, up: 300.0 });
        assert_eq!(r[1], AppRate { name: "mDNSResponder".into(), down: 1.0, up: 1.0 });
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn keeps_the_top_few() {
        let mut a = String::from("time,,bytes_in,bytes_out,\n");
        let mut b = a.clone();
        for i in 0..9 {
            a.push_str(&format!("t,app{i}.{i},0,0,\n"));
            b.push_str(&format!("t,app{i}.{i},{},0,\n", (i + 1) * 100));
        }
        let r = rates(&parse(&a), &parse(&b), 1.0);
        assert_eq!(r.len(), TOP);
        assert_eq!(r[0].name, "app8");
    }
}
