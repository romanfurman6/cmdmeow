//! The app's sounds, one family in A major: a click when a button is
//! pressed and a few short cues (a scan is ready, a cleanup is done, Keep
//! Awake goes on or off, something failed).
//!
//! Clicks come from three recorded takes of one plastic click that ship in
//! the binary (`assets/sounds/click-*.wav`). Each kind of button plays them
//! at its own pitch and level (three tiers, see [`Click`]); cues are
//! synthesized here and open with a quiet click, so both sound related.
//! Every sound is rendered once into a WAV in the app's data folder and
//! played as a macOS system sound (AudioToolbox, low latency). Clicks are
//! flagged as UI sounds, so the macOS "Play user interface sound effects"
//! setting mutes them. One switch in Settings turns everything off, and
//! nothing plays while a recording runs.
//!
//! Levels and pitches live in code (the take files are plain recordings,
//! peaking at -1 dBFS), so tuning never needs new files.

// Playback is macOS only; elsewhere the rendering still builds and is tested.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Settings key (eframe storage) of the switch, a `bool`.
pub const STORAGE_KEY: &str = "sounds_on";

/// Bump when a sound changes, so the cached WAVs are written again.
const VERSION: u32 = 3;
const RATE: u32 = 44_100;
/// Loudest sample of any cue (full scale is 1.0): quiet next to system sounds.
const PEAK: f32 = 0.32;
/// Level of the loudest click tier against the cues, in dB. 0 = clicks peak
/// like the cues. Tune by ear on a real Mac.
const CLICK_MASTER_DB: f32 = 0.0;
/// The quiet click every cue opens with, in dB under the cue.
const CUE_TICK_DB: f32 = -12.0;
/// Clicks closer together than this play once (fast clicking through a
/// list of checkboxes shouldn't machine-gun).
const CLICK_GAP_MS: u64 = 100;
/// Cues closer together than this play once.
const CUE_GAP_MS: u64 = 250;

/// The recorded takes of the click (mono, 44.1 kHz, 16-bit). Their level
/// doesn't matter: every click is normalized in code.
const TAKES: [&[u8]; 3] = [
    include_bytes!("../../assets/sounds/click-1.wav"),
    include_bytes!("../../assets/sounds/click-2.wav"),
    include_bytes!("../../assets/sounds/click-3.wav"),
];
/// How each take is varied (pitch factor, level in dB), so repeats don't
/// sound like a machine: within ±3% pitch and ±6% level.
const TAKE_VARY: [(f32, f32); 3] = [(1.0, 0.0), (1.025, -0.5), (0.975, 0.4)];

// ------------------------------------------------------------------- clicks

/// Which kind of control was pressed. Kit helpers call [`click`] with their
/// kind on mouse down; callers never play clicks themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Hero,
    Primary,
    Danger,
    Secondary,
    Side,
    Icon,
    Round,
    Tool,
    /// Toggle and filter chips, the AI chip.
    Chip,
    Segment,
    Switch,
    Check,
    Radio,
    /// The shortcut recorder field.
    Shortcut,
    /// Ghost buttons and links.
    Text,
}

/// How loud a kind of click is: the main action of a view is the loudest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Hero, primary, danger: 0 dB.
    Emphasized,
    /// Secondary, side, icon, round, tool: -3 dB.
    Standard,
    /// Choices and text buttons: -6 dB.
    Subtle,
}

impl Tier {
    pub fn db(self) -> f32 {
        match self {
            Tier::Emphasized => 0.0,
            Tier::Standard => -3.0,
            Tier::Subtle => -6.0,
        }
    }
}

/// Every distinct (tier, semitones) a click is rendered at. Pitch steps are
/// whole musical intervals, so all clicks stay in one tonal family.
const VOICES: [(Tier, i8); 9] = [
    (Tier::Emphasized, -2), // hero: a tone down, fuller
    (Tier::Emphasized, 0),  // primary
    (Tier::Emphasized, -3), // danger: a minor third down, calm, not alarming
    (Tier::Standard, 2),
    (Tier::Standard, 3),
    (Tier::Subtle, 2),
    (Tier::Subtle, 3),
    (Tier::Subtle, 4),
    (Tier::Subtle, 5),
];

