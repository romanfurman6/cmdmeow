//! Saving a video with a frame ([`scene`]: the picture on a backdrop).
//!
//! Recordings are written finished (the cursor and the camera are in the
//! picture), so a plain Save only cuts the file, without re-encoding
//! ([`super::media::export`]). A frame needs every frame drawn again: that
//! runs on the GPU (Core Image) when the video is saved, never after
//! recording.
//!
//! A framed video keeps its unframed picture in a project (raw.mp4 and
//! meta.json in the projects folder), so the frame can be changed or taken
//! off later without framing a frame.

use super::scene::Style;
use crate::tools::capture::render::Frame;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};

/// Projects of older versions (a cursor drawn on render, the camera on a
/// track of its own) can't be used any more and are pruned.
pub const VERSION: u32 = 2;

/// What a project keeps beside its unframed picture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub version: u32,
    /// The framed file (in the Capture folder).
    pub out: PathBuf,
    /// The frame it was saved with.
    pub style: Style,
}

/// A framed video's project folder: raw.mp4 and meta.json.
pub struct Project {
    pub dir: PathBuf,
}

/// FNV-1a: a stable short name for a path (std's hasher may change).
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

impl Project {
    /// The folder for the framed file `out` (two files with one name in
    /// different folders get different projects).
    pub fn for_output(root: &Path, out: &Path) -> Project {
        let stem = out.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Video".into());
        Project { dir: root.join(format!("{stem} {:016x}", fnv(out.as_os_str().as_encoded_bytes()))) }
    }

    /// The project of a framed file, if it has one this version can use.
    pub fn find(root: &Path, out: &Path) -> Option<(Project, Meta)> {
        let p = Project::for_output(root, out);
        let meta = p.meta()?;
        (meta.version == VERSION && meta.out == out && p.raw().exists()).then_some((p, meta))
    }

    pub fn raw(&self) -> PathBuf {
        self.dir.join("raw.mp4")
    }

    /// Where a new unframed picture waits until the framed file is in place.
    fn staged(&self) -> PathBuf {
        self.dir.join("raw.next.mp4")
    }

    pub fn meta(&self) -> Option<Meta> {
        let s = std::fs::read_to_string(self.dir.join("meta.json")).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn save(&self, meta: &Meta) -> bool {
        std::fs::create_dir_all(&self.dir).is_ok() && crate::kit::fs::save_json(&self.dir.join("meta.json"), meta, true)
    }

    pub fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Seconds since `p` last changed.
fn age(p: &Path) -> Option<u64> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()?.elapsed().ok().map(|d| d.as_secs())
}

/// Removes projects whose framed file is gone (deleted from the folder or
/// History) or that an older version wrote, and scratch files a crash left.
/// Returns how many.
pub fn prune(root: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(root) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let path = e.path();
        // anything half-made is left a day, in case a save is running now
        let old = || age(&path).is_some_and(|s| s > 86_400);
        if !path.is_dir() {
            if old() && std::fs::remove_file(&path).is_ok() {
                n += 1;
            }
            continue;
        }
        let p = Project { dir: path.clone() };
        let gone = match p.meta() {
            Some(m) => m.version != VERSION || !m.out.exists() || !p.raw().exists(),
            None => old(),
        };
        if gone && std::fs::remove_dir_all(&p.dir).is_ok() {
            n += 1;
        }
    }
    n
}

/// A file for a cut of the unframed picture, in the projects folder (so it
/// moves into a project without copying).
pub fn scratch(root: &Path) -> PathBuf {
    let _ = std::fs::create_dir_all(root);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    root.join(format!(".cut {} {nanos}.mp4", std::process::id()))
}

