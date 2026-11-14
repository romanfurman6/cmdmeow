//! Dictation: press the shortcut in any app, speak, and the words are typed
//! where the caret is. Hold the keys to talk (let go to finish), or tap them
//! to start and tap again to finish; Esc throws it away. While listening a
//! small pill sits under the caret. Everything runs on this Mac: Parakeet v3
//! (default) or Whisper turbo.
//!
//! Page routes: "" (as left), "history", "settings".

mod audio;
mod cut;
mod fit;
mod mac;
pub mod probe;
mod settings;
mod ui;
mod words;

use crate::core::tool::{Action, Badge, Cx, Size, Tool, ToolEnv, ToolId, View, WidgetSpec};
use crate::kit::glass::Icon;
use crate::kit::hotkeys::{Keys, Shortcut};
use crate::kit::perms::{Need, Perm, Status};
use crate::kit::whisper::Progress;
use audio::{Mic, MODEL_RATE};
use egui::{Color32, Pos2, Rect, Ui};
use settings::{Engine, History, Said, Settings};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const ID: ToolId = "dictation";
const CARD: &str = "card";
/// The tool's own setup step: the speech model.
const MODEL_NEED: &str = "dictation-model";
#[allow(non_snake_case)]
pub fn TINT() -> Color32 { crate::kit::glass::tint::CAPTURE() }

/// Held shorter than this, the shortcut was a tap: keep listening until the
/// next one.
const HOLD: Duration = Duration::from_millis(350);
/// Longest single dictation.
const MAX_TALK: Duration = Duration::from_secs(300);
/// Shorter than this, nothing was said.
const MIN_VOICE_SECS: f32 = 0.3;
/// A loaded model is let go after this long unused (it holds ~1 GB).
const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);
/// Wait after bringing the app forward before pressing ⌘V.
const PASTE_DELAY: Duration = Duration::from_millis(90);
/// Keep listening this long after the shortcut is let go: people let go
/// while still saying the last word.
const TAIL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    History,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Talk,
    Cancel,
    /// Return while listening: stop now and type it.
    Done,
}

/// `cmdmeow://dictation/<action>`. A link has no key to hold, so it always
/// starts hands-free: it stops after silence, by the shortcut, or by `stop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    Toggle,
    Start,
    Stop,
    Cancel,
}

impl Link {
    fn read(action: &str) -> Option<Link> {
        match action {
            "toggle" => Some(Link::Toggle),
            "start" => Some(Link::Start),
            "stop" => Some(Link::Stop),
            "cancel" => Some(Link::Cancel),
            _ => None,
        }
    }
}

/// What the pill says once the talking is over.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    Pasted,
    /// No Accessibility: the text is on the clipboard for ⌘V.
    Copied,
    Problem(String),
}

/// Where dictation is.
pub enum Phase {
    Idle,
    /// `heard` is when the voice was last above a whisper; `ending` is set
    /// once they've asked to stop and the mic catches the last bit.
    Listening { mic: Mic, since: Instant, tapped: bool, heard: Instant, ending: Option<Instant> },
    /// `done` is how much of the voice the model has finished (0..=1000),
    /// `expect` how long the whole take should take, in seconds.
    Working { job: u64, since: Instant, expect: f32, done: Arc<AtomicU32> },
    Showing { note: Note, until: Instant },
}

/// A guess at the model's pace (seconds of work per second of voice)
/// before the first take has been timed.
const START_PACE: f32 = 0.08;
/// Work every take has, however short.
const MIN_WORK_SECS: f32 = 0.25;

/// How far a take is, 0..=1, for the pill: what the model reports, or the
/// time spent against the time it should take, held short of the end
/// until it's really done.
pub fn work_progress(elapsed: f32, expect: f32, done: u32) -> f32 {
    let guess = (elapsed / expect.max(0.05)).min(1.0);
    // slows down near the end rather than stopping dead
    let guess = 0.92 * (1.0 - (1.0 - guess).powi(2));
    guess.max(done as f32 / 1000.0).clamp(0.0, 1.0)
}

/// How often the pill adds a bar while listening.
const LEVEL_EVERY: Duration = Duration::from_millis(33);

