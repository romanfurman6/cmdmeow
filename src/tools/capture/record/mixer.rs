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
/// A buffer that says it starts this close to where the last one ended
/// follows it: device clocks drift and timestamps jitter by a few frames,
/// and a frame of silence (or a dropped one) at every such step is a click.
/// Further off is a real gap or overlap.
pub const SNAP: i64 = (RATE / 50) as i64;
/// Frames of smoothed drift before a buffer is stretched or squeezed by one
/// frame to follow its timestamps again.
const DRIFT_STEP: f64 = 2.0;

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
    /// Frames came before, so the next buffer is expected at `end()`.
    seen: bool,
    /// How far the buffers' own times run ahead of where they land
    /// (frames, smoothed): the device's clock against the recording's.
    drift: f64,
    resampler: Resampler,
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

    /// `pcm`, which starts at timeline frame `at`, as interleaved stereo at
    /// [`RATE`], ready for [`Mixer::push`]. Each source keeps its own
    /// resampler, so buffers join without a seam, and a source whose clock
    /// runs a little off the recording's is stretched or squeezed by a frame
    /// now and then instead of gaining gaps.
    pub fn resample(&mut self, s: Src, at: i64, pcm: &Pcm) -> Vec<f32> {
        let t = self.track(s);
        if !t.on {
            return Vec::new();
        }
        let off = if t.seen { at - t.end() } else { 0 };
        let mut stretch = 0;
        if off.abs() > SNAP {
            // a real gap or overlap: what follows starts afresh
            t.drift = 0.0;
            t.resampler = Resampler::default();
        } else {
            t.drift += 0.2 * (off as f64 - t.drift);
            if t.drift >= DRIFT_STEP && off >= 1 {
                stretch = 1;
            } else if t.drift <= -DRIFT_STEP && off <= -1 {
                stretch = -1;
            }
        }
        t.resampler.process(pcm, stretch)
    }

    /// Add `stereo` (interleaved) frames that start at timeline frame `at`.
    /// A buffer within [`SNAP`] of where the last one ended follows it
    /// directly; a bigger overlap is dropped, a bigger gap becomes silence.
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
        let at = if t.seen && (at - end).abs() <= SNAP { end } else { at };
        t.seen = true;
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

/// One source's way to [`RATE`]: linear resampling that carries on from one
/// buffer to the next (where the next output frame falls, and the last
/// frame to draw the line from), so buffers of any size join without a seam
/// and the frames that come out add up to the rate's ratio exactly.
#[derive(Default)]
pub struct Resampler {
    rate: f64,
    /// Where the next output frame falls, in input frames from the start of
    /// the next buffer: -1…0 when it lies between the last buffer's final
    /// frame and the next one's first.
    phase: f64,
    last: [f32; 2],
}

impl Resampler {
    /// `pcm` as interleaved stereo at [`RATE`] (mono goes to both sides,
    /// extra channels are dropped). `stretch` makes this buffer one frame
    /// longer (1) or shorter (-1) than its rate gives.
    pub fn process(&mut self, pcm: &Pcm, stretch: i64) -> Vec<f32> {
        let n = pcm.frames();
        if n == 0 || pcm.channels == 0 || !(pcm.rate > 0.0) {
            return Vec::new();
        }
        if pcm.rate != self.rate {
            // another device or format: nothing carries over
            self.rate = pcm.rate;
            self.phase = 0.0;
        }
        let frame = |i: usize| [pcm.sample(i, 0), pcm.sample(i, if pcm.channels == 1 { 0 } else { 1 })];
        let mix = |p: [f32; 2], q: [f32; 2], f: f32| [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f];
        let ratio = pcm.rate / RATE as f64;
        let last_i = (n - 1) as f64;
        if (ratio - 1.0).abs() < 1e-9 && self.phase == 0.0 {
            let count = n as i64 + if n >= 8 { stretch } else { 0 };
            let mut out = Vec::with_capacity(count as usize * 2);
            if count == n as i64 {
                // already at the rate: the frames as they are
                for i in 0..n {
                    out.extend(frame(i));
                }
            } else {
                // the same stretch of sound over one frame more or less,
                // ending on the buffer's last frame so the next goes on as it is
                let step = last_i / (count - 1) as f64;
                for k in 0..count {
                    let x = (k as f64 * step).min(last_i);
                    let a = x.floor() as usize;
                    out.extend(mix(frame(a), frame((a + 1).min(n - 1)), (x - a as f64) as f32));
                }
            }
            self.last = frame(n - 1);
            return out;
        }
        let want = (n as f64 / ratio + stretch as f64).max(1.0);
        let step = n as f64 / want;
        let mut out = Vec::with_capacity((want as usize + 2) * 2);
        let mut x = self.phase;
        while x <= last_i {
            let a = x.floor();
            let f = (x - a) as f32;
            let v = if a < 0.0 {
                mix(self.last, frame(0), f)
            } else {
                let i = a as usize;
                mix(frame(i), frame((i + 1).min(n - 1)), f)
            };
            out.extend(v);
            x += step;
        }
        self.phase = x - n as f64;
        self.last = frame(n - 1);
        out
    }
}