/// Move a file, copying when it goes to another disk.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Put a Save over `path` in place: `written` (beside it) becomes the file,
/// and its project follows the frame: gone without one; with one, it keeps
/// the unframed picture: `cut` when cuts or the voice made a new one,
/// otherwise the one it had, or (framed for the first time) the file as it
/// was. The file is only replaced once its picture is safe; a project that
/// can't be written is dropped (the framed file stands on its own).
pub fn commit_over(root: &Path, path: &Path, written: &Path, cut: Option<&Path>, style: Style) -> Result<(), String> {
    let p = Project::for_output(root, path);
    let had = Project::find(root, path).is_some();
    let fail = |why: String, staged: Option<&Path>| -> Result<(), String> {
        let _ = std::fs::remove_file(written);
        if let Some(c) = cut {
            let _ = std::fs::remove_file(c);
        }
        if let Some(s) = staged {
            let _ = std::fs::remove_file(s);
        }
        if !had {
            p.remove();
        }
        Err(why)
    };
    if style.frame.is_none() {
        if let Err(e) = std::fs::rename(written, path) {
            return fail(e.to_string(), None);
        }
        if let Some(c) = cut {
            let _ = std::fs::remove_file(c);
        }
        p.remove();
        return Ok(());
    }
    let staged = p.staged();
    let stage = match cut {
        Some(c) => Some(std::fs::create_dir_all(&p.dir).and_then(|_| move_file(c, &staged))),
        None if !had => Some(std::fs::create_dir_all(&p.dir).and_then(|_| std::fs::copy(path, &staged).map(|_| ()))),
        None => None,
    };
    if let Some(Err(e)) = stage {
        return fail(format!("Couldn't keep the unframed video: {e}"), Some(&staged));
    }
    if let Err(e) = std::fs::rename(written, path) {
        return fail(e.to_string(), stage.is_some().then_some(staged.as_path()));
    }
    let placed = stage.is_none() || std::fs::rename(&staged, p.raw()).is_ok();
    if !(placed && p.save(&Meta { version: VERSION, out: path.to_path_buf(), style })) {
        p.remove();
    }
    Ok(())
}

/// One framed save.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Job {
    /// The unframed video.
    pub src: PathBuf,
    /// Where the framed one goes.
    pub out: PathBuf,
    pub frame: Frame,
}

/// How far a render is (per mille), and a way to stop it.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU32,
    pub cancel: AtomicBool,
}

#[cfg(target_os = "macos")]
pub use imp::render;

/// Render `job`: blocks (run it on its own thread). Gives the video's size.
#[cfg(not(target_os = "macos"))]
pub fn render(_job: &Job, _progress: &Progress) -> Result<(u32, u32), String> {
    Err("Saving a framed video needs macOS".into())
}

#[cfg(target_os = "macos")]
mod imp {
    use super::super::scene;
    use super::{Job, Progress};
    use crate::tools::capture::record::timeline;
    use crate::tools::capture::settings::RecQuality;
    use block2::RcBlock;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_av_foundation::{
        AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo,
        AVURLAsset,
    };
    use objc2_core_foundation::{CGAffineTransform, CGPoint, CGRect, CGSize};
    use objc2_core_image::{kCIContextCacheIntermediates, kCIContextOutputColorSpace, kCIContextWorkingColorSpace, kCIImageColorSpace, CIContext, CIImage, CIRenderDestination};
    use objc2_core_media::{CMFormatDescription, CMTime, CMTimeFlags};
    use objc2_core_video::CVPixelBuffer;
    use objc2_foundation::{NSDictionary, NSError, NSNull, NSNumber, NSString, NSURL};
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;

