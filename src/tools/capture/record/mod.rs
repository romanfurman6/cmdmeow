//! Screen recording, part of Capture: pick a display, a window or an area
//! (with camera, microphone and system sound), count down, record with a
//! small control bar, and save an mp4 to the Capture folder. Nothing leaves
//! the Mac.
//!
//! Flow: [`Recorder::begin`] opens the picker over the screen under the
//! mouse → Start → 3-2-1 → [`engine::start`] → Live (bar, timer in the menu
//! bar) → Stop → the file is finished on a thread → a preview card and a
//! History entry. The cursor and the camera bubble are in the picture as
//! it's recorded, so there's nothing to draw after Stop.

pub mod camera;
pub mod engine;
pub mod meter;
pub mod mixer;
pub mod sys;
pub mod timeline;
pub mod ui;

use super::settings::{RecSettings, Settings};
use egui::{Pos2, Rect, Vec2};
use engine::{Session, Stopped, Target};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use sys::{Access, Display, Media, Win};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Display,
    Window,
    Area,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::Display, Source::Window, Source::Area];
}

/// What was picked, with where it is on screen.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub target: Target,
    pub display: Display,
    /// What's being recorded, in global points.
    pub rect: Rect,
    /// Video size in pixels.
    pub size: (u32, u32),
    pub fps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Delete,
    Restart,
}

pub struct Pick {
    pub source: Source,
    pub display: usize,
    /// Windows on screen, front to back (for the Window source).
    pub windows: Vec<Win>,
    /// An area drag in progress (local points of the overlay).
    pub drag: Option<(Pos2, Pos2)>,
    /// The area, in global points.
    pub area: Option<Rect>,
    pub frames: u32,
}

pub struct Live {
    pub chosen: Chosen,
    pub session: Session,
    pub paused: bool,
    pub mic_muted: bool,
    pub confirm: Option<Confirm>,
    pub settings_open: bool,
    /// Where the bar sits (global points); None until placed.
    pub bar_at: Option<Pos2>,
    /// Said once that the microphone hears nothing.
    pub mic_warned: bool,
}

pub enum Phase {
    Idle,
    Pick(Pick),
    Countdown { chosen: Chosen, until: Instant, total: u32 },
    Starting { chosen: Chosen },
    Live(Box<Live>),
    /// The file is being finished.
    Saving,
}

/// The card after a recording is saved.
pub struct Saved {
    pub path: PathBuf,
    pub secs: f64,
    pub size: (u32, u32),
    pub bytes: u64,
    pub thumb: Option<image::RgbaImage>,
    pub tex: Option<egui::TextureHandle>,
    pub history: Option<u64>,
    pub display: Rect,
    /// Moves while the mouse is over the card.
    pub born: Instant,
    pub key: Instant,
}

pub enum Event {
    Started(Result<Session, String>),
    Stopped(Result<Stopped, String>),
    Access(Media, bool),
}

/// What the recorder needs Capture to do.
pub enum Out {
    Hud(String),
    Saved(Stopped),
    SaveSettings,
}

pub struct Recorder {
    pub phase: Phase,
    pub displays: Vec<Display>,
    pub camera: Option<camera::Camera>,
    /// Said once that the camera sends no picture.
    camera_warned: bool,
    pub mics: Vec<sys::Device>,
    pub cams: Vec<sys::Device>,
    /// The device list open in the picker.
    pub menu: Option<Media>,
    /// The picker's live mic level, and which mic it was opened for.
    pub meter: Option<meter::MicMeter>,
    meter_for: Option<Option<String>>,
    pub saved: Option<Saved>,
    pub tx: Sender<Event>,
    rx: Receiver<Event>,
    /// The last area, offered again next time (global points).
    pub last_area: Option<Rect>,
    pub out: Vec<Out>,
    /// Where recordings go; Capture keeps it in step with its folder.
    pub folder: PathBuf,
    /// The display of the last recording (for the saved card).
    pub last_display: Rect,
    ctx: egui::Context,
}

