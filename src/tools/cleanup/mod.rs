//! Cleanup tool: finds junk by rule, lets Claude judge each path, and moves
//! what you approve to the Trash. Lives in the full window.
//!
//! Page routes: "" (as left), "claude" (open on the Claude group).

use crate::kit::sync::LockExt;
pub use crate::kit::agent;
mod agents;
pub mod auto;
mod auto_page;
mod captures;
pub mod engine;
mod inuse;
mod journal;
mod projects;
mod rules;
mod run;
mod xcode;
mod page;

use crate::core::tool::{Action, Badge, Cx, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec};
use crate::kit::glass::{human_bytes, tint, Icon, ACCENT};
use crate::kit::tile;
use egui::{Rect, Ui};
use engine::Group;

pub const ID: ToolId = "cleanup";

pub struct Cleanup {
    pub(crate) clean: engine::Shared,
    pub(crate) demo: bool,
    pub(crate) use_ai: bool,
    /// The agent that checks the paths: the main agent from setup / Settings › AI Usage.
    pub(crate) agent: agent::Agent,
    pub(crate) delete_now: bool,
    /// Group shown in the results; `None` = "Needs your OK".
    pub(crate) clean_group: Option<Group>,
    /// One tool picked under AI Tools in the rail (`Item::tool`); "" = all.
    pub(crate) clean_tool: &'static str,
    pub(crate) clean_confirm: bool,
    /// Open apps with selected files, on the confirm sheet.
    pub(crate) quit: Vec<page::QuitRow>,
    /// "Quit all and clean" was pressed: clean once they're all closed.
    pub(crate) quit_then_clean: bool,
    pub(crate) icons: std::collections::HashMap<std::path::PathBuf, Option<egui::TextureHandle>>,
    /// The removal loader is up for the clean in progress (or just done).
    pub(crate) clean_loading: bool,
    pub(crate) clean_log: bool,
    /// Screenshots: show Cleanup as if Claude were in this state.
    pub(crate) demo_claude: Option<crate::kit::claude_code::Availability>,
    pub(crate) ai: crate::kit::aicheck::Check,
    /// Screenshots: the page before any scan.
    pub(crate) demo_fresh: bool,
    /// The Auto tab is showing.
    pub(crate) auto_tab: bool,
    pub(crate) auto: auto::Settings,
    pub(crate) auto_run: auto::Runner,
}

impl Cleanup {
    /// What the footer's clean (and the ticked total) covers right now.
    pub(crate) fn scope(&self) -> engine::Scope {
        engine::Scope { group: self.clean_group, tool: self.clean_tool }
    }
    pub fn new(env: &ToolEnv) -> Self {
        Cleanup {
            clean: Default::default(),
            demo: env.demo,
            use_ai: false,
            agent: if env.demo { agent::Agent::Claude } else { agent::main() },
            delete_now: false,
            clean_group: None,
            clean_tool: "",
            clean_confirm: false,
            quit: Vec::new(),
            quit_then_clean: false,
            icons: Default::default(),
            clean_loading: false,
            clean_log: false,
            demo_claude: None,
            ai: {
                // answers go on the cards, tips under the list
                let mut c = crate::kit::aicheck::Check::new(ID, env.demo);
                c.inline = true;
                c
            },
            demo_fresh: false,
            auto_tab: false,
            auto: if env.demo { auto::demo() } else { auto::Settings::load() },
            auto_run: Default::default(),
        }
    }
}

fn badge() -> Badge {
    Badge { icon: Icon::Broom, color: tint::CLEANUP() }
}

