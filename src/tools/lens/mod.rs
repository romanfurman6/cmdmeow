//! Disk Map tool: measures a folder tree and shows it as a drill-down
//! treemap; selected items go to the Trash after review. Lives in the full
//! window.

mod history;
mod page;
pub mod scan;

use crate::core::tool::{Badge, Cx, Tool, ToolEnv, ToolId, View};
use crate::kit::glass::*;
use egui::{Rect, Ui};
use std::path::PathBuf;

pub const ID: ToolId = "lens";

pub struct Lens {
    pub(crate) lens: scan::Shared,
    pub(crate) lens_root: PathBuf,
    pub(crate) lens_path: PathBuf,
    /// Zoom animation: (clicked block when zooming in, folder left when
    /// zooming out, start time).
    pub(crate) lens_morph: Option<(Option<egui::Rect>, PathBuf, f64)>,
    pub(crate) lens_selected: Vec<(PathBuf, u64)>,
    /// The folder whose details show beside the rings.
    pub(crate) lens_focus: Option<PathBuf>,
    /// Inside "Other": how many of the folder's biggest items are hidden
    /// (0 = the folder itself).
    pub(crate) lens_skip: usize,
    pub(crate) lens_review: bool,
    pub(crate) lens_msg: Option<String>,
    /// The folder list while mapping, refreshed a few times a second:
    /// (when, root, rows).
    pub(crate) lens_live: Option<(f64, PathBuf, Vec<scan::LiveRow>)>,
    /// Row order of the live list; re-sorted by size only now and then so
    /// rows don't keep swapping places.
    pub(crate) lens_order: Vec<PathBuf>,
    pub(crate) lens_order_at: f64,
    /// Where each live row and its bar were drawn last frame: (row, bar fill).
    pub(crate) lens_rows: std::collections::HashMap<PathBuf, (egui::Rect, f32)>,
    /// Top-level folders in the order they got their colour, so a folder
    /// keeps its colour while the map grows.
    pub(crate) lens_hues: Vec<PathBuf>,
    /// How the map is shown: 0 bubbles, 1 rings around the folder, 2 the
    /// biggest files as a list.
    pub(crate) lens_view: u8,
    /// Folders visited before / after the shown one (Back, Forward).
    pub(crate) lens_back: Vec<PathBuf>,
    pub(crate) lens_fwd: Vec<PathBuf>,
    /// What was under the pointer on the map last frame.
    pub(crate) lens_hover: Option<PathBuf>,
    /// What the right-click menu on the map is for: (path, bytes).
    pub(crate) lens_menu: Option<(PathBuf, u64)>,
    /// Browsing a finished folder while the rest is still being measured.
    pub(crate) lens_peek: bool,
    /// A folder chosen in the folder dialog, waiting to be mapped.
    pub(crate) lens_pick: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    /// Items moved to the Trash while mapping, to take off the finished map.
    pub(crate) lens_trashed: Vec<PathBuf>,
    /// The sheet of everything in the folder is open ("Other").
    pub(crate) lens_other: bool,
    pub(crate) ai: crate::kit::aicheck::Check,
    /// The list row the keyboard is on.
    pub(crate) lens_cursor: Option<usize>,
    /// Bytes moved to the Trash from the map since the app started.
    pub(crate) lens_freed: u64,
    /// Review: the first click on Move to Trash with app data picked.
    pub(crate) lens_confirm: bool,
    /// A menu action picked in a view's own list (files, the Other sheet).
    pub(crate) lens_menu_act: Option<(page::MenuAct, PathBuf, u64)>,
    /// Automatic updates while the page is open: when the disk last changed,
    /// and when the map was last brought up to date.
    pub(crate) lens_dirty: Option<f64>,
    pub(crate) lens_auto_at: f64,
    /// Folders mapped lately (the switcher offers them again).
    pub(crate) lens_recent: Vec<PathBuf>,
}

/// Disk Map choices kept between runs.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Prefs {
    /// Before there were three views: rings or bubbles.
    #[serde(default)]
    rings: bool,
    #[serde(default)]
    view: Option<u8>,
    /// Folders mapped lately, newest first.
    #[serde(default)]
    recent: Vec<PathBuf>,
}

fn prefs_path() -> PathBuf {
    crate::kit::fs::data_path("diskmap-prefs.json")
}

pub(crate) fn save_prefs(app: &Lens) {
    crate::kit::fs::save_json_later(
        &prefs_path(),
        &Prefs {
            rings: app.lens_view == 1,
            view: Some(app.lens_view),
            recent: app.lens_recent.clone(),
        },
        false,
    );
}

