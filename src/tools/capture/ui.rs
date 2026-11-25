//! Capture in the shell: the quick-action tile and card in the panel, and the
//! History and Settings page in the full window.

use super::icons::{self, Ico};
use super::settings::{After, Command, Format, Settings, KEEP_MAX, KEEP_MIN};
use super::folder::{self, FileShot};
use super::store::{Kind, Shot};
use super::{mac, record, Capture, Tab, VTab, ID, SHARING_ID, VIDEO_ID};
use crate::core::tool::{Cx, Size};
use crate::kit::tile;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use crate::kit::perms::Perm;
use std::path::PathBuf;
use std::time::Instant;

const ROW_H: f32 = sp::SETTINGS_ROW_H;

pub fn command_icon(c: Command) -> Ico {
    match c {
        Command::Area => Ico::Area,
        Command::Window => Ico::Window,
        Command::Screen => Ico::Screen,
        Command::Delayed => Ico::Timer,
        Command::Text => Ico::ScanText,
        Command::Color => Ico::Eyedropper,
        Command::Ruler => Ico::Ruler,
        Command::History => Ico::History,
        Command::Record => Ico::Record,
    }
}

fn key_label(c: &Capture, cmd: Command) -> String {
    c.settings.shortcut(cmd).map(|s| s.display()).unwrap_or_default()
}

// ---------------------------------------------------------------- panel

/// The Screenshot widget's actions: an area, a window or the whole screen.
fn modes_list() -> [(Command, &'static str); 3] {
    [(Command::Area, ts!("cap.mode.area")), (Command::Window, ts!("cap.mode.window")), (Command::Screen, ts!("cap.mode.screen"))]
}
/// The shortcuts each tool's settings list.
const SHOT_KEYS: [Command; 8] = [Command::Area, Command::Window, Command::Screen, Command::Delayed, Command::Text, Command::Color, Command::Ruler, Command::History];
const VIDEO_KEYS: [Command; 1] = [Command::Record];

/// The Capture widget at `size`.
pub fn widget(c: &mut Capture, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let id = ui.scope_id().with("w-capture");
    match size {
        Size::S11 => {
            let sub = if c.busy { ts!("cap.capturing").to_string() } else { key_label(c, Command::Area) };
            let sub = if sub.is_empty() { ts!("cap.mode.area").to_string() } else { sub };
            if tile::small(ui, rect, id, Icon::Crop, tint::CAPTURE(), ts!("cap.screenshot"), &sub, false).on_hover_text(ts!("cap.capture_an_area")).clicked() {
                c.queue.push((Command::Area, c.settings.delay));
            }
        }
        Size::S12 | Size::S14 => {
            // W5: the actions alone, no header; the name is in each tip
            let _ = tile::frame(ui, rect, id, false);
            let p = ui.painter().clone();
            let all = modes_list();
            let list: &[(Command, &str)] = if size == Size::S12 { &all[..2] } else { &all };
            modes(c, ui, &p, id, rect.shrink(4.0), list, true);
        }
        _ => card(c, ui, rect, cx),
    }
}

/// Capture actions side by side in `r` (W5): icon, name and (`keys`) the
/// shortcut, no frames, a hairline between.
fn modes(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, id: egui::Id, r: Rect, list: &[(Command, &str)], keys: bool) {
    let live = c.rec.tray();
    let rows: Vec<(String, String, Color32, String)> = list
        .iter()
        .map(|(cmd, label)| {
            let key = key_label(c, *cmd);
            match (&live, cmd) {
                // while recording, Record is the stop button with the time
                (Some(t), Command::Record) => (t.trim_start_matches(['●', '❚', ' ']).to_string(), String::new(), RED(), ts!("cap.stop_save").to_string()),
                // Record is the one red glyph, like a camera's button
                _ => {
                    let tip = if key.is_empty() { label.to_string() } else { format!("{label} · {key}") };
                    (label.to_string(), if keys { key } else { String::new() }, if *cmd == Command::Record { RED() } else { TEXT() }, tip)
                }
            }
        })
        .collect();
    let acts: Vec<tile::Action> = rows.iter().map(|(l, s, col, tip)| tile::Action { label: l, sub: s, color: *col, tip }).collect();
    if let Some(k) = tile::actions(ui, p, id, r, &acts, |p, k, gr, col| icons::draw(p, command_icon(list[k].0), gr, col)) {
        let cmd = list[k].0;
        let delay = if matches!(cmd, Command::Text | Command::Record) { 0 } else { c.settings.delay };
        c.queue.push((cmd, delay));
    }
}

/// The Medium widget: the three capture buttons and the latest captures.
pub fn card(c: &mut Capture, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    // thumbnails load on other threads: take the finished ones here too, so
    // a copy of the widget that isn't ticked (setup's preview) shows them
    c.drain_done();
    let p = ui.painter().clone();
    let id = ui.scope_id().with("capture-card");
    let _ = tile::frame(ui, rect, id.with("frame"), false);
    let (x0, x1, mut y) = card_head(ui, &p, rect, Icon::Crop, ts!("cap.screenshot"));
    if link(ui, Pos2::new(x1, y - sp::M - 10.0), Align2::RIGHT_CENTER, ts!("cap.history"), 12.0).clicked() {
        c.tab = Tab::History;
        cx.open_page(ID, "history");
    }
    modes(c, ui, &p, id, Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + MODE_H)), &modes_list(), false);
    y += MODE_H + sp::M;
    // the newest images in the screenshot folder; history when it has none
    // (or before macOS was asked about it)
    c.refresh_files(true);
    // pictures only: recordings in the same folder are the Video widget's
    let items: Vec<Item> = if c.files.is_empty() { c.store.shots.iter().filter(|s| s.kind != Kind::Recording).take(4).map(Item::from_shot).collect() } else { c.files.iter().filter(|f| !folder::is_video(&f.path)).take(4).map(|f| Item::from_file(c, f)).collect() };
    let row = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, rect.bottom() - sp::M));
    recent_row(c, ui, &p, id, row, &items, 4, Act::Edit, ts!("cap.click_edit"));
}

/// Height of the capture buttons in the Medium widgets.
const MODE_H: f32 = 36.0;

/// The Medium widgets' header: the tool's chip and name. Returns the
/// content's left and right edges and where the content starts.
fn card_head(_ui: &Ui, p: &egui::Painter, rect: Rect, glyph: Icon, name: &str) -> (f32, f32, f32) {
    let x0 = rect.left() + sp::M;
    let x1 = rect.right() - sp::M;
    let y = rect.top() + sp::M;
    tool_chip(p, Rect::from_min_size(Pos2::new(x0, y), Vec2::splat(20.0)), glyph, tint::CAPTURE());
    p.text(Pos2::new(x0 + 28.0, y + 10.0), Align2::LEFT_CENTER, name, bold(13.0), TEXT());
    (x0, x1, y + 20.0 + sp::M)
}

/// The latest captures in one row of `n` frameless thumbnails filling `r`
/// (recordings show their length); faint plates when there are none.
#[allow(clippy::too_many_arguments)]
fn recent_row(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, id: egui::Id, r: Rect, items: &[Item], n: usize, act: Act, tip: &str) {
    let gap = sp::S;
    let bw = (r.width() - (n as f32 - 1.0) * gap) / n as f32;
    for k in 0..n {
        let tr = Rect::from_min_size(Pos2::new(r.left() + k as f32 * (bw + gap), r.top()), Vec2::new(bw, r.height()));
        let Some(it) = items.get(k) else {
            p.rect_filled(tr, 8.0, white(10));
            continue;
        };
        let resp = ui.interact(tr, id.with(("recent", it.key)), Sense::click());
        let tex = match it.src {
            Src::Shot(sid) => c.thumb(sid),
            Src::File if it.path.as_os_str().is_empty() => None,
            Src::File => c.thumb_at(it.key, it.path.clone()),
        };
        thumb(p, tr, tex, (it.w, it.h), 8.0);
        if it.video.is_some() || it.meta.starts_with(ts!("cap.video")) {
            // a play mark in the middle and the length in the corner
            p.circle_filled(tr.center(), 11.0, Color32::from_black_alpha(115));
            let c0 = tr.center();
            p.add(egui::Shape::convex_polygon(vec![c0 + Vec2::new(-3.0, -5.0), c0 + Vec2::new(5.5, 0.0), c0 + Vec2::new(-3.0, 5.0)], Color32::WHITE, Stroke::NONE));
            let prefix = format!("{} · ", ts!("cap.video"));
            let len = it.meta.trim_start_matches(prefix.as_str());
            let g = p.layout_no_wrap(len.to_string(), bold(11.0), Color32::WHITE);
            let lr = Rect::from_min_size(Pos2::new(tr.right() - 5.0 - g.size().x - 8.0, tr.bottom() - 5.0 - g.size().y - 2.0), g.size() + Vec2::new(8.0, 2.0));
            p.rect_filled(lr, 4.0, Color32::from_black_alpha(140));
            p.galley(lr.min + Vec2::new(4.0, 1.0), g, Color32::WHITE);
        }
        // W12: no ring on hover inside the widget, just the hand
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.on_hover_text(format!("{} · {tip}", it.title)).clicked() {
            run_act(c, act, it);
        }
    }
}

/// A capture's thumbnail filling `r` (cropped to fit, top aligned), or a quiet
/// plate while it loads.
pub fn thumb(p: &egui::Painter, r: Rect, tex: Option<egui::TextureId>, size: (u32, u32), radius: f32) {
    let Some(t) = tex else {
        p.rect_filled(r, radius, white(12));
        return;
    };
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let img_aspect = w / h;
    let box_aspect = r.width() / r.height().max(1.0);
    let uv = if img_aspect > box_aspect {
        let f = box_aspect / img_aspect;
        Rect::from_min_max(Pos2::new((1.0 - f) / 2.0, 0.0), Pos2::new((1.0 + f) / 2.0, 1.0))
    } else {
        let f = img_aspect / box_aspect;
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, f))
    };
    p.add(egui::epaint::RectShape::filled(r, radius, Color32::WHITE).with_texture(t, uv));
}

// ---------------------------------------------------------------- page

