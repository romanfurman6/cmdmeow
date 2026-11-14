//! The contract between the shell and a tool. A tool is a self-contained
//! module (Mac health, Claude limits, Cleanup, Disk Map, …) that offers any
//! of: widgets for the menu-bar panel, detail sheets beside the panel, a page
//! in the full window, recommendations, menu entries and a tray title.
//!
//! Tools never import each other. To reach another tool they ask the shell
//! through [`Cx`] by the other tool's id (`cx.open_page("cleanup", "")`); if
//! that tool isn't installed the request is ignored.

use crate::kit::glass::Icon;
use egui::{Color32, Rect, Ui};

/// Stable id of a tool, used in settings keys and cross-tool requests.
pub type ToolId = &'static str;

/// Number of columns in the panel's widget grid.
pub const GRID_COLUMNS: u8 = 4;

/// A widget's size on the panel grid, written rows × columns: one cell is
/// square, so `S12` is one row tall and half the panel wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Size {
    S11,
    S12,
    S14,
    S24,
    S44,
}

impl Size {
    pub const ALL: [Size; 5] = [Size::S11, Size::S12, Size::S14, Size::S24, Size::S44];
    /// (rows, columns)
    pub fn dims(self) -> (u8, u8) {
        match self {
            Size::S11 => (1, 1),
            Size::S12 => (1, 2),
            Size::S14 => (1, 4),
            Size::S24 => (2, 4),
            Size::S44 => (4, 4),
        }
    }
    pub fn cols(self) -> u8 {
        self.dims().1
    }
    /// "1:2", also the saved form.
    pub fn label(self) -> &'static str {
        match self {
            Size::S11 => "1:1",
            Size::S12 => "1:2",
            Size::S14 => "1:4",
            Size::S24 => "2:4",
            Size::S44 => "4:4",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Size::S11 => "Small",
            Size::S12 => "Wide",
            Size::S14 => "Row",
            Size::S24 => "Medium",
            Size::S44 => "Large",
        }
    }
    pub fn parse(s: &str) -> Option<Size> {
        Size::ALL.into_iter().find(|z| z.label() == s)
    }
}

/// A widget a tool offers for the panel.
#[derive(Clone, Copy, Debug)]
pub struct WidgetSpec {
    pub id: &'static str,
    /// Name shown in the widget gallery.
    pub title: &'static str,
    /// One line for the gallery.
    pub about: &'static str,
    /// The sizes it can be drawn at, smallest first.
    pub sizes: &'static [Size],
    /// Its size until the user picks another.
    pub size: Size,
    /// Shown until the user changes the panel.
    pub default_on: bool,
}

impl WidgetSpec {
    /// `want` if this widget offers it; a dropped Medium becomes its Row
    /// when it has one; else the default size.
    pub fn fit(&self, want: Size) -> Size {
        if self.sizes.contains(&want) {
            want
        } else if want == Size::S24 && self.sizes.contains(&Size::S14) {
            Size::S14
        } else {
            self.size
        }
    }
}

/// Every size but Large.
pub const UP_TO_MEDIUM: &[Size] = &[Size::S11, Size::S12, Size::S14, Size::S24];

/// What a tool needs to start: the egui context (for repaints from its
/// background threads) and whether it should show sample data.
pub struct ToolEnv {
    pub ctx: egui::Context,
    pub demo: bool,
}

/// The tool's icon and its one tint (sidebar chip, widget settings,
/// recommendation banner).
#[derive(Clone, Copy)]
pub struct Badge {
    pub icon: Icon,
    pub color: Color32,
}

/// The panel's title line: the first tool that returns one wins.
pub struct PanelHeader {
    pub title: String,
    pub status: String,
    pub dot: Color32,
}

/// A suggestion for the panel's "Today's Recommendation". The highest
/// priority across all tools wins.
pub struct Recommendation {
    pub priority: u32,
    pub title: String,
    pub body: String,
    /// Button label.
    pub action: String,
    /// What the button does.
    pub go: Action,
}

/// Requests from a tool to the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Open the full window on a tool's page. `route` is defined by that tool
    /// ("" = its default view).
    OpenPage { tool: String, route: String },
    /// Show a tool's detail sheet beside the panel. With `toggle`, a second
    /// request for the open sheet closes it.
    ShowDetail { tool: String, detail: String, toggle: bool },
    CloseDetail,
    /// Hide the menu-bar panel (e.g. before a screenshot of the screen).
    HidePanel,
    Toast(String),
    Quit,
}

/// What a `--view` name asks the shell to show (screenshots).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Detail(&'static str),
    Page,
}

/// Per-frame context handed to tools: what's open plus an outbox of [`Action`]s.
pub struct Cx {
    open_detail: Option<(String, String)>,
    actions: Vec<Action>,
}

