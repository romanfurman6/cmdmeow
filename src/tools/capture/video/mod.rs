//! The video viewer: play a recording, trim its ends, cut pieces out of the
//! middle, and save (over the file, or as a copy). Opened from the saved
//! card, History and the Folder.
//!
//! The cuts live in an [`edit::Edl`] until Save; playback skips them, and
//! the file is only written by [`media::export`] (no re-encoding).
//!
//! The side panel transcribes the voice on this Mac ([`speech`], whisper,
//! the model downloaded once on request) and cleans it up ([`voice`],
//! DeepFilterNet3, built in); the cleaned sound goes into the file on Save.
//! It also puts the video in a frame ([`scene`]): drawn live while it's
//! chosen, rendered on the GPU on Save ([`compose`]).

pub mod edit;
pub mod media;
pub mod compose;
pub mod scene;
pub mod sound;
pub mod speech;
pub mod voice;
mod ui;

use edit::Edl;
use media::Player;
use serde::{Deserialize, Serialize};
use speech::{Line, Model, Progress};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

pub use ui::show;

pub const SPEEDS: [f32; 3] = [1.0, 1.5, 2.0];
/// Frames in the timeline strip.
const STRIP: usize = 24;

/// Buckets in the voice waveform.
const PEAKS: usize = 1200;

/// What the viewer needs Capture to do.
pub enum Out {
    /// A file was written: over the original (`copy` false) or beside it.
    Saved { path: PathBuf, secs: f64, copy: bool },
    CopyText(String),
    /// Upload the file for a link (Share).
    Share(PathBuf),
}

enum Event {
    Strip(Vec<image::RgbaImage>),
    Saved(Result<Written, String>),
    Peaks(usize, Vec<f32>),
    Downloaded(Model, Result<(), String>),
    Transcribed(Result<Vec<Line>, String>),
    Cleaned(Result<(PathBuf, Vec<f32>), String>),
}

/// A file Save wrote.
struct Written {
    /// Beside the video (it goes over it once it's done), or the copy.
    path: PathBuf,
    copy: bool,
    /// The cut unframed picture a framed save made on the way.
    cut: Option<PathBuf>,
    /// The frame the file has.
    style: scene::Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Transcript,
    Audio,
    /// The frame.
    Look,
}

/// Work running on a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Download(Model),
    Transcribe,
    Clean,
}

pub struct Job {
    pub kind: JobKind,
    pub progress: Arc<Progress>,
}

/// The viewer's choices, kept between videos.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub model: Model,
    pub captions: bool,
    pub strength: f32,
    /// The last frame chosen: a new frame starts from it.
    pub frame: crate::tools::capture::render::Frame,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { model: Model::Accurate, captions: true, strength: 0.7, frame: Default::default() }
    }
}

fn prefs_path() -> PathBuf {
    crate::kit::fs::data_path("video-viewer.json")
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Moving the start or end of what's kept.
    Trim { end: bool },
    /// Choosing a stretch, from where it began (source seconds).
    Select(f64),
    Scrub,
}

pub struct Viewer {
    pub path: PathBuf,
    pub edl: Edl,
    undo: Vec<Edl>,
    redo: Vec<Edl>,
    player: Option<Player>,
    error: Option<String>,
    tex: Option<egui::TextureHandle>,
    /// The picture's size in pixels (for fitting it).
    size: (u32, u32),
    strip: Vec<egui::TextureHandle>,
    strip_imgs: Vec<image::RgbaImage>,
    /// A chosen stretch, source seconds, a < b.
    pub sel: Option<(f64, f64)>,
    speed: usize,
    drag: Option<Drag>,
    saving: bool,
    status: Option<(String, Instant)>,
    /// Ask before closing with unsaved cuts.
    asking: bool,
    /// Repaint for a moment after a seek, so its frame shows up.
    kick: Instant,
    pub opened: bool,
    pub out: Vec<Out>,
    pub tab: Tab,
    pub prefs: Prefs,
    /// The transcript (source seconds), when there is one.
    pub lines: Vec<Line>,
    /// Lines chosen in the transcript (first, last).
    pub picked: Option<(usize, usize)>,
    pub job: Option<Job>,
    /// The last job's failure, shown on the tab the job belongs to.
    pub job_error: Option<(Tab, String)>,
    /// Audio tracks in the file (3: mix, system, mic).
    pub tracks: usize,
    /// The voice's loudness, and the cleaned voice's.
    pub peaks: Vec<f32>,
    pub clean_peaks: Vec<f32>,
    /// The cleaned sound, and whether it's what plays (and gets saved).
    pub voice: Option<PathBuf>,
    pub voice_on: bool,
    /// A framed video's unframed picture and the frame it has: what plays
    /// here, so the frame can be changed or taken off.
    pub project: Option<(compose::Project, compose::Meta)>,
    /// The frame being chosen (Save renders it).
    pub look: scene::Style,
    /// The framed save running.
    pub rendering: Option<Arc<compose::Progress>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: egui::Context,
}