/// Seconds a recording must run before Delete and Restart ask first.
pub const CONFIRM_AFTER: f64 = 5.0;

impl Recorder {
    pub fn new(ctx: egui::Context) -> Recorder {
        let (tx, rx) = channel();
        // unframed copies of videos that are gone (and old projects)
        #[cfg(not(test))]
        let _ = std::thread::Builder::new().name("cmdmeow-prune".into()).spawn(|| crate::tools::capture::video::compose::prune(&projects_dir()));
        Recorder { phase: Phase::Idle, displays: Vec::new(), camera: None, camera_warned: false, mics: Vec::new(), cams: Vec::new(), menu: None, meter: None, meter_for: None, saved: None, tx, rx, last_area: None, out: Vec::new(), folder: PathBuf::new(), last_display: Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0)), ctx }
    }

    pub fn idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    pub fn live(&self) -> Option<&Live> {
        match &self.phase {
            Phase::Live(l) => Some(l),
            _ => None,
        }
    }

    /// Something on screen needs frames.
    pub fn busy(&self) -> bool {
        !self.idle() || self.saved.is_some()
    }

    fn hud(&mut self, m: impl Into<String>) {
        self.out.push(Out::Hud(m.into()));
    }

    /// The shortcut or the panel button: open the picker, or stop.
    pub fn begin(&mut self, s: &RecSettings) {
        match &self.phase {
            Phase::Idle => self.open_picker(s),
            Phase::Live(_) => self.stop(),
            Phase::Countdown { .. } => self.cancel(),
            _ => {}
        }
    }

    fn open_picker(&mut self, s: &RecSettings) {
        self.saved = None;
        self.displays = sys::displays();
        if self.displays.is_empty() {
            self.hud("No display to record");
            return;
        }
        self.mics = sys::devices(Media::Mic);
        self.cams = sys::devices(Media::Camera);
        self.menu = None;
        let display = sys::display_at(&self.displays, sys::mouse());
        self.phase = Phase::Pick(Pick { source: Source::Display, display, windows: Vec::new(), drag: None, area: self.last_area, frames: 0 });
        if s.camera {
            self.show_camera(s);
        }
    }

    /// Esc in the picker, or cancelling the countdown.
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.menu = None;
        self.camera = None;
    }

    pub fn set_source(&mut self, src: Source) {
        if let Phase::Pick(p) = &mut self.phase {
            p.source = src;
            p.drag = None;
            if src == Source::Window {
                p.windows = sys::windows();
            }
        }
    }

    pub fn next_display(&mut self) {
        let n = self.displays.len().max(1);
        if let Phase::Pick(p) = &mut self.phase {
            p.display = (p.display + 1) % n;
            p.frames = 0;
            p.area = None;
        }
    }

    /// The camera bubble's spot: bottom left of what's recorded.
    fn camera_spot(&self, d: f32) -> Pos2 {
        let r = match &self.phase {
            Phase::Pick(p) => p.area.filter(|_| p.source == Source::Area).or_else(|| self.displays.get(p.display).map(|d| d.frame)),
            Phase::Countdown { chosen, .. } | Phase::Starting { chosen } => Some(chosen.rect),
            Phase::Live(l) => Some(l.chosen.rect),
            _ => None,
        }
        .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0)));
        Pos2::new(r.left() + 32.0 + d / 2.0, r.bottom() - 32.0 - d / 2.0)
    }

    fn show_camera(&mut self, s: &RecSettings) {
        if self.camera.is_some() {
            return;
        }
        match sys::access(Media::Camera) {
            Access::Allowed => {
                let d = s.camera_size.points();
                match camera::Camera::open(s.camera_device.as_deref(), d, self.camera_spot(d)) {
                    Ok(c) => {
                        self.camera = Some(c);
                        self.camera_warned = false;
                    }
                    Err(e) => self.hud(format!("Camera: {e}")),
                }
            }
            Access::Ask => self.ask(Media::Camera),
            Access::Denied => {
                self.hud("Allow the camera in System Settings");
                sys::open_privacy(Media::Camera);
            }
        }
    }

    fn ask(&self, m: Media) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        sys::ask(m, move |ok| {
            let _ = tx.send(Event::Access(m, ok));
            ctx.request_repaint();
        });
    }

    /// Turn the camera on or off from the picker.
    pub fn toggle_camera(&mut self, s: &mut RecSettings) {
        s.camera = !s.camera;
        if s.camera {
            self.show_camera(s);
        } else {
            self.camera = None;
        }
        self.out.push(Out::SaveSettings);
    }

    pub fn toggle_mic(&mut self, s: &mut RecSettings) {
        s.mic = !s.mic;
        if s.mic {
            match sys::access(Media::Mic) {
                Access::Allowed => {}
                Access::Ask => self.ask(Media::Mic),
                Access::Denied => {
                    s.mic = false;
                    self.hud("Allow the microphone in System Settings");
                    sys::open_privacy(Media::Mic);
                }
            }
        }
        self.out.push(Out::SaveSettings);
    }

    /// Use device `id` for the camera or microphone, turning it on.
    pub fn use_device(&mut self, m: Media, id: String, s: &mut RecSettings) {
        self.menu = None;
        let (cur, on) = match m {
            Media::Camera => (&mut s.camera_device, s.camera),
            Media::Mic => (&mut s.mic_device, s.mic),
        };
        let changed = cur.as_deref() != Some(id.as_str());
        *cur = Some(id);
        match (m, on) {
            (Media::Camera, false) => self.toggle_camera(s),
            (Media::Mic, false) => self.toggle_mic(s),
            (Media::Camera, true) if changed && self.camera.is_some() => {
                self.camera = None;
                self.show_camera(s);
            }
            _ => {}
        }
        self.out.push(Out::SaveSettings);
    }

    /// The device in use: the chosen one if it's still there, else the first.
    pub fn device_id(&self, m: Media, s: &RecSettings) -> Option<String> {
        let (list, cur) = match m {
            Media::Camera => (&self.cams, &s.camera_device),
            Media::Mic => (&self.mics, &s.mic_device),
        };
        cur.as_ref().filter(|id| list.iter().any(|d| &d.id == *id)).or_else(|| list.first().map(|d| &d.id)).cloned()
    }

    pub fn device_name(&self, m: Media, s: &RecSettings) -> String {
        let (list, cur, none) = match m {
            Media::Camera => (&self.cams, &s.camera_device, "Default camera"),
            Media::Mic => (&self.mics, &s.mic_device, "Default microphone"),
        };
        cur.as_ref().and_then(|id| list.iter().find(|d| &d.id == id)).or_else(|| list.first()).map(|d| d.name.clone()).unwrap_or_else(|| none.to_string())
    }

    /// Pick what's under the picker's choice and count down.
    pub fn choose(&mut self, chosen: Chosen, s: &RecSettings) {
        if let Target::Area { .. } = chosen.target {
            self.last_area = Some(chosen.rect);
        }
        if s.countdown > 0 {
            self.phase = Phase::Countdown { until: Instant::now() + Duration::from_secs(s.countdown as u64), total: s.countdown, chosen };
        } else {
            self.launch(chosen, s);
        }
        // the camera moves to the corner of what's recorded
        let d = s.camera_size.points();
        let spot = self.camera_spot(d);
        if let Some(c) = &self.camera {
            c.set_diameter(d);
            c.move_to(spot);
        }
    }

    /// Start the engine for `chosen`.
    pub fn launch(&mut self, chosen: Chosen, s: &RecSettings) {
        let path = free_path(&self.folder, &Settings::recording_name(chrono::Local::now()));
        let mic = (s.mic && sys::access(Media::Mic) == Access::Allowed).then(|| s.mic_device.clone().filter(|id| self.mics.iter().any(|d| &d.id == id)));
        let cfg = engine::Config {
            target: chosen.target.clone(),
            size: chosen.size,
            fps: chosen.fps,
            bitrate: timeline::bitrate(chosen.size.0, chosen.size.1, chosen.fps, s.quality),
            cursor: s.cursor,
            clicks: s.clicks,
            system_audio: s.system_audio,
            mic,
            // the bubble is recorded where it is on screen
            keep_window: self.camera.as_ref().map(|c| c.window_number()),
            path,
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        engine::start(
            cfg,
            Box::new(move |r| {
                let _ = tx.send(Event::Started(r));
                ctx.request_repaint();
            }),
        );
        self.phase = Phase::Starting { chosen };
    }

    pub fn stop(&mut self) {
        let Phase::Live(live) = std::mem::replace(&mut self.phase, Phase::Saving) else { return };
        eprintln!("[rec] stop at {:.2} s", live.session.length());
        self.camera = None;
        self.last_display = live.chosen.display.frame;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        live.session.stop(Box::new(move |r| {
            let _ = tx.send(Event::Stopped(r));
            ctx.request_repaint();
        }));
    }

    /// Throw the recording away; `again` starts over from the countdown.
    pub fn discard(&mut self, again: bool, s: &RecSettings) {
        let Phase::Live(live) = std::mem::replace(&mut self.phase, Phase::Idle) else { return };
        let Live { chosen, session, .. } = *live;
        session.discard();
        if again {
            self.choose(chosen, s);
        } else {
            self.camera = None;
            self.hud("Recording deleted");
        }
    }

    /// Delete / Restart from the bar: ask first once it's long enough.
    pub fn ask_or(&mut self, what: Confirm, s: &RecSettings) {
        let Phase::Live(live) = &mut self.phase else { return };
        if live.session.length() >= CONFIRM_AFTER && live.confirm != Some(what) {
            live.confirm = Some(what);
            return;
        }
        self.discard(what == Confirm::Restart, s);
    }

    pub fn toggle_pause(&mut self) {
        if let Phase::Live(l) = &mut self.phase {
            l.paused = !l.paused;
            if l.paused {
                l.session.pause();
            } else {
                l.session.resume();
            }
        }
    }

    pub fn toggle_mute(&mut self) {
        if let Phase::Live(l) = &mut self.phase {
            l.mic_muted = !l.mic_muted;
            l.session.set_mic_muted(l.mic_muted);
        }
    }

    /// The menu bar title while recording: "● 0:42", "❚❚ 0:42".
    pub fn tray(&self) -> Option<String> {
        let l = self.live()?;
        let t = timeline::clock_label(l.session.length());
        Some(if l.paused { format!("❚❚ {t}") } else { format!("● {t}") })
    }

    /// Listen to the chosen mic while the picker is up (for its meter).
    fn keep_meter(&mut self, s: &RecSettings) {
        let want = matches!(self.phase, Phase::Pick(_)) && s.mic && sys::access(Media::Mic) == Access::Allowed;
        if !want {
            self.meter = None;
            self.meter_for = None;
            return;
        }
        let dev = s.mic_device.clone().filter(|id| self.mics.iter().any(|d| &d.id == id));
        if self.meter_for.as_ref() != Some(&dev) {
            self.meter = None;
            self.meter = meter::MicMeter::open(dev.as_deref());
            self.meter_for = Some(dev);
        }
    }

    /// Background results and timers; call once per pass.
    pub fn tick(&mut self, s: &mut RecSettings) {
        crate::kit::activity::set_recording(!matches!(self.phase, Phase::Idle | Phase::Pick(_)));
        self.keep_meter(s);
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Started(Ok(session)) => match std::mem::replace(&mut self.phase, Phase::Idle) {
                    Phase::Starting { chosen } => {
                        session.set_mic_muted(false);
                        self.phase = Phase::Live(Box::new(Live { chosen, session, paused: false, mic_muted: false, confirm: None, settings_open: false, bar_at: None, mic_warned: false }));
                    }
                    // cancelled while starting
                    other => {
                        session.discard();
                        self.phase = other;
                    }
                },
                Event::Started(Err(e)) => {
                    eprintln!("[rec] couldn't start: {e}");
                    if matches!(self.phase, Phase::Starting { .. }) {
                        self.phase = Phase::Idle;
                        self.camera = None;
                    }
                    self.hud(format!("Couldn't record: {e}"));
                }
                Event::Stopped(r) => {
                    eprintln!("[rec] stopped: {:?}", r.as_ref().map(|st| (&st.path, st.secs)));
                    self.phase = Phase::Idle;
                    match r {
                        Ok(st) => self.out.push(Out::Saved(st)),
                        Err(e) => self.hud(format!("Couldn't save the recording: {e}")),
                    }
                }
                Event::Access(m, ok) => match (m, ok) {
                    (Media::Camera, true) if s.camera && !self.idle() => self.show_camera(s),
                    (_, false) => {
                        match m {
                            Media::Camera => s.camera = false,
                            Media::Mic => s.mic = false,
                        }
                        self.camera = if m == Media::Camera { None } else { self.camera.take() };
                        self.out.push(Out::SaveSettings);
                        self.hud(if m == Media::Camera { "Camera not allowed" } else { "Microphone not allowed" });
                    }
                    _ => {}
                },
            }
        }
        if let Phase::Countdown { until, .. } = &self.phase {
            if Instant::now() >= *until {
                if let Phase::Countdown { chosen, .. } = std::mem::replace(&mut self.phase, Phase::Idle) {
                    self.launch(chosen, s);
                }
            }
        }
        let silent = matches!(&self.phase, Phase::Live(l) if !l.mic_warned && l.session.mic_silent());
        if silent {
            if let Phase::Live(l) = &mut self.phase {
                l.mic_warned = true;
            }
            eprintln!("[rec] the microphone sends only silence");
            self.hud("The microphone hears nothing: is the lid closed, or are the AirPods out?");
        }
        if !self.camera_warned && self.camera.as_ref().is_some_and(|c| c.stalled()) {
            self.camera_warned = true;
            eprintln!("[rec] the camera sends no picture");
            self.hud("The camera sends no picture: is the lid closed, or is another app using it?");
        }
        let failed = self.live().and_then(|l| l.session.failure());
        if let Some(why) = failed {
            eprintln!("[rec] failed: {why}");
            self.hud(format!("Recording stopped: {why}"));
            self.stop();
        }
        if let (Some(sv), Some(secs)) = (&self.saved, s.card_secs) {
            if sv.born.elapsed().as_secs_f32() > secs as f32 {
                self.saved = None;
            }
        }
    }
}

