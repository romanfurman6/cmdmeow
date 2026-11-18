//! Keep Awake: one control, Off · Auto · On. Auto keeps the Mac from
//! sleeping while any AI agent (Claude Code, Codex, Gemini CLI, OpenCode)
//! is working, and 5 minutes after, so a pause between prompts doesn't put
//! it to sleep; On keeps it and the screen awake until turned off. Uses
//! macOS power assertions, the same thing `caffeinate` uses, without a
//! helper process.
//!
//! Page routes: "" only.

use crate::kit::sync::LockExt;
mod engine;
mod power;
mod sessions;
mod ui;

use crate::core::tool::{Badge, Cx, Size, Tool, ToolEnv, ToolId, View, WidgetSpec};
use crate::kit::glass::{tint, Icon};
use egui::{Rect, Ui};
use engine::{now, Engine, Mode, State, Via};
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

/// Wakes the background loop when the mode changes, so picking Auto finds
/// a working agent at once instead of at the next poll.
pub(crate) static MODE_CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();

pub struct KeepAwake {
    engine: Shared,
    /// Sample data for screenshots: the clock is frozen at this time.
    demo_now: Option<i64>,
    pub keys: KeyRec,
}

/// On's shortcut: what is bound, and the recorder on the page.
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
            // PROTOTYPE: CMDMEOW_DEMO_AWAKE=on|off|auto|winddown for the 1:2 redesign canvas
            let (engine, t) = match std::env::var("CMDMEOW_DEMO_AWAKE").as_deref() {
                Ok("on") => demo_engine(State::On),
                Ok("off") => demo_engine(State::Off),
                Ok("auto") => demo_engine_auto(State::Off, true),
                Ok("winddown") => demo_engine(State::WindDown { left: 0 }),
                _ => demo_engine(State::Agent),
            };
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

    /// On, or back to the mode before it.
    pub fn toggle(&mut self, via: Via) {
        let t = self.now();
        self.engine.lock_ok().toggle(via, t);
    }

    pub fn set_mode(&mut self, mode: Mode, via: Via) {
        let t = self.now();
        self.engine.lock_ok().set_mode(mode, via, t);
    }
}

/// `cmdmeow://awake/on`, `/auto`, `/off` pick the mode; `/toggle` turns
/// On on, or back to the mode before it.
fn url_routes(engine: &Shared, ctx: &egui::Context) {
    let (engine, ctx) = (engine.clone(), ctx.clone());
    crate::kit::url_scheme::route("awake", move |action| {
        let t = now();
        let mut e = engine.lock_ok();
        match action {
            "on" => e.set_mode(Mode::On, Via::Link, t),
            "auto" => e.set_mode(Mode::Auto, Via::Link, t),
            "off" => e.set_mode(Mode::Off, Via::Link, t),
            "toggle" => e.toggle(Via::Link, t),
            _ => return false,
        }
        drop(e);
        crate::kit::activity::repaint(&ctx);
        true
    });
}

