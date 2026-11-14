//! Displays: brightness and speaker volume of external monitors, from the
//! panel and from the keyboard's own keys. A monitor that understands DDC/CI
//! is set directly (its backlight, its speakers); one that doesn't (TVs,
//! DisplayLink, some HDMI ports) is darkened through the picture instead.
//! Levels are remembered per monitor and put back after a reconnect or wake.
//!
//! Page routes: "" only.

mod ddc;
mod engine;
mod mac;
mod ui;

use crate::core::tool::{Badge, Cx, Size, Tool, ToolEnv, ToolId, View as ShellView, WidgetSpec};
use crate::kit::glass::{tint, Icon};
use crate::kit::perms::{Need, Perm};
use crate::kit::sync::LockExt;
use egui::{Rect, Ui};
use engine::{Bus, Cmd, Engine, Levels, Screen, Settings, View};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

pub const ID: ToolId = "displays";
const LEVELS: &str = "levels";

/// How often the display list is checked (plug-ins, wakes).
const POLL: Duration = Duration::from_secs(3);
/// How long the on-screen level stays up after a key.
pub const OSD_SECS: f32 = 1.4;

const LEVELS_FILE: &str = "levels.json";
const SETTINGS_FILE: &str = "settings.json";

pub struct Displays {
    view: Arc<Mutex<View>>,
    tx: Option<Sender<Cmd>>,
    dir: Option<PathBuf>,
    /// The tool is on in Settings: the keys are ours only then.
    on: Arc<AtomicBool>,
}

impl Displays {
    pub fn new(env: &ToolEnv) -> Self {
        let on = Arc::new(AtomicBool::new(true));
        if env.demo {
            let view = View { screens: demo_screens(), ..Default::default() };
            return Displays { view: Arc::new(Mutex::new(view)), tx: None, dir: None, on };
        }
        let dir = engine::data_dir();
        let settings: Settings = crate::kit::fs::load_json(&dir.join(SETTINGS_FILE));
        let levels: HashMap<String, Levels> = crate::kit::fs::load_json(&dir.join(LEVELS_FILE));
        let view = Arc::new(Mutex::new(View { settings, ..Default::default() }));
        let (tx, rx) = channel();
        {
            let (view, ctx, dir) = (view.clone(), env.ctx.clone(), dir.clone());
            std::thread::Builder::new()
                .name("displays".into())
                .spawn(move || work(mac::MacBus::default(), levels, view, rx, ctx, &dir))
                .ok();
        }
        keys(&view, &tx, &on, &env.ctx);
        Displays { view, tx: Some(tx), dir: Some(dir), on }
    }

    /// Change a level: on screen at once, on the wire from the worker.
    pub fn set(&mut self, cmd: Cmd) {
        self.view.lock_ok().apply(cmd);
        if let Some(tx) = &self.tx {
            tx.send(cmd).ok();
        }
    }

    pub fn screens(&self) -> Vec<Screen> {
        self.view.lock_ok().screens.clone()
    }

    pub fn settings(&self) -> Settings {
        self.view.lock_ok().settings
    }

    pub fn set_settings(&mut self, s: Settings) {
        self.view.lock_ok().settings = s;
        if let Some(dir) = &self.dir {
            crate::kit::fs::save_json(&dir.join(SETTINGS_FILE), &s, true);
        }
    }
}

/// The brightness and volume keys: a press over a monitor we drive is ours.
fn keys(view: &Arc<Mutex<View>>, tx: &Sender<Cmd>, on: &Arc<AtomicBool>, ctx: &egui::Context) {
    let (view, tx, on, ctx) = (view.clone(), tx.clone(), on.clone(), ctx.clone());
    mac::start_keys(Box::new(move |key, fine, at| {
        if !on.load(Ordering::Relaxed) {
            return false;
        }
        let sound = matches!(key, engine::Key::VolumeUp | engine::Key::VolumeDown | engine::Key::Mute);
        let audio = if sound { mac::audio() } else { engine::Audio::Elsewhere };
        let mut v = view.lock_ok();
        let Some(cmd) = engine::on_key(&v, key, fine, at, &audio) else { return false };
        v.apply(cmd);
        v.osd = engine::osd_for(&v, cmd, Instant::now());
        drop(v);
        tx.send(cmd).ok();
        crate::kit::activity::repaint(&ctx);
        true
    }));
}

