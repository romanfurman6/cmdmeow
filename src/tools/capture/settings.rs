//! Capture's own settings: global shortcuts, what happens after a capture,
//! where files go and how much history to keep. Saved as JSON in the app's
//! data folder, separate from the shell's widget settings.

pub use crate::kit::hotkeys::Shortcut;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Something a global shortcut can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Command {
    Area,
    Window,
    Screen,
    Delayed,
    Text,
    Color,
    Ruler,
    History,
    Record,
}

impl Command {
    pub const ALL: [Command; 9] = [Command::Area, Command::Window, Command::Screen, Command::Delayed, Command::Text, Command::Color, Command::Ruler, Command::History, Command::Record];

    pub fn label(self) -> &'static str {
        match self {
            Command::Area => "Capture area",
            Command::Window => "Capture window",
            Command::Screen => "Capture screen",
            Command::Delayed => "Delayed capture",
            Command::Text => "Copy text from screen",
            Command::Color => "Pick a color",
            Command::Ruler => "Measure (ruler)",
            Command::History => "Open history",
            Command::Record => "Record the screen",
        }
    }

    /// Its name in links: `cmdmeow://capture/<slug>`.
    pub fn slug(self) -> &'static str {
        match self {
            Command::Area => "area",
            Command::Window => "window",
            Command::Screen => "screen",
            Command::Delayed => "delayed",
            Command::Text => "text",
            Command::Color => "color",
            Command::Ruler => "ruler",
            Command::History => "history",
            Command::Record => "record",
        }
    }

    pub fn from_slug(s: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.slug() == s)
    }

    /// Shortcut on first launch. ⌘⇧3/4/5 belong to macOS, so they're avoided.
    pub fn default_shortcut(self) -> Option<Shortcut> {
        let key = match self {
            Command::Screen => "1",
            Command::Area => "2",
            Command::Window => "6",
            Command::Text => "7",
            Command::Color => "8",
            Command::Ruler => "9",
            Command::Record => "0",
            Command::Delayed | Command::History => return None,
        };
        Some(Shortcut { cmd: true, shift: true, alt: false, ctrl: false, key: key.into() })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum After {
    Preview,
    Editor,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Png,
    Jpg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }
}

/// How Blur hides what's under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlurStyle {
    Pixelate,
    Blur,
    Solid,
}

/// What Blur and Eraser cover: the whole box, or only the text OCR finds in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cover {
    Box,
    Text,
}

/// Delay choices for the panel's switch, in seconds (0 = off).
pub const DELAYS: [u32; 4] = [0, 3, 5, 10];
/// The "Delayed capture" shortcut waits this long when the switch is off.
pub const DEFAULT_DELAY: u32 = 5;
/// History length choices bound the stepper.
pub const KEEP_MIN: usize = 10;
pub const KEEP_MAX: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Command → shortcut; a missing or `None` entry means no shortcut.
    pub shortcuts: Vec<(Command, Option<Shortcut>)>,
    pub after: After,
    pub copy: bool,
    pub save: bool,
    /// Seconds the preview stays up; None keeps it until closed.
    pub preview_secs: Option<u32>,
    pub sound: bool,
    pub folder: PathBuf,
    pub format: Format,
    pub keep: usize,
    pub delay: u32,
    pub blur: BlurStyle,
    pub cover: Cover,
    pub keep_breaks: bool,
    /// The editor's last frame (backdrop, padding, corners, shadow, ratio).
    pub frame: super::render::Frame,
    /// Screen recording.
    pub rec: RecSettings,
    /// The storage (set up on the Sharing page), and the videos' folder
    /// and switches.
    pub share: super::share::Settings,
    /// Screenshots: only their folder and switches are used; the storage
    /// is `share`'s.
    pub shot_share: super::share::Settings,
}