/// Any layout and rate → interleaved stereo at [`RATE`], for one buffer on
/// its own.
#[cfg(test)]
pub fn to_stereo(pcm: &Pcm) -> Vec<f32> {
    Resampler::default().process(pcm, 0)
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
        m.push(Src::System, 0, &tone(0.5, 2000));
        m.push(Src::System, 4000, &tone(0.5, 2000));
        m.push(Src::System, 4500, &tone(0.9, 2000)); // 1500 frames overlap
        let (_, out) = m.pull(false).unwrap();
        assert_eq!(out.len() / 2, 6500);
        assert!(out[2 * 2000..2 * 4000].iter().all(|&v| v == 0.0));
        assert_eq!(out[2 * 6000], 0.9);
    }

    #[test]
    fn late_audio_for_handed_out_time_is_dropped() {
        let mut m = Mixer::new(true, false);
        m.push(Src::System, 0, &tone(0.5, 2000));
        m.pull(false).unwrap();
        m.push(Src::System, 500, &tone(0.2, 2000));
        let (at, out) = m.pull(false).unwrap();
        assert_eq!((at, out.len() / 2), (2000, 500));
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
        // two buffers of 441 frames at 44.1 kHz are 960 frames at 48 kHz; the
        // last one lies past the final input frame and comes with the next buffer
        let mono: Vec<f32> = (0..882).map(|i| i as f32).collect();
        let mut r = Resampler::default();
        let mut out = Vec::new();
        for half in mono.chunks(441) {
            out.extend(r.process(&Pcm { rate: 44_100.0, channels: 1, planes: vec![half], interleaved: true }, 0));
        }
        assert_eq!(out.len() / 2, 959);
        assert_eq!(out[0], out[1]);
        // a ramp stays a ramp across the join
        let step = 44_100.0 / 48_000.0;
        for (k, v) in out.iter().step_by(2).enumerate() {
            assert!((v - k as f32 * step).abs() < 1e-2, "frame {k}: {v}");
        }
        let l = [1.0f32, 1.0, 1.0];
        let r = [2.0f32, 2.0, 2.0];
        let planar = Pcm { rate: 48_000.0, channels: 2, planes: vec![&l, &r], interleaved: false };
        assert_eq!(to_stereo(&planar), vec![1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    }

    /// A 440 Hz tone from a microphone at `rate` in buffers of `n` frames,
    /// each stamped with its own time (a clock `ppm` off the recording's),
    /// the way the recorder feeds the mixer. Returns the mixed left channel.
    fn mic_tone(rate: f64, n: usize, ppm: f64, secs: f64) -> Vec<f32> {
        let mut m = Mixer::new(false, true);
        let mut out = Vec::new();
        let total = (rate * secs) as usize;
        let mut i = 0;
        while i + n <= total {
            let buf: Vec<f32> = (i..i + n).map(|k| (std::f64::consts::TAU * 440.0 * k as f64 / rate).sin() as f32 * 0.5).collect();
            let at = (i as f64 / rate * (1.0 + ppm * 1e-6) * RATE as f64).round() as i64;
            let stereo = m.resample(Src::Mic, at, &Pcm { rate, channels: 1, planes: vec![&buf], interleaved: true });
            m.push(Src::Mic, at, &stereo);
            while let Some((_, mix)) = m.pull(false) {
                out.extend(mix.iter().step_by(2));
            }
            i += n;
        }
        out
    }

    /// The biggest bend in the wave (second difference): a clean 440 Hz
    /// tone bends by well under 0.03 a frame; an inserted or dropped frame
    /// by ten times that.
    fn worst_bend(x: &[f32]) -> f32 {
        x.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn a_microphone_at_any_rate_records_without_clicks() {
        for (rate, n) in [(48_000.0, 512), (44_100.0, 512), (44_100.0, 441), (24_000.0, 480), (16_000.0, 320), (96_000.0, 1024)] {
            let out = mic_tone(rate, n, 0.0, 5.0);
            assert!(worst_bend(&out) < 0.03, "{rate} Hz in buffers of {n}: bend {}", worst_bend(&out));
            // and all of it is there: 5 s less the last, partial buffer
            let want = 5.0 * RATE as f64;
            assert!((out.len() as f64 - want).abs() < RATE as f64 * 0.03, "{rate} Hz: {} frames", out.len());
        }
    }

    #[test]
    fn a_drifting_device_clock_is_followed_without_clicks() {
        for ppm in [50.0, -50.0, 300.0, -300.0] {
            let out = mic_tone(48_000.0, 512, ppm, 20.0);
            assert!(worst_bend(&out) < 0.03, "{ppm} ppm: bend {}", worst_bend(&out));
            // the sound keeps up with its timestamps: 20 s at this drift is
            // up to 288 frames, and the mixer stays within a few of them
            let want = 20.0 * RATE as f64 * (1.0 + ppm * 1e-6);
            assert!((out.len() as f64 - want).abs() < 600.0, "{ppm} ppm: {} frames, want {want}", out.len());
        }
    }

    #[test]
    fn a_real_gap_is_still_silence() {
        let mut m = Mixer::new(false, true);
        let buf = vec![0.5f32; 480];
        let pcm = || Pcm { rate: 48_000.0, channels: 1, planes: vec![&buf], interleaved: true };
        for at in [0, 480] {
            let s = m.resample(Src::Mic, at, &pcm());
            m.push(Src::Mic, at, &s);
        }
        // a quarter of a second went missing
        let at = 960 + RATE as i64 / 4;
        let s = m.resample(Src::Mic, at, &pcm());
        m.push(Src::Mic, at, &s);
        let (_, out) = m.pull(true).unwrap();
        assert_eq!(out.len() / 2, 960 + RATE as usize / 4 + 480);
        assert!(out[2 * 960..2 * (960 + RATE as usize / 4)].iter().all(|&v| v == 0.0));
        assert_eq!(out[2 * (960 + RATE as usize / 4)], 0.5);
        // a few frames early or late is jitter, not a gap
        let mut m = Mixer::new(false, true);
        for at in [0, 483, 958] {
            let s = m.resample(Src::Mic, at, &pcm());
            m.push(Src::Mic, at, &s);
        }
        let (_, out) = m.pull(true).unwrap();
        assert!(out.iter().all(|&v| v == 0.5), "no silence slipped in");
    }
}
