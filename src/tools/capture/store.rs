//! Capture history: every capture is kept as a PNG in the app's data folder
//! with a small JSON index, newest first, trimmed to the user's limit. Also
//! saving a copy to the user's folder in their format.

use super::settings::{Format, Settings};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Area,
    Window,
    Screen,
    Edited,
    /// A screen recording: the entry's picture is a frame from it, `saved`
    /// is the mp4.
    Recording,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    /// Unique, also the file stem: milliseconds since the epoch.
    pub id: u64,
    pub kind: Kind,
    pub w: u32,
    pub h: u32,
    /// Where the user's copy was saved, if it was.
    #[serde(default)]
    pub saved: Option<PathBuf>,
    #[serde(default)]
    pub pinned: bool,
    /// A recording's length in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secs: Option<f32>,
}

impl Shot {
    pub fn time(&self) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local.timestamp_millis_opt(self.id as i64).single().unwrap_or_else(chrono::Local::now)
    }
}

pub struct Store {
    dir: PathBuf,
    pub shots: Vec<Shot>,
    /// [`Store::bytes_on_disk`] for the list it was measured for.
    disk: std::cell::Cell<Option<((usize, u64), u64)>>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        let shots = std::fs::read_to_string(dir.join("index.json")).ok().and_then(|s| serde_json::from_str::<Vec<Shot>>(&s).ok()).unwrap_or_default();
        let shots = shots.into_iter().filter(|s| dir.join(format!("{}.png", s.id)).exists()).collect();
        Store { dir, shots, disk: Default::default() }
    }

    /// A capture's PNG file as it is (no decoding).
    pub fn png(&self, id: u64) -> Option<Vec<u8>> {
        std::fs::read(self.path(id)).ok()
    }

    pub fn path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{id}.png"))
    }

    pub fn get(&self, id: u64) -> Option<&Shot> {
        self.shots.iter().find(|s| s.id == id)
    }

    /// Add a capture and trim to `keep`. Returns its id.
    pub fn add(&mut self, img: &RgbaImage, kind: Kind, keep: usize) -> Result<u64, String> {
        self.add_png(&png_bytes(img), img.dimensions(), kind, keep)
    }

    /// [`Store::add`] for a picture already encoded as PNG (`size` in pixels).
    pub fn add_png(&mut self, png: &[u8], size: (u32, u32), kind: Kind, keep: usize) -> Result<u64, String> {
        let _ = std::fs::create_dir_all(&self.dir);
        let mut id = chrono::Local::now().timestamp_millis().max(0) as u64;
        while self.shots.iter().any(|s| s.id == id) {
            id += 1;
        }
        std::fs::write(self.path(id), png).map_err(|e| e.to_string())?;
        self.shots.insert(0, Shot { id, kind, w: size.0, h: size.1, saved: None, pinned: false, secs: None });
        self.trim(keep);
        self.write_index();
        Ok(id)
    }

    /// Drop the oldest captures beyond `keep` (pinned ones count but stay).
    pub fn trim(&mut self, keep: usize) {
        let mut n = 0;
        let mut gone = Vec::new();
        self.shots.retain(|s| {
            n += 1;
            if n <= keep || s.pinned {
                true
            } else {
                gone.push(s.id);
                false
            }
        });
        for id in gone {
            let _ = std::fs::remove_file(self.path(id));
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.shots.retain(|s| s.id != id);
        let _ = std::fs::remove_file(self.path(id));
        self.write_index();
    }

    pub fn clear(&mut self) {
        for s in std::mem::take(&mut self.shots) {
            let _ = std::fs::remove_file(self.path(s.id));
        }
        self.write_index();
    }

    pub fn set_saved(&mut self, id: u64, path: PathBuf) {
        if let Some(s) = self.shots.iter_mut().find(|s| s.id == id) {
            s.saved = Some(path);
        }
        self.write_index();
    }

    /// Mark an entry as a recording of `secs` saved at `video`.
    pub fn set_video(&mut self, id: u64, video: PathBuf, secs: f32) {
        if let Some(s) = self.shots.iter_mut().find(|s| s.id == id) {
            s.saved = Some(video);
            s.secs = Some(secs);
        }
        self.write_index();
    }

    /// The history entry of the video at `path`, if there is one.
    pub fn video_entry(&self, path: &Path) -> Option<&Shot> {
        self.shots.iter().find(|s| s.kind == Kind::Recording && s.saved.as_deref() == Some(path))
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) {
        if let Some(s) = self.shots.iter_mut().find(|s| s.id == id) {
            s.pinned = pinned;
        }
        self.write_index();
    }

    pub fn load(&self, id: u64) -> Option<RgbaImage> {
        image::open(self.path(id)).ok().map(|i| i.to_rgba8())
    }

    /// Size of the history files. The page shows it every frame, so it is
    /// measured again only when the list of captures changed.
    pub fn bytes_on_disk(&self) -> u64 {
        let key = (self.shots.len(), self.shots.iter().map(|s| s.id).fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b)));
        if let Some((k, b)) = self.disk.get() {
            if k == key {
                return b;
            }
        }
        let b = self.shots.iter().filter_map(|s| std::fs::metadata(self.path(s.id)).ok()).map(|m| m.len()).sum();
        self.disk.set(Some((key, b)));
        b
    }

    fn write_index(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        // off the UI thread: the index is rewritten on every capture
        crate::kit::fs::save_json_later(&self.dir.join("index.json"), &self.shots, false);
    }
}

