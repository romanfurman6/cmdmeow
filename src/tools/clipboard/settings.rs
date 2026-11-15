//! Clipboard's own settings, saved as JSON in the app's data folder.

use super::secret_apps::{self, Found};
use super::store::Limits;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// History length choices bound the stepper.
pub const KEEP_MIN: usize = 20;
pub const KEEP_MAX: usize = 1000;
/// Choices for "Remove items older than" (0 = never).
pub const AGES: [u32; 4] = [1, 7, 30, 0];
/// Biggest image kept, in bytes.
pub const MAX_IMAGE: usize = 10 * 1024 * 1024;

/// An app whose copies are never recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ignored {
    pub bundle: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub keep: usize,
    pub max_days: u32,
    pub keep_images: bool,
    pub dedupe: bool,
    /// Also record passwords (concealed clips), in memory only.
    pub record_concealed: bool,
    /// Also record API tokens, private keys and card numbers.
    pub record_secrets: bool,
    /// Record what arrives from an iPhone or another Mac (Universal Clipboard).
    pub record_remote: bool,
    pub ignore: Vec<Ignored>,
    /// Password managers found on this Mac and put in `ignore` once
    /// (lowercase bundle ids): one the user takes out stays out.
    pub seeded: Vec<String>,
    /// 0: written before defaults followed what's installed.
    pub defaults_v: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            keep: 200,
            max_days: 30,
            keep_images: true,
            dedupe: true,
            record_concealed: false,
            record_secrets: false,
            record_remote: true,
            ignore: Vec::new(),
            seeded: Vec::new(),
            defaults_v: 0,
        }
    }
}

impl Settings {
    pub fn limits(&self) -> Limits {
        Limits { keep: self.keep, max_days: self.max_days, dedupe: self.dedupe }
    }

    pub fn ignores(&self, bundle: &str) -> bool {
        !bundle.is_empty() && self.ignore.iter().any(|i| i.bundle.eq_ignore_ascii_case(bundle))
    }

    /// Never record from the password managers on this Mac (`installed` is
    /// every app found). Each one is added once; ones the user removed or
    /// added by hand are left alone. Old defaults for apps that aren't
    /// installed are dropped. True when something changed.
    pub fn adopt_installed(&mut self, installed: &[Found]) -> bool {
        let mut changed = false;
        let has = |b: &str| installed.iter().any(|f| f.bundle.eq_ignore_ascii_case(b));
        if self.defaults_v < 1 {
            self.ignore.retain(|i| has(&i.bundle) || !secret_apps::OLD_DEFAULTS.iter().any(|d| d.eq_ignore_ascii_case(&i.bundle)));
            self.defaults_v = 1;
            changed = true;
        }
        for f in secret_apps::password_managers(installed) {
            let key = f.bundle.to_lowercase();
            if self.seeded.contains(&key) {
                continue;
            }
            self.seeded.push(key);
            if !self.ignores(&f.bundle) {
                self.ignore.push(Ignored { bundle: f.bundle, name: f.name });
            }
            changed = true;
        }
        changed
    }

    /// Keep values in range after loading a file someone edited by hand.
    fn sanitize(mut self) -> Self {
        self.keep = self.keep.clamp(KEEP_MIN, KEEP_MAX);
        if !AGES.contains(&self.max_days) {
            self.max_days = 30;
        }
        self
    }
}

/// `~/Library/Application Support/cmdmeow/clipboard` (the platform's data dir elsewhere).
pub fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("clipboard")
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

    #[test]
    fn defaults() {
        let s = Settings::default();
        assert!(!s.record_concealed, "passwords stay out unless turned on");
        assert!(!s.record_secrets, "tokens stay out unless turned on");
        assert!(s.record_remote);
        assert!(s.ignore.is_empty(), "nothing listed until the Mac is scanned");
        assert!(!s.ignores(""));
    }

    fn app(bundle: &str, name: &str) -> Found {
        Found { bundle: bundle.into(), name: name.into(), path: format!("/Applications/{name}.app").into() }
    }

    fn ig(bundle: &str, name: &str) -> Ignored {
        Ignored { bundle: bundle.into(), name: name.into() }
    }

    #[test]
    fn adds_only_installed_managers() {
        let mut s = Settings::default();
        let mac = [app("com.apple.Safari", "Safari"), app("com.1password.1password", "1Password"), app("com.apple.keychainaccess", "Keychain Access")];
        assert!(s.adopt_installed(&mac));
        assert_eq!(s.ignore, [ig("com.1password.1password", "1Password"), ig("com.apple.keychainaccess", "Keychain Access")]);
        assert!(s.ignores("com.1Password.1password"));
        assert!(!s.ignores("com.bitwarden.desktop"));
        assert!(!s.adopt_installed(&mac), "second run changes nothing");
    }

    #[test]
    fn removed_stays_removed_new_install_is_added() {
        let mut s = Settings::default();
        s.adopt_installed(&[app("com.1password.1password", "1Password")]);
        s.ignore.clear();
        let mac = [app("com.1password.1password", "1Password"), app("org.keepassxc.keepassxc", "KeePassXC")];
        assert!(s.adopt_installed(&mac));
        assert_eq!(s.ignore, [ig("org.keepassxc.keepassxc", "KeePassXC")]);
    }

    #[test]
    fn migrates_old_defaults_keeps_user_apps() {
        // A file from an older build: the five fixed defaults plus one app the user added.
        let mut s = Settings {
            ignore: vec![
                ig("com.1password.1password", "1Password"),
                ig("com.agilebits.onepassword7", "1Password 7"),
                ig("com.apple.keychainaccess", "Keychain Access"),
                ig("com.apple.Passwords", "Passwords"),
                ig("com.bitwarden.desktop", "Bitwarden"),
                ig("com.tinyspeck.slackmacgap", "Slack"),
            ],
            ..Default::default()
        };
        let mac = [app("com.apple.Passwords", "Passwords"), app("com.apple.keychainaccess", "Keychain Access"), app("com.tinyspeck.slackmacgap", "Slack")];
        assert!(s.adopt_installed(&mac));
        let left: Vec<&str> = s.ignore.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(left, ["Keychain Access", "Passwords", "Slack"]);
        assert_eq!(s.defaults_v, 1);
        // Taking Passwords out later sticks.
        s.ignore.retain(|i| i.name != "Passwords");
        assert!(!s.adopt_installed(&mac));
        assert!(!s.ignores("com.apple.Passwords"));
    }

    #[test]
    fn old_file_without_new_fields_loads() {
        let dir = std::env::temp_dir().join(format!("cmb-clip-old-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"keep":200,"ignore":[{"bundle":"com.bitwarden.desktop","name":"Bitwarden"}]}"#).unwrap();
        let mut s = load(&dir);
        assert_eq!(s.defaults_v, 0);
        assert!(s.adopt_installed(&[]));
        assert!(s.ignore.is_empty(), "Bitwarden isn't installed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("cmb-clip-settings-{}", std::process::id()));
        let s = Settings { keep: 5, max_days: 3, dedupe: false, ..Default::default() };
        save(&dir, &s);
        let back = load(&dir);
        assert_eq!(back.keep, KEEP_MIN);
        assert_eq!(back.max_days, 30);
        assert!(!back.dedupe);
        let _ = std::fs::remove_dir_all(dir);
    }
}
