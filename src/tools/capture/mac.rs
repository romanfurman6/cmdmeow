//! macOS-only pieces of Capture: the Screen Recording permission, putting
//! images on the pasteboard, text recognition with the Vision framework and
//! lifting our overlay windows above the menu bar. Elsewhere these are no-ops,
//! so the tool still builds and its tests run on Linux.

use std::path::Path;

/// One line of recognized text; `rect` is in the image's pixels, top-left origin.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub text: String,
    pub rect: egui::Rect,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::TextLine;
    use objc2::rc::Retained;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF, NSScreen, NSWindowCollectionBehavior};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString, NSURL};
    use std::path::Path;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn can_record_screen() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub fn ask_to_record_screen() {
        unsafe {
            CGRequestScreenCaptureAccess();
        }
    }

    #[allow(unused_unsafe)]
    pub fn copy_png(png: &[u8]) -> bool {
        let data = NSData::with_bytes(png);
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let mut ok = pb.setData_forType(Some(&data), NSPasteboardTypePNG);
            // TIFF too: older apps only read that
            if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
                if let Some(tiff) = img.TIFFRepresentation() {
                    ok |= pb.setData_forType(Some(&tiff), NSPasteboardTypeTIFF);
                }
            }
            ok
        }
    }

    #[allow(unused_unsafe)]
    pub fn copy_text(text: &str) -> bool {
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString)
        }
    }

    /// Recognize text in an image file with Vision (on this Mac, offline).
    #[allow(unused_unsafe)]
    /// `fast`: Vision's quick pass, good for where text is (blur and erase
    /// only need its boxes), rougher on the words themselves.
    pub fn recognize(path: &Path, w: f32, h: f32, fast: bool) -> Vec<TextLine> {
        use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
        let Some(path) = path.to_str() else { return Vec::new() };
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        unsafe {
            let handler = VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new());
            let req = VNRecognizeTextRequest::new();
            req.setRecognitionLevel(if fast { VNRequestTextRecognitionLevel::Fast } else { VNRequestTextRecognitionLevel::Accurate });
            req.setUsesLanguageCorrection(!fast);
            req.setAutomaticallyDetectsLanguage(!fast);
            let as_req: Retained<VNRequest> = Retained::into_super(Retained::into_super(req.clone()));
            let list = NSArray::from_retained_slice(&[as_req]);
            if handler.performRequests_error(&list).is_err() {
                return Vec::new();
            }
            let Some(results) = req.results() else { return Vec::new() };
            let mut out = Vec::new();
            for obs in results.iter() {
                let Some(best) = obs.topCandidates(1).firstObject() else { continue };
                let text = best.string().to_string();
                // normalized, bottom-left origin → pixels, top-left origin
                let b = obs.boundingBox();
                let x0 = b.origin.x as f32 * w;
                let x1 = (b.origin.x + b.size.width) as f32 * w;
                let y0 = (1.0 - (b.origin.y + b.size.height) as f32) * h;
                let y1 = (1.0 - b.origin.y as f32) * h;
                out.push(TextLine { text, rect: egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)) });
            }
            out
        }
    }

    /// Put our window titled `title` above the menu bar and on every Space,
    /// optionally covering the whole main screen.
    pub fn float_window(title: &str, cover_screen: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        for w in app.windows().iter() {
            if w.title().to_string() != title {
                continue;
            }
            // kCGScreenSaverWindowLevel for the full-screen overlay, the
            // status-bar level for small floating windows
            w.setLevel(if cover_screen { 1000 } else { 25 });
            w.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
            if cover_screen {
                if let Some(screen) = NSScreen::mainScreen(mtm) {
                    w.setFrame_display(screen.frame(), true);
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::TextLine;
    use std::path::Path;
    pub fn can_record_screen() -> bool {
        true
    }
    pub fn ask_to_record_screen() {}
    pub fn copy_png(_png: &[u8]) -> bool {
        false
    }
    pub fn copy_text(_text: &str) -> bool {
        false
    }
    pub fn recognize(_path: &Path, _w: f32, _h: f32, _fast: bool) -> Vec<TextLine> {
        Vec::new()
    }
    pub fn float_window(_title: &str, _cover_screen: bool) {}
}

pub use imp::{ask_to_record_screen, can_record_screen, copy_png, copy_text, float_window};

/// Recognize text in an image (written to a temp file for Vision), lines in
/// reading order.
static OCR_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static OCR_WARMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What to tell the user while text recognition runs. The first run of the
/// process loads Vision's model, which takes a few seconds.
pub fn ocr_status() -> &'static str {
    if OCR_READY.load(std::sync::atomic::Ordering::Relaxed) {
        "Recognizing text…"
    } else {
        "Preparing text recognition (first time only)…"
    }
}

/// Load the OCR model in the background so the first Copy text is quick.
pub fn warm_ocr() {
    use std::sync::atomic::Ordering;
    if OCR_READY.load(Ordering::Relaxed) || OCR_WARMING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let img = image::RgbaImage::from_pixel(64, 24, image::Rgba([255, 255, 255, 255]));
        let _ = recognize_image(&img);
    });
}