/// Nothing captured yet (recordings live on the Video page).
fn no_shots(c: &Capture) -> bool {
    only_recordings(&c.store.shots)
}

fn only_recordings(shots: &[Shot]) -> bool {
    shots.iter().all(|s| s.kind == Kind::Recording)
}

pub fn page(c: &mut Capture, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let first = c.tab == Tab::History && no_shots(c);
    let sub = match c.tab {
        Tab::History if first => t!("cap.saving_to", folder = c.shown_folder()).into_owned(),
        Tab::History => {
            let n = c.store.shots.iter().filter(|s| s.kind != Kind::Recording).count();
            tn!("cap.hist_sub", n = n, size = human_bytes(c.store.bytes_on_disk()), folder = c.shown_folder()).into_owned()
        }
        Tab::Folder => {
            c.refresh_files(false);
            let n = c.files.len();
            let more = if n >= super::folder::LIMIT { ts!("cap.newest") } else { "" };
            tn!("cap.folder_sub", n = n, more = more, folder = c.shown_folder()).into_owned()
        }
        Tab::Settings => "".into(),
    };
    // SP16: Capture area at the right edge on every tab, the tabs next to it
    let label = ts!("cap.sc.area");
    let bw = icon_pill_width(&p, label, BTN_PRIMARY);
    let btns = page_header_room(&p, r, ts!("cap.screenshot"), &sub, TEXT_DIM(), 0.0, r.right() - sp::PAGE_EDGE - bw - sp::M - 246.0 - sp::M).0;
    let (tr, br) = header_controls(btns, 246.0, bw);
    let (resp, face) = pill_button_at(ui, br, "", BTN_PRIMARY, !c.busy);
    if resp.clicked() {
        c.queue.push((Command::Area, 0));
    }
    let ink = if c.busy { with_alpha(ON_ACTION, crate::kit::key::disabled_alpha()) } else { ON_ACTION };
    icons::draw(&p, Ico::Area, Rect::from_center_size(Pos2::new(face.left() + 18.0, face.center().y), Vec2::splat(15.0)), ink);
    p.text(Pos2::new(face.left() + 31.0, face.center().y), Align2::LEFT_CENTER, label, bold(13.0), ink);
    let tabs = [Tab::History, Tab::Folder, Tab::Settings];
    let cur = tabs.iter().position(|t| *t == c.tab).unwrap_or(0);
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("capture-tab"), &["History", "Folder", "Settings"], cur) {
        c.tab = tabs[i];
        stop_recording(c);
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    match c.tab {
        Tab::History => history(c, ui, body),
        Tab::Folder => folder_tab(c, ui, body),
        Tab::Settings => settings_page(c, ui, body, cx),
    }
}

fn filters() -> [&'static str; 5] {
    [ts!("cap.f.all"), ts!("cap.mode.area"), ts!("cap.mode.window"), ts!("cap.mode.screen"), ts!("cap.f.pinned")]
}

fn passes(filter: usize, s: &Shot) -> bool {
    if s.kind == Kind::Recording {
        // recordings live on the Video page
        return false;
    }
    match filter {
        1 => s.kind == Kind::Area || s.kind == Kind::Edited,
        2 => s.kind == Kind::Window,
        3 => s.kind == Kind::Screen,
        4 => s.pinned,
        _ => true,
    }
}

fn day_label(d: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    if d == today {
        ts!("cap.today").into()
    } else if Some(d) == today.pred_opt() {
        ts!("cap.yesterday").into()
    } else {
        crate::kit::date::day_title(&d)
    }
}

/// One picture in a grid: a history capture or an image in the folder.
#[derive(Clone)]
pub struct Item {
    key: u64,
    path: PathBuf,
    time: chrono::DateTime<chrono::Local>,
    w: u32,
    h: u32,
    title: String,
    meta: String,
    pinned: bool,
    src: Src,
    /// A recording: the video file (the thumbnail comes from `path` or history).
    video: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
enum Src {
    Shot(u64),
    File,
}

impl Item {
    fn from_shot(s: &Shot) -> Item {
        // one kind of fact under every picture: its size (the kind is the
        // filter above the grid)
        let meta = match s.kind {
            Kind::Window | Kind::Screen | Kind::Edited | Kind::Area => format!("{}×{}", s.w, s.h),
            Kind::Recording => t!("cap.video_len", len = record::timeline::clock_label(s.secs.unwrap_or(0.0) as f64)).into_owned(),
        };
        let recording = s.kind == Kind::Recording;
        let video = (s.kind == Kind::Recording).then(|| s.saved.clone()).flatten();
        // history files live in the app's data folder, next to index.json
        Item { key: s.id, path: PathBuf::new(), time: s.time(), w: s.w, h: s.h, title: if recording { t!("cap.recording_from", time = crate::kit::date::hm12(&s.time())).into_owned() } else { t!("cap.capture_from", time = crate::kit::date::hm12(&s.time())).into_owned() }, meta, pinned: s.pinned, src: Src::Shot(s.id), video }
    }
    /// An image in the folder, or a video (its picture comes from the
    /// history entry it was saved from, when there is one).
    fn from_file(c: &Capture, f: &FileShot) -> Item {
        if folder::is_video(&f.path) {
            let shot = c.store.video_entry(&f.path);
            let (path, w, h) = match shot {
                Some(s) => (c.store.path(s.id), s.w, s.h),
                None => (PathBuf::new(), 16, 9),
            };
            let meta = shot.and_then(|s| s.secs).map(|t| t!("cap.video_len", len = record::timeline::clock_label(t as f64)).into_owned()).unwrap_or_else(|| ts!("cap.video").into());
            return Item { key: f.key, path, time: f.time(), w, h, title: f.name(), meta, pinned: false, src: Src::File, video: Some(f.path.clone()) };
        }
        Item { key: f.key, path: f.path.clone(), time: f.time(), w: f.w, h: f.h, title: f.name(), meta: if f.w > 0 { format!("{}×{}", f.w, f.h) } else { String::new() }, pinned: false, src: Src::File, video: None }
    }
}

/// What a tile's buttons (or a click) ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Copy,
    Edit,
    /// Upload (or reuse the last upload) and copy the link.
    Link,
    Pin,
    Reveal,
    Delete,
}

fn load(c: &Capture, it: &Item) -> Option<image::RgbaImage> {
    match it.src {
        Src::Shot(id) => c.store.load(id),
        Src::File => image::open(&it.path).ok().map(|i| i.to_rgba8()),
    }
}

fn run_act(c: &mut Capture, act: Act, it: &Item) {
    if let Some(v) = &it.video {
        match act {
            Act::Edit => c.open_video(v.clone()),
            Act::Copy => c.hud(if record::sys::copy_file(v) { ts!("cap.copied_video") } else { ts!("cap.cant_copy") }),
            Act::Reveal => mac::reveal(v),
            Act::Pin => c.hud(ts!("cap.no_pin_video")),
            Act::Link => c.copy_link(v.clone()),
            Act::Delete => {}
        }
        if act != Act::Delete {
            return;
        }
    }
    match (act, &it.src) {
        // a history capture is a PNG already: onto the clipboard as it is
        (Act::Copy, Src::Shot(id)) => match c.store.png(*id) {
            Some(png) => {
                c.copy_png(png);
                c.hud(ts!("cap.copied"));
            }
            None => c.hud(ts!("cap.cant_copy")),
        },
        (Act::Copy, Src::File) => match load(c, it) {
            Some(img) => {
                c.copy_image(img);
                c.hud(ts!("cap.copied"));
            }
            None => c.hud(ts!("cap.cant_copy")),
        },
        (Act::Edit, Src::Shot(id)) => c.open_editor_for(*id),
        (Act::Edit, Src::File) => {
            if let Some(f) = c.files.iter().find(|f| f.key == it.key).cloned() {
                c.open_file(&f);
            }
        }
        (Act::Pin, Src::Shot(id)) => c.pin_shot(*id),
        (Act::Pin, Src::File) => match load(c, it) {
            Some(img) => c.pin(None, img),
            None => c.hud(ts!("cap.cant_open_it")),
        },
        (Act::Reveal, Src::Shot(id)) => {
            let id = *id;
            let saved = c.store.get(id).and_then(|s| s.saved.clone()).filter(|p| p.exists());
            let path = match saved {
                Some(p) => Some(p),
                None => c.store.load(id).and_then(|img| c.save_image(Some(id), &img)),
            };
            if let Some(p) = path {
                mac::reveal(&p);
            }
        }
        (Act::Reveal, Src::File) => mac::reveal(&it.path),
        (Act::Link, Src::Shot(id)) => {
            let path = c.store.path(*id);
            c.copy_link(path);
        }
        (Act::Link, Src::File) => c.copy_link(it.path.clone()),
        (Act::Delete, Src::Shot(id)) => {
            c.store.remove(*id);
            c.forget_dropped_thumbs();
        }
        (Act::Delete, Src::File) => match crate::kit::native::trash_item(&it.path) {
            Ok(()) => {
                c.files.retain(|f| f.key != it.key);
                c.forget_dropped_thumbs();
                c.hud(ts!("cap.moved_trash"));
            }
            Err(e) => c.hud(t!("cap.trash_fail", e = e).into_owned()),
        },
    }
}

