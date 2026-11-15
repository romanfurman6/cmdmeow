//! The speed test engine. Ping with jitter, then download and upload over
//! several connections at once, each phase boxed in time. The first moments
//! of a transfer (TCP slow start) don't count toward the result.

use super::monitor::{Shared, SpeedMode, SpeedPhase, SpeedTest};
use crate::kit::mechanics as m;
use crate::kit::sync::LockExt;
use serde_json::Value;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Who measured the test: the provider and where the server is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerInfo {
    pub isp: String,
    pub city: String,
    pub colo: String,
}

impl ServerInfo {
    /// "Provider · City", whichever parts are known.
    pub fn line(&self) -> String {
        let place = if self.city.is_empty() { &self.colo } else { &self.city };
        [self.isp.as_str(), place.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
    }
}

// ---------------------------------------------------------------- math

pub fn mbps(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        return 0.0;
    }
    bytes as f64 * 8.0 / secs / 1e6
}

/// The rate of a transfer that ignores its first `grace` seconds: once past
/// the grace, only the bytes after it count. Before that, the plain average.
#[derive(Debug)]
pub struct Meter {
    grace: f64,
    at_grace: Option<(f64, u64)>,
}

impl Meter {
    pub fn new(grace: Duration) -> Self {
        Meter { grace: grace.as_secs_f64(), at_grace: None }
    }

    /// Feed the elapsed seconds and the bytes moved so far; returns Mbps.
    pub fn sample(&mut self, secs: f64, bytes: u64) -> f64 {
        if secs < self.grace {
            return mbps(bytes, secs);
        }
        let (t0, b0) = *self.at_grace.get_or_insert((secs, bytes));
        if secs - t0 < 0.25 {
            // too short a stretch past the grace to be steady
            return mbps(bytes, secs);
        }
        mbps(bytes.saturating_sub(b0), secs - t0)
    }
}

pub fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    Some(if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 })
}

/// Mean change between one round trip and the next.
pub fn jitter(rtts: &[f64]) -> Option<f64> {
    if rtts.len() < 2 {
        return None;
    }
    let sum: f64 = rtts.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    Some(sum / (rtts.len() - 1) as f64)
}

/// The server's own time in a `Server-Timing` header
/// (`cfRequestDuration;dur=12.3`), in ms. It isn't network time, so it
/// comes off the round trip.
pub fn server_ms(header: &str) -> Option<f64> {
    header
        .split(',')
        .flat_map(|entry| entry.split(';'))
        .find_map(|part| part.trim().strip_prefix("dur=").and_then(|v| v.trim().parse::<f64>().ok()))
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// Provider and place from the test server's `/meta` JSON.
pub fn parse_meta(json: &str) -> Option<ServerInfo> {
    let v: Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let info = ServerInfo { isp: s("asOrganization"), city: s("city"), colo: s("colo") };
    (!info.line().is_empty()).then_some(info)
}

/// Where a phase is, 0..1, from its elapsed time.
fn phase_frac(elapsed: Duration, total: Duration) -> f32 {
    (elapsed.as_secs_f32() / total.as_secs_f32().max(1e-3)).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------- transfer

/// Run `streams` workers until `length` passes or the test is stopped,
/// sampling the bytes they move. `report` gets (live Mbps, 0..1).
/// Returns the measured Mbps, or None when nothing came through.
fn transfer<W>(streams: usize, length: Duration, grace: Duration, stopped: &(dyn Fn() -> bool + Sync), report: &dyn Fn(f64, f32), worker: W) -> Option<f64>
where
    W: Fn(&AtomicU64, &(dyn Fn() -> bool + Sync)) + Sync,
{
    let bytes = AtomicU64::new(0);
    let over = AtomicBool::new(false);
    let alive = AtomicUsize::new(streams);
    let done = || over.load(Ordering::Relaxed) || stopped();
    let mut meter = Meter::new(grace);
    let mut last = 0.0;
    let t0 = Instant::now();
    std::thread::scope(|sc| {
        for _ in 0..streams {
            sc.spawn(|| {
                worker(&bytes, &done);
                alive.fetch_sub(1, Ordering::Relaxed);
            });
        }
        loop {
            std::thread::sleep(Duration::from_millis(80));
            let el = t0.elapsed();
            last = meter.sample(el.as_secs_f64(), bytes.load(Ordering::Relaxed));
            report(last, phase_frac(el, length));
            if el >= length || stopped() || alive.load(Ordering::Relaxed) == 0 {
                over.store(true, Ordering::Relaxed);
                break;
            }
        }
    });
    (bytes.load(Ordering::Relaxed) > 0).then_some(last)
}

/// One download stream: asks for big files back to back, counting bytes.
fn download_stream(agent: &ureq::Agent, bytes: &AtomicU64, done: &(dyn Fn() -> bool + Sync)) {
    // the server refuses files over its cap (403 for 100 MB since 2026-11),
    // so a refused size is halved and asked again
    let mut chunk = m::SPEED_TEST_CHUNK_DOWN;
    let mut buf = vec![0u8; 64 * 1024];
    while !done() {
        let url = format!("{}?bytes={chunk}", m::SPEED_TEST_DOWN_URL);
        let r = match agent.get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(c)) if (400..500).contains(&c) && chunk > 1_000_000 => {
                chunk /= 2;
                continue;
            }
            Err(_) => return,
        };
        let mut reader = r.into_body().into_reader();
        loop {
            if done() {
                return;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.fetch_add(n as u64, Ordering::Relaxed);
                }
                Err(_) => return,
            }
        }
    }
}

