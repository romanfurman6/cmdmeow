//! Auto clean: on a schedule (or when the disk is almost full) Cleanup scans
//! the groups picked here and removes only what the built-in rules call
//! safe and rebuildable. Anything that needs a decision stays for the
//! person. Runs only while the Mac is idle and nothing is recording.

use super::engine::{self, Group, Verdict};
use crate::kit::sync::LockExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Every {
    #[default]
    Off,
    Daily,
    Weekly,
    Monthly,
}

impl Every {
    pub const ALL: [Every; 4] = [Every::Off, Every::Daily, Every::Weekly, Every::Monthly];
    pub fn label(self) -> &'static str {
        match self {
            Every::Off => ts!("cleanup.auto.off"),
            Every::Daily => ts!("cleanup.auto.daily"),
            Every::Weekly => ts!("cleanup.auto.weekly"),
            Every::Monthly => ts!("cleanup.auto.monthly"),
        }
    }
    /// Seconds between runs; `None` when off.
    pub fn secs(self) -> Option<i64> {
        match self {
            Every::Off => None,
            Every::Daily => Some(86_400),
            Every::Weekly => Some(7 * 86_400),
            Every::Monthly => Some(30 * 86_400),
        }
    }
}

/// Groups auto clean may touch; the rest always need the person.
pub const GROUPS: [Group; 5] = [Group::AiTools, Group::AppCaches, Group::Developer, Group::Logs, Group::Trash];

/// Free space under this share of the disk counts as almost full.
pub const LOW_DISK: f64 = 0.10;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub every: Every,
    pub low_disk: bool,
    pub notify: bool,
    /// `GROUPS` that are on, by title.
    pub groups: Vec<String>,
    pub runs: Vec<Run>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { every: Every::Off, low_disk: false, notify: true, groups: [Group::AiTools, Group::AppCaches, Group::Developer, Group::Logs].iter().map(|g| g.title().to_string()).collect(), runs: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub when: i64,
    pub bytes: u64,
    pub items: usize,
    /// Started because the disk was almost full.
    #[serde(default)]
    pub low_disk: bool,
}

/// Runs kept in the history.
const KEEP_RUNS: usize = 20;

fn path() -> std::path::PathBuf {
    crate::kit::fs::data_path("cleanup").join("auto.json")
}

impl Settings {
    pub fn load() -> Self {
        crate::kit::fs::load_json(&path())
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::kit::fs::data_path("cleanup"));
        crate::kit::fs::save_json(&path(), self, true);
    }
    pub fn on(&self, g: Group) -> bool {
        self.groups.iter().any(|t| t == g.title())
    }
    pub fn flip(&mut self, g: Group) {
        if self.on(g) {
            self.groups.retain(|t| t != g.title());
        } else {
            self.groups.push(g.title().to_string());
        }
    }
    pub fn picked(&self) -> Vec<Group> {
        GROUPS.into_iter().filter(|g| self.on(*g)).collect()
    }
    pub fn active(&self) -> bool {
        (self.every != Every::Off || self.low_disk) && !self.picked().is_empty()
    }
    pub fn last(&self) -> Option<&Run> {
        self.runs.last()
    }
    pub fn record(&mut self, r: Run) {
        self.runs.push(r);
        let n = self.runs.len();
        if n > KEEP_RUNS {
            self.runs.drain(..n - KEEP_RUNS);
        }
    }
    /// Why a run is due at `now` with `free_share` of the disk free, if it is.
    /// The disk trigger fires at most once a day.
    pub fn due(&self, now: i64, free_share: f64) -> Option<bool> {
        if !self.active() {
            return None;
        }
        let last = self.last().map(|r| r.when);
        if let Some(every) = self.every.secs() {
            if last.is_none_or(|l| now - l >= every) {
                return Some(false);
            }
        }
        if self.low_disk && free_share < LOW_DISK && last.is_none_or(|l| now - l >= 86_400) {
            return Some(true);
        }
        None
    }
}