/// Pictures grouped by day, five to a row, with buttons on hover.
fn grid(c: &mut Capture, ui: &mut Ui, list: Rect, items: &[Item], salt: &str) -> Vec<(Act, Item)> {
    let mut days: Vec<(chrono::NaiveDate, Vec<&Item>)> = Vec::new();
    for it in items {
        let d = it.time.date_naive();
        match days.last_mut() {
            Some((day, v)) if *day == d => v.push(it),
            _ => days.push((d, vec![it])),
        }
    }
    let x0 = list.left() + 24.0;
    let cols = 5usize;
    let gap = 12.0;
    let cw = (list.width() - 48.0 - gap * (cols as f32 - 1.0)) / cols as f32;
    let th = 112.0;
    let cell_h = th + 26.0;
    let mut acts = Vec::new();
    let mut child = ui.new_child(UiBuilder::new().max_rect(list));
    ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(&mut child, |ui| {
        let total: f32 = days.iter().map(|(_, v)| sp::SECTION_H + v.len().div_ceil(cols) as f32 * (cell_h + gap)).sum::<f32>() + 8.0;
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total), Sense::hover());
        let p = ui.painter().clone();
        let mut y = area.top();
        for (d, v) in &days {
            section_title(&p, x0, y, &day_label(*d));
            y += sp::SECTION_H;
            for (i, it) in v.iter().enumerate() {
                let (row, col) = (i / cols, i % cols);
                let r = Rect::from_min_size(Pos2::new(x0 + col as f32 * (cw + gap), y + row as f32 * (cell_h + gap)), Vec2::new(cw, th));
                if !ui.is_rect_visible(r.expand(8.0)) {
                    continue;
                }
                let tex = match it.src {
                    Src::Shot(id) => c.thumb(id),
                    Src::File if it.path.as_os_str().is_empty() => None,
                    Src::File => c.thumb_at(it.key, it.path.clone()),
                };
                if let Some(a) = tile_ui(ui, &p, r, it, tex) {
                    acts.push((a, (*it).clone()));
                }
            }
            y += v.len().div_ceil(cols) as f32 * (cell_h + gap);
        }
    });
    acts
}

fn tile_ui(ui: &mut Ui, p: &egui::Painter, r: Rect, it: &Item, tex: Option<egui::TextureId>) -> Option<Act> {
    let id = ui.scope_id().with(("pic", it.key));
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(&it.title);
    let hovered = ui.rect_contains_pointer(r);
    thumb(p, r, tex, (it.w, it.h), 8.0);
    p.rect_stroke(r, 8.0, if hovered { Stroke::new(2.0, ACCENT()) } else { Stroke::new(1.0, white(26)) }, StrokeKind::Inside);
    if it.video.is_some() {
        p.circle_filled(r.center(), 18.0, Color32::from_black_alpha(170));
        icons::draw(p, Ico::Play, Rect::from_center_size(r.center() + Vec2::new(1.5, 0.0), Vec2::splat(16.0)), Color32::WHITE);
    }
    if it.pinned {
        let b = Rect::from_min_size(r.min + Vec2::new(6.0, 6.0), Vec2::splat(20.0));
        p.circle_filled(b.center(), 10.0, Color32::from_black_alpha(160));
        icons::draw(p, Ico::Pin, b.shrink(4.0), Color32::WHITE);
    }
    let ly = r.bottom() + 12.0;
    let mr = p.text(Pos2::new(r.right(), ly), Align2::RIGHT_CENTER, &it.meta, font(12.0), TEXT_FAINT());
    text_fit(p, Pos2::new(r.left(), ly), Align2::LEFT_CENTER, &crate::kit::date::hm12(&it.time), font(12.0), TEXT(), mr.left() - r.left() - 6.0);
    let mut out = None;
    if hovered {
        let bar = Rect::from_min_max(Pos2::new(r.left() + 1.0, r.bottom() - 38.0), Pos2::new(r.right() - 1.0, r.bottom() - 1.0));
        p.rect_filled(bar, egui::CornerRadius { nw: 0, ne: 0, sw: 7, se: 7 }, Color32::from_rgba_unmultiplied(20, 20, 22, 225));
        let delete_tip = if it.src == Src::File { ts!("cap.move_to_trash") } else { ts!("cap.delete") };
        let video = it.video.is_some();
        let mut acts: Vec<(Ico, &str, Act)> = vec![
            (Ico::Copy, ts!("cap.copy"), Act::Copy),
            (if video { Ico::Scissors } else { Ico::Edit }, if video { ts!("cap.play_edit") } else { ts!("cap.edit") }, Act::Edit),
            (Ico::Link, ts!("cap.copy_link"), Act::Link),
        ];
        if !video {
            acts.push((Ico::Pin, ts!("cap.pin_on_screen"), Act::Pin));
        }
        acts.extend([(Ico::Folder, ts!("cap.show_in_finder"), Act::Reveal), (Ico::Trash, delete_tip, Act::Delete)]);
        let gap = 3.0;
        let bw = ((bar.width() - 12.0 - (acts.len() - 1) as f32 * gap) / acts.len() as f32).min(26.0);
        let total = acts.len() as f32 * bw + (acts.len() - 1) as f32 * gap;
        let mut x = bar.center().x - total / 2.0;
        for (ico, tip, a) in acts {
            let br = Rect::from_min_size(Pos2::new(x, bar.center().y - 13.0), Vec2::splat(bw));
            let b = ui.interact(br, id.with(tip), Sense::click());
            p.rect_filled(br, 6.0, white(if b.hovered() { 46 } else { 26 }));
            icons::draw(p, ico, br.shrink((bw * 0.23).min(6.0)), if a == Act::Delete && b.hovered() { RED() } else { TEXT() });
            if b.on_hover_text(tip).clicked() {
                out = Some(a);
            }
            x += bw + gap;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if out.is_none() && resp.clicked() {
        out = Some(Act::Edit);
    }
    out
}

fn history(c: &mut Capture, ui: &mut Ui, body: Rect) {
    if no_shots(c) {
        first_capture(c, ui, body);
        return;
    }
    let p = ui.painter().clone();
    let x0 = body.left() + 24.0;
    let x1 = body.right() - 24.0;
    let fy = body.top() + 12.0;
    let mut x = x0;
    for (i, f) in filters().iter().enumerate() {
        let g = p.layout_no_wrap(f.to_string(), font(12.0), TEXT());
        let r = Rect::from_min_size(Pos2::new(x, fy), Vec2::new(g.size().x + 24.0, 26.0));
        let resp = ui.interact(r, ui.scope_id().with(("filter", i)), Sense::click());
        let on = c.filter == i;
        p.rect_filled(r, 13.0, if on { with_alpha(ACTION(), 0.22) } else if resp.hovered() { white(10) } else { Color32::TRANSPARENT });
        p.rect_stroke(r, 13.0, Stroke::new(1.0, if on { with_alpha(ACCENT(), 0.45) } else { white(26) }), StrokeKind::Inside);
        p.text(r.center(), Align2::CENTER_CENTER, *f, font(12.0), if on { TEXT() } else { TEXT_DIM() });
        if resp.clicked() {
            c.filter = i;
        }
        x = r.right() + 6.0;
    }
    let lr = link(ui, Pos2::new(x1, fy + 13.0), Align2::RIGHT_CENTER, ts!("cap.change"), 12.0);
    if lr.clicked() {
        c.tab = Tab::Settings;
    }
    p.text(Pos2::new(lr.rect.left(), fy + 13.0), Align2::RIGHT_CENTER, t!("cap.keeps_last", n = c.settings.keep), font(12.0), TEXT_FAINT());

    let list = Rect::from_min_max(Pos2::new(body.left(), fy + 38.0), body.max);
    let items: Vec<Item> = c.store.shots.iter().filter(|s| passes(c.filter, s)).map(Item::from_shot).collect();
    if items.is_empty() {
        p.text(list.center() - Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, ts!("cap.empty_filter"), bold(15.0), TEXT_SOFT());
        return;
    }
    for (a, it) in grid(c, ui, list, &items, "capture-history") {
        run_act(c, a, &it);
    }
}

/// Before the first screenshot: a dragged selection, one big Capture area
/// and the other two ways under it, each with its shortcut. Like Cleanup
/// before its first scan.
fn first_capture(c: &mut Capture, ui: &mut Ui, body: Rect) {
    use crate::kit::empty::{hero_button, side_button, side_width, tile, Lead, HERO_H, TILE};
    let p = ui.painter().clone();
    let id = ui.scope_id().with("capture-first");
    let tint = tint::CAPTURE();
    let block = TILE.y + sp::XXL + HERO_H + sp::L + 34.0;
    let top = body.center().y - block / 2.0 - sp::L;
    let cx0 = body.center().x;

    // the picture: a selection being dragged, with its handles and size
    let tr = Rect::from_center_size(Pos2::new(cx0, top + TILE.y / 2.0), TILE);
    tile(&p, tr, tint);
    let sel = Rect::from_center_size(tr.center(), Vec2::new(100.0, 64.0));
    p.rect_filled(sel, 4.0, with_alpha(tint, 0.10));
    dashed_rect(&p, sel, 4.0, tint);
    for k in [sel.left_top(), sel.right_top(), sel.left_bottom(), sel.right_bottom()] {
        p.rect_filled(Rect::from_center_size(k, Vec2::splat(8.0)), 2.0, tint);
    }
    let g = p.layout_no_wrap("640 × 400".into(), bold(11.0), on_color(tint));
    let br = Rect::from_min_max(Pos2::new(tr.right() + 8.0 - g.size().x - 14.0, tr.bottom() + 10.0 - g.size().y - 6.0), Pos2::new(tr.right() + 8.0, tr.bottom() + 10.0));
    p.rect_filled(br, 6.0, tint);
    p.galley(br.center() - g.size() / 2.0, g, on_color(tint));

    let key = |cmd: Command| c.settings.shortcut(cmd).map(|s| s.display());
    let (area, window, screen) = (key(Command::Area), key(Command::Window), key(Command::Screen));
    let hc = Pos2::new(cx0, tr.bottom() + sp::XXL + HERO_H / 2.0);
    if hero_button(ui, hc, id.with("area"), ts!("cap.ui.capture_area"), area.as_deref(), Lead::None, Some(ACTION()), !c.busy, 200.0).clicked() {
        c.queue.push((Command::Area, 0));
    }
    let sy = hc.y + HERO_H / 2.0 + sp::L + 17.0;
    let (ww, sw) = (side_width(&p, ts!("cap.mode.window"), window.as_deref()), side_width(&p, ts!("cap.mode.screen"), screen.as_deref()));
    let x = cx0 - (ww + sp::S + sw) / 2.0;
    let (r1, _) = side_button(ui, Pos2::new(x, sy), id.with("window"), "Window", window.as_deref(), !c.busy, |p, r| icons::draw(p, Ico::Window, r, tint));
    let (r2, _) = side_button(ui, Pos2::new(x + ww + sp::S, sy), id.with("screen"), "Screen", screen.as_deref(), !c.busy, |p, r| icons::draw(p, Ico::Screen, r, tint));
    if r1.clicked() {
        c.queue.push((Command::Window, 0));
    }
    if r2.clicked() {
        c.queue.push((Command::Screen, 0));
    }
}

/// Before the first recording: a screen with REC on it, one big Record
/// screen, and the two sound switches a first recording most needs.
fn first_recording(c: &mut Capture, ui: &mut Ui, body: Rect) {
    use crate::kit::empty::{hero_button, tile, toggle_chip, toggle_width, Lead, HERO_H, TILE};
    let p = ui.painter().clone();
    let id = ui.scope_id().with("video-first");
    let tint = tint::CAPTURE();
    let t = ui.ctx().input(|i| i.time);
    let block = TILE.y + sp::XXL + HERO_H + sp::L + 34.0;
    let top = body.center().y - block / 2.0 - sp::L;
    let cx0 = body.center().x;

    // the picture: a display with a blinking REC on it
    let tr = Rect::from_center_size(Pos2::new(cx0, top + TILE.y / 2.0), TILE);
    tile(&p, tr, tint);
    let scr = Rect::from_center_size(tr.center() - Vec2::new(0.0, 7.0), Vec2::new(86.0, 52.0));
    p.rect_stroke(scr, 6.0, Stroke::new(2.5, tint), StrokeKind::Middle);
    let foot = scr.bottom() + 12.0;
    p.line_segment([Pos2::new(scr.center().x, scr.bottom()), Pos2::new(scr.center().x, foot)], Stroke::new(2.5, tint));
    p.line_segment([Pos2::new(scr.center().x - 12.0, foot), Pos2::new(scr.center().x + 12.0, foot)], Stroke::new(2.5, tint));
    let g = p.layout_no_wrap(ts!("cap.ui.rec").into(), bold(11.0), RED());
    let rec = Rect::from_center_size(scr.center(), Vec2::new(8.0 + 5.0 + g.size().x + 16.0, 20.0));
    p.rect_filled(rec, 8.0, with_alpha(RED(), 0.18));
    let on = (t * 0.7).fract() < 0.6;
    p.circle_filled(Pos2::new(rec.left() + 8.0 + 4.0, rec.center().y), 4.0, if on { RED() } else { with_alpha(RED(), 0.4) });
    p.galley(Pos2::new(rec.left() + 8.0 + 8.0 + 5.0, rec.center().y - g.size().y / 2.0), g, RED());
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));

    let key = c.settings.shortcut(Command::Record).map(|s| s.display());
    let hc = Pos2::new(cx0, tr.bottom() + sp::XXL + HERO_H / 2.0);
    if hero_button(ui, hc, id.with("record"), ts!("cap.ui.record_screen"), key.as_deref(), Lead::Dot(RED()), Some(ACTION()), true, 220.0).clicked() {
        c.queue.push((Command::Record, 0));
    }
    // sound: what you hear and your voice
    let sy = hc.y + HERO_H / 2.0 + sp::L + 17.0;
    let (aw, mw) = (toggle_width(&p, ts!("cap.system_audio")), toggle_width(&p, ts!("cap.microphone")));
    let x = cx0 - (aw + sp::S + mw) / 2.0;
    let (sys, mic) = (c.settings.rec.system_audio, c.settings.rec.mic);
    let (r1, _) = toggle_chip(ui, Pos2::new(x, sy), id.with("sys"), ts!("cap.system_audio"), sys, |p, r, col| icons::draw(p, Ico::Speaker, r, col));
    let (r2, _) = toggle_chip(ui, Pos2::new(x + aw + sp::S, sy), id.with("mic"), ts!("cap.microphone"), mic, |p, r, col| icons::draw(p, if mic { Ico::Mic } else { Ico::MicOff }, r, col));
    if r1.clicked() {
        c.settings.rec.system_audio = !sys;
        c.save_settings();
    }
    if r2.clicked() {
        c.rec.toggle_mic(&mut c.settings.rec);
        c.save_settings();
    }
}

