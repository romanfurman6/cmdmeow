//! The recorder itself: ScreenCaptureKit streams the screen, the system's
//! sound and the microphone; the frames go to an H.264 track and both sounds,
//! mixed, to one AAC track of an mp4 (AVAssetWriter). The system draws the
//! cursor (and click highlights) into the frames, and our own windows (the
//! control bar, the frame around the area) are left out of the picture while
//! the camera bubble is kept in: the file is finished the moment it's
//! stopped, nothing is drawn after. Off macOS, starting fails with a message.

use std::path::PathBuf;

/// What to record.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A whole display (by its display id).
    Display(u32),
    /// One window (by its window number), wherever it moves.
    Window(u32),
    /// Part of a display: x, y, w, h in points from its top-left corner.
    Area { display: u32, rect: [f64; 4] },
}

#[derive(Clone, Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Config {
    pub target: Target,
    /// Video size in pixels.
    pub size: (u32, u32),
    pub fps: u32,
    pub bitrate: u32,
    pub cursor: bool,
    pub clicks: bool,
    pub system_audio: bool,
    /// The microphone to record: None = off, Some(None) = the default one.
    pub mic: Option<Option<String>>,
    /// Our window that stays in the video (the camera bubble).
    pub keep_window: Option<u32>,
    pub path: PathBuf,
}

/// A finished recording.
pub struct Stopped {
    pub path: PathBuf,
    pub secs: f64,
    pub size: (u32, u32),
    /// A frame from near the end, small.
    pub thumb: Option<image::RgbaImage>,
}

/// Whether the system draws click highlights into the video (macOS 15+).
#[cfg(target_os = "macos")]
pub fn clicks_supported() -> bool {
    imp::clicks_supported()
}

#[cfg(not(target_os = "macos"))]
pub fn clicks_supported() -> bool {
    false
}

pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send + 'static>;

#[cfg(target_os = "macos")]
pub use imp::{start, Session};

#[cfg(not(target_os = "macos"))]
pub struct Session;

#[cfg(not(target_os = "macos"))]
pub fn start(_cfg: Config, reply: Reply<Session>) {
    reply(Err("Screen recording needs macOS".into()));
}

#[cfg(not(target_os = "macos"))]
impl Session {
    pub fn pause(&self) {}
    pub fn resume(&self) {}
    pub fn set_mic_muted(&self, _muted: bool) {}
    pub fn length(&self) -> f64 {
        0.0
    }
    pub fn failure(&self) -> Option<String> {
        None
    }
    pub fn mic_silent(&self) -> bool {
        false
    }
    pub fn stop(self, done: Reply<Stopped>) {
        done(Err("Screen recording needs macOS".into()));
    }
    pub fn discard(self) {}
}

