//! The AI usage tool's own settings, kept in `<data>/ai/settings.json`:
//! the main provider, which providers are on, budgets for API keys,
//! notifications and what the menu bar shows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Id of the provider shown on the page ("claude", "codex", …), or
    /// "all" for every provider at once.
    pub provider: String,
    /// Monthly budget in dollars, by provider name, for API keys.
    pub budgets: HashMap<String, f64>,
    /// Monthly plan price in dollars, by provider name, when the plan name
    /// alone doesn't say it.
    pub plan_usd: HashMap<String, f64>,
    /// Ids of providers turned off.
    pub off: Vec<String>,
    /// Provider ids in the person's order (H10: dragged in the page menu
    /// or Settings). Ids not named keep their place after these.
    pub order: Vec<String>,
    /// More Claude Code accounts: each its own config folder
    /// (`CLAUDE_CONFIG_DIR`). `~/.claude` is always the first.
    pub accounts: Vec<Account>,
    /// What `~/.claude` is called once there are more accounts.
    pub main_name: String,
    /// Id of the agent the person mainly uses (picked in setup): Cleanup's
    /// AI check runs with it.
    pub main: String,
    /// Show dollar amounts.
    pub show_cost: bool,
    /// The Usage tab counts tokens instead of dollars (its $ | Tokens switch).
    pub usage_tokens: bool,
    /// Notify when a limit (or budget) passes these percentages.
    pub notify_at: Vec<u8>,
    /// Notify when a limit resets after being over the first threshold.
    pub notify_reset: bool,
    /// Notify a day before a weekly limit resets with much of it unused.
    pub notify_unused: bool,
    /// What the menu bar shows: "closest" (the provider nearest a limit)
    /// or a provider id.
    pub menu_bar: String,
    /// Reading Claude Code's sign-in from the Keychain: "" not asked yet
    /// (never read until the person connects it), "ok" allowed, "denied"
    /// (limits then come from the Claude CLI).
    pub claude_keychain: String,
    /// Settings format: 1 turned the menu bar number off by default. A
    /// file without it is older (0), not the struct default.
    #[serde(default)]
    pub version: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: String::new(),
            budgets: HashMap::new(),
            plan_usd: HashMap::new(),
            off: Vec::new(),
            order: Vec::new(),
            accounts: Vec::new(),
            main_name: String::new(),
            main: "claude".into(),
            show_cost: true,
            usage_tokens: false,
            notify_at: vec![80, 95],
            notify_reset: true,
            notify_unused: true,
            // the icon alone; a number there is opt-in
            menu_bar: "off".into(),
            claude_keychain: String::new(),
            version: 1,
        }
    }
}

/// Another Claude Code account: a name and its config folder ("~/…").
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Account {
    pub name: String,
    pub dir: String,
}

impl Account {
    /// The folder with `~` expanded.
    pub fn path(&self) -> PathBuf {
        expand(&self.dir)
    }
}

/// `~/x` as a full path.
pub fn expand(dir: &str) -> PathBuf {
    match dir.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None if dir == "~" => dirs::home_dir().unwrap_or_default(),
        None => PathBuf::from(dir),
    }
}

/// A full path as "~/x" when it's under the home folder.
pub fn tilde(path: &std::path::Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

impl Settings {
    /// `~/.claude`'s name beside the other accounts.
    pub fn main_account(&self) -> String {
        if self.main_name.trim().is_empty() { ts!("ai.acc.main").to_string() } else { self.main_name.clone() }
    }
    pub fn is_on(&self, id: &str) -> bool {
        !self.off.iter().any(|o| o == id)
    }
    /// Indexes into `PROVIDERS` in the person's order.
    pub fn ordered(&self) -> Vec<usize> {
        order_of(&self.order, &ids())
    }
    /// Drag row `from` of `shown` (indexes into `PROVIDERS`, in order) to
    /// the gap before row `gap`; providers not shown keep their places.
    pub fn reorder(&mut self, shown: &[usize], from: usize, gap: usize) {
        let ids = ids();
        self.order = reordered(&self.ordered(), shown, from, gap).into_iter().map(|k| ids[k].to_string()).collect();
    }
}

fn ids() -> Vec<&'static str> {
    super::PROVIDERS.iter().map(|m| m.id).collect()
}

