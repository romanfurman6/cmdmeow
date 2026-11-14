//! The app shell: a menu-bar icon that drops down the widget panel (with a
//! detail sheet to its left), and a full window with a page per tool. It knows
//! tools only through the [`Tool`] trait.

use super::settings::{widget_key, Avail, Entry, WidgetPrefs};
use super::tool::{Action, Cx, Size, Tool, ToolEnv, View, WidgetSpec};
use crate::kit::glass::{self, *};
use crate::kit::mechanics as m;
use egui::{Color32, Pos2, Rect, Vec2, ViewportCommand};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

/// How often the app wakes while nothing is on screen.
const HIDDEN_HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Panel,
    Window,
    /// First-run setup drawn in the root window (screenshots only; the real
    /// app shows setup in its own window).
    Setup,
    /// The System Settings helper card alone (screenshots only).
    FdaHelp,
}

pub struct Options {
    pub screenshot: Option<PathBuf>,
    pub view: String,
}

pub struct App {
    pub tools: Vec<Box<dyn Tool>>,
    pub mode: Mode,
    /// Open detail sheet: (tool index, detail id).
    pub detail: Option<(usize, String)>,
    /// Tool whose page the full window shows.
    pub page: usize,
    pub demo: bool,
    /// The panel's widgets in order, with on/off.
    pub widgets: Vec<Entry>,
    /// Ids of the tools the user turned off: their widgets, pages, menu
    /// entries and suggestions are hidden.
    pub tools_off: Vec<String>,
    /// The window shows Settings (the widget editor) instead of a tool page.
    pub settings_open: bool,
    /// Settings tab: 0 widgets, 1 tools.
    pub settings_tab: usize,
    /// A widget being dragged in the editor.
    pub drag: Option<super::editor::Drag>,
    /// The widget selected in Settings › Widgets (its size bar shows).
    pub edit_sel: Option<String>,
    /// The search text above the widget gallery in Settings › Widgets.
    pub widget_search: String,
    /// The widget whose sizes the gallery shows.
    pub gallery_pick: Option<String>,
    // window/tray state
    tray: Option<tray_icon::TrayIcon>,
    tray_rx: Receiver<tray_icon::TrayIconEvent>,
    tray_title: String,
    anchor: Option<Pos2>,
    visible: bool,
    had_focus: bool,
    last_hide: Instant,
    pub app_open: bool,
    /// When the window was asked for (dev log: how long the first frame took).
    pub app_asked: Option<Instant>,
    /// When the app window (or setup in it) last drew.
    app_drawn: Option<Instant>,
    /// When the system appearance (and new windows' glass) was last checked.
    theme_checked: Option<Instant>,
    app_focus: bool,
    /// Frames left before the panel hides after the window was opened from
    /// it: the window shows first, so the app always has a window on screen
    /// and macOS doesn't hand focus back to the app before (the window
    /// opened behind other apps).
    panel_hide_in: u8,
    pending_shape: bool,
    screenshot: Option<PathBuf>,
    frame: u64,
    /// last (ppp, native ppp) logged under CMDMEOW_SCALE_LOG
    debug_scale: Option<(f32, Option<f32>)>,
    pub toast: Option<(String, Instant)>,
    /// First-run setup, while its window is open.
    pub setup: Option<super::onboard::Setup>,
    /// Live permission statuses (setup and Settings › General).
    pub perm_watch: super::onboard::PermWatch,
    /// Keep the panel hidden at launch (setup opens instead).
    start_hidden: bool,
    /// Right after setup hands over to the panel: macOS moves focus around
    /// while the setup window closes and the Dock icon goes; the panel must
    /// not take that for a click elsewhere.
    hold_panel: Option<Instant>,
    /// `cmdmeow://open/<tool>[/<route>]` links, as (tool, route).
    links: Option<crate::kit::url_scheme::Inbox<(String, String)>>,
    /// The update banner was shown last frame (see `kit::update`).
    update_banner: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        install_fonts(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let demo = opts.screenshot.is_some();
        // screenshots stay silent; otherwise sounds are on until switched off
        crate::kit::sound::set_on(!demo && cc.storage.and_then(|s| eframe::get_value(s, crate::kit::sound::STORAGE_KEY)).unwrap_or(true));
        let tools = crate::tools::all(&ToolEnv { ctx: ctx.clone(), demo });

        // the menu bar icon
        let (tx, tray_rx): (Sender<tray_icon::TrayIconEvent>, _) = std::sync::mpsc::channel();
        let c2 = ctx.clone();
        tray_icon::TrayIconEvent::set_event_handler(Some(move |e| {
            let _ = tx.send(e);
            c2.request_repaint();
        }));
        let tray = if demo {
            None
        } else {
            tray_icon::TrayIconBuilder::new()
                .with_icon(tray_image())
                .with_icon_as_template(true)
                .with_tooltip("Cmd+Meow")
                .with_menu_on_left_click(false)
                .build()
                .ok()
        };

        // screenshots always use the default widgets
        let prefs: WidgetPrefs = if demo { None } else { cc.storage.and_then(|s| eframe::get_value(s, super::settings::STORAGE_KEY)) }.unwrap_or_default();
        let stored_off: Option<Vec<String>> = if demo { None } else { cc.storage.and_then(|s| eframe::get_value(s, super::settings::TOOLS_KEY)) };
        // a fresh install shows setup and starts with a few tools off
        let first_run = !demo && !super::onboard::done_marker().exists();
        let tools_off: Vec<String> = match stored_off {
            Some(v) => v,
            None if first_run => super::onboard::off_at_first(tools.iter().map(|t| t.id())),
            None => Vec::new(),
        };
        // what macOS said about Screen Recording at launch (it only applies
        // a later grant after a restart)
        let _ = crate::kit::perms::screen_at_launch();
        // the window opens on the sidebar's first page (Clean section first)
        let page = tools.iter().position(|t| t.has_page() && t.section() == "Clean").or_else(|| tools.iter().position(|t| t.has_page())).unwrap_or(0);
        let mut app = App {
            tools,
            mode: Mode::Panel,
            detail: None,
            page,
            demo,
            widgets: Vec::new(),
            tools_off,
            settings_open: false,
            settings_tab: 0,
            drag: None,
            edit_sel: None,
            widget_search: String::new(),
            gallery_pick: None,
            tray,
            tray_rx,
            tray_title: String::new(),
            anchor: None,
            visible: true,
            had_focus: false,
            last_hide: Instant::now(),
            app_open: false,
            app_asked: None,
            app_drawn: None,
            theme_checked: None,
            app_focus: false,
            panel_hide_in: 0,
            pending_shape: true,
            screenshot: opts.screenshot,
            frame: 0,
            debug_scale: None,
            toast: None,
            setup: first_run.then(|| super::onboard::Setup::new(super::onboard::saved_step())),
            perm_watch: Default::default(),
            start_hidden: first_run,
            hold_panel: None,
            links: (!demo).then(|| crate::kit::url_scheme::Inbox::new("open", Some(ctx.clone()), open_link)),
            update_banner: false,
        };
        app.widgets = prefs.resolve(&app.available_widgets());
        for i in 0..app.tools.len() {
            if !app.tool_on(i) {
                app.tools[i].set_on(false);
            }
        }
        app.apply_view(&opts.view);
        // screenshots: CMDMEOW_STOPPED=capture,clipboard shows those tools as stopped after an error
        if let Some(v) = std::env::var_os("CMDMEOW_STOPPED") {
            let v = v.to_string_lossy().into_owned();
            for t in &app.tools {
                if v.split(',').any(|x| x.trim() == t.id()) {
                    super::tool::stop(t.id());
                }
            }
        }
        // setup is the app window's first state: nothing else opens before it
        if app.setup.is_some() && app.mode != Mode::Setup && app.screenshot.is_none() {
            app.app_open = true;
            app.app_focus = true;
        }
        crate::kit::native::set_dock_icon(app.mode == Mode::Window || app.app_open || demo);
        app
    }