    /// CMTime as the C functions return it.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RawTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetPresentationTimeStamp(sb: *const c_void) -> RawTime;
        fn CMSampleBufferGetImageBuffer(sb: *const c_void) -> *mut c_void;
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVPixelBufferCreate(alloc: *const c_void, w: usize, h: usize, format: u32, attrs: *const c_void, out: *mut *mut c_void) -> i32;
        fn CVPixelBufferLockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(pb: *mut c_void) -> *const u8;
        fn CVPixelBufferGetBytesPerRow(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetWidth(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(pb: *mut c_void) -> usize;
        fn CVPixelBufferPoolCreatePixelBuffer(alloc: *const c_void, pool: *const c_void, out: *mut *mut c_void) -> i32;
        fn CVBufferPropagateAttachments(src: *mut c_void, dst: *mut c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(p: *const c_void);
    }

    const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
    /// What the decoder makes: no conversion on the way in.
    const YUV: u32 = u32::from_be_bytes(*b"420v");

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

    fn ns_err(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    fn any<T: objc2::Message>(o: &T) -> &AnyObject {
        unsafe { &*(o as *const T as *const AnyObject) }
    }

    fn dict(pairs: &[(&str, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let vals: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
        NSDictionary::from_slices(&key_refs, &vals)
    }

    /// A pixel buffer the GPU can draw from and into (IOSurface-backed).
    fn gpu_attrs(format: u32, size: Option<(u32, u32)>) -> Retained<NSDictionary<NSString, AnyObject>> {
        let fmt = NSNumber::new_u32(format);
        let surface = dict(&[]);
        let metal = NSNumber::new_bool(true);
        match size {
            Some((w, h)) => {
                let (w, h) = (NSNumber::new_u32(w), NSNumber::new_u32(h));
                dict(&[("PixelFormatType", any(&*fmt)), ("Width", any(&*w)), ("Height", any(&*h)), ("IOSurfaceProperties", any(&*surface)), ("MetalCompatibility", any(&*metal))])
            }
            None => dict(&[("PixelFormatType", any(&*fmt)), ("IOSurfaceProperties", any(&*surface)), ("MetalCompatibility", any(&*metal))]),
        }
    }

    /// A still part of the frame as an image the GPU reads (a pixel buffer,
    /// so it stands the same way up as the video's frames).
    struct Still {
        pb: *mut c_void,
        img: Retained<CIImage>,
    }

    impl Drop for Still {
        fn drop(&mut self) {
            unsafe { CFRelease(self.pb) };
        }
    }

    unsafe fn still(pm: &tiny_skia::Pixmap, opts: &NSDictionary<NSString, AnyObject>) -> Option<Still> {
        unsafe {
            let (w, h) = (pm.width() as usize, pm.height() as usize);
            let attrs = gpu_attrs(BGRA, None);
            let mut pb: *mut c_void = std::ptr::null_mut();
            if CVPixelBufferCreate(std::ptr::null(), w, h, BGRA, Retained::as_ptr(&attrs) as *const c_void, &mut pb) != 0 || pb.is_null() {
                return None;
            }
            if CVPixelBufferLockBaseAddress(pb, 0) != 0 {
                CFRelease(pb);
                return None;
            }
            let (row, base) = (CVPixelBufferGetBytesPerRow(pb), CVPixelBufferGetBaseAddress(pb));
            if !base.is_null() {
                // locked for writing: ours until unlocked
                let dst = std::slice::from_raw_parts_mut(base as *mut u8, row * h);
                for (y, src) in pm.data().chunks_exact(w * 4).enumerate() {
                    dst[y * row..y * row + w * 4].copy_from_slice(src);
                }
            }
            CVPixelBufferUnlockBaseAddress(pb, 0);
            let img = CIImage::imageWithCVPixelBuffer_options(&*(pb as *const CVPixelBuffer), Some(opts));
            Some(Still { pb, img })
        }
    }

    /// Sound copied across untouched.
    struct Lane {
        out: Retained<AVAssetReaderTrackOutput>,
        input: Retained<AVAssetWriterInput>,
        done: bool,
    }

    pub fn render(job: &Job, progress: &Progress) -> Result<(u32, u32), String> {
        let t0 = std::time::Instant::now();
        let r = unsafe { run(job, progress) };
        match &r {
            Ok((size, frames, dur)) => eprintln!("[video] framed {:?} → {:?}: {frames} frames {size:?}, {dur:.1} s of video in {:.1} s", job.src, job.out, t0.elapsed().as_secs_f32()),
            Err(e) => eprintln!("[video] framing {:?} failed after {:.1} s: {e}", job.src, t0.elapsed().as_secs_f32()),
        }
        if r.is_err() {
            let _ = std::fs::remove_file(&job.out);
        }
        r.map(|(size, _, _)| size)
    }

    #[allow(deprecated)]
    unsafe fn run(job: &Job, progress: &Progress) -> Result<((u32, u32), u64, f64), String> {
        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&NSURL::fileURLWithPath(&NSString::from_str(&job.src.to_string_lossy())), None);
            let (vm, am) = (AVMediaTypeVideo.ok_or("no video")?, AVMediaTypeAudio.ok_or("no audio")?);
            let duration = secs(asset.duration());
            if duration <= 0.0 {
                return Err("The video is empty".into());
            }
            let track: Retained<AVAssetTrack> = asset.tracksWithMediaType(vm).firstObject().ok_or("No picture in the video")?;
            let ns = track.naturalSize();
            let src = ((ns.width.round() as u32).max(2), (ns.height.round() as u32).max(2));
            let lay = scene::layout(src, Some(&job.frame));
            let (ow, oh) = lay.out;
            let chrome = scene::chrome(&job.frame, &lay).ok_or("The frame couldn't be drawn")?;
            // a screen recording sends frames only when something moved:
            // the rate is a hint for the encoder, the frames keep their times
            let fps = if track.nominalFrameRate() > 31.0 { 60 } else { 30 };

            // the GPU, without colour conversions: pixels go out as they came in
            let null = NSNull::null();
            let no = NSNumber::new_bool(false);
            let ctx_opts = NSDictionary::from_slices(&[kCIContextWorkingColorSpace, kCIContextOutputColorSpace, kCIContextCacheIntermediates], &[any(&*null), any(&*null), any(&*no)]);
            let ctx = CIContext::contextWithOptions(Some(&ctx_opts));
            let img_opts = NSDictionary::from_slices(&[kCIImageColorSpace], &[any(&*null)]);
            let under = still(&chrome.under, &img_opts).ok_or("No memory for the frame")?;
            let over = still(&chrome.over, &img_opts).ok_or("No memory for the frame")?;
            // the picture's place, in Core Image's space (y up from the bottom)
            let s = lay.screen;
            let hole = CGRect::new(CGPoint::new(s.left() as f64, (oh as f32 - s.bottom()) as f64), CGSize::new(s.width() as f64, s.height() as f64));

            // reading
            let reader = AVAssetReader::assetReaderWithAsset_error(&asset).map_err(|e| ns_err(&e))?;
            let video_out = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&gpu_attrs(YUV, None)));
            video_out.setAlwaysCopiesSampleData(false);
            if !reader.canAddOutput(&video_out) {
                return Err("The video can't be read".into());
            }
            reader.addOutput(&video_out);

            // writing
            let _ = std::fs::remove_file(&job.out);
            let url = NSURL::fileURLWithPath(&NSString::from_str(&job.out.to_string_lossy()));
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, AVFileTypeMPEG4.ok_or("mp4 isn't available")?).map_err(|e| ns_err(&e))?;
            let num = |v: f64| NSNumber::new_f64(v);
            let (bitrate, nfps, keys) = (num(timeline::bitrate(ow, oh, fps, RecQuality::High) as f64), num(fps as f64), num((fps * 2) as f64));
            let profile = NSString::from_str("H264_High_AutoLevel");
            let compression = dict(&[("AverageBitRate", any(&*bitrate)), ("ExpectedFrameRate", any(&*nfps)), ("MaxKeyFrameInterval", any(&*keys)), ("ProfileLevel", any(&*profile)), ("AllowFrameReordering", any(&*no))]);
            let (w, h, codec) = (num(ow as f64), num(oh as f64), NSString::from_str("avc1"));
            let vset = dict(&[("AVVideoCodecKey", any(&*codec)), ("AVVideoWidthKey", any(&*w)), ("AVVideoHeightKey", any(&*h)), ("AVVideoCompressionPropertiesKey", any(&*compression))]);
            let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(vm, Some(&vset));
            video.setExpectsMediaDataInRealTime(false);
            if !writer.canAddInput(&video) {
                return Err("This Mac can't encode video at that size".into());
            }
            writer.addInput(&video);
            let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, Some(&gpu_attrs(BGRA, Some((ow, oh)))));

            let mut lanes = Vec::new();
            for t in asset.tracksWithMediaType(am).iter() {
                let o = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&t, None);
                let descs = t.formatDescriptions();
                let first = descs.firstObject();
                // the array holds CMFormatDescriptions
                let hint = first.as_ref().map(|d| &*(Retained::as_ptr(d) as *const CMFormatDescription));
                let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings_sourceFormatHint(am, None, hint);
                input.setExpectsMediaDataInRealTime(false);
                input.setMarksOutputTrackAsEnabled(t.isEnabled());
                if reader.canAddOutput(&o) && writer.canAddInput(&input) {
                    reader.addOutput(&o);
                    writer.addInput(&input);
                    lanes.push(Lane { out: o, input, done: false });
                }
            }

