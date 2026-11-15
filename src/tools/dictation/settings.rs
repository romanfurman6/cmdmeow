//! Dictation's settings and its history of what was said, as JSON in the
//! app's data folder.

use crate::kit::hotkeys::Shortcut;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Most dictations kept.
pub const HISTORY_MAX: usize = 100;

/// Which model turns the voice into text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    /// NVIDIA Parakeet TDT 0.6B v3: fast, tells the language itself.
    #[default]
    Parakeet,
    /// Whisper large-v3 turbo (shared with Capture's transcripts).
    Whisper,
}

impl Engine {
    pub const ALL: [Engine; 2] = [Engine::Parakeet, Engine::Whisper];

    pub fn name(self) -> &'static str {
        match self {
            Engine::Parakeet => "Parakeet v3",
            Engine::Whisper => "Whisper turbo",
        }
    }

    /// About how big the download is.
    pub fn bytes(self) -> u64 {
        match self {
            Engine::Parakeet => crate::kit::parakeet::BYTES,
            Engine::Whisper => crate::kit::whisper::Model::Accurate.bytes(),
        }
    }

    pub fn ready(self) -> bool {
        match self {
            Engine::Parakeet => crate::kit::parakeet::ready(),
            Engine::Whisper => crate::kit::whisper::Model::Accurate.ready(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Hold to talk, or tap to start and tap again to stop.
    pub shortcut: Option<Shortcut>,
    pub engine: Engine,
    /// Put back what was on the clipboard after pasting.
    pub restore_clipboard: bool,
    pub keep_history: bool,
    /// Hands-free: stop after this many quiet seconds; 0 never.
    pub silence_secs: u32,
    /// The person's own words (names, brands): what sounds like one of
    /// them is written their way.
    pub words: Vec<String>,
    /// Settings format; 1 moved the old default ⌥Space to ⌥D. Missing in
    /// a file means 0, not today's.
    #[serde(default)]
    pub version: u32,
}

/// The default before version 1.
fn old_default() -> Shortcut {
    Shortcut { cmd: false, shift: false, alt: true, ctrl: false, key: "Space".into() }
}

/// The choices for [`Settings::silence_secs`].
pub const SILENCE_CHOICES: [u32; 4] = [0, 3, 5, 10];

/// Most words in the list.
pub const WORDS_MAX: usize = 200;

impl Settings {
    /// Add `w` (trimmed) unless it's empty, too long or already there
    /// (any case). Returns whether it was added.
    pub fn add_word(&mut self, w: &str) -> bool {
        let w = w.split_whitespace().collect::<Vec<_>>().join(" ");
        if w.is_empty() || w.chars().count() > 40 || self.words.len() >= WORDS_MAX || self.words.iter().any(|x| x.to_lowercase() == w.to_lowercase()) {
            return false;
        }
        self.words.push(w);
        true
    }

    pub fn stop_after_silence(&self) -> Option<std::time::Duration> {
        (self.silence_secs > 0).then(|| std::time::Duration::from_secs(self.silence_secs as u64))
    }
}

pub fn default_shortcut() -> Shortcut {
    Shortcut { cmd: false, shift: false, alt: true, ctrl: false, key: "D".into() }
}

impl Default for Settings {
    fn default() -> Self {
        Settings { shortcut: Some(default_shortcut()), engine: Engine::Parakeet, restore_clipboard: true, keep_history: true, silence_secs: 5, words: Vec::new(), version: VERSION }
    }
}

pub fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("dictation")
}

const VERSION: u32 = 1;

pub fn load(dir: &Path) -> Settings {
    upgrade(crate::kit::fs::load_json(&dir.join("settings.json")))
}

/// Bring settings saved by an older build up to date.
fn upgrade(mut s: Settings) -> Settings {
    if s.version < 1 && s.shortcut == Some(old_default()) {
        s.shortcut = Some(default_shortcut());
    }
    s.version = VERSION;
    s
}

pub fn save(dir: &Path, s: &Settings) {
    crate::kit::fs::save_json(&dir.join("settings.json"), s, true);
}

/// One thing said and pasted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Said {
    /// Unix seconds.
    pub at: i64,
    pub text: String,
    /// The app it went into.
    pub app: String,
    /// How long the voice was, seconds.
    pub secs: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// Newest first.
    pub items: Vec<Said>,
}

impl History {
    pub fn add(&mut self, s: Said) {
        self.items.insert(0, s);
        self.items.truncate(HISTORY_MAX);
    }

    /// Words said since `since` (unix seconds).
    pub fn words_since(&self, since: i64) -> usize {
        self.items.iter().take_while(|s| s.at >= since).map(|s| s.text.split_whitespace().count()).sum()
    }
}

/// What was said, sealed on disk (see [`crate::kit::vault`]); `history.json`
/// from before that is sealed on first load.
pub fn load_history(dir: &Path) -> History {
    crate::kit::vault::load_json(crate::kit::vault::shared().as_deref(), &dir.join("history.bin"), &dir.join("history.json"))
}

pub fn save_history(dir: &Path, h: &History) {
    crate::kit::vault::save_json_later(crate::kit::vault::shared().as_deref(), &dir.join("history.bin"), h);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest() {
        let mut h = History::default();
        for k in 0..HISTORY_MAX + 5 {
            h.add(Said { at: k as i64, text: "one two".into(), app: String::new(), secs: 1.0 });
        }
        assert_eq!(h.items.len(), HISTORY_MAX);
        assert_eq!(h.items[0].at, (HISTORY_MAX + 4) as i64);
        assert_eq!(h.words_since((HISTORY_MAX + 3) as i64), 4);
    }

    #[test]
    fn old_default_shortcut_moves_to_option_d_once() {
        let s = upgrade(serde_json::from_str(r#"{"shortcut":{"cmd":false,"shift":false,"alt":true,"ctrl":false,"key":"Space"}}"#).unwrap());
        assert_eq!(s.shortcut.as_ref().map(|k| k.display()), Some("⌥D".to_string()));
        // chosen again after the upgrade: kept
        let again = upgrade(Settings { shortcut: Some(old_default()), ..s });
        assert_eq!(again.shortcut, Some(old_default()));
        // someone's own pick is never touched
        let mine = Shortcut { cmd: true, shift: true, alt: false, ctrl: false, key: "D".into() };
        let s = upgrade(Settings { shortcut: Some(mine.clone()), version: 0, ..Settings::default() });
        assert_eq!(s.shortcut, Some(mine));
    }

    #[test]
    fn words_are_added_once_and_tidy() {
        let mut s = Settings::default();
        assert!(s.add_word("  Cmd   Meow "));
        assert!(!s.add_word("cmd meow"));
        assert!(!s.add_word("   "));
        assert!(!s.add_word(&"x".repeat(41)));
        assert_eq!(s.words, vec!["Cmd Meow".to_string()]);
    }

    #[test]
    fn settings_round_trip_and_fill_gaps() {
        // a file from before keeps working: unknown fields are dropped
        let s: Settings = serde_json::from_str(r#"{"lang":"Uk"}"#).unwrap();
        assert_eq!(s.shortcut, Some(default_shortcut()));
        assert!(s.restore_clipboard);
        assert_eq!(s.stop_after_silence(), Some(std::time::Duration::from_secs(5)));
        assert_eq!(Settings { silence_secs: 0, ..s.clone() }.stop_after_silence(), None);
    }
}