/// Work for the model thread.
enum Job {
    /// Load the model now, while the user is still talking.
    Warm(Engine),
    Run { job: u64, engine: Engine, lang: Option<&'static str>, pcm: Vec<f32>, done: Arc<AtomicU32> },
}

struct Heard {
    job: u64,
    text: Result<String, String>,
}

/// A model download running on a thread.
pub struct Download {
    pub engine: Engine,
    pub progress: Arc<Progress>,
    pub unpacking: Arc<AtomicBool>,
    rx: Receiver<Result<(), String>>,
}

pub struct Dictation {
    settings: Settings,
    /// None for sample data (screenshots), which never saves.
    dir: Option<PathBuf>,
    history: History,
    pub phase: Phase,
    tab: Tab,
    on: bool,
    target: mac::Target,
    /// Where the pill goes: the caret and field, the mouse, the screen they're on.
    place: (mac::Spot, Option<Pos2>, Option<Rect>),
    /// Loudness, newest last, for the pill's bars.
    levels: Vec<f32>,
    /// When the last bar was added.
    level_at: Option<Instant>,
    keys: Option<Keys<Cmd>>,
    esc: Option<Keys<Cmd>>,
    esc_bound: bool,
    links: Option<crate::kit::url_scheme::Inbox<Link>>,
    bound: Option<Shortcut>,
    key_error: Option<String>,
    /// Recording a new shortcut on the Settings tab.
    recording_key: bool,
    /// When the recorder was last drawn; it stops once the page is gone.
    recording_seen: Instant,
    jobs: Sender<Job>,
    heard: Receiver<Heard>,
    next_job: u64,
    voice_secs: f32,
    /// Seconds of work per second of voice, learned from the last takes.
    pace: f32,
    /// The text around the caret when they started.
    around: Option<fit::Around>,
    paste_at: Option<(Instant, String)>,
    /// A paste whose clipboard goes back once the app has read it.
    pending: Option<mac::Pasted>,
    /// A word being typed into the Words list.
    pub word_draft: String,
    pub download: Option<Download>,
    download_error: Option<String>,
    /// Shell requests made outside a ui pass.
    actions: Vec<Action>,
    ctx: egui::Context,
    demo: bool,
    /// The take was started from the page's try-it field (no history
    /// yet): its words land in that field instead of being pasted.
    trying: bool,
    /// When the try-it field was last drawn.
    try_seen: Option<Instant>,
    /// The last take made in the try-it field, shown there until they
    /// leave the page.
    try_last: Option<Said>,
    /// A problem with a take from the try-it field, shown in the field.
    try_note: Option<String>,
    /// The page's Hold to talk button is down.
    try_down: bool,
    /// Our window had the keyboard when the try-it field was last drawn.
    try_focused: bool,
    /// The try-it field was clicked: it shows a caret and waits for the shortcut.
    try_armed: bool,
    /// Sample data (screenshots): what the try-it field shows.
    try_fake: Option<ui::Shown>,
    /// Done was pressed on the take: when, and where the field was, so the
    /// field turns into the newest History entry.
    try_done: Option<(Instant, egui::Rect)>,
    /// Where the try-it field was last drawn.
    try_field: egui::Rect,
}

impl Dictation {
    pub fn new(env: &ToolEnv) -> Self {
        let (jobs, job_rx) = channel();
        let (heard_tx, heard) = channel();
        let (settings, dir, history) = if env.demo {
            (Settings::default(), None, demo_history())
        } else {
            let dir = settings::data_dir();
            (settings::load(&dir), Some(dir.clone()), settings::load_history(&dir))
        };
        if !env.demo {
            let ctx = env.ctx.clone();
            std::thread::Builder::new().name("dictation".into()).spawn(move || worker(job_rx, heard_tx, ctx)).ok();
        }
        Dictation {
            settings,
            dir,
            history,
            phase: Phase::Idle,
            tab: Tab::History,
            on: false,
            target: mac::Target::default(),
            place: (mac::Spot::default(), None, None),
            levels: Vec::new(),
            level_at: None,
            keys: None,
            esc: None,
            esc_bound: false,
            links: (!env.demo).then(|| crate::kit::url_scheme::Inbox::new("dictation", Some(env.ctx.clone()), Link::read)),
            bound: None,
            key_error: None,
            recording_key: false,
            recording_seen: Instant::now(),
            jobs,
            heard,
            next_job: 0,
            voice_secs: 0.0,
            pace: START_PACE,
            around: None,
            paste_at: None,
            pending: None,
            word_draft: String::new(),
            download: None,
            download_error: None,
            actions: Vec::new(),
            ctx: env.ctx.clone(),
            demo: env.demo,
            trying: false,
            try_seen: None,
            try_last: None,
            try_note: None,
            try_down: false,
            try_focused: false,
            try_armed: false,
            try_fake: None,
            try_done: None,
            try_field: egui::Rect::NOTHING,
        }
    }

    fn save_settings(&self) {
        if let Some(d) = &self.dir {
            settings::save(d, &self.settings);
        }
    }

    fn save_history(&self) {
        if let Some(d) = &self.dir {
            settings::save_history(d, &self.history);
        }
    }

    /// Words said today.
    pub fn words_today(&self) -> usize {
        self.history.words_since(today_start())
    }

    pub fn shortcut_label(&self) -> String {
        self.settings.shortcut.as_ref().map(|s| s.display()).unwrap_or_default()
    }

