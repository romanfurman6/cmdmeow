//! The displays and their levels, independent of macOS: what is plugged in,
//! how each one's brightness is set, what the user chose last time, and what
//! the brightness and volume keys should do. The OS side is a [`Bus`];
//! tests drive the engine with a fake one.

use super::ddc::{self, vcp, Reading};
use egui::{Pos2, Rect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// How a display's brightness is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// macOS does it (the built-in panel, Apple displays): we only show it.
    Native,
    /// The monitor itself, over DDC/CI.
    Ddc,
    /// No way to reach the backlight: the picture is darkened instead.
    Dim,
}

/// A display as the OS reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub id: u32,
    /// Stable across reconnects: vendor, model, serial.
    pub key: String,
    pub name: String,
    pub builtin: bool,
    /// macOS can set its brightness.
    pub native: bool,
    /// A DDC line to it was found.
    pub ddc: bool,
    /// Global points, top-left origin.
    pub frame: Rect,
}

/// One display and its levels, 0…1.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub id: u32,
    pub key: String,
    pub name: String,
    pub builtin: bool,
    pub how: How,
    pub frame: Rect,
    pub brightness: f32,
    /// None: no speakers we can drive.
    pub volume: Option<f32>,
    pub muted: bool,
    max_brightness: u16,
    max_volume: u16,
    /// The monitor has a mute control (else mute = volume 0 on the wire).
    has_mute: bool,
    /// A DDC line was there when it was probed.
    tried_ddc: bool,
}

impl Screen {
    pub fn adjustable(&self) -> bool {
        self.how != How::Native
    }

    /// A display for screenshots and tests.
    pub fn sample(id: u32, name: &str, builtin: bool, how: How, frame: Rect, brightness: f32, volume: Option<f32>) -> Screen {
        Screen {
            id,
            key: format!("sample-{id}"),
            name: name.into(),
            builtin,
            how,
            frame,
            brightness,
            volume,
            muted: false,
            max_brightness: 100,
            max_volume: 100,
            has_mute: true,
            tried_ddc: how == How::Ddc,
        }
    }
}

/// What the user set on a display, kept across launches and reconnects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub brightness: Option<f32>,
    pub volume: Option<f32>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The keyboard's brightness and volume keys drive external monitors.
    pub keys: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { keys: true }
    }
}

/// A change to one display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    Brightness(u32, f32),
    Volume(u32, f32),
    Mute(u32, bool),
    /// Look for displays now.
    Rescan,
    /// The tool was turned off (true) or on again: off, every darkened
    /// display gets its full picture back and nothing is written.
    Pause(bool),
}

impl Cmd {
    fn id(self) -> Option<u32> {
        match self {
            Cmd::Brightness(id, _) | Cmd::Volume(id, _) | Cmd::Mute(id, _) => Some(id),
            Cmd::Rescan | Cmd::Pause(_) => None,
        }
    }
}

/// The on-screen level shown after a key press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Osd {
    pub id: u32,
    pub volume: bool,
    pub level: f32,
    pub muted: bool,
    pub at: Instant,
}

/// What the UI and the key handler share with the worker.
#[derive(Default)]
pub struct View {
    pub screens: Vec<Screen>,
    pub settings: Settings,
    pub osd: Option<Osd>,
}

impl View {
    pub fn screen(&self, id: u32) -> Option<&Screen> {
        self.screens.iter().find(|s| s.id == id)
    }
    /// Put a change into the view at once, so sliders follow the hand.
    pub fn apply(&mut self, cmd: Cmd) {
        if let Some(s) = cmd.id().and_then(|id| self.screens.iter_mut().find(|s| s.id == id)) {
            set_level(s, cmd);
        }
    }
}

fn set_level(s: &mut Screen, cmd: Cmd) {
    match cmd {
        Cmd::Brightness(_, v) => s.brightness = clamp01(v),
        Cmd::Volume(_, v) => {
            if s.volume.is_some() {
                s.volume = Some(clamp01(v));
            }
        }
        Cmd::Mute(_, m) => s.muted = m,
        Cmd::Rescan | Cmd::Pause(_) => {}
    }
}