/// One upload stream: posts zeros back to back, counting what goes out.
fn upload_stream(agent: &ureq::Agent, bytes: &AtomicU64, done: &(dyn Fn() -> bool + Sync)) {
    let len = m::SPEED_TEST_CHUNK_UP;
    while !done() {
        let mut body = Zeros { left: len, bytes, done };
        let sent = agent
            .post(m::SPEED_TEST_UP_URL)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", &len.to_string())
            .send(ureq::SendBody::from_reader(&mut body));
        if sent.is_err() {
            return;
        }
    }
}

/// `left` zero bytes as an upload body, counted as they're handed out.
struct Zeros<'a> {
    left: u64,
    bytes: &'a AtomicU64,
    done: &'a (dyn Fn() -> bool + Sync),
}

impl Read for Zeros<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if (self.done)() {
            return Err(std::io::Error::other("stopped"));
        }
        let n = (buf.len() as u64).min(self.left).min(64 * 1024) as usize;
        buf[..n].fill(0);
        self.left -= n as u64;
        self.bytes.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

// ---------------------------------------------------------------- the test

/// What a mode measures with: pings, seconds per phase, the grace before
/// the meter counts, and download / upload streams.
struct Plan {
    pings: usize,
    phase: Duration,
    grace: Duration,
    down: usize,
    up: usize,
}

fn plan(mode: SpeedMode) -> Plan {
    match mode {
        // about 6 s: few pings, short phases, enough streams to fill the line fast
        SpeedMode::Quick => Plan { pings: 3, phase: Duration::from_millis(2500), grace: Duration::from_millis(600), down: 6, up: 4 },
        SpeedMode::Full => Plan { pings: m::SPEED_TEST_PINGS, phase: m::SPEED_TEST_PHASE, grace: m::SPEED_TEST_GRACE, down: m::SPEED_TEST_STREAMS_DOWN, up: m::SPEED_TEST_STREAMS_UP },
    }
}

