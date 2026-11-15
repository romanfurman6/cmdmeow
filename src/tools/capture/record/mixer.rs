//! Mixes system audio and the microphone into one stereo track, so every
//! player (and every chat app) hears both. Each source pushes stereo frames
//! at the frame where they belong on the recording's timeline; the mixer
//! hands out the stretch every live source has reached, adding them up.
//! A source that falls far behind (its device stalls, nothing plays) counts
//! as silence, so the other one never waits on it for long.
// only the macOS recorder uses all of this outside tests
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::VecDeque;

/// Output sample rate.
pub const RATE: u32 = 48_000;
/// How far a source may lag before its missing part counts as silence.
pub const MAX_LAG: i64 = (RATE / 4) as i64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    System,
    Mic,
}

/// One source's frames not yet handed out. `start` is the timeline frame of
/// the first one; samples are interleaved stereo.
#[derive(Default)]
struct Track {
    on: bool,
    start: i64,
    buf: VecDeque<f32>,
}

impl Track {
    fn end(&self) -> i64 {
        self.start + (self.buf.len() / 2) as i64
    }
}

#[derive(Default)]
pub struct Mixer {
    sys: Track,
    mic: Track,
    /// The next frame to hand out.
    pos: i64,
    pub mic_gain: f32,
}

impl Mixer {
    pub fn new(system: bool, mic: bool) -> Mixer {
        let mut m = Mixer { mic_gain: 1.0, ..Default::default() };
        m.sys.on = system;
        m.mic.on = mic;
        m
    }

    /// The next frame to hand out.
    pub fn position(&self) -> i64 {
        self.pos
    }

    fn track(&mut self, s: Src) -> &mut Track {
        match s {
            Src::System => &mut self.sys,
            Src::Mic => &mut self.mic,
        }
    }

    /// Add `stereo` (interleaved) frames that start at timeline frame `at`.
    /// Overlap with what came before is dropped, a gap becomes silence.
    pub fn push(&mut self, s: Src, at: i64, stereo: &[f32]) {
        let pos = self.pos;
        let gain = if s == Src::Mic { self.mic_gain } else { 1.0 };
        let t = self.track(s);
        if !t.on {
            return;
        }
        if t.buf.is_empty() && t.start < pos {
            t.start = pos;
        }
        let end = t.end();
        let mut skip = 0usize;
        if t.buf.is_empty() && at > t.start {
            t.start = at;
        } else if at > end {
            t.buf.extend(std::iter::repeat_n(0.0, ((at - end) * 2) as usize));
        } else if at < end {
            skip = ((end - at) * 2) as usize;
        }
        // nothing before what was already handed out
        let first = at + (skip / 2) as i64;
        if first < pos {
            skip += ((pos - first) * 2) as usize;
            if t.buf.is_empty() {
                t.start = pos;
            }
        }
        if skip < stereo.len() {
            t.buf.extend(stereo[skip..].iter().map(|v| v * gain));
        }
    }

    #[cfg(test)]
    /// The next mixed stretch: (first frame, interleaved stereo), or None
    /// when there's nothing new. `flush` hands out everything left.
    pub fn pull(&mut self, flush: bool) -> Option<(i64, Vec<f32>)> {
        self.pull_split(flush).map(|(at, mix, _)| (at, mix))
    }