impl Cx {
    pub fn new(open_detail: Option<(String, String)>) -> Self {
        Cx { open_detail, actions: Vec::new() }
    }
    pub fn open_page(&mut self, tool: &str, route: &str) {
        self.actions.push(Action::OpenPage { tool: tool.into(), route: route.into() });
    }
    /// Open or close (if already open) a detail sheet.
    pub fn toggle_detail(&mut self, tool: &str, detail: &str) {
        self.actions.push(Action::ShowDetail { tool: tool.into(), detail: detail.into(), toggle: true });
    }
    pub fn close_detail(&mut self) {
        self.actions.push(Action::CloseDetail);
    }
    pub fn toast(&mut self, msg: impl Into<String>) {
        self.actions.push(Action::Toast(msg.into()));
    }
    pub fn quit(&mut self) {
        self.actions.push(Action::Quit);
    }
    pub fn push(&mut self, a: Action) {
        self.actions.push(a);
    }
    /// Whether this tool's detail sheet `detail` is the one showing.
    pub fn is_detail_open(&self, tool: &str, detail: &str) -> bool {
        self.open_detail.as_ref().is_some_and(|(t, d)| t == tool && d == detail)
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
}

/// A tool. Every method except identity has a do-nothing default, so a tool
/// implements only what it offers.
pub trait Tool {
    fn id(&self) -> ToolId;
    fn name(&self) -> &'static str;
    fn badge(&self) -> Badge;
    /// Sidebar group, e.g. "Clean" or "Monitor".
    fn section(&self) -> &'static str {
        "Tools"
    }
    /// One line on what the tool does, for Settings › Tools.
    fn about(&self) -> &'static str {
        ""
    }
    /// The tool's shortcut beside it in the window sidebar ("⇧⌘2"), or what
    /// that shortcut is doing right now ("Listening", "0:12"). Nothing else:
    /// one column holds one kind of thing (docs/ui-rules.md T15).
    fn sidebar_meta(&self) -> String {
        String::new()
    }

    /// Working in the background (a scan, an AI Check): the sidebar shows a
    /// spinner in place of [`Tool::sidebar_meta`].
    fn busy(&self) -> bool {
        false
    }

    // ---- what it needs from macOS (setup and Settings › Permissions)
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        Vec::new()
    }
    /// Status of a need the tool sets up itself ([`crate::kit::perms::Perm::Own`]).
    fn own_status(&self, _id: &str) -> crate::kit::perms::Status {
        crate::kit::perms::Status::NotAsked
    }
    /// Start setting up an own need (may show an admin prompt; never blocks).
    fn ask_own(&mut self, _id: &str) {}
    /// How setting up an own need is going, when the tool can tell:
    /// (still working, last error). `None` keeps the plain timed spinner.
    fn own_progress(&self, _id: &str) -> Option<(bool, String)> {
        None
    }
    /// Models the tool downloads before it works (setup's own step, apart
    /// from the permissions).
    fn setup_models(&self) -> Vec<SetupModel> {
        Vec::new()
    }
    /// Start (or retry) downloading one of [`Tool::setup_models`].
    fn download_model(&mut self, _id: &str) {}
    /// Global shortcuts worth showing at the end of setup: (label, keys).
    fn setup_shortcuts(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    // ---- panel widgets
    fn panel_header(&self) -> Option<PanelHeader> {
        None
    }
    fn widgets(&self) -> Vec<WidgetSpec> {
        Vec::new()
    }
    /// Draw a widget at `size` (one the spec offers) filling `rect`.
    fn widget_ui(&mut self, _widget: &str, _size: Size, _ui: &mut Ui, _rect: Rect, _cx: &mut Cx) {}
    /// True while the widget has no data yet: the panel draws a loading
    /// card (glyph, name, pulsing bars) in its place.
    fn widget_loading(&self, _widget: &str) -> bool {
        false
    }

    // ---- detail sheets beside the panel
    /// Draws the whole sheet, header included (see `glass::sheet_header`).
    fn detail_ui(&mut self, _detail: &str, _ui: &mut Ui, _rect: Rect, _cx: &mut Cx) {}

    // ---- full-window page
    fn has_page(&self) -> bool {
        false
    }
    /// Called before the page is shown through a request; `route` is the
    /// tool-defined deep link.
    fn open_page(&mut self, _route: &str) {}
    fn page_ui(&mut self, _ui: &mut Ui, _rect: Rect, _cx: &mut Cx) {}

    // ---- shell hooks
    fn recommendations(&self) -> Vec<Recommendation> {
        Vec::new()
    }
    /// Text next to the menu-bar icon. The first tool that returns one wins.
    fn tray_title(&self) -> Option<String> {
        None
    }

    // ---- background
    /// Called on every frame, and also while every shell window is hidden
    /// (then without a ui pass): poll background work, global shortcuts and
    /// so on. Return true while the tool needs frames with a ui pass even
    /// though nothing of the shell is showing, e.g. for its own windows.
    fn tick(&mut self, _ctx: &egui::Context) -> bool {
        false
    }
    /// The user turned the tool on or off in Settings (also called at launch
    /// for tools left off). Off, `tick` and `windows` aren't called; release
    /// anything system-wide, such as global shortcuts.
    fn set_on(&mut self, _on: bool) {}
    /// Called on every frame with a ui pass: show the tool's own extra
    /// windows (`ctx.show_viewport_immediate`), whatever the shell shows.
    fn windows(&mut self, _ctx: &egui::Context, _cx: &mut Cx) {}
    /// Handle a `--view` name for screenshots: load sample data and say what to show.
    fn demo_view(&mut self, _view: &str) -> Option<View> {
        None
    }
}