    /// Every widget any tool (or the shell) offers, as (key, shown by
    /// default, default size).
    pub fn available_widgets(&self) -> Vec<Avail> {
        let mut v: Vec<Avail> = Vec::new();
        for t in &self.tools {
            for w in t.widgets() {
                v.push((widget_key(t.id(), w.id), w.default_on, w.size));
            }
        }
        // the registry's preferred order first, the rest as the tools list them
        let rank = |k: &str| crate::tools::DEFAULT_WIDGET_ORDER.iter().position(|o| *o == k).unwrap_or(usize::MAX);
        v.sort_by_key(|(k, _, _)| rank(k));
        v
    }

    /// Spec and owner of a widget key; `None` owner means the shell.
    pub fn widget_spec(&self, key: &str) -> Option<(Option<usize>, WidgetSpec)> {
        let (tool, id) = key.split_once('/')?;
        let i = self.tools.iter().position(|t| t.id() == tool)?;
        let spec = self.tools[i].widgets().into_iter().find(|w| w.id == id)?;
        Some((Some(i), spec))
    }

    pub fn save_widgets(&self, storage: &mut dyn eframe::Storage) {
        if !self.demo {
            eframe::set_value(storage, super::settings::STORAGE_KEY, &WidgetPrefs::from_entries(&self.widgets));
            eframe::set_value(storage, super::settings::TOOLS_KEY, &self.tools_off);
            eframe::set_value(storage, crate::kit::sound::STORAGE_KEY, &crate::kit::sound::on());
        }
    }

    /// Back to the default widgets in the default order.
    pub fn reset_widgets(&mut self) {
        self.widgets = WidgetPrefs::default().resolve(&self.available_widgets());
    }

    /// Whether the user left tool `i` on.
    pub fn tool_on(&self, i: usize) -> bool {
        self.tools.get(i).is_some_and(|t| !self.tools_off.iter().any(|id| id == t.id()))
    }

    pub fn set_tool_on(&mut self, i: usize, on: bool) {
        let id = self.tools[i].id().to_string();
        self.tools_off.retain(|x| *x != id);
        self.tools[i].set_on(on);
        if !on {
            self.tools_off.push(id);
            if self.detail.as_ref().is_some_and(|(d, _)| *d == i) {
                self.detail = None;
            }
        }
    }

    /// Open the window on Settings (`tools`: on the Tools tab).
    pub fn open_settings(&mut self, ctx: &egui::Context, tools: bool) {
        let page = self.page;
        self.open_window(ctx, page);
        self.settings_open = true;
        self.settings_tab = if tools { 1 } else { 0 };
    }