/// Indexes into `ids`: the ones `order` names first, in its order, then
/// the rest as listed.
fn order_of(order: &[String], ids: &[&str]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..ids.len()).collect();
    v.sort_by_key(|&k| match order.iter().position(|o| o == ids[k]) {
        Some(at) => (0, at),
        None => (1, k),
    });
    v
}

/// `full` with row `from` of `shown` moved to the gap before row `gap`
/// (0..=shown.len()); the rest keep their slots.
fn reordered(full: &[usize], shown: &[usize], from: usize, gap: usize) -> Vec<usize> {
    if from >= shown.len() {
        return full.to_vec();
    }
    let mut sub = shown.to_vec();
    let k = sub.remove(from);
    let to = if gap > from { gap - 1 } else { gap };
    sub.insert(to.min(sub.len()), k);
    let mut it = sub.into_iter();
    full.iter().map(|k| if shown.contains(k) { it.next().unwrap_or(*k) } else { *k }).collect()
}

fn path() -> PathBuf {
    crate::kit::fs::data_path("ai").join("settings.json")
}

fn cell() -> &'static Mutex<Settings> {
    static S: OnceLock<Mutex<Settings>> = OnceLock::new();
    S.get_or_init(|| {
        let mut s: Settings = crate::kit::fs::load_json(&path());
        // before version 1 the menu bar showed the closest limit by default
        if s.version < 1 {
            s.menu_bar = "off".into();
            s.version = 1;
            crate::kit::fs::save_json(&path(), &s, true);
        }
        Mutex::new(s)
    })
}

/// The current settings.
pub fn get() -> Settings {
    cell().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Change the settings and save them (unless `save` is false, for demos).
pub fn update(save: bool, f: impl FnOnce(&mut Settings)) {
    let mut g = cell().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g);
    if save {
        crate::kit::fs::save_json(&path(), &*g, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_folders_round_trip_through_tilde() {
        let home = dirs::home_dir().unwrap_or_default();
        assert_eq!(expand("~/.claude-work"), home.join(".claude-work"));
        assert_eq!(tilde(&home.join(".claude-work")), "~/.claude-work");
        assert_eq!(expand("/opt/x"), PathBuf::from("/opt/x"));
    }

    #[test]
    fn order_names_first_then_the_rest() {
        let ids = ["claude", "codex", "gemini", "opencode"];
        assert_eq!(order_of(&[], &ids), vec![0, 1, 2, 3]);
        assert_eq!(order_of(&["gemini".into(), "gone".into(), "claude".into()], &ids), vec![2, 0, 1, 3]);
    }

    #[test]
    fn dragging_moves_only_shown_rows() {
        // Codex dragged above Claude; Gemini (off, not shown) keeps its slot
        assert_eq!(reordered(&[0, 2, 1, 3], &[0, 1, 3], 1, 0), vec![1, 2, 0, 3]);
        // Claude dragged to the end
        assert_eq!(reordered(&[0, 1, 2], &[0, 1, 2], 0, 3), vec![1, 2, 0]);
        // dropped where it was
        assert_eq!(reordered(&[0, 1, 2], &[0, 1, 2], 1, 2), vec![0, 1, 2]);
    }

    #[test]
    fn a_file_without_version_is_old() {
        let s: Settings = serde_json::from_str(r#"{"menu_bar":"closest"}"#).unwrap();
        assert_eq!((s.version, s.menu_bar.as_str()), (0, "closest"));
        assert_eq!(Settings::default().version, 1);
    }

    #[test]
    fn old_files_keep_their_provider_and_get_defaults() {
        let s: Settings = serde_json::from_str("{\"provider\":\"codex\"}").unwrap();
        assert_eq!(s.provider, "codex");
        assert!(s.show_cost);
        assert_eq!(s.notify_at, vec![80, 95]);
        assert!(s.is_on("claude"));
        assert_eq!(s.main, "claude");
        let s: Settings = serde_json::from_str("{\"off\":[\"gemini\"]}").unwrap();
        assert!(!s.is_on("gemini"));
    }
}