impl Click {
    pub const ALL: [Click; 15] = [
        Click::Hero,
        Click::Primary,
        Click::Danger,
        Click::Secondary,
        Click::Side,
        Click::Icon,
        Click::Round,
        Click::Tool,
        Click::Chip,
        Click::Segment,
        Click::Switch,
        Click::Check,
        Click::Radio,
        Click::Shortcut,
        Click::Text,
    ];

    /// Index into [`VOICES`].
    fn voice(self) -> usize {
        match self {
            Click::Hero => 0,
            Click::Primary => 1,
            Click::Danger => 2,
            Click::Secondary | Click::Side => 3,
            Click::Icon | Click::Round | Click::Tool => 4,
            Click::Chip | Click::Segment => 5,
            Click::Switch => 6,
            Click::Check | Click::Radio => 7,
            Click::Shortcut | Click::Text => 8,
        }
    }

    pub fn tier(self) -> Tier {
        VOICES[self.voice()].0
    }

    pub fn semitones(self) -> i8 {
        VOICES[self.voice()].1
    }
}

static ON: AtomicBool = AtomicBool::new(true);
/// When the last cue / click started (ms since UNIX epoch), to drop bursts.
static LAST_CUE: AtomicU64 = AtomicU64::new(0);
static LAST_CLICK: AtomicU64 = AtomicU64::new(0);
/// The take the last click used, so the same take never plays twice in a row.
static LAST_TAKE: AtomicUsize = AtomicUsize::new(usize::MAX);

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Sounds may play now: the switch is on and no recording is running.
fn allowed() -> bool {
    on() && !super::activity::recording()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Whether at least `gap` ms passed since `last`; records now if so.
fn pace(last: &AtomicU64, gap: u64) -> bool {
    let now = now_ms();
    let prev = last.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < gap {
        return false;
    }
    last.store(now, Ordering::Relaxed);
    true
}

/// The take to play next: any but the last one.
fn next_take() -> usize {
    let last = LAST_TAKE.load(Ordering::Relaxed);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0);
    let mut t = (seed >> 7) % TAKES.len();
    if t == last {
        t = (t + 1 + (seed >> 3) % (TAKES.len() - 1)) % TAKES.len();
    }
    LAST_TAKE.store(t, Ordering::Relaxed);
    t
}

/// Play the click of `kind`. Called by kit helpers on mouse down, never
/// for a disabled or busy control, never for a click the user didn't make.
pub fn click(kind: Click) {
    if !allowed() || !pace(&LAST_CLICK, CLICK_GAP_MS) {
        return;
    }
    let take = next_take();
    #[cfg(target_os = "macos")]
    mac::play(&click_name(kind.voice(), take), true, move || render_click(kind.voice(), take));
    #[cfg(not(target_os = "macos"))]
    let _ = (kind, take);
}

fn click_name(voice: usize, take: usize) -> String {
    let (tier, st) = VOICES[voice];
    format!("click-{tier:?}{st:+}-{take}-v{VERSION}").to_lowercase()
}

/// The samples of one click voice and take, peaking at its tier's level.
pub fn render_click(voice: usize, take: usize) -> Vec<f32> {
    let (tier, st) = VOICES[voice];
    let (vary_pitch, vary_db) = TAKE_VARY[take];
    let src = decode(TAKES[take]);
    let mut out = resample(&src, 2f32.powf(st as f32 / 12.0) * vary_pitch);
    normalize(&mut out, PEAK * db(CLICK_MASTER_DB + tier.db() + vary_db));
    out
}

// --------------------------------------------------------------------- cues

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A scan finished and its results are ready.
    Ready,
    /// A cleanup or removal finished.
    Done,
    /// Keep Awake turned on.
    Awake,
    /// Keep Awake turned off.
    Asleep,
    /// Something didn't work (some items couldn't be removed).
    Oops,
}

impl Cue {
    pub const ALL: [Cue; 5] = [Cue::Ready, Cue::Done, Cue::Awake, Cue::Asleep, Cue::Oops];