pub fn recognize_image(img: &image::RgbaImage) -> Vec<TextLine> {
    let lines = recognize_image_inner(img, false);
    OCR_READY.store(true, std::sync::atomic::Ordering::Relaxed);
    lines
}

/// Where the text is in `img`, quickly (for blurring or erasing only text).
pub fn text_boxes(img: &image::RgbaImage) -> Vec<TextLine> {
    recognize_image_inner(img, true)
}

fn recognize_image_inner(img: &image::RgbaImage, fast: bool) -> Vec<TextLine> {
    let path = std::env::temp_dir().join(format!("cmdmeow-ocr-{}-{}.png", std::process::id(), rand_id()));
    if std::fs::write(&path, super::store::png_bytes(img)).is_err() {
        return Vec::new();
    }
    let lines = recognize_path(&path, fast);
    let _ = std::fs::remove_file(&path);
    lines
}

/// Recognize text in an image file as it is (no decoding or re-encoding),
/// lines in reading order.
pub fn recognize_file(path: &Path) -> Vec<TextLine> {
    recognize_path(path, false)
}

fn recognize_path(path: &Path, fast: bool) -> Vec<TextLine> {
    let Ok((w, h)) = image::image_dimensions(path) else { return Vec::new() };
    let mut lines = imp::recognize(path, w as f32, h as f32, fast);
    sort_reading_order(&mut lines);
    OCR_READY.store(true, std::sync::atomic::Ordering::Relaxed);
    lines
}

/// Top to bottom, then left to right for lines on the same row.
pub fn sort_reading_order(lines: &mut Vec<TextLine>) {
    lines.sort_by(|a, b| a.rect.center().y.total_cmp(&b.rect.center().y));
    let mut rows: Vec<Vec<TextLine>> = Vec::new();
    for l in lines.drain(..) {
        match rows.last_mut() {
            Some(row) if (l.rect.center().y - row[0].rect.center().y).abs() < row[0].rect.height().min(l.rect.height()) * 0.5 => row.push(l),
            _ => rows.push(vec![l]),
        }
    }
    for mut row in rows {
        row.sort_by(|a, b| a.rect.left().total_cmp(&b.rect.left()));
        lines.extend(row);
    }
}

/// Lines joined with line breaks, or spaces when `keep_breaks` is off.
pub fn join_lines(lines: &[TextLine], keep_breaks: bool) -> String {
    let parts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    parts.join(if keep_breaks { "\n" } else { " " })
}

/// Open System Settings on the Screen Recording list.
pub fn open_privacy_settings() {
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture").spawn();
}

/// Show a file in Finder.
pub fn reveal(path: &Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

/// The system's screenshot sound, if it's there.
pub fn play_shutter() {
    let candidates = [
        "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
        "/System/Library/Sounds/Tink.aiff",
    ];
    if let Some(p) = candidates.iter().find(|p| Path::new(p).exists()) {
        let _ = std::process::Command::new("afplay").arg(p).spawn();
    }
}

fn rand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    fn line(t: &str, x: f32, y: f32) -> TextLine {
        TextLine { text: t.into(), rect: Rect::from_min_max(pos2(x, y), pos2(x + 40.0, y + 10.0)) }
    }

    #[test]
    fn lines_read_top_down_then_left_right() {
        let mut v = vec![line("c", 0.0, 40.0), line("b", 60.0, 1.0), line("a", 0.0, 0.0)];
        sort_reading_order(&mut v);
        assert_eq!(join_lines(&v, true), "a\nb\nc");
        assert_eq!(join_lines(&v, false), "a b c");
    }
}
