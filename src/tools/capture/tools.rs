//! Capture shows as three tools in the Create section: Screenshot, Video
//! and Sharing (the storage both upload to).
//! Both are views of one [`Capture`]: one engine, one settings file, one
//! set of windows. Whichever is on drives the shared ticks and windows
//! (Screenshot when it's on), so nothing runs twice.

use super::{settings::Command, ui, Capture, VTab, CARD, ID, SHARING_ID, VIDEO_CARD, VIDEO_ID};
use crate::core::tool::{Badge, Cx, Recommendation, Size, Tool, ToolEnv, ToolId, View, WidgetSpec, UP_TO_MEDIUM};
use crate::kit::glass::{tint, Icon};
use egui::{Rect, Ui};
use std::cell::RefCell;
use std::rc::Rc;

pub struct Screenshot(Rc<RefCell<Capture>>);
pub struct Video(Rc<RefCell<Capture>>);
pub struct Sharing(Rc<RefCell<Capture>>);

/// The three tools over one shared Capture.
pub fn tools(env: &ToolEnv) -> (Screenshot, Video, Sharing) {
    let c = Rc::new(RefCell::new(Capture::new(env)));
    (Screenshot(c.clone()), Video(c.clone()), Sharing(c))
}


/// The setup step's shortcuts of `cmds`: each with its keys, on or off.
fn keys_of(c: &Capture, cmds: &[Command]) -> Vec<crate::core::tool::SetupKey> {
    cmds.iter()
        .filter_map(|&cmd| {
            let cur = c.settings.shortcut(cmd).cloned();
            let keys = cur.clone().or_else(|| cmd.default_shortcut())?.display();
            Some(crate::core::tool::SetupKey { id: cmd.slug(), label: cmd.label(), keys, on: cur.is_some() })
        })
        .collect()
}

/// Turn one of [`keys_of`] on (its default keys) or off.
fn set_key(c: &mut Capture, id: &str, on: bool) {
    let Some(cmd) = Command::from_slug(id) else { return };
    if on == c.settings.shortcut(cmd).is_some() {
        return;
    }
    c.settings.set_shortcut(cmd, if on { cmd.default_shortcut() } else { None });
    c.save_settings();
    c.apply_keys();
}

/// Give one of [`keys_of`] other keys (None: off).
fn set_key_to(c: &mut Capture, id: &str, keys: Option<crate::kit::hotkeys::Shortcut>) {
    let Some(cmd) = Command::from_slug(id) else { return };
    c.recording = None;
    c.settings.set_shortcut(cmd, keys);
    c.save_settings();
    c.apply_keys();
}

/// Typing a new shortcut for `id`: no shortcut fires meanwhile.
fn record_key(c: &mut Capture, id: &str, on: bool) {
    let cmd = Command::from_slug(id);
    if on && c.recording != cmd {
        c.recording = cmd;
        if let Some(k) = &mut c.keys {
            k.clear();
        }
    } else if !on && c.recording.is_some() {
        c.recording = None;
        c.apply_keys();
    }
}

fn key_icon(id: &str, p: &egui::Painter, r: Rect, col: egui::Color32) -> bool {
    match Command::from_slug(id) {
        Some(cmd) => {
            super::icons::draw(p, ui::command_icon(cmd), r, col);
            true
        }
        None => false,
    }
}

/// Screenshot's shortcuts on its setup step (Delayed and History have no default).
const SHOT_SETUP: [Command; 6] = [Command::Area, Command::Window, Command::Screen, Command::Text, Command::Color, Command::Ruler];

impl Tool for Sharing {
    fn id(&self) -> ToolId {
        SHARING_ID
    }
    fn name(&self) -> &'static str {
        ts!("cap.sharing")
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::Link, color: tint::CAPTURE() }
    }
    fn section(&self) -> &'static str {
        "Create"
    }
    fn about(&self) -> &'static str {
        ts!("cap.sharing_about")
    }
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::sharing_page(&mut self.0.borrow_mut(), ui, rect, cx);
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        (view == "sharing").then_some(View::Page)
    }
}

fn key(c: &Capture, cmd: Command) -> String {
    c.settings.shortcut(cmd).map(|s| s.display()).unwrap_or_default()
}

