//! Capture: screenshots of an area, a window or the screen with macOS's own
//! `screencapture`, system-wide shortcuts, a floating preview, an editor,
//! pinned captures, text recognition, a color picker and a ruler, plus a
//! History and a Settings page.
//!
//! Screen recording (mp4, with system sound, microphone and camera) lives
//! in [`record`].
//!
//! Page routes: "" (as left), "history", "settings".

pub mod editor;
mod folder;
mod grab;
mod hotkeys;
mod icons;
mod mac;
pub mod record;
mod render;
mod settings;
mod store;
mod tools;
mod ui;
pub mod share;
pub mod video;
mod windows;

use crate::core::tool::{Action, Badge, Cx, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::glass::{tint, Icon};
use editor::Editor;
use egui::{Rect, Ui};
use grab::Grab;
use image::RgbaImage;
use settings::{Command, Settings};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use store::{Kind, Store};

pub const ID: ToolId = "capture";
const CARD: &str = "capture";
/// The Video tool's id and its widget: recording lives in the same state
/// as screenshots (one engine, one settings file), shown as its own tool.
pub const VIDEO_ID: ToolId = "video";
/// The Sharing page: the storage both use, and what's been shared.
pub const SHARING_ID: ToolId = "sharing";
const VIDEO_CARD: &str = "record";
pub use tools::tools;

/// What a grab is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    /// A capture for history (preview, editor…).
    Keep(Kind),
    /// Copy the text in the area.
    Text,
    /// Freeze the screen for the picker or the ruler.
    Measure(Measure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    Color,
    Ruler,
}

/// A new capture's small pictures, made off the UI thread.
struct Small {
    thumb: egui::ColorImage,
    preview: egui::ColorImage,
}

/// Results from background threads.
enum Done {
    /// A picture, with its thumbnail when it's for history.
    Grab(Purpose, Result<Option<(grab::Grabbed, Option<Small>)>, String>),
    Text(Vec<mac::TextLine>),
    /// Something to say once background work finished.
    Note(String),
    /// A copy was saved to the user's folder (for a history entry); `announce`
    /// says where in a toast.
    Saved(Option<u64>, Result<PathBuf, String>, bool),
    Thumb(u64, Option<egui::ColorImage>),
    Folder(Option<PathBuf>),
    Files(PathBuf, Vec<folder::FileShot>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    History,
    Folder,
    Settings,
}

/// The Video page's tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VTab {
    Recordings,
    Settings,
}

/// Small textures of history captures, loaded on a thread when first shown.
#[derive(Default)]
struct Thumbs {
    tex: HashMap<u64, egui::TextureHandle>,
    pending: HashSet<u64>,
}

/// Longest side of a thumbnail, in pixels.
const THUMB_PX: u32 = 360;

/// Longest side of the floating preview's picture, in pixels (sharp at 2×).
const PREVIEW_PX: u32 = 640;

fn thumb_image(img: &RgbaImage) -> egui::ColorImage {
    scaled_image(img, THUMB_PX)
}

/// `img` shrunk so its longest side is at most `max_px`.
pub fn scaled_image(img: &RgbaImage, max_px: u32) -> egui::ColorImage {
    let (w, h) = img.dimensions();
    let k = (max_px as f32 / w.max(h).max(1) as f32).min(1.0);
    let t = image::imageops::thumbnail(img, ((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1));
    egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
}

pub struct Capture {
    settings: Settings,
    /// Where settings live; None for sample data (screenshots), which never saves.
    dir: Option<PathBuf>,
    store: Store,
    keys: Option<hotkeys::Keys>,
    key_errors: Vec<(Command, String)>,
    /// Commands to run on the next ui pass, with their delay in seconds.
    queue: Vec<(Command, u32)>,
    /// `cmdmeow://capture/<command>` links.
    links: Option<crate::kit::url_scheme::Inbox<Command>>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// A grab or text recognition is running.
    busy: bool,
    countdown: Option<windows::Countdown>,
    preview: Option<windows::Preview>,
    pins: Vec<windows::Pin>,
    editor: Option<windows::EditorWin>,
    overlay: Option<windows::Overlay>,
    hud: Option<(String, Instant)>,
    /// The Screen Recording card, with the capture that waits for it.
    perm_card: Option<(Grab, Purpose, u32)>,
    /// The user said "Not now" this session: capture without asking.
    perm_skip: bool,
    perm_needed: bool,
    thumbs: Thumbs,
    /// Images in the screenshot folder, newest first.
    files: Vec<folder::FileShot>,
    files_at: Option<Instant>,
    files_busy: bool,
    tab: Tab,
    vtab: VTab,
    /// Which of the two tools are on: Screenshot (`shot_on`) and Video.
    shot_on: bool,
    video_on: bool,
    /// Recording a new shortcut for this command.
    recording: Option<Command>,
    filter: usize,
    confirm_clear: Option<Instant>,
    /// Shell requests made outside a ui pass.
    actions: Vec<Action>,
    /// Points → pixels of the main screen.
    ppp: f32,
    demo_editor: Option<Editor>,
    ctx: egui::Context,
    next_pin: u64,
    rec: record::Recorder,
    /// The video viewer, when one is open.
    video: Option<video::Viewer>,
    /// Recordings being uploaded for a link.
    uploads: share::Uploads,
    /// The secret key as it's being typed in settings (never shown back).
    share_secret: String,
    /// Whether a secret key is in the Keychain: read once, off the frame loop
    /// (and never in demo mode, where a Keychain prompt would block).
    pub(crate) has_secret: Option<bool>,
}

impl Capture {
    /// The Whisper models for transcripts of recordings: the ones already on
    /// this Mac, or the default one, fetched on the first Transcribe.
    pub fn transcript_models(&self) -> Vec<crate::core::tool::SetupModel> {
        use crate::core::tool::{ModelState, SetupModel};
        use crate::kit::whisper::Model;
        let row = |m: Model, state| SetupModel { id: m.file(), what: "Transcripts of recordings", name: m.setup_name(), bytes: m.bytes(), state };
        let found: Vec<SetupModel> = if self.dir.is_none() { Vec::new() } else { Model::ALL.into_iter().filter(|m| m.ready()).map(|m| row(m, ModelState::Ready)).collect() };
        if found.is_empty() {
            vec![row(Model::default(), ModelState::OnFirstUse)]
        } else {
            found
        }
    }

    pub fn new(env: &ToolEnv) -> Self {
        let (tx, rx) = channel();
        let (settings, dir, store) = if env.demo {
            let mut s = Settings::default();
            s.folder = demo_folder();
            (s, None, demo_store())
        } else {
            let dir = settings::data_dir();
            (settings::load(&dir), Some(dir.clone()), Store::open(dir.join("history")))
        };
        let mut c = Capture {
            settings,
            dir,
            store,
            keys: if env.demo { None } else { hotkeys::Keys::new(env.ctx.clone()) },
            key_errors: Vec::new(),
            queue: Vec::new(),
            links: (!env.demo).then(|| crate::kit::url_scheme::Inbox::new("capture", Some(env.ctx.clone()), Command::from_slug)),
            tx,
            rx,
            busy: false,
            countdown: None,
            preview: None,
            pins: Vec::new(),
            editor: None,
            overlay: None,
            hud: None,
            perm_card: None,
            perm_skip: false,
            perm_needed: false,
            thumbs: Thumbs::default(),
            files: Vec::new(),
            files_at: None,
            files_busy: false,
            tab: Tab::History,
            vtab: VTab::Recordings,
            shot_on: true,
            video_on: true,
            recording: None,
            filter: 0,
            confirm_clear: None,
            actions: Vec::new(),
            ppp: if cfg!(target_os = "macos") { 2.0 } else { 1.0 },
            demo_editor: None,
            ctx: env.ctx.clone(),
            next_pin: 1,
            rec: record::Recorder::new(env.ctx.clone()),
            video: None,
            uploads: share::Uploads::default(),
            share_secret: String::new(),
            has_secret: if env.demo { Some(false) } else { None },
        };
        c.apply_keys();
        c
    }

    fn save_settings(&self) {
        if let Some(dir) = &self.dir {
            settings::save(dir, &self.settings);
        }
    }

    /// Register the shortcuts from settings (not while one is being recorded).
    fn apply_keys(&mut self) {
        let (shot, video) = (self.shot_on, self.video_on);
        if let Some(k) = &mut self.keys {
            // only the keys of the tools that are on
            let bound: Vec<_> = self.settings.bound().into_iter().filter(|(c, _)| if *c == Command::Record { video } else { shot }).collect();
            self.key_errors = k.apply(&bound);
        }
    }

    /// Turn Screenshot and/or Video on or off. Both off is the old "off";
    /// one of them off drops its keys and closes what belongs to it.
    fn set_parts(&mut self, shot: Option<bool>, video: Option<bool>) {
        let was = self.shot_on || self.video_on;
        if let Some(v) = shot {
            self.shot_on = v;
        }
        if let Some(v) = video {
            self.video_on = v;
        }
        let any = self.shot_on || self.video_on;
        if !any {
            Tool::set_on(self, false);
            return;
        }
        if !was {
            Tool::set_on(self, true);
        }
        if shot == Some(false) {
            self.queue.retain(|(c, _)| *c == Command::Record);
            self.countdown = None;
            self.preview = None;
            self.pins.clear();
            self.overlay = None;
            if let Some(mut ew) = self.editor.take() {
                self.commit_edit(&mut ew.ed);
            }
        }
        if video == Some(false) {
            self.queue.retain(|(c, _)| *c != Command::Record);
            if self.rec.live().is_some() {
                self.rec.stop();
            } else if !matches!(self.rec.phase, record::Phase::Saving) {
                self.rec.cancel();
            }
        }
        if self.recording.is_none() {
            self.apply_keys();
        }
    }

    fn hud(&mut self, msg: impl Into<String>) {
        self.hud = Some((msg.into(), Instant::now()));
        self.ctx.request_repaint();
    }

    /// Put a picture on the clipboard. Encoding a big capture takes a
    /// moment, so it happens on a thread; a failure shows as a toast.
    fn copy_image(&self, img: RgbaImage) {
        self.in_background(move || (!mac::copy_png(&store::png_bytes(&img))).then(|| Done::Note("Couldn't copy".into())));
    }

    /// [`Capture::copy_image`] for a picture already encoded as PNG.
    fn copy_png(&self, png: Vec<u8>) {
        self.in_background(move || (!mac::copy_png(&png)).then(|| Done::Note("Couldn't copy".into())));
    }

    /// Run `job` on a thread; what it returns comes back to [`Capture::done`].
    fn in_background(&self, job: impl FnOnce() -> Option<Done> + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            if let Some(d) = job() {
                let _ = tx.send(d);
                ctx.request_repaint();
            }
        });
    }

    fn copy_text(&self, text: &str) {
        if !mac::copy_text(text) {
            self.ctx.copy_text(text.to_owned());
        }
    }

    /// Start a command (from a shortcut or a button). Needs the shell for
    /// hiding the panel, so it runs on a ui pass.
    fn run(&mut self, c: Command, delay: u32, cx: &mut Cx) {
        match c {
            Command::Area => self.start(Grab::Area, Purpose::Keep(Kind::Area), delay, cx),
            Command::Window => self.start(Grab::Window, Purpose::Keep(Kind::Window), delay, cx),
            Command::Screen => self.start(Grab::Screen, Purpose::Keep(Kind::Screen), delay, cx),
            Command::Delayed => {
                let d = if self.settings.delay > 0 { self.settings.delay } else { settings::DEFAULT_DELAY };
                self.start(Grab::Screen, Purpose::Keep(Kind::Screen), d, cx)
            }
            Command::Text => self.start(Grab::Area, Purpose::Text, 0, cx),
            Command::Color => self.start(Grab::Screen, Purpose::Measure(Measure::Color), 0, cx),
            Command::Ruler => self.start(Grab::Screen, Purpose::Measure(Measure::Ruler), 0, cx),
            Command::History => {
                self.tab = Tab::History;
                cx.open_page(ID, "history");
            }
            Command::Record => {
                if self.rec.idle() {
                    cx.push(Action::HidePanel);
                    self.preview = None;
                    if !mac::can_record_screen() {
                        self.perm_needed = true;
                        mac::ask_to_record_screen();
                        self.hud("Allow Screen Recording for Cmd+Meow, then try again");
                        return;
                    }
                }
                self.rec.folder = self.settings.folder.clone();
                self.rec.begin(&self.settings.rec);
            }
        }
    }

    /// A recording is saved: into history (a frame as its picture), then
    /// the card in the corner.
    fn keep_recording(&mut self, st: record::engine::Stopped) {
        let thumb = st.thumb.unwrap_or_else(|| image::RgbaImage::from_pixel(640, 360, image::Rgba([40, 40, 46, 255])));
        let id = match self.store.add(&thumb, Kind::Recording, self.settings.keep) {
            Ok(id) => {
                self.store.set_video(id, st.path.clone(), st.secs as f32);
                self.put_thumb(id, &thumb);
                self.forget_dropped_thumbs();
                Some(id)
            }
            Err(e) => {
                self.hud(format!("Couldn't keep it in history: {e}"));
                None
            }
        };
        self.files_at = None;
        if self.settings.share.auto && self.settings.share.set_up(share::VIDEO_ACCOUNT) {
            self.share(st.path.clone());
        }
        if self.settings.rec.open_editor {
            self.open_video(st.path);
            return;
        }
        let bytes = std::fs::metadata(&st.path).map(|m| m.len()).unwrap_or(0);
        let display = self.rec.last_display;
        self.rec.saved = Some(record::Saved { path: st.path, secs: st.secs, size: st.size, bytes, thumb: Some(thumb), tex: None, history: id, display, born: Instant::now(), key: Instant::now() });
    }

    /// Upload `path` for a link (again, if it was shared before: the link
    /// stays the same).
    fn share(&mut self, path: PathBuf) {
        if let Err(e) = self.try_share(path) {
            self.hud(e);
        }
    }

    /// [`Capture::share`] that hands a failure back instead of a toast.
    fn try_share(&mut self, path: PathBuf) -> Result<(), String> {
        let (s, account) = self.share_for(&path);
        self.uploads.start(&s, account, path, &self.ctx)
    }

    /// The sharing settings for `path`: the one storage, with the video's
    /// or the screenshot's folder and switches.
    fn share_for(&self, path: &std::path::Path) -> (share::Settings, &'static str) {
        if !share::is_image(path) {
            return (self.settings.share.clone(), share::VIDEO_ACCOUNT);
        }
        let own = &self.settings.shot_share;
        let s = share::Settings { auto: own.auto, copy_link: own.copy_link, folder: own.folder.clone(), ..self.settings.share.clone() };
        (s, share::VIDEO_ACCOUNT)
    }

    /// Remove a shared file from the storage, then forget it here.
    fn delete_shared(&mut self, path: PathBuf, key: String) {
        let b = self.settings.share.bucket(share::VIDEO_ACCOUNT);
        self.in_background(move || match share::s3::delete(&b, &key) {
            Ok(()) => {
                share::forget(&path);
                Some(Done::Note("Deleted from your storage".into()))
            }
            Err(e) => Some(Done::Note(e)),
        });
    }

    /// Copy the link of `path`: shared already, or upload it first.
    fn copy_link(&mut self, path: PathBuf) {
        if self.uploads.progress(&path).is_some() {
            return;
        }
        match share::link_for(&path) {
            Some(link) => {
                self.copy_known_link(&link);
                self.hud("Link copied");
            }
            None => self.share(path),
        }
    }

    /// Put a link made earlier on the clipboard, and make sure the share
    /// page can still read the bucket (it may have moved since).
    fn copy_known_link(&mut self, link: &str) {
        self.copy_text(link);
        if self.settings.share.set_up(share::VIDEO_ACCOUNT) {
            share::allow_page_later(self.settings.share.bucket(share::VIDEO_ACCOUNT));
        }
    }

    /// Finished uploads: copy the link, say so.
    fn uploads_done(&mut self) {
        for (path, r) in self.uploads.poll() {
            // the editor's Share: its own button shows the progress, and one
            // note at its bottom says how it ended
            if self.editor.as_ref().is_some_and(|ew| ew.share.as_deref() == Some(path.as_path())) {
                let note = match r {
                    Ok(link) => {
                        self.copy_text(&link);
                        Some("Link copied".to_string())
                    }
                    Err(e) if e == "Stopped" => None,
                    Err(e) => {
                        eprintln!("[share] {}: {e}", path.display());
                        Some(e)
                    }
                };
                if let Some(ew) = self.editor.as_mut() {
                    ew.share = None;
                    if let Some(n) = note {
                        ew.ed.flash(n);
                    }
                }
                continue;
            }
            match r {
                Ok(link) => {
                    if self.share_for(&path).0.copy_link {
                        self.copy_text(&link);
                        self.hud("Uploaded · link copied");
                    } else {
                        self.hud("Uploaded");
                    }
                }
                Err(e) if e == "Stopped" => {}
                Err(e) => {
                    eprintln!("[share] {}: {e}", path.display());
                    self.hud(format!("{e} ({})", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                }
            }
        }
    }

    /// Hand what the recorder asked for to the rest of Capture.
    fn recorder_out(&mut self) {
        for o in std::mem::take(&mut self.rec.out) {
            match o {
                record::Out::Hud(m) => self.hud(m),
                record::Out::SaveSettings => self.save_settings(),
                record::Out::Saved(st) => self.keep_recording(st),
            }
        }
    }

    fn start(&mut self, g: Grab, p: Purpose, delay: u32, cx: &mut Cx) {
        if self.busy || self.countdown.is_some() || self.overlay.is_some() {
            return;
        }
        cx.push(Action::HidePanel);
        self.preview = None;
        if !self.perm_skip && !mac::can_record_screen() {
            self.perm_needed = true;
            self.perm_card = Some((g, p, delay));
            return;
        }
        if delay > 0 {
            self.countdown = Some(windows::Countdown { until: Instant::now() + std::time::Duration::from_secs(delay as u64), total: delay, grab: g, purpose: p });
        } else {
            // a moment for the panel or window to get out of the picture
            // (none needed when neither is showing, e.g. from a shortcut)
            let settle = if !crate::kit::activity::visible() { 0 } else if g == Grab::Screen { 250 } else { 120 };
            self.spawn_grab(g, p, settle);
        }
    }

    fn spawn_grab(&mut self, g: Grab, p: Purpose, settle_ms: u64) {
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(settle_ms));
            let shot = grab::shoot(g);
            let d = match (p, shot) {
                (_, Err(e)) => Done::Grab(p, Err(e)),
                (_, Ok(None)) => Done::Grab(p, Ok(None)),
                // text straight from screencapture's file: nothing decoded or encoded
                (Purpose::Text, Ok(Some(path))) => {
                    let _ = tx.send(Done::Note(mac::ocr_status().into()));
                    ctx.request_repaint();
                    let lines = mac::recognize_file(&path);
                    let _ = std::fs::remove_file(&path);
                    Done::Text(lines)
                }
                // decoding and the thumbnail happen here, not on the UI thread
                (_, Ok(Some(path))) => Done::Grab(p, grab::load(&path).map(|g| {
                    let small = matches!(p, Purpose::Keep(_)).then(|| Small { thumb: thumb_image(&g.img), preview: scaled_image(&g.img, PREVIEW_PX) });
                    Some((g, small))
                })),
            };
            let _ = tx.send(d);
            ctx.request_repaint();
        });
    }

    fn spawn_ocr(&mut self, img: RgbaImage) {
        self.busy = true;
        self.hud(mac::ocr_status());
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Done::Text(mac::recognize_image(&img)));
            ctx.request_repaint();
        });
    }

    fn done(&mut self, d: Done) {
        match d {
            Done::Grab(p, r) => {
                self.busy = false;
                match (p, r) {
                    (_, Ok(None)) => {}
                    (_, Err(e)) => self.hud(format!("Couldn't capture: {e}")),
                    (Purpose::Keep(kind), Ok(Some((g, small)))) => self.keep(g, small, kind),
                    (Purpose::Text, Ok(Some((g, _)))) => self.spawn_ocr(g.img),
                    (Purpose::Measure(m), Ok(Some((g, _)))) => self.overlay = Some(windows::Overlay::new(m, g.img)),
                }
            }
            Done::Note(msg) => self.hud(msg),
            Done::Saved(id, r, announce) => match r {
                Ok(path) => {
                    if let Some(id) = id {
                        self.store.set_saved(id, path.clone());
                    }
                    self.files_at = None;
                    if announce {
                        self.hud(format!("Saved to {}", windows::short_path(&path)));
                    }
                }
                Err(e) => self.hud(format!("Couldn't save: {e}")),
            },
            Done::Text(lines) => {
                self.busy = false;
                if lines.is_empty() {
                    self.hud("No text found");
                } else {
                    self.copy_text(&mac::join_lines(&lines, self.settings.keep_breaks));
                    let n = lines.len();
                    self.hud(format!("Copied {n} line{}", if n == 1 { "" } else { "s" }));
                }
            }
            Done::Thumb(id, img) => {
                self.thumbs.pending.remove(&id);
                if let Some(ci) = img {
                    let t = self.ctx.load_texture(format!("capture-thumb-{id}"), ci, egui::TextureOptions::LINEAR);
                    self.thumbs.tex.insert(id, t);
                }
            }
            Done::Folder(Some(p)) => {
                self.settings.folder = p;
                self.save_settings();
                self.files.clear();
                self.files_at = None;
            }
            Done::Folder(None) => {}
            Done::Files(dir, files) => {
                self.files_busy = false;
                if dir == self.settings.folder {
                    self.files = files;
                    self.files_at = Some(Instant::now());
                    self.forget_dropped_thumbs();
                } else {
                    self.files_at = None;
                }
            }
        }
    }

    /// A fresh capture: history, then copy / save / sound, then what the
    /// user picked to happen next.
    /// A new capture: into history, then the clipboard and the folder (on a
    /// thread, from the PNG screencapture wrote, so the preview shows at once).
    fn keep(&mut self, g: grab::Grabbed, small: Option<Small>, kind: Kind) {
        let grab::Grabbed { png, img } = g;
        let (thumb, preview) = match small {
            Some(s) => (Some(s.thumb), Some(s.preview)),
            None => (None, None),
        };
        let id = match self.store.add_png(&png, img.dimensions(), kind, self.settings.keep) {
            Ok(id) => {
                let t = thumb.unwrap_or_else(|| thumb_image(&img));
                self.thumbs.tex.insert(id, self.ctx.load_texture(format!("capture-thumb-{id}"), t, egui::TextureOptions::LINEAR));
                Some(id)
            }
            Err(e) => {
                self.hud(format!("Couldn't keep it in history: {e}"));
                None
            }
        };
        self.forget_dropped_thumbs();
        if self.settings.sound && crate::kit::sound::on() {
            mac::play_shutter();
        }
        // upload each screenshot (the history copy: a PNG that's there now)
        if let Some(id) = id {
            let path = self.store.path(id);
            let (s, account) = self.share_for(&path);
            if s.auto && s.set_up(account) {
                self.share(path);
            }
        }
        let (copied, saved) = (self.settings.copy, self.settings.save);
        if copied || saved {
            let settings = self.settings.clone();
            // JPG copies need the picture; PNG copies are the bytes as they are
            let jpg = (saved && settings.format == settings::Format::Jpg).then(|| img.clone());
            let t = chrono::Local::now();
            self.in_background(move || {
                if copied && !mac::copy_png(&png) {
                    // rare; the save below still reports
                    eprintln!("capture: couldn't copy to the clipboard");
                }
                saved.then(|| Done::Saved(id, store::save_copy_png(&png, jpg.as_ref(), &settings, t), false))
            });
        }
        let note = match (copied, saved) {
            (true, true) => "Copied and saved",
            (true, false) => "Copied",
            (false, true) => "Saved",
            _ => "Captured",
        };
        match self.settings.after {
            settings::After::Preview => self.preview = Some(windows::Preview::new(id, img, preview, note.into())),
            settings::After::Editor => self.open_editor(id, img),
            settings::After::Nothing => {
                if copied || saved {
                    self.hud(note);
                }
            }
        }
    }

    fn put_thumb(&mut self, id: u64, img: &RgbaImage) {
        let t = self.ctx.load_texture(format!("capture-thumb-{id}"), thumb_image(img), egui::TextureOptions::LINEAR);
        self.thumbs.tex.insert(id, t);
    }

    fn forget_dropped_thumbs(&mut self) {
        let ids: HashSet<u64> = self.store.shots.iter().map(|s| s.id).chain(self.files.iter().map(|f| f.key)).collect();
        self.thumbs.tex.retain(|id, _| ids.contains(id));
    }

    /// The thumbnail of a history capture; starts loading it when missing.
    fn thumb(&mut self, id: u64) -> Option<egui::TextureId> {
        let path = self.store.path(id);
        self.thumb_at(id, path)
    }

    /// The thumbnail of the image at `path`, cached under `id`.
    fn thumb_at(&mut self, id: u64, path: PathBuf) -> Option<egui::TextureId> {
        if let Some(t) = self.thumbs.tex.get(&id) {
            return Some(t.id());
        }
        if self.thumbs.pending.len() < 3 && self.thumbs.pending.insert(id) {
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let img = image::open(&path).ok().map(|i| thumb_image(&i.to_rgba8()));
                let _ = tx.send(Done::Thumb(id, img));
                ctx.request_repaint();
            });
        }
        None
    }

    fn title_for(&self, id: Option<u64>) -> String {
        match id.and_then(|id| self.store.get(id)) {
            Some(s) => format!("Capture from {}", s.time().format("%-I:%M %P")),
            None => "Capture".into(),
        }
    }

    /// Keep an editor's picture in history as its own entry when it has
    /// changes; returns the id the editor now stands for.
    fn commit_edit(&mut self, ed: &mut Editor) -> Option<u64> {
        if ed.edited {
            let img = ed.rendered();
            self.commit_rendered(ed, &img)
        } else {
            ed.shot
        }
    }

    /// [`Capture::commit_edit`] with the editor's picture already rendered.
    fn commit_rendered(&mut self, ed: &mut Editor, img: &RgbaImage) -> Option<u64> {
        if ed.edited {
            match self.store.add(img, Kind::Edited, self.settings.keep) {
                Ok(id) => {
                    self.put_thumb(id, img);
                    self.forget_dropped_thumbs();
                    ed.shot = Some(id);
                    ed.edited = false;
                }
                Err(e) => ed.flash(format!("Couldn't keep the edit: {e}")),
            }
        }
        ed.shot
    }

    fn open_editor(&mut self, id: Option<u64>, img: RgbaImage) {
        // one editor at a time; the one open keeps its changes in history
        if let Some(mut old) = self.editor.take() {
            self.commit_edit(&mut old.ed);
            // the next editor starts with the frame chosen in this one
            if self.settings.frame != old.ed.frame_style {
                self.settings.frame = old.ed.frame_style;
                self.save_settings();
            }
        }
        let title = self.title_for(id);
        let s = &self.settings;
        let ed = Editor::new(id, title, img, self.ppp, s.blur, s.cover, s.keep_breaks, s.frame);
        mac::warm_ocr();
        self.editor = Some(windows::EditorWin::new(ed));
    }

    /// Open a video in the viewer (one at a time).
    fn open_video(&mut self, path: PathBuf) {
        if let Some(v) = &self.video {
            if v.path == path {
                return;
            }
        }
        self.video = Some(video::Viewer::open(&self.ctx, path));
    }

    fn open_editor_for(&mut self, id: u64) {
        match self.store.load(id) {
            Some(img) => self.open_editor(Some(id), img),
            None => self.hud("That capture is gone"),
        }
    }

    fn pin(&mut self, id: Option<u64>, img: RgbaImage) {
        if let Some(id) = id {
            self.store.set_pinned(id, true);
        }
        let n = self.next_pin;
        self.next_pin += 1;
        let offset = (self.pins.len() as f32) * 24.0;
        self.pins.push(windows::Pin::new(n, id, img, self.ppp, offset));
    }

    fn pin_shot(&mut self, id: u64) {
        match self.store.load(id) {
            Some(img) => self.pin(Some(id), img),
            None => self.hud("That capture is gone"),
        }
    }

    /// Save a copy to the user's folder on a thread; a toast says where (or
    /// why not) and the history entry remembers it.
    fn save_image_later(&self, id: Option<u64>, img: RgbaImage) {
        let settings = self.settings.clone();
        let t = chrono::Local::now();
        self.in_background(move || Some(Done::Saved(id, store::save_copy(&img, &settings, t), true)));
    }

    /// Save a copy to the user's folder now; remembers it on the history entry.
    fn save_image(&mut self, id: Option<u64>, img: &RgbaImage) -> Option<PathBuf> {
        match store::save_copy(img, &self.settings, chrono::Local::now()) {
            Ok(path) => {
                if let Some(id) = id {
                    self.store.set_saved(id, path.clone());
                }
                self.files_at = None;
                Some(path)
            }
            Err(e) => {
                self.hud(format!("Couldn't save: {e}"));
                None
            }
        }
    }

    fn shown_folder(&self) -> String {
        let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
        let f = self.settings.folder.display().to_string();
        // sample data for screenshots lives in a temp folder
        if f.contains("cmdmeow-capture-demo") {
            return "~/Pictures/Screenshots".into();
        }
        if !home.is_empty() && f.starts_with(&home) {
            f.replacen(&home, "~", 1)
        } else {
            f
        }
    }

    /// Re-read the screenshot folder when the list is a few seconds old.
    fn refresh_files(&mut self) {
        if self.files_busy || self.files_at.is_some_and(|t| t.elapsed().as_secs_f32() < 3.0) {
            return;
        }
        self.files_busy = true;
        let dir = self.settings.folder.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let files = folder::scan(&dir);
            let _ = tx.send(Done::Files(dir, files));
            ctx.request_repaint();
        });
    }

    fn open_file(&mut self, f: &folder::FileShot) {
        match image::open(&f.path) {
            Ok(img) => {
                self.open_editor(None, img.to_rgba8());
                if let Some(ew) = &mut self.editor {
                    ew.ed.title = f.name();
                }
            }
            Err(e) => self.hud(format!("Couldn't open {}: {e}", f.name())),
        }
    }

    fn choose_folder(&mut self) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("osascript").args(["-e", "POSIX path of (choose folder with prompt \"Save captures to\")"]).output();
            let path = out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).map(PathBuf::from);
            let _ = tx.send(Done::Folder(path));
            ctx.request_repaint();
        });
    }
}