    pub fn listening(&self) -> bool {
        matches!(self.phase, Phase::Listening { .. })
    }

    // ------------------------------------------------------------ shortcuts

    /// Bind the shortcut when it changed, the tool turned on, or recording
    /// a new one ended.
    fn bind_keys(&mut self) {
        let want = if self.on && !self.recording_key { self.settings.shortcut.clone() } else { None };
        if self.keys.is_some() && self.bound == want {
            return;
        }
        if self.keys.is_none() {
            self.keys = Keys::new(self.ctx.clone());
        }
        let Some(keys) = self.keys.as_mut() else {
            self.key_error = Some("Shortcuts are unavailable".into());
            return;
        };
        self.key_error = None;
        match &want {
            Some(s) => {
                if let Some((_, e)) = keys.apply(&[(Cmd::Talk, s.clone())]).into_iter().next() {
                    self.key_error = Some(e);
                }
            }
            None => keys.clear(),
        }
        self.bound = want;
    }

    /// Esc cancels and Return finishes, but only while dictating: they're
    /// every app's keys otherwise.
    fn bind_esc(&mut self) {
        let want = matches!(self.phase, Phase::Listening { .. } | Phase::Working { .. });
        if want == self.esc_bound {
            return;
        }
        if self.esc.is_none() {
            self.esc = Keys::new(self.ctx.clone());
        }
        let Some(esc) = self.esc.as_mut() else { return };
        if want {
            let k = |key: &str| Shortcut { cmd: false, shift: false, alt: false, ctrl: false, key: key.into() };
            esc.apply(&[(Cmd::Cancel, k("Escape")), (Cmd::Done, k("Return"))]);
        } else {
            esc.clear();
        }
        self.esc_bound = want;
    }

    fn release_keys(&mut self) {
        if let Some(k) = self.keys.as_mut() {
            k.clear();
        }
        if let Some(k) = self.esc.as_mut() {
            k.clear();
        }
        self.bound = None;
        self.esc_bound = false;
    }

    // ------------------------------------------------------------ the flow

    fn show(&mut self, note: Note, secs: f32) {
        if self.trying {
            // the try-it field says it; no pill over our own window
            self.trying = false;
            self.phase = Phase::Idle;
            if let Note::Problem(m) = note {
                self.try_note = Some(m);
            }
            return;
        }
        self.phase = Phase::Showing { note, until: Instant::now() + Duration::from_secs_f32(secs) };
    }

    /// The try-it field is on screen (drawn in the last half second).
    fn try_open(&self) -> bool {
        self.try_seen.is_some_and(|t| t.elapsed() < Duration::from_millis(500))
    }

    /// The talk key or the page's Hold to talk button went down.
    fn press_talk(&mut self, from_page: bool) {
        match &self.phase {
            Phase::Idle | Phase::Showing { .. } => self.start_from(from_page),
            Phase::Listening { tapped: true, .. } => self.end(),
            _ => {}
        }
    }

    /// …and came up: a short press keeps listening hands-free, a hold ends
    /// the take.
    fn release_talk(&mut self) {
        if let Phase::Listening { since, tapped, ending, .. } = &mut self.phase {
            if !*tapped && ending.is_none() {
                if since.elapsed() < HOLD {
                    *tapped = true;
                } else {
                    self.end();
                }
            }
        }
    }

    /// The shortcut went down while idle: check what's needed, find the
    /// caret, start the mic.
    pub fn start(&mut self) {
        self.start_from(false);
    }

    /// `from_page`: the page's own button started it. A take started with
    /// the shortcut while our try-it field is in front goes there too.
    fn start_from(&mut self, from_page: bool) {
        self.target = mac::front_app().unwrap_or_default();
        let ours = self.target.pid == std::process::id() as i32 || self.try_focused;
        self.trying = from_page || (ours && self.try_open());
        self.try_note = None;
        let spot = mac::spot(&self.target);
        self.around = mac::around(&self.target);
        let mouse = mac::mouse();
        let at = spot.center().or(mouse).unwrap_or(Pos2::new(600.0, 400.0));
        self.place = (spot, mouse, mac::screen_at(at));
        eprintln!("[dictation] spot {:?}", self.place);
        self.levels.clear();
        match Perm::Microphone.status() {
            Status::Allowed => {}
            Status::NotAsked => {
                Perm::Microphone.ask();
                return self.show(Note::Problem("Allow the microphone".into()), 2.5);
            }
            Status::Denied => {
                crate::kit::perms::open_settings(Perm::Microphone);
                return self.show(Note::Problem("Microphone is off for Cmd+Meow".into()), 3.0);
            }
        }
        let engine = self.settings.engine;
        if !engine.ready() && self.trying {
            // the try-it field shows the download in its button
            self.trying = false;
            if self.download.is_none() {
                self.start_download(engine);
            }
            return;
        }
        if !engine.ready() {
            self.tab = Tab::Settings;
            self.actions.push(Action::OpenPage { tool: ID.into(), route: "settings".into() });
            if self.download.is_none() {
                self.start_download(engine);
            }
            return self.show(Note::Problem("Getting the speech model".into()), 2.5);
        }
        match Mic::start() {
            Ok(mic) => {
                let _ = self.jobs.send(Job::Warm(engine));
                self.phase = Phase::Listening { mic, since: Instant::now(), tapped: false, heard: Instant::now(), ending: None };
            }
            Err(e) => self.show(Note::Problem(e), 2.5),
        }
    }