/// The worker: owns the OS side, writes levels, and checks the display
/// list every few seconds, also while every window is hidden (a monitor
/// plugged in at night should still get its levels back).
fn work<B: Bus>(bus: B, levels: HashMap<String, Levels>, view: Arc<Mutex<View>>, rx: Receiver<Cmd>, ctx: egui::Context, dir: &Path) {
    let mut e = Engine::new(bus, levels);
    let mut clock = (SystemTime::now(), Instant::now());
    let mut next = Instant::now();
    loop {
        let mut rescan = false;
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(first) => {
                for c in std::iter::once(first).chain(rx.try_iter()) {
                    match c {
                        Cmd::Rescan => rescan = true,
                        c => e.apply(c),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        e.flush();
        if rescan || Instant::now() >= next {
            let (wall, mono) = (SystemTime::now(), Instant::now());
            let woke = engine::slept(wall.duration_since(clock.0).map(|d| d.as_secs_f64()).unwrap_or(0.0), mono.duration_since(clock.1).as_secs_f64());
            clock = (wall, mono);
            if e.refresh(woke) {
                view.lock_ok().screens = e.screens.clone();
                crate::kit::activity::repaint(&ctx);
            }
            next = Instant::now() + POLL;
        }
        if e.levels_dirty {
            e.levels_dirty = false;
            crate::kit::fs::save_json_later(&dir.join(LEVELS_FILE), &e.levels, true);
        }
    }
}

/// Sample displays for screenshots.
fn demo_screens() -> Vec<Screen> {
    use egui::{Pos2, Vec2};
    use engine::How;
    let at = |x: f32| Rect::from_min_size(Pos2::new(x, 0.0), Vec2::new(1512.0, 982.0));
    vec![
        Screen::sample(1, "Built-in Retina Display", true, How::Native, at(0.0), 0.72, None),
        Screen::sample(2, "DELL U2723QE", false, How::Ddc, at(1512.0), 0.45, Some(0.3)),
        Screen::sample(3, "LG TV SSCR2", false, How::Dim, at(3024.0), 0.8, None),
    ]
}

impl Tool for Displays {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Displays"
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::Display, color: tint::DISPLAYS() }
    }
    fn section(&self) -> &'static str {
        "Utilities"
    }
    fn about(&self) -> &'static str {
        "Brightness and volume of external monitors from the menu bar"
    }
    fn needs(&self) -> Vec<Need> {
        // only the brightness and volume keys need it
        vec![Need::may(Perm::Accessibility)]
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: LEVELS, title: "Displays", about: "Brightness and volume of your monitors.", sizes: &[Size::S14, Size::S24], size: Size::S14, default_on: false }]
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
        self.on.store(on, Ordering::Relaxed);
        if let Some(tx) = &self.tx {
            tx.send(Cmd::Pause(!on)).ok();
            tx.send(Cmd::Rescan).ok();
        }
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        // the level after a key needs frames while it shows, panel or not
        let showing = self.view.lock_ok().osd.is_some_and(|o| o.at.elapsed().as_secs_f32() < OSD_SECS);
        if showing {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        showing
    }
    fn windows(&mut self, ctx: &egui::Context, _cx: &mut Cx) {
        ui::osd(self, ctx);
    }
    fn demo_view(&mut self, view: &str) -> Option<ShellView> {
        match view {
            "displays" => Some(ShellView::Page),
            "displays-empty" => {
                self.view.lock_ok().screens.retain(|s| !s.adjustable());
                Some(ShellView::Page)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Displays {
        Displays::new(&ToolEnv { ctx: egui::Context::default(), demo: true })
    }

    #[test]
    fn the_built_in_panel_is_shown_but_not_adjustable() {
        let d = demo();
        assert_eq!(d.screens().iter().filter(|s| s.adjustable()).count(), 2);
        assert!(d.screens().iter().any(|s| s.builtin && !s.adjustable()));
    }

    #[test]
    fn set_moves_the_level_at_once() {
        let mut d = demo();
        d.set(Cmd::Brightness(2, 0.9));
        d.set(Cmd::Volume(2, 0.6));
        d.set(Cmd::Mute(2, true));
        let s = d.screens().into_iter().find(|s| s.id == 2).unwrap();
        assert!((s.brightness - 0.9).abs() < 1e-6);
        assert_eq!(s.volume, Some(0.6));
        assert!(s.muted);
        // no speakers: volume stays unknown
        d.set(Cmd::Volume(3, 0.6));
        assert_eq!(d.screens().into_iter().find(|s| s.id == 3).unwrap().volume, None);
    }

    #[test]
    fn the_worker_writes_and_saves() {
        use engine::tests::{found, Fake};
        let mut bus = Fake::default();
        bus.found = vec![found(2, "dell", 0.0)];
        bus.ddc.insert((2, ddc::vcp::BRIGHTNESS), ddc::Reading { current: 40, max: 100 });
        let dir = std::env::temp_dir().join(format!("cmdmeow-displays-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let view = Arc::new(Mutex::new(View::default()));
        let (tx, rx) = channel();
        let (v2, d2) = (view.clone(), dir.clone());
        let h = std::thread::spawn(move || work(bus, HashMap::new(), v2, rx, egui::Context::default(), &d2));
        // the first refresh publishes the monitor
        let t0 = Instant::now();
        while view.lock_ok().screens.is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(view.lock_ok().screens.len(), 1);
        tx.send(Cmd::Brightness(2, 0.7)).unwrap();
        drop(tx);
        h.join().unwrap();
        crate::kit::fs::flush_writes();
        let saved: HashMap<String, Levels> = crate::kit::fs::load_json(&dir.join(LEVELS_FILE));
        assert_eq!(saved.get("dell").and_then(|l| l.brightness), Some(0.7));
        std::fs::remove_dir_all(&dir).ok();
    }
}