/// Write the user's copy into their folder. A name that's taken gets " 2", " 3"…
pub fn save_copy(img: &RgbaImage, settings: &Settings, t: chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    let path = copy_path(settings, t)?;
    write_image(img, &path, settings.format)?;
    Ok(path)
}

/// [`save_copy`] for a picture already encoded as PNG: a PNG copy is those
/// bytes as they are; `img` (decoded from `png` when missing) is only
/// needed for JPG.
pub fn save_copy_png(png: &[u8], img: Option<&RgbaImage>, settings: &Settings, t: chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    let path = copy_path(settings, t)?;
    match (settings.format, img) {
        (Format::Png, _) => std::fs::write(&path, png).map_err(|e| e.to_string())?,
        (Format::Jpg, Some(img)) => write_image(img, &path, Format::Jpg)?,
        (Format::Jpg, None) => {
            let img = image::load_from_memory(png).map_err(|e| e.to_string())?.to_rgba8();
            write_image(&img, &path, Format::Jpg)?
        }
    }
    Ok(path)
}

/// A free file name for a new copy in the user's folder.
fn copy_path(settings: &Settings, t: chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&settings.folder).map_err(|e| e.to_string())?;
    let name = settings.file_name(t);
    let mut path = settings.folder.join(&name);
    let mut n = 2;
    while path.exists() {
        let stem = name.trim_end_matches(&format!(".{}", settings.format.ext()));
        path = settings.folder.join(format!("{stem} {n}.{}", settings.format.ext()));
        n += 1;
    }
    Ok(path)
}

pub fn write_image(img: &RgbaImage, path: &Path, format: Format) -> Result<(), String> {
    match format {
        Format::Png => std::fs::write(path, png_bytes(img)).map_err(|e| e.to_string()),
        Format::Jpg => {
            // JPEG has no alpha: flatten on white
            let rgb = image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
                let p = img.get_pixel(x, y).0;
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
            });
            rgb.save_with_format(path, image::ImageFormat::Jpeg).map_err(|e| e.to_string())
        }
    }
}

/// PNG bytes of an image (history, clipboard, OCR input). Fast compression:
/// a full Retina screen encodes several times quicker, for files a little
/// larger.
pub fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive);
    let _ = enc.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cmb-capture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn history_keeps_the_newest_and_pinned() {
        let dir = tmp("store");
        let mut st = Store::open(dir.clone());
        let img = RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]));
        let first = st.add(&img, Kind::Area, 3).unwrap();
        st.set_pinned(first, true);
        for _ in 0..4 {
            st.add(&img, Kind::Screen, 3).unwrap();
        }
        assert_eq!(st.shots.len(), 4, "3 newest plus the pinned one");
        assert!(st.get(first).is_some());
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "png")).count(), 4);
        // survives a reload
        crate::kit::fs::flush_writes();
        let again = Store::open(dir.clone());
        assert_eq!(again.shots, st.shots);
        assert_eq!(again.load(first).unwrap().dimensions(), (4, 3));
        st.remove(first);
        assert!(st.get(first).is_none() && !st.path(first).exists());
        st.clear();
        crate::kit::fs::flush_writes();
        assert!(Store::open(dir.clone()).shots.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn copies_never_overwrite() {
        use chrono::TimeZone;
        let dir = tmp("save");
        let mut s = Settings::default();
        s.folder = dir.clone();
        let t = chrono::Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let img = RgbaImage::from_pixel(2, 2, image::Rgba([200, 0, 0, 128]));
        let a = save_copy(&img, &s, t).unwrap();
        let b = save_copy(&img, &s, t).unwrap();
        assert_ne!(a, b);
        assert!(b.file_name().unwrap().to_string_lossy().ends_with(" 2.png"));
        s.format = Format::Jpg;
        let c = save_copy(&img, &s, t).unwrap();
        assert_eq!(image::open(&c).unwrap().width(), 2);
        // from PNG bytes: a PNG copy is the bytes themselves, a JPG one decodes them
        let png = png_bytes(&img);
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), img);
        let d = save_copy_png(&png, None, &s, t).unwrap();
        assert_eq!(image::open(&d).unwrap().width(), 2);
        s.format = Format::Png;
        let e = save_copy_png(&png, None, &s, t).unwrap();
        assert_eq!(std::fs::read(&e).unwrap(), png);
        let _ = std::fs::remove_dir_all(dir);
    }
}