    /// `--view`: "panel", or whatever a tool recognizes.
    fn apply_view(&mut self, view: &str) {
        // "locked-<view>": the same view with Screen Recording and Full Disk Access missing
        if let Some(rest) = view.strip_prefix("locked-") {
            use crate::kit::perms::{Perm, Status};
            self.perm_watch.demo = Some(Perm::SYSTEM.into_iter().map(|p| (p, if matches!(p, Perm::Screen | Perm::FullDisk) { Status::NotAsked } else { Status::Allowed })).collect());
            return self.apply_view(rest);
        }
        match view {
            "panel" => return,
            // the widget editor, with a widget selected / a search typed
            "edit" | "gallery" => {
                self.mode = Mode::Window;
                self.settings_open = true;
                self.settings_tab = 0;
                if view == "edit" {
                    self.edit_sel = Some("fan/fans".into());
                } else {
                    self.widget_search = "mem".into();
                }
                return;
            }
            // one widget alone at one size: "widget-system/devices@4:4"
            v if v.starts_with("widget-") => {
                if let Some((key, z)) = v["widget-".len()..].split_once('@').and_then(|(k, z)| Size::parse(z).map(|z| (k.to_string(), z))) {
                    for e in &mut self.widgets {
                        e.on = false;
                    }
                    super::settings::add_entry(&mut self.widgets, &key, z, None);
                }
                return;
            }
            // showcases of the sizes: the new tools, then the monitors
            "sizes" | "sizes2" => {
                for t in &mut self.tools {
                    t.demo_view("sizes");
                }
                let show: &[(&str, Size)] = if view == "sizes" {
                    &[("cleanup/clean", Size::S11), ("system/cpu", Size::S11), ("system/memory", Size::S12), ("fan/fans", Size::S14), ("capture/capture", Size::S14), ("keep_awake/card", Size::S24), ("clipboard/recent", Size::S24)]
                } else {
                    &[("system/health", Size::S24), ("lens/map", Size::S24), ("claude/limits", Size::S24), ("system/storage", Size::S11), ("system/memory", Size::S11), ("system/battery", Size::S12), ("fan/fans", Size::S44)]
                };
                for e in &mut self.widgets {
                    e.on = false;
                }
                for (k, z) in show {
                    super::settings::add_entry(&mut self.widgets, k, *z, None);
                }
                return;
            }
            // every widget that offers Row (1:4), at that size
            "rows" => {
                for t in &mut self.tools {
                    t.demo_view("sizes");
                }
                for e in &mut self.widgets {
                    e.on = false;
                }
                let keys: Vec<String> = self.tools.iter().flat_map(|t| t.widgets().into_iter().filter(|w| w.sizes.contains(&Size::S14)).map(|w| super::settings::widget_key(t.id(), w.id)).collect::<Vec<_>>()).collect();
                for k in keys {
                    super::settings::add_entry(&mut self.widgets, &k, Size::S14, None);
                }
                return;
            }
            // first-run setup, one step each
            "setup" | "setup-tools" | "setup-tools-all" | "setup-tools-preview" | "setup-ai" | "setup-models" | "setup-perms" | "setup-crash" | "setup-ready" => {
                self.mode = Mode::Setup;
                let step = ["setup", "setup-tools", "setup-ai", "setup-models", "setup-perms", "setup-crash", "setup-ready"].iter().position(|v| *v == view).unwrap_or(0);
                self.setup = Some(super::onboard::Setup::new(step));
                use crate::kit::perms::{Perm, Status};
                self.perm_watch.demo = Some([(Perm::Screen, Status::Allowed), (Perm::Accessibility, Status::Allowed), (Perm::FullDisk, Status::NotAsked), (Perm::SystemEvents, Status::Denied), (Perm::Microphone, Status::Allowed)].into_iter().collect());
                // the full list, with a few more tools on (the cost line fills up)
                let all = view == "setup-tools-all";
                let more = [crate::tools::capture::ID, crate::tools::capture::VIDEO_ID, crate::tools::dictation::ID, crate::tools::system::ID];
                for i in 0..self.tools.len() {
                    let id = self.tools[i].id();
                    // the models step only exists with Dictation on
                    let on = super::onboard::ON_AT_FIRST.contains(&id) || (all && more.contains(&id)) || (view == "setup-models" && id == crate::tools::dictation::ID);
                    self.set_tool_on(i, on);
                }
                if all || view == "setup-tools-preview" {
                    let mut s = super::onboard::Setup::new(1);
                    s.all_tools = all;
                    // the pointer on "Screenshots & video"
                    s.demo_hover = (!all).then_some(1);
                    self.setup = Some(s);
                }
                return;
            }
            "fda-help" => {
                self.mode = Mode::FdaHelp;
                return;
            }
            "customize" | "settings-tools" | "settings-general" | "settings-ai" | "settings-ai-check" => {
                self.mode = Mode::Window;
                self.settings_open = true;
                self.settings_tab = match view {
                    "customize" => 0,
                    "settings-tools" => 1,
                    "settings-ai" => 3,
                    "settings-ai-check" => 4,
                    _ => 2,
                };
                return;
            }
            _ => {}
        }
        for i in 0..self.tools.len() {
            match self.tools[i].demo_view(view) {
                Some(View::Detail(d)) => {
                    self.detail = Some((i, d.to_string()));
                    return;
                }
                Some(View::Page) => {
                    self.mode = Mode::Window;
                    self.page = i;
                    return;
                }
                None => {}
            }
        }
        eprintln!("unknown --view {view:?}; showing the panel");
    }