    /// They asked to stop: listen a moment longer, then finish.
    fn end(&mut self) {
        if let Phase::Listening { ending, .. } = &mut self.phase {
            ending.get_or_insert(Instant::now() + TAIL);
        }
    }

    /// Stop listening and hand the voice to the model.
    fn finish(&mut self) {
        let Phase::Listening { mic, .. } = std::mem::replace(&mut self.phase, Phase::Idle) else { return };
        let pcm = mic.stop();
        let secs = pcm.len() as f32 / MODEL_RATE as f32;
        let peak = pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        eprintln!("[dictation] listened {secs:.1} s, peak {peak:.3}");
        // a real room is never exactly zero: macOS hands out digital silence
        // when it keeps the microphone from us
        if secs >= MIN_VOICE_SECS && peak == 0.0 {
            return self.show(Note::Problem("The microphone gives only silence".into()), 2.5);
        }
        if secs < MIN_VOICE_SECS || audio::silent(&pcm, MODEL_RATE) {
            return self.show(Note::Problem("Didn't hear anything".into()), 1.4);
        }
        // the quiet before and after isn't sent: faster, and models make
        // words up in silence
        let keep = cut::trim(&pcm, MODEL_RATE);
        let pcm = if keep.len() < pcm.len() { pcm[keep].to_vec() } else { pcm };
        self.next_job += 1;
        self.voice_secs = secs;
        let expect = MIN_WORK_SECS + pcm.len() as f32 / MODEL_RATE as f32 * self.pace;
        let done = Arc::new(AtomicU32::new(0));
        let job = Job::Run { job: self.next_job, engine: self.settings.engine, lang: None, pcm, done: done.clone() };
        if self.jobs.send(job).is_err() {
            return self.show(Note::Problem("Speech to text isn't running".into()), 2.5);
        }
        self.phase = Phase::Working { job: self.next_job, since: Instant::now(), expect, done };
    }

    fn follow(&mut self, link: Link) {
        let listening = matches!(self.phase, Phase::Listening { .. });
        match link {
            Link::Toggle if listening => self.finish(),
            Link::Toggle | Link::Start => {
                if matches!(self.phase, Phase::Idle | Phase::Showing { .. }) {
                    self.start();
                    if let Phase::Listening { tapped, .. } = &mut self.phase {
                        *tapped = true;
                    }
                }
            }
            Link::Stop => self.finish(),
            Link::Cancel => self.cancel(),
        }
    }

    fn cancel(&mut self) {
        self.trying = false;
        self.try_down = false;
        if let Phase::Listening { mic, .. } = std::mem::replace(&mut self.phase, Phase::Idle) {
            let _ = mic.stop();
        }
        // a Working job's answer is ignored now: phase is Idle
    }

    fn on_heard(&mut self, h: Heard) {
        let Phase::Working { job, since, .. } = &self.phase else { return };
        if *job != h.job {
            return;
        }
        if h.text.is_ok() && self.voice_secs > 0.5 {
            let pace = (since.elapsed().as_secs_f32() - MIN_WORK_SECS).max(0.0) / self.voice_secs;
            self.pace = (self.pace * 0.6 + pace.min(2.0) * 0.4).max(0.01);
        }
        match h.text {
            Err(e) => self.show(Note::Problem(e), 3.0),
            Ok(t) if t.is_empty() => self.show(Note::Problem("Didn't catch that".into()), 1.6),
            Ok(t) => {
                let t = words::apply(&t, &self.settings.words);
                let said = Said { at: chrono::Local::now().timestamp(), text: t.clone(), app: self.target.name.clone(), secs: self.voice_secs };
                if self.settings.keep_history {
                    self.history.add(said.clone());
                    self.save_history();
                }
                if self.trying {
                    // tried on the page: the words stay in its field
                    self.trying = false;
                    self.try_last = Some(said);
                    self.phase = Phase::Idle;
                    return;
                }
                if mac::accessibility() {
                    mac::activate(&self.target);
                    let t = fit::fit(&t, self.around.as_ref(), &self.settings.words);
                    self.paste_at = Some((Instant::now() + PASTE_DELAY, t));
                    self.show(Note::Pasted, 0.8);
                } else {
                    let _ = mac::paste(&t, false);
                    self.show(Note::Copied, 2.5);
                }
            }
        }
    }