impl Viewer {
    pub fn open(ctx: &egui::Context, path: PathBuf) -> Viewer {
        let (tx, rx) = channel();
        let mut v = Viewer {
            path,
            edl: Edl::new(0.0),
            undo: Vec::new(),
            redo: Vec::new(),
            player: None,
            error: None,
            tex: None,
            size: (16, 9),
            strip: Vec::new(),
            strip_imgs: Vec::new(),
            sel: None,
            speed: 0,
            drag: None,
            saving: false,
            status: None,
            asking: false,
            kick: Instant::now(),
            opened: false,
            out: Vec::new(),
            tab: Tab::Transcript,
            prefs: crate::kit::fs::load_json(&prefs_path()),
            lines: Vec::new(),
            picked: None,
            job: None,
            job_error: None,
            tracks: 0,
            peaks: Vec::new(),
            clean_peaks: Vec::new(),
            voice: None,
            voice_on: false,
            project: None,
            look: scene::Style::default(),
            rendering: None,
            tx,
            rx,
            ctx: ctx.clone(),
        };
        v.load();
        v
    }

    /// (Re)open the file: the player, a fresh edit, the strip.
    fn load(&mut self) {
        self.player = None;
        self.tex = None;
        self.strip.clear();
        self.strip_imgs.clear();
        self.undo.clear();
        self.redo.clear();
        self.sel = None;
        self.picked = None;
        self.voice = None;
        self.voice_on = false;
        self.clean_peaks.clear();
        self.peaks.clear();
        self.lines = speech::load(&self.path).unwrap_or_default();
        self.project = compose::Project::find(&projects_dir(), &self.path);
        self.look = self.saved_style();
        match Player::open(&self.source(), None) {
            Ok(p) => {
                self.edl = Edl::new(p.duration);
                self.size = p.size;
                p.seek(0.0);
                self.player = Some(p);
                self.error = None;
            }
            Err(e) => {
                self.edl = Edl::new(0.0);
                self.error = Some(e);
            }
        }
        self.kick = Instant::now();
        let (tx, ctx, path, src) = (self.tx.clone(), self.ctx.clone(), self.path.clone(), self.source());
        std::thread::spawn(move || {
            let imgs = media::strip(&src, STRIP, 112.0);
            let _ = tx.send(Event::Strip(imgs));
            ctx.request_repaint();
            let tracks = sound::audio_tracks(&path);
            let _ = tx.send(Event::Peaks(tracks, voice::voice_peaks(&path, PEAKS)));
            ctx.request_repaint();
        });
    }

    /// Open the player again (the cleaned sound on or off), where it was.
    fn reopen(&mut self) {
        let t = self.time();
        let voice = self.voice.clone().filter(|_| self.voice_on);
        self.player = None;
        match Player::open(&self.source(), voice.as_deref()) {
            Ok(p) => {
                p.seek(t);
                self.player = Some(p);
            }
            Err(e) => self.error = Some(e),
        }
        self.kick = Instant::now();
    }

    pub fn save_prefs(&self) {
        crate::kit::fs::save_json(&prefs_path(), &self.prefs, true);
    }

    /// The unframed picture: what plays and what a frame goes around.
    pub fn source(&self) -> PathBuf {
        self.project.as_ref().map(|p| p.0.raw()).unwrap_or_else(|| self.path.clone())
    }

