//! The voice: the microphone while the shortcut is held (macOS), and the
//! sums done on it — mono, 16 kHz for the model, a level for the pill, and
//! whether anything was said at all.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// The rate speech models take.
pub const MODEL_RATE: u32 = 16_000;

/// Interleaved or planar frames of `channels` → one channel (the average).
pub fn to_mono(samples: &[f32], channels: usize, planar: bool) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    (0..frames)
        .map(|f| {
            let sum: f32 = (0..channels).map(|c| if planar { samples[c * frames + f] } else { samples[f * channels + c] }).sum();
            sum / channels as f32
        })
        .collect()
}

/// `input` at `from` Hz → `to` Hz. Going down, each output sample is the
/// mean of the input it covers (a box filter keeps the worst aliasing out);
/// going up, straight lines between samples.
pub fn resample(input: &[f32], from: f64, to: u32) -> Vec<f32> {
    if input.is_empty() || from <= 0.0 {
        return Vec::new();
    }
    let ratio = from / to as f64;
    if (ratio - 1.0).abs() < 1e-6 {
        return input.to_vec();
    }
    let n = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 * ratio;
        if ratio > 1.0 {
            let a = t as usize;
            let b = (((i + 1) as f64 * ratio) as usize).clamp(a + 1, input.len());
            let s: f32 = input[a..b].iter().sum();
            out.push(s / (b - a) as f32);
        } else {
            let a = t as usize;
            let b = (a + 1).min(input.len() - 1);
            let f = (t - a as f64) as f32;
            out.push(input[a] * (1.0 - f) + input[b] * f);
        }
    }
    out
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// RMS → 0..1 for the pill's bars: -55 dBFS and below is flat, -12 full.
pub fn level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 55.0) / 43.0).clamp(0.0, 1.0)
}

/// The [`level`] of the quietest speech: `silent`'s threshold.
pub const VOICE_LEVEL: f32 = 0.25;

/// Nothing above a whisper in any tenth of a second: don't bother the
/// model (on silence it makes words up).
pub fn silent(pcm: &[f32], rate: u32) -> bool {
    let win = (rate / 10).max(1) as usize;
    !pcm.chunks(win).any(|c| rms(c) > 0.006)
}

