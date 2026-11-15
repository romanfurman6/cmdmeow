//! The speed test engine. Ping with jitter, then download and upload over
//! several connections at once, each phase boxed in time. The first moments
//! of a transfer (TCP slow start) don't count toward the result.

use super::monitor::{Shared, SpeedMode, SpeedPhase, SpeedTest};
use crate::kit::mechanics as m;
use crate::kit::sync::LockExt;
use serde_json::Value;
use std::io::Read;
use std::sync::Arc;
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

/// What the streams of one transfer share.
#[derive(Default)]
struct Streams {
    bytes: AtomicU64,
    over: AtomicBool,
    alive: AtomicUsize,
}

/// How long a transfer that ran its course waits for its streams to
/// finish their last read. A stop doesn't wait at all.
const WIND_DOWN: Duration = Duration::from_millis(500);

/// Run `streams` workers until `length` passes or the test is stopped,
/// sampling the bytes they move. `report` gets (live Mbps, 0..1).
/// Returns the measured Mbps, or None when nothing came through.
///
/// The workers run on their own threads and are never joined: one stuck in
/// a read or waiting on the server can't hold the test (or a Stop) up. It
/// sees `over` once its call returns and quits.
fn transfer<W>(streams: usize, length: Duration, grace: Duration, stopped: &dyn Fn() -> bool, report: &dyn Fn(f64, f32), worker: W) -> Option<f64>
where
    W: Fn(&AtomicU64, &dyn Fn() -> bool) + Send + Sync + 'static,
{
    let st = Arc::new(Streams { alive: AtomicUsize::new(streams), ..Default::default() });
    let worker = Arc::new(worker);
    for _ in 0..streams {
        let (st, worker) = (st.clone(), worker.clone());
        std::thread::spawn(move || {
            worker(&st.bytes, &|| st.over.load(Ordering::Relaxed));
            st.alive.fetch_sub(1, Ordering::Relaxed);
        });
    }
    let mut meter = Meter::new(grace);
    let t0 = Instant::now();
    let last = loop {
        std::thread::sleep(Duration::from_millis(80));
        if stopped() {
            st.over.store(true, Ordering::Relaxed);
            return None;
        }
        let el = t0.elapsed();
        let live = meter.sample(el.as_secs_f64(), st.bytes.load(Ordering::Relaxed));
        report(live, phase_frac(el, length));
        if el >= length || st.alive.load(Ordering::Relaxed) == 0 {
            st.over.store(true, Ordering::Relaxed);
            break live;
        }
    };
    // let the streams close before the next phase, but not for long
    let t1 = Instant::now();
    while st.alive.load(Ordering::Relaxed) > 0 && t1.elapsed() < WIND_DOWN && !stopped() {
        std::thread::sleep(Duration::from_millis(10));
    }
    (st.bytes.load(Ordering::Relaxed) > 0).then_some(last)
}