    /// The frame the file has now.
    pub fn saved_style(&self) -> scene::Style {
        self.project.as_ref().map(|p| p.1.style).unwrap_or_default()
    }

    /// Choose a frame (None takes it off); a new one starts from the last.
    pub fn set_frame(&mut self, f: Option<crate::tools::capture::render::Frame>) {
        self.look.frame = f;
        if let Some(f) = f {
            if f != self.prefs.frame {
                self.prefs.frame = f;
                self.save_prefs();
            }
        }
    }

    // ------------------------------------------------------------ jobs

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn spawn(&mut self, kind: JobKind, work: impl FnOnce(&Arc<Progress>) -> Event + Send + 'static) {
        if self.job.is_some() {
            return;
        }
        let progress = Arc::new(Progress::default());
        self.job = Some(Job { kind, progress: progress.clone() });
        self.job_error = None;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let e = work(&progress);
            let _ = tx.send(e);
            ctx.request_repaint();
        });
    }

    pub fn cancel_job(&mut self) {
        if let Some(j) = &self.job {
            j.progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Fetch the chosen speech model.
    pub fn download(&mut self, m: Model) {
        self.spawn(JobKind::Download(m), move |p| Event::Downloaded(m, speech::download(m, p)));
    }

    /// Turn the voice into timed lines with the chosen model.
    pub fn transcribe(&mut self) {
        let m = self.prefs.model;
        if !m.ready() {
            self.download(m);
            return;
        }
        let path = self.path.clone();
        self.spawn(JobKind::Transcribe, move |p| Event::Transcribed(speech::transcribe(&path, m, p)));
    }

    /// Take the noise out of the voice (at the chosen strength).
    pub fn clean_voice(&mut self) {
        let (path, strength) = (self.path.clone(), self.prefs.strength);
        self.spawn(JobKind::Clean, move |p| Event::Cleaned(voice::clean(&path, strength, p).map(|f| {
            let peaks = voice::cleaned_peaks(&f, PEAKS);
            (f, peaks)
        })));
    }

    /// Hear (and save) the cleaned voice, or the original.
    pub fn set_voice_on(&mut self, on: bool) {
        if self.voice.is_some() && on != self.voice_on {
            self.voice_on = on;
            self.reopen();
        }
    }

    // ------------------------------------------------------------ transcript

    /// Pick line `i`; with `extend`, from the line picked before to it.
    pub fn pick_line(&mut self, i: usize, extend: bool) {
        if i >= self.lines.len() {
            return;
        }
        let (a, b) = match (self.picked, extend) {
            (Some((a, _)), true) => (a.min(i), a.max(i)),
            _ => (i, i),
        };
        self.picked = Some((a, b));
        self.sel = Some((self.lines[a].start, self.lines[b].end));
        self.seek(self.lines[a].start);
    }

    /// The lines as they'll be in the saved video.
    pub fn edited_lines(&self) -> Vec<Line> {
        speech::edited_lines(&self.lines, &self.edl)
    }

    pub fn copy_text(&mut self) {
        let t = speech::txt(&self.edited_lines());
        self.out.push(Out::CopyText(t));
        self.flash("Copied the text");
    }

    /// Write captions (.srt) or text (.txt) beside the video.
    pub fn export_text(&mut self, srt: bool) {
        let lines = self.edited_lines();
        let (body, ext) = if srt { (speech::srt(&lines), "srt") } else { (speech::txt(&lines), "txt") };
        let path = self.path.with_extension(ext);
        match std::fs::write(&path, body) {
            Ok(()) => self.flash(format!("Saved {}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
            Err(e) => self.flash(format!("Couldn't save: {e}")),
        }
    }

    /// The line to show over the picture now.
    pub fn caption(&self) -> Option<&str> {
        if !self.prefs.captions {
            return None;
        }
        let t = self.time();
        if !self.edl.kept(t) {
            return None;
        }
        speech::line_at(&self.lines, t).map(|i| self.lines[i].text.as_str())
    }

    pub fn title(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Video".into())
    }

    /// Cuts not saved yet.
    pub fn edited(&self) -> bool {
        !self.edl.untouched()
    }

    /// Anything Save would write: cuts, the cleaned voice or a new frame.
    pub fn dirty(&self) -> bool {
        self.edited() || (self.voice_on && self.voice.is_some()) || self.restyled()
    }

    /// The frame differs from the one in the file.
    pub fn restyled(&self) -> bool {
        self.saved_style() != self.look
    }

    pub fn duration(&self) -> f64 {
        self.edl.duration
    }

    pub fn time(&self) -> f64 {
        self.player.as_ref().map(|p| p.time()).unwrap_or(0.0)
    }

    pub fn playing(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.playing())
    }

    fn flash(&mut self, m: impl Into<String>) {
        self.status = Some((m.into(), Instant::now()));
    }

    // ------------------------------------------------------------ playback

    pub fn toggle_play(&mut self) {
        let Some(p) = &self.player else { return };
        if p.playing() {
            p.pause();
            return;
        }
        let t = p.time();
        let at_end = t >= self.edl.end() - 0.05;
        let from = if at_end { Some(self.edl.start()) } else { self.edl.playable(t) };
        if let Some(f) = from {
            if (f - t).abs() > 1e-3 {
                p.seek(f);
            }
        }
        p.play(SPEEDS[self.speed]);
    }

    pub fn seek(&mut self, t: f64) {
        if let Some(p) = &self.player {
            p.seek(t.clamp(0.0, self.edl.duration));
            self.kick = Instant::now();
        }
    }

    pub fn set_speed(&mut self, k: usize) {
        self.speed = k.min(SPEEDS.len() - 1);
        if let Some(p) = &self.player {
            if p.playing() {
                p.play(SPEEDS[self.speed]);
            }
        }
    }

    /// Step `d` seconds of the edited video.
    pub fn step(&mut self, d: f64) {
        let e = self.edl.to_edited(self.time()).unwrap_or_else(|| self.edl.to_edited(self.edl.playable(self.time()).unwrap_or(self.edl.end())).unwrap_or(0.0));
        let t = self.edl.to_source((e + d).clamp(0.0, self.edl.length()));
        self.seek(t);
    }

    // ------------------------------------------------------------ edits

    fn change(&mut self, f: impl FnOnce(&mut Edl) -> bool) -> bool {
        let before = self.edl.clone();
        if f(&mut self.edl) {
            self.undo.push(before);
            self.redo.clear();
            true
        } else {
            false
        }
    }

    pub fn cut_selection(&mut self) {
        let Some((a, b)) = self.sel else { return };
        if self.change(|e| e.cut(a, b)) {
            self.sel = None;
            if let Some(n) = self.edl.playable(a) {
                self.seek(n);
            }
        } else {
            self.flash("Can't cut all of it");
        }
    }

    /// Keep only the selection.
    pub fn keep_selection(&mut self) {
        let Some((a, b)) = self.sel else { return };
        if self.change(|e| {
            let x = e.trim_start(a);
            let y = e.trim_end(b);
            x || y
        }) {
            self.sel = None;
            self.seek(self.edl.start());
        }
    }

    pub fn undo(&mut self) {
        if let Some(e) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edl, e));
        }
    }

    pub fn redo(&mut self) {
        if let Some(e) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.edl, e));
        }
    }

    /// Start of a trim-handle drag: one undo step for the whole drag.
    fn begin_trim(&mut self) {
        self.undo.push(self.edl.clone());
        self.redo.clear();
    }

    fn trim_to(&mut self, end: bool, t: f64) {
        if end {
            let t = t.max(self.edl.start() + 0.2);
            self.edl.trim_end(t);
        } else {
            let t = t.min(self.edl.end() - 0.2);
            self.edl.trim_start(t);
        }
    }

    /// A drag that changed nothing leaves no undo step.
    fn end_trim(&mut self) {
        if self.undo.last() == Some(&self.edl) {
            self.undo.pop();
        }
    }

    // ------------------------------------------------------------ saving

    /// Write the edit: over the file, or as a copy beside it. Without a
    /// frame the cuts are made without re-encoding; with one, the cut
    /// unframed picture is then framed on the GPU.
    pub fn save(&mut self, copy: bool) {
        if self.saving || self.player.is_none() || self.busy() || (!copy && !self.dirty()) {
            return;
        }
        if let Some(p) = &self.player {
            p.pause();
        }
        self.saving = true;
        let dest = if copy { media::copy_name(&self.path) } else { media::temp_beside(&self.path) };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let voice = self.voice.clone().filter(|_| self.voice_on);
        let (src, style, edl) = (self.source(), self.look, self.edl.clone());
        let Some(frame) = style.frame else {
            media::export(
                &src,
                voice.as_deref(),
                &edl,
                dest,
                Box::new(move |r| {
                    let _ = tx.send(Event::Saved(r.map(|path| Written { path, copy, cut: None, style })));
                    ctx.request_repaint();
                }),
            );
            return;
        };
        let progress = Arc::new(compose::Progress::default());
        self.rendering = Some(progress.clone());
        let edited = !edl.untouched() || voice.is_some();
        let root = projects_dir();
        std::thread::spawn(move || {
            let r = (|| -> Result<Written, String> {
                let cut = match edited {
                    true => Some(export_now(&src, voice.as_deref(), &edl, compose::scratch(&root))?),
                    false => None,
                };
                let job = compose::Job { src: cut.clone().unwrap_or(src), out: dest.clone(), frame };
                match compose::render(&job, &progress) {
                    Ok(_) => Ok(Written { path: dest, copy, cut, style }),
                    Err(e) => {
                        if let Some(c) = cut {
                            let _ = std::fs::remove_file(c);
                        }
                        Err(e)
                    }
                }
            })();
            let _ = tx.send(Event::Saved(r));
            ctx.request_repaint();
        });
    }

    /// Stop a framed save (the window closes).
    pub fn cancel_save(&self) {
        if let Some(r) = &self.rendering {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Background results; call once per pass.
    pub fn tick(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Strip(imgs) => {
                    self.strip_imgs = imgs;
                    self.strip.clear();
                }
                Event::Saved(Ok(w)) => {
                    self.saving = false;
                    self.rendering = None;
                    let secs = self.edl.length();
                    let lines = self.edited_lines();
                    if w.copy {
                        if let Some(c) = &w.cut {
                            let _ = std::fs::remove_file(c);
                        }
                        if !lines.is_empty() {
                            speech::store(&w.path, &lines);
                        }
                        self.flash(format!("Saved a copy: {}", w.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                        self.out.push(Out::Saved { path: w.path, secs, copy: true });
                        continue;
                    }
                    // the player lets go of the files before they're replaced
                    self.player = None;
                    match compose::commit_over(&projects_dir(), &self.path, &w.path, w.cut.as_deref(), w.style) {
                        Ok(()) => {
                            if let Some(v) = self.voice.take() {
                                let _ = std::fs::remove_file(v);
                            }
                            if !lines.is_empty() {
                                speech::store(&self.path, &lines);
                            }
                            self.load();
                            self.flash("Saved");
                            self.out.push(Out::Saved { path: self.path.clone(), secs: self.edl.duration, copy: false });
                        }
                        Err(e) => {
                            self.load();
                            self.flash(format!("Couldn't save: {e}"));
                        }
                    }
                }
                Event::Saved(Err(e)) => {
                    self.saving = false;
                    self.rendering = None;
                    self.flash(format!("Couldn't save: {e}"));
                }
                Event::Peaks(n, p) => {
                    self.tracks = n;
                    self.peaks = p;
                }
                Event::Downloaded(m, r) => {
                    self.job = None;
                    match r {
                        // straight on to what the download was for
                        Ok(()) if m == self.prefs.model && self.lines.is_empty() => self.transcribe(),
                        Ok(()) => {}
                        Err(e) => self.job_error = Some((Tab::Transcript, e)),
                    }
                }
                Event::Transcribed(r) => {
                    self.job = None;
                    match r {
                        Ok(lines) => {
                            speech::store(&self.path, &lines);
                            if lines.is_empty() {
                                self.job_error = Some((Tab::Transcript, "No speech found".into()));
                            }
                            self.lines = lines;
                            self.picked = None;
                        }
                        Err(e) => self.job_error = Some((Tab::Transcript, e)),
                    }
                }
                Event::Cleaned(r) => {
                    self.job = None;
                    match r {
                        Ok((f, peaks)) => {
                            self.voice = Some(f);
                            self.clean_peaks = peaks;
                            self.voice_on = false;
                            self.set_voice_on(true);
                            self.flash("Voice cleaned up: Save puts it in the video");
                        }
                        Err(e) => self.job_error = Some((Tab::Audio, e)),
                    }
                }
            }
        }
        // playback jumps over cuts and stops at the end
        if let Some(p) = &self.player {
            if p.playing() {
                let t = p.time();
                if t >= self.edl.end() - 1e-3 {
                    p.pause();
                    p.seek(self.edl.end());
                } else if !self.edl.kept(t) {
                    match self.edl.playable(t) {
                        Some(n) => p.seek(n),
                        None => p.pause(),
                    }
                }
            }
        }
    }
}

fn projects_dir() -> PathBuf {
    crate::tools::capture::record::projects_dir()
}

/// [`media::export`], waited for (off the UI thread).
fn export_now(src: &Path, voice: Option<&Path>, edl: &Edl, dest: PathBuf) -> Result<PathBuf, String> {
    let (tx, rx) = channel();
    media::export(src, voice, edl, dest, Box::new(move |r| drop(tx.send(r))));
    rx.recv().map_err(|_| "The cut video didn't come back".to_string())?
}

/// `--video-probe`: the viewer's work on `path` without UI, printed to
/// stderr. Writes only temp files and "<name> edited.mp4" beside it.
pub fn probe(path: &std::path::Path, download: bool) {
    let t = Instant::now();
    let n = sound::audio_tracks(path);
    eprintln!("[video] {} audio tracks: {n}", path.display());
    for (k, name) in ["mix", "system", "mic"].iter().enumerate().take(n) {
        if let Ok(s) = sound::read_nth(path, k, 8_000, 1) {
            eprintln!("[video] track {k} ({name}): peak {:.3}", s.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        }
    }
    match sound::read_audio(path, sound::voice_track(n), 16_000, 1) {
        Ok(s) => eprintln!("[video] voice at 16 kHz: {} samples ({:.2} s), peak {:.3} ({:?})", s.len(), s.len() as f64 / 16_000.0, s.iter().fold(0.0f32, |m, v| m.max(v.abs())), t.elapsed()),
        Err(e) => eprintln!("[video] reading the voice failed: {e}"),
    }
    let p = Progress::default();
    let t = Instant::now();
    let cleaned = match voice::clean(path, 0.7, &p) {
        Ok(f) => {
            let peaks = voice::cleaned_peaks(&f, 50);
            eprintln!("[video] cleaned voice: {} ({} bytes, {} audio tracks, peak {:.3}) in {:?}", f.display(), std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0), sound::audio_tracks(&f), peaks.iter().fold(0.0f32, |a, b| a.max(*b)), t.elapsed());
            Some(f)
        }
        Err(e) => {
            eprintln!("[video] voice cleanup failed: {e}");
            None
        }
    };
    let d = media::duration(path);
    eprintln!("[video] length {d:.2} s");
    let mut edl = Edl::new(d);
    edl.cut(d * 0.25, d * 0.5);
    let dest = media::copy_name(path);
    let (tx, rx) = channel();
    let t = Instant::now();
    media::export(path, cleaned.as_deref(), &edl, dest, Box::new(move |r| drop(tx.send(r))));
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(Ok(f)) => eprintln!("[video] saved a copy with a quarter cut and the cleaned voice: {} ({} audio tracks) in {:?}", f.display(), sound::audio_tracks(&f), t.elapsed()),
        Ok(Err(e)) => eprintln!("[video] saving failed: {e}"),
        Err(_) => eprintln!("[video] saving timed out"),
    }
    probe_frame(path, d);
    let m = Model::Fast;
    if !m.ready() && download {
        let p = Progress::default();
        eprintln!("[video] downloading {}…", m.file());
        if let Err(e) = speech::download(m, &p) {
            eprintln!("[video] download failed: {e}");
        }
    }
    if let Some(m) = [Model::Accurate, Model::Fast].into_iter().find(|m| m.ready()) {
        let t = Instant::now();
        match speech::transcribe(path, m, &Arc::new(Progress::default())) {
            Ok(lines) => {
                eprintln!("[video] {} lines with {} in {:?}:", lines.len(), m.name(), t.elapsed());
                for l in lines.iter().take(10) {
                    eprintln!("[video]   {:6.2}–{:6.2} {}", l.start, l.end, l.text);
                }
            }
            Err(e) => eprintln!("[video] transcribing failed: {e}"),
        }
    } else {
        eprintln!("[video] no speech model (pass --download to fetch the fast one)");
    }
    if let Some(f) = cleaned {
        let _ = std::fs::remove_file(f);
    }
}

