//! The user's screenshot folder: the images in it, newest first, so the
//! panel and the Folder tab show what's there (our saved captures and any
//! other screenshots that land in it).

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// At most this many images are listed.
pub const LIMIT: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub struct FileShot {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub w: u32,
    pub h: u32,
    /// Thumbnail key; the top bit keeps it apart from history ids.
    pub key: u64,
}

impl FileShot {
    pub fn time(&self) -> chrono::DateTime<chrono::Local> {
        self.modified.into()
    }
    pub fn name(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

pub fn is_video(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "mov"))
}

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

fn key(path: &Path, modified: SystemTime) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    modified.hash(&mut h);
    h.finish() | (1 << 63)
}

/// PNG, JPG and video files directly in `dir`, newest first.
pub fn scan(dir: &Path) -> Vec<FileShot> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(PathBuf, SystemTime)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && (is_image(&e.path()) || is_video(&e.path())) && !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found.truncate(LIMIT);
    found
        .into_iter()
        .map(|(path, modified)| {
            let (w, h) = if is_video(&path) { (0, 0) } else { image::image_dimensions(&path).unwrap_or((0, 0)) };
            FileShot { key: key(&path, modified), path, modified, w, h }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_images_newest_first() {
        let dir = std::env::temp_dir().join(format!("cmdmeow-folder-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]));
        img.save(dir.join("old.png")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        image::DynamicImage::ImageRgba8(img.clone()).to_rgb8().save(dir.join("new.JPG")).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden.png"), "x").unwrap();
        let v = scan(&dir);
        assert_eq!(v.iter().map(|f| f.name()).collect::<Vec<_>>(), ["new", "old"]);
        assert_eq!((v[1].w, v[1].h), (4, 3));
        assert!(v[0].key >> 63 == 1 && v[0].key != v[1].key);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