/// The screenshot folder: every PNG and JPG in it, newest first.
fn folder_tab(c: &mut Capture, ui: &mut Ui, body: Rect) {
    let p = ui.painter().clone();
    let x0 = body.left() + 24.0;
    let x1 = body.right() - 24.0;
    let fy = body.top() + 12.0;
    let cy = fy + 13.0;
    icons::draw(&p, Ico::Folder, Rect::from_center_size(Pos2::new(x0 + 8.0, cy), Vec2::splat(16.0)), TEXT_DIM());
    let ow = button_width(&p, ts!("cap.open_finder"), BTN_SECONDARY) - 6.0;
    let or = Rect::from_min_max(Pos2::new(x1 - ow, cy - 13.0), Pos2::new(x1, cy + 13.0));
    if pill_button(ui, or, ts!("cap.open_finder"), BTN_SECONDARY, true).clicked() {
        let _ = std::process::Command::new("open").arg(&c.settings.folder).spawn();
    }
    let cw = button_width(&p, ts!("cap.change_folder"), BTN_SECONDARY) - 6.0;
    let cr = Rect::from_min_max(Pos2::new(or.left() - 8.0 - cw, cy - 13.0), Pos2::new(or.left() - 8.0, cy + 13.0));
    if pill_button(ui, cr, ts!("cap.change_folder"), BTN_SECONDARY, true).clicked() {
        c.choose_folder(false);
    }
    text_fit(&p, Pos2::new(x0 + 24.0, cy), Align2::LEFT_CENTER, &c.shown_folder(), mono(12.0), TEXT_SOFT(), cr.left() - x0 - 36.0);

    let list = Rect::from_min_max(Pos2::new(body.left(), fy + 38.0), body.max);
    let items: Vec<Item> = c.files.iter().map(|f| Item::from_file(c, f)).collect();
    if items.is_empty() {
        let (msg, hint) = if c.files_busy && c.files_at.is_none() {
            (ts!("cap.looking_folder"), String::new())
        } else if !c.settings.save {
            (ts!("cap.no_images"), ts!("cap.no_images_hint").to_string())
        } else {
            (ts!("cap.no_images"), "".to_string())
        };
        p.text(list.center() - Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, msg, bold(15.0), TEXT_SOFT());
        p.text(list.center() - Vec2::new(0.0, 6.0), Align2::CENTER_CENTER, hint, font(13.0), TEXT_DIM());
        return;
    }
    for (a, it) in grid(c, ui, list, &items, "capture-folder") {
        run_act(c, a, &it);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(3));
}

// ---------------------------------------------------------------- settings

fn stop_recording(c: &mut Capture) {
    if c.recording.take().is_some() {
        c.apply_keys();
    }
}

fn card_rows(p: &egui::Painter, x: f32, y: f32, w: f32, rows: usize) -> Rect {
    let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rows as f32 * ROW_H + 8.0));
    glass_card(p, r, R_CARD, 0.0, false);
    for k in 1..rows {
        let ly = r.top() + 4.0 + k as f32 * ROW_H;
        p.line_segment([Pos2::new(r.left() + 12.0, ly), Pos2::new(r.right() - 12.0, ly)], Stroke::new(1.0, white(13)));
    }
    r
}

/// Recording: frame rate, quality, the countdown, the cursor and its
/// clicks (drawn by the system, so only where it can).
fn recording_options(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, col: Rect) -> f32 {
    use super::settings::{RecQuality, RECORD_FPS};
    #[derive(Clone, Copy, PartialEq)]
    enum Row {
        Fps,
        Quality,
        Countdown,
        Cursor,
        Clicks,
    }
    let clicks = super::record::engine::clicks_supported();
    let rows: Vec<(Row, &str)> = [(Row::Fps, ts!("cap.frame_rate")), (Row::Quality, ts!("cap.quality")), (Row::Countdown, ts!("cap.countdown")), (Row::Cursor, ts!("cap.show_cursor")), (Row::Clicks, ts!("cap.highlight_clicks"))]
        .into_iter()
        .filter(|(r, _)| *r != Row::Clicks || clicks)
        .collect();
    let x0 = col.left();
    crate::kit::glass::card_title(p, Pos2::new(x0, col.top()), ts!("cap.recording"));
    let card = card_rows(p, x0, col.top() + 28.0, col.width(), rows.len());
    let id = ui.scope_id().with("rec-options");
    let mut s = c.settings.rec.clone();
    for (k, (row, label)) in rows.iter().enumerate() {
        let y = row_y(card, k);
        p.text(Pos2::new(card.left() + 14.0, y), Align2::LEFT_CENTER, *label, font(13.0), TEXT());
        let seg = |ui: &mut Ui, w: f32, labels: &[&str], cur: usize| segmented(ui, Rect::from_min_max(Pos2::new(card.right() - 12.0 - w, y - 12.0), Pos2::new(card.right() - 12.0, y + 12.0)), id.with(k), labels, cur);
        let sw = Rect::from_min_size(Pos2::new(card.right() - 12.0 - 30.0, y - 9.0), Vec2::new(30.0, 18.0));
        match row {
            Row::Fps => {
                if let Some(i) = seg(ui, 110.0, &[ts!("cap.fps30"), ts!("cap.fps60")], if s.fps == 30 { 0 } else { 1 }) {
                    s.fps = RECORD_FPS[i];
                }
            }
            Row::Quality => {
                if let Some(i) = seg(ui, 180.0, &[ts!("cap.q1080"), ts!("cap.q4k")], if s.quality == RecQuality::Smaller { 0 } else { 1 }) {
                    s.quality = if i == 0 { RecQuality::Smaller } else { RecQuality::High };
                }
            }
            Row::Countdown => {
                let cd = match s.countdown {
                    0 => 0,
                    5 => 2,
                    _ => 1,
                };
                if let Some(i) = seg(ui, 150.0, &[ts!("cap.off"), ts!("cap.s3"), ts!("cap.s5")], cd) {
                    s.countdown = [0, 3, 5][i];
                }
            }
            Row::Cursor => {
                if switch(ui, sw, id.with(("sw", k)), s.cursor, true).clicked() {
                    s.cursor = !s.cursor;
                }
            }
            Row::Clicks => {
                if switch(ui, sw, id.with(("sw", k)), s.clicks, s.cursor).clicked() {
                    s.clicks = !s.clicks;
                }
            }
        }
    }
    if s != c.settings.rec {
        c.settings.rec = s;
        c.save_settings();
    }
    card.bottom() + 30.0
}

