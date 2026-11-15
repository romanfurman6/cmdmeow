//! Speech to text with NVIDIA's Parakeet TDT 0.6B v3 (int8, run by
//! sherpa-onnx): about as accurate as Whisper large-v3 for Ukrainian,
//! Russian and English (and 22 more European languages, told apart by
//! itself), and several times faster, so it suits dictation. The model is
//! downloaded once (an archive, resumable) and unpacked into the app's
//! models folder.

use crate::kit::whisper::Progress;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// The archive's name, also the folder it unpacks to.
const NAME: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
const URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2";
/// About how big the download is.
pub const BYTES: u64 = 487_211_443;
/// The files the recognizer needs.
const FILES: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];

pub fn dir() -> PathBuf {
    crate::kit::whisper::models_dir().join(NAME)
}

fn archive() -> PathBuf {
    crate::kit::whisper::models_dir().join(format!("{NAME}.tar.bz2"))
}

pub fn ready() -> bool {
    FILES.iter().all(|f| dir().join(f).is_file())
}

/// Delete the model.
pub fn remove() {
    let _ = std::fs::remove_dir_all(dir());
    let _ = std::fs::remove_file(archive());
    let _ = std::fs::remove_file(part());
}

fn part() -> PathBuf {
    crate::kit::whisper::models_dir().join(format!("{NAME}.tar.bz2.part"))
}

/// Fetch and unpack the model, picking up a download that stopped part
/// way. Blocks. Progress counts download bytes; `unpacking` turns true once
/// the download is in.
pub fn download(p: &Progress, unpacking: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    let models = crate::kit::whisper::models_dir();
    std::fs::create_dir_all(&models).map_err(|e| e.to_string())?;
    if !archive().is_file() {
        fetch(p)?;
    }
    unpacking.store(true, Ordering::Relaxed);
    let tmp = crate::kit::whisper::models_dir().join(format!("{NAME}.unpacking"));
    let r = unpack(p, &tmp);
    unpacking.store(false, Ordering::Relaxed);
    if let Err(e) = &r {
        let _ = std::fs::remove_dir_all(&tmp);
        // a damaged archive is fetched again next time; a stop keeps it
        if e != "Stopped" {
            let _ = std::fs::remove_file(archive());
        }
    }
    r
}

fn fetch(p: &Progress) -> Result<(), String> {
    let part = part();
    let have = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
    let agent = crate::kit::whisper::agent();
    let mut req = agent.get(URL).header("User-Agent", "Cmd+Meow");
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("Couldn't reach the download: {e}"))?;
    let status = resp.status().as_u16();
    if status == 416 && have > 0 {
        // the part is already whole (or the file changed): start over
        let _ = std::fs::remove_file(&part);
        return fetch(p);
    }
    let resumed = status == 206;
    if status != 200 && !resumed {
        return Err(format!("The download answered {status}"));
    }
    let len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let start = if resumed { have } else { 0 };
    p.total.store(len.map(|l| l + start).unwrap_or(BYTES), Ordering::Relaxed);
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
    // a bzip2 stream starts with "BZh"
    let mut magic = [0u8; 3];
    std::fs::File::open(&part).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    if &magic != b"BZh" {
        let _ = std::fs::remove_file(&part);
        return Err("That wasn't the speech model; try again".into());
    }
    std::fs::rename(&part, archive()).map_err(|e| e.to_string())
}

/// Unpack only the files the recognizer needs into [`dir`], then drop the
/// archive.
fn unpack(p: &Progress, tmp: &std::path::Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(tmp);
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let f = std::fs::File::open(archive()).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(f)));
    let mut got = 0;
    for entry in tar.entries().map_err(|e| format!("The model archive is damaged: {e}"))? {
        if p.cancelled() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("Stopped".into());
        }
        let mut entry = entry.map_err(|e| format!("The model archive is damaged: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        if !FILES.contains(&name.as_str()) {
            continue;
        }
        entry.unpack(tmp.join(&name)).map_err(|e| format!("Unpacking the model failed: {e}"))?;
        got += 1;
    }
    if got < FILES.len() {
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_file(archive());
        return Err("The model archive is missing files; try again".into());
    }
    let _ = std::fs::remove_dir_all(dir());
    std::fs::rename(&tmp, dir()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(archive());
    Ok(())
}

/// Shorter input than this (0.1 s at 16 kHz) is never handed to the model.
#[cfg(target_os = "macos")]
const MIN_SAMPLES: usize = 1_600;

/// The model loaded and ready to run.
pub struct Loaded {
    #[cfg(target_os = "macos")]
    rec: sherpa_onnx::OfflineRecognizer,
}

impl Loaded {
    #[cfg(target_os = "macos")]
    pub fn load() -> Result<Loaded, String> {
        use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
        let d = dir();
        let s = |f: &str| Some(d.join(f).to_string_lossy().into_owned());
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig { encoder: s(FILES[0]), decoder: s(FILES[1]), joiner: s(FILES[2]) };
        config.model_config.tokens = s(FILES[3]);
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = crate::kit::work_threads() as i32;
        config.model_config.provider = Some("cpu".into());
        let rec = OfflineRecognizer::create(&config).ok_or("The speech model didn't load")?;
        Ok(Loaded { rec })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn load() -> Result<Loaded, String> {
        Err("Speech to text needs macOS".into())
    }

    /// The text of 16 kHz mono `pcm`. Blocks.
    #[cfg(target_os = "macos")]
    pub fn run(&self, pcm: &[f32]) -> Result<String, String> {
        // under a tenth of a second the encoder gets no frames and aborts
        // the process (an ONNX exception Rust can't catch)
        if pcm.len() < MIN_SAMPLES {
            return Ok(String::new());
        }
        let stream = self.rec.create_stream();
        stream.accept_waveform(16_000, pcm);
        self.rec.decode(&stream);
        Ok(stream.get_result().map(|r| r.text).unwrap_or_default())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn run(&self, _pcm: &[f32]) -> Result<String, String> {
        Err("Speech to text needs macOS".into())
    }
}