    pub fn start_download(&mut self, engine: Engine) {
        if self.download.is_some() || engine.ready() {
            return;
        }
        let progress = Arc::new(Progress::default());
        let unpacking = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (p, u, ctx) = (progress.clone(), unpacking.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("dictation-model".into())
            .spawn(move || {
                let r = match engine {
                    Engine::Parakeet => crate::kit::parakeet::download(&p, &u),
                    Engine::Whisper => crate::kit::whisper::download(crate::kit::whisper::Model::Accurate, &p),
                };
                let _ = tx.send(r);
                ctx.request_repaint();
            })
            .ok();
        self.download_error = None;
        self.download = Some(Download { engine, progress, unpacking, rx });
    }

    pub fn stop_download(&mut self) {
        if let Some(d) = &self.download {
            d.progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_download(&mut self, ctx: &egui::Context) {
        let Some(d) = &self.download else { return };
        match d.rx.try_recv() {
            Ok(r) => {
                self.download_error = r.err().filter(|e| e != "Stopped");
                self.download = None;
            }
            Err(_) => ctx.request_repaint_after(Duration::from_millis(250)),
        }
    }

    /// Everything that moves the flow on; called on every frame.
    fn step(&mut self, ctx: &egui::Context) {
        self.bind_keys();
        let (presses, releases) = match &self.keys {
            Some(k) => (k.poll(), k.poll_released()),
            None => (Vec::new(), Vec::new()),
        };
        let side = self.esc.as_ref().map(|k| k.poll()).unwrap_or_default();
        let cancels: Vec<_> = side.iter().filter(|c| **c == Cmd::Cancel).collect();
        if side.contains(&Cmd::Done) && matches!(self.phase, Phase::Listening { .. }) {
            self.end();
        }
        for c in presses {
            if c == Cmd::Talk {
                self.press_talk(false);
            }
        }
        for c in releases {
            if c == Cmd::Talk {
                self.release_talk();
            }
        }
        if !cancels.is_empty() {
            self.cancel();
        }
        let links = self.links.as_ref().map(|l| l.take()).unwrap_or_default();
        for l in links {
            self.follow(l);
        }
        let stop_after = self.settings.stop_after_silence();
        if let Phase::Listening { mic, since, tapped, heard, ending } = &mut self.phase {
            let failed = mic.failed();
            let level = mic.level();
            if level > audio::VOICE_LEVEL {
                *heard = Instant::now();
            }
            // hands-free (tapped) only: holding the key is its own stop
            let quiet = *tapped && stop_after.is_some_and(|d| heard.elapsed() > d);
            let ended = ending.is_some_and(|t| Instant::now() >= t);
            let too_long = since.elapsed() > MAX_TALK || quiet || ended;
            if quiet {
                eprintln!("[dictation] stopped after silence");
            }
            // one bar every 33 ms whatever the frame rate, so the bars move evenly
            let due = self.level_at.is_none_or(|t| t.elapsed() >= LEVEL_EVERY);
            if due {
                self.level_at = Some(Instant::now());
                self.levels.push(level);
                if self.levels.len() > 64 {
                    self.levels.remove(0);
                }
            }
            if too_long {
                self.finish();
            } else if let Some(e) = failed {
                self.cancel();
                self.show(Note::Problem(e), 2.5);
            }
        }
        while let Ok(h) = self.heard.try_recv() {
            self.on_heard(h);
        }
        if self.paste_at.as_ref().is_some_and(|(t, _)| Instant::now() >= *t) {
            let (_, text) = self.paste_at.take().unwrap();
            if self.settings.restore_clipboard {
                if let Some(mut p) = mac::paste_tracked(&text) {
                    // a restore still pending holds the clipboard from before
                    if let Some(mut old) = self.pending.take() {
                        p.saved = old.take_saved();
                    }
                    self.pending = Some(p);
                }
            } else {
                let _ = mac::paste(&text, true);
            }
        }
        if self.pending.as_ref().is_some_and(|p| mac::restore_due(p.pressed, p.last_read(), Instant::now())) {
            mac::put_back(self.pending.take().unwrap());
        }
        if self.recording_key && self.recording_seen.elapsed() > Duration::from_secs(1) {
            self.recording_key = false;
        }
        // they left the page: the try-it field starts fresh next time
        if !self.trying && self.try_seen.is_some_and(|t| t.elapsed() > Duration::from_millis(1500)) {
            self.try_seen = None;
            self.try_last = None;
            self.try_note = None;
            self.try_armed = false;
        }
        if let Phase::Showing { until, .. } = self.phase {
            if Instant::now() >= until {
                self.phase = Phase::Idle;
            }
        }
        self.bind_esc();
        self.poll_download(ctx);
        if matches!(self.phase, Phase::Listening { .. } | Phase::Working { .. }) {
            // the pill moves: a steady 60 fps
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if !matches!(self.phase, Phase::Idle) || self.paste_at.is_some() || self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }
}

/// The model thread: keeps one model loaded while it's used, lets it go
/// after a while.
/// A speech model in memory.
pub(crate) enum Loaded {
    Parakeet(crate::kit::parakeet::Loaded),
    Whisper(crate::kit::whisper::Loaded),
}

impl Loaded {
    pub(crate) fn load(e: Engine) -> Result<Loaded, String> {
        match e {
            Engine::Parakeet => crate::kit::parakeet::Loaded::load().map(Loaded::Parakeet),
            Engine::Whisper => crate::kit::whisper::Loaded::load(crate::kit::whisper::Model::Accurate).map(Loaded::Whisper),
        }
    }

    /// The voice as one line of text. A long take goes in pieces cut at
    /// its pauses.
    pub(crate) fn run(&self, pcm: &[f32], lang: Option<&str>, done: Option<&AtomicU32>) -> Result<String, String> {
        let mut said = Vec::new();
        for piece in cut::split(pcm, MODEL_RATE) {
            let end = piece.end;
            let pcm_all = pcm;
            let pcm = &pcm[piece];
            match self {
                Loaded::Parakeet(m) => said.push(m.run(pcm)?),
                Loaded::Whisper(m) => said.extend(m.run(pcm, lang, None)?.into_iter().map(|s| s.2)),
            }
            if let Some(d) = done {
                d.store((end as u64 * 1000 / pcm_all.len().max(1) as u64) as u32, Ordering::Relaxed);
            }
        }
        Ok(audio::tidy_text(&said))
    }
}

fn worker(rx: Receiver<Job>, tx: Sender<Heard>, ctx: egui::Context) {
    // below the UI: the model's threads (they inherit this) must not starve
    // the pill's spinner while it transcribes
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
    let mut loaded: Option<(Engine, Loaded)> = None;
    loop {
        let job = match rx.recv_timeout(IDLE_UNLOAD) {
            Ok(j) => j,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                loaded = None;
                continue;
            }
            Err(_) => return,
        };
        let engine = match &job {
            Job::Warm(e) | Job::Run { engine: e, .. } => *e,
        };
        if loaded.as_ref().is_none_or(|(e, _)| *e != engine) {
            loaded = None;
            match Loaded::load(engine) {
                Ok(l) => loaded = Some((engine, l)),
                Err(e) => {
                    eprintln!("[dictation] {e}");
                    if let Job::Run { job, .. } = job {
                        let _ = tx.send(Heard { job, text: Err(e) });
                        ctx.request_repaint();
                    }
                    continue;
                }
            }
        }
        let Job::Run { job, lang, pcm, done, .. } = job else { continue };
        let t0 = Instant::now();
        let text = match &loaded {
            Some((_, m)) => m.run(&pcm, lang, Some(&done)),
            None => Err("The speech model didn't load".into()),
        };
        eprintln!("[dictation] {:.1} s of voice in {} ms", pcm.len() as f32 / MODEL_RATE as f32, t0.elapsed().as_millis());
        let _ = tx.send(Heard { job, text });
        ctx.request_repaint();
    }
}

fn today_start() -> i64 {
    use chrono::{Local, TimeZone};
    let now = Local::now();
    now.date_naive().and_hms_opt(0, 0, 0).and_then(|d| Local.from_local_datetime(&d).single()).map(|d| d.timestamp()).unwrap_or(now.timestamp())
}

fn demo_history() -> History {
    let now = chrono::Local::now().timestamp();
    let mut h = History::default();
    for (ago, text, app) in [
        (3 * 3600, "Ship the dictation widget before Friday, then we test it on the real Mac together.", "Slack"),
        (40 * 60, "Привіт! Скинь, будь ласка, посилання на дизайн, я гляну ввечері.", "Telegram"),
        (12 * 60, "Fix the caret position in Electron apps and fall back to the mouse.", "Linear"),
        (90, "Треба оновити README і додати скріншоти нового вікна.", "Notes"),
    ] {
        h.add(Said { at: now - ago, text: text.into(), app: app.into(), secs: text.len() as f32 / 14.0 });
    }
    h
}

fn badge() -> Badge {
    Badge { icon: Icon::Mic, color: TINT() }
}

impl Tool for Dictation {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Dictation"
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn section(&self) -> &'static str {
        "Create"
    }
    fn about(&self) -> &'static str {
        "Speak, and the text is typed where the cursor is"
    }
    fn sidebar_meta(&self) -> String {
        if self.listening() { "Listening".into() } else { self.shortcut_label() }
    }
    fn needs(&self) -> Vec<Need> {
        // the speech model is a download, not a permission: setup has a step for it
        vec![Need::must(Perm::Microphone), Need::must(Perm::Accessibility)]
    }
    fn setup_models(&self) -> Vec<crate::core::tool::SetupModel> {
        use crate::core::tool::{ModelState, SetupModel};
        let engine = self.settings.engine;
        // sample data (screenshots) shows a download under way
        if self.dir.is_none() {
            let total = engine.bytes();
            return vec![SetupModel { id: MODEL_NEED, what: "Speech to text", name: engine.name(), bytes: total, state: ModelState::Downloading { done: total * 42 / 100, total } }];
        }
        let state = match &self.download {
            Some(d) if d.unpacking.load(Ordering::Relaxed) => ModelState::Unpacking,
            Some(d) => ModelState::Downloading { done: d.progress.done.load(Ordering::Relaxed), total: d.progress.total.load(Ordering::Relaxed) },
            None if engine.ready() => ModelState::Ready,
            None => match &self.download_error {
                Some(e) => ModelState::Failed(e.clone()),
                None => ModelState::Missing,
            },
        };
        vec![SetupModel { id: MODEL_NEED, what: "Speech to text", name: engine.name(), bytes: engine.bytes(), state }]
    }
    fn download_model(&mut self, id: &str) {
        if id == MODEL_NEED {
            self.start_download(self.settings.engine);
        }
    }
    fn setup_shortcuts(&self) -> Vec<(&'static str, String)> {
        self.settings.shortcut.as_ref().map(|s| vec![("Dictate", s.display())]).unwrap_or_default()
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: "Dictation", about: "Your shortcut, words today and the latest dictations.", sizes: &[Size::S11, Size::S14, Size::S24], size: Size::S11, default_on: true }]
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::widget(self, size, ui, rect, cx);
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        match route {
            "history" => self.tab = Tab::History,
            "settings" => self.tab = Tab::Settings,
            _ => {}
        }
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::page(self, ui, rect, cx);
    }
    fn set_on(&mut self, on: bool) {
        self.on = on;
        // links that came while off aren't run late
        if let Some(l) = &self.links {
            l.take();
        }
        if !on {
            self.cancel();
            self.paste_at = None;
            if let Some(p) = self.pending.take() {
                mac::put_back(p);
            }
            self.recording_key = false;
            self.release_keys();
        }
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        if self.demo {
            return false;
        }
        // a tool left on is never told so; it is on once it ticks
        self.on = true;
        self.step(ctx);
        !matches!(self.phase, Phase::Idle) || !self.actions.is_empty()
    }
    fn windows(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        for a in self.actions.drain(..) {
            cx.push(a);
        }
        ui::pill(self, ctx);
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "dictation" => {
                self.tab = Tab::History;
                Some(View::Page)
            }
            "dictation-settings" => {
                self.tab = Tab::Settings;
                Some(View::Page)
            }
            // the first visit: the try-it field, in each of its states
            "dictation-empty" | "dictation-try-armed" | "dictation-try-listening" | "dictation-try-holding" | "dictation-try-working" | "dictation-try-done" | "dictation-try-model" | "dictation-try-download" => {
                self.tab = Tab::History;
                self.history = History::default();
                self.try_armed = view != "dictation-empty";
                let words = "Okay, this is my first dictation and it already works.";
                self.try_fake = Some(match view {
                    "dictation-try-listening" => ui::Shown::Listening { secs: 4.0, tapped: true },
                    "dictation-try-holding" => ui::Shown::Listening { secs: 4.0, tapped: false },
                    "dictation-try-working" => ui::Shown::Working { frac: 0.6 },
                    "dictation-try-done" => ui::Shown::Done(Said { at: chrono::Local::now().timestamp(), text: words.into(), app: String::new(), secs: 4.0 }),
                    "dictation-try-model" => ui::Shown::NeedModel { failed: None },
                    "dictation-try-download" => ui::Shown::Downloading { frac: 0.44, done: 212_000_000, total: crate::kit::parakeet::BYTES, unpacking: false },
                    _ => ui::Shown::Idle,
                });
                Some(View::Page)
            }
            // Done on the first take: the field turns into the History entry
            // (CMDMEOW_DICT_T picks the moment, in seconds)
            "dictation-try-finish" => {
                self.tab = Tab::History;
                self.history = History::default();
                self.history.add(Said { at: chrono::Local::now().timestamp(), text: "Okay, this is my first dictation and it already works.".into(), app: String::new(), secs: 4.0 });
                let at = std::env::var("CMDMEOW_DICT_T").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.2);
                let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(480.0, 112.0));
                self.try_done = Some((Instant::now() - Duration::from_secs_f32(at), field));
                Some(View::Page)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_name_their_action() {
        assert_eq!(Link::read("toggle"), Some(Link::Toggle));
        assert_eq!(Link::read("start"), Some(Link::Start));
        assert_eq!(Link::read("stop"), Some(Link::Stop));
        assert_eq!(Link::read("cancel"), Some(Link::Cancel));
        assert_eq!(Link::read("talk"), None);
    }