/// "Hide after" choices; past the end means never.
const PREVIEW_SECS: [u32; 3] = [3, 6, 10];
const CARD_SECS: [u32; 3] = [5, 8, 15];

fn secs_index(v: Option<u32>, choices: &[u32]) -> usize {
    match v {
        None => choices.len(),
        Some(s) => choices.iter().position(|&c| c >= s).unwrap_or(choices.len() - 1),
    }
}

/// One line of text input in a settings card (`secret` hides what's typed).
fn field(ui: &mut Ui, r: Rect, text: &mut String, hint: &str, secret: bool) -> egui::Response {
    let id = ui.scope_id().with(("field", if secret { "secret" } else { hint }));
    text_input(ui, r, id, text, hint, FieldStyle { secret, ..Default::default() })
}

/// Sharing: upload recordings to the person's S3-compatible storage
/// (Hetzner Object Storage…) and copy the link.
/// Sharing in Screenshot or Video settings: the two switches, and where
/// the storage is set up (the Sharing page). Returns the bottom.
fn sharing(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, col: Rect, shots: bool, cx: &mut Cx) -> f32 {
    use super::share::VIDEO_ACCOUNT;
    let x0 = col.left();
    let id = ui.scope_id().with(("share", shots));
    crate::kit::glass::card_title(p, Pos2::new(x0, col.top()), ts!("cap.sharing"));
    let mut s = if shots { c.settings.shot_share.clone() } else { c.settings.share.clone() };
    let labels = [if shots { ts!("cap.up_shot") } else { ts!("cap.up_rec") }, ts!("cap.copy_when_done"), ts!("cap.storage")];
    let card = card_rows(p, x0, col.top() + 28.0, col.width(), labels.len());
    for (k, l) in labels.iter().enumerate() {
        p.text(Pos2::new(card.left() + 14.0, row_y(card, k)), Align2::LEFT_CENTER, *l, font(13.0), TEXT());
    }
    for (k, on) in [(0usize, s.auto), (1, s.copy_link)] {
        let y = row_y(card, k);
        if switch(ui, Rect::from_min_size(Pos2::new(card.right() - 12.0 - 30.0, y - 9.0), Vec2::new(30.0, 18.0)), id.with(("sw", k)), on, true).clicked() {
            if k == 0 {
                s.auto = !s.auto;
            } else {
                s.copy_link = !s.copy_link;
            }
        }
    }
    // where it goes, and the way to change it
    let y = row_y(card, 2);
    let lr = link(ui, Pos2::new(card.right() - 14.0, y), Align2::RIGHT_CENTER, ts!("cap.sharing_link"), 12.0);
    if lr.clicked() {
        cx.open_page(SHARING_ID, "");
    }
    let st = &c.settings.share;
    let (where_, col_) = if st.set_up(VIDEO_ACCOUNT) { (st.place(&s.folder), TEXT_DIM()) } else { (ts!("cap.not_set_up").to_string(), AMBER()) };
    text_fit(p, Pos2::new(lr.rect.left() - 10.0, y), Align2::RIGHT_CENTER, &where_, font(12.0), col_, card.width() * 0.55);
    if (shots && s != c.settings.shot_share) || (!shots && s != c.settings.share) {
        if shots {
            c.settings.shot_share = s;
        } else {
            c.settings.share = s;
        }
        c.save_settings();
    }
    card.bottom()
}

/// The Sharing page: the one storage, a folder for screenshots and one for
/// videos, and everything shared so far.
pub fn sharing_page(c: &mut Capture, ui: &mut Ui, r: Rect, _cx: &mut Cx) {
    use super::share::{secret, LinkKind, SHOT_FOLDER, VIDEO_ACCOUNT, VIDEO_FOLDER};
    // the storage file Claude saved, dropped on the page
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| super::share::import::is_storage_file(p)).collect());
    for f in dropped {
        c.import_storage(&f);
    }
    let p = ui.painter().clone();
    let shared = super::share::all();
    let ready = c.settings.share.set_up(VIDEO_ACCOUNT);
    let sub = if ready { t!("cap.shared_sub", n = shared.len(), bucket = c.settings.share.bucket.trim()).into_owned() } else { String::new() };
    // the main agent's chat walks them through a bucket and keys
    let agent = crate::kit::agent::main();
    let ask = super::share::setup::label(agent);
    let aw = crate::kit::aicheck::ui::agent_button_width(ui, &ask);
    let (_, chip_at) = page_header_room(&p, r, ts!("cap.sharing"), &sub, if ready { TEXT_DIM() } else { AMBER() }, aw, r.right() - sp::PAGE_EDGE);
    if crate::kit::aicheck::ui::agent_button(ui, chip_at, ui.scope_id().with("share-ask"), agent, &ask).clicked() {
        let _ = std::process::Command::new("open").arg(super::share::setup::chat_url(agent)).spawn();
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    let gap = 20.0;
    let x0 = body.left() + 24.0;
    let colw = ((body.width() - 48.0 - gap) / 2.0).max(300.0);
    let id = ui.scope_id().with("sharing-page");

    // left: the storage and the folders
    let mut s = c.settings.share.clone();
    let mut shot = c.settings.shot_share.clone();
    let mut y = body.top() + 16.0;
    crate::kit::glass::card_title(&p, Pos2::new(x0, y), ts!("cap.storage"));
    // R2 (the default) needs its public URL for Public links; Region only
    // for storages it can't be read from (MinIO…) or when already typed
    let host = s.endpoint.trim().to_lowercase();
    let known = s.is_r2() || host.contains("your-objectstorage.com") || host.contains("amazonaws.com");
    let show_public = host.is_empty() || s.is_r2() || !s.public_url.trim().is_empty();
    let show_region = !s.region.trim().is_empty() || (!host.is_empty() && !known);
    let mut labels = vec![ts!("cap.endpoint"), ts!("cap.bucket"), ts!("cap.access_key"), ts!("cap.secret_key")];
    if show_public {
        labels.push(ts!("cap.public_url"));
    }
    if show_region {
        labels.push(ts!("cap.region"));
    }
    labels.push(ts!("cap.link"));
    let card = card_rows(&p, x0, y + 28.0, colw, labels.len());
    for (k, l) in labels.iter().enumerate() {
        p.text(Pos2::new(card.left() + 14.0, row_y(card, k)), Align2::LEFT_CENTER, *l, font(13.0), TEXT());
    }
    let fw = (card.width() * 0.6).min(280.0);
    let frect = |card: Rect, k: usize| {
        let y = row_y(card, k);
        Rect::from_min_max(Pos2::new(card.right() - 6.0 - fw, y - sp::CONTROL_H / 2.0), Pos2::new(card.right() - 6.0, y + sp::CONTROL_H / 2.0))
    };
    field(ui, frect(card, 0), &mut s.endpoint, "account-id.r2.cloudflarestorage.com", false);
    field(ui, frect(card, 1), &mut s.bucket, "my-bucket", false);
    field(ui, frect(card, 2), &mut s.access_key, ts!("cap.access_key"), false);
    // the secret goes straight to the Keychain and is never shown back.
    // Opening Sharing never reads it: only whether the item is there (no
    // macOS prompt). The key is read when something needs it, after the
    // Keychain card (docs/ui-rules.md, Keychain access).
    let has_secret = *c.has_secret.get_or_insert_with(|| secret::exists(VIDEO_ACCOUNT));
    // on the page's own copy: the end of the page writes `s` back, so a
    // flag set on the settings themselves was undone the same frame and
    // Screenshot settings said "Not set up" with a key in the Keychain
    s.secret_saved = has_secret;
    let resp = field(ui, frect(card, 3), &mut c.share_secret, if has_secret { ts!("cap.in_keychain") } else { ts!("cap.secret_key") }, true);
    if std::mem::take(&mut c.focus_secret) {
        resp.request_focus();
    }
    if resp.lost_focus() && !c.share_secret.is_empty() {
        let v = std::mem::take(&mut c.share_secret);
        let v = v.trim();
        match secret::set(VIDEO_ACCOUNT, v, false) {
            Ok(()) if v.is_empty() => {
                c.has_secret = Some(false);
                s.secret_saved = false;
            }
            Ok(()) => {
                c.has_secret = Some(true);
                s.secret_saved = true;
                c.hud(ts!("cap.secret_saved"))
            }
            // an older key from another build is in the way: ask first
            Err(None) => c.need_keychain(super::KeychainPending::Save(v.to_string())),
            Err(Some(e)) => c.hud(t!("cap.keychain_fail", e = e).into_owned()),
        }
    }
    let mut k = 4;
    if show_public {
        field(ui, frect(card, k), &mut s.public_url, "pub-….r2.dev", false);
        k += 1;
    }
    if show_region {
        field(ui, frect(card, k), &mut s.region, ts!("cap.from_endpoint"), false);
        k += 1;
    }
    let ly = row_y(card, k);
    if let Some(i) = segmented(ui, Rect::from_min_max(Pos2::new(card.right() - 12.0 - 210.0, ly - 12.0), Pos2::new(card.right() - 12.0, ly + 12.0)), id.with("link"), &[ts!("cap.ui.public"), ts!("cap.ui.expires_7")], (s.link == LinkKind::Week) as usize) {
        s.link = if i == 1 { LinkKind::Week } else { LinkKind::Public };
    }
    y = card.bottom() + 24.0;
    crate::kit::glass::card_title(&p, Pos2::new(x0, y), ts!("cap.folders"));
    let fcard = card_rows(&p, x0, y + 28.0, colw, 2);
    for (k, (ico, l)) in [(Icon::Crop, ts!("cap.screenshots")), (Icon::Camera, ts!("cap.videos"))].into_iter().enumerate() {
        let ry = row_y(fcard, k);
        icon(&p, ico, Rect::from_center_size(Pos2::new(fcard.left() + 21.0, ry), Vec2::splat(15.0)), TEXT_DIM());
        p.text(Pos2::new(fcard.left() + 38.0, ry), Align2::LEFT_CENTER, l, font(13.0), TEXT());
    }
    field(ui, frect(fcard, 0), &mut shot.folder, SHOT_FOLDER, false);
    field(ui, frect(fcard, 1), &mut s.folder, VIDEO_FOLDER, false);
    if s != c.settings.share || shot != c.settings.shot_share {
        c.settings.share = s;
        c.settings.shot_share = shot;
        c.save_settings();
    }

    // right: what's been shared
    let rx = x0 + colw + gap;
    let rw = body.right() - 24.0 - rx;
    let ty = body.top() + 16.0;
    crate::kit::glass::card_title(&p, Pos2::new(rx, ty), ts!("cap.shared"));
    let list = Rect::from_min_max(Pos2::new(rx, ty + 28.0), Pos2::new(rx + rw, body.bottom() - 16.0));
    if shared.is_empty() {
        let er = Rect::from_min_size(list.min, Vec2::new(rw, 90.0));
        glass_card(&p, er, R_CARD, 0.0, false);
        p.text(er.center(), Align2::CENTER_CENTER, ts!("cap.nothing_shared"), bold(13.0), TEXT_SOFT());
        return;
    }
    let row_h = 54.0;
    let mut act: Option<(usize, u8)> = None;
    let mut child = ui.new_child(UiBuilder::new().max_rect(list));
    ScrollArea::vertical().id_salt("shared-list").auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(rw.min(ui.available_width()), shared.len() as f32 * (row_h + 6.0)), Sense::hover());
        let p = ui.painter().clone();
        for (k, (path, sh)) in shared.iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(area.left(), area.top() + k as f32 * (row_h + 6.0)), Vec2::new(rw, row_h));
            glass_card(&p, rr, R_CARD, 0.0, false);
            let img = super::share::is_image(path);
            icon(&p, if img { Icon::Crop } else { Icon::Camera }, Rect::from_center_size(Pos2::new(rr.left() + 22.0, rr.center().y), Vec2::splat(16.0)), TEXT_DIM());
            // buttons on the right: copy link, open, delete
            let mut bx = rr.right() - 10.0;
            for (j, (ico, tip)) in [(Icon::Trash, ts!("cap.deleted_storage_tip")), (Icon::Forward, ts!("cap.open_browser")), (Icon::Link, ts!("cap.copy_link"))].into_iter().enumerate() {
                let br = Rect::from_min_size(Pos2::new(bx - 28.0, rr.center().y - 14.0), Vec2::splat(28.0));
                if round_button(ui, br, ico, tip).clicked() {
                    act = Some((k, j as u8));
                }
                bx = br.left() - 4.0;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let tw = bx - 10.0 - (rr.left() + 42.0);
            let (ly1, ly2) = two_lines_y(&p, rr.center().y, &font(13.0), &mono(11.0));
            text_fit(&p, Pos2::new(rr.left() + 42.0, ly1), Align2::LEFT_CENTER, &name, font(13.0), if path.exists() { TEXT() } else { TEXT_DIM() }, tw);
            let when = if sh.at > 0 { chrono::DateTime::from_timestamp(sh.at, 0).map(|t| t.with_timezone(&chrono::Local)).map(|t| crate::kit::date::comma(&crate::kit::date::md(&t), &t.format("%H:%M").to_string()) + " · ").unwrap_or_default() } else { String::new() };
            text_fit(&p, Pos2::new(rr.left() + 42.0, ly2), Align2::LEFT_CENTER, &format!("{when}{}", sh.link), mono(11.0), TEXT_FAINT(), tw);
        }
    });
    if let Some((k, j)) = act {
        let (path, sh) = shared[k].clone();
        match j {
            2 => {
                c.copy_text(&sh.link);
                c.hud(ts!("cap.link_copied"));
            }
            1 => {
                let _ = std::process::Command::new("open").arg(&sh.link).spawn();
            }
            _ => c.delete_shared(path, sh.key),
        }
    }
}