/// Where framed videos keep their unframed picture, so the frame can be
/// changed or taken off later.
pub fn projects_dir() -> PathBuf {
    super::settings::data_dir().join("projects")
}

/// `--rec-probe <secs> [--no-mic] [--no-sys]`: record the main display for
/// `secs` without any UI, print what the recorder saw (stderr) and where the
/// file went. For checking the capture pipeline from a terminal or a script.
pub fn probe(secs: f64, system_audio: bool, mic: bool) {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
    }
    let s = RecSettings::default();
    let Some(d) = sys::displays().into_iter().next() else {
        eprintln!("[probe] no display");
        return;
    };
    let chosen = chose_display(&d, &s);
    let path = free_path(&std::env::temp_dir(), "rec-probe.mp4");
    let mic_ok = mic && sys::access(Media::Mic) == Access::Allowed;
    eprintln!("[probe] display {} {:?} -> {:?} @{} fps; mic asked {mic}, allowed {mic_ok}", d.name, d.pixels(), chosen.size, chosen.fps);
    let cfg = engine::Config {
        target: chosen.target,
        size: chosen.size,
        fps: chosen.fps,
        bitrate: timeline::bitrate(chosen.size.0, chosen.size.1, chosen.fps, s.quality),
        cursor: true,
        clicks: false,
        system_audio,
        mic: mic_ok.then_some(None),
        keep_window: None,
        path,
    };
    let (tx, rx) = channel();
    engine::start(cfg, Box::new(move |r| drop(tx.send(r))));
    let session = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return eprintln!("[probe] start failed: {e}"),
        Err(_) => return eprintln!("[probe] start timed out"),
    };
    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(250));
        let t = Instant::now();
        let l = session.length();
        eprintln!("[probe] length {l:.2} (took {:?})", t.elapsed());
    }
    let (tx, rx) = channel();
    session.stop(Box::new(move |r| drop(tx.send(r))));
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(st)) => eprintln!("[probe] saved {} ({:.2} s, {:?})", st.path.display(), st.secs, st.size),
        Ok(Err(e)) => eprintln!("[probe] save failed: {e}"),
        Err(_) => eprintln!("[probe] save timed out"),
    }
}