    pub fn name(self) -> &'static str {
        match self {
            Cue::Ready => "ready",
            Cue::Done => "done",
            Cue::Awake => "awake",
            Cue::Asleep => "asleep",
            Cue::Oops => "oops",
        }
    }
}

/// Play `cue` in the background, unless sounds are off, a recording runs or
/// another cue started less than a quarter second ago.
pub fn play(cue: Cue) {
    if !allowed() || !pace(&LAST_CUE, CUE_GAP_MS) {
        return;
    }
    #[cfg(target_os = "macos")]
    mac::play(&format!("{}-v{VERSION}", cue.name()), false, move || synth(cue));
    #[cfg(not(target_os = "macos"))]
    let _ = cue;
}

/// Render every sound into the data folder on a background thread, so the
/// first click doesn't wait for it.
pub fn prewarm() {
    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        super::activity::low_priority();
        for v in 0..VOICES.len() {
            for t in 0..TAKES.len() {
                mac::load(&click_name(v, t), true, || render_click(v, t));
            }
        }
        for c in Cue::ALL {
            mac::load(&format!("{}-v{VERSION}", c.name()), false, || synth(c));
        }
    });
}

// ---------------------------------------------------------------- synthesis

/// A note of the A major family, `n` semitones from A4 (440 Hz).
fn note(n: i32) -> f32 {
    440.0 * 2f32.powf(n as f32 / 12.0)
}

fn db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A short pitch glide (a soft "pop"), `from` to `to` Hz over `glide` s.
fn pop(out: &mut [f32], start: f32, from: f32, to: f32, glide: f32, amp: f32, decay: f32) {
    let s0 = (start * RATE as f32) as usize;
    let mut phase = 0.0f32;
    for (i, o) in out.iter_mut().enumerate().skip(s0) {
        let t = (i - s0) as f32 / RATE as f32;
        let k = (t / glide).min(1.0);
        // ease out: most of the bend happens at the start
        let f = from + (to - from) * (1.0 - (1.0 - k).powi(3));
        phase += TAU * f / RATE as f32;
        let attack = (t / 0.004).min(1.0);
        *o += amp * attack * phase.sin() * (-t / decay).exp();
    }
}

