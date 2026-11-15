//! What the viewer asks of AVFoundation: play a file and hand its frames
//! over as pixels, make a strip of small frames, and write the kept pieces
//! to a new file. Elsewhere: nothing plays and nothing is written.

use super::edit::Edl;
use std::path::{Path, PathBuf};

pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send + 'static>;

/// A frame for the screen, RGBA.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// The widest frame the viewer asks for (it's scaled to fit anyway).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const VIEW_W: f64 = 1600.0;

/// BGRA rows (with padding) → tight RGBA.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bgra_to_rgba(src: &[u8], w: usize, h: usize, row: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let s = &src[y * row..y * row + w * 4];
        let d = &mut out[y * w * 4..(y + 1) * w * 4];
        for (dp, sp) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            dp[0] = sp[2];
            dp[1] = sp[1];
            dp[2] = sp[0];
            dp[3] = 255;
        }
    }
    out
}

/// Where an edited file goes before it replaces `path`.
pub fn temp_beside(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "video.mp4".into());
    path.with_file_name(format!(".{name}.edit.mp4"))
}

/// "Recording 1.mp4" → "Recording 1 edited.mp4" (not taken yet).
pub fn copy_name(path: &Path) -> PathBuf {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Video".into());
    crate::tools::capture::record::free_path(&dir, &format!("{stem} edited.mp4"))
}