/// What auto clean removes from a finished scan: only what the rules call
/// safe, the person never kept, no app holds and that the run's groups cover.
pub fn eligible(it: &engine::Item, groups: &[Group]) -> bool {
    groups.contains(&it.group)
        && !it.hard_keep
        && !it.app_running
        && it.verdict == Verdict::Safe
        && it.rule_verdict == Verdict::Safe
        && (it.group == Group::Trash || it.rebuilds())
}

/// Untick what an open app's files hold: the same items the manual clean's
/// sheet would ask to quit that app for. An auto run never asks, so it skips them.
pub fn skip_open_apps(items: &mut [engine::Item], apps: Vec<crate::kit::native::OpenApp>) {
    let held: Vec<usize> = super::page::quit_rows(items, apps, engine::Scope::default()).into_iter().flat_map(|r| r.idx).collect();
    for i in held {
        items[i].selected = false;
    }
}

/// Seconds since the last keyboard, mouse or trackpad input.
pub fn idle_secs() -> f64 {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
        }
        // 1: the hardware's own state; !0: any input
        unsafe { CGEventSourceSecondsSinceLastEventType(1, u32::MAX) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        f64::MAX
    }
}

/// Idle this long before a run starts.
pub const IDLE_SECS: f64 = 120.0;

/// The auto run in progress, apart from the page's own scan.
#[derive(Default)]
pub struct Runner {
    pub shared: engine::Shared,
    pub busy: bool,
    pub low_disk: bool,
    /// When the schedule was last checked.
    pub checked: Option<std::time::Instant>,
}

impl Runner {
    /// Called every pass; checks once a minute and starts a run when due.
    pub fn tick(&mut self, s: &mut Settings, page_busy: bool, ctx: &egui::Context) {
        if self.busy {
            self.poll(s, ctx);
            return;
        }
        if self.checked.is_some_and(|t| t.elapsed().as_secs() < 60) {
            return;
        }
        self.checked = Some(std::time::Instant::now());
        if page_busy || crate::kit::activity::recording() || idle_secs() < IDLE_SECS {
            return;
        }
        let (total, free) = crate::kit::fs::volume_space("/");
        let share = if total > 0 { free as f64 / total as f64 } else { 1.0 };
        if let Some(low) = s.due(chrono::Utc::now().timestamp(), share) {
            self.start(s, low, ctx);
        }
    }

    pub fn start(&mut self, s: &Settings, low_disk: bool, ctx: &egui::Context) {
        if self.busy || s.picked().is_empty() {
            return;
        }
        self.busy = true;
        self.low_disk = low_disk;
        engine::start_auto(self.shared.clone(), ctx.clone(), s.picked());
    }

    fn poll(&mut self, s: &mut Settings, _ctx: &egui::Context) {
        let g = self.shared.lock_ok();
        if g.phase != engine::Phase::Done {
            return;
        }
        let run = Run { when: chrono::Utc::now().timestamp(), bytes: g.cleaned_bytes, items: g.cleaned_count, low_disk: self.low_disk };
        drop(g);
        if s.notify && run.bytes > 0 && crate::kit::notify::allowed() {
            crate::kit::notify::send(ts!("cleanup.name"), &t!("cleanup.auto.freed", size = crate::kit::glass::human_bytes(run.bytes)));
        }
        s.record(run);
        s.save();
        self.busy = false;
    }
}