/// The background loop: while Auto decides, read the agents' logs; keep
/// the assertions in line, also while every window is hidden.
fn watch(engine: Shared, ctx: egui::Context) {
    crate::kit::activity::low_priority();
    let mut scanner = sessions::Scanner::default();
    // logs are read again only when macOS says they changed (and once a
    // minute in case an event was missed); the cheap state step still runs
    // every POLL so timers and the switch stay prompt
    static CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();
    let roots: Vec<_> = sessions::Source::ALL.iter().map(|s| s.root()).collect();
    let hooked = crate::kit::fswatch::watch(&roots, 1.0, &CHANGED);
    let mut rescan = crate::kit::activity::Every::default();
    let mut was_watching = None;
    loop {
        let t = now();
        // with Off picked nothing is read
        let watching = engine.lock_ok().watching();
        let fresh = !hooked || CHANGED.take() || was_watching != Some(watching) || rescan.due(RESCAN);
        was_watching = Some(watching);
        let found = match (watching, fresh) {
            (false, _) => Some(Vec::new()),
            (true, true) => {
                rescan.mark();
                let mut all: Vec<_> = sessions::Source::ALL.into_iter().filter(|s| s.installed()).flat_map(|s| scanner.scan(s, t)).collect();
                all.sort_by_key(|s| std::cmp::Reverse(s.last_write));
                Some(all)
            }
            (true, false) => None,
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
        crate::kit::activity::nap(POLL, Some(&MODE_CHANGED));
    }
}

fn badge() -> Badge {
    Badge { icon: Icon::Cup, color: tint::KEEP_AWAKE() }
}

/// Sample data for screenshots, showing `state` (Off and On, or Auto
/// with agents working, winding down or none).
fn demo_engine(state: State) -> (Engine, i64) {
    demo_engine_auto(state, false)
}

/// The same; `idle`: Auto with no agent working.
fn demo_engine_auto(state: State, idle: bool) -> (Engine, i64) {
    use sessions::{Session, Source};
    let t = engine::today_start(now()) + 13 * 3600 + 7 * 60;
    let mut e = Engine::new(None);
    let h = |x: f64| t - ((13.0 + 7.0 / 60.0 - x) * 3600.0) as i64;
    let span = |a: f64, b: f64, who: Option<(Source, &str)>, via: Option<Via>| engine::Span {
        start: h(a),
        end: h(b),
        agent: who.is_some(),
        by: who.map(|w| w.0),
        who: who.map(|(s, p)| vec![(s, p.to_string())]).unwrap_or_default(),
        via,
    };
    e.log = vec![span(9.2, 9.9, None, Some(Via::Panel)), span(10.7, 11.3, Some((Source::Codex, "api-server")), None), span(11.3, 11.9, None, Some(Via::Shortcut))];
    let s = |src: Source, p: &str, at: i64| Session { source: src, project: p.into(), last_write: at, tool_running: false };
    let idle_s = s(Source::Claude, "notes", t - 3 * 3600);
    let auto = matches!(state, State::Agent | State::WindDown { .. }) || idle;
    e.settings.auto = auto;
    match state {
        State::Off => e.sessions = vec![idle_s],
        State::On => {
            e.sessions = vec![idle_s];
            e.on = Some((h(11.9), Via::Page));
        }
        State::Agent => {
            e.sessions = vec![s(Source::Claude, "cmdmeow", h(11.9)), s(Source::Codex, "api-server", h(11.9)), idle_s];
            e.step(h(11.9));
            e.sessions[0].last_write = t - 12;
            e.sessions[1].last_write = t - 48;
        }
        State::WindDown { .. } => {
            e.sessions = vec![s(Source::Claude, "cmdmeow", h(11.9)), idle_s];
            e.step(h(11.9));
            // quiet for 2:48: 4:12 of the 5 minutes left
            e.sessions[0].last_write = t - 168;
            e.step(t - 168);
        }
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
        "Keep the Mac awake"
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: "Keep Awake", about: "Off, Auto or On", sizes: &[Size::S11, Size::S12], size: Size::S12, default_on: true }]
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
                self.toggle(Via::Shortcut);
                crate::kit::activity::repaint(ctx);
            }
        }
        // nothing loops; only the clocks and the ring move, by the second,
        // while awake and on screen. The watcher thread does the
        // rest (60 fps here kept an open panel at ~24% CPU)
        if crate::kit::activity::visible() && self.state().awake() {
            ctx.request_repaint_after(crate::kit::glass::next_second());
        }
        false
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        let (state, idle) = match view {
            "awake" | "awake-claude" => (State::Agent, false),
            "awake-off" => (State::Off, false),
            "awake-auto" => (State::Off, true),
            "awake-on" | "awake-manual" => (State::On, false),
            "awake-winddown" => (State::WindDown { left: 0 }, false),
            _ => return None,
        };
        let (mut e, t) = demo_engine_auto(state, idle);
        if std::env::var_os("CMB_DEMO_MOUSE").is_some() {
            e.settings.shortcut = Shortcut::mouse(4, false, false, true, false);
        }
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
            (State::On, |s| s == State::On),
            (State::Agent, |s| s == State::Agent),
            (State::WindDown { left: 0 }, |s| matches!(s, State::WindDown { .. })),
        ] {
            let (e, t) = demo_engine(want);
            assert!(matches(e.state(t)), "{want:?} → {:?}", e.state(t));
        }
        let (e, t) = demo_engine_auto(State::Off, true);
        assert_eq!((e.mode(), e.state(t)), (Mode::Auto, State::Off));
    }
}