    /// Like [`Mixer::pull`], plus each source on its own (system, mic),
    /// over the same stretch; a source that's off is all silence.
    pub fn pull_split(&mut self, flush: bool) -> Option<(i64, Vec<f32>, [Vec<f32>; 2])> {
        let live: Vec<&Track> = [&self.sys, &self.mic].into_iter().filter(|t| t.on).collect();
        if live.is_empty() {
            return None;
        }
        let ends: Vec<i64> = live.iter().filter(|t| !t.buf.is_empty()).map(|t| t.end()).collect();
        let furthest = ends.iter().copied().max()?;
        let until = if flush {
            furthest
        } else {
            // every live source has to get there, unless it lags too far
            let slowest = live.iter().map(|t| if t.buf.is_empty() { t.start.min(self.pos) } else { t.end() }).min().unwrap_or(furthest);
            slowest.max(furthest - MAX_LAG)
        };
        if until <= self.pos {
            return None;
        }
        let n = (until - self.pos) as usize;
        let mut parts = [vec![0.0f32; n * 2], vec![0.0f32; n * 2]];
        for (t, out) in [&mut self.sys, &mut self.mic].into_iter().zip(parts.iter_mut()) {
            if !t.on {
                continue;
            }
            // frames of this track before `pos` are stale
            while t.start < self.pos && !t.buf.is_empty() {
                t.buf.pop_front();
                t.buf.pop_front();
                t.start += 1;
            }
            let offset = (t.start - self.pos).max(0) as usize;
            let mut i = offset * 2;
            while i < out.len() {
                let (Some(l), Some(r)) = (t.buf.pop_front(), t.buf.pop_front()) else { break };
                out[i] = l;
                out[i + 1] = r;
                i += 2;
                t.start += 1;
            }
            if t.buf.is_empty() && t.start < until {
                t.start = until;
            }
        }
        let mix: Vec<f32> = parts[0].iter().zip(&parts[1]).map(|(a, b)| (a + b).clamp(-1.0, 1.0)).collect();
        for part in &mut parts {
            for v in part.iter_mut() {
                *v = v.clamp(-1.0, 1.0);
            }
        }
        let at = self.pos;
        self.pos = until;
        Some((at, mix, parts))
    }
}

/// Audio as it comes from a device: channel count, rate, and samples as
/// f32 (interleaved when `interleaved`, else one plane per channel).
pub struct Pcm<'a> {
    pub rate: f64,
    pub channels: usize,
    pub planes: Vec<&'a [f32]>,
    pub interleaved: bool,
}

impl Pcm<'_> {
    fn frames(&self) -> usize {
        match (self.interleaved, self.planes.first()) {
            (true, Some(p)) => p.len() / self.channels.max(1),
            (false, Some(p)) => p.len(),
            _ => 0,
        }
    }

    fn sample(&self, frame: usize, ch: usize) -> f32 {
        let ch = ch.min(self.channels.saturating_sub(1));
        if self.interleaved {
            self.planes[0].get(frame * self.channels + ch).copied().unwrap_or(0.0)
        } else {
            self.planes.get(ch).and_then(|p| p.get(frame)).copied().unwrap_or(0.0)
        }
    }
}