    pub fn tool_index(&self, id: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.id() == id)
    }

    pub fn page_tools(&self) -> Vec<usize> {
        (0..self.tools.len()).filter(|&i| self.tools[i].has_page() && self.tool_on(i)).collect()
    }

    /// A fresh per-frame context for tools.
    pub fn cx(&self) -> Cx {
        Cx::new(self.detail.as_ref().map(|(i, d)| (self.tools[*i].id().to_string(), d.clone())))
    }

    /// Carry out what tools asked for this frame.
    pub fn run_actions(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        for a in cx.take_actions() {
            match a {
                Action::OpenPage { tool, route } => {
                    if let Some(i) = self.tool_index(&tool).filter(|&i| self.tools[i].has_page()) {
                        let t = &mut self.tools[i];
                        let _ = super::tool::guard(t.id(), || t.open_page(&route));
                        self.open_window(ctx, i);
                    }
                }
                Action::ShowDetail { tool, detail, toggle } => {
                    if let Some(i) = self.tool_index(&tool) {
                        let same = self.detail.as_ref() == Some(&(i, detail.clone()));
                        self.detail = if same && toggle { None } else { Some((i, detail)) };
                        self.reshape(ctx);
                    }
                }
                Action::CloseDetail => {
                    self.detail = None;
                    self.reshape(ctx);
                }
                Action::HidePanel => {
                    if self.mode == Mode::Panel && self.visible && self.screenshot.is_none() {
                        self.hide(ctx);
                    }
                }
                Action::Toast(msg) => self.toast = Some((msg, Instant::now())),
                Action::Quit => self.quit(ctx),
            }
        }
    }

    /// Whether a tool's detail sheet sits beside the panel.
    pub fn sheet_open(&self) -> bool {
        self.detail.is_some()
    }

    /// The panel's height for its current widgets, before the screen limit.
    fn panel_height(&self) -> f32 {
        let content = super::panel::content_height(self);
        let h = super::panel::HEADER + content + super::panel::banner_h() + super::panel::FOOTER;
        if self.sheet_open() {
            h.max(m::PANEL_HEIGHT)
        } else {
            h
        }
    }

    pub fn max_panel_height(&self, ctx: &egui::Context) -> f32 {
        let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1512.0, 982.0));
        (screen.y - 40.0).max(400.0)
    }

    pub fn window_size(&self, ctx: &egui::Context) -> Vec2 {
        match self.mode {
            Mode::Panel => {
                let w = m::PANEL_WIDTH + if self.sheet_open() { m::DETAIL_WIDTH + 12.0 } else { 0.0 };
                Vec2::new(w, self.panel_height().min(self.max_panel_height(ctx)))
            }
            // screenshots: CMDMEOW_SHOT_W=<pt> (or "min") checks narrow windows
            Mode::Window => match std::env::var("CMDMEOW_SHOT_W").ok().filter(|_| self.screenshot.is_some()) {
                Some(w) => Vec2::new(if w == "min" { Self::min_app_width() } else { w.parse().unwrap_or(m::APP_WIDTH).max(Self::min_app_width()) }, m::APP_HEIGHT),
                None => Vec2::new(m::APP_WIDTH, m::APP_HEIGHT),
            },
            Mode::Setup => Vec2::new(super::onboard::SETUP_W, super::onboard::SETUP_H),
            Mode::FdaHelp => Vec2::new(360.0, 132.0),
        }
    }

    /// Resize/move the window for the current mode.
    pub fn reshape(&mut self, ctx: &egui::Context) {
        let size = self.window_size(ctx);
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::InnerSize(size));
        if self.screenshot.is_some() {
            return;
        }
        let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1512.0, 982.0));
        match self.mode {
            Mode::Panel => {
                let anchor = self.anchor.unwrap_or(Pos2::new(screen.x - m::PANEL_WIDTH / 2.0 - 16.0, 30.0));
                let right = (anchor.x + m::PANEL_WIDTH / 2.0).clamp(m::PANEL_WIDTH, screen.x - 8.0);
                let x = (right - size.x).max(8.0);
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::OuterPosition(Pos2::new(x, anchor.y + 6.0)));
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                // the app window may still be open (or minimized) behind the
                // panel: it keeps its Dock icon. Dropping it made macOS pull
                // a minimized window back up, and an open one look quit
                crate::kit::native::set_dock_icon(self.app_open || self.setup.is_some());
            }
            Mode::Window | Mode::Setup | Mode::FdaHelp => {
                let pos = Pos2::new(((screen.x - size.x) / 2.0).max(0.0), ((screen.y - size.y) / 2.0).max(30.0));
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
                crate::kit::native::set_dock_icon(true);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.visible = true;
        self.had_focus = false;
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Visible(true));
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Focus);
        crate::kit::native::activate();
    }

    pub fn hide(&mut self, ctx: &egui::Context) {
        self.visible = false;
        self.last_hide = Instant::now();
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Visible(false));
    }

    /// Open the full app in its own normal window on a tool's page; the menu
    /// panel steps aside.
    pub fn open_window(&mut self, ctx: &egui::Context, page: usize) {
        self.page = page;
        self.settings_open = false;
        if self.screenshot.is_some() {
            return;
        }
        self.app_open = true;
        self.app_focus = true;
        self.app_asked = Some(Instant::now());
        crate::kit::native::set_dock_icon(true);
        if self.visible {
            self.panel_hide_in = 2;
            ctx.request_repaint();
        }
    }

    pub fn open_panel(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Panel;
        self.reshape(ctx);
        self.show(ctx);
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.tray_rx.try_recv() {
            if let tray_icon::TrayIconEvent::Click { rect, button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = ev {
                let ppp = ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(2.0);
                let cx = (rect.position.x + rect.size.width as f64 / 2.0) as f32 / ppp;
                let bottom = (rect.position.y + rect.size.height as f64) as f32 / ppp;
                self.anchor = Some(Pos2::new(cx, bottom));
                let just_hid = self.last_hide.elapsed().as_millis() < 350;
                if self.setup.is_some() && self.mode == Mode::Panel {
                    // until setup is finished the menu bar only leads back to it
                    self.open_setup_window(ctx);
                } else if self.visible && self.mode == Mode::Panel {
                    self.hide(ctx);
                } else if !just_hid || self.mode == Mode::Window {
                    self.open_panel(ctx);
                }
            }
        }
    }

    fn update_tray_title(&mut self) {
        let Some(tray) = &self.tray else { return };
        // until setup is finished the menu bar says so (and a click opens setup)
        let title = if self.setup.is_some() || !super::onboard::is_done() {
            "Finish setup".to_string()
        } else {
            self.tools.iter().enumerate().filter(|(i, _)| self.tool_on(*i)).find_map(|(_, t)| t.tray_title()).unwrap_or_default()
        };
        if title != self.tray_title {
            // tray-icon's set_title(None) leaves the old text on macOS (the
            // recording clock stayed after a stop); an empty title clears it
            tray.set_title(Some(&title));
            self.tray_title = title;
        }
    }
}

/// `cmdmeow://open/disk-map` → ("disk-map", ""); `open/capture/history` →
/// ("capture", "history"). The tool is matched later, when the tools are known.
fn open_link(action: &str) -> Option<(String, String)> {
    let (tool, route) = action.split_once('/').unwrap_or((action, ""));
    (!tool.is_empty()).then(|| (tool.to_string(), route.to_string()))
}

/// A tool named in a link: its id (`keep_awake`) or its name written as a
/// link (`keep-awake`, `disk-map`, `mac-health`).
fn link_matches(id: &str, name: &str, wanted: &str) -> bool {
    let slug = |s: &str| s.to_ascii_lowercase().replace([' ', '_'], "-");
    slug(id) == slug(wanted) || slug(name) == slug(wanted)
}