impl Lens {
    pub fn new(env: &ToolEnv) -> Self {
        let root = PathBuf::from("/");
        let prefs = crate::kit::fs::load_json::<Prefs>(&prefs_path());
        Lens {
            lens: Default::default(),
            lens_root: root.clone(),
            lens_path: root,
            lens_morph: None,
            lens_selected: vec![],
            lens_focus: None,
            lens_skip: 0,
            lens_review: false,
            lens_msg: None,
            lens_live: None,
            lens_order: vec![],
            lens_order_at: 0.0,
            lens_rows: Default::default(),
            lens_hues: vec![],
            lens_view: prefs.view.unwrap_or(prefs.rings as u8).min(2),
            lens_recent: prefs.recent,
            lens_back: vec![],
            lens_fwd: vec![],
            lens_hover: None,
            lens_menu: None,
            lens_peek: false,
            lens_pick: Default::default(),
            lens_trashed: vec![],
            lens_other: false,
            ai: {
                let mut c = crate::kit::aicheck::Check::new(ID, env.demo);
                c.locate = true;
                c
            },
            lens_cursor: None,
            lens_freed: 0,
            lens_confirm: false,
            lens_menu_act: None,
            lens_dirty: None,
            lens_auto_at: 0.0,
        }
    }
}

impl Tool for Lens {
    fn id(&self) -> ToolId {
        ID
    }
    fn name(&self) -> &'static str {
        "Disk Map"
    }
    fn about(&self) -> &'static str {
        "Shows which folders and files take up your disk"
    }
    fn busy(&self) -> bool {
        self.lens.running.load(std::sync::atomic::Ordering::SeqCst)
            || self.ai.phase() == crate::kit::aicheck::Phase::Running
    }
    fn badge(&self) -> Badge {
        Badge {
            icon: Icon::Map,
            color: tint::DISK_MAP(),
        }
    }
    fn needs(&self) -> Vec<crate::kit::perms::Need> {
        use crate::kit::perms::{Need, Perm};
        vec![Need::must(Perm::FullDisk)]
    }

    fn section(&self) -> &'static str {
        "Clean"
    }
    fn has_page(&self) -> bool {
        true
    }
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
        page::page(self, ui, rect, cx);
    }
    fn demo_view(&mut self, view: &str) -> Option<View> {
        if view == "lens"
            || view == "sizes"
            || view == "lens-rings"
            || view == "lens-files"
            || view == "lens-ai-shown"
        {
            page::demo_tree(self);
            {
                use crate::kit::aicheck::Check;
                self.ai.set_demo(
                    "Two big items you can drop",
                    vec![
                        Check::demo_rec_for(
                            "/Users/me/Library/Caches",
                            "remove",
                            "Remove",
                            "Clear app caches",
                            "34.0 GB",
                            "Apps make them again when they need them",
                            "Caches",
                        ),
                        Check::demo_rec_for(
                            "/Users/me/Downloads/old-movies.zip",
                            "remove",
                            "Remove",
                            "Remove old-movies.zip",
                            "9.4 GB",
                            "The same videos are in Movies",
                            "Downloads",
                        ),
                        Check::demo_rec_for(
                            "/Users/me/Pictures/Photos Library",
                            "reveal",
                            "Show in Finder",
                            "Optimize Photos storage",
                            "21.0 GB",
                            "Originals stay in iCloud",
                            "Photos",
                        ),
                    ],
                );
            }
            if view == "lens-ai-shown" {
                self.lens_path = self.lens_root.join("Library");
                self.lens_focus = None;
                self.ai.demo_show("/Users/me/Library/Caches");
            }
            self.lens_view = match view {
                "lens-rings" => 1,
                "lens-files" => 2,
                _ => 0,
            };
            if view == "sizes" {
                return None;
            }
            Some(View::Page)
        } else if view == "lens-intro" {
            Some(View::Page)
        } else if view == "lens-partial" {
            page::demo_tree(self);
            self.lens
                .partial
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.lens_selected.clear();
            Some(View::Page)
        } else if view == "lens-none" {
            page::demo_tree(self);
            self.lens_selected.clear();
            self.lens_focus = None;
            Some(View::Page)
        } else if view == "lens-live" {
            page::demo_live(self);
            Some(View::Page)
        } else if view == "lens-peek" {
            page::demo_live(self);
            self.lens_peek = true;
            self.lens_path = self.lens_root.join("Projects");
            Some(View::Page)
        } else {
            None
        }
    }
}
