//! Taking the picture: macOS's own `screencapture` does the selection (area,
//! window) or grabs the whole main screen. Runs on a thread and reports back
//! through a channel.

use image::RgbaImage;
use std::path::PathBuf;

/// What to grab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grab {
    /// Drag out an area (Space switches to window picking, as in macOS).
    Area,
    /// Click a window.
    Window,
    /// The whole main screen.
    Screen,
}

impl Grab {
    /// `screencapture` arguments before the output path. `-x` keeps it silent:
    /// we play our own sound when the user wants one.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Grab::Area => &["-i", "-x"],
            Grab::Window => &["-i", "-W", "-x"],
            Grab::Screen => &["-x", "-m"],
        }
    }
}

/// A capture as `screencapture` wrote it: the PNG bytes (kept as they are
/// for history, the clipboard and a PNG copy, so nothing is encoded again)
/// and the decoded picture.
pub struct Grabbed {
    pub png: Vec<u8>,
    pub img: RgbaImage,
}

/// Run `screencapture`; the PNG file it wrote. Ok(None) means the user
/// pressed Esc (no file). The caller removes the file.
pub fn shoot(grab: Grab) -> Result<Option<PathBuf>, String> {
    let path = temp_path();
    let status = std::process::Command::new("/usr/sbin/screencapture").args(grab.args()).arg(&path).status().map_err(|e| format!("couldn't start screencapture: {e}"))?;
    if !path.exists() {
        return if status.success() { Ok(None) } else { Err(format!("screencapture failed ({status})")) };
    }
    Ok(Some(path))
}

/// Read and decode what [`shoot`] wrote, then remove the file.
pub fn load(path: &std::path::Path) -> Result<Grabbed, String> {
    let png = std::fs::read(path).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(path);
    let png = png?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Grabbed { png, img })
}

fn temp_path() -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("cmdmeow-grab-{}-{n}.png", std::process::id()))
}

/// A picture of a fake app window, for screenshots of the tool's own UI
/// (`--view capture-editor`) and as a stand-in where there's no screen.
pub fn sample(w: u32, h: u32) -> RgbaImage {
    use image::Rgba;
    let mut img = RgbaImage::from_pixel(w, h, Rgba([242, 242, 244, 255]));
    let fill = |img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 4]| {
        for y in y0.min(h)..y1.min(h) {
            for x in x0.min(w)..x1.min(w) {
                img.put_pixel(x, y, Rgba(c));
            }
        }
    };
    let s = w as f32 / 794.0;
    let px = |v: f32| (v * s) as u32;
    fill(&mut img, 0, 0, w, px(34.0), [227, 227, 231, 255]);
    fill(&mut img, 0, px(34.0), px(170.0), h, [234, 234, 238, 255]);
    for (i, len) in [120.0, 90.0, 105.0, 75.0].iter().enumerate() {
        let y = px(52.0 + i as f32 * 18.0);
        fill(&mut img, px(14.0), y, px(14.0 + len), y + px(8.0), [199, 199, 204, 255]);
    }
    // title, subtitle, two fields, a button
    fill(&mut img, px(210.0), px(64.0), px(420.0), px(84.0), [28, 28, 31, 255]);
    fill(&mut img, px(210.0), px(98.0), px(520.0), px(108.0), [150, 150, 156, 255]);
    for y in [150.0, 220.0] {
        fill(&mut img, px(210.0), px(y), px(260.0), px(y + 8.0), [60, 60, 66, 255]);
        fill(&mut img, px(210.0), px(y + 18.0), px(530.0), px(y + 48.0), [209, 209, 214, 255]);
        fill(&mut img, px(211.0), px(y + 19.0), px(529.0), px(y + 47.0), [255, 255, 255, 255]);
    }
    fill(&mut img, px(220.0), px(176.0), px(380.0), px(186.0), [90, 90, 96, 255]);
    fill(&mut img, px(210.0), px(300.0), px(330.0), px(332.0), [11, 107, 216, 255]);
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grab_is_silent_and_area_window_are_interactive() {
        for g in [Grab::Area, Grab::Window, Grab::Screen] {
            assert!(g.args().contains(&"-x"), "{g:?} must not play the system sound");
        }
        assert!(Grab::Area.args().contains(&"-i"));
        assert!(Grab::Window.args().contains(&"-W"));
        assert!(!Grab::Screen.args().contains(&"-i"));
    }

    #[test]
    fn sample_has_the_requested_size() {
        assert_eq!(sample(400, 225).dimensions(), (400, 225));
    }
}
