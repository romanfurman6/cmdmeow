//! Speech to text for recordings: a video's voice as timed lines, and those
//! lines as captions (.srt) or plain text. The models and whisper.cpp itself
//! live in `kit::whisper` (Dictation uses them too).

use super::edit::Edl;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

pub use crate::kit::whisper::{download, Model, Progress};

/// One timed line of speech, source seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// The line's words with their times (older transcripts have none).
    #[serde(default)]
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A transcript kept for a file (in the app's data, not beside the video).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Saved {
    pub path: PathBuf,
    pub len: u64,
    pub modified: u64,
    pub lines: Vec<Line>,
}

fn stamp(path: &Path) -> (u64, u64) {
    let m = std::fs::metadata(path).ok();
    let len = m.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    (len, modified)
}

fn cache_path(video: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    video.hash(&mut h);
    crate::kit::fs::data_path("transcripts").join(format!("{:016x}.json", h.finish()))
}

/// The transcript made for this file as it is now, if any.
pub fn load(video: &Path) -> Option<Vec<Line>> {
    let s: Saved = serde_json::from_slice(&std::fs::read(cache_path(video)).ok()?).ok()?;
    ((s.len, s.modified) == stamp(video) && s.path == video).then_some(s.lines)
}

pub fn store(video: &Path, lines: &[Line]) {
    let (len, modified) = stamp(video);
    let p = cache_path(video);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::kit::fs::save_json(&p, &Saved { path: video.to_path_buf(), len, modified, lines: lines.to_vec() }, false);
}

/// The lines as they fall in the edited video: times moved up past the
/// cuts, lines (almost) all cut away dropped.
pub fn edited_lines(lines: &[Line], edl: &Edl) -> Vec<Line> {
    lines
        .iter()
        .filter_map(|l| {
            let (s, e) = (edl.edited_at(l.start), edl.edited_at(l.end));
            (e - s >= 0.2).then(|| Line { start: s, end: e, text: l.text.clone(), words: Vec::new() })
        })
        .collect()
}

/// Is most of the line cut away?
pub fn line_cut(l: &Line, edl: &Edl) -> bool {
    edl.edited_at(l.end) - edl.edited_at(l.start) < 0.2
}

/// The line being spoken at source `t`.
pub fn line_at(lines: &[Line], t: f64) -> Option<usize> {
    lines.iter().position(|l| t >= l.start && t < l.end)
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn srt(lines: &[Line]) -> String {
    let mut out = String::new();
    for (k, l) in lines.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", k + 1, srt_time(l.start), srt_time(l.end), l.text.trim()));
    }
    out
}

pub fn txt(lines: &[Line]) -> String {
    lines.iter().map(|l| l.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Whisper's raw segments → clean lines (no blanks, no "[music]" asides).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tidy(raw: Vec<crate::kit::whisper::Segment>) -> Vec<Line> {
    raw.into_iter()
        .filter_map(|s| {
            let t = s.text.trim().to_string();
            let aside = (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')'));
            let words = s.words.into_iter().filter(|w| w.1 >= w.0).map(|(start, end, text)| Word { start, end, text }).collect();
            (!t.is_empty() && !aside && s.end > s.start).then_some(Line { start: s.start, end: s.end, text: t, words })
        })
        .collect()
}

/// What Whisper writes over silence, music or noise (it learned them from
/// the last lines of subtitles), lower case, letters and spaces only.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MADE_UP: [&str; 16] = [
    "thank you",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "bye",
    "you",
    "subtitles by the amaraorg community",
    "продолжение следует",
    "спасибо за просмотр",
    "субтитры сделал dimatorzok",
    "субтитры создавал dimatorzok",
    "редактор субтитров асинецкая корректор аегорова",
    "дякую за перегляд",
    "дякуємо за перегляд",
    "субтитри зроблені спільнотою amaraorg",
];

/// Lines Whisper made up: one of [`MADE_UP`] said over more than 2.5 s (a
/// real "Thank you." takes about one), or anything over near silence.
/// `pcm` is the 16 kHz mono sound the lines came from.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn made_up(l: &Line, pcm: &[f32]) -> bool {
    let plain: String = l.text.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().to_lowercase();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let (a, b) = match (l.words.first(), l.words.last()) {
        (Some(f), Some(w)) => (f.start, w.end),
        _ => (l.start, l.end),
    };
    if MADE_UP.contains(&plain.as_str()) && b - a > 2.5 {
        return true;
    }
    let at = |t: f64| ((t.max(0.0) * 16_000.0) as usize).min(pcm.len());
    let part = &pcm[at(l.start)..at(l.end)];
    // about -50 dBFS: nothing anyone said
    !part.is_empty() && (part.iter().map(|v| v * v).sum::<f32>() / part.len() as f32).sqrt() < 0.003
}

