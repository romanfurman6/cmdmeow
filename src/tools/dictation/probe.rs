//! `--dictation-probe` runs dictation's work from the command line, without
//! the hotkey or the pill, so a script can check it on a real Mac:
//!
//! `--dictation-probe <file.wav | mic:SECS> [--download] [--whisper]
//! [--lang uk|en|ru] [--paste]` reads a WAV (16-bit or float) or listens to
//! the microphone for SECS seconds, turns it into text with the chosen model
//! (downloaded first with `--download`) and prints it. `--paste` then pastes
//! it into the front app the way dictation does, and puts the clipboard back.

use super::audio::{self, Mic, MODEL_RATE};
use super::mac;
use super::settings::Engine;
use crate::kit::whisper::Progress;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub fn probe(a: &[String]) -> Result<(), String> {
    let input = a.first().ok_or("say a WAV file or mic:SECS")?;
    let has = |f: &str| a.iter().any(|v| v == f);
    let engine = if has("--whisper") { Engine::Whisper } else { Engine::Parakeet };
    let lang = match a.iter().position(|v| v == "--lang") {
        Some(k) => match a.get(k + 1) {
            Some(l) if !l.starts_with("--") => Some(l.as_str()),
            _ => return Err("--lang needs a language code, like uk".into()),
        },
        None => None,
    };
    if !engine.ready() {
        if !has("--download") {
            return Err(format!("{} isn't downloaded; add --download", engine.name()));
        }
        let t = Instant::now();
        let p = Progress::default();
        match engine {
            Engine::Parakeet => crate::kit::parakeet::download(&p, &AtomicBool::new(false))?,
            Engine::Whisper => crate::kit::whisper::download(crate::kit::whisper::Model::Accurate, &p)?,
        }
        eprintln!("[dictation] downloaded {} in {:?}", engine.name(), t.elapsed());
    }
    let pcm = match input.strip_prefix("mic:") {
        Some(secs) => {
            let secs = secs.parse::<f32>().ok().and_then(|s| Duration::try_from_secs_f32(s).ok()).ok_or("mic:SECS needs a number of seconds")?;
            let mic = Mic::start()?;
            eprintln!("[dictation] listening {secs:?}");
            std::thread::sleep(secs);
            if let Some(e) = mic.failed() {
                return Err(e);
            }
            mic.stop()
        }
        None => read_wav(&std::fs::read(input).map_err(|e| format!("{input}: {e}"))?)?,
    };
    let secs = pcm.len() as f32 / MODEL_RATE as f32;
    let peak = pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let silent = audio::silent(&pcm, MODEL_RATE);
    eprintln!("[dictation] {secs:.2} s of voice, peak {peak:.3}, silent {silent}");
    if silent {
        return Err("Nothing heard (silent input; for the mic, check the Microphone permission)".into());
    }
    let t = Instant::now();
    let m = super::Loaded::load(engine)?;
    eprintln!("[dictation] loaded {} in {:?}", engine.name(), t.elapsed());
    let t = Instant::now();
    let keep = super::cut::trim(&pcm, MODEL_RATE);
    eprintln!("[dictation] kept {:.2}–{:.2} s, {} piece(s)", keep.start as f32 / MODEL_RATE as f32, keep.end as f32 / MODEL_RATE as f32, super::cut::split(&pcm[keep.clone()], MODEL_RATE).len());
    let text = m.run(&pcm[keep], lang, None)?;
    eprintln!("[dictation] ran in {:?}", t.elapsed());
    if text.is_empty() {
        return Err("Nothing heard".into());
    }
    println!("{text}");
    if has("--paste") {
        if !mac::accessibility() {
            return Err("pasting needs Accessibility for the process running the probe".into());
        }
        let app = mac::front_app().map(|t| t.name).unwrap_or_default();
        let (saved, count) = mac::paste(&text, true).ok_or("pasting failed")?;
        std::thread::sleep(Duration::from_millis(600));
        mac::restore(saved, count);
        eprintln!("[dictation] pasted into {app:?}, clipboard put back");
    }
    Ok(())
}

