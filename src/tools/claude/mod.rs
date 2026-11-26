//! AI usage tool (id "claude" for saved prefs): plan limits and local token
//! use of the main AI coding assistant, Claude Code or Codex, picked on the
//! page. A panel card, a limits sheet, a page in the full window and the
//! session % next to the menu-bar icon. Claude also lists its projects.

use crate::kit::sync::LockExt;
pub mod backend;
mod page;
mod parts;
pub mod settings_ui;
mod ui;

use crate::core::tool::{Action, Badge, Cx, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::glass::{tint, Icon};
use crate::kit::ai::{self, UsageInfo};
use egui::{Rect, Ui};

pub const ID: ToolId = "claude";
const LIMITS: &str = "limits";

/// Connecting to Claude Code's sign-in in the Keychain (setup's Sign-ins
/// step and Settings › AI Usage; nothing reads it before that). The macOS
/// prompt is up while busy.
pub mod keychain {
    use crate::kit::ai::{self, claude::Connect};
    use crate::kit::sync::LockExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static BUSY: AtomicBool = AtomicBool::new(false);
    /// The macOS prompt was answered: our window goes back in front (it can
    /// end up behind another app's window).
    static ANSWERED: AtomicBool = AtomicBool::new(false);
    static ERROR: Mutex<String> = Mutex::new(String::new());
    /// Whether the item exists, looked up at most every 30 s (setup draws
    /// the permission rows every frame).
    static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

    fn exists() -> bool {
        let mut g = SEEN.lock_ok();
        match *g {
            Some((at, v)) if at.elapsed() < Duration::from_secs(30) => v,
            _ => {
                let v = ai::account::claude_keychain_exists();
                *g = Some((Instant::now(), v));
                v
            }
        }
    }

    /// Whether to offer it: Claude is tracked and Claude Code keeps its
    /// sign-in in the Keychain (or it's connected already).
    pub fn wanted(demo: bool) -> bool {
        let set = ai::settings::get();
        demo || ((set.is_on("claude") || set.main == "claude") && (set.claude_keychain == "ok" || exists()))
    }

    pub fn status() -> crate::kit::perms::Status {
        use crate::kit::perms::Status;
        match ai::settings::get().claude_keychain.as_str() {
            "ok" => Status::Allowed,
            "denied" => Status::Denied,
            _ => Status::NotAsked,
        }
    }

    pub fn busy() -> bool {
        BUSY.load(Ordering::Relaxed)
    }

    pub fn error() -> String {
        ERROR.lock_ok().clone()
    }

    /// Bring the window back in front once the prompt was answered (call
    /// every frame where the Allow / Connect button is drawn).
    pub fn refocus(ctx: &egui::Context) {
        if ANSWERED.swap(false, Ordering::Relaxed) {
            crate::kit::native::activate();
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Read it now (macOS asks), off the ui thread; limits refresh after.
    pub fn connect(ctx: Option<egui::Context>) {
        if BUSY.swap(true, Ordering::Relaxed) {
            return;
        }
        ERROR.lock_ok().clear();
        std::thread::spawn(move || {
            let r = ai::claude::connect_keychain();
            eprintln!("[ai] claude keychain: {r:?}");
            *ERROR.lock_ok() = match r {
                Connect::Ok | Connect::Denied => String::new(),
                Connect::Missing => ts!("ai.err.sign_in_first").into(),
                Connect::Failed => ts!("ai.err.keychain").into(),
            };
            if r == Connect::Ok {
                super::backend::request_refresh();
            }
            BUSY.store(false, Ordering::Relaxed);
            ANSWERED.store(true, Ordering::Relaxed);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }
}

pub struct Claude {
    all: backend::All,
    demo: bool,
    /// What to open on the page: [`LIMITS`] opens the Limits tab.
    route: Option<String>,
    /// The tips chip: Claude Code's own /usage tips and AI Check.
    ai: crate::kit::aicheck::Check,
}

impl Claude {
    pub fn new(env: &ToolEnv) -> Self {
        Claude { all: backend::start(env.ctx.clone(), env.demo), demo: env.demo, route: None, ai: crate::kit::aicheck::Check::new(ID, env.demo) }
    }
}

impl Claude {
    /// The provider shown on the page and in the widget.
    fn info(&self) -> UsageInfo {
        backend::current(&self.all).lock_ok().clone()
    }
}

fn badge() -> Badge {
    Badge { icon: Icon::Gauge, color: tint::CLAUDE() }
}

impl Tool for Claude {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        ts!("ai.name")
    }
    fn about(&self) -> &'static str {
        ts!("ai.about")
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn section(&self) -> &'static str {
        "Monitor"
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: LIMITS, title: ts!("ai.widget_title"), about: ts!("ai.widget_about"), sizes: UP_TO_MEDIUM, size: Size::S14, default_on: true }]
    }
    fn widget_ui(&mut self, widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        if widget != LIMITS {
            return;
        }
        // W13: the first provider in the person's order that's on and installed
        let cl = backend::widget_pick(&self.all).map(|k| self.all[k].lock_ok().clone()).unwrap_or_else(|| self.info());
        let all: Vec<_> = backend::enabled(&self.all).into_iter().filter(|i| i.unavailable() != Some(crate::kit::claude_code::Availability::NotInstalled)).collect();
        if ui::widget(ui, rect, size, &cl, &all, false).clicked() {
            cx.open_page(ID, "");
        }
    }
    fn set_on(&mut self, on: bool) {
        crate::kit::claude_code::set_tool_on(on);
        if on {
            backend::request_refresh();
        }
    }
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        match self.route.take().as_deref() {
            Some(LIMITS) => page::set_tab(ui.ctx(), page::Tab::Limits),
            Some("cost") => page::set_tab(ui.ctx(), page::Tab::Cost),
            Some("tokens") => page::set_tab(ui.ctx(), page::Tab::Tokens),
            _ => {}
        }
        page::page(&self.all, &mut self.ai, self.demo, ui, rect, cx);
    }
    fn open_page(&mut self, route: &str) {
        self.route = (!route.is_empty()).then(|| route.to_string());
    }
    fn recommendations(&self) -> Vec<Recommendation> {
        let info = self.info();
        let Some(l) = info.session().filter(|l| l.pct >= 80.0) else { return Vec::new() };
        vec![Recommendation {
            priority: 60,
            title: t!("ai.rec.title", provider = info.provider).into_owned(),
            body: t!("ai.rec.body", pct = l.pct : {:.0}, when = parts::reset_when(l)).into_owned(),
            action: ts!("ai.rec.action").into(),
            go: Action::OpenPage { tool: ID.into(), route: LIMITS.into() },
        }]
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "claude-detail" => {
                self.route = Some(LIMITS.into());
                Some(View::Page)
            }
            "claude" => Some(View::Page),
            // the tips chip open: Claude Code's /usage tips and two AI Check tips
            "claude-tips" => {
                use crate::kit::aicheck::Check;
                self.ai.set_demo(
                    "",
                    vec![
                        Check::demo_rec_for("limit:Claude:Current week (all models)", "show", ts!("ai.ac.show"), "Week runs out Fri 19:10", "81%", "Opus did 81% of this week; Sonnet for routine edits", "Claude"),
                        Check::demo_rec_for("caches", "clean", ts!("ai.ac.clean"), "Claude caches · 2.1 GB", "2.1 GB", "Rebuilt when needed", "Claude"),
                    ],
                );
                Some(View::Page)
            }
            "claude-cost" | "claude-tokens" => {
                self.route = Some(view.trim_start_matches("claude-").into());
                Some(View::Page)
            }
            "codex" | "codex-detail" | "gemini" | "all" => {
                backend::select(ai::index_of(view.trim_end_matches("-detail")), true);
                if view.ends_with("-detail") {
                    self.route = Some(LIMITS.into());
                }
                Some(View::Page)
            }
            "claude-api" => {
                let mut g = self.all[0].lock_ok();
                g.billing = ai::Billing::Api;
                g.plan.clear();
                g.limits.clear();
                ai::settings::update(false, |s| {
                    s.budgets.insert(ai::claude::NAME.into(), 500.0);
                });
                Some(View::Page)
            }
            "claude-missing" => {
                let error = Some(t!("ai.perr.claude_missing", how = crate::kit::claude_code::Availability::NotInstalled.how()).into_owned());
                *self.all[0].lock_ok() = UsageInfo { provider: ai::claude::NAME, loading: false, scanned: true, installed: false, fetched: Some(chrono::Local::now()), error, ..Default::default() };
                Some(View::Page)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keychain;
    use crate::kit::ai::{self, claude::Connect};
    use crate::kit::perms::Status;

    #[test]
    fn keychain_row_and_status() {
        let set = |state: &str, main: &str, off: &[&str]| {
            let (state, main, off) = (state.to_string(), main.to_string(), off.iter().map(|s| s.to_string()).collect());
            ai::settings::update(false, move |s| {
                s.claude_keychain = state;
                s.main = main;
                s.off = off;
            });
        };
        // connected: the row shows as allowed while Claude is tracked
        set("ok", "claude", &[]);
        assert!(keychain::wanted(false));
        assert_eq!(keychain::status(), Status::Allowed);
        set("ok", "codex", &["claude"]);
        assert!(!keychain::wanted(false));
        set("denied", "claude", &[]);
        assert_eq!(keychain::status(), Status::Denied);
        set("", "claude", &[]);
        assert_eq!(keychain::status(), Status::NotAsked);
        // screenshots always show it
        assert!(keychain::wanted(true));
        // no sign-in in the Keychain (always so off macOS): nothing is remembered
        if !cfg!(target_os = "macos") {
            assert!(!keychain::wanted(false));
            assert_eq!(ai::claude::connect_keychain(), Connect::Missing);
            assert_eq!(ai::settings::get().claude_keychain, "");
        }
        set("", "claude", &[]);
    }
}
