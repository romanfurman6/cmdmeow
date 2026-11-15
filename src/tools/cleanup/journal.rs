//! What each clean removed, where it went and what couldn't go: kept on disk
//! (the last few cleans) so "where did my file go?" always has an answer.

use super::engine::How;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Cleans kept.
const KEEP: usize = 30;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub when: i64,
    pub items: Vec<Removed>,
    /// (path, why)
    pub failed: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Removed {
    pub title: String,
    pub path: String,
    pub bytes: u64,
    pub how: How,
}

impl Entry {
    pub fn now() -> Entry {
        Entry { when: chrono::Utc::now().timestamp(), ..Default::default() }
    }
    pub fn bytes(&self, how: How) -> u64 {
        self.items.iter().filter(|i| i.how == how).map(|i| i.bytes).sum()
    }
}

fn file() -> PathBuf {
    crate::kit::fs::data_path("cleanup").join("journal.json")
}

pub fn record(e: Entry) {
    save_to(&file(), e);
}

fn save_to(f: &std::path::Path, e: Entry) {
    let mut v: Vec<Entry> = crate::kit::fs::load_json(f);
    v.push(e);
    let drop = v.len().saturating_sub(KEEP);
    v.drain(..drop);
    crate::kit::fs::save_json(f, &v, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_cleans() {
        let f = std::env::temp_dir().join(format!("cmdmeow-journal-{}/journal.json", std::process::id()));
        for i in 0..(KEEP + 5) {
            let mut e = Entry::now();
            e.when = i as i64;
            e.items.push(Removed { title: "x".into(), path: "/x".into(), bytes: 10, how: How::Trash });
            e.items.push(Removed { title: "y".into(), path: "/y".into(), bytes: 5, how: How::Deleted });
            save_to(&f, e);
        }
        let v: Vec<Entry> = crate::kit::fs::load_json(&f);
        assert_eq!(v.len(), KEEP);
        assert_eq!(v[0].when, 5, "the oldest go first");
        assert_eq!(v[0].bytes(How::Trash), 10);
        assert_eq!(v[0].bytes(How::Deleted), 5);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("\"trash\""), "how is stored as a word: {text}");
        std::fs::remove_dir_all(f.parent().unwrap()).unwrap();
    }
}