fn row_y(card: Rect, k: usize) -> f32 {
    card.top() + 4.0 + k as f32 * ROW_H + ROW_H / 2.0
}

fn settings_page(c: &mut Capture, ui: &mut Ui, body: Rect, cx: &mut Cx) {
    record_keys(c, ui, cx);
    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
    ScrollArea::vertical().id_salt("capture-settings").auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1220.0), Sense::hover());
        let p = ui.painter().clone();
        let gap = 20.0;
        let x0 = area.left() + 24.0;
        let colw = (area.width() - 48.0 - gap) / 2.0;
        let below = shortcuts(c, ui, &p, Rect::from_min_size(Pos2::new(x0, area.top() + 16.0), Vec2::new(colw, area.height())), &SHOT_KEYS);
        let sy = below + 28.0;
        sharing(c, ui, &p, Rect::from_min_size(Pos2::new(x0, sy), Vec2::new(colw, area.bottom() - sy)), true, cx);
        options(c, ui, &p, Rect::from_min_size(Pos2::new(x0 + colw + gap, area.top() + 16.0), Vec2::new(colw, area.height())));
    });
}

/// While recording: the next key press with a modifier, or extra mouse
/// button, becomes the shortcut.
fn record_keys(c: &mut Capture, ui: &mut Ui, cx: &mut Cx) {
    use crate::kit::hotkeys::{listen, Heard};
    let Some(cmd) = c.recording else { return };
    let s = match listen(ui.ctx()) {
        Heard::Nothing => None,
        Heard::Cancel => {
            stop_recording(c);
            return;
        }
        Heard::Remove => Some(None),
        Heard::Set(s) => Some(Some(s)),
    };
    if let Some(s) = s {
        if let Some(w) = s.as_ref().and_then(|s| s.conflict()) {
            cx.toast(format!("{}: {w}", s.as_ref().map(|s| s.display()).unwrap_or_default()));
        }
        c.settings.set_shortcut(cmd, s);
        c.save_settings();
        stop_recording(c);
    }
    ui.ctx().request_repaint();
}

/// Returns the bottom of what it drew.
fn shortcuts(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, col: Rect, list: &[Command]) -> f32 {
    let x0 = col.left();
    crate::kit::glass::card_title(p, Pos2::new(x0, col.top()), ts!("cap.shortcuts"));
    let card = card_rows(p, x0, col.top() + 28.0, col.width(), list.len());
    for (k, cmd) in list.iter().enumerate() {
        let y = row_y(card, k);
        icons::draw(p, command_icon(*cmd), Rect::from_center_size(Pos2::new(card.left() + 21.0, y), Vec2::splat(16.0)), TEXT_DIM());
        let lr = p.text(Pos2::new(card.left() + 38.0, y), Align2::LEFT_CENTER, cmd.label(), font(13.0), TEXT());
        let rec = c.recording == Some(*cmd);
        let cur = c.settings.shortcut(*cmd).cloned();
        let label = if rec { ts!("cap.type_shortcut").to_string() } else { cur.as_ref().map(|s| s.display()).unwrap_or_else(|| ts!("cap.record").into()) };
        let g = p.layout_no_wrap(label.clone(), font(12.0), TEXT());
        let bw = (g.size().x + 20.0).max(76.0);
        let clear = Rect::from_center_size(Pos2::new(card.right() - 22.0, y), Vec2::splat(20.0));
        let br = Rect::from_min_max(Pos2::new(clear.left() - 6.0 - bw, y - 12.0), Pos2::new(clear.left() - 6.0, y + 12.0));
        let resp = ui.interact(br, ui.scope_id().with(("key", *cmd as usize)), Sense::click());
        if rec {
            p.rect_filled(br, 6.0, with_alpha(ACTION(), 0.22));
            p.rect_stroke(br, 6.0, Stroke::new(1.0, ACCENT()), StrokeKind::Inside);
        } else if cur.is_some() {
            p.rect_filled(br, 6.0, white(if resp.hovered() { 30 } else { 20 }));
            p.rect_stroke(br, 6.0, Stroke::new(1.0, white(26)), StrokeKind::Inside);
        } else {
            dashed_rect(p, br, 6.0, white(60));
        }
        p.text(br.center(), Align2::CENTER_CENTER, label, font(12.0), if cur.is_some() || rec { TEXT() } else { TEXT_FAINT() });
        if resp.on_hover_text(ts!("cap.shortcut_tip")).clicked() {
            if rec {
                stop_recording(c);
            } else {
                c.recording = Some(*cmd);
                if let Some(keys) = &mut c.keys {
                    keys.clear();
                }
            }
        }
        if cur.is_some() && !rec {
            let cr = ui.interact(clear, ui.scope_id().with(("clear", *cmd as usize)), Sense::click());
            if cr.hovered() {
                p.circle_filled(clear.center(), 10.0, white(26));
            }
            icons::draw(p, Ico::Close, clear.shrink(5.0), TEXT_FAINT());
            if cr.on_hover_text(ts!("cap.remove_shortcut")).clicked() {
                c.settings.set_shortcut(*cmd, None);
                c.save_settings();
                c.apply_keys();
            }
        }
        // why it may not work
        let warn = c
            .key_errors
            .iter()
            .find(|(e, _)| e == cmd)
            .map(|(_, m)| m.clone())
            .or_else(|| cur.as_ref().and_then(|s| s.conflict()).map(|m| m.to_string()));
        if let Some(w) = warn {
            let wx = lr.right() + 10.0;
            if br.left() - 10.0 - wx > 40.0 {
                text_fit(p, Pos2::new(wx, y), Align2::LEFT_CENTER, &w, font(11.0), AMBER(), br.left() - 10.0 - wx);
            }
        }
    }
    card.bottom()
}

