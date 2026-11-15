//! Where to cut the voice before the model hears it: the quiet at both ends
//! goes (models make words up in silence, and it costs time), and a long
//! take is split at its pauses, because speech models get worse past about
//! half a minute in one piece.
//!
//! Loudness is measured in 20 ms frames; "speech" is a frame clearly above
//! the take's own background noise.

use super::audio::rms;
use std::ops::Range;

/// Frame length, ms.
const FRAME_MS: usize = 20;
/// Kept before the first and after the last speech, ms: soft starts and
/// trailing consonants stay in.
const PAD_BEFORE_MS: usize = 250;
const PAD_AFTER_MS: usize = 350;
/// Speech must last this many frames in a row to count (a key click doesn't).
const MIN_RUN: usize = 3;
/// Takes up to this long go to the model in one piece, seconds.
const ONE_PIECE_SECS: usize = 35;
/// A piece of a long take: at least / at most, seconds.
const PIECE_MIN_SECS: usize = 15;
const PIECE_MAX_SECS: usize = 30;
/// The last piece is never shorter than this, seconds.
const REST_MIN_SECS: usize = 5;

fn frame_len(rate: u32) -> usize {
    (rate as usize * FRAME_MS / 1000).max(1)
}

/// Loudness of each frame.
fn frames(pcm: &[f32], rate: u32) -> Vec<f32> {
    pcm.chunks(frame_len(rate)).map(rms).collect()
}

/// The level above which a frame is speech: well over the quietest tenth
/// of the take, but never so high that soft speech falls under it.
fn threshold(levels: &[f32]) -> f32 {
    if levels.is_empty() {
        return 0.006;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 10];
    (floor * 2.5).clamp(0.006, 0.015)
}

/// Frames that are speech, with runs shorter than [`MIN_RUN`] dropped.
fn speech(levels: &[f32]) -> Vec<bool> {
    let thr = threshold(levels);
    let mut out: Vec<bool> = levels.iter().map(|l| *l > thr).collect();
    let mut k = 0;
    while k < out.len() {
        if !out[k] {
            k += 1;
            continue;
        }
        let start = k;
        while k < out.len() && out[k] {
            k += 1;
        }
        if k - start < MIN_RUN {
            out[start..k].iter_mut().for_each(|v| *v = false);
        }
    }
    out
}

/// The samples worth keeping: from a little before the first speech to a
/// little after the last. All of it when no speech is found.
pub fn trim(pcm: &[f32], rate: u32) -> Range<usize> {
    let fl = frame_len(rate);
    let sp = speech(&frames(pcm, rate));
    let (Some(first), Some(last)) = (sp.iter().position(|s| *s), sp.iter().rposition(|s| *s)) else {
        return 0..pcm.len();
    };
    let ms = |m: usize| rate as usize * m / 1000;
    let start = (first * fl).saturating_sub(ms(PAD_BEFORE_MS));
    let end = ((last + 1) * fl + ms(PAD_AFTER_MS)).min(pcm.len());
    start..end
}

/// `pcm` in pieces the model handles well, cut in the middle of the
/// longest pause in each 15–30 s window (or its quietest moment if there is
/// no pause). Pieces cover all of `pcm`, in order, without overlap.
pub fn split(pcm: &[f32], rate: u32) -> Vec<Range<usize>> {
    let secs = |s: usize| s * rate as usize;
    if pcm.len() <= secs(ONE_PIECE_SECS) {
        return vec![0..pcm.len()];
    }
    let fl = frame_len(rate);
    let levels = frames(pcm, rate);
    let sp = speech(&levels);
    let mut out = Vec::new();
    let mut start = 0;
    while pcm.len() - start > secs(PIECE_MAX_SECS) {
        let lo = (start + secs(PIECE_MIN_SECS)) / fl;
        let hi = ((start + secs(PIECE_MAX_SECS)).min(pcm.len() - secs(REST_MIN_SECS))) / fl;
        let cut_frame = longest_pause(&sp[lo..hi]).map(|r| lo + (r.start + r.end) / 2).unwrap_or_else(|| lo + quietest(&levels[lo..hi]));
        let cut = (cut_frame * fl).clamp(start + 1, pcm.len());
        out.push(start..cut);
        start = cut;
    }
    out.push(start..pcm.len());
    out
}