impl Tool for Cleanup {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        ts!("cleanup.name")
    }
    fn about(&self) -> &'static str {
        ts!("cleanup.about")
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::FullDisk)]
    }

    fn section(&self) -> &'static str {
        "Clean"
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: "clean", title: ts!("cleanup.short"), about: ts!("cleanup.widget_about"), sizes: &[Size::S12, Size::S14], size: Size::S12, default_on: true }]
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        let g = self.clean.lock_ok().clone();
        let (value, sub, frac) = match g.phase {
            engine::Phase::Results => (human_bytes(g.total()), ts!("cleanup.w.to_review").to_string(), None),
            engine::Phase::Scanning | engine::Phase::Judging => (ts!("cleanup.scanning").into(), g.current.clone(), Some(g.shown_progress())),
            engine::Phase::Cleaning => (ts!("cleanup.cleaning").into(), g.current.clone(), Some(g.progress)),
            engine::Phase::Done => (human_bytes(g.cleaned_bytes), ts!("cleanup.w.cleaned").into(), None),
            engine::Phase::Intro => (ts!("cleanup.scan").into(), String::new(), None),
        };
        // before a scan: what past cleans removed (the disk itself is Storage's)
        let hist = match g.phase {
            // samples (setup's preview, screenshots): a few past cleans
            engine::Phase::Intro if self.demo && !self.demo_fresh => {
                let now = chrono::Utc::now().timestamp();
                vec![(now - 20 * 86400, 6_100_000_000), (now - 9 * 86400, 4_200_000_000), (now - 3 * 86400, 2_100_000_000)]
            }
            engine::Phase::Intro => engine::history(),
            _ => Vec::new(),
        };
        let last = hist.last().copied();
        let freed: u64 = hist.iter().map(|h| h.1).sum();
        let intro = g.phase == engine::Phase::Intro;
        let (value, sub) = match last {
            Some(_) if intro => (human_bytes(freed), ts!("cleanup.w.freed_so_far").to_string()),
            None if intro && size != Size::S12 => (ts!("cleanup.w.not_cleaned").to_string(), String::new()),
            _ => (value, sub),
        };
        let id = ui.scope_id().with("w-clean");
        let col = tint::CLEANUP();
        let ring_ = frac.map(|f| (f.clamp(0.01, 1.0), ACCENT()));
        let never = intro && hist.is_empty();
        // W7: no lime button in the panel; never cleaned, the card itself opens Clean
        let (value, sub) = if never { (ts!("cleanup.w.not_cleaned").to_string(), String::new()) } else { (value, sub) };
        let resp = match size {
            Size::S12 => tile::wide_full(ui, rect, id, Icon::Broom, col, ring_, ts!("cleanup.short"), &value, &sub, crate::kit::glass::ty::caption_color(), false),
            _ => {
                let (resp, right) = tile::row_split_full(ui, rect, id, Icon::Broom, col, ring_, ts!("cleanup.short"), &value, &sub, crate::kit::glass::ty::caption_color(), false);
                let p = ui.painter();
                let lines: Vec<(String, Option<(f32, egui::Color32)>, String)> = match g.phase {
                    engine::Phase::Results => {
                        let tot = g.total().max(1) as f32;
                        let mut groups: Vec<(engine::Group, u64)> = engine::Group::ALL.iter().map(|k| (*k, g.group_bytes(*k))).filter(|(_, b)| *b > 0).collect();
                        groups.sort_by_key(|g| std::cmp::Reverse(g.1));
                        groups.iter().take(3).map(|(grp, b)| (grp.label().to_string(), Some(((*b as f32 / tot).clamp(0.01, 1.0), ACCENT())), human_bytes(*b))).collect()
                    }
                    engine::Phase::Scanning | engine::Phase::Judging | engine::Phase::Cleaning => vec![(ts!("cleanup.w.progress").into(), frac.map(|f| (f.clamp(0.01, 1.0), ACCENT())), format!("{:.0}%", frac.unwrap_or(0.0) * 100.0))],
                    engine::Phase::Intro if !hist.is_empty() => vec![
                        (ts!("cleanup.w.last").into(), None, last.map(|l| engine::days_ago(l.0)).unwrap_or_default()),
                        (ts!("cleanup.w.freed").into(), None, last.map(|l| human_bytes(l.1)).unwrap_or_default()),
                    ],
                    _ => Vec::new(),
                };
                let empty = if g.phase == engine::Phase::Done { ts!("cleanup.w.all_clean") } else if never { ts!("cleanup.w.never") } else { "" };
                tile::lines(p, right, &lines, empty);
                resp
            }
        };
        if resp.clicked() {
            cx.open_page(ID, "");
        }
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        if route == "claude" {
            self.clean_group = None;
            self.clean_tool = "";
        }
    }
    fn busy(&self) -> bool {
        let scanning = matches!(self.clean.lock_ok().phase, engine::Phase::Scanning | engine::Phase::Judging | engine::Phase::Cleaning);
        scanning || self.ai.phase() == crate::kit::aicheck::Phase::Running
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        if !self.demo {
            let busy = self.busy();
            self.auto_run.tick(&mut self.auto, busy, ctx);
            if self.auto.active() || self.auto_run.busy {
                ctx.request_repaint_after(std::time::Duration::from_secs(if self.auto_run.busy { 1 } else { 60 }));
            }
        }
        false
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        page::page(self, ui, rect, cx);
    }
    fn recommendations(&self) -> Vec<Recommendation> {
        let go = Action::OpenPage { tool: ID.into(), route: String::new() };
        // always on offer, at the lowest priority
        let mut v = vec![Recommendation {
            priority: 0,
            title: ts!("cleanup.rec.title").into(),
            body: if self.demo || crate::kit::claude_code::availability().ready() {
                ts!("cleanup.rec.body_claude").into()
            } else {
                ts!("cleanup.rec.body").into()
            },
            action: ts!("cleanup.rec.action").into(),
            go: go.clone(),
        }];
        let claude_junk = self.clean.lock_ok().claude_safe_bytes();
        if claude_junk > 50_000_000 {
            v.push(Recommendation {
                priority: 4,
                title: ts!("cleanup.rec.claude_title").into(),
                body: t!("cleanup.rec.claude_body", size = human_bytes(claude_junk)).into_owned(),
                action: ts!("cleanup.rec.claude_action").into(),
                go: Action::OpenPage { tool: ID.into(), route: "claude".into() },
            });
        }
        v
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "cleanup" => Some(View::Page),
            "cleanup-auto" => {
                self.auto_tab = true;
                Some(View::Page)
            }
            "cleanup-fresh" => {
                self.demo_fresh = true;
                Some(View::Page)
            }
            "cleanup-no-claude" => {
                self.demo_claude = Some(crate::kit::claude_code::Availability::NotInstalled);
                Some(View::Page)
            }
            "results" => {
                *self.clean.lock_ok() = page::demo_results();
                let rec = |key: &str, action: &str, label: &str, title: &str, value: &str, why: &str, name: &str| crate::kit::aicheck::Rec {
                    key: key.into(),
                    action: action.into(),
                    label: label.into(),
                    title: title.into(),
                    value: value.into(),
                    why: why.into(),
                    icon: None,
                    name: name.into(),
                };
                let asks: Vec<String> = self.clean.lock_ok().items.iter().filter(|i| i.verdict == engine::Verdict::Ask).map(|i| i.path.display().to_string()).collect();
                let tip = |key: &str, action: &str, label: &str, title: &str, value: &str, why: &str, name: &str| crate::kit::aicheck::Rec { key: key.into(), ..rec(key, action, label, title, value, why, name) };
                let xcode = self.clean.lock_ok().items.iter().find(|i| i.path.components().any(|c| c.as_os_str() == "DerivedData")).map(|i| i.path.display().to_string()).unwrap_or_default();
                self.ai.set_demo(
                    "Both questions answered, one more tip",
                    vec![
                        tip(&asks[0], "select", "Clean", "Claude Desktop VM image", "9.00 GB", "Sandbox image; Claude downloads it again when needed", "Claude"),
                        tip(&asks[1], "keep", "Keep", "Project · old-experiment", "412 MB", "The folder is gone, but you opened this chat 3 days ago", "Claude"),
                        tip(&xcode, "keep", "Keep", "Keep MyApp build data", "3.40 GB", "You built MyApp today; rebuilding takes 20 minutes", "Xcode"),
                    ],
                );
                self.ai.answer = asks.into_iter().collect();
                self.ai.actions = vec![("select", "Clean"), ("keep", "Keep")];
                self.ai.open = false;
                Some(View::Page)
            }
            "results-captures" => {
                let v = self.demo_view("results");
                self.clean_group = Some(engine::Group::Captures);
                v
            }
            // one group picked: the footer cleans only it
            "results-group" | "results-group-confirm" => {
                let v = self.demo_view(if view == "results-group" { "results" } else { "results-confirm" });
                self.clean_group = Some(engine::Group::AppCaches);
                if self.clean_confirm {
                    let items = self.clean.lock_ok().items.clone();
                    self.quit = page::quit_rows(&items, page::demo_apps(), self.scope());
                }
                v
            }
            // a clean where open apps kept some items back
            "cleanup-done-failed" => {
                let mut st = page::demo_done();
                let fails = [
                    ("Notion Calendar", "a running app has files in it open"),
                    ("Spotify", "the app was opened after the scan"),
                    ("Slack", "a running app has files in it open"),
                    ("Google Chrome · Code Cache", "a running app has files in it open"),
                    ("Telegram media cache", "a running app has files in it open"),
                ];
                st.failed = fails.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
                st.in_way = true;
                *self.clean.lock_ok() = st;
                Some(View::Page)
            }
            // back from cleaning one group: the rest waits, the cleaned group says so
            "results-after-group" => {
                let v = self.demo_view("results");
                let mut g = self.clean.lock_ok();
                let gone: u64 = g.items.iter().filter(|i| i.group == engine::Group::AppCaches).map(|i| i.bytes).sum();
                g.items.retain(|i| i.group != engine::Group::AppCaches);
                g.cleaned_groups = vec![(engine::Group::AppCaches, gone)];
                v
            }
            "cleanup-done-group" => {
                *self.clean.lock_ok() = page::demo_done_group();
                Some(View::Page)
            }
            "results-confirm" => {
                let v = self.demo_view("results");
                let items = self.clean.lock_ok().items.clone();
                self.quit = page::quit_rows(&items, page::demo_apps(), self.scope());
                // one of each state
                let states = [page::Quit::Stuck, page::Quit::Open, page::Quit::Closed];
                for (row, st) in self.quit.iter_mut().zip(states) {
                    row.state = st;
                }
                self.clean_confirm = true;
                v
            }
            "cleanup-scanning" => {
                // mid-scan: some groups measured, some still going
                let mut st = engine::State { phase: engine::Phase::Scanning, with_ai: true, listed: true, progress: 0.46, current: "/Users/me/Library/Caches/com.spotify.client".into(), ..Default::default() };
                st.live = [
                    engine::GroupLive { bytes: 4_200_000_000, items: 9, total: 14, done: 14 },
                    engine::GroupLive { bytes: 1_900_000_000, items: 22, total: 61, done: 30 },
                    engine::GroupLive { bytes: 2_600_000_000, items: 3, total: 8, done: 5 },
                    engine::GroupLive { bytes: 1_100_000_000, items: 4, total: 6, done: 2 },
                    engine::GroupLive { bytes: 0, items: 0, total: 2, done: 0 },
                    engine::GroupLive { bytes: 0, items: 0, total: 4, done: 1 },
                    engine::GroupLive { bytes: 0, items: 0, total: 3, done: 3 },
                    engine::GroupLive { bytes: 0, items: 0, total: 1, done: 0 },
                ];
                *self.clean.lock_ok() = st;
                Some(View::Page)
            }
            "cleanup-done" => {
                *self.clean.lock_ok() = page::demo_done();
                Some(View::Page)
            }
            _ => None,
        }
    }
}

/// Read-only check of the Cleanup rules on this Mac: scans, optionally asks
/// Claude, prints what it would offer. Nothing is removed.
pub fn scan_report(ai: bool) {
    let state: engine::Shared = Default::default();
    engine::start_scan(state.clone(), egui::Context::default(), ai.then(agent::main));
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if state.lock_ok().phase == engine::Phase::Results {
            break;
        }
    }
    let g = state.lock_ok();
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    for it in &g.items {
        println!(
            "{:<12} {:>10} {:<14} {:<5} {} | {}",
            format!("{:?}", it.group),
            human_bytes(it.bytes),
            format!("{:?}", it.verdict),
            format!("{:?}", it.by),
            it.path.display().to_string().replacen(&home, "~", 1),
            it.reason
        );
    }
    println!("total offered {} · ai used {} · ai error {:?}", human_bytes(g.total()), g.ai_used, g.ai_error);
}