fn options(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, col: Rect) {
    let x0 = col.left();
    let w = col.width();
    let id = ui.scope_id().with("capture-options");
    let mut changed = false;
    let mut y = col.top();

    // after a capture
    crate::kit::glass::card_title(p, Pos2::new(x0, y), ts!("cap.after_capture"));
    let card = card_rows(p, x0, y + 28.0, w, 5);
    let ry = row_y(card, 0);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.then"), font(13.0), TEXT());
    let sr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 270.0, ry - 13.0), Pos2::new(card.right() - 12.0, ry + 13.0));
    let cur = match c.settings.after {
        After::Preview => 0,
        After::Editor => 1,
        After::Nothing => 2,
    };
    if let Some(i) = segmented(ui, sr, id.with("after"), &[ts!("cap.show_preview"), ts!("cap.open_editor"), ts!("cap.nothing")], cur) {
        c.settings.after = [After::Preview, After::Editor, After::Nothing][i];
        changed = true;
    }
    let ry = row_y(card, 3);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.hide_after"), font(13.0), TEXT());
    if let Some(i) = segmented(ui, Rect::from_min_max(Pos2::new(card.right() - 12.0 - 200.0, ry - 12.0), Pos2::new(card.right() - 12.0, ry + 12.0)), id.with("hide"), &[ts!("cap.s3"), ts!("cap.s6"), ts!("cap.s10"), ts!("cap.never")], secs_index(c.settings.preview_secs, &PREVIEW_SECS)) {
        c.settings.preview_secs = PREVIEW_SECS.get(i).copied();
        changed = true;
    }
    let switches: [(&str, &str, bool); 4] = [
        (ts!("cap.copy_clip"), "", c.settings.copy),
        (ts!("cap.save_folder"), "", c.settings.save),
        ("", "", false),
        (ts!("cap.play_sound"), "", c.settings.sound),
    ];
    for (k, (label, hint, on)) in switches.iter().enumerate() {
        if k == 2 {
            continue;
        }
        let ry = row_y(card, k + 1);
        p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, *label, font(13.0), TEXT());
        let sw = Rect::from_center_size(Pos2::new(card.right() - 27.0, ry), Vec2::new(30.0, 18.0));
        if !hint.is_empty() {
            p.text(Pos2::new(sw.left() - 10.0, ry), Align2::RIGHT_CENTER, *hint, font(12.0), TEXT_FAINT());
        }
        if switch(ui, sw, id.with(("sw", k)), *on, true).clicked() {
            match k {
                0 => c.settings.copy = !c.settings.copy,
                1 => c.settings.save = !c.settings.save,
                _ => c.settings.sound = !c.settings.sound,
            }
            changed = true;
        }
    }
    y = card.bottom() + 16.0;

    // saving
    crate::kit::glass::card_title(p, Pos2::new(x0, y), ts!("cap.saving"));
    let card = card_rows(p, x0, y + 28.0, w, 4);
    folder_row(c, ui, p, card, row_y(card, 0), false);
    let ry = row_y(card, 1);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.format"), font(13.0), TEXT());
    let fr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 110.0, ry - 13.0), Pos2::new(card.right() - 12.0, ry + 13.0));
    if let Some(i) = segmented(ui, fr, id.with("format"), &["PNG", "JPG"], (c.settings.format == Format::Jpg) as usize) {
        c.settings.format = if i == 1 { Format::Jpg } else { Format::Png };
        changed = true;
    }
    let ry = row_y(card, 2);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.file_name"), font(13.0), TEXT());
    let example = c.settings.file_name(chrono::Local::now());
    text_fit(p, Pos2::new(card.right() - 12.0, ry), Align2::RIGHT_CENTER, &example, mono(12.0), TEXT_DIM(), card.width() - 110.0);
    changed |= history_row(c, ui, p, card, row_y(card, 3), id, false);
    y = card.bottom() + 16.0;

    // permission
    permissions(ui, p, x0, y, w, &[Perm::Screen]);
    if changed {
        c.save_settings();
    }
}

/// The Folder row of a Saving card: where screenshots, or recordings
/// (`video`), go, and Change….
fn folder_row(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, card: Rect, ry: f32, video: bool) {
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.folder"), font(13.0), TEXT());
    let bw = button_width(p, ts!("cap.change_btn"), BTN_SECONDARY) - 6.0;
    let br = Rect::from_min_max(Pos2::new(card.right() - 12.0 - bw, ry - 12.0), Pos2::new(card.right() - 12.0, ry + 12.0));
    if pill_button(ui, br, ts!("cap.change_btn"), BTN_SECONDARY, true).clicked() {
        c.choose_folder(video);
    }
    let shown = if video { c.shown_rec_folder() } else { c.shown_folder() };
    text_fit(p, Pos2::new(br.left() - 10.0, ry), Align2::RIGHT_CENTER, &shown, mono(12.0), TEXT_DIM(), br.left() - card.left() - 90.0);
}

/// The History keeps row: − N + and Clear history, for screenshots or
/// recordings (`video`), each with its own limit. True when the limit moved.
fn history_row(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, card: Rect, ry: f32, id: egui::Id, video: bool) -> bool {
    let mut changed = false;
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.history_keeps"), font(13.0), TEXT());
    let confirming = c.confirm_clear.is_some_and(|(v, t)| v == video && t.elapsed().as_secs_f32() < 3.0);
    let clear_label = if confirming { ts!("cap.click_again") } else { ts!("cap.clear_history") };
    let cw = button_width(p, clear_label, BTN_DANGER) - 6.0;
    let cr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - cw, ry - 12.0), Pos2::new(card.right() - 12.0, ry + 12.0));
    let any = c.store.shots.iter().any(|s| (s.kind == Kind::Recording) == video);
    if pill_button(ui, cr, clear_label, if confirming { BTN_DANGER } else { BTN_SECONDARY }, any).clicked() {
        if confirming {
            c.store.clear(video);
            c.forget_dropped_thumbs();
            c.confirm_clear = None;
            c.hud(ts!("cap.history_cleared"));
        } else {
            c.confirm_clear = Some((video, Instant::now()));
        }
    }
    if confirming {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    let keep = if video { c.settings.rec.keep } else { c.settings.keep };
    let plus = Rect::from_center_size(Pos2::new(cr.left() - 24.0, ry), Vec2::new(24.0, 24.0));
    let num = Rect::from_center_size(Pos2::new(plus.left() - 20.0, ry), Vec2::new(40.0, 24.0));
    let minus = Rect::from_center_size(Pos2::new(num.left() - 12.0, ry), Vec2::new(24.0, 24.0));
    for (r, s, delta) in [(minus, "−", -10i64), (plus, "+", 10)] {
        let resp = ui.interact(r, id.with(("step", s, video)), Sense::click());
        p.rect_filled(r, 6.0, white(if resp.hovered() { 34 } else { 20 }));
        p.text(r.center(), Align2::CENTER_CENTER, s, bold(13.0), TEXT());
        if resp.clicked() {
            let n = (keep as i64 + delta).clamp(KEEP_MIN as i64, KEEP_MAX as i64) as usize;
            if video {
                c.settings.rec.keep = n;
            } else {
                c.settings.keep = n;
            }
            c.store.trim(c.settings.keeps());
            changed = true;
        }
    }
    p.text(num.center(), Align2::CENTER_CENTER, keep.to_string(), bold(13.0), TEXT());
    changed
}

/// A Permission card at `y` (R17): one row per permission.
fn permissions(ui: &mut Ui, p: &egui::Painter, x0: f32, y: f32, w: f32, list: &[Perm]) -> f32 {
    crate::kit::glass::card_title(p, Pos2::new(x0, y), ts!("cap.permission"));
    let card = card_rows(p, x0, y + 28.0, w, list.len());
    for (k, perm) in list.iter().enumerate() {
        perm_row(ui, p, card, row_y(card, k), *perm);
    }
    card.bottom()
}

/// Video › Settings, right column: what happens after a recording, where
/// it's saved and how many history keeps, and the permissions it uses.
fn recording_after(c: &mut Capture, ui: &mut Ui, p: &egui::Painter, col: Rect) {
    let (x0, w) = (col.left(), col.width());
    let id = ui.scope_id().with("rec-after");
    let mut changed = false;
    let mut y = col.top();
    crate::kit::glass::card_title(p, Pos2::new(x0, y), ts!("cap.after_recording"));
    let card = card_rows(p, x0, y + 28.0, w, 3);
    let ry = row_y(card, 0);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.then"), font(13.0), TEXT());
    let sr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 270.0, ry - 13.0), Pos2::new(card.right() - 12.0, ry + 13.0));
    let cur = match c.settings.rec.after {
        After::Preview => 0,
        After::Editor => 1,
        After::Nothing => 2,
    };
    if let Some(i) = segmented(ui, sr, id.with("after"), &[ts!("cap.show_preview"), ts!("cap.open_editor"), ts!("cap.nothing")], cur) {
        c.settings.rec.after = [After::Preview, After::Editor, After::Nothing][i];
        changed = true;
    }
    let ry = row_y(card, 1);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.copy_clip"), font(13.0), TEXT());
    let sw = Rect::from_center_size(Pos2::new(card.right() - 27.0, ry), Vec2::new(30.0, 18.0));
    if switch(ui, sw, id.with("copy"), c.settings.rec.copy, true).clicked() {
        c.settings.rec.copy = !c.settings.rec.copy;
        changed = true;
    }
    let ry = row_y(card, 2);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.hide_after"), font(13.0), TEXT());
    let hr = Rect::from_min_max(Pos2::new(card.right() - 12.0 - 200.0, ry - 12.0), Pos2::new(card.right() - 12.0, ry + 12.0));
    if let Some(i) = segmented(ui, hr, id.with("hide"), &[ts!("cap.s5"), ts!("cap.s8"), ts!("cap.s15"), ts!("cap.never")], secs_index(c.settings.rec.card_secs, &CARD_SECS)) {
        c.settings.rec.card_secs = CARD_SECS.get(i).copied();
        changed = true;
    }
    y = card.bottom() + 16.0;

    crate::kit::glass::card_title(p, Pos2::new(x0, y), ts!("cap.saving"));
    let card = card_rows(p, x0, y + 28.0, w, 3);
    folder_row(c, ui, p, card, row_y(card, 0), true);
    let ry = row_y(card, 1);
    p.text(Pos2::new(card.left() + 12.0, ry), Align2::LEFT_CENTER, ts!("cap.file_name"), font(13.0), TEXT());
    let example = Settings::recording_name(chrono::Local::now());
    text_fit(p, Pos2::new(card.right() - 12.0, ry), Align2::RIGHT_CENTER, &example, mono(12.0), TEXT_DIM(), card.width() - 110.0);
    changed |= history_row(c, ui, p, card, row_y(card, 2), id, true);
    y = card.bottom() + 16.0;

    permissions(ui, p, x0, y, w, &[Perm::Screen, Perm::Microphone, Perm::Camera]);
    if changed {
        c.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters() {
        let s = |kind, pinned| Shot { id: 1, kind, w: 1, h: 1, saved: None, pinned, secs: None };
        assert!(passes(0, &s(Kind::Screen, false)));
        assert!(passes(1, &s(Kind::Edited, false)));
        assert!(!passes(2, &s(Kind::Area, false)));
        assert!(passes(4, &s(Kind::Window, true)));
        assert!(!passes(4, &s(Kind::Window, false)));
    }

    #[test]
    fn recordings_alone_still_show_the_first_capture_screen() {
        let s = |kind| Shot { id: 1, kind, w: 1, h: 1, saved: None, pinned: false, secs: None };
        assert!(only_recordings(&[]));
        assert!(only_recordings(&[s(Kind::Recording)]));
        assert!(!only_recordings(&[s(Kind::Recording), s(Kind::Area)]));
    }
}

