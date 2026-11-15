//! A recording's sound as samples: read a track out of the file (at the
//! rate and channel count asked for), and write cleaned-up sound to a small
//! audio file. macOS only; elsewhere there's no sound to read.

/// Which audio track. A recording with both system sound and the
/// microphone has three: the mix (on), system, mic (both off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Mix,
    System,
    Mic,
}

impl Track {
    /// Its place among the file's audio tracks, given how many there are.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn index(self, count: usize) -> Option<usize> {
        match (self, count) {
            (_, 0) => None,
            (Track::Mix, _) => Some(0),
            (Track::System, n) if n >= 3 => Some(1),
            (Track::Mic, n) if n >= 3 => Some(2),
            _ => None,
        }
    }
}

/// The voice to work on: the mic on its own when there is one, else all.
pub fn voice_track(count: usize) -> Track {
    if count >= 3 {
        Track::Mic
    } else {
        Track::Mix
    }
}

#[cfg(target_os = "macos")]
pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use super::Track;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVAssetTrack, AVAssetWriter, AVAssetWriterInput, AVFileTypeAppleM4A, AVMediaTypeAudio, AVURLAsset};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};
    use std::ffi::c_void;
    use std::path::Path;

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

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RawTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
    const AAC: u32 = u32::from_be_bytes(*b"aac ");

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetDataBuffer(sb: *const c_void) -> *const c_void;
        fn CMBlockBufferGetDataLength(b: *const c_void) -> usize;
        fn CMBlockBufferCopyDataBytes(b: *const c_void, offset: usize, len: usize, dest: *mut c_void) -> i32;
        fn CMAudioFormatDescriptionCreate(alloc: *const c_void, asbd: *const Asbd, layout_size: usize, layout: *const c_void, cookie_size: usize, cookie: *const c_void, extensions: *const c_void, out: *mut *mut c_void) -> i32;
        fn CMBlockBufferCreateWithMemoryBlock(alloc: *const c_void, memory: *mut c_void, len: usize, block_alloc: *const c_void, custom: *const c_void, offset: usize, data_len: usize, flags: u32, out: *mut *mut c_void) -> i32;
        fn CMBlockBufferReplaceDataBytes(src: *const c_void, dest: *mut c_void, offset: usize, len: usize) -> i32;
        fn CMAudioSampleBufferCreateReadyWithPacketDescriptions(alloc: *const c_void, data: *mut c_void, format: *const c_void, samples: isize, pts: RawTime, packets: *const c_void, out: *mut *mut c_void) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(p: *const c_void);
    }

    fn dict(pairs: &[(&str, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let vals: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
        NSDictionary::from_slices(&key_refs, &vals)
    }

    fn url(p: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))
    }

    /// The file's audio tracks, with the asset: a track only points weakly
    /// at its asset, so the asset has to be kept while they're used.
    #[allow(deprecated)]
    fn audio_list(path: &Path) -> (Retained<AVURLAsset>, Vec<Retained<AVAssetTrack>>) {
        unsafe {
            let a = AVURLAsset::URLAssetWithURL_options(&url(path), None);
            let Some(m) = AVMediaTypeAudio else { return (a, Vec::new()) };
            let list: Retained<NSArray<AVAssetTrack>> = a.tracksWithMediaType(m);
            let tracks = list.iter().collect();
            (a, tracks)
        }
    }

    /// How many audio tracks the file has.
    pub fn audio_tracks(path: &Path) -> usize {
        audio_list(path).1.len()
    }

    /// `which` track as float samples at `rate`, `channels` interleaved.
    /// Slow for long files: call it off the UI thread.
    pub fn read_audio(path: &Path, which: Track, rate: u32, channels: u32) -> Result<Vec<f32>, String> {
        let n = audio_list(path).1.len();
        read_nth(path, which.index(n).ok_or("This video has no sound")?, rate, channels)
    }

    /// The `index`th audio track as float samples (see [`read_audio`]).
    pub fn read_nth(path: &Path, index: usize, rate: u32, channels: u32) -> Result<Vec<f32>, String> {
        let (asset, tracks) = audio_list(path);
        let track = tracks.get(index).ok_or("This video has no sound")?;
        unsafe {
            let reader = AVAssetReader::assetReaderWithAsset_error(&asset).map_err(|e| e.localizedDescription().to_string())?;
            let n = |v: f64| NSNumber::new_f64(v);
            let (fmt, sr, ch, bits) = (NSNumber::new_u32(LPCM), n(rate as f64), n(channels as f64), n(32.0));
            let (yes, no) = (NSNumber::new_bool(true), NSNumber::new_bool(false));
            let settings = dict(&[
                ("AVFormatIDKey", &fmt),
                ("AVSampleRateKey", &sr),
                ("AVNumberOfChannelsKey", &ch),
                ("AVLinearPCMBitDepthKey", &bits),
                ("AVLinearPCMIsFloatKey", &yes),
                ("AVLinearPCMIsNonInterleaved", &no),
                ("AVLinearPCMIsBigEndianKey", &no),
            ]);
            let out = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(track, Some(&settings));
            if !reader.canAddOutput(&out) {
                return Err("The sound can't be read".into());
            }
            reader.addOutput(&out);
            if !reader.startReading() {
                return Err(reader.error().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "The sound can't be read".into()));
            }
            let mut samples = Vec::new();
            while let Some(sb) = out.copyNextSampleBuffer() {
                let data = CMSampleBufferGetDataBuffer(&*sb as *const _ as *const c_void);
                if data.is_null() {
                    continue;
                }
                let len = CMBlockBufferGetDataLength(data);
                let mut bytes = vec![0u8; len];
                if len > 0 && CMBlockBufferCopyDataBytes(data, 0, len, bytes.as_mut_ptr() as *mut c_void) == 0 {
                    samples.extend(bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])));
                }
            }
            Ok(samples)
        }
    }

    /// Write `tracks` (48 kHz stereo, interleaved) to an m4a, the first one
    /// switched on and the rest off. Blocks until it's written.
    pub fn write_m4a(dest: &Path, tracks: &[&[f32]]) -> Result<(), String> {
        const RATE: f64 = 48_000.0;
        let _ = std::fs::remove_file(dest);
        unsafe {
            let ft = AVFileTypeAppleM4A.ok_or("m4a isn't available")?;
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url(dest), ft).map_err(|e| e.localizedDescription().to_string())?;
            let n = |v: f64| NSNumber::new_f64(v);
            let (fmt, sr, ch, br) = (NSNumber::new_u32(AAC), n(RATE), n(2.0), n(160_000.0));
            let aset = dict(&[("AVFormatIDKey", &fmt), ("AVSampleRateKey", &sr), ("AVNumberOfChannelsKey", &ch), ("AVEncoderBitRateKey", &br)]);
            let mut inputs = Vec::new();
            for k in 0..tracks.len() {
                let a = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeAudio.ok_or("no audio")?, Some(&aset));
                a.setExpectsMediaDataInRealTime(false);
                a.setMarksOutputTrackAsEnabled(k == 0);
                if !writer.canAddInput(&a) {
                    return Err("The sound can't be written".into());
                }
                writer.addInput(&a);
                inputs.push(a);
            }
            if !writer.startWriting() {
                return Err(writer.error().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "The sound can't be written".into()));
            }
            writer.startSessionAtSourceTime(objc2_core_media::CMTime { value: 0, timescale: 48_000, flags: objc2_core_media::CMTimeFlags::Valid, epoch: 0 });
            let asbd = Asbd { sample_rate: RATE, format_id: LPCM, format_flags: 1 | (1 << 3), bytes_per_packet: 8, frames_per_packet: 1, bytes_per_frame: 8, channels_per_frame: 2, bits_per_channel: 32, reserved: 0 };
            let mut desc = std::ptr::null_mut();
            if CMAudioFormatDescriptionCreate(std::ptr::null(), &asbd, 0, std::ptr::null(), 0, std::ptr::null(), std::ptr::null(), &mut desc) != 0 {
                return Err("The sound format isn't available".into());
            }
            // a second at a time, taking turns so the file stays interleaved
            let chunk = 48_000 * 2;
            let longest = tracks.iter().map(|t| t.len()).max().unwrap_or(0);
            let mut at = 0usize;
            let mut failed = None;
            'outer: while at < longest {
                for (input, samples) in inputs.iter().zip(tracks) {
                    let end = (at + chunk).min(samples.len());
                    if at >= end {
                        continue;
                    }
                    let part = &samples[at..end];
                    let mut waited = 0;
                    while !input.isReadyForMoreMediaData() {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        waited += 1;
                        if waited > 5000 {
                            failed = Some("Writing the sound stalled".to_string());
                            break 'outer;
                        }
                    }
                    let len = std::mem::size_of_val(part);
                    let mut block = std::ptr::null_mut();
                    if CMBlockBufferCreateWithMemoryBlock(std::ptr::null(), std::ptr::null_mut(), len, std::ptr::null(), std::ptr::null(), 0, len, 1, &mut block) != 0 {
                        failed = Some("Out of memory".into());
                        break 'outer;
                    }
                    CMBlockBufferReplaceDataBytes(part.as_ptr() as *const c_void, block, 0, len);
                    let pts = RawTime { value: (at / 2) as i64, timescale: 48_000, flags: 1, epoch: 0 };
                    let mut sb = std::ptr::null_mut();
                    let ok = CMAudioSampleBufferCreateReadyWithPacketDescriptions(std::ptr::null(), block, desc, (part.len() / 2) as isize, pts, std::ptr::null(), &mut sb) == 0;
                    CFRelease(block);
                    if !ok || sb.is_null() {
                        failed = Some("The sound couldn't be packed".into());
                        break 'outer;
                    }
                    let appended = input.appendSampleBuffer(&*(sb as *const objc2_core_media::CMSampleBuffer));
                    CFRelease(sb);
                    if !appended {
                        failed = Some(writer.error().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "The sound couldn't be written".into()));
                        break 'outer;
                    }
                }
                at += chunk;
            }
            CFRelease(desc);
            if let Some(e) = failed {
                writer.cancelWriting();
                let _ = std::fs::remove_file(dest);
                return Err(e);
            }
            for i in &inputs {
                i.markAsFinished();
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let block = block2::RcBlock::new(move || {
                let _ = tx.send(());
            });
            writer.finishWritingWithCompletionHandler(&block);
            let _ = rx.recv_timeout(std::time::Duration::from_secs(60));
            if writer.status() == objc2_av_foundation::AVAssetWriterStatus::Completed {
                Ok(())
            } else {
                Err(writer.error().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "The sound couldn't be finished".into()))
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::*;

#[cfg(not(target_os = "macos"))]
mod other {
    use super::Track;
    use std::path::Path;

    pub fn audio_tracks(_path: &Path) -> usize {
        0
    }

    pub fn read_audio(_path: &Path, _which: Track, _rate: u32, _channels: u32) -> Result<Vec<f32>, String> {
        Err("Reading sound needs macOS".into())
    }

    pub fn read_nth(_path: &Path, _index: usize, _rate: u32, _channels: u32) -> Result<Vec<f32>, String> {
        Err("Reading sound needs macOS".into())
    }

    pub fn write_m4a(_dest: &Path, _tracks: &[&[f32]]) -> Result<(), String> {
        Err("Writing sound needs macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_are_found_by_how_many_there_are() {
        assert_eq!(Track::Mic.index(3), Some(2));
        assert_eq!(Track::Mic.index(1), None);
        assert_eq!(Track::Mix.index(1), Some(0));
        assert_eq!(Track::Mix.index(0), None);
        assert_eq!(voice_track(3), Track::Mic);
        assert_eq!(voice_track(1), Track::Mix);
    }
}