/// The samples of a PCM WAV (16-bit or 32-bit float), at 16 kHz mono.
pub fn read_wav(b: &[u8]) -> Result<Vec<f32>, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at = |k: usize| u16::from_le_bytes([b[k], b[k + 1]]);
    let u32_at = |k: usize| u32::from_le_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]);
    let (mut fmt, mut data) = (None, None);
    let mut k = 12;
    while k + 8 <= b.len() {
        let len = u32_at(k + 4) as usize;
        let body = k + 8..k + 8 + len;
        if body.end > b.len() {
            return Err("the WAV file is cut short".into());
        }
        match &b[k..k + 4] {
            b"fmt " if len >= 16 => {
                let mut format = u16_at(k + 8);
                // extensible: the real format is the first two bytes of the subformat GUID
                if format == 0xFFFE && len >= 40 {
                    format = u16_at(k + 32);
                }
                fmt = Some((format, u16_at(k + 10) as usize, u32_at(k + 12), u16_at(k + 22)))
            }
            b"data" => data = Some(body),
            _ => {}
        }
        k += 8 + len + (len & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("WAV without a format")?;
    if channels == 0 || rate == 0 {
        return Err("the WAV header has no channels or no sample rate".into());
    }
    let data = &b[data.ok_or("WAV without samples")?];
    let samples: Vec<f32> = match (format, bits) {
        (1, 16) => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        (1, 32) => data.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0).collect(),
        (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        _ => return Err(format!("WAV format {format} at {bits} bits isn't read; use 16-bit PCM")),
    };
    Ok(audio::resample(&audio::to_mono(&samples, channels, false), rate as f64, MODEL_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        build(1, rate, channels, 16, &data)
    }

    /// A WAV of raw `data`; format 0xFFFE writes the extensible header with
    /// PCM as its subformat.
    fn build(format: u16, rate: u32, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let ext = format == 0xFFFE;
        let fmt_len: u32 = if ext { 40 } else { 16 };
        let block = channels * bits / 8;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(20 + fmt_len + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&fmt_len.to_le_bytes());
        b.extend_from_slice(&format.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * block as u32).to_le_bytes());
        b.extend_from_slice(&block.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        if ext {
            b.extend_from_slice(&22u16.to_le_bytes());
            b.extend_from_slice(&bits.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            // KSDATAFORMAT_SUBTYPE_PCM
            b.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn reads_wav_at_model_rate() {
        let pcm = read_wav(&wav(16_000, 1, &[16384; 1600])).unwrap();
        assert_eq!(pcm.len(), 1600);
        assert!((pcm[10] - 0.5).abs() < 1e-3);
        // stereo at 32 kHz: half the frames, one channel
        let pcm = read_wav(&wav(32_000, 2, &[8192; 6400])).unwrap();
        assert!((pcm.len() as i64 - 1600).abs() <= 2, "{}", pcm.len());
        assert!(read_wav(b"nope").is_err());
    }

    #[test]
    fn reads_extensible_pcm_and_rejects_bad_headers() {
        let d16: Vec<u8> = [16384i16; 160].iter().flat_map(|s| s.to_le_bytes()).collect();
        let pcm = read_wav(&build(0xFFFE, 16_000, 1, 16, &d16)).unwrap();
        assert!((pcm[5] - 0.5).abs() < 1e-3);
        let d32: Vec<u8> = [1i32 << 30; 160].iter().flat_map(|s| s.to_le_bytes()).collect();
        let pcm = read_wav(&build(0xFFFE, 16_000, 1, 32, &d32)).unwrap();
        assert!((pcm[5] - 0.5).abs() < 1e-3, "{}", pcm[5]);
        assert!(read_wav(&build(1, 0, 1, 16, &d16)).is_err());
        assert!(read_wav(&build(1, 16_000, 0, 16, &d16)).is_err());
        let mut cut = build(1, 16_000, 1, 16, &d16);
        cut.truncate(cut.len() - 10);
        assert!(read_wav(&cut).is_err());
    }
}