/// The longest run of non-speech frames; the later one on a tie.
fn longest_pause(sp: &[bool]) -> Option<Range<usize>> {
    let mut best: Option<Range<usize>> = None;
    let mut k = 0;
    while k < sp.len() {
        if sp[k] {
            k += 1;
            continue;
        }
        let s = k;
        while k < sp.len() && !sp[k] {
            k += 1;
        }
        if best.as_ref().is_none_or(|b| k - s >= b.len()) {
            best = Some(s..k);
        }
    }
    best
}

fn quietest(levels: &[f32]) -> usize {
    levels.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| k).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// `parts` of (seconds, loud): a tone for speech, faint noise for quiet.
    fn take(parts: &[(f32, bool)]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut n = 0u32;
        for (secs, loud) in parts {
            for _ in 0..(secs * RATE as f32) as usize {
                n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (n >> 16) as f32 / 65_536.0 - 0.5;
                let t = out.len() as f32 / RATE as f32;
                out.push(if *loud { 0.2 * (t * 220.0 * std::f32::consts::TAU).sin() } else { noise * 0.004 });
            }
        }
        out
    }

    fn secs(n: usize) -> f32 {
        n as f32 / RATE as f32
    }

    #[test]
    fn quiet_ends_are_trimmed_with_a_margin() {
        let pcm = take(&[(1.5, false), (2.0, true), (2.0, false)]);
        let r = trim(&pcm, RATE);
        assert!((secs(r.start) - 1.25).abs() < 0.03, "start {}", secs(r.start));
        assert!((secs(r.end) - 3.85).abs() < 0.03, "end {}", secs(r.end));
    }

    #[test]
    fn a_click_is_not_speech() {
        // 20 ms of loud at the very end, like the shortcut's key
        let pcm = take(&[(1.0, true), (1.0, false), (0.02, true), (0.3, false)]);
        let r = trim(&pcm, RATE);
        assert!(secs(r.end) < 1.4, "end {}", secs(r.end));
    }

    #[test]
    fn all_quiet_or_all_speech_keeps_everything() {
        let quiet = take(&[(1.0, false)]);
        assert_eq!(trim(&quiet, RATE), 0..quiet.len());
        let loud = take(&[(1.0, true)]);
        assert_eq!(trim(&loud, RATE), 0..loud.len());
        assert_eq!(trim(&[], RATE), 0..0);
    }

    #[test]
    fn short_takes_stay_whole() {
        let pcm = take(&[(20.0, true)]);
        assert_eq!(split(&pcm, RATE), vec![0..pcm.len()]);
    }

    #[test]
    fn long_takes_are_cut_in_their_pauses() {
        // speech with a pause at 12 s (too early), 22 s and 27 s (longer)
        let pcm = take(&[(12.0, true), (0.4, false), (9.6, true), (0.3, false), (4.7, true), (0.8, false), (22.2, true)]);
        let parts = split(&pcm, RATE);
        assert_eq!(parts.len(), 2);
        let cut = secs(parts[0].end);
        assert!((cut - 27.4).abs() < 0.05, "cut at {cut}");
        assert_eq!(parts[1].end, pcm.len());
    }

    #[test]
    fn pieces_cover_everything_and_stay_in_bounds() {
        // 100 s without one pause
        let pcm = take(&[(100.0, true)]);
        let parts = split(&pcm, RATE);
        assert_eq!(parts.first().unwrap().start, 0);
        assert_eq!(parts.last().unwrap().end, pcm.len());
        for w in parts.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for p in &parts {
            let s = secs(p.len());
            assert!((REST_MIN_SECS as f32..=PIECE_MAX_SECS as f32 + 0.1).contains(&s), "piece {s}");
        }
    }

    #[test]
    fn the_last_piece_is_never_a_crumb() {
        let pcm = take(&[(31.0, true), (0.5, false), (5.0, true)]);
        let parts = split(&pcm, RATE);
        assert!(secs(parts.last().unwrap().len()) >= REST_MIN_SECS as f32);
    }
}