pub fn clamp01(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The darkest a dimmed display gets, so it never goes fully black.
pub const DIM_FLOOR: f32 = 0.15;

/// A dimmed display's brightness → how much of its light is kept.
pub fn dim_scale(level: f32) -> f32 {
    DIM_FLOOR + (1.0 - DIM_FLOOR) * clamp01(level)
}

/// One key step: sixteenths like macOS, or sixty-fourths with ⌥⇧.
pub fn step(level: f32, up: bool, fine: bool) -> f32 {
    let n = if fine { 64.0 } else { 16.0 };
    let k = (clamp01(level) * n).round() + if up { 1.0 } else { -1.0 };
    (k.clamp(0.0, n)) / n
}

// ---------------------------------------------------------------- keys

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    BrightnessUp,
    BrightnessDown,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// Where the sound goes now.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub enum Audio {
    /// To a monitor over HDMI or DisplayPort; macOS can't change its volume.
    Monitor(String),
    Elsewhere,
}

/// The display a volume key is for: the one the sound goes to by name,
/// else the one under the pointer, else the only one with speakers.
fn volume_target<'a>(v: &'a View, audio: &Audio, at: Pos2) -> Option<&'a Screen> {
    let Audio::Monitor(name) = audio else { return None };
    let speakers: Vec<&Screen> = v.screens.iter().filter(|s| s.volume.is_some()).collect();
    let n = norm(name);
    speakers
        .iter()
        .find(|s| !n.is_empty() && (norm(&s.name).contains(&n) || n.contains(&norm(&s.name))))
        .or_else(|| speakers.iter().find(|s| s.frame.contains(at)))
        .or(if speakers.len() == 1 { speakers.first() } else { None })
        .copied()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// What a key press does: the change to make, or None to leave the key to
/// macOS (built-in panel, Apple displays, sound not going to a monitor).
pub fn on_key(v: &View, key: Key, fine: bool, at: Pos2, audio: &Audio) -> Option<Cmd> {
    if !v.settings.keys {
        return None;
    }
    match key {
        Key::BrightnessUp | Key::BrightnessDown => {
            let s = v.screens.iter().find(|s| s.frame.contains(at))?;
            s.adjustable().then(|| Cmd::Brightness(s.id, step(s.brightness, key == Key::BrightnessUp, fine)))
        }
        Key::Mute => volume_target(v, audio, at).map(|s| Cmd::Mute(s.id, !s.muted)),
        Key::VolumeUp | Key::VolumeDown => {
            let s = volume_target(v, audio, at)?;
            let cur = s.volume.unwrap_or(0.0);
            Some(Cmd::Volume(s.id, step(cur, key == Key::VolumeUp, fine)))
        }
    }
}

/// The OSD to show after `cmd` was put into the view.
pub fn osd_for(v: &View, cmd: Cmd, at: Instant) -> Option<Osd> {
    let s = v.screen(cmd.id()?)?;
    let volume = !matches!(cmd, Cmd::Brightness(..));
    let level = if volume { s.volume.unwrap_or(0.0) } else { s.brightness };
    Some(Osd { id: s.id, volume, level, muted: volume && s.muted, at })
}

// ---------------------------------------------------------------- the OS side

/// What the engine needs from the OS. Calls may be slow (DDC waits for the
/// monitor), so they run on the worker thread.
pub trait Bus {
    fn list(&mut self) -> Vec<Found>;
    fn ddc_read(&mut self, id: u32, code: u8) -> Option<Reading>;
    fn ddc_write(&mut self, id: u32, code: u8, value: u16) -> bool;
    fn native_get(&mut self, id: u32) -> Option<f32>;
    /// Darken the picture: `scale` of its light kept (1 = untouched).
    fn dim(&mut self, id: u32, scale: f32) -> bool;
}