impl Tool for Screenshot {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        ts!("cap.screenshot")
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::Crop, color: tint::CAPTURE() }
    }
    fn section(&self) -> &'static str {
        "Create"
    }
    fn about(&self) -> &'static str {
        ts!("cap.shot_about")
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::Screen)]
    }
    fn setup_shortcuts(&self) -> Vec<(&'static str, String)> {
        let c = self.0.borrow();
        c.settings.shortcut(Command::Area).map(|s| vec![(ts!("cap.sc.area"), s.display())]).unwrap_or_default()
    }
    fn setup_keys(&self) -> Vec<crate::core::tool::SetupKey> {
        keys_of(&self.0.borrow(), &SHOT_SETUP)
    }
    fn set_setup_key(&mut self, id: &str, on: bool) {
        set_key(&mut self.0.borrow_mut(), id, on);
    }
    fn set_setup_key_to(&mut self, id: &str, keys: Option<crate::kit::hotkeys::Shortcut>) {
        set_key_to(&mut self.0.borrow_mut(), id, keys);
    }
    fn record_setup_key(&mut self, id: &str, on: bool) {
        record_key(&mut self.0.borrow_mut(), id, on);
    }
    fn setup_key_icon(&self, id: &str, p: &egui::Painter, r: Rect, col: egui::Color32) -> bool {
        key_icon(id, p, r, col)
    }
    fn sidebar_meta(&self) -> String {
        key(&self.0.borrow(), Command::Area)
    }
    fn set_on(&mut self, on: bool) {
        self.0.borrow_mut().set_parts(Some(on), None);
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: CARD, title: ts!("cap.screenshot"), about: ts!("cap.w_about"), sizes: UP_TO_MEDIUM, size: Size::S11, default_on: true }]
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::widget(&mut self.0.borrow_mut(), size, ui, rect, cx);
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        Tool::open_page(&mut *self.0.borrow_mut(), route);
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        Tool::page_ui(&mut *self.0.borrow_mut(), ui, rect, cx);
    }
    fn recommendations(&self) -> Vec<Recommendation> {
        Tool::recommendations(&*self.0.borrow())
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        let mut c = self.0.borrow_mut();
        c.shot_on && Tool::tick(&mut *c, ctx)
    }
    fn windows(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        let mut c = self.0.borrow_mut();
        if c.shot_on {
            Tool::windows(&mut *c, ctx, cx);
        }
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        Tool::demo_view(&mut *self.0.borrow_mut(), view)
    }
}

impl Tool for Video {
    fn id(&self) -> ToolId {
        VIDEO_ID
    }
    fn name(&self) -> &'static str {
        ts!("cap.video")
    }
    /// Whisper, for transcripts and captions of recordings.
    fn setup_models(&self) -> Vec<crate::core::tool::SetupModel> {
        self.0.borrow().transcript_models()
    }
    fn badge(&self) -> Badge {
        Badge { icon: Icon::Camera, color: tint::CAPTURE() }
    }
    fn section(&self) -> &'static str {
        "Create"
    }
    fn about(&self) -> &'static str {
        ts!("cap.video_about")
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::Screen), Need::may(Perm::Microphone), Need::may(Perm::Camera)]
    }
    fn setup_shortcuts(&self) -> Vec<(&'static str, String)> {
        let c = self.0.borrow();
        c.settings.shortcut(Command::Record).map(|s| vec![(ts!("cap.sc.record"), s.display())]).unwrap_or_default()
    }
    fn setup_keys(&self) -> Vec<crate::core::tool::SetupKey> {
        keys_of(&self.0.borrow(), &[Command::Record])
    }
    fn set_setup_key(&mut self, id: &str, on: bool) {
        set_key(&mut self.0.borrow_mut(), id, on);
    }
    fn set_setup_key_to(&mut self, id: &str, keys: Option<crate::kit::hotkeys::Shortcut>) {
        set_key_to(&mut self.0.borrow_mut(), id, keys);
    }
    fn record_setup_key(&mut self, id: &str, on: bool) {
        record_key(&mut self.0.borrow_mut(), id, on);
    }
    fn setup_key_icon(&self, id: &str, p: &egui::Painter, r: Rect, col: egui::Color32) -> bool {
        key_icon(id, p, r, col)
    }
    fn sidebar_meta(&self) -> String {
        let c = self.0.borrow();
        c.rec.tray().map(|t| t.trim_start_matches(['●', '❚', ' ']).to_string()).unwrap_or_else(|| key(&c, Command::Record))
    }
    fn set_on(&mut self, on: bool) {
        self.0.borrow_mut().set_parts(None, Some(on));
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        vec![WidgetSpec { id: VIDEO_CARD, title: ts!("cap.video"), about: ts!("cap.vw_about"), sizes: UP_TO_MEDIUM, size: Size::S11, default_on: true }]
    }
    fn widget_ui(&mut self, _widget: &str, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::video_widget(&mut self.0.borrow_mut(), size, ui, rect, cx);
    }
    fn has_page(&self) -> bool {
        true
    }
    fn open_page(&mut self, route: &str) {
        let mut c = self.0.borrow_mut();
        c.vtab = if route == "settings" { VTab::Settings } else { VTab::Recordings };
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        ui::video_page(&mut self.0.borrow_mut(), ui, rect, cx);
    }
    fn tray_title(&self) -> Option<String> {
        self.0.borrow().rec.tray()
    }
    /// A click on the recording clock stops and saves, as the bar's stop does.
    fn tray_clicked(&mut self) -> bool {
        self.0.borrow_mut().rec.stop_from_menu_bar()
    }
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        let mut c = self.0.borrow_mut();
        !c.shot_on && c.video_on && Tool::tick(&mut *c, ctx)
    }
    fn windows(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        let mut c = self.0.borrow_mut();
        if !c.shot_on && c.video_on {
            Tool::windows(&mut *c, ctx, cx);
        }
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        let mut c = self.0.borrow_mut();
        match view {
            "video" => {
                c.vtab = VTab::Recordings;
                Some(View::Page)
            }
            "video-settings" => {
                c.vtab = VTab::Settings;
                Some(View::Page)
            }
            "video-empty" => {
                c.vtab = VTab::Recordings;
                c.store.shots.retain(|s| s.kind != super::store::Kind::Recording);
                Some(View::Page)
            }
            _ => None,
        }
    }
}
