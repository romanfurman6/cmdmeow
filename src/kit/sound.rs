//! Short, quiet sounds for a few moments: a scan is ready, a cleanup is
//! done, Keep Awake goes on or off, something failed. Every sound is
//! synthesized here (no audio files ship with the app), written once as a
//! WAV into the app's data folder and played with `afplay`. One switch in
//! Settings turns them all off.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Settings key (eframe storage) of the switch, a `bool`.
pub const STORAGE_KEY: &str = "sounds_on";

/// Bump when a sound changes, so the cached WAVs are written again.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const VERSION: u32 = 2;
const RATE: u32 = 44_100;
/// Loudest sample of any cue (full scale is 1.0): quiet next to system sounds.
const PEAK: f32 = 0.32;

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

static ON: AtomicBool = AtomicBool::new(true);
/// When the last cue started (ms since UNIX epoch), to drop bursts.
static LAST: AtomicU64 = AtomicU64::new(0);

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Play `cue` in the background, unless sounds are off or another cue
/// started less than a quarter second ago.
pub fn play(cue: Cue) {
    if !on() {
        return;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let last = LAST.swap(now, Ordering::Relaxed);
    if now.saturating_sub(last) < 250 {
        return;
    }
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        if let Some(path) = file(cue) {
            let _ = std::process::Command::new("afplay").arg(path).status();
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = cue;
}

/// The cue's WAV in the data folder, written on first use.
#[cfg(target_os = "macos")]
fn file(cue: Cue) -> Option<std::path::PathBuf> {
    let path = super::fs::data_path("sounds").join(format!("{}-v{VERSION}.wav", cue.name()));
    if !path.exists() {
        std::fs::create_dir_all(path.parent()?).ok()?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, wav(&synth(cue))).ok()?;
        std::fs::rename(&tmp, &path).ok()?;
    }
    Some(path)
}

// ---------------------------------------------------------------- synthesis

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

/// The samples of `cue`, mono at 44.1 kHz, peaking at [`PEAK`].
pub fn synth(cue: Cue) -> Vec<f32> {
    let len = match cue {
        Cue::Ready | Cue::Oops => 0.4,
        Cue::Done => 0.6,
        Cue::Awake | Cue::Asleep => 0.35,
    };
    let mut out = vec![0.0f32; (len * RATE as f32) as usize];
    match cue {
        // two pops stepping up
        Cue::Ready => {
            pop(&mut out, 0.0, 520.0, 700.0, 0.06, 0.8, 0.06);
            pop(&mut out, 0.09, 700.0, 1040.0, 0.07, 1.0, 0.08);
        }
        // three pops climbing, the last one rings a little longer
        Cue::Done => {
            pop(&mut out, 0.0, 440.0, 660.0, 0.06, 0.7, 0.06);
            pop(&mut out, 0.085, 660.0, 880.0, 0.06, 0.85, 0.06);
            pop(&mut out, 0.17, 880.0, 1320.0, 0.07, 1.0, 0.12);
        }
        Cue::Awake => pop(&mut out, 0.0, 520.0, 880.0, 0.07, 1.0, 0.07),
        Cue::Asleep => pop(&mut out, 0.0, 880.0, 520.0, 0.07, 1.0, 0.07),
        // two low pops sliding down
        Cue::Oops => {
            pop(&mut out, 0.0, 440.0, 330.0, 0.07, 1.0, 0.06);
            pop(&mut out, 0.12, 400.0, 290.0, 0.08, 0.9, 0.07);
        }
    }
    // fade the last 20 ms so nothing clicks, then set the level
    let fade = (0.02 * RATE as f32) as usize;
    let n = out.len();
    for (j, s) in out[n - fade..].iter_mut().enumerate() {
        *s *= 1.0 - j as f32 / fade as f32;
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        for s in out.iter_mut() {
            *s *= PEAK / peak;
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_quiet_and_end_silent() {
        for c in Cue::ALL {
            let s = synth(c);
            assert!(!s.is_empty() && s.len() <= (1.25 * RATE as f32) as usize, "{c:?} too long");
            let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - PEAK).abs() < 1e-3, "{c:?} peak {peak}");
            assert!(s.last().unwrap().abs() < 1e-3, "{c:?} clicks at the end");
            assert!(s.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn wav_header_matches_the_data() {
        let s = synth(Cue::Awake);
        let b = wav(&s);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()) as usize, s.len() * 2);
        assert_eq!(b.len(), 44 + s.len() * 2);
    }

    #[test]
    fn switch_off_is_respected() {
        set_on(false);
        assert!(!on());
        play(Cue::Done); // no-op, no panic
        set_on(true);
        assert!(on());
    }

    /// Writes every cue as a WAV into `$SOUND_PREVIEW_DIR` (to listen to them).
    #[test]
    #[ignore]
    fn write_previews() {
        let dir = std::path::PathBuf::from(std::env::var("SOUND_PREVIEW_DIR").unwrap_or_else(|_| "sound-previews".into()));
        std::fs::create_dir_all(&dir).unwrap();
        for c in Cue::ALL {
            std::fs::write(dir.join(format!("{}.wav", c.name())), wav(&synth(c))).unwrap();
        }
    }
}