/// One download stream: asks for big files back to back, counting bytes.
fn download_stream(agent: &ureq::Agent, bytes: &AtomicU64, done: &dyn Fn() -> bool) {
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
fn upload_stream(agent: &ureq::Agent, bytes: &AtomicU64, done: &dyn Fn() -> bool) {
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
    done: &'a dyn Fn() -> bool,
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

/// Lost pings in a row after which the line counts as down and pinging stops.
const LOST_IN_A_ROW: usize = 3;

/// `n` round trips with `ping`, each under `SPEED_TEST_PING_TIMEOUT`.
/// A lost one counts for nothing, and the next ping first opens a fresh
/// connection uncounted (with the longer connect deadline), so a handshake
/// never reads as a round trip. After [`LOST_IN_A_ROW`] losses it gives up,
/// so a dead line ends the phase in seconds. `report` gets the trips so far
/// and 0..1.
fn round_trips(n: usize, stopped: &dyn Fn() -> bool, ping: impl Fn(Duration) -> Option<f64>, report: impl Fn(&[f64], f32)) -> Vec<f64> {
    let mut rtts = Vec::new();
    let mut lost = 0;
    for i in 0..n {
        if lost > 0 {
            let _ = ping(m::SPEED_TEST_CONNECT_TIMEOUT);
        }
        if stopped() {
            break;
        }
        match ping(m::SPEED_TEST_PING_TIMEOUT) {
            Some(ms) => {
                rtts.push(ms);
                lost = 0;
            }
            None => lost += 1,
        }
        report(&rtts, (i + 1) as f32 / n as f32);
        if lost >= LOST_IN_A_ROW {
            break;
        }
    }
    rtts
}

/// Apply `f` to the speed test state, unless the run that owns `cancel` was
/// stopped: Stop already shows it as stopped, and a new run may be under way.
/// Stop sets the flag under the same lock, so nothing slips in after it.
fn write(shared: &Shared, cancel: &AtomicBool, f: &dyn Fn(&mut SpeedTest)) -> bool {
    let mut s = shared.lock_ok();
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    f(&mut s.speed);
    true
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
        let stopped = || cancel.load(Ordering::Relaxed);
        let set = |f: &dyn Fn(&mut SpeedTest)| {
            if write(&shared, &cancel, f) {
                ctx.request_repaint();
            }
        };
        let finish = |err: Option<String>| {
            set(&|s| {
                s.running = false;
                s.phase = SpeedPhase::Done;
                s.live_mbps = 0.0;
                if err.is_some() {
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

        // who's measuring, on the side: it lands whenever it comes and never
        // holds the test up
        {
            let (agent, shared, cancel, ctx) = (agent.clone(), shared.clone(), cancel.clone(), ctx.clone());
            std::thread::spawn(move || {
                let body = agent.get(m::SPEED_TEST_META_URL).config().timeout_global(Some(m::SPEED_TEST_META_TIMEOUT)).build().call().ok()?.into_body().read_to_string().ok()?;
                let info = parse_meta(&body)?;
                if write(&shared, &cancel, &|s| s.server = Some(info.clone())) {
                    ctx.request_repaint();
                }
                Some(())
            });
        }

        // ping: the first call opens the connection; the rest are round
        // trips without the server's own time. Each has a short deadline: a
        // reply stuck for seconds is a lost ping, not a round trip (it held
        // the dial for most of a minute and read as jitter of 13 seconds).
        let ping_url = format!("{}?bytes=0", m::SPEED_TEST_DOWN_URL);
        let ping = |limit: Duration| {
            let t0 = Instant::now();
            let r = agent.get(&ping_url).config().timeout_global(Some(limit)).build().call().ok()?;
            let total = t0.elapsed().as_secs_f64() * 1000.0;
            let server = r.headers().get("server-timing").and_then(|h| h.to_str().ok()).and_then(server_ms).unwrap_or(0.0);
            Some((total - server).max(0.1))
        };
        if let Err(e) = agent.get(&ping_url).config().timeout_global(Some(m::SPEED_TEST_CONNECT_TIMEOUT)).build().call() {
            finish(Some(format!("No internet connection ({e})")));
            return;
        }
        let rtts = round_trips(pl.pings, &stopped, ping, |rtts, frac| {
            let (lat, jit) = (median(rtts), jitter(rtts));
            set(&|s| {
                s.latency_ms = lat;
                s.jitter_ms = jit;
                s.progress = 0.1 * frac;
            });
        });
        if stopped() {
            return;
        }
        if rtts.is_empty() {
            finish(Some("No internet connection".into()));
            return;
        }
        if stopped() {
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
        let a = agent.clone();
        let down = transfer(pl.down, pl.phase, pl.grace, &stopped, &report(0.1, 0.5), move |b, d| download_stream(&a, b, d));
        if stopped() {
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
        let a = agent.clone();
        let up = transfer(pl.up, pl.phase, pl.grace, &stopped, &report(0.6, 0.4), move |b, d| upload_stream(&a, b, d));
        set(&|s| {
            s.up_mbps = up;
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
    fn stop_does_not_wait_for_a_stuck_stream() {
        // a stream stuck in a read for seconds, as on a stalled connection
        let t0 = Instant::now();
        let stopped = || t0.elapsed() > Duration::from_millis(200);
        let r = transfer(2, Duration::from_secs(10), Duration::ZERO, &stopped, &|_, _| {}, |b, _| {
            b.fetch_add(1_000_000, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(3));
        });
        assert_eq!(r, None);
        assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    }

    #[test]
    fn phase_end_does_not_wait_for_a_stuck_stream() {
        let t0 = Instant::now();
        let r = transfer(2, Duration::from_millis(200), Duration::ZERO, &|| false, &|_, _| {}, |b, _| {
            b.fetch_add(1_000_000, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(3));
        });
        assert!(r.is_some());
        assert!(t0.elapsed() < Duration::from_millis(1500), "{:?}", t0.elapsed());
    }

    #[test]
    fn a_stopped_run_writes_nothing() {
        let shared: Shared = Default::default();
        let cancel = {
            let mut s = shared.lock_ok();
            s.speed = SpeedTest { running: true, phase: SpeedPhase::Download, live_mbps: 120.0, ..Default::default() };
            s.speed.cancel.clone()
        };
        assert!(write(&shared, &cancel, &|s| s.progress = 0.3));
        shared.lock_ok().speed.stop();
        // the thread's late "done" can't undo the stop or touch a new run
        assert!(!write(&shared, &cancel, &|s| {
            s.running = true;
            s.error = Some("Upload test failed".into());
        }));
        let s = shared.lock_ok().speed.clone();
        assert!(!s.running);
        assert_eq!(s.phase, SpeedPhase::Done);
        assert_eq!(s.error, None);
        // the download it had reached stays
        assert_eq!(s.down_mbps, Some(120.0));
        assert_eq!(s.live_mbps, 0.0);
    }

    #[test]
    fn a_lost_ping_counts_for_nothing_and_reconnects_uncounted() {
        use std::cell::RefCell;
        // the 3rd ping is lost; the call after it opens a new connection
        let calls = RefCell::new(Vec::new());
        let ping = |limit: Duration| {
            let k = calls.borrow().len();
            calls.borrow_mut().push(limit);
            match k {
                2 => None,
                3 => Some(900.0), // the reconnect: a handshake, not a trip
                _ => Some(20.0 + k as f64),
            }
        };
        let rtts = round_trips(4, &|| false, ping, |_, _| {});
        assert_eq!(rtts, vec![20.0, 21.0, 24.0]);
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[3], m::SPEED_TEST_CONNECT_TIMEOUT);
        assert!(calls.iter().enumerate().all(|(k, l)| k == 3 || *l == m::SPEED_TEST_PING_TIMEOUT));
        assert!(jitter(&rtts).unwrap() < 5.0);
    }

    #[test]
    fn a_dead_line_gives_up_after_a_few_lost_pings() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let rtts = round_trips(10, &|| false, |_| {
            calls.set(calls.get() + 1);
            None
        }, |_, _| {});
        assert!(rtts.is_empty());
        // 3 pings and the 2 reconnects between them, not 10 of each
        assert_eq!(calls.get(), 2 * LOST_IN_A_ROW - 1);
    }

    #[test]
    fn round_trips_stop_when_stopped() {
        let rtts = round_trips(10, &|| true, |_| Some(20.0), |_, _| {});
        assert!(rtts.is_empty());
    }

    #[test]
    fn transfer_with_nothing_through_is_none() {
        let r = transfer(2, Duration::from_millis(200), Duration::ZERO, &|| false, &|_, _| {}, |_, _| {});
        assert_eq!(r, None);
    }
}