/// Any layout and rate → interleaved stereo at [`RATE`] (linear resampling;
/// mono goes to both sides, extra channels are dropped).
pub fn to_stereo(pcm: &Pcm) -> Vec<f32> {
    let n = pcm.frames();
    if n == 0 || pcm.channels == 0 {
        return Vec::new();
    }
    let ratio = pcm.rate / RATE as f64;
    let out_n = if (ratio - 1.0).abs() < 1e-9 { n } else { ((n as f64) / ratio).floor().max(1.0) as usize };
    let mut out = Vec::with_capacity(out_n * 2);
    for i in 0..out_n {
        let x = i as f64 * ratio;
        let a = (x.floor() as usize).min(n - 1);
        let b = (a + 1).min(n - 1);
        let f = (x - a as f64) as f32;
        for ch in 0..2 {
            let ch = if pcm.channels == 1 { 0 } else { ch };
            let v = pcm.sample(a, ch) * (1.0 - f) + pcm.sample(b, ch) * f;
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(v: f32, frames: usize) -> Vec<f32> {
        vec![v; frames * 2]
    }

    #[test]
    fn one_source_passes_straight_through() {
        let mut m = Mixer::new(true, false);
        m.push(Src::System, 0, &tone(0.25, 100));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len()), (0, 200));
        assert!(out.iter().all(|&v| v == 0.25));
        assert!(m.pull(false).is_none());
    }

    #[test]
    fn two_sources_add_up_where_both_have_reached() {
        let mut m = Mixer::new(true, true);
        m.push(Src::System, 0, &tone(0.25, 100));
        m.push(Src::Mic, 0, &tone(0.5, 60));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len() / 2), (0, 60));
        assert!(out.iter().all(|&v| v == 0.75));
        m.push(Src::Mic, 60, &tone(0.5, 40));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len() / 2), (60, 40));
        assert!(out.iter().all(|&v| v == 0.75));
    }

    #[test]
    fn each_source_also_comes_out_on_its_own() {
        let mut m = Mixer::new(true, true);
        m.push(Src::System, 0, &tone(0.25, 50));
        m.push(Src::Mic, 10, &tone(0.5, 40));
        let (at, mix, [sys, mic]) = m.pull_split(false).unwrap();
        assert_eq!((at, mix.len(), sys.len(), mic.len()), (0, 100, 100, 100));
        assert_eq!((sys[0], mic[0], mix[0]), (0.25, 0.0, 0.25));
        assert_eq!((sys[40], mic[40], mix[40]), (0.25, 0.5, 0.75));
    }

    #[test]
    fn a_silent_source_holds_things_up_only_briefly() {
        let mut m = Mixer::new(true, true);
        m.push(Src::System, 0, &tone(0.1, 100));
        assert!(m.pull(false).is_none(), "the mic may still come");
        m.push(Src::System, 100, &tone(0.1, MAX_LAG as usize));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len() / 2), (0, 100));
        assert!(out.iter().all(|&v| (v - 0.1).abs() < 1e-6), "missing mic = silence");
    }

    #[test]
    fn gaps_become_silence_and_overlaps_are_dropped() {
        let mut m = Mixer::new(true, false);
        m.push(Src::System, 0, &tone(0.5, 10));
        m.push(Src::System, 20, &tone(0.5, 10));
        m.push(Src::System, 25, &tone(0.9, 10)); // 5 frames overlap
        let (_, out) = m.pull(false).unwrap();
        assert_eq!(out.len() / 2, 35);
        assert!(out[20..40].iter().all(|&v| v == 0.0));
        assert_eq!(out[2 * 30], 0.9);
    }

    #[test]
    fn late_audio_for_handed_out_time_is_dropped() {
        let mut m = Mixer::new(true, false);
        m.push(Src::System, 0, &tone(0.5, 10));
        m.pull(false).unwrap();
        m.push(Src::System, 5, &tone(0.2, 10));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len() / 2), (10, 5));
    }

    #[test]
    fn muted_mic_adds_nothing() {
        let mut m = Mixer::new(true, true);
        m.mic_gain = 0.0;
        m.push(Src::System, 0, &tone(0.2, 10));
        m.push(Src::Mic, 0, &tone(0.7, 10));
        let (_, out) = m.pull(false).unwrap();
        assert!(out.iter().all(|&v| (v - 0.2).abs() < 1e-6));
    }

    #[test]
    fn loud_sums_are_clipped() {
        let mut m = Mixer::new(true, true);
        m.push(Src::System, 0, &tone(0.8, 4));
        m.push(Src::Mic, 0, &tone(0.8, 4));
        let (_, out) = m.pull(true).unwrap();
        assert!(out.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn mono_and_other_rates_become_stereo_48k() {
        let mono: Vec<f32> = (0..441).map(|i| i as f32).collect();
        let pcm = Pcm { rate: 44_100.0, channels: 1, planes: vec![&mono], interleaved: true };
        let out = to_stereo(&pcm);
        assert_eq!(out.len() / 2, 480);
        assert_eq!(out[0], out[1]);
        let l = [1.0f32, 1.0, 1.0];
        let r = [2.0f32, 2.0, 2.0];
        let planar = Pcm { rate: 48_000.0, channels: 2, planes: vec![&l, &r], interleaved: false };
        assert_eq!(to_stereo(&planar), vec![1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    }
}
