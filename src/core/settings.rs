//! Which panel widgets the user shows, in which order, and which tools are
//! turned off, and at which size each widget is shown. Saved between launches through eframe's storage under
//! [`STORAGE_KEY`] and [`TOOLS_KEY`].

use super::tool::Size;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const STORAGE_KEY: &str = "widgets";
/// Ids of the tools the user turned off (`Vec<String>`).
pub const TOOLS_KEY: &str = "tools_off";

/// `tool/widget`, the settings key of a widget.
pub fn widget_key(tool: &str, widget: &str) -> String {
    format!("{tool}/{widget}")
}

/// One row of the panel's widget list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub on: bool,
    pub size: Size,
}

/// A widget that can be shown: key, shown by default, default size.
pub type Avail = (String, bool, Size);

/// What's saved: the order the user arranged and which widgets they hid.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetPrefs {
    pub order: Vec<String>,
    pub hidden: Vec<String>,
    /// Sizes the user picked ("1:2"); widgets not listed use their default.
    #[serde(default)]
    pub sizes: BTreeMap<String, String>,
}

impl WidgetPrefs {
    /// The widget list for the available widgets (key, shown by default):
    /// saved widgets first in saved order, then widgets the user hasn't seen
    /// yet (a newly installed tool) at their defaults. Widgets of tools that
    /// are gone are dropped.
    pub fn resolve(&self, available: &[Avail]) -> Vec<Entry> {
        let size = |k: &str, default: Size| self.sizes.get(k).and_then(|s| Size::parse(s)).unwrap_or(default);
        let mut out: Vec<Entry> = Vec::with_capacity(available.len());
        for k in &self.order {
            if let Some((_, _, d)) = available.iter().find(|(a, _, _)| a == k) {
                if !out.iter().any(|e| &e.key == k) {
                    out.push(Entry { key: k.clone(), on: !self.hidden.contains(k), size: size(k, *d) });
                }
            }
        }
        for (k, default_on, d) in available {
            if !out.iter().any(|e| &e.key == k) {
                let on = if self.hidden.contains(k) { false } else { *default_on };
                out.push(Entry { key: k.clone(), on, size: size(k, *d) });
            }
        }
        out
    }

    /// What to save. Sizes are saved for every widget, so a later change of
    /// a default doesn't resize what the user already arranged.
    pub fn from_entries(entries: &[Entry]) -> Self {
        WidgetPrefs {
            order: entries.iter().map(|e| e.key.clone()).collect(),
            hidden: entries.iter().filter(|e| !e.on).map(|e| e.key.clone()).collect(),
            sizes: entries.iter().map(|e| (e.key.clone(), e.size.label().to_string())).collect(),
        }
    }
}

/// Show the widget `key` and move it just before `before` (or to the end).
/// Unknown keys are ignored.
pub fn place_entry(entries: &mut Vec<Entry>, key: &str, before: Option<&str>) {
    let Some(i) = entries.iter().position(|e| e.key == key) else { return };
    let mut e = entries.remove(i);
    e.on = true;
    let at = before.and_then(|b| entries.iter().position(|x| x.key == b)).unwrap_or(entries.len());
    entries.insert(at, e);
}

/// Show `key` at `size`, before `before` (or at the end).
pub fn add_entry(entries: &mut Vec<Entry>, key: &str, size: Size, before: Option<&str>) {
    place_entry(entries, key, before);
    set_size(entries, key, size);
}

pub fn set_size(entries: &mut [Entry], key: &str, size: Size) {
    if let Some(e) = entries.iter_mut().find(|e| e.key == key) {
        e.size = size;
    }
}

/// Hide the widget `key`; it keeps its place for when it comes back.
pub fn hide_entry(entries: &mut [Entry], key: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.key == key) {
        e.on = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avail(v: &[(&str, bool)]) -> Vec<Avail> {
        v.iter().map(|(k, d)| (k.to_string(), *d, Size::S12)).collect()
    }

    fn entry(key: &str, on: bool) -> Entry {
        Entry { key: key.into(), on, size: Size::S12 }
    }

    #[test]
    fn defaults_without_saved_prefs() {
        let e = WidgetPrefs::default().resolve(&avail(&[("a/x", true), ("b/y", false)]));
        assert_eq!(e, vec![entry("a/x", true), entry("b/y", false)]);
    }

    #[test]
    fn saved_order_wins_new_widgets_append_and_gone_ones_drop() {
        let prefs = WidgetPrefs { order: vec!["b/y".into(), "gone/z".into(), "a/x".into()], hidden: vec!["a/x".into()], sizes: Default::default() };
        let e = prefs.resolve(&avail(&[("a/x", true), ("b/y", true), ("c/new", true)]));
        let keys: Vec<&str> = e.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["b/y", "a/x", "c/new"]);
        assert!(e[0].on);
        assert!(!e[1].on);
        assert!(e[2].on);
    }

    #[test]
    fn round_trip_and_reorder() {
        let mut e = WidgetPrefs::default().resolve(&avail(&[("a/x", true), ("b/y", true), ("c/z", true)]));
        hide_entry(&mut e, "b/y");
        place_entry(&mut e, "c/z", Some("b/y"));
        place_entry(&mut e, "nope", None); // unknown: no change
        let prefs = WidgetPrefs::from_entries(&e);
        assert_eq!(prefs.order, vec!["a/x", "c/z", "b/y"]);
        assert_eq!(prefs.hidden, vec!["b/y"]);
        assert_eq!(prefs.resolve(&avail(&[("a/x", true), ("b/y", true), ("c/z", true)])), e);
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<WidgetPrefs>(&json).unwrap(), prefs);
    }

    #[test]
    fn sizes_are_saved_and_old_prefs_still_load() {
        let mut e = WidgetPrefs::default().resolve(&avail(&[("a/x", true), ("b/y", true)]));
        set_size(&mut e, "b/y", Size::S44);
        add_entry(&mut e, "a/x", Size::S11, None);
        let prefs = WidgetPrefs::from_entries(&e);
        let back = prefs.resolve(&avail(&[("a/x", true), ("b/y", true)]));
        assert_eq!(back[0].key, "b/y");
        assert_eq!(back[0].size, Size::S44);
        assert_eq!(back[1].size, Size::S11);
        // prefs saved before sizes existed; a bad size falls back to the default
        let old: WidgetPrefs = serde_json::from_str(r#"{"order":["a/x"],"hidden":[]}"#).unwrap();
        assert_eq!(old.resolve(&avail(&[("a/x", true)]))[0].size, Size::S12);
        let bad: WidgetPrefs = serde_json::from_str(r#"{"order":[],"hidden":[],"sizes":{"a/x":"9:9"}}"#).unwrap();
        assert_eq!(bad.resolve(&avail(&[("a/x", true)]))[0].size, Size::S12);
    }

    #[test]
    fn place_shows_and_moves() {
        let mut e = WidgetPrefs::default().resolve(&avail(&[("a/x", true), ("b/y", false), ("c/z", true)]));
        place_entry(&mut e, "b/y", Some("a/x"));
        let keys: Vec<&str> = e.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["b/y", "a/x", "c/z"]);
        assert!(e[0].on);
        place_entry(&mut e, "b/y", None);
        let keys: Vec<&str> = e.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["a/x", "c/z", "b/y"]);
        place_entry(&mut e, "a/x", Some("a/x")); // before itself: goes to the end
        assert_eq!(e[2].key, "a/x");
    }
}