/// Frame rates the recorder offers.
pub const RECORD_FPS: [u32; 2] = [30, 60];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecQuality {
    /// Up to 1080p, smaller files.
    Smaller,
    /// Up to 4K.
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraSize {
    S,
    M,
    L,
}

impl CameraSize {
    /// Diameter of the camera bubble, in points.
    pub fn points(self) -> f32 {
        match self {
            CameraSize::S => 120.0,
            CameraSize::M => 180.0,
            CameraSize::L => 260.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecSettings {
    pub camera: bool,
    pub mic: bool,
    pub system_audio: bool,
    /// Device unique ids; None = the system default.
    pub camera_device: Option<String>,
    pub mic_device: Option<String>,
    pub fps: u32,
    pub quality: RecQuality,
    pub camera_size: CameraSize,
    pub cursor: bool,
    pub clicks: bool,
    /// Seconds of 3-2-1 before recording (0 = none).
    pub countdown: u32,
    /// After a recording: open it in the video editor right away (instead
    /// of the small saved card).
    pub open_editor: bool,
    /// Seconds the saved card stays up; None keeps it until closed.
    pub card_secs: Option<u32>,
}

impl Default for RecSettings {
    fn default() -> Self {
        RecSettings {
            camera: false,
            mic: false,
            system_audio: true,
            camera_device: None,
            mic_device: None,
            fps: 60,
            quality: RecQuality::High,
            camera_size: CameraSize::M,
            cursor: true,
            clicks: false,
            countdown: 3,
            open_editor: false,
            card_secs: Some(8),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shortcuts: Command::ALL.iter().map(|&c| (c, c.default_shortcut())).collect(),
            after: After::Preview,
            copy: true,
            save: true,
            preview_secs: Some(6),
            sound: false,
            folder: crate::kit::fs::home().join("Desktop"),
            format: Format::Png,
            keep: 50,
            delay: 0,
            blur: BlurStyle::Pixelate,
            cover: Cover::Box,
            keep_breaks: true,
            frame: Default::default(),
            rec: RecSettings::default(),
            share: Default::default(),
            shot_share: super::share::Settings::shots(),
        }
    }
}

impl Settings {
    pub fn shortcut(&self, c: Command) -> Option<&Shortcut> {
        self.shortcuts.iter().find(|(k, _)| *k == c).and_then(|(_, s)| s.as_ref())
    }

    /// Set (or clear) a command's shortcut. Another command holding the same
    /// keys loses them, so one combination never starts two things.
    pub fn set_shortcut(&mut self, c: Command, s: Option<Shortcut>) {
        if let Some(new) = &s {
            for (_, other) in self.shortcuts.iter_mut() {
                if other.as_ref() == Some(new) {
                    *other = None;
                }
            }
        }
        match self.shortcuts.iter_mut().find(|(k, _)| *k == c) {
            Some(e) => e.1 = s,
            None => self.shortcuts.push((c, s)),
        }
    }

    /// Commands that have a shortcut, for registering.
    pub fn bound(&self) -> Vec<(Command, Shortcut)> {
        Command::ALL.iter().filter_map(|&c| self.shortcut(c).map(|s| (c, s.clone()))).collect()
    }

    /// File name for a capture taken at `t`: "Capture 2026-11-08 at 17.41.12.png".
    pub fn file_name(&self, t: chrono::DateTime<chrono::Local>) -> String {
        format!("Capture {}.{}", t.format("%Y-%m-%d at %H.%M.%S"), self.format.ext())
    }

    /// File name for a recording that started at `t`: "Recording 2026-11-09 at 17.26.12.mp4".
    pub fn recording_name(t: chrono::DateTime<chrono::Local>) -> String {
        format!("Recording {}.mp4", t.format("%Y-%m-%d at %H.%M.%S"))
    }

    /// Keep values in range after loading a file someone edited by hand.
    fn sanitize(mut self) -> Self {
        self.keep = self.keep.clamp(KEEP_MIN, KEEP_MAX);
        // the first default folder; videos and screenshots now sit side by side
        if self.share.folder.trim() == "recordings/" {
            self.share.folder = super::share::VIDEO_FOLDER.into();
        }
        if !DELAYS.contains(&self.delay) {
            self.delay = 0;
        }
        for c in Command::ALL {
            if !self.shortcuts.iter().any(|(k, _)| *k == c) {
                // a command added in an update gets its default keys unless
                // the user already gave them to something else
                let free = c.default_shortcut().filter(|d| !self.shortcuts.iter().any(|(_, s)| s.as_ref() == Some(d)));
                self.shortcuts.push((c, free));
            }
        }
        if !RECORD_FPS.contains(&self.rec.fps) {
            self.rec.fps = 60;
        }
        self
    }
}

/// `~/Library/Application Support/cmdmeow/capture` (the platform's data dir elsewhere).
pub fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("capture")
}

pub fn load(dir: &Path) -> Settings {
    crate::kit::fs::load_json::<Settings>(&dir.join("settings.json")).sanitize()
}

pub fn save(dir: &Path, s: &Settings) {
    crate::kit::fs::save_json(&dir.join("settings.json"), s, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::hotkeys::key_code;

    fn sc(key: &str) -> Shortcut {
        Shortcut { cmd: true, shift: true, alt: false, ctrl: false, key: key.into() }
    }

    #[test]
    fn defaults_avoid_macos_screenshot_keys() {
        let s = Settings::default();
        for (c, k) in s.bound() {
            assert!(k.conflict().is_none(), "{c:?} default {} conflicts", k.display());
        }
        assert_eq!(s.shortcut(Command::Area).unwrap().display(), "⇧⌘2");
        assert!(s.shortcut(Command::History).is_none());
        assert!(sc("3").conflict().is_some());
    }

    #[test]
    fn a_shortcut_moves_instead_of_duplicating() {
        let mut s = Settings::default();
        s.set_shortcut(Command::History, Some(sc("2")));
        assert_eq!(s.shortcut(Command::History), Some(&sc("2")));
        assert!(s.shortcut(Command::Area).is_none());
        s.set_shortcut(Command::History, None);
        assert!(s.shortcut(Command::History).is_none());
    }

    #[test]
    fn key_codes() {
        assert_eq!(key_code("2").as_deref(), Some("Digit2"));
        assert_eq!(key_code("K").as_deref(), Some("KeyK"));
        assert_eq!(key_code("F12").as_deref(), Some("F12"));
        assert_eq!(key_code("F99"), None);
        assert_eq!(key_code("Escape"), None);
    }

    #[test]
    fn recorder_needs_a_modifier() {
        let none = egui::Modifiers::NONE;
        assert!(Shortcut::from_press(egui::Key::A, none).is_none());
        let cmd_shift = egui::Modifiers { mac_cmd: true, command: true, shift: true, ..Default::default() };
        assert_eq!(Shortcut::from_press(egui::Key::Num2, cmd_shift), Some(sc("2")));
        assert!(Shortcut::from_press(egui::Key::Escape, cmd_shift).is_none());
    }

    #[test]
    fn saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("cmb-capture-settings-{}", std::process::id()));
        let mut s = Settings::default();
        s.keep = 9999;
        s.format = Format::Jpg;
        s.set_shortcut(Command::Ruler, None);
        save(&dir, &s);
        let back = load(&dir);
        assert_eq!(back.format, Format::Jpg);
        assert_eq!(back.keep, KEEP_MAX);
        assert!(back.shortcut(Command::Ruler).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_names() {
        use chrono::TimeZone;
        let t = chrono::Local.with_ymd_and_hms(2026, 11, 8, 17, 41, 12).unwrap();
        assert_eq!(Settings::default().file_name(t), "Capture 2026-11-08 at 17.41.12.png");
        assert_eq!(Settings::recording_name(t), "Recording 2026-11-08 at 17.41.12.mp4");
    }

    #[test]
    fn a_command_added_later_gets_its_default_keys_when_free() {
        let mut s = Settings::default();
        s.shortcuts.retain(|(c, _)| *c != Command::Record);
        assert_eq!(s.clone().sanitize().shortcut(Command::Record), Some(&sc("0")));
        // keys already taken by something else stay there
        s.shortcuts.retain(|(c, _)| *c != Command::Ruler);
        s.shortcuts.push((Command::Ruler, Some(sc("0"))));
        let back = s.sanitize();
        assert!(back.shortcut(Command::Record).is_none());
        assert_eq!(back.shortcut(Command::Ruler), Some(&sc("0")));
    }
}