/// The samples of `cue`, mono at 44.1 kHz, peaking at [`PEAK`]. Tonal
/// centre A major (A4 C#5 E5 A5); each cue opens with a quiet click.
pub fn synth(cue: Cue) -> Vec<f32> {
    let len = match cue {
        Cue::Ready => 0.4,
        Cue::Done => 0.6,
        Cue::Oops => 0.45,
        Cue::Awake | Cue::Asleep => 0.35,
    };
    let mut out = vec![0.0f32; (len * RATE as f32) as usize];
    match cue {
        // C#5 → E5 → A5: up to the tonic
        Cue::Ready => {
            pop(&mut out, 0.0, note(4), note(7), 0.05, 0.8, 0.06);
            pop(&mut out, 0.09, note(7), note(12), 0.06, 1.0, 0.08);
        }
        // A4 C#5 E5 A5 arpeggio, the last note rings
        Cue::Done => {
            pop(&mut out, 0.0, note(0), note(4), 0.05, 0.7, 0.06);
            pop(&mut out, 0.085, note(4), note(7), 0.05, 0.85, 0.06);
            pop(&mut out, 0.17, note(7), note(12), 0.06, 1.0, 0.13);
        }
        // E5 ↔ A5, mirrors of each other
        Cue::Awake => pop(&mut out, 0.0, note(7), note(12), 0.06, 1.0, 0.07),
        Cue::Asleep => pop(&mut out, 0.0, note(12), note(7), 0.06, 1.0, 0.07),
        // A4 → G#4 → E4: falling, not an alarm
        Cue::Oops => {
            pop(&mut out, 0.0, note(0), note(-1), 0.06, 1.0, 0.07);
            pop(&mut out, 0.13, note(-1), note(-5), 0.08, 0.9, 0.09);
        }
    }
    normalize(&mut out, 1.0);
    // the quiet click on top, then fade the last 20 ms so nothing clicks
    let tick = decode(TAKES[0]);
    for (o, t) in out.iter_mut().zip(tick.iter()) {
        *o += t / peak(&tick).max(1e-6) * db(CUE_TICK_DB);
    }
    let fade = (0.02 * RATE as f32) as usize;
    let n = out.len();
    for (j, s) in out[n - fade..].iter_mut().enumerate() {
        *s *= 1.0 - j as f32 / fade as f32;
    }
    // Keep Awake on/off sits a little under the others
    let level = if matches!(cue, Cue::Awake | Cue::Asleep) { db(-3.0) } else { 1.0 };
    normalize(&mut out, PEAK * level);
    out
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

fn normalize(s: &mut [f32], to: f32) {
    let p = peak(s);
    if p > 0.0 {
        for x in s.iter_mut() {
            *x *= to / p;
        }
    }
}

/// Play `src` `factor` times faster (higher and shorter), linear interpolation.
fn resample(src: &[f32], factor: f32) -> Vec<f32> {
    if src.is_empty() || factor <= 0.0 {
        return Vec::new();
    }
    let n = ((src.len() as f32) / factor) as usize;
    (0..n)
        .map(|i| {
            let x = i as f32 * factor;
            let k = x as usize;
            let f = x - k as f32;
            let a = src[k.min(src.len() - 1)];
            let b = src[(k + 1).min(src.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// The samples of a 16-bit mono PCM WAV (other chunks skipped).
pub fn decode(bytes: &[u8]) -> Vec<f32> {
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap_or([0; 4])) as usize;
        let body = i + 8;
        if id == b"data" {
            let end = (body + size).min(bytes.len());
            return bytes[body..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        }
        i = body + size + (size & 1);
    }
    Vec::new()
}

/// 16-bit mono PCM WAV bytes.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

// ------------------------------------------------------------------ playback

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};

    use objc2_foundation::{NSString, NSURL};

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioServicesCreateSystemSoundID(url: *const c_void, out: *mut u32) -> i32;
        fn AudioServicesPlaySystemSound(id: u32);
        fn AudioServicesSetProperty(prop: u32, spec_size: u32, spec: *const c_void, size: u32, data: *const c_void) -> i32;
    }

    /// `kAudioServicesPropertyIsUISound` ('isui'): the sound follows the
    /// macOS "Play user interface sound effects" setting.
    const IS_UI_SOUND: u32 = u32::from_be_bytes(*b"isui");

    /// Loaded system sounds by name (0 = couldn't be loaded, don't retry).
    fn ids() -> &'static Mutex<HashMap<String, u32>> {
        static IDS: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
        IDS.get_or_init(Default::default)
    }

    /// The system sound `name`, written from `render` on first use. The
    /// lock is only held to look up and insert, never while rendering.
    pub fn load(name: &str, ui: bool, render: impl FnOnce() -> Vec<f32>) -> u32 {
        if let Some(&id) = ids().lock().unwrap_or_else(|e| e.into_inner()).get(name) {
            return id;
        }
        let id = create(name, ui, render).unwrap_or(0);
        *ids().lock().unwrap_or_else(|e| e.into_inner()).entry(name.to_string()).or_insert(id)
    }

    fn create(name: &str, ui: bool, render: impl FnOnce() -> Vec<f32>) -> Option<u32> {
        let path = crate::kit::fs::data_path("sounds").join(format!("{name}.wav"));
        if !path.exists() {
            std::fs::create_dir_all(path.parent()?).ok()?;
            // a temp name of its own, so two threads writing one sound don't mix
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let tmp = path.with_extension(format!("tmp{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::write(&tmp, super::wav(&render())).ok()?;
            std::fs::rename(&tmp, &path).ok()?;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        let mut id = 0u32;
        // SAFETY: NSURL is toll-free bridged to CFURL; the call copies what it needs.
        let err = unsafe { AudioServicesCreateSystemSoundID(objc2::rc::Retained::as_ptr(&url) as *const c_void, &mut id) };
        if err != 0 || id == 0 {
            return None;
        }
        if ui {
            let flag: u32 = 1;
            // SAFETY: the specifier is the sound id, the value a UInt32, both live for the call.
            unsafe { AudioServicesSetProperty(IS_UI_SOUND, 4, &id as *const u32 as *const c_void, 4, &flag as *const u32 as *const c_void) };
        }
        Some(id)
    }

    /// Play `name` (async, returns at once).
    pub fn play(name: &str, ui: bool, render: impl FnOnce() -> Vec<f32> + Send + 'static) {
        // a loaded sound plays at once; the UI thread never waits on the lock
        // or renders: a sound not loaded yet is loaded and played off-thread
        if let Some(id) = ids().try_lock().ok().and_then(|m| m.get(name).copied()) {
            play_id(id);
            return;
        }
        let name = name.to_string();
        std::thread::spawn(move || play_id(load(&name, ui, render)));
    }

    fn play_id(id: u32) {
        if id != 0 {
            // SAFETY: a sound id from AudioServicesCreateSystemSoundID; the call doesn't block.
            unsafe { AudioServicesPlaySystemSound(id) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_quiet_and_end_silent() {
        for c in Cue::ALL {
            let s = synth(c);
            assert!(!s.is_empty() && s.len() <= (1.25 * RATE as f32) as usize, "{c:?} too long");
            let want = if matches!(c, Cue::Awake | Cue::Asleep) { PEAK * db(-3.0) } else { PEAK };
            assert!((peak(&s) - want).abs() < 1e-3, "{c:?} peak {}", peak(&s));
            assert!(s.last().unwrap().abs() < 1e-3, "{c:?} clicks at the end");
            assert!(s.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn takes_decode() {
        for t in TAKES {
            let s = decode(t);
            // 45–90 ms of sound, recorded near full scale
            assert!(s.len() > RATE as usize * 40 / 1000 && s.len() < RATE as usize * 100 / 1000, "{} samples", s.len());
            assert!(peak(&s) > 0.1, "take is silent");
        }
    }

    #[test]
    fn clicks_follow_their_tier() {
        for c in Click::ALL {
            for t in 0..TAKES.len() {
                let s = render_click(c.voice(), t);
                assert!(!s.is_empty() && s.iter().all(|x| x.is_finite()));
                let want = PEAK * db(CLICK_MASTER_DB + c.tier().db() + TAKE_VARY[t].1);
                assert!((peak(&s) - want).abs() < 1e-3, "{c:?} take {t}: {} vs {want}", peak(&s));
            }
        }
        // higher pitch, shorter sound
        assert!(render_click(Click::Text.voice(), 0).len() < render_click(Click::Hero.voice(), 0).len());
        assert_eq!(Click::Primary.tier(), Tier::Emphasized);
        assert_eq!(Click::Danger.semitones(), -3);
        assert_eq!(Click::Check.tier(), Tier::Subtle);
    }

    #[test]
    fn takes_never_repeat() {
        let mut last = next_take();
        for _ in 0..200 {
            let t = next_take();
            assert_ne!(t, last);
            last = t;
        }
    }

    #[test]
    fn wav_round_trips() {
        let s = synth(Cue::Awake);
        let b = wav(&s);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()) as usize, s.len() * 2);
        assert_eq!(b.len(), 44 + s.len() * 2);
        let back = decode(&b);
        assert_eq!(back.len(), s.len());
        assert!(back.iter().zip(&s).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn switch_off_is_respected() {
        set_on(false);
        assert!(!on());
        play(Cue::Done); // no-op, no panic
        click(Click::Primary);
        set_on(true);
        assert!(on());
    }

    /// Writes every cue and click voice as a WAV into `$SOUND_PREVIEW_DIR`.
    #[test]
    #[ignore]
    fn write_previews() {
        let dir = std::path::PathBuf::from(std::env::var("SOUND_PREVIEW_DIR").unwrap_or_else(|_| "sound-previews".into()));
        std::fs::create_dir_all(&dir).unwrap();
        for c in Cue::ALL {
            std::fs::write(dir.join(format!("{}.wav", c.name())), wav(&synth(c))).unwrap();
        }
        for v in 0..VOICES.len() {
            for t in 0..TAKES.len() {
                std::fs::write(dir.join(format!("{}.wav", click_name(v, t))), wav(&render_click(v, t))).unwrap();
            }
        }
    }
}