/// Sample settings for screenshots.
pub fn demo() -> Settings {
    let now = chrono::Utc::now().timestamp();
    let mut s = Settings { every: Every::Weekly, low_disk: true, ..Default::default() };
    for (d, b, n, low) in [(23, 3_100_000_000u64, 214, false), (16, 5_600_000_000, 388, false), (11, 2_400_000_000, 97, true), (9, 4_900_000_000, 341, false), (2, 4_200_000_000, 312, false)] {
        s.record(Run { when: now - d * 86_400 - 3 * 3600, bytes: b, items: n, low_disk: low });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_never_runs() {
        let s = Settings::default();
        assert_eq!(s.due(1_000_000, 0.01), None);
    }

    #[test]
    fn weekly_runs_once_a_week() {
        let mut s = Settings { every: Every::Weekly, ..Default::default() };
        assert_eq!(s.due(1_000_000, 0.5), Some(false));
        s.record(Run { when: 1_000_000, bytes: 1, items: 1, low_disk: false });
        assert_eq!(s.due(1_000_000 + 6 * 86_400, 0.5), None);
        assert_eq!(s.due(1_000_000 + 7 * 86_400, 0.5), Some(false));
    }

    #[test]
    fn low_disk_at_most_daily() {
        let mut s = Settings { low_disk: true, ..Default::default() };
        assert_eq!(s.due(1_000_000, 0.5), None);
        assert_eq!(s.due(1_000_000, 0.05), Some(true));
        s.record(Run { when: 1_000_000, bytes: 1, items: 1, low_disk: true });
        assert_eq!(s.due(1_000_000 + 3600, 0.05), None);
        assert_eq!(s.due(1_000_000 + 86_400, 0.05), Some(true));
    }

    #[test]
    fn eligible_only_safe_rebuildable_in_picked_groups() {
        use crate::tools::cleanup::engine::{tests::it, Who};
        let gs = [Group::AppCaches, Group::Logs];
        let safe = it(Group::AppCaches, "/c", "Caches", Verdict::Safe, Who::Rule);
        assert!(eligible(&safe, &gs));
        // a group not picked
        assert!(!eligible(&it(Group::Developer, "/d", "Caches", Verdict::Safe, Who::Rule), &gs));
        // needs the person
        assert!(!eligible(&it(Group::AppCaches, "/a", "Caches", Verdict::Ask, Who::Rule), &gs));
        // not rebuilt by its app
        assert!(!eligible(&it(Group::AppCaches, "/s", "Stale Versions", Verdict::Safe, Who::Rule), &gs));
        // the person said so before, an app holds it, or it's hard kept
        assert!(!eligible(&it(Group::AppCaches, "/y", "Caches", Verdict::Safe, Who::You), &gs));
        let mut open = safe.clone();
        open.app_running = true;
        assert!(!eligible(&open, &gs));
        let mut hard = safe.clone();
        hard.hard_keep = true;
        assert!(!eligible(&hard, &gs));
        // the rules weren't sure, even if the item now says safe
        let mut relaxed = safe.clone();
        relaxed.rule_verdict = Verdict::Ask;
        assert!(!eligible(&relaxed, &gs));
        // the Trash only when picked
        let trash = it(Group::Trash, "/t", "Trash", Verdict::Safe, Who::Rule);
        assert!(!eligible(&trash, &gs));
        assert!(eligible(&trash, &[Group::Trash]));
    }

    #[test]
    fn skips_what_an_open_app_holds() {
        use crate::tools::cleanup::engine::{tests::it, Who};
        let claude = crate::kit::native::OpenApp { pid: 1, id: "com.anthropic.claudefordesktop".into(), name: "Claude".into(), bundle: None };
        let mut items = vec![
            it(Group::AiTools, "/u/Library/Caches/com.anthropic.claudefordesktop", "Caches", Verdict::Safe, Who::Rule),
            it(Group::AiTools, "/u/.claude/cache", "Caches", Verdict::Safe, Who::Rule),
            it(Group::AppCaches, "/u/Library/Caches/com.other.app", "Caches", Verdict::Safe, Who::Rule),
        ];
        items[1].title = "Claude cache".into();
        skip_open_apps(&mut items, vec![claude]);
        assert!(!items[0].selected && !items[1].selected);
        assert!(items[2].selected);
    }

    #[test]
    fn no_groups_no_run() {
        let s = Settings { every: Every::Daily, groups: vec![], ..Default::default() };
        assert_eq!(s.due(1_000_000, 0.01), None);
    }
}