/// A volume change on the wire: 0 while muted on a monitor without mute.
fn wire_volume(s: &Screen) -> u16 {
    if s.muted && !s.has_mute {
        0
    } else {
        ddc::to_raw(s.volume.unwrap_or(0.0), s.max_volume)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Ctl {
    Brightness,
    Volume,
    Mute,
}

pub struct Engine<B: Bus> {
    pub bus: B,
    pub screens: Vec<Screen>,
    pub levels: HashMap<String, Levels>,
    /// Displays met since launch, by key: one coming back is a reconnect.
    seen: HashSet<String>,
    /// Writes waiting, the last value per display and control.
    pending: Vec<(u32, Ctl)>,
    /// Refreshes left that write every level again (after a wake).
    reapply: u8,
    pub levels_dirty: bool,
    paused: bool,
}

impl<B: Bus> Engine<B> {
    pub fn new(bus: B, levels: HashMap<String, Levels>) -> Self {
        Engine { bus, screens: Vec::new(), levels, seen: HashSet::new(), pending: Vec::new(), reapply: 0, levels_dirty: false, paused: false }
    }

    /// Bring the list in line with what is plugged in. `woke`: the Mac just
    /// woke up, monitors may have forgotten their levels. Returns true when
    /// the list or a probed level changed.
    pub fn refresh(&mut self, woke: bool) -> bool {
        if woke {
            // monitors take a moment to wake: write twice, a refresh apart
            self.reapply = 2;
        }
        let found = self.bus.list();
        let before = self.screens.clone();
        self.screens.retain(|s| found.iter().any(|f| f.id == s.id && f.key == s.key));
        for f in &found {
            if let Some(i) = self.screens.iter().position(|s| s.id == f.id) {
                let s = &mut self.screens[i];
                s.name = f.name.clone();
                s.frame = f.frame;
                if woke {
                    s.tried_ddc = false;
                }
                // a line that showed up late (a monitor still waking when
                // plugged in): ask it again, keeping the level set so far
                if s.how == How::Dim && f.ddc && !s.tried_ddc {
                    s.tried_ddc = true;
                    let level = s.brightness;
                    let mut fresh = self.probe(f);
                    if fresh.how == How::Ddc {
                        // the picture was darkened: undo that, the monitor takes over
                        self.bus.dim(f.id, 1.0);
                        fresh.brightness = level;
                        self.want(f.id, Ctl::Brightness);
                        self.screens[i] = fresh;
                    }
                }
                continue;
            }
            let back = self.seen.contains(&f.key);
            self.seen.insert(f.key.clone());
            let mut s = self.probe(f);
            match (self.levels.get(&f.key).copied(), back || s.how == How::Dim) {
                (Some(l), true) if s.how != How::Native => {
                    if let Some(b) = l.brightness {
                        s.brightness = clamp01(b);
                    }
                    if let (Some(v), Some(_)) = (l.volume, s.volume) {
                        s.volume = Some(clamp01(v));
                        s.muted = l.muted;
                    }
                    self.want(s.id, Ctl::Brightness);
                    if s.volume.is_some() {
                        self.want(s.id, Ctl::Volume);
                        self.want(s.id, Ctl::Mute);
                    }
                }
                _ if s.how != How::Native => self.remember(&s),
                _ => {}
            }
            self.screens.push(s);
        }
        self.screens.sort_by_key(|s| (!s.builtin, s.frame.left() as i32, s.id));
        if self.reapply > 0 {
            self.reapply -= 1;
            let all: Vec<(u32, bool)> = self.screens.iter().filter(|s| s.adjustable()).map(|s| (s.id, s.volume.is_some())).collect();
            for (id, speakers) in all {
                self.want(id, Ctl::Brightness);
                if speakers {
                    self.want(id, Ctl::Volume);
                    self.want(id, Ctl::Mute);
                }
            }
        }
        self.flush();
        self.screens != before
    }

    fn probe(&mut self, f: &Found) -> Screen {
        let mut s = Screen {
            id: f.id,
            key: f.key.clone(),
            name: f.name.clone(),
            builtin: f.builtin,
            how: How::Dim,
            frame: f.frame,
            brightness: 1.0,
            volume: None,
            muted: false,
            max_brightness: 100,
            max_volume: 100,
            has_mute: false,
            tried_ddc: f.ddc,
        };
        if f.native {
            s.how = How::Native;
            s.brightness = self.bus.native_get(f.id).map(clamp01).unwrap_or(1.0);
            return s;
        }
        if f.ddc {
            if let Some(r) = self.bus.ddc_read(f.id, vcp::BRIGHTNESS) {
                s.how = How::Ddc;
                s.max_brightness = r.max;
                s.brightness = ddc::to_level(r);
                if let Some(v) = self.bus.ddc_read(f.id, vcp::VOLUME) {
                    s.max_volume = v.max;
                    s.volume = Some(ddc::to_level(v));
                    if let Some(m) = self.bus.ddc_read(f.id, vcp::MUTE) {
                        s.has_mute = true;
                        s.muted = m.current == ddc::MUTE_ON;
                    }
                }
            }
        }
        s
    }

    fn remember(&mut self, s: &Screen) {
        let l = Levels { brightness: Some(s.brightness), volume: s.volume, muted: s.muted };
        if self.levels.get(&s.key) != Some(&l) {
            self.levels.insert(s.key.clone(), l);
            self.levels_dirty = true;
        }
    }

    fn want(&mut self, id: u32, c: Ctl) {
        if !self.pending.contains(&(id, c)) {
            self.pending.push((id, c));
        }
    }

    /// Take a change from the user (or a key). The write waits for
    /// [`flush`](Self::flush), so a fast slider sends only its last value.
    pub fn apply(&mut self, cmd: Cmd) {
        if let Cmd::Pause(p) = cmd {
            if p && !self.paused {
                self.pending.clear();
                for s in self.screens.iter().filter(|s| s.how == How::Dim) {
                    self.bus.dim(s.id, 1.0);
                }
            }
            if !p && self.paused {
                self.reapply = 1;
            }
            self.paused = p;
            return;
        }
        if self.paused {
            return;
        }
        let Some(id) = cmd.id() else { return };
        let Some(i) = self.screens.iter().position(|s| s.id == id) else { return };
        if !self.screens[i].adjustable() {
            return;
        }
        set_level(&mut self.screens[i], cmd);
        let s = self.screens[i].clone();
        self.remember(&s);
        match cmd {
            Cmd::Brightness(..) => self.want(id, Ctl::Brightness),
            Cmd::Volume(..) if s.volume.is_some() => self.want(id, Ctl::Volume),
            Cmd::Mute(..) if s.volume.is_some() => self.want(id, if s.has_mute { Ctl::Mute } else { Ctl::Volume }),
            _ => {}
        }
    }

    /// Send the waiting writes.
    pub fn flush(&mut self) {
        if self.paused {
            self.pending.clear();
            return;
        }
        for (id, c) in std::mem::take(&mut self.pending) {
            let Some(s) = self.screens.iter().find(|s| s.id == id).cloned() else { continue };
            match (s.how, c) {
                (How::Native, _) => {}
                (How::Dim, Ctl::Brightness) => {
                    self.bus.dim(id, dim_scale(s.brightness));
                }
                (How::Dim, _) => {}
                (How::Ddc, Ctl::Brightness) => {
                    self.bus.ddc_write(id, vcp::BRIGHTNESS, ddc::to_raw(s.brightness, s.max_brightness));
                }
                (How::Ddc, Ctl::Volume) => {
                    self.bus.ddc_write(id, vcp::VOLUME, wire_volume(&s));
                }
                (How::Ddc, Ctl::Mute) => {
                    if s.has_mute {
                        self.bus.ddc_write(id, vcp::MUTE, if s.muted { ddc::MUTE_ON } else { ddc::MUTE_OFF });
                    } else {
                        self.bus.ddc_write(id, vcp::VOLUME, wire_volume(&s));
                    }
                }
            }
        }
    }
}

/// True when the wall clock ran much further than the monotonic one: the
/// Mac slept in between (the monotonic clock stops during sleep).
pub fn slept(wall_secs: f64, mono_secs: f64) -> bool {
    wall_secs - mono_secs > 10.0
}

pub fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("displays")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use egui::Vec2;

    /// A pretend Mac: displays, monitor controls, and a log of writes.
    #[derive(Default)]
    pub struct Fake {
        pub found: Vec<Found>,
        /// (display, vcp) → reading; missing = no answer.
        pub ddc: HashMap<(u32, u8), Reading>,
        pub native: HashMap<u32, f32>,
        pub writes: Vec<(u32, u8, u16)>,
        pub dims: Vec<(u32, f32)>,
    }

    impl Bus for Fake {
        fn list(&mut self) -> Vec<Found> {
            self.found.clone()
        }
        fn ddc_read(&mut self, id: u32, code: u8) -> Option<Reading> {
            self.ddc.get(&(id, code)).copied()
        }
        fn ddc_write(&mut self, id: u32, code: u8, value: u16) -> bool {
            self.writes.push((id, code, value));
            if let Some(r) = self.ddc.get_mut(&(id, code)) {
                r.current = value;
            }
            true
        }
        fn native_get(&mut self, id: u32) -> Option<f32> {
            self.native.get(&id).copied()
        }
        fn dim(&mut self, id: u32, scale: f32) -> bool {
            self.dims.push((id, scale));
            true
        }
    }

    pub fn found(id: u32, key: &str, x: f32) -> Found {
        Found { id, key: key.into(), name: format!("Display {id}"), builtin: false, native: false, ddc: true, frame: Rect::from_min_size(Pos2::new(x, 0.0), Vec2::new(1000.0, 800.0)) }
    }

    fn r(current: u16, max: u16) -> Reading {
        Reading { current, max }
    }

    /// A built-in panel at x 0 and a DDC monitor with speakers at x 1000.
    fn desk() -> Fake {
        let mut f = Fake::default();
        let mut builtin = found(1, "apple-panel", 0.0);
        builtin.builtin = true;
        builtin.native = true;
        builtin.ddc = false;
        f.found = vec![builtin, found(2, "dell", 1000.0)];
        f.native.insert(1, 0.7);
        f.ddc.insert((2, vcp::BRIGHTNESS), r(40, 100));
        f.ddc.insert((2, vcp::VOLUME), r(20, 100));
        f.ddc.insert((2, vcp::MUTE), r(2, 2));
        f
    }

    #[test]
    fn probes_what_each_display_can_do() {
        let mut f = desk();
        let mut tv = found(3, "tv", 2000.0);
        tv.ddc = false;
        f.found.push(tv);
        let mut e = Engine::new(f, HashMap::new());
        assert!(e.refresh(false));
        let hows: Vec<How> = e.screens.iter().map(|s| s.how).collect();
        assert_eq!(hows, vec![How::Native, How::Ddc, How::Dim]);
        let dell = &e.screens[1];
        assert!((dell.brightness - 0.4).abs() < 1e-6);
        assert_eq!(dell.volume.map(|v| (v * 100.0).round()), Some(20.0));
        assert!(!dell.muted);
        assert!((e.screens[0].brightness - 0.7).abs() < 1e-6);
        // nothing changed: no update
        assert!(!e.refresh(false));
    }

    #[test]
    fn a_monitor_that_does_not_answer_is_dimmed_instead() {
        let mut f = desk();
        f.ddc.clear();
        let mut e = Engine::new(f, HashMap::new());
        e.refresh(false);
        assert_eq!(e.screens[1].how, How::Dim);
        assert_eq!(e.screens[1].volume, None);
        e.apply(Cmd::Brightness(2, 0.0));
        e.flush();
        assert_eq!(e.bus.dims.last(), Some(&(2, DIM_FLOOR)));
        assert!(e.bus.writes.is_empty());
    }

    #[test]
    fn fast_changes_send_only_the_last_value() {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        e.bus.writes.clear();
        for v in [0.1, 0.2, 0.3, 0.55] {
            e.apply(Cmd::Brightness(2, v));
        }
        e.flush();
        assert_eq!(e.bus.writes, vec![(2, vcp::BRIGHTNESS, 55)]);
        e.flush();
        assert_eq!(e.bus.writes.len(), 1);
    }

    #[test]
    fn the_built_in_panel_is_left_to_macos() {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        e.bus.writes.clear();
        e.apply(Cmd::Brightness(1, 0.1));
        e.flush();
        assert!(e.bus.writes.is_empty() && e.bus.dims.is_empty());
        assert!((e.screens[0].brightness - 0.7).abs() < 1e-6);
    }

    #[test]
    fn mute_uses_the_mute_control_or_volume_zero() {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        e.bus.writes.clear();
        e.apply(Cmd::Mute(2, true));
        e.flush();
        assert_eq!(e.bus.writes, vec![(2, vcp::MUTE, ddc::MUTE_ON)]);

        let mut f = desk();
        f.ddc.remove(&(2, vcp::MUTE));
        let mut e = Engine::new(f, HashMap::new());
        e.refresh(false);
        e.apply(Cmd::Mute(2, true));
        e.flush();
        assert_eq!(e.bus.writes.last(), Some(&(2, vcp::VOLUME, 0)));
        e.apply(Cmd::Mute(2, false));
        e.flush();
        assert_eq!(e.bus.writes.last(), Some(&(2, vcp::VOLUME, 20)));
        // the volume is kept while muted
        assert_eq!(e.screens[1].volume.map(|v| (v * 100.0).round()), Some(20.0));
    }

    #[test]
    fn at_launch_the_monitor_keeps_its_own_levels_and_they_are_remembered() {
        let mut saved = HashMap::new();
        saved.insert("dell".to_string(), Levels { brightness: Some(0.9), volume: Some(0.5), muted: false });
        let mut e = Engine::new(desk(), saved);
        e.refresh(false);
        assert!(e.bus.writes.is_empty());
        assert!((e.screens[1].brightness - 0.4).abs() < 1e-6);
        assert_eq!(e.levels["dell"].brightness, Some(0.4));
        assert!(e.levels_dirty);
    }

    #[test]
    fn a_reconnected_monitor_gets_its_levels_back() {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        e.apply(Cmd::Brightness(2, 0.8));
        e.apply(Cmd::Volume(2, 0.3));
        e.flush();
        // unplugged
        let dell = e.bus.found.pop().unwrap();
        assert!(e.refresh(false));
        assert_eq!(e.screens.len(), 1);
        // the monitor forgot (power cycle), and comes back with a new id
        e.bus.ddc.insert((7, vcp::BRIGHTNESS), r(100, 100));
        e.bus.ddc.insert((7, vcp::VOLUME), r(50, 100));
        e.bus.found.push(Found { id: 7, ..dell });
        e.bus.writes.clear();
        e.refresh(false);
        assert!(e.bus.writes.contains(&(7, vcp::BRIGHTNESS, 80)));
        assert!(e.bus.writes.contains(&(7, vcp::VOLUME, 30)));
        assert!((e.screens[1].brightness - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_dimmed_display_is_dimmed_again_at_launch() {
        let mut f = desk();
        f.ddc.clear();
        let mut saved = HashMap::new();
        saved.insert("dell".to_string(), Levels { brightness: Some(0.5), volume: None, muted: false });
        let mut e = Engine::new(f, saved);
        e.refresh(false);
        assert_eq!(e.bus.dims, vec![(2, dim_scale(0.5))]);
    }

    #[test]
    fn after_a_wake_levels_are_written_again_twice() {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        e.bus.writes.clear();
        e.refresh(true);
        assert!(e.bus.writes.contains(&(2, vcp::BRIGHTNESS, 40)));
        let n = e.bus.writes.len();
        e.refresh(false);
        assert!(e.bus.writes.len() > n, "second pass");
        let n = e.bus.writes.len();
        e.refresh(false);
        assert_eq!(e.bus.writes.len(), n, "then it stops");
    }

    #[test]
    fn a_late_ddc_line_takes_over_from_dimming() {
        let mut f = desk();
        f.found[1].ddc = false;
        let mut e = Engine::new(f, HashMap::new());
        e.refresh(false);
        assert_eq!(e.screens[1].how, How::Dim);
        e.apply(Cmd::Brightness(2, 0.25));
        e.flush();
        e.bus.found[1].ddc = true;
        e.bus.writes.clear();
        assert!(e.refresh(false));
        assert_eq!(e.screens[1].how, How::Ddc);
        assert_eq!(e.bus.dims.last(), Some(&(2, 1.0)));
        assert_eq!(e.bus.writes, vec![(2, vcp::BRIGHTNESS, 25)]);
        // a monitor that never answers is asked once, not on every refresh
        let mut f = desk();
        f.ddc.clear();
        let mut e = Engine::new(f, HashMap::new());
        e.refresh(false);
        e.refresh(false);
        assert_eq!(e.screens[1].how, How::Dim);
    }

    #[test]
    fn turned_off_it_gives_the_picture_back_and_writes_nothing() {
        let mut f = desk();
        f.ddc.clear();
        let mut e = Engine::new(f, HashMap::new());
        e.refresh(false);
        e.apply(Cmd::Brightness(2, 0.2));
        e.flush();
        e.apply(Cmd::Pause(true));
        assert_eq!(e.bus.dims.last(), Some(&(2, 1.0)));
        let n = e.bus.dims.len();
        e.apply(Cmd::Brightness(2, 0.5));
        e.refresh(true);
        e.flush();
        assert_eq!(e.bus.dims.len(), n);
        // on again: darkened as before
        e.apply(Cmd::Pause(false));
        e.refresh(false);
        assert_eq!(e.bus.dims.last(), Some(&(2, dim_scale(0.2))));
    }

    #[test]
    fn steps_like_macos() {
        assert_eq!(step(0.5, true, false), 0.5625);
        assert_eq!(step(0.5, false, false), 0.4375);
        assert_eq!(step(1.0, true, false), 1.0);
        assert_eq!(step(0.0, false, false), 0.0);
        assert_eq!(step(0.5, true, true), 0.515625);
        // off-grid values snap first
        assert_eq!(step(0.51, true, false), 0.5625);
        assert_eq!(step(f32::NAN, true, false), 0.0625);
    }

    #[test]
    fn dim_never_goes_black() {
        assert_eq!(dim_scale(0.0), DIM_FLOOR);
        assert_eq!(dim_scale(1.0), 1.0);
        assert!(dim_scale(0.5) > DIM_FLOOR && dim_scale(0.5) < 1.0);
    }

    fn view() -> View {
        let mut e = Engine::new(desk(), HashMap::new());
        e.refresh(false);
        View { screens: e.screens, settings: Settings::default(), osd: None }
    }

    #[test]
    fn brightness_keys_follow_the_pointer() {
        let v = view();
        let on_dell = Pos2::new(1500.0, 300.0);
        let on_panel = Pos2::new(200.0, 300.0);
        assert_eq!(on_key(&v, Key::BrightnessUp, false, on_dell, &Audio::Elsewhere), Some(Cmd::Brightness(2, 0.4375)));
        // the built-in panel is macOS's
        assert_eq!(on_key(&v, Key::BrightnessUp, false, on_panel, &Audio::Elsewhere), None);
        // nowhere
        assert_eq!(on_key(&v, Key::BrightnessUp, false, Pos2::new(-50.0, 0.0), &Audio::Elsewhere), None);
        // turned off in settings
        let mut off = view();
        off.settings.keys = false;
        assert_eq!(on_key(&off, Key::BrightnessUp, false, on_dell, &Audio::Elsewhere), None);
    }

    #[test]
    fn volume_keys_only_when_the_sound_goes_to_a_monitor() {
        let mut v = view();
        let p = Pos2::new(200.0, 300.0);
        assert_eq!(on_key(&v, Key::VolumeUp, false, p, &Audio::Elsewhere), None);
        // by name, wherever the pointer is
        let audio = Audio::Monitor("DISPLAY 2".into());
        assert_eq!(on_key(&v, Key::VolumeUp, false, p, &audio), Some(Cmd::Volume(2, 0.25)));
        assert_eq!(on_key(&v, Key::Mute, false, p, &audio), Some(Cmd::Mute(2, true)));
        // an unknown name: the only monitor with speakers
        let other = Audio::Monitor("LG HDR 4K".into());
        assert_eq!(on_key(&v, Key::VolumeDown, false, p, &other), Some(Cmd::Volume(2, 0.125)));
        // no monitor with speakers: macOS keeps the key
        v.screens[1].volume = None;
        assert_eq!(on_key(&v, Key::VolumeUp, false, p, &audio), None);
    }

    #[test]
    fn the_osd_shows_the_new_level() {
        let mut v = view();
        let cmd = Cmd::Volume(2, 0.25);
        v.apply(cmd);
        let o = osd_for(&v, cmd, Instant::now()).unwrap();
        assert!(o.volume && !o.muted && (o.level - 0.25).abs() < 1e-6);
        v.apply(Cmd::Mute(2, true));
        assert!(osd_for(&v, Cmd::Mute(2, true), Instant::now()).unwrap().muted);
        let o = osd_for(&v, Cmd::Brightness(2, 0.9), Instant::now()).unwrap();
        assert!(!o.volume && (o.level - 0.4).abs() < 1e-6);
        assert!(osd_for(&v, Cmd::Rescan, Instant::now()).is_none());
    }

    #[test]
    fn sleep_shows_as_a_clock_gap() {
        assert!(!slept(3.0, 3.0));
        assert!(!slept(3.5, 3.0));
        assert!(slept(600.0, 3.0));
    }

    #[test]
    fn levels_and_settings_survive_json() {
        let l = Levels { brightness: Some(0.3), volume: None, muted: true };
        let s = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Levels>(&s).unwrap(), l);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default());
    }
}