#[cfg(target_os = "macos")]
mod imp {
    use super::super::mixer::{self, Mixer, Pcm, Src};
    use super::super::timeline::Clock;
    use super::{Config, Reply, Stopped, Target};
    use block2::RcBlock;
    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, Message};
    use objc2_av_foundation::{AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
    use objc2_core_video::CVPixelBuffer;
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL};
    use objc2_screen_capture_kit::{SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};

    // ------------------------------------------------------------ C pieces

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RawTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    impl RawTime {
        fn secs(self) -> Option<f64> {
            (self.flags & 1 == 1 && self.timescale > 0).then(|| self.value as f64 / self.timescale as f64)
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
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
    const AAC: u32 = u32::from_be_bytes(*b"aac ");
    const FLAG_FLOAT: u32 = 1;
    const FLAG_SIGNED: u32 = 1 << 2;
    const FLAG_PACKED: u32 = 1 << 3;
    const FLAG_PLANAR: u32 = 1 << 5;
    /// '420v': what the H.264 encoder wants, no conversion on the way.
    const PIXELS_420V: u32 = u32::from_be_bytes(*b"420v");

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetPresentationTimeStamp(sb: *const c_void) -> RawTime;
        fn CMSampleBufferGetImageBuffer(sb: *const c_void) -> *mut c_void;
        fn CMSampleBufferGetFormatDescription(sb: *const c_void) -> *const c_void;
        fn CMSampleBufferIsValid(sb: *const c_void) -> u8;
        fn CMAudioFormatDescriptionGetStreamBasicDescription(desc: *const c_void) -> *const Asbd;
        fn CMSampleBufferGetDataBuffer(sb: *const c_void) -> *const c_void;
        fn CMSampleBufferGetNumSamples(sb: *const c_void) -> isize;
        fn CMBlockBufferGetDataLength(b: *const c_void) -> usize;
        fn CMBlockBufferCopyDataBytes(b: *const c_void, offset: usize, len: usize, dest: *mut c_void) -> i32;
        fn CMAudioFormatDescriptionCreate(
            alloc: *const c_void,
            asbd: *const Asbd,
            layout_size: usize,
            layout: *const c_void,
            cookie_size: usize,
            cookie: *const c_void,
            extensions: *const c_void,
            out: *mut *mut c_void,
        ) -> i32;
        fn CMBlockBufferCreateWithMemoryBlock(
            alloc: *const c_void,
            memory: *mut c_void,
            len: usize,
            block_alloc: *const c_void,
            custom: *const c_void,
            offset: usize,
            data_len: usize,
            flags: u32,
            out: *mut *mut c_void,
        ) -> i32;
        fn CMBlockBufferReplaceDataBytes(src: *const c_void, dest: *mut c_void, offset: usize, len: usize) -> i32;
        fn CMAudioSampleBufferCreateReadyWithPacketDescriptions(
            alloc: *const c_void,
            data: *mut c_void,
            format: *const c_void,
            samples: isize,
            pts: RawTime,
            packets: *const c_void,
            out: *mut *mut c_void,
        ) -> i32;
        fn CMClockGetHostTimeClock() -> *const c_void;
        fn CMClockGetTime(clock: *const c_void) -> RawTime;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(p: *const c_void);
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVPixelBufferLockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddressOfPlane(pb: *mut c_void, plane: usize) -> *const u8;
        fn CVPixelBufferGetBytesPerRowOfPlane(pb: *mut c_void, plane: usize) -> usize;
        fn CVPixelBufferGetWidth(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetPixelFormatType(pb: *mut c_void) -> u32;
    }

    /// Now on the clock the sample buffers use.
    fn host_now() -> f64 {
        unsafe { CMClockGetTime(CMClockGetHostTimeClock()) }.secs().unwrap_or(0.0)
    }

    fn cm_time(secs: f64) -> CMTime {
        CMTime { value: (secs * 60_000.0).round() as i64, timescale: 60_000, flags: CMTimeFlags::Valid, epoch: 0 }
    }

    fn ns_err(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    fn any<T: Message>(o: &T) -> &AnyObject {
        // every Objective-C object is an AnyObject
        unsafe { &*(o as *const T as *const AnyObject) }
    }

    fn dict(pairs: &[(&str, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let vals: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
        NSDictionary::from_slices(&key_refs, &vals)
    }

    // ------------------------------------------------------------ diagnostics

    /// Recorder diagnostics go to stderr (the dev log), prefixed "[rec]".
    macro_rules! rlog {
        ($($a:tt)*) => { eprintln!("[rec {:.3}] {}", host_now(), format!($($a)*)) };
    }

    /// The most audio held back for an input that isn't ready, in seconds.
    const BACKLOG_SECS: usize = 4;

    /// Counters, printed about once a second while recording.
    #[derive(Default, Debug)]
    struct Stats {
        frames_in: u64,
        frames_blank: u64,
        frames_not_ready: u64,
        frames_failed: u64,
        sys_in: u64,
        mic_in: u64,
        audio_unmapped: u64,
        audio_unreadable: u64,
        audio_out_frames: u64,
        audio_not_ready: u64,
        audio_failed: u64,
        /// The loudest sample read from each source (before mixing).
        sys_peak: f32,
        mic_peak: f32,
        first: [Option<f64>; 3],
        printed_at: f64,
    }

    // ------------------------------------------------------------ writer side

    /// Everything the sample handler touches, behind one lock.
    struct Shared {
        writer: Retained<AVAssetWriter>,
        video: Retained<AVAssetWriterInput>,
        adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
        audio: Option<Retained<AVAssetWriterInput>>,
        /// System sound and the microphone, each on its own (switched off)
        /// track, for editing later; only when both are recorded.
        parts: Option<[Retained<AVAssetWriterInput>; 2]>,
        /// The format of the mixed audio we hand the writer.
        audio_fmt: *mut c_void,
        /// Audio an input wasn't ready for yet, per input (by address), in
        /// order: written as soon as it is, so a busy moment leaves no gap.
        backlog: std::collections::HashMap<usize, std::collections::VecDeque<(i64, Vec<f32>)>>,
        clock: Clock,
        mixer: Mixer,
        last_video: f64,
        frames: u64,
        thumb: Option<image::RgbaImage>,
        thumb_at: f64,
        failed: Option<String>,
        done: bool,
        stats: Stats,
    }

    // The writer objects are thread-safe for the use here (one serial queue
    // appends, other threads only pause/stop under the same lock).
    unsafe impl Send for Shared {}

    impl Drop for Shared {
        fn drop(&mut self) {
            if !self.audio_fmt.is_null() {
                unsafe { CFRelease(self.audio_fmt) };
            }
        }
    }

    impl Shared {
        fn on_sample(&mut self, sb: &CMSampleBuffer, kind: SCStreamOutputType) {
            if self.done || self.failed.is_some() {
                return;
            }
            let raw = sb as *const CMSampleBuffer as *const c_void;
            if unsafe { CMSampleBufferIsValid(raw) } == 0 {
                return;
            }
            let Some(t) = unsafe { CMSampleBufferGetPresentationTimeStamp(raw) }.secs() else { return };
            let k = if kind == SCStreamOutputType::Screen { 0 } else if kind == SCStreamOutputType::Audio { 1 } else { 2 };
            if self.stats.first[k].is_none() {
                self.stats.first[k] = Some(t);
                rlog!("first {} buffer at {t:.3} (host now {:.3}){}", ["screen", "system audio", "mic"][k], host_now(), if k > 0 { describe_audio(raw) } else { String::new() });
            }
            if kind == SCStreamOutputType::Screen {
                self.on_frame(raw, t);
            } else if kind == SCStreamOutputType::Audio {
                self.on_audio(raw, t, Src::System);
            } else if kind == SCStreamOutputType::Microphone {
                self.on_audio(raw, t, Src::Mic);
            }
            if unsafe { self.writer.status() } == AVAssetWriterStatus::Failed {
                let why = unsafe { self.writer.error() }.map(|e| ns_err(&e)).unwrap_or_else(|| "the video file couldn't be written".into());
                rlog!("writer failed: {why}");
                self.failed = Some(why);
            }
            let now = host_now();
            if now - self.stats.printed_at >= 1.0 {
                self.stats.printed_at = now;
                rlog!("{:?} mixer pos {} writer status {:?}", self.stats, self.mixer.position(), unsafe { self.writer.status() });
            }
        }

        fn on_frame(&mut self, raw: *const c_void, t: f64) {
            // frames where nothing changed come without a picture
            let pb = unsafe { CMSampleBufferGetImageBuffer(raw) };
            self.stats.frames_in += 1;
            if pb.is_null() {
                self.stats.frames_blank += 1;
                return;
            }
            self.clock.start(t);
            let Some(vt) = self.clock.map(t) else { return };
            if self.frames > 0 && vt <= self.last_video {
                return;
            }
            if unsafe { self.video.isReadyForMoreMediaData() } {
                let pixels = unsafe { &*(pb as *const CVPixelBuffer) };
                if unsafe { self.adaptor.appendPixelBuffer_withPresentationTime(pixels, cm_time(vt)) } {
                    self.last_video = vt;
                    self.frames += 1;
                } else {
                    self.stats.frames_failed += 1;
                }
            } else {
                self.stats.frames_not_ready += 1;
            }
            if self.thumb.is_none() || vt - self.thumb_at >= 1.0 {
                self.thumb = thumbnail(pb, 640);
                self.thumb_at = vt;
            }
        }

        fn on_audio(&mut self, raw: *const c_void, t: f64, src: Src) {
            if self.audio.is_none() {
                return;
            }
            match src {
                Src::System => self.stats.sys_in += 1,
                Src::Mic => self.stats.mic_in += 1,
            }
            let Some(vt) = self.clock.map(t) else {
                self.stats.audio_unmapped += 1;
                return;
            };
            let stereo = match read_pcm(raw) {
                Ok(v) => v,
                Err(why) => {
                    if self.stats.audio_unreadable == 0 {
                        rlog!("can't read {src:?} audio: {why}");
                    }
                    self.stats.audio_unreadable += 1;
                    return;
                }
            };
            let peak = stereo.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            match src {
                Src::System => self.stats.sys_peak = self.stats.sys_peak.max(peak),
                Src::Mic => self.stats.mic_peak = self.stats.mic_peak.max(peak),
            }
            let at = (vt * mixer::RATE as f64).round() as i64;
            self.mixer.push(src, at, &stereo);
            self.drain(false);
        }

        /// Hand the mixed audio that's ready to the writer.
        fn drain(&mut self, flush: bool) {
            while let Some((at, mix, parts)) = self.mixer.pull_split(flush) {
                if let Some(input) = self.audio.clone() {
                    self.append_audio(&input, at, &mix);
                }
                if let Some(inputs) = self.parts.clone() {
                    for (input, part) in inputs.iter().zip(parts.iter()) {
                        self.append_audio(input, at, part);
                    }
                }
            }
            if flush {
                // at the end, give each input a moment to take what it owes
                let inputs: Vec<Retained<AVAssetWriterInput>> = self.audio.iter().cloned().chain(self.parts.iter().flat_map(|p| p.iter().cloned())).collect();
                for _ in 0..50 {
                    for input in &inputs {
                        self.append_audio(input, 0, &[]);
                    }
                    if self.backlog.is_empty() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                self.stats.audio_not_ready += self.backlog.values().map(|q| q.len() as u64).sum::<u64>();
                self.backlog.clear();
            }
        }

        /// Queue `stereo` behind what `input` still owes, then write as much
        /// as it takes now. More than [`BACKLOG_SECS`] waiting drops the
        /// oldest (counted as not ready).
        fn append_audio(&mut self, input: &AVAssetWriterInput, at: i64, stereo: &[f32]) {
            let key = input as *const AVAssetWriterInput as usize;
            let mut q = self.backlog.remove(&key).unwrap_or_default();
            if !stereo.is_empty() {
                q.push_back((at, stereo.to_vec()));
            }
            let cap = BACKLOG_SECS * mixer::RATE as usize * 2;
            while q.iter().map(|(_, s)| s.len()).sum::<usize>() > cap {
                q.pop_front();
                self.stats.audio_not_ready += 1;
            }
            while !q.is_empty() && unsafe { input.isReadyForMoreMediaData() } {
                if let Some((at, chunk)) = q.pop_front() {
                    self.write_audio(input, at, &chunk);
                }
            }
            if !q.is_empty() {
                self.backlog.insert(key, q);
            }
        }

        fn write_audio(&mut self, input: &AVAssetWriterInput, at: i64, stereo: &[f32]) {
            if stereo.is_empty() {
                return;
            }
            unsafe {
                if self.audio_fmt.is_null() {
                    let asbd = Asbd {
                        sample_rate: mixer::RATE as f64,
                        format_id: LPCM,
                        format_flags: FLAG_FLOAT | FLAG_PACKED,
                        bytes_per_packet: 8,
                        frames_per_packet: 1,
                        bytes_per_frame: 8,
                        channels_per_frame: 2,
                        bits_per_channel: 32,
                        reserved: 0,
                    };
                    let mut fmt = std::ptr::null_mut();
                    if CMAudioFormatDescriptionCreate(std::ptr::null(), &asbd, 0, std::ptr::null(), 0, std::ptr::null(), std::ptr::null(), &mut fmt) != 0 {
                        return;
                    }
                    self.audio_fmt = fmt;
                }
                let len = std::mem::size_of_val(stereo);
                let mut block = std::ptr::null_mut();
                // kCMBlockBufferAssureMemoryNowFlag
                if CMBlockBufferCreateWithMemoryBlock(std::ptr::null(), std::ptr::null_mut(), len, std::ptr::null(), std::ptr::null(), 0, len, 1, &mut block) != 0 {
                    return;
                }
                if CMBlockBufferReplaceDataBytes(stereo.as_ptr() as *const c_void, block, 0, len) != 0 {
                    CFRelease(block);
                    return;
                }
                let pts = RawTime { value: at, timescale: mixer::RATE as i32, flags: 1, epoch: 0 };
                let mut sb = std::ptr::null_mut();
                let ok = CMAudioSampleBufferCreateReadyWithPacketDescriptions(std::ptr::null(), block, self.audio_fmt, (stereo.len() / 2) as isize, pts, std::ptr::null(), &mut sb) == 0;
                CFRelease(block);
                if ok && !sb.is_null() {
                    if input.appendSampleBuffer(&*(sb as *const CMSampleBuffer)) {
                        self.stats.audio_out_frames += (stereo.len() / 2) as u64;
                    } else {
                        self.stats.audio_failed += 1;
                    }
                    CFRelease(sb);
                } else {
                    self.stats.audio_failed += 1;
                }
            }
        }
    }

    /// The format of an audio sample buffer, for the log.
    fn describe_audio(raw: *const c_void) -> String {
        unsafe {
            let desc = CMSampleBufferGetFormatDescription(raw);
            let asbd = if desc.is_null() { None } else { CMAudioFormatDescriptionGetStreamBasicDescription(desc).as_ref().copied() };
            match asbd {
                Some(a) => format!(", {} Hz, {} ch, format {:08x} flags {:x}, {} bits", a.sample_rate, a.channels_per_frame, a.format_id, a.format_flags, a.bits_per_channel),
                None => ", no format".into(),
            }
        }
    }

    /// The audio in a sample buffer as interleaved stereo at the mixer's rate.
    fn read_pcm(raw: *const c_void) -> Result<Vec<f32>, String> {
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
            if !float && !int16 {
                return Err(format!("unsupported samples (flags {:x}, {} bits)", asbd.format_flags, asbd.bits_per_channel));
            }
            // the bytes as they are: one run of interleaved frames, or one
            // plane per channel back to back
            let data = CMSampleBufferGetDataBuffer(raw);
            if data.is_null() {
                return Err("no data buffer".into());
            }
            let len = CMBlockBufferGetDataLength(data);
            let mut bytes = vec![0u8; len];
            if len > 0 {
                let st = CMBlockBufferCopyDataBytes(data, 0, len, bytes.as_mut_ptr() as *mut c_void);
                if st != 0 {
                    return Err(format!("copying the samples failed ({st})"));
                }
            }
            let samples: Vec<f32> = if float {
                bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect()
            } else {
                bytes.chunks_exact(2).map(|b| i16::from_ne_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
            };
            let planar = asbd.format_flags & FLAG_PLANAR != 0 && ch > 1;
            let planes: Vec<&[f32]> = if planar {
                let stride = samples.len() / ch;
                let frames = (CMSampleBufferGetNumSamples(raw).max(0) as usize).min(stride);
                (0..ch).map(|c| &samples[c * stride..c * stride + frames]).collect()
            } else {
                vec![&samples[..]]
            };
            let pcm = Pcm { rate: asbd.sample_rate, channels: ch, planes, interleaved: !planar };
            Ok(mixer::to_stereo(&pcm))
        }
    }

    /// A small RGB copy of a '420v' (or BGRA) frame, at most `max_w` wide.
    fn thumbnail(pb: *mut c_void, max_w: u32) -> Option<image::RgbaImage> {
        unsafe {
            if CVPixelBufferLockBaseAddress(pb, 1) != 0 {
                return None;
            }
            let (w, h) = (CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb));
            let fmt = CVPixelBufferGetPixelFormatType(pb);
            let k = (max_w as f64 / w.max(1) as f64).min(1.0);
            let (tw, th) = (((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1));
            let mut img = image::RgbaImage::new(tw, th);
            let y_base = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
            let y_row = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
            let ok = !y_base.is_null();
            if ok && fmt == PIXELS_420V {
                let c_base = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
                let c_row = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
                for ty in 0..th {
                    let sy = ((ty as f64 / k) as usize).min(h - 1);
                    for tx in 0..tw {
                        let sx = ((tx as f64 / k) as usize).min(w - 1);
                        let y = *y_base.add(sy * y_row + sx) as f32;
                        let (cb, cr) = if c_base.is_null() {
                            (128.0, 128.0)
                        } else {
                            let c = c_base.add((sy / 2) * c_row + (sx / 2) * 2);
                            (*c as f32, *c.add(1) as f32)
                        };
                        // BT.709, video range
                        let yy = 1.164 * (y - 16.0);
                        let r = yy + 1.793 * (cr - 128.0);
                        let g = yy - 0.213 * (cb - 128.0) - 0.533 * (cr - 128.0);
                        let b = yy + 2.112 * (cb - 128.0);
                        img.put_pixel(tx, ty, image::Rgba([r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8, 255]));
                    }
                }
            } else if ok {
                // BGRA in one plane
                for ty in 0..th {
                    let sy = ((ty as f64 / k) as usize).min(h - 1);
                    for tx in 0..tw {
                        let sx = ((tx as f64 / k) as usize).min(w - 1);
                        let p = y_base.add(sy * y_row + sx * 4);
                        img.put_pixel(tx, ty, image::Rgba([*p.add(2), *p.add(1), *p, 255]));
                    }
                }
            }
            CVPixelBufferUnlockBaseAddress(pb, 1);
            ok.then_some(img)
        }
    }

    // ------------------------------------------------------------ stream output

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowRecordOutput"]
        #[ivars = Arc<Mutex<Shared>>]
        struct Output;

        unsafe impl NSObjectProtocol for Output {}

        unsafe impl SCStreamOutput for Output {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
                if let Ok(mut s) = self.ivars().lock() {
                    s.on_sample(sample, kind);
                }
            }
        }

        unsafe impl SCStreamDelegate for Output {
            #[unsafe(method(stream:didStopWithError:))]
            fn did_stop(&self, _stream: &SCStream, error: &NSError) {
                rlog!("stream stopped: {}", ns_err(error));
                if let Ok(mut s) = self.ivars().lock() {
                    if !s.done {
                        s.failed = Some(ns_err(error));
                    }
                }
            }
        }
    );

    impl Output {
        fn new(shared: Arc<Mutex<Shared>>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(shared);
            unsafe { msg_send![super(this), init] }
        }
    }

    // ------------------------------------------------------------ session

    /// A running recording.
    pub struct Session {
        stream: Retained<SCStream>,
        output: Retained<Output>,
        shared: Arc<Mutex<Shared>>,
        path: std::path::PathBuf,
        size: (u32, u32),
    }

    // Only handed between threads; ScreenCaptureKit and AVFoundation objects
    // used here may be called from any thread.
    unsafe impl Send for Session {}

    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    impl Session {
        pub fn pause(&self) {
            if let Ok(mut s) = self.shared.lock() {
                s.clock.pause(host_now());
            }
        }

        pub fn resume(&self) {
            if let Ok(mut s) = self.shared.lock() {
                s.clock.resume(host_now());
            }
        }

        pub fn set_mic_muted(&self, muted: bool) {
            if let Ok(mut s) = self.shared.lock() {
                s.mixer.mic_gain = if muted { 0.0 } else { 1.0 };
            }
        }

        /// Length of the video so far, in seconds.
        pub fn length(&self) -> f64 {
            self.shared.lock().map(|s| s.clock.length(host_now())).unwrap_or(0.0)
        }

        /// Why the recording broke off, if it did.
        pub fn failure(&self) -> Option<String> {
            self.shared.lock().ok().and_then(|s| s.failed.clone())
        }

        /// The microphone has sent a good while of pure digital silence
        /// (a closed lid, AirPods not worn): no real mic is ever that quiet.
        pub fn mic_silent(&self) -> bool {
            self.shared.lock().map(|s| s.stats.mic_in >= 200 && s.stats.mic_peak == 0.0).unwrap_or(false)
        }

        /// Stop and finish the file; `done` runs on a background thread.
        pub fn stop(self, done: Reply<Stopped>) {
            let Session { stream, output, shared, path, size } = self;
            let cell = Mutex::new(Some(SendBox((done, output, shared, path))));
            let block = RcBlock::new(move |_err: *mut NSError| {
                let Some(SendBox((done, _output, shared, path))) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
                finish(shared, path, size, done);
            });
            unsafe { stream.stopCaptureWithCompletionHandler(Some(&block)) };
        }

        /// Stop and throw the file away.
        pub fn discard(self) {
            let path = self.path.clone();
            if let Ok(mut s) = self.shared.lock() {
                s.done = true;
                unsafe { s.writer.cancelWriting() };
            }
            let keep = SendBox(self.output.clone());
            let block = RcBlock::new(move |_err: *mut NSError| {
                let _ = &keep;
                let _ = std::fs::remove_file(&path);
            });
            unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&block)) };
        }
    }

    fn finish(shared: Arc<Mutex<Shared>>, path: std::path::PathBuf, size: (u32, u32), done: Reply<Stopped>) {
        let mut s = match shared.lock() {
            Ok(s) => s,
            Err(_) => return done(Err("The recorder broke".into())),
        };
        s.done = true;
        rlog!("stop: {:?} frames {} length {:.2}", s.stats, s.frames, s.clock.length(host_now()));
        if let Some(why) = s.failed.clone() {
            unsafe { s.writer.cancelWriting() };
            let _ = std::fs::remove_file(&path);
            return done(Err(why));
        }
        if s.frames == 0 {
            unsafe { s.writer.cancelWriting() };
            let _ = std::fs::remove_file(&path);
            return done(Err("Nothing was recorded".into()));
        }
        s.drain(true);
        let secs = s.clock.length(host_now()).max(s.last_video);
        let thumb = s.thumb.take();
        unsafe {
            s.video.markAsFinished();
            if let Some(a) = &s.audio {
                a.markAsFinished();
            }
            for a in s.parts.iter().flatten() {
                a.markAsFinished();
            }
            s.writer.endSessionAtSourceTime(cm_time(secs));
        }
        let writer = s.writer.clone();
        drop(s);
        let cell = Mutex::new(Some(SendBox((done, thumb, writer.clone(), shared))));
        let block = RcBlock::new(move || {
            let Some(SendBox((done, thumb, writer, _shared))) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
            rlog!("finished: status {:?} error {:?}", unsafe { writer.status() }, unsafe { writer.error() }.map(|e| ns_err(&e)));
            if unsafe { writer.status() } == AVAssetWriterStatus::Completed {
                done(Ok(Stopped { path: path.clone(), secs, size, thumb }));
            } else {
                let why = unsafe { writer.error() }.map(|e| ns_err(&e)).unwrap_or_else(|| "the video file couldn't be finished".into());
                done(Err(why));
            }
        });
        unsafe { writer.finishWritingWithCompletionHandler(&block) };
    }

    // ------------------------------------------------------------ start

    /// Start recording; `reply` gets the session (or why not) on a
    /// background thread once frames are flowing.
    pub fn start(cfg: Config, reply: Reply<Session>) {
        let cell = Mutex::new(Some(SendBox((cfg, reply))));
        let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
            let Some(SendBox((cfg, reply))) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
            let content = unsafe { content.as_ref() };
            match (content, unsafe { err.as_ref() }) {
                (Some(content), _) => match build(&cfg, content) {
                    Ok((stream, output, shared)) => run(stream, output, shared, cfg, reply),
                    Err(e) => reply(Err(e)),
                },
                (None, Some(e)) => reply(Err(ns_err(e))),
                (None, None) => reply(Err("Screen Recording isn't allowed".into())),
            }
        });
        unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &block) };
    }

    type Built = (Retained<SCStream>, Retained<Output>, Arc<Mutex<Shared>>);

    fn responds(obj: &AnyObject, sel: objc2::runtime::Sel) -> bool {
        unsafe { msg_send![obj, respondsToSelector: sel] }
    }

    fn build(cfg: &Config, content: &SCShareableContent) -> Result<Built, String> {
        unsafe {
            let me = std::process::id() as i32;
            let apps = content.applications();
            let ours: Vec<_> = apps.iter().filter(|a| a.processID() == me).collect();
            let windows = content.windows();
            let keep: Vec<_> = windows.iter().filter(|w| Some(w.windowID()) == cfg.keep_window).collect();
            let display_of = |id: u32| content.displays().iter().find(|d| d.displayID() == id);
            // the part of the display that's recorded (display points, top-left origin)
            let mut source: Option<CGRect> = None;
            let filter = match &cfg.target {
                Target::Display(id) | Target::Area { display: id, .. } => {
                    let d = display_of(*id).ok_or("That display is gone")?;
                    let ours = NSArray::from_retained_slice(&ours);
                    let keep = NSArray::from_retained_slice(&keep);
                    if let Target::Area { rect, .. } = &cfg.target {
                        source = Some(CGRect { origin: CGPoint { x: rect[0], y: rect[1] }, size: CGSize { width: rect[2], height: rect[3] } });
                    }
                    SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &d, &ours, &keep)
                }
                Target::Window(id) => {
                    let w = windows.iter().find(|w| w.windowID() == *id).ok_or("That window is gone")?;
                    if keep.is_empty() {
                        // the window on its own, wherever it moves
                        SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &w)
                    } else {
                        // with the camera: the window's spot on its display,
                        // the window and the bubble over it
                        let f = w.frame();
                        let mid = CGPoint { x: f.origin.x + f.size.width / 2.0, y: f.origin.y + f.size.height / 2.0 };
                        let all = content.displays();
                        let d = all
                            .iter()
                            .find(|d| {
                                let r = d.frame();
                                mid.x >= r.origin.x && mid.x < r.origin.x + r.size.width && mid.y >= r.origin.y && mid.y < r.origin.y + r.size.height
                            })
                            .or_else(|| all.firstObject())
                            .ok_or("No display to record")?;
                        let r = d.frame();
                        source = Some(CGRect { origin: CGPoint { x: f.origin.x - r.origin.x, y: f.origin.y - r.origin.y }, size: f.size });
                        let mut both = vec![w.clone()];
                        both.extend(keep.iter().cloned());
                        let both = NSArray::from_retained_slice(&both);
                        SCContentFilter::initWithDisplay_includingWindows(SCContentFilter::alloc(), &d, &both)
                    }
                }
            };

            let conf = SCStreamConfiguration::new();
            conf.setWidth(cfg.size.0 as usize);
            conf.setHeight(cfg.size.1 as usize);
            conf.setMinimumFrameInterval(CMTime { value: 1, timescale: cfg.fps.max(1) as i32, flags: CMTimeFlags::Valid, epoch: 0 });
            conf.setPixelFormat(PIXELS_420V);
            conf.setQueueDepth(6);
            // the system draws the cursor (and the clicks) right into the frames
            conf.setShowsCursor(cfg.cursor);
            if responds(any(&*conf), sel!(setShowMouseClicks:)) {
                conf.setShowMouseClicks(cfg.cursor && cfg.clicks);
            }
            if let Some(r) = source {
                conf.setSourceRect(r);
            }
            // audio capture came with macOS 13: older systems record video only
            if responds(any(&*conf), sel!(setCapturesAudio:)) {
                conf.setCapturesAudio(cfg.system_audio);
                conf.setSampleRate(mixer::RATE as isize);
                conf.setChannelCount(2);
                conf.setExcludesCurrentProcessAudio(true);
            }
            let mic = cfg.mic.is_some() && responds(any(&*conf), sel!(setCaptureMicrophone:));
            if mic {
                conf.setCaptureMicrophone(true);
                if let Some(Some(dev)) = &cfg.mic {
                    conf.setMicrophoneCaptureDeviceID(Some(&NSString::from_str(dev)));
                }
            }

            // the file
            let _ = std::fs::remove_file(&cfg.path);
            if let Some(dir) = cfg.path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let url = NSURL::fileURLWithPath(&NSString::from_str(&cfg.path.to_string_lossy()));
            let file_type = AVFileTypeMPEG4.ok_or("mp4 isn't available")?;
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type).map_err(|e| ns_err(&e))?;

            let num = |v: f64| NSNumber::new_f64(v);
            let (bitrate, fps, keyframes) = (num(cfg.bitrate as f64), num(cfg.fps as f64), num((cfg.fps * 2) as f64));
            let profile = NSString::from_str("H264_High_AutoLevel");
            let compression = dict(&[("AverageBitRate", any(&*bitrate)), ("ExpectedFrameRate", any(&*fps)), ("MaxKeyFrameInterval", any(&*keyframes)), ("ProfileLevel", any(&*profile))]);
            let (w, h) = (num(cfg.size.0 as f64), num(cfg.size.1 as f64));
            let codec = NSString::from_str("avc1");
            let vset = dict(&[("AVVideoCodecKey", any(&*codec)), ("AVVideoWidthKey", any(&*w)), ("AVVideoHeightKey", any(&*h)), ("AVVideoCompressionPropertiesKey", any(&*compression))]);
            let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeVideo.ok_or("no video")?, Some(&vset));
            video.setExpectsMediaDataInRealTime(true);
            if !writer.canAddInput(&video) {
                return Err("This Mac can't encode video at that size".into());
            }
            writer.addInput(&video);
            let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, None);

            let audio = if cfg.system_audio || mic {
                let (fmt, rate, ch, br) = (NSNumber::new_u32(AAC), num(mixer::RATE as f64), num(2.0), num(160_000.0));
                let aset = dict(&[("AVFormatIDKey", any(&*fmt)), ("AVSampleRateKey", any(&*rate)), ("AVNumberOfChannelsKey", any(&*ch)), ("AVEncoderBitRateKey", any(&*br))]);
                let a = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeAudio.ok_or("no audio")?, Some(&aset));
                a.setExpectsMediaDataInRealTime(true);
                if writer.canAddInput(&a) {
                    writer.addInput(&a);
                    Some(a)
                } else {
                    None
                }
            } else {
                None
            };
            // both sources: each also on its own track, off by default
            let parts = if audio.is_some() && cfg.system_audio && mic {
                let (fmt, rate, ch, br) = (NSNumber::new_u32(AAC), num(mixer::RATE as f64), num(2.0), num(128_000.0));
                let aset = dict(&[("AVFormatIDKey", any(&*fmt)), ("AVSampleRateKey", any(&*rate)), ("AVNumberOfChannelsKey", any(&*ch)), ("AVEncoderBitRateKey", any(&*br))]);
                let make = || {
                    let a = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeAudio?, Some(&aset));
                    a.setExpectsMediaDataInRealTime(true);
                    a.setMarksOutputTrackAsEnabled(false);
                    writer.canAddInput(&a).then_some(a)
                };
                match (make(), make()) {
                    (Some(a), Some(b)) => {
                        writer.addInput(&a);
                        writer.addInput(&b);
                        Some([a, b])
                    }
                    _ => None,
                }
            } else {
                None
            };
            if !writer.startWriting() {
                return Err(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "Couldn't start the video file".into()));
            }
            writer.startSessionAtSourceTime(cm_time(0.0));

            let shared = Arc::new(Mutex::new(Shared {
                writer,
                video,
                adaptor,
                audio,
                parts,
                audio_fmt: std::ptr::null_mut(),
                backlog: std::collections::HashMap::new(),
                clock: Clock::default(),
                mixer: Mixer::new(cfg.system_audio, mic),
                last_video: 0.0,
                frames: 0,
                thumb: None,
                thumb_at: 0.0,
                failed: None,
                done: false,
                stats: Stats::default(),
            }));
            rlog!("start: {:?} {}x{} @{} fps, system audio {}, mic {:?} (capture mic: {mic}), audio input {}, separate tracks {}", cfg.target, cfg.size.0, cfg.size.1, cfg.fps, cfg.system_audio, cfg.mic, shared.lock().map(|s| s.audio.is_some()).unwrap_or(false), shared.lock().map(|s| s.parts.is_some()).unwrap_or(false));
            let output = Output::new(shared.clone());
            let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*output);
            let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &conf, Some(delegate));
            let queue = DispatchQueue::new("cmdmeow.record", None);
            let out: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
            stream.addStreamOutput_type_sampleHandlerQueue_error(out, SCStreamOutputType::Screen, Some(&queue)).map_err(|e| ns_err(&e))?;
            if cfg.system_audio {
                stream.addStreamOutput_type_sampleHandlerQueue_error(out, SCStreamOutputType::Audio, Some(&queue)).map_err(|e| ns_err(&e))?;
            }
            if mic {
                stream.addStreamOutput_type_sampleHandlerQueue_error(out, SCStreamOutputType::Microphone, Some(&queue)).map_err(|e| ns_err(&e))?;
            }
            Ok((stream, output, shared))
        }
    }

    fn run(stream: Retained<SCStream>, output: Retained<Output>, shared: Arc<Mutex<Shared>>, cfg: Config, reply: Reply<Session>) {
        let session = Session { stream: stream.clone(), output, shared, path: cfg.path.clone(), size: cfg.size };
        let cell = Mutex::new(Some(SendBox((session, reply))));
        let block = RcBlock::new(move |err: *mut NSError| {
            let Some(SendBox((session, reply))) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
            match unsafe { err.as_ref() } {
                Some(e) => {
                    let why = ns_err(e);
                    rlog!("start failed: {why}");
                    session.discard();
                    reply(Err(why));
                }
                None => reply(Ok(session)),
            }
        });
        unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
    }

    /// The system draws click highlights into the frames (macOS 15+).
    pub fn clicks_supported() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| {
            let conf = unsafe { SCStreamConfiguration::new() };
            responds(any(&*conf), sel!(setShowMouseClicks:))
        })
    }
}
