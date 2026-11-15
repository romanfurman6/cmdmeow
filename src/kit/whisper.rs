//! Speech to text on this Mac with whisper.cpp: the models (downloaded on
//! request, resumable) and running one on 16 kHz mono audio. Shared by
//! Capture (transcripts of recordings) and Dictation. Nothing leaves the Mac
//! except the one-time model download.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Model {
    /// Whisper large-v3-turbo, quantized: close to the best, still quick on
    /// Apple silicon.
    #[default]
    Accurate,
    /// Whisper small: a third of the size, rougher.
    Fast,
}

impl Model {
    pub const ALL: [Model; 2] = [Model::Accurate, Model::Fast];

    pub fn file(self) -> &'static str {
        match self {
            Model::Accurate => "ggml-large-v3-turbo-q5_0.bin",
            Model::Fast => "ggml-small-q5_1.bin",
        }
    }

    pub fn url(self) -> String {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", self.file())
    }

    /// About how big the download is.
    pub fn bytes(self) -> u64 {
        match self {
            Model::Accurate => 574_041_195,
            Model::Fast => 190_085_487,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::Accurate => "Accurate",
            Model::Fast => "Fast",
        }
    }

    /// The name setup shows, the same one Dictation uses for Accurate.
    pub fn setup_name(self) -> &'static str {
        match self {
            Model::Accurate => "Whisper turbo",
            Model::Fast => "Whisper small",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Model::Accurate => "Whisper large-v3 turbo · best for most voices and languages",
            Model::Fast => "Whisper small · quicker, rougher",
        }
    }

    pub fn path(self) -> PathBuf {
        models_dir().join(self.file())
    }

    pub fn ready(self) -> bool {
        self.path().is_file()
    }

    /// Delete the downloaded model.
    pub fn remove(self) {
        let _ = std::fs::remove_file(self.path());
        let _ = std::fs::remove_file(part_path(self));
    }
}

pub fn models_dir() -> PathBuf {
    crate::kit::fs::data_path("models")
}

fn part_path(m: Model) -> PathBuf {
    models_dir().join(format!("{}.part", m.file()))
}

/// Shared between a job's thread and the window: how far, and a way to stop.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 {
            0.0
        } else {
            (self.done.load(Ordering::Relaxed) as f64 / t as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The HTTP client for model downloads: gives up on a server that stops
/// answering, not on a long download.
pub fn agent() -> ureq::Agent {
    use std::time::Duration;
    ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(20))).timeout_recv_response(Some(Duration::from_secs(30))).http_status_as_error(false).build())
}

/// Fetch `m`, picking up a download that stopped part way. Blocks. One
/// download at a time, so Capture and Dictation never share a `.part`.
pub fn download(m: Model, p: &Progress) -> Result<(), String> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if m.ready() {
        return Ok(());
    }
    fetch(m, p)
}