            if !reader.startReading() {
                return Err(reader.error().map(|e| ns_err(&e)).unwrap_or_else(|| "The video can't be read".into()));
            }
            if !writer.startWriting() {
                reader.cancelReading();
                return Err(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "Couldn't start the video file".into()));
            }
            writer.startSessionAtSourceTime(cm(0.0));
            let pool = adaptor.pixelBufferPool().ok_or("No frames to draw into")?;
            let pool_ptr = Retained::as_ptr(&pool) as *const c_void;

            let fail = |why: String| -> Result<((u32, u32), u64, f64), String> {
                reader.cancelReading();
                writer.cancelWriting();
                Err(why)
            };
            let mut frames = 0u64;
            let mut last: Option<f64> = None;
            let mut video_done = false;
            while !(video_done && lanes.iter().all(|l| l.done)) {
                if progress.cancel.load(Ordering::Relaxed) {
                    return fail("Cancelled".into());
                }
                if writer.status() == AVAssetWriterStatus::Failed {
                    return fail(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "The video couldn't be written".into()));
                }
                if reader.status() == AVAssetReaderStatus::Failed {
                    return fail(reader.error().map(|e| ns_err(&e)).unwrap_or_else(|| "The video couldn't be read".into()));
                }
                let mut moved = false;
                if !video_done && video.isReadyForMoreMediaData() {
                    moved = true;
                    // one frame: its objects go when it's written
                    let step = autoreleasepool(|_| -> Result<(), String> {
                        let Some(sb) = video_out.copyNextSampleBuffer() else {
                            video.markAsFinished();
                            video_done = true;
                            return Ok(());
                        };
                        let raw = &*sb as *const _ as *const c_void;
                        let pic_pb = CMSampleBufferGetImageBuffer(raw);
                        if pic_pb.is_null() {
                            return Ok(());
                        }
                        let pts = CMSampleBufferGetPresentationTimeStamp(raw);
                        let t = if pts.flags & 1 == 1 && pts.timescale > 0 { pts.value as f64 / pts.timescale as f64 } else { return Ok(()) };
                        if last.is_some_and(|l| t <= l) {
                            return Ok(());
                        }
                        let (pw, ph) = (CVPixelBufferGetWidth(pic_pb).max(1) as f64, CVPixelBufferGetHeight(pic_pb).max(1) as f64);
                        let pic = CIImage::imageWithCVPixelBuffer_options(&*(pic_pb as *const CVPixelBuffer), Some(&img_opts));
                        let fit = CGAffineTransform { a: hole.size.width / pw, b: 0.0, c: 0.0, d: hole.size.height / ph, tx: hole.origin.x, ty: hole.origin.y };
                        // clamped first, so the picture's edge pixels are whole (no seam)
                        let pic = pic.imageByClampingToExtent().imageByApplyingTransform_highQualityDownsample(fit, true).imageByCroppingToRect(hole);
                        let frame = over.img.imageByCompositingOverImage(&pic.imageByCompositingOverImage(&under.img));
                        let mut out: *mut c_void = std::ptr::null_mut();
                        if CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), pool_ptr, &mut out) != 0 || out.is_null() {
                            return Err("Out of memory for frames".into());
                        }
                        let out_ref = &*(out as *const CVPixelBuffer);
                        let dest = CIRenderDestination::initWithPixelBuffer(CIRenderDestination::alloc(), out_ref);
                        dest.setColorSpace(None);
                        let drawn = ctx.startTaskToRender_toDestination_error(&frame, &dest).and_then(|task| task.waitUntilCompletedAndReturnError());
                        if let Err(e) = drawn {
                            CFRelease(out);
                            return Err(format!("The frame couldn't be drawn: {}", ns_err(&e)));
                        }
                        // the same colour tags as the recording
                        CVBufferPropagateAttachments(pic_pb, out);
                        // the first frame shows from the start, whenever it came
                        let at = if last.is_none() { cm(0.0) } else { CMTime { value: pts.value, timescale: pts.timescale, flags: CMTimeFlags(pts.flags), epoch: pts.epoch } };
                        let ok = adaptor.appendPixelBuffer_withPresentationTime(out_ref, at);
                        CFRelease(out);
                        if !ok {
                            return Err(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "A frame couldn't be written".into()));
                        }
                        last = Some(t);
                        frames += 1;
                        progress.done.store(((t / duration) * 1000.0).clamp(0.0, 999.0) as u32, Ordering::Relaxed);
                        Ok(())
                    });
                    if let Err(e) = step {
                        return fail(e);
                    }
                }
                for l in lanes.iter_mut().filter(|l| !l.done) {
                    while l.input.isReadyForMoreMediaData() {
                        moved = true;
                        match l.out.copyNextSampleBuffer() {
                            Some(sb) => {
                                if !l.input.appendSampleBuffer(&sb) {
                                    return fail(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "The sound couldn't be written".into()));
                                }
                            }
                            None => {
                                l.input.markAsFinished();
                                l.done = true;
                                break;
                            }
                        }
                    }
                }
                if !moved {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            if frames == 0 {
                return fail("No picture in the video".into());
            }
            reader.cancelReading();
            writer.endSessionAtSourceTime(cm(duration));
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let tx = std::sync::Mutex::new(Some(tx));
            let block = RcBlock::new(move || {
                if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                    let _ = tx.send(());
                }
            });
            writer.finishWritingWithCompletionHandler(&block);
            let _ = rx.recv_timeout(std::time::Duration::from_secs(120));
            progress.done.store(1000, Ordering::Relaxed);
            if writer.status() == AVAssetWriterStatus::Completed {
                Ok(((ow, oh), frames, duration))
            } else {
                Err(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "The video couldn't be finished".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let d = std::env::temp_dir().join(format!("cmdmeow-proj-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(d.join("out")).unwrap();
            Tmp(d)
        }
        fn root(&self) -> PathBuf {
            self.0.join("projects")
        }
        fn video(&self, name: &str, body: &str) -> PathBuf {
            let p = self.0.join("out").join(name);
            std::fs::write(&p, body).unwrap();
            p
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn framed() -> Style {
        Style { frame: Some(Frame::default()) }
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn projects_are_found_by_their_file_and_version() {
        let t = Tmp::new("find");
        let out = t.video("Recording 1.mp4", "framed");
        let p = Project::for_output(&t.root(), &out);
        assert!(p.save(&Meta { version: VERSION, out: out.clone(), style: framed() }));
        std::fs::write(p.raw(), b"raw").unwrap();
        let (found, meta) = Project::find(&t.root(), &out).expect("found");
        assert_eq!(found.dir, p.dir);
        assert_eq!(meta.style, framed());
        assert!(Project::find(&t.root(), &t.0.join("other.mp4")).is_none());
        // the same name in another folder is another project
        assert_ne!(Project::for_output(&t.root(), &t.0.join("Recording 1.mp4")).dir, p.dir);
        // an older version's project isn't used
        assert!(p.save(&Meta { version: 1, out: out.clone(), style: framed() }));
        assert!(Project::find(&t.root(), &out).is_none());
    }

    #[test]
    fn old_and_orphaned_projects_are_pruned() {
        let t = Tmp::new("prune");
        let keep = t.video("Keep.mp4", "x");
        let gone = t.video("Gone.mp4", "x");
        let old = t.video("Old.mp4", "x");
        for (out, version) in [(&keep, VERSION), (&gone, VERSION), (&old, 1)] {
            let p = Project::for_output(&t.root(), out);
            assert!(p.save(&Meta { version, out: out.clone(), style: framed() }));
            std::fs::write(p.raw(), b"raw").unwrap();
        }
        std::fs::remove_file(&gone).unwrap();
        // a save running now: its scratch and fresh folder stay
        std::fs::write(scratch(&t.root()), b"cut").unwrap();
        std::fs::create_dir_all(t.root().join("Busy 0")).unwrap();
        assert_eq!(prune(&t.root()), 2);
        assert!(Project::find(&t.root(), &keep).is_some());
        assert_eq!(std::fs::read_dir(t.root()).unwrap().count(), 3);
    }

    #[test]
    fn framing_the_first_time_keeps_the_file_as_it_was() {
        let t = Tmp::new("first");
        let path = t.video("Rec.mp4", "plain");
        let written = t.video(".Rec.mp4.edit.mp4", "framed");
        commit_over(&t.root(), &path, &written, None, framed()).unwrap();
        assert_eq!(read(&path), "framed");
        assert!(!written.exists());
        let (p, meta) = Project::find(&t.root(), &path).expect("a project");
        assert_eq!(read(&p.raw()), "plain");
        assert_eq!(meta.style, framed());
    }

    #[test]
    fn a_cut_becomes_the_unframed_picture() {
        let t = Tmp::new("cut");
        let path = t.video("Rec.mp4", "plain");
        let cut = scratch(&t.root());
        std::fs::write(&cut, "plain, cut").unwrap();
        let written = t.video(".Rec.mp4.edit.mp4", "framed, cut");
        commit_over(&t.root(), &path, &written, Some(&cut), framed()).unwrap();
        let (p, _) = Project::find(&t.root(), &path).expect("a project");
        assert_eq!(read(&p.raw()), "plain, cut");
        assert_eq!(read(&path), "framed, cut");
        assert!(!cut.exists());
        // a new frame on the same picture keeps it
        let other = Style { frame: Some(Frame { bg: 3, ..Frame::default() }) };
        let again = t.video(".Rec.mp4.edit.mp4", "framed 2");
        commit_over(&t.root(), &path, &again, None, other).unwrap();
        let (p, meta) = Project::find(&t.root(), &path).expect("still a project");
        assert_eq!((read(&p.raw()).as_str(), read(&path).as_str(), meta.style), ("plain, cut", "framed 2", other));
    }

    #[test]
    fn taking_the_frame_off_drops_the_project() {
        let t = Tmp::new("off");
        let path = t.video("Rec.mp4", "plain");
        commit_over(&t.root(), &path, &t.video(".w.mp4", "framed"), None, framed()).unwrap();
        let (p, _) = Project::find(&t.root(), &path).unwrap();
        commit_over(&t.root(), &path, &t.video(".w.mp4", "plain again"), None, Style::default()).unwrap();
        assert_eq!(read(&path), "plain again");
        assert!(!p.dir.exists());
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        let t = Tmp::new("fail");
        let path = t.video("Rec.mp4", "plain");
        // the written file isn't there: the rename fails
        let missing = t.0.join("out").join(".nothing.mp4");
        let cut = scratch(&t.root());
        std::fs::write(&cut, "cut").unwrap();
        assert!(commit_over(&t.root(), &path, &missing, Some(&cut), framed()).is_err());
        assert_eq!(read(&path), "plain");
        assert!(Project::find(&t.root(), &path).is_none());
        assert!(!Project::for_output(&t.root(), &path).dir.exists(), "no half-made project");
        assert!(!cut.exists());
    }
}