/// Transcribe the voice in `video` with model `m`. Blocks; progress is in
/// percent (`total` 100).
#[cfg(target_os = "macos")]
pub fn transcribe(video: &Path, m: Model, p: &std::sync::Arc<Progress>) -> Result<Vec<Line>, String> {
    p.total.store(100, Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    // the mix: everyone in a call, not only this Mac's microphone
    let pcm = super::sound::read_audio(video, super::sound::Track::Mix, 16_000, 1)?;
    if pcm.is_empty() {
        return Err("This video has no sound".into());
    }
    if p.cancelled() {
        return Err("Stopped".into());
    }
    let raw = crate::kit::whisper::Loaded::load(m)?.run_timed(&pcm, None, Some(p))?;
    p.done.store(100, Ordering::Relaxed);
    let mut lines = tidy(raw);
    lines.retain(|l| !made_up(l, &pcm));
    Ok(lines)
}

#[cfg(not(target_os = "macos"))]
pub fn transcribe(_video: &Path, _m: Model, _p: &std::sync::Arc<Progress>) -> Result<Vec<Line>, String> {
    Err("Transcribing needs macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: f64, e: f64, t: &str) -> Line {
        Line { start: s, end: e, text: t.into(), words: Vec::new() }
    }

    #[test]
    fn lines_whisper_made_up_are_dropped() {
        let loud: Vec<f32> = (0..16_000 * 12).map(|i| ((i as f32) * 0.05).sin() * 0.2).collect();
        // a "Thank you." over ten seconds of beeps, and a real one
        assert!(made_up(&line(0.0, 10.1, "Thank you."), &loud));
        assert!(!made_up(&line(9.0, 10.0, "Thank you."), &loud));
        assert!(made_up(&line(0.0, 6.0, "Продолжение следует..."), &loud));
        assert!(!made_up(&line(0.0, 6.0, "Thank you all for coming today"), &loud));
        // anything over near silence
        let mut quiet = loud.clone();
        quiet[16_000 * 2..16_000 * 5].iter_mut().for_each(|v| *v *= 0.001);
        assert!(made_up(&line(2.0, 5.0, "Hello there"), &quiet));
        assert!(!made_up(&line(5.0, 8.0, "Hello there"), &quiet));
    }

    #[test]
    fn captions_follow_the_cuts() {
        let mut edl = Edl::new(10.0);
        edl.cut(2.0, 5.0);
        let lines = vec![line(0.5, 1.5, "hello"), line(2.5, 4.5, "gone"), line(5.0, 6.0, "after")];
        let e = edited_lines(&lines, &edl);
        assert_eq!(e, vec![line(0.5, 1.5, "hello"), line(2.0, 3.0, "after")]);
        assert!(line_cut(&lines[1], &edl));
        assert!(!line_cut(&lines[2], &edl));
    }

    #[test]
    fn srt_and_text_read_right() {
        let lines = vec![line(1.0, 2.5, " Hi there "), line(61.25, 3725.0, "Bye")];
        assert_eq!(srt(&lines), "1\n00:00:01,000 --> 00:00:02,500\nHi there\n\n2\n00:01:01,250 --> 01:02:05,000\nBye\n\n");
        assert_eq!(txt(&lines), "Hi there\nBye");
    }

    #[test]
    fn asides_and_blanks_are_dropped() {
        let seg = |a: f64, b: f64, t: &str| crate::kit::whisper::Segment { start: a, end: b, text: t.into(), words: Vec::new() };
        let raw = vec![seg(0.0, 1.0, " [Music] "), seg(1.0, 2.0, " Okay "), seg(2.0, 2.0, "x"), seg(3.0, 4.0, "  ")];
        assert_eq!(tidy(raw), vec![line(1.0, 2.0, "Okay")]);
    }

    #[test]
    fn the_spoken_line_is_found() {
        let lines = vec![line(0.0, 1.0, "a"), line(2.0, 3.0, "b")];
        assert_eq!(line_at(&lines, 2.5), Some(1));
        assert_eq!(line_at(&lines, 1.5), None);
    }

    #[test]
    fn a_transcript_is_only_used_for_the_file_it_was_made_for() {
        let dir = std::env::temp_dir().join(format!("cmb-speech-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let v = dir.join("a.mp4");
        std::fs::write(&v, b"one").unwrap();
        store(&v, &[line(0.0, 1.0, "x")]);
        assert_eq!(load(&v).map(|l| l.len()), Some(1));
        std::fs::write(&v, b"changed!").unwrap();
        assert!(load(&v).is_none());
        let _ = std::fs::remove_file(cache_path(&v));
        let _ = std::fs::remove_dir_all(dir);
    }
}