fn fetch(m: Model, p: &Progress) -> Result<(), String> {
    std::fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;
    let part = part_path(m);
    let have = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
    let agent = crate::kit::whisper::agent();
    let mut req = agent.get(&m.url()).header("User-Agent", "Cmd+Meow");
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("Couldn't reach the download: {e}"))?;
    let status = resp.status().as_u16();
    if status == 416 && have > 0 {
        // the part is already whole (or the file changed): start over
        let _ = std::fs::remove_file(&part);
        return fetch(m, p);
    }
    let resumed = status == 206;
    if status != 200 && !resumed {
        return Err(format!("The download answered {status}"));
    }
    let len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let start = if resumed { have } else { 0 };
    let total = len.map(|l| l + start).unwrap_or(m.bytes());
    p.total.store(total, Ordering::Relaxed);
    p.done.store(start, Ordering::Relaxed);
    let mut file = std::fs::OpenOptions::new().create(true).append(resumed).write(true).truncate(!resumed).open(&part).map_err(|e| e.to_string())?;
    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = start;
    loop {
        if p.cancelled() {
            return Err("Stopped".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("The download broke off: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        p.done.store(done, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if len.is_some_and(|l| done < l + start) {
        return Err("The download ended early; try again to pick it up".into());
    }
    // a whisper.cpp model starts with "lmgg"
    let mut magic = [0u8; 4];
    std::fs::File::open(&part).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    if &magic != b"lmgg" {
        let _ = std::fs::remove_file(&part);
        return Err("That wasn't a speech model; try again".into());
    }
    std::fs::rename(&part, m.path()).map_err(|e| e.to_string())
}

/// A model loaded and ready to run: loading takes a moment and a lot of
/// memory, so keep one while it's in use and drop it when idle.
pub struct Loaded {
    #[cfg(target_os = "macos")]
    ctx: whisper_rs::WhisperContext,
}

impl Loaded {
    #[cfg(target_os = "macos")]
    pub fn load(m: Model) -> Result<Loaded, String> {
        use whisper_rs::{WhisperContext, WhisperContextParameters};
        let ctx = WhisperContext::new_with_params(m.path(), WhisperContextParameters::default()).map_err(|e| format!("The speech model didn't load: {e}"))?;
        Ok(Loaded { ctx })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn load(_m: Model) -> Result<Loaded, String> {
        Err("Speech to text needs macOS".into())
    }

    /// Whisper's raw segments (start s, end s, text) for 16 kHz mono `pcm`.
    /// `lang` is a code like "uk", None to detect it. With `p`, progress is
    /// in percent and its cancel flag stops the run. Blocks.
    pub fn run(&self, pcm: &[f32], lang: Option<&str>, p: Option<&std::sync::Arc<Progress>>) -> Result<Vec<(f64, f64, String)>, String> {
        Ok(self.run_timed(pcm, lang, p)?.into_iter().map(|s| (s.start, s.end, s.text)).collect())
    }

    /// [`Loaded::run`] with each segment's words and their times.
    #[cfg(target_os = "macos")]
    pub fn run_timed(&self, pcm: &[f32], lang: Option<&str>, p: Option<&std::sync::Arc<Progress>>) -> Result<Vec<Segment>, String> {
        use whisper_rs::{FullParams, SamplingStrategy};
        let mut state = self.ctx.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(lang.unwrap_or("auto")));
        params.set_n_threads(crate::kit::work_threads() as i32);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        params.set_token_timestamps(true);
        if let Some(p) = p {
            let pa = p.clone();
            params.set_progress_callback_safe(move |pct: i32| pa.done.store(pct.clamp(0, 100) as u64, Ordering::Relaxed));
            // whisper-rs 0.16's safe abort callback reads its closure wrongly (and
            // aborts every encode), so the raw one, on the cancel flag itself
            unsafe extern "C" fn stop(flag: *mut std::ffi::c_void) -> bool {
                unsafe { (*(flag as *const AtomicBool)).load(Ordering::Relaxed) }
            }
            // `p` outlives `full` below, so the flag stays where it is
            unsafe {
                params.set_abort_callback(Some(stop));
                params.set_abort_callback_user_data(&p.cancel as *const AtomicBool as *mut std::ffi::c_void);
            }
        }
        let cancelled = || p.is_some_and(|p| p.cancelled());
        state.full(params, pcm).map_err(|e| if cancelled() { "Stopped".to_string() } else { format!("Transcribing failed: {e}") })?;
        Ok((0..state.full_n_segments())
            .filter_map(|i| state.get_segment(i))
            .map(|s| {
                let tokens: Vec<(f64, f64, Vec<u8>)> = (0..s.n_tokens())
                    .filter_map(|j| s.get_token(j))
                    .map(|t| {
                        let d = t.token_data();
                        (d.t0 as f64 / 100.0, d.t1 as f64 / 100.0, t.to_bytes().map(|b| b.to_vec()).unwrap_or_default())
                    })
                    .collect();
                Segment {
                    start: s.start_timestamp() as f64 / 100.0,
                    end: s.end_timestamp() as f64 / 100.0,
                    text: s.to_str_lossy().map(|t| t.to_string()).unwrap_or_default(),
                    words: group_words(&tokens),
                }
            })
            .collect())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn run_timed(&self, _pcm: &[f32], _lang: Option<&str>, _p: Option<&std::sync::Arc<Progress>>) -> Result<Vec<Segment>, String> {
        Err("Speech to text needs macOS".into())
    }
}

/// One stretch of speech whisper found, with its words (start s, end s, text).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<(f64, f64, String)>,
}

/// Whisper's tokens (start s, end s, raw bytes) → words. A token starting
/// with a space starts a word; the others continue it. Bytes are joined
/// before decoding, since tokens can split a letter (Cyrillic does).
/// Special tokens ("[_BEG_]", "<|endoftext|>") are skipped.
pub fn group_words(tokens: &[(f64, f64, Vec<u8>)]) -> Vec<(f64, f64, String)> {
    let mut words: Vec<(f64, f64, Vec<u8>)> = Vec::new();
    for (t0, t1, b) in tokens {
        if b.starts_with(b"[_") || b.starts_with(b"<|") || b.is_empty() {
            continue;
        }
        match words.last_mut() {
            Some(w) if !b.starts_with(b" ") => {
                w.1 = w.1.max(*t1);
                w.2.extend_from_slice(b);
            }
            _ => words.push((*t0, *t1, b.clone())),
        }
    }
    words
        .into_iter()
        .map(|(a, b, t)| (a, b, String::from_utf8_lossy(&t).trim().to_string()))
        .filter(|w| !w.2.is_empty())
        .collect()
}

#[cfg(test)]
mod word_tests {
    use super::group_words;

    #[test]
    fn tokens_join_into_words_even_mid_letter() {
        let pryvit = "Привіт".as_bytes();
        let toks = vec![
            (0.0, 0.0, b"[_BEG_]".to_vec()),
            (0.0, 0.3, [b" ", &pryvit[..3]].concat()),
            (0.3, 0.6, pryvit[3..].to_vec()),
            (0.7, 0.9, b" all".to_vec()),
            (0.9, 1.0, b"!".to_vec()),
            (1.0, 1.0, b"<|endoftext|>".to_vec()),
        ];
        assert_eq!(group_words(&toks), vec![(0.0, 0.6, "Привіт".to_string()), (0.7, 1.0, "all!".to_string())]);
    }
}