/// The model's segments as one line to paste: trimmed, joined with spaces,
/// asides like "[music]" or "(laughs)" dropped.
pub fn tidy_text(segments: &[String]) -> String {
    let mut out = String::new();
    for s in segments {
        let t = s.trim();
        let aside = (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')')) || (t.starts_with('*') && t.ends_with('*'));
        if t.is_empty() || aside {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(target_os = "macos")]
pub use imp::Mic;

#[cfg(not(target_os = "macos"))]
pub struct Mic;

#[cfg(not(target_os = "macos"))]
impl Mic {
    pub fn start() -> Result<Mic, String> {
        Err("Dictation needs macOS".into())
    }
    pub fn level(&self) -> f32 {
        0.0
    }
    pub fn seconds(&self) -> f32 {
        0.0
    }
    pub fn failed(&self) -> Option<String> {
        None
    }
    pub fn stop(self) -> Vec<f32> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{level, resample, rms, to_mono, MODEL_RATE};
    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass};
    use objc2_av_foundation::{AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession, AVMediaTypeAudio};
    use objc2_core_media::CMSampleBuffer;
    use objc2_foundation::{NSObject, NSObjectProtocol};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Asbd {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
    const FLAG_FLOAT: u32 = 1;
    const FLAG_SIGNED: u32 = 1 << 2;
    const FLAG_PLANAR: u32 = 1 << 5;

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetFormatDescription(sb: *const c_void) -> *const c_void;
        fn CMAudioFormatDescriptionGetStreamBasicDescription(desc: *const c_void) -> *const Asbd;
        fn CMSampleBufferGetDataBuffer(sb: *const c_void) -> *const c_void;
        fn CMBlockBufferGetDataLength(b: *const c_void) -> usize;
        fn CMBlockBufferCopyDataBytes(b: *const c_void, offset: usize, len: usize, dest: *mut c_void) -> i32;
    }

    /// What the capture queue hands over, at the 16 kHz the model takes.
    #[derive(Default)]
    struct Buf {
        pcm: Vec<f32>,
        level: f32,
        failed: Option<String>,
        /// Kept from one buffer to the next so the rate never drifts.
        carry: Vec<f32>,
    }

    impl Buf {
        fn take(&mut self, raw: *const c_void) {
            match read(raw) {
                Ok((mono, rate)) => {
                    self.level = level(rms(&mono));
                    // resample in whole runs of the ratio; the rest waits
                    self.carry.extend_from_slice(&mono);
                    let ratio = rate / MODEL_RATE as f64;
                    let whole = if ratio >= 1.0 { ((self.carry.len() as f64 / ratio).floor() * ratio).floor() as usize } else { self.carry.len() };
                    if whole > 0 {
                        let chunk: Vec<f32> = self.carry.drain(..whole).collect();
                        self.pcm.extend(resample(&chunk, rate, MODEL_RATE));
                    }
                }
                Err(e) => {
                    if self.failed.is_none() {
                        eprintln!("[dictation] mic buffer: {e}");
                        self.failed = Some(e);
                    }
                }
            }
        }
    }

    /// One mono run of samples and its rate.
    fn read(raw: *const c_void) -> Result<(Vec<f32>, f64), String> {
        unsafe {
            let desc = CMSampleBufferGetFormatDescription(raw);
            if desc.is_null() {
                return Err("no format".into());
            }
            let asbd = *CMAudioFormatDescriptionGetStreamBasicDescription(desc).as_ref().ok_or("no stream description")?;
            let ch = asbd.channels_per_frame as usize;
            if asbd.format_id != LPCM || ch == 0 {
                return Err(format!("not linear PCM ({:08x}, {ch} ch)", asbd.format_id));
            }
            let float = asbd.format_flags & FLAG_FLOAT != 0 && asbd.bits_per_channel == 32;
            let int16 = !float && asbd.format_flags & FLAG_SIGNED != 0 && asbd.bits_per_channel == 16;
            let int32 = !float && asbd.format_flags & FLAG_SIGNED != 0 && asbd.bits_per_channel == 32;
            if !float && !int16 && !int32 {
                return Err(format!("unsupported samples (flags {:x}, {} bits)", asbd.format_flags, asbd.bits_per_channel));
            }
            let data = CMSampleBufferGetDataBuffer(raw);
            if data.is_null() {
                return Err("no data buffer".into());
            }
            let len = CMBlockBufferGetDataLength(data);
            let mut bytes = vec![0u8; len];
            if len > 0 && CMBlockBufferCopyDataBytes(data, 0, len, bytes.as_mut_ptr() as *mut c_void) != 0 {
                return Err("copying the samples failed".into());
            }
            let samples: Vec<f32> = if float {
                bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect()
            } else if int32 {
                bytes.chunks_exact(4).map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0).collect()
            } else {
                bytes.chunks_exact(2).map(|b| i16::from_ne_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
            };
            let planar = asbd.format_flags & FLAG_PLANAR != 0 && ch > 1;
            Ok((to_mono(&samples, ch, planar), asbd.sample_rate))
        }
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowDictationSink"]
        #[ivars = Arc<Mutex<Buf>>]
        struct Sink;

        unsafe impl NSObjectProtocol for Sink {}

        unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for Sink {
            #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
            fn did_output(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {
                if let Ok(mut b) = self.ivars().lock() {
                    b.take(sample as *const CMSampleBuffer as *const c_void);
                }
            }
        }
    );

    impl Sink {
        fn new(buf: Arc<Mutex<Buf>>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(buf);
            unsafe { msg_send![super(this), init] }
        }
    }

    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    /// The default microphone, listening.
    pub struct Mic {
        session: Retained<AVCaptureSession>,
        _sink: Retained<Sink>,
        buf: Arc<Mutex<Buf>>,
    }

    impl Mic {
        pub fn start() -> Result<Mic, String> {
            unsafe {
                let media = AVMediaTypeAudio.ok_or("no audio media type")?;
                let dev = AVCaptureDevice::defaultDeviceWithMediaType(media).ok_or("No microphone")?;
                let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&dev).map_err(|e| e.localizedDescription().to_string())?;
                let session = AVCaptureSession::new();
                let output = AVCaptureAudioDataOutput::new();
                if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                    return Err("The microphone is busy".into());
                }
                session.addInput(&input);
                session.addOutput(&output);
                let buf = Arc::new(Mutex::new(Buf::default()));
                let sink = Sink::new(buf.clone());
                let queue = DispatchQueue::new("cmdmeow.dictation", None);
                output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*sink)), Some(&queue));
                // starting takes a moment; not on the UI thread
                let s = SendBox(session.clone());
                serial(move || {
                    let s = s;
                    s.0.startRunning();
                });
                Ok(Mic { session, _sink: sink, buf })
            }
        }

        /// How loud the voice is right now, 0..1.
        pub fn level(&self) -> f32 {
            self.buf.lock().map(|b| b.level).unwrap_or(0.0)
        }

        /// Voice heard so far.
        pub fn seconds(&self) -> f32 {
            self.buf.lock().map(|b| b.pcm.len() as f32 / MODEL_RATE as f32).unwrap_or(0.0)
        }

        pub fn failed(&self) -> Option<String> {
            self.buf.lock().ok().and_then(|b| b.failed.clone())
        }

        /// Stop listening; the voice at 16 kHz mono.
        pub fn stop(self) -> Vec<f32> {
            let s = SendBox(self.session.clone());
            serial(move || {
                let s = s;
                unsafe { s.0.stopRunning() };
            });
            self.buf.lock().map(|mut b| std::mem::take(&mut b.pcm)).unwrap_or_default()
        }
    }

    /// Run `f` on the one thread that starts and stops sessions, so a stop
    /// never overtakes its start.
    fn serial(f: impl FnOnce() + Send + 'static) {
        type Job = Box<dyn FnOnce() + Send>;
        static Q: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<Job>>> = std::sync::OnceLock::new();
        let q = Q.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::spawn(move || {
                for job in rx {
                    job();
                }
            });
            Mutex::new(tx)
        });
        if let Ok(q) = q.lock() {
            let _ = q.send(Box::new(f));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_becomes_mono() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2, false), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[1.0, 0.5, 0.0, 0.5], 2, true), vec![0.5, 0.5]);
    }

    #[test]
    fn resampling_keeps_length_and_level() {
        let one_sec: Vec<f32> = vec![0.25; 48_000];
        let out = resample(&one_sec, 48_000.0, MODEL_RATE);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-6));
        let up = resample(&vec![0.5; 8_000], 8_000.0, MODEL_RATE);
        assert_eq!(up.len(), 16_000);
        assert_eq!(resample(&[], 44_100.0, MODEL_RATE), Vec::<f32>::new());
    }

    #[test]
    fn silence_is_told_from_speech() {
        assert!(silent(&vec![0.001; 16_000], MODEL_RATE));
        let mut v = vec![0.0; 16_000];
        for (k, s) in v.iter_mut().enumerate().skip(4000).take(1600) {
            *s = 0.2 * ((k as f32) * 0.3).sin();
        }
        assert!(!silent(&v, MODEL_RATE));
        assert_eq!(level(0.0), 0.0);
        assert_eq!(level(1.0), 1.0);
    }

    #[test]
    fn text_is_tidied() {
        let segs = vec![" Привіт,".to_string(), " [music] ".into(), "  як справи?  ".into(), "(laughs)".into()];
        assert_eq!(tidy_text(&segs), "Привіт, як справи?");
        assert_eq!(tidy_text(&[]), "");
    }
}