impl App {
    /// Open the window where a `cmdmeow://open/…` link points. `settings`
    /// opens Settings; a tool that's off or has no page is ignored.
    fn follow_links(&mut self, ctx: &egui::Context) {
        let Some(links) = &self.links else { return };
        for (tool, route) in links.take() {
            if tool == "settings" {
                self.open_settings(ctx, route == "tools");
                continue;
            }
            // the app opened again while running (Finder, Spotlight, Dock):
            // show the window, so it is reachable with the menu bar icon hidden
            if tool == "window" {
                if self.setup.is_some() {
                    self.open_setup_window(ctx);
                } else {
                    let page = self.page;
                    self.open_window(ctx, page);
                }
                continue;
            }
            let found = (0..self.tools.len()).find(|&i| link_matches(self.tools[i].id(), self.tools[i].name(), &tool));
            match found.filter(|&i| self.tool_on(i) && self.tools[i].has_page()) {
                Some(i) => {
                    let t = &mut self.tools[i];
                    let _ = super::tool::guard(t.id(), || t.open_page(&route));
                    self.open_window(ctx, i);
                }
                None => eprintln!("[url] no tool page \"{tool}\" to open"),
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::kit::repaint_log::begin(ctx);
        crate::kit::url_scheme::after_launch();
        self.follow_links(ctx);
        crate::kit::update::tick();
        if crate::kit::update::take_restart() {
            self.quit(ctx);
            return;
        }
        // the update banner came or went: the panel changes height
        let banner = crate::kit::update::banner();
        if banner != self.update_banner {
            self.update_banner = banner;
            self.pending_shape = true;
        }
        // tools poll their background work; some need frames while hidden
        let mut tools_need_frames = false;
        for i in 0..self.tools.len() {
            if self.tool_on(i) {
                let t = &mut self.tools[i];
                tools_need_frames |= super::tool::guard(t.id(), || t.tick(ctx)).unwrap_or(false);
            }
        }
        // a tool that stopped after an error lets go of what it holds system-wide
        for id in super::tool::newly_broken() {
            if let Some(t) = self.tools.iter_mut().find(|t| t.id() == id) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.set_on(false)));
            }
            if self.detail.as_ref().is_some_and(|(d, _)| self.tools.get(*d).is_some_and(|t| t.id() == id)) {
                self.detail = None;
            }
        }
        // the app window is drawn inside the hidden root window's pass; once
        // it's on screen eframe runs that pass for it anyway. Asking again
        // every pass was a loop: each ask is itself a repaint request, so a
        // still window drew 10 times a second
        let app_drawing = self.app_drawn.is_some_and(|t| t.elapsed().as_millis() < 1500);
        if ((self.app_open || self.setup.is_some()) && !app_drawing || tools_need_frames) && !self.visible {
            ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
        }
        // background work slows down and stops asking for frames while
        // neither the panel nor the window is on screen (a tool's own small
        // windows, like a pinned capture, show none of it); this heartbeat
        // still runs the ticks and keeps the menu-bar title current
        let on_screen = self.visible || self.app_open || self.setup.is_some();
        crate::kit::activity::set_visible(on_screen);
        if !on_screen {
            ctx.request_repaint_after(HIDDEN_HEARTBEAT);
        }
        if self.start_hidden {
            self.start_hidden = false;
            self.hide(ctx);
        }
        self.handle_tray(ctx);
        self.update_tray_title();
        if self.pending_shape {
            self.pending_shape = false;
            self.reshape(ctx);
        }
        // closing the window keeps the app in the menu bar
        if ctx.input(|i| i.viewport().close_requested()) && self.screenshot.is_none() && !QUIT.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        // the panel behaves like a popover: it hides when you click elsewhere
        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.had_focus = true;
        }
        if let Some(at) = self.hold_panel {
            if at.elapsed().as_millis() > 1500 {
                self.hold_panel = None;
            } else if focused == Some(false) && self.visible {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Focus);
                crate::kit::native::activate();
            }
        }
        if self.hold_panel.is_none() && self.mode == Mode::Panel && self.visible && self.had_focus && focused == Some(false) && self.screenshot.is_none() {
            self.hide(ctx);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_widgets(storage);
    }

    fn on_exit(&mut self) {
        // tools save some files in the background; let them land
        crate::kit::fs::flush_writes();
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        // screenshots and demo views render egui alone: no native layers
        crate::kit::spin::set_allowed(self.screenshot.is_none() && !self.demo);
        // by time, not by pass count: a still window has few passes, and a
        // theme switch (which brings a pass of its own) must show at once.
        // A pass too soon after the last check asks for one more later
        const THEME_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
        let since = self.theme_checked.map(|t| t.elapsed());
        if since.is_some_and(|d| d < THEME_EVERY) && self.frame > 3 {
            ctx.request_repaint_after(THEME_EVERY - since.unwrap_or_default());
        } else {
            self.theme_checked = Some(Instant::now());
            sync_theme(&ctx, self.screenshot.is_none());
            let r = if self.mode == Mode::Panel { m::CARD_RADIUS as f64 + 6.0 } else { 16.0 };
            if self.screenshot.is_none() {
                crate::kit::native::apply_glass(r);
            }
        }
        // CMDMEOW_SELF_SHOT=<dir>: open the panel, then let screencapture
        // (run by the app, so with its Screen Recording permission) save the
        // screen, to check how the panel looks over the desktop
        if let Some(dir) = env_once("CMDMEOW_SELF_SHOT") {
            if self.frame == 60 {
                self.open_panel(&ctx);
            }
            if self.frame == 150 {
                let out = std::path::Path::new(&dir).join("panel.png");
                let ok = std::process::Command::new("/usr/sbin/screencapture").arg("-x").arg(&out).status().map(|s| s.success()).unwrap_or(false);
                eprintln!("[self-shot] {} {}", out.display(), if ok { "saved" } else { "failed" });
            }
            ctx.request_repaint();
        }
        if env_once("CMDMEOW_SCALE_LOG").is_some() {
            // the scale egui renders at, logged when it changes (the panel
            // shown on another display)
            let scale = (ctx.pixels_per_point(), ctx.input(|i| i.viewport().native_pixels_per_point));
            if self.debug_scale != Some(scale) {
                self.debug_scale = Some(scale);
                eprintln!("[scale] ppp {} native {:?} frame {}\n{}", scale.0, scale.1, self.frame, crate::kit::native::debug_windows());
            }
        }
        if env_once("CMDMEOW_DEBUG").is_some() {
            match self.frame {
                30 => eprintln!("== start screen {:?}\n{}", ui.max_rect(), crate::kit::native::debug_windows()),
                120 => eprintln!("== panel screen {:?}\n{}", ui.max_rect(), crate::kit::native::debug_windows()),
                150 => {
                    let p = self.page;
                    self.open_window(&ctx, p)
                }
                220 => eprintln!("== app screen {:?}\n{}", ui.max_rect(), crate::kit::native::debug_windows()),
                _ => {}
            }
            ctx.request_repaint();
        }
        let full = ui.max_rect();
        // screenshots: wait until the window has its size for the chosen view
        if self.screenshot.is_some() && (full.size() - self.window_size(&ctx)).length() > 2.0 && self.frame < 60 {
            self.screenshot_step(&ctx);
            return;
        }
        let opaque = self.screenshot.is_some();
        let t = ctx.input(|i| i.time);
        let mut cx = self.cx();
        match self.mode {
            // the root window is the panel; while it's hidden, its pass still
            // runs for the app window and tools' own windows (drawn inside
            // it), but nobody sees the panel: don't build it
            Mode::Panel if !self.visible && self.screenshot.is_none() => {}
            Mode::Panel => {
                // one glass sheet; the detail opens on its left behind a hairline
                glass::backdrop(ui.painter(), full, m::CARD_RADIUS + 2.0, opaque, t);
                let panel = Rect::from_min_size(Pos2::new(full.right() - m::PANEL_WIDTH, full.top()), Vec2::new(m::PANEL_WIDTH, full.height()));
                if self.sheet_open() {
                    let sheet = Rect::from_min_size(full.left_top(), Vec2::new(m::DETAIL_WIDTH, full.height()));
                    let x = panel.left() - 6.0;
                    ui.painter().line_segment([Pos2::new(x, full.top() + 18.0), Pos2::new(x, full.bottom() - 18.0)], egui::Stroke::new(1.0, white(30)));
                    super::panel::detail(self, ui, sheet, &mut cx);
                }
                let before = self.widgets.clone();
                super::panel::panel(self, ui, panel, &mut cx);
                if self.widgets != before {
                    if let Some(s) = frame.storage_mut() {
                        self.save_widgets(s);
                    }
                    self.reshape(&ctx);
                }
            }
            Mode::FdaHelp => {
                let now = ctx.input(|i| i.time);
                super::onboard::help_card(ui, full, now + 0.9, crate::kit::perms::Perm::FullDisk);
            }
            Mode::Setup => {
                glass::backdrop(ui.painter(), full, 14.0, opaque, t);
                super::onboard::setup_ui(self, ui, full);
            }
            Mode::Window => {
                glass::backdrop(ui.painter(), full, 14.0, opaque, t);
                let before = (self.widgets.clone(), self.tools_off.clone());
                super::window::window_ui(self, ui, full, opaque, &mut cx);
                if (&self.widgets, &self.tools_off) != (&before.0, &before.1) {
                    if let Some(s) = frame.storage_mut() {
                        self.save_widgets(s);
                    }
                }
            }
        }
        for i in 0..self.tools.len() {
            if self.tool_on(i) {
                let t = &mut self.tools[i];
                let _ = super::tool::guard(t.id(), || t.windows(&ctx, &mut cx));
            }
        }
        self.run_actions(&ctx, &mut cx);
        if let Some((msg, at)) = &self.toast {
            if at.elapsed().as_secs_f32() < 3.0 {
                let p = ui.painter();
                let g = p.layout_no_wrap(msg.clone(), font(13.0), TEXT());
                let r = Rect::from_center_size(Pos2::new(full.center().x, full.bottom() - 40.0), g.size() + Vec2::new(28.0, 16.0));
                p.rect_filled(r, r.height() / 2.0, POPOVER());
                p.rect_stroke(r, r.height() / 2.0, egui::Stroke::new(1.0, CARD_EDGE()), egui::StrokeKind::Inside);
                p.galley(r.center() - g.size() / 2.0, g, TEXT());
                ctx.request_repaint();
            } else {
                self.toast = None;
            }
        }
        let off_before = (self.tools_off.clone(), crate::kit::sound::on());
        if self.app_open {
            self.app_viewport(&ctx);
            if self.panel_hide_in > 0 {
                self.panel_hide_in -= 1;
                if self.panel_hide_in == 0 && self.visible && self.mode == Mode::Panel {
                    self.hide(&ctx);
                } else {
                    ctx.request_repaint();
                }
            }
        } else if self.visible && self.mode == Mode::Panel && self.screenshot.is_none() {
            self.prewarm_app(&ctx);
        }
        if self.screenshot.is_none() {
            super::onboard::perm_helper(self, &ctx);
        }
        if (&self.tools_off, crate::kit::sound::on()) != (&off_before.0, off_before.1) {
            if let Some(s) = frame.storage_mut() {
                self.save_widgets(s);
            }
        }
        // keep clocks and live numbers moving, once a second on the wall
        // clock's beat (background work asks for its own frames when its
        // numbers change); with nothing on screen the hidden heartbeat above
        // is the only wake-up (a shorter one here would win, since egui keeps
        // the earliest)
        if self.visible {
            ctx.request_repaint_after(glass::next_second());
        } else if self.app_open || self.setup.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(2000));
        }
        self.screenshot_step(&ctx);
        crate::kit::spin::flush(&ctx);
        crate::kit::spin::drop_unflushed();
        crate::kit::viewport::end_pass();
        crate::kit::repaint_log::end();
    }
}