    #[test]
    fn stop_and_cancel_links_do_nothing_while_idle() {
        let mut d = demo();
        d.follow(Link::Stop);
        d.follow(Link::Cancel);
        assert!(matches!(d.phase, Phase::Idle));
    }

    fn demo() -> Dictation {
        Dictation::new(&ToolEnv { ctx: egui::Context::default(), demo: true })
    }

    fn working(d: &mut Dictation) {
        d.next_job += 1;
        d.phase = Phase::Working { job: d.next_job, since: Instant::now(), expect: 1.0, done: Arc::new(AtomicU32::new(0)) };
    }

    #[test]
    fn a_take_from_the_try_field_stays_in_the_field() {
        let mut d = demo();
        d.history = History::default();
        d.trying = true;
        working(&mut d);
        d.on_heard(Heard { job: d.next_job, text: Ok("hello there".into()) });
        assert_eq!(d.try_last.as_ref().map(|s| s.text.as_str()), Some("hello there"));
        assert!(!d.trying);
        assert!(matches!(d.phase, Phase::Idle));
        // nothing waits to be pasted into another app
        assert!(d.paste_at.is_none());
        assert_eq!(d.history.items.len(), 1);
    }

    #[test]
    fn a_problem_while_trying_shows_in_the_field_not_the_pill() {
        let mut d = demo();
        d.trying = true;
        working(&mut d);
        d.on_heard(Heard { job: d.next_job, text: Ok(String::new()) });
        assert_eq!(d.try_note.as_deref(), Some("Didn't catch that"));
        assert!(matches!(d.phase, Phase::Idle));
    }

