//! Voice cleanup: DeepFilterNet3 (built in, runs on the CPU) takes the noise
//! out of the microphone, then the sound is mixed again with system audio
//! and kept in a small audio file beside the edit, until Save puts it into
//! the video.

use super::sound::{self, Track};
use super::speech::Progress;
use deep_filter_rt::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Strength 0..1 → how far the model may turn noise down, in dB (1 is no
/// limit at all).
pub fn atten_db(strength: f32) -> f32 {
    let s = strength.clamp(0.0, 1.0);
    if s >= 0.99 {
        100.0
    } else {
        6.0 + s * 34.0
    }
}

/// Take the noise out of `mono` (48 kHz). The output lines up with the
/// input (the model's delay is taken back out).
pub fn denoise(mono: &[f32], strength: f32, p: &Progress) -> Result<Vec<f32>, String> {
    let mut df = DfTract::new(DfParams::default(), &RuntimeParams::default_with_ch(1).with_atten_lim(atten_db(strength))).map_err(|e| e.to_string())?;
    let hop = df.hop_size;
    let delay = df.delay();
    let frames = (mono.len() + delay).div_ceil(hop);
    p.total.store(frames as u64, Ordering::Relaxed);
    let mut out = Vec::with_capacity(frames * hop);
    let mut noisy = Array2::<f32>::zeros((1, hop));
    let mut clean = Array2::<f32>::zeros((1, hop));
    for f in 0..frames {
        if p.cancelled() {
            return Err("Stopped".into());
        }
        for i in 0..hop {
            noisy[[0, i]] = mono.get(f * hop + i).copied().unwrap_or(0.0);
        }
        df.process(noisy.view(), clean.view_mut()).map_err(|e| e.to_string())?;
        out.extend(clean.row(0).iter().copied());
        p.done.store(f as u64 + 1, Ordering::Relaxed);
    }
    Ok(out.into_iter().skip(delay).take(mono.len()).collect())
}

fn to_mono(stereo: &[f32]) -> Vec<f32> {
    stereo.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect()
}

fn to_stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|&v| [v, v]).collect()
}

/// `a + b`, sample by sample (the longer one sets the length).
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    (0..a.len().max(b.len())).map(|i| (a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0)).clamp(-1.0, 1.0)).collect()
}

/// Where a video's cleaned sound is kept while it's being edited.
pub fn sound_path(video: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    video.hash(&mut h);
    std::env::temp_dir().join(format!("cmdmeow-voice-{:016x}.m4a", h.finish()))
}

/// Clean the voice of `video` and write the new sound (the mix first, then
/// the cleaned voice) to [`sound_path`]. Blocks.
pub fn clean(video: &Path, strength: f32, p: &Progress) -> Result<PathBuf, String> {
    let n = sound::audio_tracks(video);
    if n == 0 {
        return Err("This video has no sound".into());
    }
    let voice = sound::read_audio(video, sound::voice_track(n), 48_000, 2)?;
    if voice.iter().all(|v| v.abs() < 1e-4) {
        return Err(if n >= 3 { "The microphone track is silent: nothing to clean up".into() } else { "The sound is silent: nothing to clean up".into() });
    }
    let system = if n >= 3 { Some(sound::read_audio(video, Track::System, 48_000, 2)?) } else { None };
    let cleaned = to_stereo(&denoise(&to_mono(&voice), strength, p)?);
    let mixed = match &system {
        Some(sys) => mix(sys, &cleaned),
        None => cleaned.clone(),
    };
    let out = sound_path(video);
    sound::write_m4a(&out, &[&mixed, &cleaned])?;
    Ok(out)
}

/// The loudness of `samples` in `n` buckets, 0..1, for drawing.
pub fn peaks(samples: &[f32], n: usize) -> Vec<f32> {
    if samples.is_empty() || n == 0 {
        return Vec::new();
    }
    let per = samples.len().div_ceil(n).max(1);
    samples.chunks(per).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())).min(1.0)).collect()
}

/// The voice's loudness over the whole video, for the timeline.
pub fn voice_peaks(video: &Path, n: usize) -> Vec<f32> {
    let count = sound::audio_tracks(video);
    match sound::read_audio(video, sound::voice_track(count), 8_000, 1) {
        Ok(s) => peaks(&s, n),
        Err(_) => Vec::new(),
    }
}

/// The cleaned voice's loudness (the second track of the cleaned sound).
pub fn cleaned_peaks(sound_file: &Path, n: usize) -> Vec<f32> {
    sound::read_nth(sound_file, 1, 8_000, 1).map(|s| peaks(&s, n)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_sets_how_far_noise_goes_down() {
        assert_eq!(atten_db(0.0), 6.0);
        assert_eq!(atten_db(0.5), 23.0);
        assert_eq!(atten_db(1.0), 100.0);
    }

    #[test]
    fn noise_goes_down_and_the_timing_stays() {
        // a 440 Hz tone in the middle, white-ish noise all along
        let n = 48_000;
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.05
        };
        let x: Vec<f32> = (0..n).map(|_| noise()).collect();
        let p = Progress::default();
        let y = denoise(&x, 1.0, &p).unwrap();
        assert_eq!(y.len(), x.len());
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        assert!(rms(&y[24_000..]) < rms(&x[24_000..]) * 0.5, "noise alone is turned down: {} vs {}", rms(&y), rms(&x));
    }

    #[test]
    fn mixing_and_peaks() {
        assert_eq!(mix(&[0.5, 0.75], &[0.75]), vec![1.0, 0.75]);
        assert_eq!(peaks(&[0.1, -0.5, 0.2, 0.3], 2), vec![0.5, 0.3]);
        assert_eq!(to_stereo(&to_mono(&[0.2, 0.4])), vec![0.3f32, 0.3]);
    }
}