/// A debugging switch from the environment, read once: `getenv` locks, and
/// the pass runs up to the display's rate.
fn env_once(key: &'static str) -> Option<&'static std::ffi::OsStr> {
    static VARS: std::sync::Mutex<Vec<(&'static str, Option<&'static std::ffi::OsStr>)>> = std::sync::Mutex::new(Vec::new());
    let mut v = VARS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, val)) = v.iter().find(|(k, _)| *k == key) {
        return *val;
    }
    let val: Option<&'static std::ffi::OsStr> = std::env::var_os(key).map(|s| &*Box::leak(s.into_boxed_os_str()));
    v.push((key, val));
    val
}

/// macOS: right after launch the app window kept showing its first frames
/// (setup frozen on Welcome, clicks working underneath) until it was resized
/// once; from then on it drew fine. So resize it by a point and back once,
/// shortly after it first shows.
fn kick_first_frames(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0);
    let Some(r) = ctx.input(|i| i.viewport().inner_rect) else { return };
    let id = egui::Id::new("kick-first-frames");
    let now = ctx.input(|i| i.time);
    let first = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    let t = now - first;
    match STATE.load(Ordering::Relaxed) {
        0 if t > 0.5 => {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(r.size() + egui::vec2(1.0, 0.0)));
            STATE.store(1, Ordering::Relaxed);
        }
        1 if t > 0.6 => {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(r.size() - egui::vec2(1.0, 0.0)));
            STATE.store(2, Ordering::Relaxed);
        }
        0 | 1 => ctx.request_repaint(),
        _ => {}
    }
}