    #[test]
    fn sample_data_counts_words_today() {
        let d = demo();
        assert_eq!(d.history.items.len(), 4);
        assert!(d.words_today() <= d.history.items.iter().map(|s| s.text.split_whitespace().count()).sum());
        assert_eq!(d.shortcut_label(), "⌥D");
    }

    #[test]
    fn needs_mic_accessibility_and_the_model() {
        let d = demo();
        let n = d.needs();
        assert!(n.iter().any(|x| x.perm == Perm::Microphone));
        assert!(n.iter().any(|x| x.perm == Perm::Accessibility));
        // the model is its own setup step, not a permission row
        assert!(!n.iter().any(|x| matches!(x.perm, Perm::Own { .. })));
        let m = d.setup_models();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, MODEL_NEED);
        assert!(m[0].bytes > 0);
    }

    // the stand-in microphone exists off macOS only
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn letting_go_keeps_listening_a_moment() {
        let mut d = demo();
        d.phase = Phase::Listening { mic: Mic, since: Instant::now(), tapped: true, heard: Instant::now(), ending: None };
        d.end();
        let Phase::Listening { ending: Some(first), .. } = d.phase else { panic!("stopped at once") };
        assert!(first > Instant::now());
        // asking again doesn't push the end out
        d.end();
        assert!(matches!(d.phase, Phase::Listening { ending: Some(t), .. } if t == first));
    }

    #[test]
    fn an_answer_for_an_old_job_is_ignored() {
        let mut d = demo();
        d.phase = Phase::Working { job: 2, since: Instant::now(), expect: 1.0, done: Default::default() };
        d.on_heard(Heard { job: 1, text: Ok("old".into()) });
        assert!(matches!(d.phase, Phase::Working { job: 2, .. }));
        d.on_heard(Heard { job: 2, text: Ok(String::new()) });
        assert!(matches!(&d.phase, Phase::Showing { note: Note::Problem(_), .. }));
    }

    #[test]
    fn the_work_bar_moves_on_its_own_and_only_ends_when_done() {
        assert_eq!(work_progress(0.0, 1.0, 0), 0.0);
        let half = work_progress(0.5, 1.0, 0);
        assert!(half > 0.5 && half < 0.92);
        // late: waits just short of the end
        assert!(work_progress(5.0, 1.0, 0) < 0.93);
        // the model's own count wins when it's further
        assert_eq!(work_progress(0.1, 1.0, 1000), 1.0);
        assert!(work_progress(0.2, 1.0, 100) < work_progress(0.4, 1.0, 100));
    }
}