#[cfg(target_os = "macos")]
pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use super::{Edl, Frame, Reply};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_av_foundation::{
        AVAsset, AVAssetExportPresetPassthrough, AVAssetExportSession, AVAssetExportSessionStatus, AVAssetImageGenerator, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVMutableComposition, AVPlayer, AVPlayerItem,
        AVPlayerItemVideoOutput, AVURLAsset,
    };
    use objc2_core_foundation::CGSize;
    use objc2_core_media::{CMTime, CMTimeFlags, CMTimeRange};
    use objc2_core_video::CVPixelBuffer;
    use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString, NSURL};
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVPixelBufferLockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(pb: *mut c_void) -> *const u8;
        fn CVPixelBufferGetBytesPerRow(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetWidth(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(pb: *mut c_void) -> usize;
    }

    #[repr(C)]
    struct Rect64 {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(img: *const c_void) -> usize;
        fn CGImageGetHeight(img: *const c_void) -> usize;
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(cs: *mut c_void);
        fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row: usize, cs: *mut c_void, info: u32) -> *mut c_void;
        fn CGContextDrawImage(ctx: *mut c_void, r: Rect64, img: *const c_void);
        fn CGContextRelease(ctx: *mut c_void);
    }

    struct SendBox<T>(T);
    unsafe impl<T> Send for SendBox<T> {}

    fn cm(secs: f64) -> CMTime {
        CMTime { value: (secs.max(0.0) * 60_000.0).round() as i64, timescale: 60_000, flags: CMTimeFlags::Valid, epoch: 0 }
    }

    fn secs(t: CMTime) -> f64 {
        let s = unsafe { t.seconds() };
        if s.is_finite() {
            s
        } else {
            0.0
        }
    }

    fn url(p: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))
    }

    fn ns_err(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    fn asset(p: &Path) -> Retained<AVURLAsset> {
        unsafe { AVURLAsset::URLAssetWithURL_options(&url(p), None) }
    }

    /// The video's size in pixels.
    #[allow(deprecated)]
    fn video_size(a: &AVAsset) -> Option<(f64, f64)> {
        unsafe {
            let tracks = a.tracksWithMediaType(AVMediaTypeVideo?);
            let s = tracks.firstObject()?.naturalSize();
            (s.width > 0.0 && s.height > 0.0).then_some((s.width, s.height))
        }
    }

    /// What plays and what's saved: the file itself, or its picture with
    /// the cleaned sound from `voice` (the mix on; system sound and the
    /// cleaned voice off, as the recorder writes them).
    #[allow(deprecated)]
    fn source(path: &Path, voice: Option<&Path>) -> Result<Retained<AVAsset>, String> {
        let orig = asset(path);
        let Some(voice) = voice else { return Ok(Retained::into_super(orig)) };
        unsafe {
            let snd = asset(voice);
            let (vm, am) = (AVMediaTypeVideo.ok_or("no video")?, AVMediaTypeAudio.ok_or("no audio")?);
            let d = orig.duration();
            let comp = AVMutableComposition::composition();
            let add = |media: &objc2_av_foundation::AVMediaType, from: &objc2_av_foundation::AVAssetTrack, on: bool| -> Result<(), String> {
                let t = comp.addMutableTrackWithMediaType_preferredTrackID(media, 0).ok_or("can't add a track")?;
                let r = from.timeRange();
                let len = if secs(r.duration) > secs(d) { d } else { r.duration };
                t.insertTimeRange_ofTrack_atTime_error(CMTimeRange { start: r.start, duration: len }, from, cm(0.0)).map_err(|e| ns_err(&e))?;
                t.setEnabled(on);
                Ok(())
            };
            // a recording's camera track stays off, as it was
            for v in orig.tracksWithMediaType(vm).iter() {
                add(vm, &v, v.isEnabled())?;
            }
            let (oa, sa) = (orig.tracksWithMediaType(am), snd.tracksWithMediaType(am));
            let mix = sa.firstObject().ok_or("the cleaned sound is empty")?;
            add(am, &mix, true)?;
            if oa.count() >= 3 {
                add(am, &oa.objectAtIndex(1), false)?;
                if sa.count() >= 2 {
                    add(am, &sa.objectAtIndex(1), false)?;
                }
            }
            Ok(Retained::into_super(Retained::into_super(comp)))
        }
    }

    pub struct Player {
        player: Retained<AVPlayer>,
        item: Retained<AVPlayerItem>,
        output: Retained<AVPlayerItemVideoOutput>,
        pub duration: f64,
        /// The video's own size in pixels.
        pub size: (u32, u32),
    }

    impl Player {
        /// Main thread only.
        pub fn open(path: &Path, voice: Option<&Path>) -> Result<Player, String> {
            let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
            if !path.exists() {
                return Err("The file isn't there any more".into());
            }
            unsafe {
                let a = source(path, voice)?;
                let duration = secs(a.duration());
                let (w, h) = video_size(&a).ok_or("No video in this file")?;
                let scale = (super::VIEW_W / w).min(1.0);
                let (ow, oh) = (((w * scale) as u32 & !1).max(2), ((h * scale) as u32 & !1).max(2));
                let fmt = NSNumber::new_u32(u32::from_be_bytes(*b"BGRA"));
                let (nw, nh) = (NSNumber::new_u32(ow), NSNumber::new_u32(oh));
                let keys = [NSString::from_str("PixelFormatType"), NSString::from_str("Width"), NSString::from_str("Height")];
                let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
                let vals: [&AnyObject; 3] = [&fmt, &nw, &nh];
                let attrs = NSDictionary::from_slices(&key_refs, &vals);
                let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(AVPlayerItemVideoOutput::alloc(), Some(&attrs));
                let item = AVPlayerItem::playerItemWithAsset(&a, mtm);
                item.addOutput(&output);
                let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);
                Ok(Player { player, item, output, duration, size: (w as u32, h as u32) })
            }
        }

        /// Where playback is, source seconds.
        pub fn time(&self) -> f64 {
            secs(unsafe { self.item.currentTime() })
        }

        pub fn playing(&self) -> bool {
            unsafe { self.player.rate() > 0.0 }
        }

        pub fn play(&self, rate: f32) {
            unsafe { self.player.playImmediatelyAtRate(rate) };
        }

        pub fn pause(&self) {
            unsafe { self.player.pause() };
        }

        /// Jump to `t` exactly.
        pub fn seek(&self, t: f64) {
            let zero = cm(0.0);
            unsafe { self.player.seekToTime_toleranceBefore_toleranceAfter(cm(t), zero, zero) };
        }

        /// The frame to show now, when there's a new one.
        pub fn frame(&self) -> Option<Frame> {
            unsafe {
                let t = self.item.currentTime();
                if !self.output.hasNewPixelBufferForItemTime(t) {
                    return None;
                }
                let pb = self.output.copyPixelBufferForItemTime_itemTimeForDisplay(t, std::ptr::null_mut())?;
                let raw = &*pb as *const CVPixelBuffer as *mut c_void;
                if CVPixelBufferLockBaseAddress(raw, 1) != 0 {
                    return None;
                }
                let (w, h, row) = (CVPixelBufferGetWidth(raw), CVPixelBufferGetHeight(raw), CVPixelBufferGetBytesPerRow(raw));
                let base = CVPixelBufferGetBaseAddress(raw);
                let out = (!base.is_null() && w > 0 && h > 0).then(|| super::bgra_to_rgba(std::slice::from_raw_parts(base, row * h), w, h, row));
                CVPixelBufferUnlockBaseAddress(raw, 1);
                out.map(|rgba| Frame { w, h, rgba })
            }
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            unsafe { self.player.pause() };
        }
    }

    /// CGImage → RGBA by drawing it into a bitmap we own.
    unsafe fn cg_rgba(img: *const c_void) -> Option<image::RgbaImage> {
        unsafe {
            let (w, h) = (CGImageGetWidth(img), CGImageGetHeight(img));
            if w == 0 || h == 0 {
                return None;
            }
            let mut buf = vec![0u8; w * h * 4];
            let cs = CGColorSpaceCreateDeviceRGB();
            // 8 bits, RGBA, premultiplied last (frames are opaque anyway)
            let ctx = CGBitmapContextCreate(buf.as_mut_ptr() as *mut c_void, w, h, 8, w * 4, cs, 1);
            CGColorSpaceRelease(cs);
            if ctx.is_null() {
                return None;
            }
            CGContextDrawImage(ctx, Rect64 { x: 0.0, y: 0.0, w: w as f64, h: h as f64 }, img);
            CGContextRelease(ctx);
            image::RgbaImage::from_raw(w as u32, h as u32, buf)
        }
    }

    /// The file's length in seconds.
    pub fn duration(path: &Path) -> f64 {
        unsafe { secs(asset(path).duration()) }
    }

    /// `n` small frames spread over the file, `height` px tall. Slow: call
    /// it off the UI thread.
    #[allow(deprecated)]
    pub fn strip(path: &Path, n: usize, height: f64) -> Vec<image::RgbaImage> {
        unsafe {
            let a = asset(path);
            let d = secs(a.duration());
            let Some((w, h)) = video_size(&a) else { return Vec::new() };
            let gen = AVAssetImageGenerator::assetImageGeneratorWithAsset(&a);
            gen.setAppliesPreferredTrackTransform(true);
            gen.setMaximumSize(CGSize { width: (height * w / h).ceil(), height });
            (0..n)
                .filter_map(|k| {
                    let t = d * (k as f64 + 0.5) / n as f64;
                    let img = gen.copyCGImageAtTime_actualTime_error(cm(t), std::ptr::null_mut()).ok()?;
                    cg_rgba(&*img as *const _ as *const c_void)
                })
                .collect()
        }
    }

    /// Write the kept pieces of `src` to `dest` (no re-encoding). `done`
    /// runs on a background thread.
    #[allow(deprecated)]
    pub fn export(src: &Path, voice: Option<&Path>, edl: &Edl, dest: PathBuf, done: Reply<PathBuf>) {
        let run = || -> Result<Retained<AVAssetExportSession>, String> {
            unsafe {
                let a = source(src, voice)?;
                let comp = AVMutableComposition::composition();
                let mut at = 0.0;
                for &(s, e) in &edl.keep {
                    let r = CMTimeRange { start: cm(s), duration: cm(e - s) };
                    comp.insertTimeRange_ofAsset_atTime_error(r, &a, cm(at)).map_err(|e| ns_err(&e))?;
                    at += e - s;
                }
                // keep each track on or off as it was (the separate voice tracks are off)
                for t in comp.tracks().iter() {
                    if let Some(st) = a.trackWithTrackID(t.trackID()) {
                        t.setEnabled(st.isEnabled());
                    }
                }
                let session = AVAssetExportSession::exportSessionWithAsset_presetName(&comp, AVAssetExportPresetPassthrough).ok_or("This video can't be saved that way")?;
                let _ = std::fs::remove_file(&dest);
                session.setOutputURL(Some(&url(&dest)));
                session.setOutputFileType(AVFileTypeMPEG4);
                Ok(session)
            }
        };
        let session = match run() {
            Ok(s) => s,
            Err(e) => return done(Err(e)),
        };
        let cell = Mutex::new(Some(SendBox((done, session.clone(), dest))));
        let block = RcBlock::new(move || {
            let Some(SendBox((done, session, dest))) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
            let status = unsafe { session.status() };
            if status == AVAssetExportSessionStatus::Completed {
                done(Ok(dest));
            } else {
                let _ = std::fs::remove_file(&dest);
                done(Err(unsafe { session.error() }.map(|e| ns_err(&e)).unwrap_or_else(|| "the video couldn't be written".into())));
            }
        });
        unsafe { session.exportAsynchronouslyWithCompletionHandler(&block) };
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::*;

#[cfg(not(target_os = "macos"))]
mod other {
    use super::{Edl, Frame, Reply};
    use std::path::{Path, PathBuf};

    pub struct Player {
        pub duration: f64,
        pub size: (u32, u32),
    }

    impl Player {
        pub fn open(_path: &Path, _voice: Option<&Path>) -> Result<Player, String> {
            Err("Video playback needs macOS".into())
        }
        pub fn time(&self) -> f64 {
            0.0
        }
        pub fn playing(&self) -> bool {
            false
        }
        pub fn play(&self, _rate: f32) {}
        pub fn pause(&self) {}
        pub fn seek(&self, _t: f64) {}
        pub fn frame(&self) -> Option<Frame> {
            None
        }
    }

    pub fn duration(_path: &Path) -> f64 {
        0.0
    }

    pub fn strip(_path: &Path, _n: usize, _height: f64) -> Vec<image::RgbaImage> {
        Vec::new()
    }

    pub fn export(_src: &Path, _voice: Option<&Path>, _edl: &Edl, _dest: PathBuf, done: Reply<PathBuf>) {
        done(Err("Saving video needs macOS".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_rows_lose_their_padding() {
        // 2×1 with 4 bytes of padding
        let src = [1, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9];
        assert_eq!(bgra_to_rgba(&src, 2, 1, 12), vec![3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn edits_are_written_beside_the_file() {
        let p = Path::new("/x/Recording 1.mp4");
        assert_eq!(temp_beside(p), Path::new("/x/.Recording 1.mp4.edit.mp4"));
    }
}
