//! Keep Awake: keeps the Mac from sleeping, by hand with one switch, or by
//! itself while the main AI agent (Claude Code, Codex, Gemini CLI or
//! OpenCode, picked in Settings › AI Usage) is working, and a few minutes
//! after, so a pause between prompts doesn't put the Mac to sleep. Uses macOS power
//! assertions, the same thing `caffeinate` uses, without a helper process.
//!
//! Page routes: "" only.

use crate::kit::sync::LockExt;
mod engine;
mod power;
mod sessions;
mod ui;

use crate::core::tool::{Badge, Cx, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::glass::{tint, Icon};
use egui::{Rect, Ui};
use engine::{now, Engine, State};
use crate::kit::hotkeys::{Keys, Shortcut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ID: ToolId = "keep_awake";
const CARD: &str = "card";

/// How often the agent's logs are checked and the state is brought up to date.
const POLL: Duration = Duration::from_secs(5);
/// With folder events on, the full read that catches anything they missed.
const RESCAN: Duration = Duration::from_secs(60);

type Shared = Arc<Mutex<Engine>>;

pub struct KeepAwake {
    engine: Shared,
    /// Sample data for screenshots: the clock is frozen at this time.
    demo_now: Option<i64>,
    pub keys: KeyRec,
}

/// The switch's shortcut: what is bound, and the recorder on the page.
#[derive(Default)]
pub struct KeyRec {
    keys: Option<Keys<()>>,
    bound: Option<Shortcut>,
    pub error: Option<String>,
    /// Recording a new shortcut on the page.
    pub recording: bool,
    /// When the recorder was last drawn; it stops once the page is gone.
    pub seen: Option<Instant>,
}

impl KeyRec {
    /// Bind `want` (None while recording or off). Cheap when nothing changed.
    fn bind(&mut self, ctx: &egui::Context, want: Option<Shortcut>) {
        if self.recording && self.seen.is_some_and(|t| t.elapsed() > Duration::from_secs(1)) {
            self.recording = false;
        }
        let want = if self.recording { None } else { want };
        if self.bound == want && (self.keys.is_some() || want.is_none()) {
            return;
        }
        if self.keys.is_none() {
            self.keys = Keys::new(ctx.clone());
        }
        let Some(keys) = self.keys.as_mut() else {
            self.error = Some("Shortcuts are unavailable".into());
            return;
        };
        self.error = None;
        match &want {
            Some(s) => self.error = keys.apply(&[((), s.clone())]).into_iter().next().map(|(_, e)| e),
            None => keys.clear(),
        }
        self.bound = want;
    }

    fn pressed(&self) -> bool {
        self.keys.as_ref().is_some_and(|k| !k.poll().is_empty())
    }

    fn release(&mut self) {
        if let Some(k) = self.keys.as_mut() {
            k.clear();
        }
        self.bound = None;
        self.recording = false;
    }
}

impl KeepAwake {
    pub fn new(env: &ToolEnv) -> Self {
        if env.demo {
            let (engine, t) = demo_engine(State::Agent);
            return KeepAwake { engine: Arc::new(Mutex::new(engine)), demo_now: Some(t), keys: KeyRec::default() };
        }
        let engine: Shared = Arc::new(Mutex::new(Engine::new(Some(engine::data_dir()))));
        url_routes(&engine, &env.ctx);
        let shared = engine.clone();
        let ctx = env.ctx.clone();
        std::thread::Builder::new()
            .name("keep-awake".into())
            .spawn(move || watch(shared, ctx))
            .ok();
        KeepAwake { engine, demo_now: None, keys: KeyRec::default() }
    }

    fn now(&self) -> i64 {
        self.demo_now.unwrap_or_else(now)
    }

    fn state(&self) -> State {
        self.engine.lock_ok().state(self.now())
    }

    pub fn toggle(&mut self) {
        let t = self.now();
        let mut e = self.engine.lock_ok();
        let on = e.manual_since.is_none();
        e.set_manual(on, t);
    }
}

/// `cmdmeow://awake/on`, `/off` and `/toggle` work the switch.
fn url_routes(engine: &Shared, ctx: &egui::Context) {
    let (engine, ctx) = (engine.clone(), ctx.clone());
    crate::kit::url_scheme::route("awake", move |action| {
        let t = now();
        let mut e = engine.lock_ok();
        let on = match action {
            "on" => true,
            "off" => false,
            "toggle" => e.manual_since.is_none(),
            _ => return false,
        };
        e.set_manual(on, t);
        drop(e);
        crate::kit::activity::repaint(&ctx);
        true
    });
}

/// The background loop: follow the main agent, read its logs and keep the
/// assertions in line, also while every window is hidden.
fn watch(engine: Shared, ctx: egui::Context) {
    crate::kit::activity::low_priority();
    let mut scanner = sessions::Scanner::default();
    // logs are read again only when macOS says they changed (and once a
    // minute in case an event was missed); the cheap state step still runs
    // every POLL so timers and the switch stay prompt. Every agent's folder
    // is watched, so switching the main one needs no new stream
    static CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();
    let roots: Vec<_> = sessions::Source::ALL.iter().map(|s| s.root()).collect();
    let hooked = crate::kit::fswatch::watch(&roots, 1.0, &CHANGED);
    let mut rescan = crate::kit::activity::Every::default();
    let mut was_watching = None;
    loop {
        let t = now();
        let source = sessions::Source::main();
        let watching = {
            let mut e = engine.lock_ok();
            if e.agent != source {
                e.set_agent(source, t);
                scanner = sessions::Scanner::default();
                crate::kit::activity::repaint(&ctx);
            }
            e.settings.agent.then_some(source).flatten()
        };
        let fresh = !hooked || CHANGED.take() || was_watching != watching || rescan.due(RESCAN);
        was_watching = watching;
        let found = match (watching, fresh) {
            (None, _) => Some(Vec::new()),
            (Some(src), true) => {
                rescan.mark();
                Some(scanner.scan(src, t))
            }
            (Some(_), false) => None,
        };
        {
            let mut e = engine.lock_ok();
            let before = (e.state(t), e.sessions.clone());
            if let Some(found) = found {
                e.sessions = found;
            }
            e.step(t);
            if before != (e.state(t), e.sessions.clone()) {
                crate::kit::activity::repaint(&ctx);
            }
        }
        std::thread::sleep(POLL);
    }
}

fn badge() -> Badge {
    Badge { icon: Icon::Cup, color: tint::KEEP_AWAKE() }
}

/// Sample data for screenshots, showing `state`.
fn demo_engine(state: State) -> (Engine, i64) {
    demo_engine_for(state, Some(sessions::Source::Claude))
}

/// The same, watching `agent` (None: no agent on this Mac).
fn demo_engine_for(state: State, agent: Option<sessions::Source>) -> (Engine, i64) {
    use sessions::Session;
    let t = engine::today_start(now()) + 13 * 3600 + 7 * 60;
    let mut e = Engine::new(None);
    e.agent = agent;
    let h = |x: f64| t - ((13.0 + 7.0 / 60.0 - x) * 3600.0) as i64;
    e.log = vec![
        engine::Span { start: h(9.2), end: h(9.9), agent: false, by: None },
        engine::Span { start: h(10.7), end: h(11.3), agent: agent.is_some(), by: agent },
        engine::Span { start: h(11.3), end: h(11.9), agent: false, by: None },
    ];
    // earlier days of the week
    for (d, c, m) in [(6, 2.5, 0.5), (5, 4.0, 1.0), (4, 1.0, 0.0), (3, 5.5, 0.8), (2, 0.0, 1.5), (1, 3.2, 0.4)] {
        let base = t - d * 86_400;
        if agent.is_some() {
            e.log.insert(0, engine::Span { start: base - ((c + m) * 3600.0) as i64, end: base - (m * 3600.0) as i64, agent: true, by: agent });
        }
        if m > 0.0 {
            e.log.push(engine::Span { start: base - (m * 3600.0) as i64, end: base, agent: false, by: None });
        }
    }
    e.log.sort_by_key(|s| s.start);
    let idle = Session { project: "notes".into(), last_write: t - 3 * 3600, tool_running: false };
    match state {
        State::Off => {
            e.sessions = vec![idle];
            e.step(t);
        }
        State::Manual => {
            e.sessions = vec![idle];
            e.set_manual(true, t - 25 * 60);
        }
        State::Agent => {
            e.sessions = vec![
                Session { project: "cmdmeow".into(), last_write: h(11.9), tool_running: false },
                Session { project: "api-server".into(), last_write: h(11.9), tool_running: false },
                idle.clone(),
            ];
            e.step(h(11.9));
            e.sessions[0].last_write = t - 12;
            e.sessions[1].last_write = t - 48;
        }
        State::WindDown { .. } => {
            e.sessions = vec![Session { project: "cmdmeow".into(), last_write: t - 300, tool_running: false }, idle];
            e.step(t - 300);
        }
    }
    if agent.is_none() {
        e.sessions.clear();
    }
    e.step(t);
    (e, t)
}

impl Tool for KeepAwake {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Keep Awake"
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn section(&self) -> &'static str {
        "Utilities"
    }
    fn about(&self) -> &'static str {
        "Keep the Mac awake, or only while your AI agent works"
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: "Keep Awake", about: "The switch, and what keeps your Mac up.", sizes: UP_TO_MEDIUM, size: Size::S12, default_on: true }]
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::widget(self, size, ui, rect, cx);
    }
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, _cx: &mut Cx) {
        ui::page(self, ui, rect);
    }
    fn set_on(&mut self, on: bool) {
        let t = self.now();
        self.engine.lock_ok().set_enabled(on, t);
        if !on {
            self.keys.release();
        }
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        if self.demo_now.is_none() {
            let want = self.engine.lock_ok().settings.shortcut.clone();
            self.keys.bind(ctx, want);
            if self.keys.pressed() {
                self.toggle();
                crate::kit::activity::repaint(ctx);
            }
        }
        // the steam holds still; only the clocks and the ring move, by the
        // second, while awake and on screen. The watcher thread does the
        // rest (60 fps here kept an open panel at ~24% CPU)
        if crate::kit::activity::visible() && self.state().awake() {
            ctx.request_repaint_after(crate::kit::glass::next_second());
        }
        false
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        use sessions::Source;
        let (state, agent) = match view {
            "awake" | "awake-claude" => (State::Agent, Some(Source::Claude)),
            "awake-codex" => (State::Agent, Some(Source::Codex)),
            "awake-off" => (State::Off, Some(Source::Claude)),
            // the main agent isn't on this Mac: no trigger, no sessions
            "awake-no-agent" => (State::Off, None),
            "awake-manual" => (State::Manual, Some(Source::Claude)),
            "awake-winddown" => (State::WindDown { left: 0 }, Some(Source::Claude)),
            _ => return None,
        };
        let (e, t) = demo_engine_for(state, agent);
        *self.engine.lock_ok() = e;
        self.demo_now = Some(t);
        Some(View::Page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_states_show_what_they_say() {
        for (want, matches) in [
            (State::Off, (|s: State| s == State::Off) as fn(State) -> bool),
            (State::Manual, |s| s == State::Manual),
            (State::Agent, |s| s == State::Agent),
            (State::WindDown { left: 0 }, |s| matches!(s, State::WindDown { .. })),
        ] {
            let (e, t) = demo_engine(want);
            assert!(matches(e.state(t)), "{want:?} → {:?}", e.state(t));
        }
    }
}