pub fn start(shared: Shared, ctx: egui::Context, mode: SpeedMode) {
    let pl = plan(mode);
    {
        let mut s = shared.lock_ok();
        if s.speed.running {
            return;
        }
        s.speed = SpeedTest { mode, running: true, phase: SpeedPhase::Ping, ..Default::default() };
    }
    let cancel = shared.lock_ok().speed.cancel.clone();
    std::thread::spawn(move || {
        let stopped = move || cancel.load(Ordering::Relaxed);
        let set = |f: &dyn Fn(&mut SpeedTest)| {
            let mut s = shared.lock_ok();
            f(&mut s.speed);
            ctx.request_repaint();
        };
        // a stop ends the test quietly with what it has so far
        let finish = |err: Option<String>| {
            let quiet = stopped();
            set(&|s| {
                s.running = false;
                s.phase = SpeedPhase::Done;
                s.live_mbps = 0.0;
                if !quiet && err.is_some() {
                    s.error = err.clone();
                }
            })
        };
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(8)))
                .timeout_global(Some(Duration::from_secs(60)))
                .build(),
        );

        // who's measuring, on the side
        std::thread::scope(|sc| {
            let meta = sc.spawn(|| {
                let body = agent.get(m::SPEED_TEST_META_URL).call().ok()?.into_body().read_to_string().ok()?;
                parse_meta(&body)
            });

            // ping: the first call opens the connection; the rest are round
            // trips without the server's own time
            let ping_url = format!("{}?bytes=0", m::SPEED_TEST_DOWN_URL);
            if let Err(e) = agent.get(&ping_url).call() {
                finish(Some(format!("No internet connection ({e})")));
                return;
            }
            let mut rtts = Vec::new();
            for i in 0..pl.pings {
                if stopped() {
                    finish(None);
                    return;
                }
                let t0 = Instant::now();
                if let Ok(r) = agent.get(&ping_url).call() {
                    let total = t0.elapsed().as_secs_f64() * 1000.0;
                    let server = r.headers().get("server-timing").and_then(|h| h.to_str().ok()).and_then(server_ms).unwrap_or(0.0);
                    rtts.push((total - server).max(0.1));
                }
                let frac = (i + 1) as f32 / pl.pings as f32;
                let (lat, jit) = (median(&rtts), jitter(&rtts));
                set(&|s| {
                    s.latency_ms = lat;
                    s.jitter_ms = jit;
                    s.progress = 0.1 * frac;
                });
            }
            if rtts.is_empty() {
                finish(Some("No internet connection".into()));
                return;
            }
            if let Ok(Some(info)) = meta.join() {
                set(&|s| s.server = Some(info.clone()));
            }
        });
        if stopped() || shared.lock_ok().speed.phase == SpeedPhase::Done {
            finish(None);
            return;
        }

        // download
        set(&|s| s.phase = SpeedPhase::Download);
        let report = |base: f32, share: f32| {
            move |live: f64, frac: f32| set(&|s| {
                s.live_mbps = live;
                s.progress = base + share * frac;
            })
        };
        let down = transfer(pl.down, pl.phase, pl.grace, &stopped, &report(0.1, 0.5), |b, d| download_stream(&agent, b, d));
        if stopped() {
            set(&|s| s.down_mbps = down);
            finish(None);
            return;
        }
        let Some(down) = down else {
            finish(Some("Download test failed".into()));
            return;
        };
        set(&|s| {
            s.down_mbps = Some(down);
            s.live_mbps = 0.0;
            s.progress = 0.6;
            s.phase = SpeedPhase::Upload;
        });

        // upload
        let up = transfer(pl.up, pl.phase, pl.grace, &stopped, &report(0.6, 0.4), |b, d| upload_stream(&agent, b, d));
        set(&|s| {
            if !stopped() {
                s.up_mbps = up;
            }
            s.progress = 1.0;
        });
        finish(if up.is_none() { Some("Upload test failed".into()) } else { None });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_skips_the_grace() {
        let mut m = Meter::new(Duration::from_millis(1500));
        // slow start: 1 MB in the first 1.5 s
        assert!((m.sample(1.0, 500_000) - 4.0).abs() < 1e-9);
        let at_grace = m.sample(1.5, 1_000_000);
        assert!((at_grace - mbps(1_000_000, 1.5)).abs() < 1e-9);
        // then 10 MB/s: 80 Mbps, whatever came before
        let steady = m.sample(3.5, 21_000_000);
        assert!((steady - 80.0).abs() < 1e-9, "{steady}");
    }

    #[test]
    fn meter_stays_with_the_average_just_past_grace() {
        let mut m = Meter::new(Duration::from_secs(1));
        m.sample(1.0, 1_000_000);
        // only 0.1 s past the grace: too short to trust on its own
        assert!((m.sample(1.1, 1_100_000) - mbps(1_100_000, 1.1)).abs() < 1e-9);
    }

    #[test]
    fn mbps_handles_zero_time() {
        assert_eq!(mbps(1000, 0.0), 0.0);
        assert!((mbps(1_000_000, 1.0) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn median_and_jitter() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[30.0, 10.0, 20.0]), Some(20.0));
        assert_eq!(median(&[10.0, 20.0, 30.0, 40.0]), Some(25.0));
        assert_eq!(jitter(&[20.0]), None);
        assert_eq!(jitter(&[20.0, 24.0, 20.0, 21.0]), Some(3.0));
    }

    #[test]
    fn server_timing() {
        assert_eq!(server_ms("cfRequestDuration;dur=12.5"), Some(12.5));
        assert_eq!(server_ms("cdn-cache; desc=MISS, cfRequestDuration;dur=3"), Some(3.0));
        assert_eq!(server_ms("cfL4;desc=\"?proto=TCP\""), None);
        assert_eq!(server_ms(""), None);
    }

    #[test]
    fn meta() {
        let j = r#"{"clientIp":"1.2.3.4","asOrganization":"Telefonica de Espana","colo":"MAD","city":"Madrid","country":"ES"}"#;
        let info = parse_meta(j).unwrap();
        assert_eq!(info.line(), "Telefonica de Espana · Madrid");
        let only_colo = parse_meta(r#"{"colo":"MAD"}"#).unwrap();
        assert_eq!(only_colo.line(), "MAD");
        assert_eq!(parse_meta("{}"), None);
        assert_eq!(parse_meta("not json"), None);
    }

    #[test]
    fn phase_fraction_is_bounded() {
        assert_eq!(phase_frac(Duration::ZERO, Duration::from_secs(8)), 0.0);
        assert_eq!(phase_frac(Duration::from_secs(4), Duration::from_secs(8)), 0.5);
        assert_eq!(phase_frac(Duration::from_secs(9), Duration::from_secs(8)), 1.0);
    }

    #[test]
    fn transfer_counts_bytes_and_stops_on_time() {
        // workers that move 1 MB every 10 ms until told to stop
        let t0 = Instant::now();
        let r = transfer(3, Duration::from_millis(400), Duration::ZERO, &|| false, &|_, _| {}, |b, done| {
            while !done() {
                b.fetch_add(1_000_000, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert!(r.unwrap() > 0.0);
    }

    #[test]
    fn transfer_with_nothing_through_is_none() {
        let r = transfer(2, Duration::from_millis(200), Duration::ZERO, &|| false, &|_, _| {}, |_, _| {});
        assert_eq!(r, None);
    }
}