/// A file name in `dir` that isn't taken yet.
pub fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let mut p = dir.join(name);
    let stem = name.trim_end_matches(".mp4");
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} {n}.mp4"));
        n += 1;
    }
    p
}

/// Recording a whole display.
pub fn chose_display(d: &Display, s: &RecSettings) -> Chosen {
    let (w, h) = d.pixels();
    Chosen { target: Target::Display(d.id), display: d.clone(), rect: d.frame, size: timeline::out_size(w, h, s.quality), fps: s.fps.min(d.fps) }
}

/// Recording one window (its frame now, in global points).
pub fn chose_window(d: &Display, w: &Win, s: &RecSettings) -> Chosen {
    let px = (w.frame.width() * d.scale).round() as u32;
    let py = (w.frame.height() * d.scale).round() as u32;
    Chosen { target: Target::Window(w.id), display: d.clone(), rect: w.frame, size: timeline::out_size(px, py, s.quality), fps: s.fps.min(d.fps) }
}

/// Recording part of a display; `area` in global points, kept inside it.
pub fn chose_area(d: &Display, area: Rect, s: &RecSettings) -> Option<Chosen> {
    let a = area.intersect(d.frame);
    if a.width() < 16.0 || a.height() < 16.0 {
        return None;
    }
    // whole points, so the frame and the video agree
    let a = Rect::from_min_max(a.min.round(), a.max.round());
    let local = a.translate(-d.frame.min.to_vec2());
    let (px, py) = ((a.width() * d.scale).round() as u32, (a.height() * d.scale).round() as u32);
    Some(Chosen {
        target: Target::Area { display: d.id, rect: [local.left() as f64, local.top() as f64, local.width() as f64, local.height() as f64] },
        display: d.clone(),
        rect: a,
        size: timeline::out_size(px, py, s.quality),
        fps: s.fps.min(d.fps),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> Display {
        Display { id: 7, name: "Studio Display".into(), frame: Rect::from_min_size(Pos2::new(1512.0, 0.0), Vec2::new(2560.0, 1440.0)), scale: 2.0, fps: 60 }
    }

    #[test]
    fn a_whole_display_records_at_its_pixels_up_to_4k() {
        let s = RecSettings::default();
        let c = chose_display(&display(), &s);
        assert_eq!(c.target, Target::Display(7));
        assert_eq!(c.size, (3840, 2160));
        assert_eq!(c.fps, 60);
    }

    #[test]
    fn an_area_is_local_to_its_display_and_clipped() {
        let s = RecSettings::default();
        let area = Rect::from_min_max(Pos2::new(1612.4, 100.0), Pos2::new(1612.4 + 640.0, 460.0));
        let c = chose_area(&display(), area, &s).unwrap();
        assert_eq!(c.target, Target::Area { display: 7, rect: [100.0, 100.0, 640.0, 360.0] });
        assert_eq!(c.size, (1280, 720));
        let off = Rect::from_min_max(Pos2::new(3900.0, 1300.0), Pos2::new(4500.0, 1700.0));
        let c = chose_area(&display(), off, &s).unwrap();
        assert_eq!(c.rect.max, Pos2::new(4072.0, 1440.0));
        assert!(chose_area(&display(), Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::splat(10.0)), &s).is_none());
    }

    #[test]
    fn file_names_never_clash() {
        let dir = std::env::temp_dir().join(format!("cmb-rec-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = free_path(&dir, "Recording x.mp4");
        std::fs::write(&first, b"").unwrap();
        assert_eq!(free_path(&dir, "Recording x.mp4"), dir.join("Recording x 2.mp4"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