/// Sample history for screenshots, in a throwaway folder.
fn demo_store() -> Store {
    let dir = std::env::temp_dir().join(format!("cmdmeow-capture-demo-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut st = Store::open(dir);
    let now = chrono::Local::now().timestamp_millis() as u64;
    let hour = 3_600_000u64;
    let items: [(u64, Kind, u32, u32); 8] = [
        (1, Kind::Area, 794, 446),
        (2, Kind::Window, 794, 500),
        (4, Kind::Area, 640, 410),
        (5, Kind::Screen, 900, 560),
        (7, Kind::Edited, 920, 300),
        (26, Kind::Window, 794, 520),
        (28, Kind::Area, 760, 520),
        (31, Kind::Screen, 900, 560),
    ];
    for (i, (h, kind, w, ht)) in items.iter().enumerate() {
        let id = now - h * hour - i as u64;
        let img = grab::sample(*w, *ht);
        if img.save(st.path(id)).is_ok() {
            st.shots.push(store::Shot { id, kind: *kind, w: *w, h: *ht, saved: None, pinned: i == 2, secs: None });
        }
    }
    st
}

/// A sample screenshot folder for screenshots of the tool.
fn demo_folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cmdmeow-capture-demo-{}-folder", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for (k, (w, h)) in [(794, 446), (640, 410), (920, 300), (794, 520), (760, 520), (900, 560)].iter().enumerate() {
        let name = format!("Capture 2026-11-08 at 1{}.{:02}.12.png", 7 - k / 3, 41 - k * 6);
        let _ = grab::sample(*w, *h).save(dir.join(name));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    dir
}

fn badge() -> Badge {
    Badge { icon: Icon::Crop, color: tint::CAPTURE() }
}

impl Tool for Capture {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Capture"
    }
    fn badge(&self) -> Badge {
        badge()
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::Screen), Need::may(Perm::Microphone), Need::may(Perm::Camera)]
    }

    fn setup_shortcuts(&self) -> Vec<(&'static str, String)> {
        [(Command::Area, "Capture area"), (Command::Record, "Record screen")].into_iter().filter_map(|(c, l)| self.settings.shortcut(c).map(|s| (l, s.display()))).collect()
    }

    fn section(&self) -> &'static str {
        "Create"
    }
    fn about(&self) -> &'static str {
        "Screenshots, markup and text from the screen"
    }
    fn set_on(&mut self, on: bool) {
        if on {
            // links that came while off aren't run late
            if let Some(l) = &self.links {
                l.take();
            }
            self.apply_keys();
        } else {
            if let Some(k) = &mut self.keys {
                k.clear();
            }
            self.key_errors.clear();
            self.recording = None;
            self.queue.clear();
            self.countdown = None;
            self.preview = None;
            self.pins.clear();
            self.overlay = None;
            self.perm_card = None;
            self.video = None;
            if self.rec.live().is_some() {
                self.rec.stop();
            } else if !matches!(self.rec.phase, record::Phase::Saving) {
                self.rec.cancel();
            }
            if let Some(mut ew) = self.editor.take() {
                self.commit_edit(&mut ew.ed);
            }
        }
    }
    fn sidebar_meta(&self) -> String {
        self.settings.shortcut(Command::Area).map(|s| s.display()).unwrap_or_default()
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: "Capture", about: "Take a screenshot; recent captures at the bigger sizes.", sizes: UP_TO_MEDIUM, size: Size::S11, default_on: true }]
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
            "folder" => self.tab = Tab::Folder,
            "settings" => self.tab = Tab::Settings,
            _ => {}
        }
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        if let Some(ed) = &mut self.demo_editor {
            // screenshots only: CMB_DEMO_EDITOR_W draws the editor at that width
            let w = std::env::var("CMB_DEMO_EDITOR_W").ok().and_then(|v| v.parse::<f32>().ok());
            let r = w.map(|w| Rect::from_min_size(rect.min, egui::Vec2::new(w.min(rect.width()), rect.height()))).unwrap_or(rect);
            ed.ui(ui, r, false);
            return;
        }
        ui::page(self, ui, rect, cx);
    }
    fn recommendations(&self) -> Vec<Recommendation> {
        if !self.perm_needed || mac::can_record_screen() {
            return Vec::new();
        }
        vec![Recommendation {
            priority: 40,
            title: "Capture needs Screen Recording".into(),
            body: "Without it, captures show only the desktop. Turn it on in System Settings.".into(),
            action: "Settings".into(),
            go: Action::OpenPage { tool: ID.into(), route: "settings".into() },
        }]
    }
    fn tray_title(&self) -> Option<String> {
        self.rec.tray()
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        if let Some(k) = &self.keys {
            if self.recording.is_none() {
                for c in k.poll() {
                    self.queue.push((c, 0));
                }
            }
        }
        if let Some(l) = &self.links {
            for c in l.take() {
                self.queue.push((c, 0));
            }
        }
        while let Ok(d) = self.rx.try_recv() {
            self.done(d);
        }
        if let Some(c) = &self.countdown {
            if Instant::now() >= c.until {
                let (g, p) = (c.grab, c.purpose);
                self.countdown = None;
                self.spawn_grab(g, p, 220);
            }
        }
        self.rec.tick(&mut self.settings.rec);
        self.recorder_out();
        self.uploads_done();
        if let Some(sv) = &mut self.rec.saved {
            // the card waits for its upload
            if self.uploads.progress(&sv.path).is_some() {
                sv.born = Instant::now();
            }
        }
        if self.rec.live().is_some() {
            // the timer in the menu bar
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let timed = self.countdown.is_some() || self.hud.is_some() || self.preview.is_some() || self.rec.busy() || self.uploads.busy();
        if timed {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.queue.is_empty() || !self.actions.is_empty() {
            ctx.request_repaint();
        }
        let windows = !self.pins.is_empty() || self.editor.is_some() || self.video.is_some() || self.overlay.is_some() || self.perm_card.is_some() || self.rec.busy();
        timed || windows || !self.queue.is_empty() || !self.actions.is_empty()
    }
    fn windows(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        if let Some(p) = ctx.input(|i| i.viewport().native_pixels_per_point) {
            self.ppp = p;
        }
        for a in self.actions.drain(..) {
            cx.push(a);
        }
        for (c, d) in std::mem::take(&mut self.queue) {
            self.run(c, d, cx);
        }
        windows::show(self, ctx, cx);
        record::ui::show(self, ctx, cx);
        video::show(self, ctx);
        self.recorder_out();
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        match view {
            "capture" => {
                self.tab = Tab::History;
                Some(View::Page)
            }
            "capture-empty" => {
                self.tab = Tab::History;
                self.store.shots.retain(|s| s.kind == store::Kind::Recording);
                Some(View::Page)
            }
            "capture-folder" => {
                self.tab = Tab::Folder;
                Some(View::Page)
            }
            "capture-settings" => {
                self.tab = Tab::Settings;
                Some(View::Page)
            }
            "capture-editor" => {
                let mut ed = Editor::new(None, "Capture from 5:41 pm".into(), grab::sample(1588, 892), 2.0, self.settings.blur, self.settings.cover, true, self.settings.frame);
                ed.demo_marks();
                // screenshots only: CMB_DEMO_EDITOR_TOOL picks the tool shown
                ed.tool = match std::env::var("CMB_DEMO_EDITOR_TOOL").unwrap_or_default().as_str() {
                    "arrow" => editor::Tool::Arrow,
                    "blur" => editor::Tool::Blur,
                    _ => ed.tool,
                };
                match std::env::var("CMB_DEMO_EDITOR_TOOL").unwrap_or_default().as_str() {
                    "area" | "crop" => ed.demo_area(),
                    "selected" => ed.demo_selected(),
                    _ => {}
                }
                self.demo_editor = Some(ed);
                Some(View::Page)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Capture {
        Capture::new(&ToolEnv { ctx: egui::Context::default(), demo: true })
    }

    #[test]
    fn setup_lists_the_transcript_model_for_first_use() {
        let m = demo().transcript_models();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].state, crate::core::tool::ModelState::OnFirstUse);
        assert_eq!(m[0].name, "Whisper turbo");
    }

    #[test]
    fn thumbnails_are_small() {
        let t = thumb_image(&grab::sample(1600, 900));
        assert_eq!(t.size, [THUMB_PX as usize, 202]);
    }

    #[test]
    fn closing_an_edited_capture_keeps_it_as_a_new_entry() {
        let mut c = demo();
        let before = c.store.shots.len();
        let first = c.store.shots[0].id;
        let mut ed = Editor::new(Some(first), "t".into(), grab::sample(200, 100), 2.0, c.settings.blur, c.settings.cover, true, c.settings.frame);
        assert_eq!(c.commit_edit(&mut ed), Some(first), "no changes, nothing new");
        ed.edited = true;
        let id = c.commit_edit(&mut ed).expect("kept");
        assert_ne!(id, first);
        assert_eq!(c.store.shots.len(), before + 1);
        assert_eq!(c.store.shots[0].kind, Kind::Edited);
        assert!(!ed.edited);
        assert!(c.store.get(first).is_some(), "the original stays");
    }

    #[test]
    fn panel_buttons_queue_commands_for_the_next_frame() {
        let mut c = demo();
        c.queue.push((Command::History, 0));
        assert!(c.tick(&c.ctx.clone()), "queued work asks for a frame even while hidden");
        let mut cx = Cx::new(None);
        c.windows(&c.ctx.clone(), &mut cx);
        assert!(c.queue.is_empty());
        assert_eq!(cx.take_actions(), vec![Action::OpenPage { tool: ID.into(), route: "history".into() }]);
    }
}