// ---------------------------------------------------------------- Video

/// The Video widget at `size`: Record (Stop with the time while recording)
/// and, at Medium, the latest recordings.
pub fn video_widget(c: &mut Capture, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let id = ui.scope_id().with("w-video");
    match size {
        Size::S11 => {
            let live = c.rec.tray().map(|t| t.trim_start_matches(['●', '❚', ' ']).to_string());
            let sub = live.clone().unwrap_or_else(|| key_label(c, Command::Record));
            let sub = if sub.is_empty() { "Record".to_string() } else { sub };
            let tip = if live.is_some() { "Stop and save the recording" } else { "Record the screen" };
            if tile::small(ui, rect, id, Icon::Camera, tint::CAPTURE(), "Video", &sub, live.is_some()).on_hover_text(tip).clicked() {
                c.queue.push((Command::Record, 0));
            }
        }
        Size::S12 | Size::S14 => {
            // Record and Recordings, like Screenshot's actions; the Row adds the last recording
            let _ = tile::frame(ui, rect, id, false);
            let p = ui.painter().clone();
            let live = c.rec.tray().map(|t| t.trim_start_matches(['●', '❚', ' ']).to_string());
            let keys = key_label(c, Command::Record);
            let last = if size == Size::S14 { recordings(c).into_iter().next() } else { None };
            let ago = last.as_ref().map(|it| since((chrono::Local::now() - it.time).num_seconds().max(0))).unwrap_or_default();
            let n = recordings(c).len();
            let count = if n == 1 { "1 recording".to_string() } else { format!("{n} recordings") };
            let mut acts = vec![match &live {
                Some(t) => tile::Action { label: t, sub: "", color: RED(), tip: "Stop and save the recording" },
                None => tile::Action { label: "Record", sub: &keys, color: RED(), tip: "Record the screen" },
            }];
            let mut glyphs = vec![Ico::Record];
            if last.is_some() {
                acts.push(tile::Action { label: "Last", sub: &ago, color: TEXT(), tip: "Open the last recording" });
                glyphs.push(Ico::Play);
            }
            acts.push(tile::Action { label: "Recordings", sub: "", color: TEXT(), tip: &count });
            glyphs.push(Ico::Folder);
            match tile::actions(ui, &p, id, rect.shrink(4.0), &acts, |p, k, gr, col| icons::draw(p, glyphs[k], gr, col)) {
                Some(0) => c.queue.push((Command::Record, 0)),
                Some(k) if glyphs[k] == Ico::Play => {
                    if let Some(it) = &last {
                        run_act(c, Act::Edit, it);
                    }
                }
                Some(_) => {
                    c.vtab = VTab::Recordings;
                    cx.open_page(VIDEO_ID, "");
                }
                None => {}
            }
        }
        _ => video_card(c, ui, rect, cx),
    }
}

/// "just now", "12 min ago", "3 h ago", "2 d ago".
fn since(secs: i64) -> String {
    match secs {
        ..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=172_799 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

fn recordings(c: &Capture) -> Vec<Item> {
    c.store.shots.iter().filter(|s| s.kind == Kind::Recording).map(Item::from_shot).collect()
}

/// The Medium Video widget: Record and the latest recordings.
fn video_card(c: &mut Capture, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    c.drain_done();
    let p = ui.painter().clone();
    let id = ui.scope_id().with("video-card");
    let _ = tile::frame(ui, rect, id.with("frame"), false);
    let (x0, x1, mut y) = card_head(ui, &p, rect, Icon::Camera, "Video");
    if link(ui, Pos2::new(x1, y - sp::M - 10.0), Align2::RIGHT_CENTER, "Recordings", 12.0).clicked() {
        c.vtab = VTab::Recordings;
        cx.open_page(VIDEO_ID, "");
    }
    // one wide Record button: the red dot, its name, the shortcut on the right
    let b = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + MODE_H));
    let resp = ui.interact(b, id.with("record"), Sense::click());
    p.rect_filled(b, R_BUTTON, white(12));
    let live = c.rec.tray().map(|t| t.trim_start_matches(['●', '❚', ' ']).to_string());
    let dot = Pos2::new(b.left() + sp::M + 5.0, b.center().y);
    p.circle_filled(dot, 5.0, RED());
    let label = match &live {
        Some(t) => format!("Stop · {t}"),
        None => "Record screen".to_string(),
    };
    p.text(Pos2::new(dot.x + 5.0 + sp::S, b.center().y), Align2::LEFT_CENTER, label, font(12.0), if live.is_some() { RED() } else { TEXT() });
    let keys = key_label(c, Command::Record);
    if live.is_none() && !keys.is_empty() {
        p.text(Pos2::new(b.right() - sp::M, b.center().y), Align2::RIGHT_CENTER, keys, font(12.0), TEXT_FAINT());
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text(if live.is_some() { "Stop and save the recording" } else { "Record the screen" }).clicked() {
        c.queue.push((Command::Record, 0));
    }
    y += MODE_H + sp::M;
    let items: Vec<Item> = recordings(c).into_iter().take(3).collect();
    let row = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, rect.bottom() - sp::M));
    recent_row(c, ui, &p, id, row, &items, 3, Act::Edit, "click to open");
}

/// The Video page: recordings, or the recording settings.
pub fn video_page(c: &mut Capture, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let n = c.store.shots.iter().filter(|s| s.kind == Kind::Recording).count();
    let first = c.vtab == VTab::Recordings && n == 0 && c.rec.tray().is_none();
    let sub = match c.vtab {
        VTab::Recordings if first => format!("Saving to {}", c.shown_rec_folder()),
        VTab::Recordings => format!("{n} recording{} · saving to {}", if n == 1 { "" } else { "s" }, c.shown_rec_folder()),
        VTab::Settings => String::new(),
    };
    // SP16: Record screen at the right edge on every tab, one width for
    // "Record screen" and "Stop · 12:34", so the tabs never move
    let live = c.rec.tray().map(|t| t.trim_start_matches(['●', '❚', ' ']).to_string());
    let label = match &live {
        Some(t) => format!("Stop · {t}"),
        None => "Record screen".to_string(),
    };
    let bw = ["Record screen", "Stop · 00:00", label.as_str()].iter().map(|l| icon_pill_width(&p, l, BTN_PRIMARY)).fold(0.0, f32::max);
    let btns = page_header_room(&p, r, "Video", &sub, TEXT_DIM(), 0.0, r.right() - sp::PAGE_EDGE - bw - sp::M - 200.0 - sp::M).0;
    let (tr, br) = header_controls(btns, 200.0, bw);
    let (resp, face) = pill_button_at(ui, br, "", BTN_PRIMARY, true);
    if resp.clicked() {
        c.queue.push((Command::Record, 0));
    }
    p.circle_filled(Pos2::new(face.left() + 18.0, face.center().y), 6.0, if live.is_some() { ON_ACTION } else { RED() });
    p.text(Pos2::new(face.left() + 31.0, face.center().y), Align2::LEFT_CENTER, label, bold(13.0), ON_ACTION);
    let tabs = [VTab::Recordings, VTab::Settings];
    let cur = tabs.iter().position(|t| *t == c.vtab).unwrap_or(0);
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("video-tab"), &["Recordings", "Settings"], cur) {
        c.vtab = tabs[i];
        stop_recording(c);
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    match c.vtab {
        VTab::Recordings => {
            let list = Rect::from_min_max(Pos2::new(body.left(), body.top() + 12.0), body.max);
            let items = recordings(c);
            if items.is_empty() {
                first_recording(c, ui, body);
                return;
            }
            for (a, it) in grid(c, ui, list, &items, "video-history") {
                run_act(c, a, &it);
            }
        }
        VTab::Settings => {
            record_keys(c, ui, cx);
            let mut child = ui.new_child(UiBuilder::new().max_rect(body));
            ScrollArea::vertical().id_salt("video-settings").auto_shrink([false, false]).show(&mut child, |ui| {
                let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 900.0), Sense::hover());
                let p = ui.painter().clone();
                let gap = 20.0;
                let x0 = area.left() + 24.0;
                let colw = (area.width() - 48.0 - gap) / 2.0;
                let below = shortcuts(c, ui, &p, Rect::from_min_size(Pos2::new(x0, area.top() + 16.0), Vec2::new(colw, area.height())), &VIDEO_KEYS);
                let ry = below + 28.0;
                let ry = recording_options(c, ui, &p, Rect::from_min_size(Pos2::new(x0, ry), Vec2::new(colw, area.bottom() - ry)));
                sharing(c, ui, &p, Rect::from_min_size(Pos2::new(x0, ry), Vec2::new(colw, area.bottom() - ry)), false, cx);
                recording_after(c, ui, &p, Rect::from_min_size(Pos2::new(x0 + colw + gap, area.top() + 16.0), Vec2::new(colw, area.height())));
            });
        }
    }
}