pub static QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl App {
    pub fn quit(&mut self, ctx: &egui::Context) {
        QUIT.store(true, std::sync::atomic::Ordering::SeqCst);
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Close);
    }

    /// The narrowest the app window goes (Settings › Widgets sets it).
    fn min_app_width() -> f32 {
        crate::kit::glass::SIDEBAR_W + super::editor::MIN_WIDTH
    }

    /// The full app in its own native window (traffic lights, resizable).
    fn app_builder() -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title("Cmd+Meow")
            .with_inner_size([m::APP_WIDTH, m::APP_HEIGHT])
            // Settings › Widgets sets the narrowest the window can go
            .with_min_inner_size([Self::min_app_width(), 640.0])
            .with_transparent(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_has_shadow(true)
    }

    /// While the panel is open, keep the app window made but hidden, so a
    /// click on a widget only shows it. Making the native window (and its GPU
    /// surface) on the click was the lag before the window appeared.
    fn prewarm_app(&mut self, ctx: &egui::Context) {
        let id = egui::ViewportId::from_hash_of("cmdmeow-app");
        ctx.show_viewport_deferred(id, Self::app_builder().with_visible(false), |_, _| {});
    }

    fn app_viewport(&mut self, ctx: &egui::Context) {
        let mut builder = Self::app_builder().with_visible(true);
        if self.app_focus {
            builder = builder.with_active(true);
        }
        let id = egui::ViewportId::from_hash_of("cmdmeow-app");
        use super::onboard::Finish;
        let mut finish = Finish::No;
        crate::kit::viewport::show(ctx, id, builder, |ui, _class| {
            self.app_drawn = Some(Instant::now());
            let vctx = ui.ctx().clone();
            if let Some(at) = self.app_asked.take() {
                if crate::kit::fs::is_dev() {
                    eprintln!("[timing] window first frame {} ms after the click", at.elapsed().as_millis());
                }
            }
            if self.app_focus {
                self.app_focus = false;
                // a minimized window comes back up when it's asked for (a
                // no-op otherwise)
                vctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                vctx.send_viewport_cmd(ViewportCommand::Focus);
                crate::kit::native::activate();
            }
            if vctx.input(|i| i.viewport().close_requested()) {
                // closing during setup only hides it: the menu bar brings it back
                self.app_open = false;
                crate::kit::native::set_dock_icon(false);
                return;
            }
            crate::kit::native::apply_glass(16.0);
            kick_first_frames(&vctx);
            let full = ui.max_rect();
            let t = vctx.input(|i| i.time);
            glass::backdrop(ui.painter(), full, 16.0, false, t);
            if self.setup.is_some() {
                finish = super::onboard::setup_ui(self, ui, full);
                return;
            }
            let mut cx = self.cx();
            super::window::window_ui(self, ui, full, false, &mut cx);
            self.run_actions(&vctx, &mut cx);
            vctx.request_repaint_after(glass::next_second());
        });
        if finish != Finish::No {
            crate::kit::login::default_on();
            super::onboard::mark_done();
            self.setup = None;
            self.app_open = false;
            crate::kit::native::set_dock_icon(false);
            match finish {
                Finish::Restart => {
                    crate::kit::perms::relaunch();
                    self.quit(ctx);
                }
                Finish::Panel => {
                    self.hold_panel = Some(Instant::now());
                    self.open_panel(ctx);
                }
                Finish::No => {}
            }
        }
    }

    /// Show unfinished setup in the app window, on the step it was left at.
    pub fn open_setup_window(&mut self, ctx: &egui::Context) {
        if self.setup.is_none() {
            self.setup = Some(super::onboard::Setup::new(super::onboard::saved_step()));
        }
        self.app_open = true;
        self.app_focus = true;
        self.app_asked = Some(Instant::now());
        crate::kit::native::set_dock_icon(true);
        if self.visible {
            self.panel_hide_in = 2;
            ctx.request_repaint();
        }
    }

    /// Open first-run setup again (Settings › General).
    pub fn open_setup(&mut self) {
        self.setup = Some(super::onboard::Setup::new(0));
        self.app_open = true;
        self.app_focus = true;
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        if self.frame == 120 {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                save_png(&img, &path);
                std::process::exit(0);
            })));
        }
        if self.frame > 400 {
            eprintln!("screenshot was not delivered");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------- helpers

fn save_png(img: &egui::ColorImage, path: &std::path::Path) {
    let [w, h] = img.size;
    let bg = [18u8, 10, 40];
    let mut buf = image::RgbaImage::new(w as u32, h as u32);
    for (i, px) in img.pixels.iter().enumerate() {
        let [r, g, b, a] = px.to_srgba_unmultiplied();
        let af = a as f32 / 255.0;
        let mix = |c: u8, k: u8| (c as f32 * af + k as f32 * (1.0 - af)).round() as u8;
        buf.put_pixel((i % w) as u32, (i / w) as u32, image::Rgba([mix(r, bg[0]), mix(g, bg[1]), mix(b, bg[2]), 255]));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = buf.save(path) {
        eprintln!("couldn't save screenshot: {e}");
    }
}

/// The menu bar icon: the Cmd+Meow keycap cat with ⌘ cut out, drawn from
/// vector paths at 36 px (18 pt @2x) and set as a template so macOS tints it.
fn tray_image() -> tray_icon::Icon {
    let n = 36;
    tray_icon::Icon::from_rgba(crate::kit::brand::glyph_rgba(n), n, n).expect("tray icon")
}

/// Follow the system appearance (or `CMDMEOW_THEME=light|dark`). On a
/// change: new palette, font weights, egui visuals and window material.
fn sync_theme(ctx: &egui::Context, live: bool) {
    let light = match std::env::var("CMDMEOW_THEME").as_deref() {
        Ok("light") => true,
        Ok("dark") => false,
        _ => live && crate::kit::native::system_light(),
    };
    if glass::set_light(light) {
        install_fonts(ctx);
        if live {
            crate::kit::native::set_appearance(light);
        }
        ctx.request_repaint();
    }
}

fn install_fonts(ctx: &egui::Context) {
    use egui::epaint::text::VariationCoords;
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    // read once and shared by every weight (and every theme switch, which
    // installs the fonts again): SF is one big variable font file, and a
    // copy per weight held it in memory three times
    fn system_font(path: &str, cell: &'static std::sync::OnceLock<Option<egui::epaint::text::Blob>>) -> Option<egui::epaint::text::Blob> {
        cell.get_or_init(|| std::fs::read(path).ok().map(|b| Arc::new(b) as egui::epaint::text::Blob)).clone()
    }
    static SF: std::sync::OnceLock<Option<egui::epaint::text::Blob>> = std::sync::OnceLock::new();
    static MONO: std::sync::OnceLock<Option<egui::epaint::text::Blob>> = std::sync::OnceLock::new();
    let sf = system_font("/System/Library/Fonts/SFNS.ttf", &SF);
    let mono = system_font("/System/Library/Fonts/SFNSMono.ttf", &MONO);
    let mut bold_family = vec![];
    let mut hero_family = vec![];
    if let Some(bytes) = sf {
        // light text on dark reads heavier than dark text on light, so light
        // mode sets every weight a step up
        let light = glass::is_light();
        let (w_regular, w_bold, w_hero) = if light { (510.0, 680.0, 520.0) } else { (430.0, 620.0, 330.0) };
        let mut regular = FontData::from_blob(bytes.clone(), 0);
        regular.tweak.coords = VariationCoords::new([("wght", w_regular)]);
        let mut semibold = FontData::from_blob(bytes.clone(), 0);
        semibold.tweak.coords = VariationCoords::new([("wght", w_bold)]);
        let mut hero = FontData::from_blob(bytes, 0);
        hero.tweak.coords = VariationCoords::new([("wght", w_hero)]);
        fonts.font_data.insert("sf".into(), Arc::new(regular));
        fonts.font_data.insert("sf-bold".into(), Arc::new(semibold));
        fonts.font_data.insert("sf-hero".into(), Arc::new(hero));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "sf".into());
        bold_family.push("sf-bold".to_string());
        hero_family.push("sf-hero".to_string());
    }
    if let Some(bytes) = mono {
        fonts.font_data.insert("sf-mono".into(), Arc::new(FontData::from_blob(bytes, 0)));
        fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "sf-mono".into());
    }
    let mut fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // without SF the hero family is the bold one
    if hero_family.is_empty() {
        hero_family = bold_family.clone();
    }
    hero_family.extend(fallback.iter().cloned());
    bold_family.append(&mut fallback);
    fonts.families.insert(FontFamily::Name("bold".into()), bold_family);
    fonts.families.insert(FontFamily::Name("hero".into()), hero_family);
    ctx.set_fonts(fonts);
    ctx.set_visuals(if glass::is_light() { egui::Visuals::light() } else { egui::Visuals::dark() });
    ctx.all_styles_mut(|s| {
        s.visuals.panel_fill = Color32::TRANSPARENT;
        s.visuals.window_fill = glass::BG();
        // one scroll bar everywhere: slim, floating, in a gutter the content
        // leaves free, so even hovered it never sits on a row or button
        s.spacing.scroll.bar_width = glass::sp::SCROLL_BAR_W;
        s.spacing.scroll.floating = true;
        s.spacing.scroll.floating_allocated_width = glass::sp::SCROLL_GUTTER;
        s.spacing.scroll.bar_outer_margin = (glass::sp::SCROLL_GUTTER - glass::sp::SCROLL_BAR_W) / 2.0;
        s.visuals.widgets.noninteractive.fg_stroke.color = TEXT();
    });
}

#[cfg(test)]
mod tests {
    use super::{link_matches, open_link};

    #[test]
    fn open_links_name_a_tool_and_a_route() {
        assert_eq!(open_link("disk-map"), Some(("disk-map".into(), "".into())));
        assert_eq!(open_link("capture/history"), Some(("capture".into(), "history".into())));
        assert_eq!(open_link(""), None);
    }

    #[test]
    fn tools_match_by_id_or_name() {
        assert!(link_matches("lens", "Disk Map", "disk-map"));
        assert!(link_matches("lens", "Disk Map", "lens"));
        assert!(link_matches("keep_awake", "Keep Awake", "keep-awake"));
        assert!(link_matches("keep_awake", "Keep Awake", "keep_awake"));
        assert!(link_matches("claude", "AI Usage", "ai-usage"));
        assert!(!link_matches("lens", "Disk Map", "disk"));
    }
}