// ---------------------------------------------------------------- failures

/// Tools that panicked this run, in order, and how many of them the shell
/// has already turned off.
static BROKEN: std::sync::Mutex<(Vec<ToolId>, usize)> = std::sync::Mutex::new((Vec::new(), 0));

fn broken_list() -> std::sync::MutexGuard<'static, (Vec<ToolId>, usize)> {
    BROKEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether tool `id` panicked this run (it stays stopped until a restart).
pub fn is_broken(id: ToolId) -> bool {
    broken_list().0.contains(&id)
}

/// Run a piece of tool `id`'s code on the shell's thread. A panic inside
/// stops that tool for the rest of the run (None here, a "Stopped" tile in
/// its places) instead of taking the whole app down; the panic is still
/// logged and reported like any other. A stopped tool's code isn't run again.
pub fn guard<R>(id: ToolId, f: impl FnOnce() -> R) -> Option<R> {
    if is_broken(id) {
        return None;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("[tool] {id} stopped after a panic");
            stop(id);
            None
        }
    }
}

/// Mark tool `id` stopped, as a panic would.
pub fn stop(id: ToolId) {
    let mut b = broken_list();
    if !b.0.contains(&id) {
        b.0.push(id);
    }
}

/// Tools that broke since the last call, for the shell to switch off
/// (global shortcuts and the like released).
pub fn newly_broken() -> Vec<ToolId> {
    let mut b = broken_list();
    let new = b.0[b.1..].to_vec();
    b.1 = b.0.len();
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panicking_tool_stops_alone() {
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        assert_eq!(super::guard("test-ok", || 7), Some(7));
        let r: Option<()> = super::guard("test-boom", || panic!("boom"));
        std::panic::set_hook(quiet);
        assert!(r.is_none());
        assert!(super::is_broken("test-boom") && !super::is_broken("test-ok"));
        // never run again
        let mut ran = false;
        assert!(super::guard("test-boom", || ran = true).is_none());
        assert!(!ran);
        assert!(super::newly_broken().contains(&"test-boom"));
        assert!(!super::newly_broken().contains(&"test-boom"));
    }

    #[test]
    fn sizes_round_trip() {
        for z in Size::ALL {
            assert_eq!(Size::parse(z.label()), Some(z));
            assert!(z.cols() <= GRID_COLUMNS);
        }
        assert_eq!(Size::parse("3:3"), None);
    }

    #[test]
    fn cx_collects_actions_in_order() {
        let mut cx = Cx::new(Some(("system".into(), "memory".into())));
        assert!(cx.is_detail_open("system", "memory"));
        assert!(!cx.is_detail_open("system", "cpu"));
        assert!(!cx.is_detail_open("claude", "memory"));
        cx.open_page("cleanup", "claude");
        cx.toast("hi");
        cx.toggle_detail("system", "cpu");
        let a = cx.take_actions();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], Action::OpenPage { tool: "cleanup".into(), route: "claude".into() });
        assert_eq!(a[1], Action::Toast("hi".into()));
        assert_eq!(a[2], Action::ShowDetail { tool: "system".into(), detail: "cpu".into(), toggle: true });
        assert!(cx.take_actions().is_empty());
    }
}

/// A model a tool needs, as setup shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct SetupModel {
    pub id: &'static str,
    /// What the tool uses it for ("Speech to text").
    pub what: &'static str,
    /// Which one ("Parakeet v3").
    pub name: &'static str,
    /// About how big the download is.
    pub bytes: u64,
    pub state: ModelState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelState {
    Missing,
    /// Bytes so far and the total (0 until the server says).
    Downloading { done: u64, total: u64 },
    Unpacking,
    Ready,
    Failed(String),
    /// Not needed yet: the tool downloads it the first time it's used.
    OnFirstUse,
}