/// The probe's framed save: how fast the GPU frames `path`, and one frame
/// of it beside the same frame framed on the CPU (PNGs next to the video).
fn probe_frame(path: &Path, secs: f64) {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".into());
    let out = path.with_file_name(format!("{stem} framed.mp4"));
    let frame = crate::tools::capture::render::Frame::default();
    let t = Instant::now();
    match compose::render(&compose::Job { src: path.to_path_buf(), out: out.clone(), frame }, &compose::Progress::default()) {
        Ok(size) => eprintln!("[video] framed on the GPU: {} {size:?} in {:?} ({:.1}× real time)", out.display(), t.elapsed(), secs / t.elapsed().as_secs_f64().max(1e-3)),
        Err(e) => {
            eprintln!("[video] framing failed: {e}");
            return;
        }
    }
    let (Some(src), Some(gpu)) = (media::strip(path, 1, 720.0).pop(), media::strip(&out, 1, 720.0).pop()) else { return };
    let png = |name: &str, img: &image::RgbaImage| {
        let p = path.with_file_name(format!("{stem} {name}.png"));
        match img.save(&p) {
            Ok(()) => eprintln!("[video] wrote {}", p.display()),
            Err(e) => eprintln!("[video] {name}: {e}"),
        }
    };
    png("probe gpu", &gpu);
    let size = src.dimensions();
    let lay = scene::layout(size, Some(&frame));
    let bgra: Vec<u8> = src.pixels().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    if let Some(cpu) = scene::chrome(&frame, &lay).and_then(|ch| scene::compose_cpu(&ch, &lay, &bgra, size)) {
        let rgba: Vec<u8> = cpu.data().chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        if let Some(img) = image::RgbaImage::from_raw(cpu.width(), cpu.height(), rgba) {
            png("probe cpu", &img);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer(d: f64) -> Viewer {
        let mut v = Viewer::open(&egui::Context::default(), PathBuf::from("/nowhere.mp4"));
        v.edl = Edl::new(d);
        v
    }

    #[test]
    fn a_missing_file_says_so() {
        let v = Viewer::open(&egui::Context::default(), PathBuf::from("/nowhere.mp4"));
        assert!(v.error.is_some());
        assert!(!v.edited());
    }

    #[test]
    fn cuts_undo_and_redo() {
        let mut v = viewer(10.0);
        v.sel = Some((2.0, 4.0));
        v.cut_selection();
        assert_eq!(v.edl.keep, vec![(0.0, 2.0), (4.0, 10.0)]);
        assert!(v.sel.is_none());
        v.undo();
        assert!(!v.edited());
        v.redo();
        assert!(v.edited());
    }

    #[test]
    fn keeping_a_selection_trims_both_ends() {
        let mut v = viewer(10.0);
        v.sel = Some((3.0, 7.0));
        v.keep_selection();
        assert_eq!(v.edl.keep, vec![(3.0, 7.0)]);
        v.undo();
        assert!(!v.edited(), "one step");
    }

    #[test]
    fn a_trim_drag_is_one_undo_step_and_none_if_nothing_moved() {
        let mut v = viewer(10.0);
        v.begin_trim();
        v.end_trim();
        assert!(v.undo.is_empty());
        v.begin_trim();
        v.trim_to(false, 1.0);
        v.trim_to(false, 2.0);
        v.end_trim();
        assert_eq!(v.undo.len(), 1);
        assert_eq!(v.edl.start(), 2.0);
        v.trim_to(true, 1.0);
        assert!(v.edl.end() >= 2.2, "the end stays after the start");
    }
}
