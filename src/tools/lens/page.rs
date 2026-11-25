//! Disk Map screens: choose what to map (with live progress while it runs)
//! → bubbles you can zoom into folder by folder, with a list to pick from → review and move to Trash.

use crate::kit::aicheck::{self, ui::Event};
use crate::kit::sync::{LockExt, RwLockExt};
use super::scan::{self, Node};
use super::Lens as App;
use crate::core::tool::Cx;
use crate::kit::glass::*;
use crate::kit::menu;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

const HEADER_H: f32 = 50.0;
const INFO_H: f32 = 28.0;

pub fn page(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    view(app, ui, r, cx);
    // every frame, whatever the page shows: a frame without the card would
    // drop a click on Stop (a rescan after the trash switches views)
    trash_loader(app, ui, r, cx);
}

fn view(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let shared = app.lens.clone();
    let now = ui.ctx().input(|i| i.time);
    let running = shared.running.load(Ordering::SeqCst);
    // a folder picked in the folder dialog, or dropped on the window
    let picked = app.lens_pick.lock_ok().take();
    let dropped = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| p.is_dir()));
    if let Some(dir) = picked.or(dropped).filter(|_| !running) {
        begin_at(app, ui.ctx(), dir);
        return;
    }
    if !running && ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
        drop_hint(ui, r);
    }
    if shared.running.load(Ordering::SeqCst) && !shared.updating.load(Ordering::SeqCst) {
        // while mapping: a calm list of folders that grows in place; the
        // treemap is drawn once, when the sizes are final
        if app.lens_live.as_ref().is_none_or(|l| now - l.0 > 0.25) {
            app.lens_live = scan::partial(&shared).map(|(root, rows)| (now, root, rows));
        }
        // a finished folder opened while the rest is still being measured
        if app.lens_peek {
            let found = app.lens_live.as_ref().and_then(|l| {
                let done = l.2.iter().filter(|x| x.done).count();
                l.2.iter().find(|x| app.lens_path.starts_with(&x.path)).and_then(|x| x.node.clone()).map(|n| (n, done, l.2.len()))
            });
            if let Some((sub, done, all)) = found {
                match results(app, ui, r, &sub, Some((done, all))) {
                    Act::Stop => shared.stop.store(true, Ordering::SeqCst),
                    Act::Switch(to) => switch(app, ui.ctx(), to),
                    Act::Overview => {
                        app.lens_peek = false;
                        app.lens_path = app.lens_root.clone();
                        app.lens_back.clear();
                        app.lens_fwd.clear();
                    }
                    _ => {}
                }
                if app.lens_review {
                    review(app, ui, r, cx);
                }
                return;
            }
            app.lens_peek = false;
        }
        match app.lens_live.take() {
            Some(live) => {
                let act = mapping(app, ui, r, &live.1, &live.2);
                app.lens_live = Some(live);
                if act == Act::Stop {
                    shared.stop.store(true, Ordering::SeqCst);
                }
            }
            None => start(app, ui, r),
        }
        return;
    }
    if app.lens_live.is_some() || !app.lens_order.is_empty() {
        // mapping just finished: the treemap starts fresh, no glide from the list
        app.lens_order.clear();
        app.lens_rows.clear();
    }
    app.lens_live = None;
    app.lens_peek = false;
    // items moved to the Trash while the map was still being made
    if !app.lens_trashed.is_empty() && !running {
        if let Some(root) = shared.result.write_ok().as_mut() {
            scan::prune(root, &app.lens_trashed);
        }
        app.lens_trashed.clear();
        scan::save_later(&shared);
    }
    let guard = shared.result.read_ok();
    let Some(root) = guard.as_ref() else {
        drop(guard);
        // show the last map if there is one (even from a previous run);
        // map the disk only the very first time
        scan::load(&shared, ui.ctx());
        if shared.loading.load(Ordering::SeqCst) {
            page_header(&ui.painter().clone(), r, ts!("lens.name"), ts!("lens.opening"), TEXT_DIM());
            return;
        }
        // nothing mapped yet: mapping walks the whole disk, so it waits
        // for a click
        if shared.result.read_ok().is_none() {
            match intro(ui, r, &root_label(&app.lens_root)) {
                Some(Start::Disk) => begin(app, ui.ctx()),
                Some(Start::Folder(p)) => begin_at(app, ui.ctx(), p),
                Some(Start::Choose) => choose_folder(app, ui.ctx()),
                None => {}
            }
        }
        return;
    };
    let root_path = root.path.clone();
    let act = results(app, ui, r, root, None);
    // what `results` asks for runs only once the map's lock is let go:
    // switching to the disk clears the map, and doing that under the lock
    // froze the page for good
    drop(guard);
    auto_update(app, ui.ctx(), &root_path, now);
    match act {
        Act::Stop => shared.stop.store(true, Ordering::SeqCst),
        Act::Update => {
            app.lens_selected.clear();
            if shared.running.load(Ordering::SeqCst) {
                // an automatic update is already on it: show it, with Stop
                shared.quiet.store(false, Ordering::SeqCst);
            } else {
                scan::update(shared.clone(), ui.ctx().clone(), false);
            }
        }
        Act::Switch(to) => switch(app, ui.ctx(), to),
        _ if app.lens_review => review(app, ui, r, cx),
        _ => {}
    }
}

#[derive(PartialEq)]
enum Act {
    None,
    Update,
    Stop,
    /// Leave a finished folder for the live overview of the whole map.
    Overview,
    /// Map something else, picked in the switcher (None: ask for a folder).
    Switch(Option<PathBuf>),
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn tilde(p: &Path) -> String {
    let h = home().display().to_string();
    let s = p.display().to_string();
    if s == h {
        "~".into()
    } else {
        s.replacen(&(h + "/"), "~/", 1)
    }
}

// ---------------------------------------------------------------- start

/// What the start screen asks for.
enum Start {
    Disk,
    Folder(PathBuf),
    Choose,
}

/// Before the first map, no cards (SP1): the disk ring and Map disk on the
/// left, a hairline, and on the right the places that can be mapped instead.
fn intro(ui: &mut Ui, r: Rect, name: &str) -> Option<Start> {
    let p = ui.painter().clone();
    page_header(&p, r, ts!("lens.name"), "", TEXT_DIM());
    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, r.top() + PAGE_HEADER_H + 1.0 + sp::L), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - sp::L));
    let (total, free) = crate::kit::fs::volume_space("/");
    const LEFT_W: f32 = 240.0;
    const GAP: f32 = 48.0;
    const BTN: Vec2 = Vec2::new(200.0, 44.0);
    let h = home();
    // a place that isn't there (a renamed Desktop) is left out
    let rows: Vec<(&str, Icon, Option<PathBuf>)> = [
        (ts!("lens.home"), Icon::Home, Some(h.clone())),
        (ts!("kit.place.downloads"), Icon::Down, Some(h.join("Downloads"))),
        (ts!("kit.place.documents"), Icon::Doc, Some(h.join("Documents"))),
        (ts!("kit.place.desktop"), Icon::Monitor, Some(h.join("Desktop"))),
        (ts!("kit.place.applications"), Icon::Window, Some(PathBuf::from("/Applications"))),
        (ts!("lens.choose_folder"), Icon::Folder, None),
    ]
    .into_iter()
    .filter(|(_, _, p)| p.as_ref().is_none_or(|p| p.is_dir()))
    .collect();
    let left_h = DISK_RING_H + sp::XXL + BTN.y;
    let rows_h = sp::SECTION_H + rows.len() as f32 * sp::ROW_H;
    let block_w = body.width().min(840.0);
    let block_h = left_h.max(rows_h);
    let top = (body.center().y - block_h / 2.0).max(body.top());
    let block = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, top), Vec2::new(block_w, block_h));
    let lc = block.left() + LEFT_W / 2.0;
    let ly = block.top() + (block_h - left_h) / 2.0;
    disk_ring(&p, Pos2::new(lc, ly), name, total, free, tint::DISK_MAP());
    let br = Rect::from_center_size(Pos2::new(lc, ly + DISK_RING_H + sp::XXL + BTN.y / 2.0), BTN);
    let id = ui.scope_id().with("map-disk");
    if crate::kit::empty::hero_button(ui, br.center(), id, ts!("lens.map_disk"), None, crate::kit::empty::Lead::None, Some(ACTION()), true, br.width()).clicked() {
        return Some(Start::Disk);
    }
    let dx = block.left() + LEFT_W + GAP;
    hairline(&p, Pos2::new(dx, block.top()), Pos2::new(dx, block.bottom()));
    let x0 = dx + 1.0 + GAP;
    let x1 = block.right();
    let mut y = block.top() + (block_h - rows_h) / 2.0;
    section_title(&p, x0, y, ts!("lens.or_folder"));
    y += sp::SECTION_H;
    let mut out = None;
    for (k, (label, ic, path)) in rows.into_iter().enumerate() {
        let rr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + sp::ROW_H));
        let resp = ui.interact(rr, ui.scope_id().with(("map-place", k)), Sense::click());
        if resp.hovered() {
            crate::kit::glass::start_row_fill(&p, rr);
        }
        if k > 0 {
            crate::kit::glass::row_divider(ui, &p, rr.left_top(), rr.right_top(), resp.hovered(), rr.bottom());
        }
        let cy = rr.center().y;
        // H9: content 12 in from the row's edges
        let x0 = x0 + crate::kit::glass::START_PAD;
        let icp = Pos2::new(x0 + 14.0, cy);
        let col = tint::DISK_MAP();
        p.circle_filled(icp, 14.0, with_alpha(col, 0.14));
        icon(&p, ic, Rect::from_center_size(icp, Vec2::splat(16.0)), col);
        p.text(Pos2::new(x0 + 28.0 + sp::M, cy), Align2::LEFT_CENTER, label, ty::body(), TEXT());
        if resp.clicked() {
            out = Some(path.map_or(Start::Choose, Start::Folder));
        }
        y += sp::ROW_H;
    }
    out
}

/// A folder dragged over the window: an outline says it can be dropped.
fn drop_hint(ui: &mut Ui, r: Rect) {
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("diskmap-drop")));
    let a = r.shrink(10.0);
    p.rect_filled(a, 14.0, with_alpha(ACCENT(), 0.06));
    dashed_rect(&p, a, 14.0, with_alpha(ACCENT(), 0.8));
}

/// What a mapped location is called: "Macintosh HD", "Home", a volume or
/// folder name.
fn root_label(p: &Path) -> String {
    if p == home() {
        ts!("lens.home").into()
    } else {
        scan::display_name(p)
    }
}

/// The volume a map is on, by name.
fn volume_label(p: &Path) -> String {
    match p.strip_prefix("/Volumes").ok().and_then(|r| r.components().next()) {
        Some(v) => v.as_os_str().to_string_lossy().into_owned(),
        None => "Macintosh HD".into(),
    }
}

/// Places to map: the disk, Home, every other mounted volume.
fn locations() -> Vec<(String, PathBuf)> {
    let mut out = vec![("Macintosh HD".to_string(), PathBuf::from("/")), (ts!("lens.home").to_string(), home())];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        let mut vols: Vec<(String, PathBuf)> = rd
            .flatten()
            // the startup disk shows up here as a link to "/"
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(n, _)| !n.starts_with('.') && !n.starts_with("com.apple."))
            .collect();
        vols.sort();
        out.extend(vols);
    }
    out
}

/// Something changed on disk (any volume): set by the file-system watch.
static CHANGED: crate::kit::activity::Kick = crate::kit::activity::Kick::new();

/// While the finished map is on screen, it follows the disk: a few seconds
/// after something changes (at most every 20 s) only the changed folders are
/// read again, quietly.
fn auto_update(app: &mut App, ctx: &egui::Context, root: &Path, now: f64) {
    static WATCHING: std::sync::Once = std::sync::Once::new();
    WATCHING.call_once(|| {
        crate::kit::fswatch::watch(&[PathBuf::from("/"), PathBuf::from("/System/Volumes/Data"), PathBuf::from("/Volumes")], 2.0, &CHANGED);
    });
    if CHANGED.take() {
        app.lens_dirty.get_or_insert(now);
    }
    let busy = app.lens.running.load(Ordering::SeqCst) || app.lens_review || app.lens.event.lock_ok().is_none();
    if let Some(at) = app.lens_dirty {
        if !busy && now - at > 3.0 && now - app.lens_auto_at > 20.0 {
            app.lens_dirty = None;
            app.lens_auto_at = now;
            scan::update(app.lens.clone(), ctx.clone(), true);
        }
    }
    let _ = root;
    // look again now and then: the watch only flags, it doesn't repaint
    ctx.request_repaint_after(std::time::Duration::from_secs(5));
}

/// Map what was picked in the switcher: the disk (its last map opens), a
/// volume or folder, or None for the folder dialog. Never call it while
/// holding `lens.result`: going back to the disk clears the map.
fn switch(app: &mut App, ctx: &egui::Context, to: Option<PathBuf>) {
    match to {
        Some(path) if path == Path::new("/") => open_disk(app),
        Some(path) => begin_at(app, ctx, path),
        None => choose_folder(app, ctx),
    }
}

/// Ask for a folder to map (the system folder dialog, off the UI thread).
fn choose_folder(app: &mut App, ctx: &egui::Context) {
    let (slot, ctx) = (app.lens_pick.clone(), ctx.clone());
    std::thread::spawn(move || {
        let script = format!("POSIX path of (choose folder with prompt \"{}\")", ts!("lens.map_prompt").replace(['\\', '"'], ""));
        let out = std::process::Command::new("osascript").args(["-e", script.as_str()]).output();
        let path = out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).map(PathBuf::from);
        if let Some(p) = path {
            *slot.lock_ok() = Some(p);
            ctx.request_repaint();
        }
    });
}

/// The first moments of a map, before the disk's top folders are listed:
/// what's being measured, with Stop. There is nothing to choose: Disk Map
/// always maps the whole startup disk.
fn start(app: &mut App, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let name = root_label(&app.lens_root);
    page_header(&p, r, ts!("lens.name"), &name, TEXT_DIM());
    let w = (r.width() - 48.0).min(520.0);
    let pr = Rect::from_center_size(Pos2::new(r.center().x, r.top() + PAGE_HEADER_H + 28.0 + 90.0), Vec2::new(w, 180.0));
    glass_card(&p, pr, R_CARD, 0.0, false);
    let bytes = app.lens.bytes.load(Ordering::Relaxed);
    let files = app.lens.files.load(Ordering::Relaxed);
    p.text(Pos2::new(pr.left() + 16.0, pr.top() + 14.0), Align2::LEFT_TOP, t!("lens.mapping", name = name), bold(13.0), TEXT_DIM());
    let g = p.layout_no_wrap(human_bytes(bytes), bold(28.0), TEXT());
    let gw = g.size().x;
    p.galley(Pos2::new(pr.left() + 16.0, pr.top() + 36.0), g, TEXT());
    p.text(Pos2::new(pr.left() + 24.0 + gw, pr.top() + 55.0), Align2::LEFT_CENTER, t!("lens.measured_files", files = group_digits(files)), font(13.0), TEXT_DIM());
    let track = Rect::from_min_size(Pos2::new(pr.left() + 16.0, pr.top() + 84.0), Vec2::new(pr.width() - 32.0, 6.0));
    p.rect_filled(track, 3.0, white(20));
    let t = ctx.input(|i| i.time);
    let f = ((t * 0.6).fract() as f32) * 1.35 - 0.35;
    let seg = Rect::from_min_max(Pos2::new(track.left() + track.width() * f.max(0.0), track.top()), Pos2::new(track.left() + track.width() * (f + 0.35).min(1.0), track.bottom()));
    p.with_clip_rect(track).rect_filled(seg, 3.0, tint::DISK_MAP());
    let cur = app.lens.current.lock_ok().clone();
    text_fit(&p, Pos2::new(pr.left() + 16.0, pr.top() + 102.0), Align2::LEFT_TOP, &tilde(Path::new(&cur)), mono(11.0), TEXT_FAINT(), pr.width() - 32.0);
    let sw = button_width(&p, ts!("lens.stop"), BTN_SECONDARY);
    if pill_button(ui, Rect::from_min_size(Pos2::new(pr.left() + 16.0, pr.bottom() - 44.0), Vec2::new(sw, 28.0)), ts!("lens.stop"), BTN_SECONDARY, true).clicked() {
        app.lens.stop.store(true, Ordering::SeqCst);
    }
    paragraph(&p, Pos2::new(pr.left(), pr.bottom() + 14.0), ts!("lens.fda_hint"), font(12.0), TEXT_FAINT(), pr.width());
    crate::kit::glass::animate(&ctx);
}

/// Map the whole startup disk from scratch.
fn begin(app: &mut App, ctx: &egui::Context) {
    begin_at(app, ctx, PathBuf::from("/"));
}

/// Map `root` from scratch.
fn begin_at(app: &mut App, ctx: &egui::Context, root: PathBuf) {
    if app.lens.running.load(Ordering::SeqCst) {
        return;
    }
    show_root(app, root.clone());
    // folders (not disks or Home) are offered again in the switcher
    if remember_folder(&mut app.lens_recent, &root, &home()) {
        super::save_prefs(app);
    }
    scan::start(app.lens.clone(), root, ctx.clone());
}

/// Put a mapped folder first in `recent` (no repeats, at most 5). Disks,
/// volumes and Home are in the switcher already: false, nothing changes.
fn remember_folder(recent: &mut Vec<PathBuf>, root: &Path, home: &Path) -> bool {
    let volume = root == Path::new("/") || root.parent() == Some(Path::new("/Volumes"));
    if volume || root == home {
        return false;
    }
    recent.retain(|r| r != root);
    recent.insert(0, root.to_path_buf());
    recent.truncate(5);
    true
}

/// A small ▾ that says a control opens a menu.
fn chevron_down(p: &egui::Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.4, color);
    p.line_segment([Pos2::new(c.x - 3.5, c.y - 1.8), Pos2::new(c.x, c.y + 1.8)], s);
    p.line_segment([Pos2::new(c.x, c.y + 1.8), Pos2::new(c.x + 3.5, c.y - 1.8)], s);
}

/// Point the page at a new place to map; nothing is measured yet.
fn show_root(app: &mut App, root: PathBuf) {
    app.lens_root = root.clone();
    app.lens_path = root;
    app.lens_selected.clear();
    app.lens_msg = None;
    app.lens_morph = None;
    app.lens_hues.clear();
    app.lens_skip = 0;
    app.lens_back.clear();
    app.lens_fwd.clear();
    app.lens_peek = false;
}

/// Back to the startup disk: its last map opens again if there is one.
fn open_disk(app: &mut App) {
    if app.lens.running.load(Ordering::SeqCst) {
        return;
    }
    show_root(app, PathBuf::from("/"));
    // freeing a big map takes a while: not on the UI thread
    let old = app.lens.result.write_ok().take();
    if let Some(old) = old {
        std::thread::spawn(move || drop(old));
    }
    *app.lens.event.lock_ok() = None;
    app.lens.loaded.store(false, Ordering::SeqCst);
    app.lens.partial.store(false, Ordering::SeqCst);
}

fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------- results

/// Colour of the top-level folder that holds `path`. Folders get their
/// colour the first time they're drawn and keep it while the map changes.
fn hue_of(app: &mut App, root: &Path, path: &Path) -> Color32 {
    let Some(first) = path.strip_prefix(root).ok().and_then(|r| r.components().next()) else { return OTHER() };
    let top = root.join(first);
    let i = match app.lens_hues.iter().position(|q| *q == top) {
        Some(i) => i,
        None => {
            app.lens_hues.push(top);
            app.lens_hues.len() - 1
        }
    };
    junk_ramp(i % 7)
}

fn pct(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        ((part as f64 / whole as f64) * 100.0).round() as u64
    }
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min + (b.min - a.min) * t, a.max + (b.max - a.max) * t)
}

/// The screen while mapping: the root's folders as rows with a size bar,
/// biggest first. Rows are re-sorted at most once a second and glide to
/// their new places; bars grow smoothly. Nothing jumps.
fn mapping(app: &mut App, ui: &mut Ui, r: Rect, root: &Path, rows: &[scan::LiveRow]) -> Act {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let now = ctx.input(|i| i.time);
    let mut act = Act::None;
    let root_label = if root == home() { ts!("lens.home").to_string() } else { scan::display_name(root) };
    let measured = app.lens.bytes.load(Ordering::Relaxed);

    // header: what's mapped and how much so far; progress and Stop
    let hr = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER_H));
    hairline(&p, hr.left_bottom(), hr.right_bottom());
    let g = p.layout_no_wrap(root_label, bold(13.0), TEXT());
    let tw = g.size().x;
    p.galley(Pos2::new(hr.left() + 24.0, hr.center().y - g.size().y / 2.0), g, TEXT());
    p.text(Pos2::new(hr.left() + 32.0 + tw, hr.center().y), Align2::LEFT_CENTER, human_bytes(measured), font(13.0), TEXT_FAINT());
    let nb = button_width(&p, ts!("lens.stop"), BTN_SECONDARY);
    let stop_r = Rect::from_min_size(Pos2::new(hr.right() - 20.0 - nb, hr.center().y - 15.0), Vec2::new(nb, 30.0));
    // the rows below show which folders are done; the header only counts files
    let hint_text = t!("lens.files", files = group_digits(app.lens.files.load(Ordering::Relaxed))).into_owned();
    let hint = p.text(Pos2::new(stop_r.left() - 14.0, hr.center().y), Align2::RIGHT_CENTER, hint_text, font(12.0), TEXT_DIM());
    let t = now as f32;
    let c = Pos2::new(hint.left() - 12.0, hr.center().y);
    crate::kit::glass::stroke_circle(&p, c, 5.0, Stroke::new(1.5, white(40)));
    let pts: Vec<Pos2> = (0..=12).map(|i| t * 5.0 + i as f32 * 0.15).map(|a| c + Vec2::angled(a) * 5.0).collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, tint::DISK_MAP())));
    if pill_button(ui, stop_r, ts!("lens.stop"), BTN_SECONDARY, true).clicked() {
        act = Act::Stop;
    }
    let track = Rect::from_min_size(Pos2::new(hr.left(), hr.bottom() - 2.0), Vec2::new(hr.width(), 2.0));
    let f = ((now * 0.5).fract() as f32) * 1.4 - 0.4;
    let seg = Rect::from_min_max(Pos2::new(track.left() + track.width() * f.max(0.0), track.top()), Pos2::new(track.left() + track.width() * (f + 0.4).min(1.0), track.bottom()));
    p.with_clip_rect(track).rect_filled(seg, 1.0, tint::DISK_MAP());

    // order: new folders join at the end; a full re-sort by size at most once a second
    for row in rows {
        if !app.lens_order.contains(&row.path) {
            app.lens_order.push(row.path.clone());
        }
    }
    if now - app.lens_order_at > 1.5 {
        let size = |pa: &PathBuf| rows.iter().find(|x| &x.path == pa).map(|x| x.bytes).unwrap_or(0);
        app.lens_order.sort_by_key(|pa| std::cmp::Reverse(size(pa)));
        app.lens_order_at = now;
    }

    // live bubbles, the same look as the finished map: folders keep their
    // order (re-sorted at most every 1.5 s), the packing is redone at most
    // once a second, and every bubble glides to its new place and size, so
    // nothing jumps while sizes grow
    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, hr.bottom() + 16.0), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - 16.0));
    let fr = Rect::from_min_max(Pos2::new(body.left(), body.bottom() - INFO_H), body.max);
    let area = Rect::from_min_max(body.min, Pos2::new(body.right(), fr.top() - 8.0));
    let ordered: Vec<&scan::LiveRow> = app.lens_order.iter().filter_map(|pa| rows.iter().find(|x| &x.path == pa)).collect();
    let (shown_rows, rest) = if ordered.len() > MAX_BUBBLES { (&ordered[..MAX_BUBBLES - 1], &ordered[MAX_BUBBLES - 1..]) } else { (&ordered[..], &ordered[..0]) };
    let more = PathBuf::from("\0more");
    let total: u64 = rows.iter().map(|x| x.bytes).sum::<u64>().max(1);
    let floor = (total as f64 * 0.004) as u64;
    let mut keys: Vec<PathBuf> = shown_rows.iter().map(|x| x.path.clone()).collect();
    let mut weights: Vec<u64> = shown_rows.iter().map(|x| x.bytes.max(floor).max(1)).collect();
    if !rest.is_empty() {
        keys.push(more.clone());
        weights.push(rest.iter().map(|x| x.bytes).sum::<u64>().max(floor).max(1));
    }
    let targets = LIVE_LAYOUT.with(|c| {
        let mut c = c.borrow_mut();
        let stale = match c.as_ref() {
            Some((at, a, k, _)) => now - at > 1.0 || *a != area || *k != keys,
            None => true,
        };
        if stale {
            // pack biggest first, then hand the circles back in `keys` order
            let mut idx: Vec<usize> = (0..weights.len()).collect();
            idx.sort_by_key(|&i| std::cmp::Reverse(weights[i]));
            let sorted: Vec<u64> = idx.iter().map(|&i| weights[i]).collect();
            let packed = pack(&sorted);
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for (at, rr) in &packed {
                lo = lo.min(*at - Vec2::splat(*rr));
                hi = hi.max(*at + Vec2::splat(*rr));
            }
            let span = (hi - lo).max(Vec2::splat(0.01));
            let sc = ((area.width() - 24.0) / span.x).min((area.height() - 24.0) / span.y);
            let mid = (lo + hi) / 2.0;
            let mut rects = vec![Rect::NOTHING; weights.len()];
            for (n, &i) in idx.iter().enumerate() {
                let (at, rr) = packed[n];
                rects[i] = Rect::from_center_size(area.center() + (at - mid) * sc, Vec2::splat(rr * sc * 2.0));
            }
            *c = Some((now, area, keys.clone(), rects));
        }
        c.as_ref().map(|x| x.3.clone()).unwrap_or_default()
    });
    let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let k = 1.0 - (-dt * 5.0).exp();
    let mut drawn = std::collections::HashMap::new();
    let pointer = crate::kit::modal::pointer(ui);
    let mut hovered: Option<(&scan::LiveRow, Color32)> = None;
    let mut hovered_at: Option<Rect> = None;
    let bp = p.with_clip_rect(area.expand(4.0));
    // place every bubble first, then paint big to small: while the packing
    // glides they can overlap, and a small one must stay on top of a big one
    let mut placed: Vec<(usize, Pos2, f32)> = Vec::new();
    for (n, key) in keys.iter().enumerate() {
        let Some(&target) = targets.get(n) else { continue };
        // glide from last frame; a new bubble grows from its centre
        let prev = app.lens_rows.get(key).map(|x| x.0).unwrap_or(Rect::from_center_size(target.center(), Vec2::ZERO));
        let rr = lerp_rect(prev, target, k);
        drawn.insert(key.clone(), (rr, 0.0));
        let (c, rad) = (rr.center(), rr.width().min(rr.height()) / 2.0);
        if rad >= 1.0 {
            placed.push((n, c, rad));
        }
    }
    placed.sort_by(|a, b| b.2.total_cmp(&a.2));
    // the pointer belongs to the smallest (topmost) bubble under it
    let hot = pointer.filter(|q| area.contains(*q)).and_then(|q| placed.iter().rev().find(|(_, c, rad)| (q - *c).length() <= *rad).map(|x| x.0));
    for &(n, c, rad) in &placed {
        if keys[n] == more {
            let bytes: u64 = rest.iter().map(|x| x.bytes).sum();
            live_bubble(&bp, c, rad, &t!("lens.n_more", n = rest.len()), bytes, OTHER(), rest.iter().any(|x| !x.done), false, now);
            continue;
        }
        let row = shown_rows[n];
        let hov = hot == Some(n);
        live_bubble(&bp, c, rad, &row.name, row.bytes, tint::DISK_MAP(), !row.done, hov, now);
        if hov {
            hovered = Some((row, tint::DISK_MAP()));
            hovered_at = Some(Rect::from_center_size(c, Vec2::splat(rad * 2.0)));
        }
    }
    // a finished folder can be opened while the rest is still measured
    let click = ui.interact(area, ui.scope_id().with("live-area"), Sense::click());
    if let (Some((row, _)), Some(at)) = (hovered, hovered_at) {
        if row.node.as_ref().is_some_and(|n| !n.children.is_empty()) {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            if click.clicked() {
                app.lens_peek = true;
                app.lens_path = row.path.clone();
                app.lens_back.clear();
                app.lens_fwd.clear();
                app.lens_morph = Some((Some(at), row.path.clone(), now));
            }
        }
    }
    if rows.is_empty() {
        p.text(area.center(), Align2::CENTER_CENTER, ts!("lens.looking"), font(13.0), TEXT_FAINT());
    }
    app.lens_rows = drawn;
    // rows glide at the animation pace (~60 fps), not as fast as the display
    // goes (asking for every frame ran a scan at 120 fps on a ProMotion Mac)
    crate::kit::glass::animate(&ctx);

    // under the list: what's under the pointer, or what's being measured
    match hovered {
        Some((row, hue)) => {
            p.rect_filled(Rect::from_center_size(Pos2::new(fr.left() + 5.0, fr.center().y), Vec2::splat(10.0)), 3.0, hue);
            let info = if row.done { t!("lens.share_measured", size = human_bytes(row.bytes), pct = pct(row.bytes, measured)).into_owned() } else { t!("lens.so_far", size = human_bytes(row.bytes)).into_owned() };
            let ir = p.text(Pos2::new(fr.right(), fr.center().y), Align2::RIGHT_CENTER, info, font(13.0), TEXT_DIM());
            text_fit(&p, Pos2::new(fr.left() + 18.0, fr.center().y), Align2::LEFT_CENTER, &tilde(&row.path), mono(12.0), TEXT_SOFT(), ir.left() - fr.left() - 34.0);
        }
        None => {
            let cur = app.lens.current.lock_ok().clone();
            text_fit(&p, Pos2::new(fr.left(), fr.center().y), Align2::LEFT_CENTER, &tilde(Path::new(&cur)), mono(12.0), TEXT_FAINT(), fr.width());
        }
    }
    act
}

thread_local! {
    /// The live bubbles' last packing: when, the area, which folders, their squares on screen.
    static LIVE_LAYOUT: std::cell::RefCell<Option<(f64, Rect, Vec<PathBuf>, Vec<Rect>)>> = const { std::cell::RefCell::new(None) };
}

/// One live bubble, in the finished map's look: an opaque tinted circle
/// with a hairline. Still measuring: fainter, with a spinner on top.
#[allow(clippy::too_many_arguments)]
fn live_bubble(p: &egui::Painter, c: Pos2, rad: f32, name: &str, bytes: u64, base: Color32, busy: bool, hov: bool, now: f64) {
    let fill = if busy { 0.10 } else { 0.2 } + if hov { 0.12 } else { 0.0 };
    // opaque: one bubble over another hides it instead of showing through
    p.circle_filled(c, rad, lerp_color(BG(), base, fill));
    crate::kit::glass::stroke_circle(p, c, rad, Stroke::new(1.0, with_alpha(base, if busy { 0.35 } else { 0.6 })));
    let txt = if busy { TEXT_SOFT() } else { TEXT() };
    if rad >= 30.0 {
        let (f1, f2) = (bold(if rad >= 46.0 { 12.0 } else { 11.0 }), font(if rad >= 46.0 { 11.0 } else { 10.0 }));
        let (y1, y2) = two_lines_y(p, c.y, &f1, &f2);
        text_fit(p, Pos2::new(c.x, y1), Align2::CENTER_CENTER, name, f1, txt, rad * 1.6);
        text_fit(p, Pos2::new(c.x, y2), Align2::CENTER_CENTER, &human_bytes(bytes), f2, TEXT_SOFT(), rad * 1.6);
    }
    if busy && rad >= 16.0 {
        let y = if rad >= 30.0 { c.y - 26.0 } else { c.y };
        spinner(p, Pos2::new(c.x, y), 5.0, now, base);
    }
}

/// The treemap screen. `pending` is set while mapping: those folders are
/// still being measured and the header shows progress with Stop.
/// Bubbles drawn on their own; the rest fold into one grey "Other" bubble.
const MAX_BUBBLES: usize = 18;
const LIST_W: f32 = 320.0;
const ROW_H: f32 = sp::BAR_ROW_H;
/// How long a zoom into a bubble takes, seconds.
const ZOOM_SECS: f64 = 0.35;

/// One bubble: where it sits in pack space, its radius, and what it is.
struct Bubble {
    at: Vec2,
    r: f32,
    kind: Kind,
}

/// What a bubble or a ring segment stands for.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// One of the folder's children, by index.
    Kid(usize),
    /// The smaller items and loose files, together.
    Other,
    /// The disk's free space.
    Free,
}

/// Circles for `sizes` (biggest first), packed around the centre: each new
/// circle goes to the free spot nearest the middle that touches one already
/// placed. Area follows size, so a folder twice as big looks twice as big.
fn pack(sizes: &[u64]) -> Vec<(Vec2, f32)> {
    let max = sizes.first().copied().unwrap_or(1).max(1) as f32;
    let radii: Vec<f32> = sizes.iter().map(|b| (*b as f32 / max).sqrt().max(0.08)).collect();
    let gap = 0.03;
    let mut out: Vec<(Vec2, f32)> = Vec::with_capacity(radii.len());
    for &r in &radii {
        if out.is_empty() {
            out.push((Vec2::ZERO, r));
            continue;
        }
        let mut best: Option<(f32, Vec2)> = None;
        for &(c, rc) in &out {
            for k in 0..48 {
                let a = k as f32 / 48.0 * std::f32::consts::TAU;
                let at = c + Vec2::angled(a) * (rc + r + gap);
                let d = at.length() + r;
                if best.is_some_and(|b| b.0 <= d) {
                    continue;
                }
                if out.iter().all(|&(o, ro)| (o - at).length() >= ro + r + gap - 1e-4) {
                    best = Some((d, at));
                }
            }
        }
        out.push((best.map(|b| b.1).unwrap_or(Vec2::ZERO), r));
    }
    out
}

thread_local! {
    /// The last pack, by folder (and how many bubbles), so it isn't redone every frame.
    static PACK: std::cell::RefCell<Option<(PathBuf, Vec<u64>, Vec<(Vec2, f32)>)>> = const { std::cell::RefCell::new(None) };
}

fn pack_cached(path: &Path, sizes: &[u64]) -> Vec<(Vec2, f32)> {
    PACK.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((p, s, v)) = c.as_ref() {
            if p == path && s == sizes {
                return v.clone();
            }
        }
        let v = pack(sizes);
        *c = Some((path.to_path_buf(), sizes.to_vec(), v.clone()));
        v
    })
}

fn is_folder(n: &Node) -> bool {
    !n.children.is_empty() || n.items > 1
}

/// The folder's contents as packed bubbles, one level, area by size.
#[allow(clippy::too_many_arguments)]
fn bubbles(app: &mut App, ui: &mut Ui, pa: Rect, node: &Node, here: &str, free: Option<(u64, u64)>, list_hover: Option<&Path>) -> MapOut {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let now = ctx.input(|i| i.time);
    let pointer = crate::kit::modal::pointer(ui);
    let resp = ui.interact(pa, ui.scope_id().with("map-area"), Sense::click());
    let mut out = MapOut { resp: resp.clone(), hover: None, go: None, up: false, other: false };
    let kids = &node.children;
    let keep = kids.len().min(MAX_BUBBLES);
    let other = kids[keep..].iter().map(|k| k.bytes).sum::<u64>() + node.other;
    let mut items: Vec<(Kind, u64)> = (0..keep).map(|i| (Kind::Kid(i), kids[i].bytes.max(1))).collect();
    if other > 0 {
        items.push((Kind::Other, other));
    }
    if let Some((f, _)) = free {
        items.push((Kind::Free, f));
    }
    // packing wants biggest first; Other and Free can be bigger than some folders
    items.sort_by_key(|x| std::cmp::Reverse(x.1));
    let sorted: Vec<u64> = items.iter().map(|x| x.1).collect();
    let bubbles: Vec<Bubble> = pack_cached(&node.path, &sorted).into_iter().zip(&items).map(|((at, r), it)| Bubble { at, r, kind: it.0 }).collect();
    // fit the pack into the area
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for b in &bubbles {
        lo = lo.min(b.at - Vec2::splat(b.r));
        hi = hi.max(b.at + Vec2::splat(b.r));
    }
    let span = (hi - lo).max(Vec2::splat(0.01));
    let s = ((pa.width() - 24.0) / span.x).min((pa.height() - 24.0) / span.y);
    let mid = (lo + hi) / 2.0;
    let place = |b: &Bubble| (pa.center() + (b.at - mid) * s, b.r * s);
    // zoom: the new level grows out of the bubble that was clicked
    let (from, start) = app.lens_morph.as_ref().map(|m| (m.0, m.2)).unwrap_or((None, now));
    let t = ((now - start) / ZOOM_SECS).clamp(0.0, 1.0) as f32;
    let k = 1.0 - (1.0 - t).powi(3);
    if t < 1.0 {
        ctx.request_repaint();
    }
    let fit_r = pa.width().min(pa.height()) / 2.0;
    let morph = |c: Pos2, rad: f32| -> (Pos2, f32) {
        match from {
            Some(fr) => {
                let z = fr.width() / 2.0 / fit_r;
                let c0 = fr.center() + (c - pa.center()) * z;
                (c0 + (c - c0) * k, rad * (z + (1.0 - z) * k))
            }
            None => (pa.center() + (c - pa.center()) * (0.94 + 0.06 * k), rad * (0.94 + 0.06 * k)),
        }
    };
    let bp = p.with_clip_rect(pa.expand(4.0));
    let alpha = if from.is_some() { k.max(0.15) } else { k };
    let mut tip: Option<(String, String)> = None;
    for b in &bubbles {
        let (c, rad) = morph(place(b).0, place(b).1);
        let kid = match b.kind {
            Kind::Kid(i) => Some(&kids[i]),
            _ => None,
        };
        let hot = t >= 1.0 && pointer.is_some_and(|q| (q - c).length() <= rad && pa.contains(q));
        let linked = kid.is_some_and(|kd| list_hover == Some(kd.path.as_path()));
        let on = kid.is_some_and(|kd| app.lens_selected.iter().any(|(q, _)| q == &kd.path));
        let base = match b.kind {
            Kind::Kid(_) => tint::DISK_MAP(),
            Kind::Other => OTHER(),
            Kind::Free => ACCENT(),
        };
        if b.kind == Kind::Free {
            bp.circle_filled(c, rad, with_alpha(base, (if hot { 0.12 } else { 0.06 }) * alpha));
            dashed_circle(&bp, c, rad, with_alpha(base, 0.5 * alpha));
        } else {
            bp.circle_filled(c, rad, with_alpha(base, (if hot || linked { 0.34 } else { 0.2 }) * alpha));
            crate::kit::glass::stroke_circle(&bp, c, rad, Stroke::new(if on { 2.0 } else { 1.0 }, if on { Color32::WHITE } else { with_alpha(base, 0.6 * alpha) }));
            if let Some(kd) = kid {
                reveal::circle(&bp, c, rad, app.ai.ring(&ctx, &kd.path.to_string_lossy()) * alpha);
            }
        }
        // label by room: icon, name and size; icon only; nothing
        let txt = with_alpha(TEXT(), alpha);
        match (kid, b.kind) {
            (Some(kd), _) if rad >= 46.0 => {
                let isz = (rad * 0.42).min(56.0);
                icon(&bp, if is_folder(kd) { Icon::Folder } else { Icon::Doc }, Rect::from_center_size(c - Vec2::new(0.0, isz * 0.45), Vec2::splat(isz)), with_alpha(ACCENT(), alpha));
                text_fit(&bp, c + Vec2::new(0.0, isz * 0.15), Align2::CENTER_TOP, &kd.name, bold(12.0), txt, rad * 1.6);
                text_fit(&bp, c + Vec2::new(0.0, isz * 0.15 + line_h(&bp, &bold(12.0)) + sp::LINE_GAP), Align2::CENTER_TOP, &human_bytes(kd.bytes), font(11.0), with_alpha(TEXT_SOFT(), alpha), rad * 1.6);
            }
            (Some(kd), _) if rad >= 30.0 => {
                let (y1, y2) = two_lines_y(&bp, c.y, &bold(11.0), &font(11.0));
                text_fit(&bp, Pos2::new(c.x, y1), Align2::CENTER_CENTER, &kd.name, bold(11.0), txt, rad * 1.7);
                text_fit(&bp, Pos2::new(c.x, y2), Align2::CENTER_CENTER, &human_bytes(kd.bytes), font(11.0), with_alpha(TEXT_SOFT(), alpha), rad * 1.7);
            }
            (Some(kd), _) if rad >= 16.0 => {
                let isz = (rad * 0.8).min(24.0);
                icon(&bp, if is_folder(kd) { Icon::Folder } else { Icon::Doc }, Rect::from_center_size(c, Vec2::splat(isz)), with_alpha(ACCENT(), alpha));
            }
            (None, Kind::Other) if rad >= 30.0 => {
                text_fit(&bp, c - Vec2::new(0.0, 1.0), Align2::CENTER_BOTTOM, ts!("lens.other"), bold(12.0), with_alpha(TEXT_DIM(), alpha), rad * 1.6);
                text_fit(&bp, c + Vec2::new(0.0, 1.0), Align2::CENTER_TOP, &human_bytes(other), font(11.0), with_alpha(TEXT_FAINT(), alpha), rad * 1.6);
            }
            (None, Kind::Free) if rad >= 30.0 => {
                let f = free.map(|x| x.0).unwrap_or(0);
                text_fit(&bp, c - Vec2::new(0.0, 1.0), Align2::CENTER_BOTTOM, ts!("lens.free"), bold(12.0), with_alpha(TEXT_DIM(), alpha), rad * 1.6);
                text_fit(&bp, c + Vec2::new(0.0, 1.0), Align2::CENTER_TOP, &human_bytes(f), font(11.0), with_alpha(TEXT_FAINT(), alpha), rad * 1.6);
            }
            _ => {}
        }
        if hot {
            match kid {
                Some(kd) => {
                    out.hover = Some((kd.path.clone(), kd.bytes));
                    tip = Some((kd.name.clone(), tn!("lens.tip_share", n = kd.items, size = human_bytes(kd.bytes), items = group_digits(kd.items), pct = pct(kd.bytes, node.bytes), here = here).into_owned()));
                    if !kd.children.is_empty() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            out.go = Some((kd.path.clone(), Some(Rect::from_center_size(c, Vec2::splat(rad * 2.0)))));
                        }
                    }
                }
                None if b.kind == Kind::Other => {
                    tip = Some((ts!("lens.other").into(), human_bytes(other)));
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    out.other = resp.clicked();
                }
                None => tip = Some((ts!("lens.free").into(), free_info(free))),
            }
        }
    }
    // hover card: name and numbers, beside the pointer
    if let (Some((name, info)), Some(q)) = (tip, pointer) {
        let w = p.layout_no_wrap(info.clone(), font(12.0), TEXT()).size().x.max(p.layout_no_wrap(name.clone(), bold(13.0), TEXT()).size().x) + 24.0;
        let mut tr = Rect::from_min_size(q + Vec2::new(14.0, 14.0), Vec2::new(w, 50.0));
        if tr.right() > pa.right() {
            tr = tr.translate(Vec2::new(-w - 28.0, 0.0));
        }
        if tr.bottom() > pa.bottom() {
            tr = tr.translate(Vec2::new(0.0, -78.0));
        }
        crate::kit::glass::float_shadow(&p, tr, 10.0, 1.0);
        p.rect_filled(tr, 10.0, POPOVER());
        p.rect_stroke(tr, 10.0, Stroke::new(1.0, crate::kit::glass::POPOVER_EDGE()), StrokeKind::Inside);
        p.text(tr.min + Vec2::new(12.0, 9.0), Align2::LEFT_TOP, name, bold(13.0), TEXT());
        p.text(tr.min + Vec2::new(12.0, 28.0), Align2::LEFT_TOP, info, font(12.0), TEXT_DIM());
    }
    out
}

/// "120 GB · 14 GB purgeable".
fn free_info(free: Option<(u64, u64)>) -> String {
    match free {
        Some((f, pg)) if pg >= 1_000_000_000 => t!("lens.purgeable", free = human_bytes(f), purgeable = human_bytes(pg)).into_owned(),
        Some((f, _)) => human_bytes(f),
        None => String::new(),
    }
}

/// A dashed circle outline (free space: room, not a thing).
fn dashed_circle(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let n = ((r * std::f32::consts::TAU / 8.0) as usize).clamp(12, 160) & !1;
    for k in (0..n).step_by(2) {
        let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
        p.line_segment([c + Vec2::angled(a0) * r, c + Vec2::angled(a1) * r], Stroke::new(1.0, color));
    }
}

/// How many rings around the folder.
const RING_LEVELS: usize = 3;
/// The folder's own disc in the middle, as a part of the radius.
const RING_CENTER: f32 = 0.22;
/// Slices narrower than this (radians, ~2°) join the ring's "Other" slice.
const RING_MIN_ANGLE: f32 = 0.035;

/// One slice of a ring.
struct Seg {
    kind: Kind,
    path: Option<PathBuf>,
    name: String,
    bytes: u64,
    a0: f32,
    a1: f32,
    level: usize,
    hue: Color32,
    drill: bool,
}

/// Slices for `node`'s contents between angles `a0`..`a1` on ring `level`,
/// and for their contents on the rings outside.
#[allow(clippy::too_many_arguments)]
fn ring_segs(app: &mut App, root: &Path, node: &Node, a0: f32, a1: f32, level: usize, extra: u64, out: &mut Vec<Seg>) {
    let whole = (node.bytes + extra).max(1) as f32;
    let per = (a1 - a0) / whole;
    let mut a = a0;
    let mut small = node.other;
    for k in &node.children {
        let span = k.bytes as f32 * per;
        if span < RING_MIN_ANGLE {
            small += k.bytes;
            continue;
        }
        let hue = hue_of(app, root, &k.path);
        out.push(Seg { kind: Kind::Kid(0), path: Some(k.path.clone()), name: k.name.clone(), bytes: k.bytes, a0: a, a1: a + span, level, hue, drill: !k.children.is_empty() });
        if level + 1 < RING_LEVELS && !k.children.is_empty() {
            ring_segs(app, root, k, a, a + span, level + 1, 0, out);
        }
        a += span;
    }
    let span = small as f32 * per;
    if span >= RING_MIN_ANGLE / 4.0 {
        out.push(Seg { kind: Kind::Other, path: None, name: ts!("lens.other").into(), bytes: small, a0: a, a1: a + span, level, hue: TEXT_FAINT(), drill: false });
    }
}

/// An annular slice as triangles (it may be wider than half a turn, so
/// it's not convex).
fn ring_mesh(mesh: &mut egui::Mesh, c: Pos2, r0: f32, r1: f32, a0: f32, a1: f32, color: Color32) {
    if a1 <= a0 || r1 <= r0 {
        return;
    }
    let steps = (((a1 - a0) / 0.02).ceil() as usize).max(1);
    let base = mesh.vertices.len() as u32;
    for i in 0..=steps {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        let d = Vec2::angled(a);
        mesh.colored_vertex(c + d * r0, color);
        mesh.colored_vertex(c + d * r1, color);
    }
    for i in 0..steps as u32 {
        let v = base + i * 2;
        mesh.add_triangle(v, v + 1, v + 2);
        mesh.add_triangle(v + 1, v + 3, v + 2);
    }
}

/// The folder as rings: its contents on the first ring, theirs on the next,
/// angle by size; the folder itself in the middle (click it to go up).
/// Hovering a slice lights up its branch and names it in the middle.
#[allow(clippy::too_many_arguments)]
fn rings(app: &mut App, ui: &mut Ui, pa: Rect, root: &Node, node: &Node, here: &str, free: Option<(u64, u64)>, list_hover: Option<&Path>, can_up: bool) -> MapOut {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let now = ctx.input(|i| i.time);
    let resp = ui.interact(pa, ui.scope_id().with("map-area"), Sense::click());
    let mut out = MapOut { resp: resp.clone(), hover: None, go: None, up: false, other: false };
    // grow in on every new folder
    let start = app.lens_morph.as_ref().map(|m| m.2).unwrap_or(now);
    let t = ((now - start) / ZOOM_SECS).clamp(0.0, 1.0) as f32;
    let k = 1.0 - (1.0 - t).powi(3);
    if t < 1.0 {
        ctx.request_repaint();
    }
    let c = pa.center();
    let big_r = (pa.width().min(pa.height()) / 2.0 - 12.0).max(40.0) * (0.92 + 0.08 * k);
    let r_in = big_r * RING_CENTER;
    let ring_w = (big_r * 0.98 - r_in) / RING_LEVELS as f32;
    let (lo, hi) = (-FRAC_PI_2, -FRAC_PI_2 + TAU);

    let mut segs = Vec::new();
    let extra = free.map(|x| x.0).unwrap_or(0);
    ring_segs(app, &root.path, node, lo, hi, 0, extra, &mut segs);
    if let Some((f, _)) = free {
        let a0 = segs.iter().filter(|s| s.level == 0).map(|s| s.a1).fold(lo, f32::max);
        let span = f as f32 * (hi - lo) / (node.bytes + f).max(1) as f32;
        segs.push(Seg { kind: Kind::Free, path: None, name: ts!("lens.free").into(), bytes: f, a0, a1: a0 + span, level: 0, hue: ACCENT(), drill: false });
    }

    // what's under the pointer
    let pointer = crate::kit::modal::pointer(ui).filter(|q| pa.contains(*q) && t >= 1.0);
    let mut hot: Option<usize> = None;
    let mut on_center = false;
    if let Some(q) = pointer {
        let d = q - c;
        let dist = d.length();
        if dist <= r_in {
            on_center = true;
        } else {
            let lvl = ((dist - r_in) / ring_w).floor() as usize;
            let mut a = d.y.atan2(d.x);
            if a < lo {
                a += TAU;
            }
            hot = segs.iter().position(|s| s.level == lvl && a >= s.a0 && a < s.a1);
        }
    }
    // the lit branch: the hovered slice (or the list's row), what holds it and what's in it
    let lit: Option<PathBuf> = hot.and_then(|i| segs[i].path.clone()).or_else(|| list_hover.map(|x| x.to_path_buf()));
    let mut mesh = egui::Mesh::default();
    let gap_px = 1.0;
    for (i, s) in segs.iter().enumerate() {
        let r0 = r_in + ring_w * s.level as f32 + gap_px;
        let r1 = r_in + ring_w * (s.level + 1) as f32 - gap_px;
        let ga = gap_px / r1.max(1.0);
        let (a0, a1) = (s.a0 + ga, s.a1 - ga);
        let depth_a = [0.85, 0.62, 0.46][s.level.min(2)];
        let in_branch = match (&lit, &s.path) {
            (Some(l), Some(sp)) => l.starts_with(sp) || sp.starts_with(l),
            (Some(_), None) => hot == Some(i),
            (None, _) => true,
        };
        let a = depth_a * if in_branch { 1.0 } else { 0.3 } * k.max(0.2);
        let col = match s.kind {
            Kind::Free => with_alpha(s.hue, if hot == Some(i) { 0.22 } else { 0.1 } * k.max(0.2)),
            Kind::Other => with_alpha(s.hue, 0.35 * a),
            Kind::Kid(_) => with_alpha(s.hue, a),
        };
        ring_mesh(&mut mesh, c, r0, r1, a0, a1, col);
    }
    p.add(egui::Shape::mesh(mesh));
    // picked items: a white edge on the outside of their slice
    for s in &segs {
        if s.path.as_ref().is_some_and(|sp| app.lens_selected.iter().any(|(q, _)| q == sp)) {
            let r1 = r_in + ring_w * (s.level + 1) as f32 - 1.0;
            let n = (((s.a1 - s.a0) / 0.02).ceil() as usize).max(2);
            let pts: Vec<Pos2> = (0..=n).map(|j| c + Vec2::angled(s.a0 + (s.a1 - s.a0) * j as f32 / n as f32) * r1).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.5, Color32::WHITE)));
        }
    }
    // the slice an AI Check tip was clicked for: a ring around it that fades
    if let Some((key, kr)) = app.ai.revealed(&ctx) {
        for s in segs.iter().filter(|s| s.path.as_ref().is_some_and(|sp| sp.to_string_lossy() == key)) {
            let r0 = r_in + ring_w * s.level as f32 + gap_px;
            let r1 = r_in + ring_w * (s.level + 1) as f32 - gap_px;
            let ga = gap_px / r1.max(1.0);
            let (a0, a1) = (s.a0 + ga, s.a1 - ga);
            let n = (((a1 - a0) / 0.02).ceil() as usize).max(2);
            let mut pts: Vec<Pos2> = (0..=n).map(|j| c + Vec2::angled(a0 + (a1 - a0) * j as f32 / n as f32) * r1).collect();
            pts.extend((0..=n).rev().map(|j| c + Vec2::angled(a0 + (a1 - a0) * j as f32 / n as f32) * r0));
            p.add(egui::Shape::closed_line(pts, Stroke::new(reveal::RING_W, with_alpha(AI(), kr))));
        }
    }
    // names on the slices that have room
    for s in &segs {
        let rm = r_in + ring_w * (s.level as f32 + 0.5);
        let arc = (s.a1 - s.a0) * rm;
        if arc < 54.0 || ring_w < 30.0 || s.kind == Kind::Other && s.path.is_none() && arc < 80.0 {
            continue;
        }
        let mid = (s.a0 + s.a1) / 2.0;
        let at = c + Vec2::angled(mid) * rm;
        // the slice's room for flat text: across the ring where it runs
        // sideways, along it where it runs up and down
        let (cs, sn) = (mid.cos().abs(), mid.sin().abs());
        let w = (ring_w * cs + arc * sn) * 0.8;
        let h = (ring_w * sn + arc * cs) * 0.8;
        let size = human_bytes(s.bytes);
        let name_w = p.layout_no_wrap(s.name.clone(), bold(11.0), TEXT()).size().x;
        let size_w = p.layout_no_wrap(size.clone(), font(11.0), TEXT()).size().x;
        // a label shows whole or not at all (hover names every slice)
        if name_w.max(size_w) > w || h < 30.0 {
            continue;
        }
        let ta = k.max(0.2);
        p.text(at - Vec2::new(0.0, 1.0), Align2::CENTER_BOTTOM, &s.name, bold(11.0), with_alpha(TEXT(), ta));
        p.text(at + Vec2::new(0.0, 1.0), Align2::CENTER_TOP, size, font(11.0), with_alpha(TEXT_SOFT(), ta));
    }
    // the middle: the folder, or what's hovered
    let center_hot = on_center && can_up;
    p.circle_filled(c, r_in - 2.0, white(if center_hot { 24 } else { 12 }));
    crate::kit::glass::stroke_circle(&p, c, r_in - 2.0, Stroke::new(1.0, white(31)));
    let (title, size, sub) = match hot.map(|i| &segs[i]) {
        Some(s) if s.kind == Kind::Free => (s.name.clone(), human_bytes(s.bytes), free_info(free).split(" · ").nth(1).unwrap_or_default().to_string()),
        Some(s) => (s.name.clone(), human_bytes(s.bytes), format!("{}%", pct(s.bytes, node.bytes))),
        None => (here.to_string(), human_bytes(node.bytes), String::new()),
    };
    let tw = r_in * 1.7;
    text_fit(&p, c - Vec2::new(0.0, 10.0), Align2::CENTER_BOTTOM, &title, bold(13.0), TEXT(), tw);
    text_fit(&p, c - Vec2::new(0.0, 8.0), Align2::CENTER_TOP, &size, font(13.0), TEXT_SOFT(), tw);
    if !sub.is_empty() {
        text_fit(&p, c + Vec2::new(0.0, 10.0), Align2::CENTER_TOP, &sub, font(11.0), TEXT_FAINT(), tw);
    }
    if center_hot {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            out.up = true;
        }
    }
    if let Some(s) = hot.map(|i| &segs[i]).filter(|s| s.kind == Kind::Other && s.path.is_none()) {
        let _ = s;
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        out.other = resp.clicked();
    }
    if let Some(s) = hot.map(|i| &segs[i]) {
        if let Some(sp) = &s.path {
            out.hover = Some((sp.clone(), s.bytes));
            if s.drill {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    out.go = Some((sp.clone(), None));
                }
            }
        }
    }
    out
}

/// A file on the map: no contents, one item, nothing folded into it.
fn is_file(n: &Node) -> bool {
    n.children.is_empty() && n.items == 1 && n.other == 0
}

/// The biggest files anywhere under `node`, biggest first (only files big
/// enough to be kept on the map, 8 MB and up).
pub(super) fn biggest_files(node: &Node, max: usize) -> Vec<&Node> {
    fn walk<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
        for c in &n.children {
            if is_file(c) {
                out.push(c);
            } else {
                walk(c, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out.sort_by_key(|n| std::cmp::Reverse(n.bytes));
    out.truncate(max);
    out
}

/// Folder and file rows: icon and name x from the content edge (12 in
/// from the lit edge, H9), after the 15 pt checkbox.
const ROW_ICON_X: f32 = 34.0;
const ROW_TEXT_X: f32 = 50.0;

/// The biggest files under the folder as a flat list: name, where it is,
/// size and a bar; pick, open the menu, or Quick Look like anywhere else.
fn files(app: &mut App, ui: &mut Ui, pa: Rect, node: &Node) -> MapOut {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let resp = ui.interact(pa, ui.scope_id().with("map-area"), Sense::click());
    let mut out = MapOut { resp: resp.clone(), hover: None, go: None, up: false, other: false };
    let list = biggest_files(node, 200);
    glass_card(&p, pa, R_CARD, 0.0, false);
    if list.is_empty() {
        p.text(pa.center(), Align2::CENTER_CENTER, ts!("lens.no_big"), font(13.0), TEXT_FAINT());
        return out;
    }
    let top = list[0].bytes.max(1);
    let row_h = 40.0;
    // rows scroll to the card's edge (1 pt in, past the hairline); the 8 pt
    // breathing room sits inside the scroll, above the first and below the last row
    let inner = pa.shrink2(Vec2::new(sp::XS, 1.0));
    let mut toggle: Option<(PathBuf, u64)> = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt("map-files").auto_shrink([false, false]).show(&mut child, |ui| {
        let w = ui.available_width();
        // 8 from the card edge to the first row (the viewport starts 1 in);
        // below the last row the row spacing already counts toward the 8
        ui.add_space(7.0);
        let pad = 7.0 - ui.spacing().item_spacing.y;
        for (i, f) in list.iter().enumerate() {
            let (rr, rresp) = ui.allocate_exact_size(Vec2::new(w, row_h), Sense::click());
            let lp = ui.painter();
            if rresp.hovered() {
                lp.rect_filled(rr, R_SMALL, white(ROW_HOVER));
                out.hover = Some((f.path.clone(), f.bytes));
            }
            let on = app.lens_selected.iter().any(|(q, _)| q == &f.path);
            // H9: content 12 in from the lit edge, both sides
            let x = rr.left() + sp::M;
            let xr = rr.right() - sp::M;
            let cb = Rect::from_center_size(Pos2::new(x + 7.5, rr.center().y), Vec2::splat(15.0));
            if trashable(&f.path) && checkbox(ui, cb, ui.scope_id().with(("file-cb", i)), on, true).clicked() {
                toggle = Some((f.path.clone(), f.bytes));
            }
            let lp = ui.painter();
            icon(lp, Icon::Doc, Rect::from_center_size(Pos2::new(x + ROW_ICON_X, rr.center().y), Vec2::splat(16.0)), TEXT_DIM());
            let sr = lp.text(Pos2::new(xr, rr.top() + 13.0), Align2::RIGHT_CENTER, human_bytes(f.bytes), font(12.0), TEXT_SOFT());
            let tx = x + ROW_TEXT_X;
            text_fit(lp, Pos2::new(tx, rr.top() + 13.0), Align2::LEFT_CENTER, &f.name, font(13.0), TEXT(), sr.left() - 10.0 - tx);
            let at = f.path.parent().map(tilde).unwrap_or_default();
            text_fit(lp, Pos2::new(tx, rr.top() + 28.0), Align2::LEFT_CENTER, &at, mono(11.0), TEXT_FAINT(), (xr - tx) * 0.6);
            let bw = (rr.width() * 0.3) * (f.bytes as f32 / top as f32);
            lp.rect_filled(Rect::from_min_size(Pos2::new(xr - bw, rr.top() + 26.0), Vec2::new(bw, 3.0)), 1.5, with_alpha(ACCENT(), 0.6));
            menu::context(&rresp).show(|ui| {
                if let Some(a) = item_menu(ui, &f.path, false) {
                    app.lens_menu_act = Some((a, f.path.clone(), f.bytes));
                }
            });
            if rresp.double_clicked() {
                quick_look(&f.path);
            }
        }
        ui.add_space(pad);
    });
    let _ = ctx;
    if let Some(t) = toggle {
        pick(app, t);
    }
    out
}

/// Pick or unpick an item for the Trash; a folder replaces anything picked
/// inside it.
fn pick(app: &mut App, (path, b): (PathBuf, u64)) {
    if let Some(i) = app.lens_selected.iter().position(|(q, _)| q == &path) {
        app.lens_selected.remove(i);
    } else {
        app.lens_selected.retain(|(q, _)| !q.starts_with(&path));
        if !app.lens_selected.iter().any(|(q, _)| path.starts_with(q)) {
            app.lens_selected.push((path, b));
        }
    }
}

/// Everything in the folder, in a sheet over the map: what the map folds
/// into "Other" can be picked or opened here.
fn other_sheet(app: &mut App, ui: &mut Ui, r: Rect, node: &Node) -> Option<PathBuf> {
    crate::kit::modal::layer(ui, "lens-other", |ui| other_sheet_body(app, ui, r, node))
}

fn other_sheet_body(app: &mut App, ui: &mut Ui, r: Rect, node: &Node) -> Option<PathBuf> {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let scrim = ui.interact(r, ui.scope_id().with("other-scrim"), Sense::click());
    p.rect_filled(r, CornerRadius { ne: 12, se: 12, ..Default::default() }, crate::kit::glass::SCRIM());
    let rows = node.children.len() + usize::from(node.other > 0);
    let h = (70.0 + rows as f32 * 34.0 + 16.0).min(r.height() - 80.0);
    let m = Rect::from_center_size(r.center(), Vec2::new(520.0, h));
    crate::kit::glass::float_shadow(&p, m, 14.0, 1.0);
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, crate::kit::glass::POPOVER_EDGE()), StrokeKind::Inside);
    text_fit(&p, m.min + Vec2::new(22.0, 20.0), Align2::LEFT_TOP, &node.name, bold(15.0), TEXT(), m.width() - 90.0);
    p.text(m.min + Vec2::new(22.0, 44.0), Align2::LEFT_TOP, tn!("lens.size_items", n = node.items, size = human_bytes(node.bytes), items = group_digits(node.items)), font(12.0), TEXT_DIM());
    let xr = Rect::from_center_size(Pos2::new(m.right() - 26.0, m.top() + 30.0), Vec2::splat(24.0));
    let mut close = round_button(ui, xr, Icon::Close, ts!("lens.close")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
    if scrim.clicked() && ctx.pointer_interact_pos().is_some_and(|q| !m.contains(q)) {
        close = true;
    }
    // rows scroll to the popover's edge; the 8 pt end gap sits inside the scroll
    let list = Rect::from_min_max(m.min + Vec2::new(10.0, 70.0), m.max - Vec2::new(10.0, 1.0));
    let mut go = None;
    let mut toggle = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt("other-list").auto_shrink([false, false]).show(&mut child, |ui| {
        let w = ui.available_width();
        for (i, k) in node.children.iter().enumerate() {
            let (rr, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
            let lp = ui.painter().clone();
            if resp.hovered() {
                lp.rect_filled(rr, R_SMALL, white(ROW_HOVER));
            }
            let on = app.lens_selected.iter().any(|(q, _)| q == &k.path);
            // H9: content 12 in from the lit edge
            let x = rr.left() + sp::M;
            let cb = Rect::from_center_size(Pos2::new(x + 7.5, rr.center().y), Vec2::splat(15.0));
            if trashable(&k.path) && checkbox(ui, cb, ui.scope_id().with(("other-cb", i)), on, true).clicked() {
                toggle = Some((k.path.clone(), k.bytes));
            }
            let folder = is_folder(k);
            icon(&lp, if folder { Icon::Folder } else { Icon::Doc }, Rect::from_center_size(Pos2::new(x + ROW_ICON_X, rr.center().y), Vec2::splat(16.0)), if folder { ACCENT() } else { TEXT_DIM() });
            let sr = lp.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(k.bytes), font(12.0), TEXT_SOFT());
            text_fit(&lp, Pos2::new(x + ROW_TEXT_X, rr.center().y), Align2::LEFT_CENTER, &k.name, font(13.0), TEXT(), sr.left() - 10.0 - (x + ROW_TEXT_X));
            if !k.children.is_empty() {
                if resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    go = Some(k.path.clone());
                }
            }
            menu::context(&resp).show(|ui| {
                if let Some(a) = item_menu(ui, &k.path, !k.children.is_empty()) {
                    app.lens_menu_act = Some((a, k.path.clone(), k.bytes));
                }
            });
        }
        if node.other > 0 {
            let (rr, _) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::hover());
            let lp = ui.painter();
            let sr = lp.text(Pos2::new(rr.right() - 10.0, rr.center().y), Align2::RIGHT_CENTER, human_bytes(node.other), font(12.0), TEXT_FAINT());
            text_fit(lp, Pos2::new(rr.left() + 58.0, rr.center().y), Align2::LEFT_CENTER, ts!("lens.small_files"), font(13.0), TEXT_DIM(), sr.left() - rr.left() - 68.0);
        }
        // 8 from the last row to the popover edge (the viewport ends 1 in)
        ui.add_space(7.0 - ui.spacing().item_spacing.y);
    });
    if let Some(t) = toggle {
        pick(app, t);
    }
    if close || go.is_some() || app.lens_menu_act.is_some() {
        app.lens_other = false;
    }
    go
}

/// The finished map (or, with `peek`, one finished folder while the rest is
/// still measured: `peek` is how many top folders are done, of how many).
fn results(app: &mut App, ui: &mut Ui, r: Rect, root: &Node, peek: Option<(usize, usize)>) -> Act {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let mut act = Act::None;
    if scan::find(root, &app.lens_path).is_none() {
        app.lens_path = root.path.clone();
    }
    let node = scan::find(root, &app.lens_path).unwrap_or(root);
    let mut go_to: Option<(PathBuf, Option<Rect>)> = None;
    // breadcrumbs start at what was mapped (in a peek, above `root`)
    let base = if peek.is_some() { app.lens_root.clone() } else { root.path.clone() };
    let root_label = root_label(&base);
    let here = if node.path == base { root_label.clone() } else { node.name.clone() };

    // header: back, forward, breadcrumbs, view, when it was mapped, Update
    let hr = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER_H));
    hairline(&p, hr.left_bottom(), hr.right_bottom());
    let can_up = node.path != root.path || peek.is_some();
    // SP17 H1: the page title first, like every page; back, forward and
    // the place switcher after it
    let tx = p.text(Pos2::new(hr.left() + sp::PAGE_EDGE, hr.center().y), Align2::LEFT_CENTER, ts!("lens.name"), crate::kit::glass::ty::lead(), TEXT()).right() + sp::M;
    let mut nav: Option<Nav> = None;
    for (k, (ic, on, tip, go)) in [(Icon::Back, !app.lens_back.is_empty() || peek.is_some(), ts!("lens.back"), Nav::Back), (Icon::Forward, !app.lens_fwd.is_empty(), ts!("lens.forward"), Nav::Forward)].into_iter().enumerate() {
        let br = Rect::from_min_size(Pos2::new(tx + k as f32 * 36.0, hr.center().y - 16.0), Vec2::splat(32.0));
        let resp = ui.add_enabled_ui(on, |ui| crate::kit::glass::icon_button(ui, br, ic, tip)).inner;
        if resp.clicked() && on {
            nav = Some(go);
        }
    }
    // keys and mouse side buttons: ⌘[ back, ⌘] forward, ⌘↑ up
    ui.input(|i| {
        if i.modifiers.command && i.key_pressed(egui::Key::OpenBracket) || i.pointer.button_pressed(egui::PointerButton::Extra1) {
            nav = Some(Nav::Back);
        } else if i.modifiers.command && i.key_pressed(egui::Key::CloseBracket) || i.pointer.button_pressed(egui::PointerButton::Extra2) {
            nav = Some(Nav::Forward);
        } else if i.modifiers.command && i.key_pressed(egui::Key::ArrowUp) && can_up {
            nav = Some(Nav::Up);
        }
    });
    // what's mapped, as one switcher: disks, Home, recent folders, any folder
    let volume = base == Path::new("/") || base.parent() == Some(Path::new("/Volumes"));
    let tg = p.layout_no_wrap(root_label.clone(), bold(13.0), TEXT());
    let loc = Rect::from_min_size(Pos2::new(tx + 72.0 + sp::M, hr.center().y - 16.0), Vec2::new((tg.size().x + 56.0).min(240.0), 32.0));
    let lr_resp = ui.interact(loc, ui.scope_id().with("map-where"), if peek.is_none() { Sense::click() } else { Sense::hover() });
    let at_top = node.path == base;
    if peek.is_none() && lr_resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut pick_root: Option<Option<PathBuf>> = None;
    menu::popup(&lr_resp).show(|ui| {
        for (name, path) in locations() {
            if menu::item(ui, &name).clicked() {
                pick_root = Some(Some(path));
            }
        }
        let recent: Vec<PathBuf> = app.lens_recent.iter().filter(|r| r.is_dir()).cloned().collect();
        if !recent.is_empty() {
            menu::separator(ui);
            for path in recent {
                if menu::item(ui, &tilde(&path)).clicked() {
                    pick_root = Some(Some(path));
                }
            }
        }
        menu::separator(ui);
        if menu::item(ui, ts!("lens.choose_folder_menu")).clicked() {
            pick_root = Some(None);
        }
    });
    let mut crumbs: Vec<(String, PathBuf)> = vec![(root_label.clone(), base.clone())];
    if let Ok(rel) = node.path.strip_prefix(&base) {
        let mut acc = base.clone();
        for c in rel.components() {
            acc.push(c);
            crumbs.push((c.as_os_str().to_string_lossy().into_owned(), acc.clone()));
        }
    }
    let updating = app.lens.updating.load(Ordering::SeqCst) && !app.lens.quiet.load(Ordering::SeqCst);
    let partial = app.lens.partial.load(Ordering::SeqCst);
    // Stop while anything runs; a stopped map is mapped again from scratch
    let busy = peek.is_some() || updating;
    // SP16: the page action at the right edge, one fixed width for every
    // label so the tabs next to it never move; while a map runs it reads
    // Stop with a spinner (B8), and its tip says how far it got
    let btn = if busy { ts!("lens.stop") } else { ts!("lens.update") };
    let nb = [ts!("lens.stop"), ts!("lens.update")].iter().map(|l| icon_pill_width(&p, l, BTN_SECONDARY)).fold(0.0, f32::max);
    let new_r = Rect::from_min_size(Pos2::new(hr.right() - sp::PAGE_EDGE - nb, hr.center().y - sp::HEADER_CONTROL_H / 2.0), Vec2::new(nb, sp::HEADER_CONTROL_H));
    let progress = match (updating, peek) {
        (_, Some((done, all))) => t!("lens.n_of_folders", done = done, all = all).into_owned(),
        (true, _) => ts!("lens.updating").to_string(),
        _ => String::new(),
    };
    // the view switch right next to the action (SP16)
    let seg_w = crate::kit::glass::tabs_width(&p, &[ts!("lens.view.bubbles"), ts!("lens.view.rings"), ts!("lens.view.files")]);
    let seg_r = Rect::from_min_size(Pos2::new(new_r.left() - sp::M - seg_w, hr.center().y - sp::HEADER_CONTROL_H / 2.0), Vec2::new(seg_w, sp::HEADER_CONTROL_H));
    if let Some(i) = segmented(ui, seg_r, ui.scope_id().with("map-view"), &[ts!("lens.view.bubbles"), ts!("lens.view.rings"), ts!("lens.view.files")], app.lens_view as usize) {
        app.lens_view = i as u8;
        super::save_prefs(app);
        app.lens_morph = Some((None, node.path.clone(), ctx.input(|i| i.time)));
    }
    // AI Check: a chip left of the view switch, once the map is final and whole
    let ai_data = !busy && !partial && !big_folders(root).is_empty();
    let cw = if !busy && !partial { aicheck::ui::chip_width(&app.ai, ui, ai_data) } else { 0.0 };
    let chip_at = Pos2::new(seg_r.left() - sp::M - cw, hr.center().y);
    let crumbs_end = if cw > 0.0 { chip_at.x - 12.0 } else { seg_r.left() - 16.0 };
    // the switcher gives way to the chip at narrow widths: its name is cut
    // (a name the user brings, R10), never drawn under the chip
    let loc = loc.with_max_x(loc.right().min(crumbs_end + 4.0).max(loc.left() + 64.0));
    p.rect_filled(loc, R_SMALL, white(if peek.is_some() { 8 } else if lr_resp.hovered() { 32 } else if at_top { 20 } else { 14 }));
    icon(&p, if volume { Icon::Disk } else { Icon::Folder }, Rect::from_center_size(Pos2::new(loc.left() + 16.0, loc.center().y), Vec2::splat(16.0)), if peek.is_none() { ACCENT() } else { TEXT_MUTED() });
    text_fit(&p, Pos2::new(loc.left() + 30.0, loc.center().y), Align2::LEFT_CENTER, &root_label, bold(13.0), if at_top { TEXT() } else { TEXT_DIM() }, loc.width() - 50.0);
    chevron_down(&p, Pos2::new(loc.right() - 12.0, loc.center().y), TEXT_DIM());
    let mut x = loc.right() + 2.0;
    // no room for "› … › name" before the AI chip: show no crumbs at all,
    // never a separator or "…" under the chip
    if x + 10.0 + 26.0 + 40.0 > crumbs_end {
        crumbs.truncate(1);
    }
    if crumbs.len() > 1 {
        p.text(Pos2::new(x + 4.0, hr.center().y), Align2::CENTER_CENTER, "›", font(13.0), TEXT_MUTED());
        x += 10.0;
    }
    let n_crumbs = crumbs.len();
    // the first crumb is the switcher itself. When the path doesn't fit, the
    // folders in between give way ("…") so the one you're in always shows
    let limit = crumbs_end;
    let crumb_w = |k: usize| p.layout_no_wrap(crumbs[k].0.clone(), if k + 1 == n_crumbs { bold(13.0) } else { font(13.0) }, TEXT()).size().x + 16.0 + 12.0;
    let mut first = 1;
    while first + 1 < n_crumbs && x + (first..n_crumbs).map(crumb_w).sum::<f32>() + if first > 1 { 22.0 } else { 0.0 } > limit {
        first += 1;
    }
    if first > 1 {
        p.text(Pos2::new(x + 8.0, hr.center().y), Align2::CENTER_CENTER, "…", font(13.0), TEXT_MUTED());
        x += 16.0;
        p.text(Pos2::new(x + 4.0, hr.center().y), Align2::CENTER_CENTER, "›", font(13.0), TEXT_MUTED());
        x += 10.0;
    }
    for (k, (label, path)) in crumbs.iter().enumerate().skip(first) {
        let last = k + 1 == n_crumbs;
        let f = if last { bold(13.0) } else { font(13.0) };
        let tw = (p.layout_no_wrap(label.clone(), f.clone(), TEXT()).size().x).min(limit - x - 16.0);
        if tw < 24.0 {
            break;
        }
        let br = Rect::from_min_size(Pos2::new(x, hr.center().y - 12.0), Vec2::new(tw + 16.0, 24.0));
        let resp = ui.interact(br, ui.scope_id().with(("crumb", k)), Sense::click());
        if last {
            p.rect_filled(br, 6.0, white(20));
        } else if resp.hovered() {
            p.rect_filled(br, 6.0, white(10));
        }
        text_fit(&p, Pos2::new(br.left() + 8.0, br.center().y), Align2::LEFT_CENTER, label, f, if last { TEXT() } else { TEXT_DIM() }, tw);
        if resp.clicked() && !last {
            go_to = Some((path.clone(), None));
        }
        x = br.right() + 2.0;
        if !last {
            p.text(Pos2::new(x + 4.0, hr.center().y), Align2::CENTER_CENTER, "›", font(13.0), TEXT_MUTED());
            x += 10.0;
        }
    }
    let resp = if busy {
        crate::kit::glass::busy_pill(ui, new_r, btn, tint::DISK_MAP()).on_hover_text(progress)
    } else {
        icon_pill(ui, new_r, Icon::Refresh, btn, BTN_SECONDARY, true)
    };
    if resp.clicked() {
        act = if busy { Act::Stop } else { Act::Update };
    }
    // the place already mapped: back to its top (a stopped map maps again)
    let same = matches!(&pick_root, Some(Some(path)) if *path == base && peek.is_none() && !app.lens.partial.load(Ordering::SeqCst));
    if same {
        if node.path != base {
            go_to = Some((base.clone(), None));
        }
        pick_root = None;
    }
    // the caller switches, once it has let go of the map
    if let Some(to) = pick_root {
        return Act::Switch(to);
    }

    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, hr.bottom() + 16.0), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - 16.0));
    let mut ai_act: Option<(MenuAct, PathBuf, u64)> = None;
    let mut ai_go: Option<PathBuf> = None;
    if !busy && !partial {
        // once per map: removing a folder changes the bytes, not the map
        let scan = format!("{}:{}", root.path.display(), app.lens.maps.load(Ordering::SeqCst));
        app.ai.auto(&ctx, &scan, || map_ask(root));
        // a tip may only offer what the map offers for that folder
        app.ai.fix_recs(|r| {
            if ai_action(&r.action, Path::new(&r.key)) == "reveal" && r.action != "reveal" {
                r.action = "reveal".into();
                r.label = ts!("lens.show_in_finder").into();
            }
        });
        match aicheck::ui::chip(&mut app.ai, ui, chip_at, ai_data) {
            Some(Event::Start) => {
                let ask = map_ask(root);
                app.ai.start(&ctx, ask);
            }
            Some(Event::Act(rec)) => {
                let path = PathBuf::from(&rec.key);
                let bytes = scan::find(root, &path).map(|n| n.bytes).unwrap_or(0);
                match ai_action(&rec.action, &path) {
                    "open" => ai_go = Some(path),
                    "reveal" => ai_act = Some((MenuAct::Reveal, path, bytes)),
                    _ => ai_act = Some((MenuAct::Trash, path, bytes)),
                }
                app.ai.done(&rec);
            }
            // show where it is: the map opens the folder that holds it (the
            // ring marks it there), or the item itself when that's the top
            Some(Event::Show(rec)) => {
                let path = PathBuf::from(&rec.key);
                ai_go = path.parent().filter(|q| q.starts_with(&root.path) && scan::find(root, q).is_some()).map(Path::to_path_buf).or(Some(path.clone()).filter(|q| scan::find(root, q).is_some()));
                // not on this map (a newer scan moved it): Finder still knows
                if ai_go.is_none() && path.exists() {
                    ai_act = Some((MenuAct::Reveal, path, 0));
                }
            }
            None => {}
        }
    }
    let picked_total: u64 = app.lens_selected.iter().map(|x| x.1).sum();
    let list_w = (body.width() * 0.36).clamp(260.0, LIST_W);
    let lr = Rect::from_min_max(body.min, Pos2::new(body.left() + list_w, body.bottom()));
    let area = Rect::from_min_max(Pos2::new(lr.right() + 16.0, body.top()), body.max);
    let now = ctx.input(|i| i.time);
    let mut hover_path: Option<PathBuf> = None;
    // what's under the pointer, on the map or in the list: (path, bytes)
    let mut hover_item: Option<(PathBuf, u64)> = None;
    let mut menu_act: Option<(MenuAct, PathBuf, u64)> = None;
    let mut toggle: Option<(PathBuf, u64)> = None;
    if let Some(a) = ai_act {
        menu_act = Some(a);
    }
    if let Some(p) = ai_go {
        go_to = Some((p, None));
    }

    // ---- the list: this folder, everything in it, the disk. While a
    // folder on the map is under the pointer, the list shows what's in it.
    let preview = app.lens_hover.as_ref().filter(|h| **h != node.path).and_then(|h| scan::find(root, h)).filter(|n| !n.children.is_empty());
    let shown_node = preview.unwrap_or(node);
    let previewing = preview.is_some();
    glass_card(&p, lr, R_CARD, 0.0, false);
    // the header lines up with the rows' checkboxes and sizes (H9)
    let x0 = lr.left() + sp::L;
    let x1 = lr.right() - sp::L;
    let mut y = lr.top() + 14.0;
    let title = if previewing { shown_node.name.clone() } else { here.clone() };
    icon(&p, Icon::Folder, Rect::from_min_size(Pos2::new(x0, y + 2.0), Vec2::splat(28.0)), ACCENT());
    text_fit(&p, Pos2::new(x0 + 38.0, y + 1.0), Align2::LEFT_TOP, &title, ty::title(), TEXT(), x1 - x0 - 38.0);
    text_fit(&p, Pos2::new(x0 + 38.0, y + 19.0), Align2::LEFT_TOP, &tn!("lens.size_items", n = shown_node.items, size = human_bytes(shown_node.bytes), items = group_digits(shown_node.items)), ty::caption(), ty::caption_color(), x1 - x0 - 38.0);
    y += 46.0;
    // its share of the folder above (a shadow under the hairline)
    let above = shown_node.path.parent().and_then(|q| scan::find(root, q)).filter(|_| shown_node.path != root.path);
    hairline(&p, Pos2::new(lr.left(), y), Pos2::new(lr.right(), y));
    if let Some(a) = above {
        let f = (shown_node.bytes as f32 / a.bytes.max(1) as f32).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_size(Pos2::new(lr.left(), y - 1.0), Vec2::new(lr.width() * f, 2.0)), 0.0, with_alpha(ACCENT(), 0.5));
    }
    y += 6.0;
    let disk_h = 46.0;
    let list_bottom = lr.bottom() - disk_h - 6.0;
    let fit = (((list_bottom - y) / ROW_H).floor().max(0.0)) as usize;
    let kids = &shown_node.children;
    let shown = if kids.len() > fit { fit.saturating_sub(1) } else { kids.len() };
    // keys: ↑↓ move, Enter or → opens, ⌫ or ← goes up, X picks
    if !previewing && !ctx.egui_wants_keyboard_input() && !app.lens_review && !app.lens_other {
        let n = shown.max(1);
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) && !i.modifiers.command {
                app.lens_cursor = Some(app.lens_cursor.map_or(0, |c| (c + 1).min(n - 1)));
            } else if i.key_pressed(egui::Key::ArrowUp) && !i.modifiers.command {
                app.lens_cursor = Some(app.lens_cursor.map_or(0, |c| c.saturating_sub(1)));
            } else if (i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::ArrowRight)) && !i.modifiers.command {
                if let Some(k) = app.lens_cursor.and_then(|c| kids.get(c)).filter(|k| !k.children.is_empty()) {
                    go_to = Some((k.path.clone(), None));
                }
            } else if i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::ArrowLeft) && !i.modifiers.command {
                if can_up {
                    nav = Some(Nav::Up);
                }
            } else if i.key_pressed(egui::Key::X) && !i.modifiers.command {
                if let Some(k) = app.lens_cursor.and_then(|c| kids.get(c)).filter(|k| trashable(&k.path)) {
                    toggle = Some((k.path.clone(), k.bytes));
                }
            }
        });
    }
    let cursor = if previewing { None } else { app.lens_cursor.filter(|c| *c < shown) };
    let lp = p.with_clip_rect(Rect::from_min_max(Pos2::new(lr.left(), y), Pos2::new(lr.right(), list_bottom)));
    for (i, k) in kids.iter().take(shown).enumerate() {
        let rr = Rect::from_min_size(Pos2::new(lr.left() + sp::XS, y), Vec2::new(lr.width() - 2.0 * sp::XS, ROW_H));
        let resp = ui.interact(rr, ui.scope_id().with(("lens-row", i)), Sense::click());
        let selectable = trashable(&k.path);
        let on = app.lens_selected.iter().any(|(q, _)| q == &k.path);
        let lit = !previewing && app.lens_hover.as_ref().is_some_and(|h| h.starts_with(&k.path));
        // inset list: lit rows sit 1 pt in, so two never touch (H1)
        let lit_r = rr.shrink2(Vec2::new(0.0, crate::kit::glass::LIT_INSET));
        if resp.hovered() || lit || cursor == Some(i) {
            lp.rect_filled(lit_r, R_SMALL, white(ROW_HOVER));
        }
        if cursor == Some(i) {
            lp.rect_stroke(lit_r, R_SMALL, Stroke::new(1.0, with_alpha(ACCENT(), 0.6)), StrokeKind::Inside);
        }
        if !previewing {
            reveal::rect(&lp, rr, R_SMALL, app.ai.ring(&ctx, &k.path.to_string_lossy()));
        }
        if resp.hovered() {
            hover_path = Some(k.path.clone());
            hover_item = Some((k.path.clone(), k.bytes));
        }
        menu::context(&resp).show(|ui| {
            if let Some(a) = item_menu(ui, &k.path, !k.children.is_empty()) {
                menu_act = Some((a, k.path.clone(), k.bytes));
            }
        });
        // H9: content 12 in from the lit edge
        let x = rr.left() + sp::M;
        let cb = Rect::from_center_size(Pos2::new(x + 7.5, rr.center().y), Vec2::splat(15.0));
        if selectable && checkbox(ui, cb, ui.scope_id().with(("lens-cb", i)), on, true).clicked() {
            toggle = Some((k.path.clone(), k.bytes));
        }
        let folder = is_folder(k);
        icon(&lp, if folder { Icon::Folder } else { Icon::Doc }, Rect::from_center_size(Pos2::new(x + ROW_ICON_X, rr.center().y), Vec2::splat(16.0)), if folder { ACCENT() } else { TEXT_DIM() });
        let sr = lp.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(k.bytes), font(12.0), TEXT_SOFT());
        text_fit(&lp, Pos2::new(x + ROW_TEXT_X, rr.center().y), Align2::LEFT_CENTER, &k.name, font(13.0), TEXT(), sr.left() - 10.0 - (x + ROW_TEXT_X));
        if resp.clicked() {
            app.lens_cursor = Some(i);
            if !k.children.is_empty() {
                go_to = Some((k.path.clone(), None));
            }
        }
        if resp.hovered() && !k.children.is_empty() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        y += ROW_H;
    }
    let rest: u64 = kids.iter().skip(shown).map(|k| k.bytes).sum::<u64>() + shown_node.other;
    if rest > 0 && y + ROW_H <= list_bottom + 1.0 {
        let rr = Rect::from_min_size(Pos2::new(lr.left() + sp::XS, y), Vec2::new(lr.width() - 2.0 * sp::XS, ROW_H));
        let n = kids.len() - shown;
        let label = if n > 0 { t!("lens.more_small", n = n).into_owned() } else { ts!("lens.small_files").to_string() };
        // the hidden ones open in a sheet
        let resp = ui.interact(rr, ui.scope_id().with("lens-rest"), if n > 0 && !previewing { Sense::click() } else { Sense::hover() });
        if n > 0 && resp.hovered() {
            lp.rect_filled(rr, R_SMALL, white(ROW_HOVER));
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if n > 0 && resp.clicked() {
            app.lens_other = true;
        }
        let sr = lp.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(rest), font(12.0), TEXT_FAINT());
        let tx = rr.left() + sp::M + ROW_TEXT_X;
        text_fit(&lp, Pos2::new(tx, rr.center().y), Align2::LEFT_CENTER, &label, font(13.0), TEXT_DIM(), sr.left() - 10.0 - tx);
    }
    // the disk this map is on
    let (disk_total, disk_free) = crate::kit::fs::volume_space(&root.path.to_string_lossy());
    if disk_total > 0 {
        let used = disk_total.saturating_sub(disk_free);
        let dy = lr.bottom() - disk_h;
        hairline(&p, Pos2::new(lr.left(), dy), Pos2::new(lr.right(), dy));
        // the same disk, words and tint as the start ring (C6), bar 8 under (R4)
        let ty0 = dy + 16.0;
        let total = p.text(Pos2::new(x1, ty0), Align2::RIGHT_CENTER, t!("lens.free_of", total = human_bytes(disk_total)), ty::label(), TEXT_DIM());
        let free = p.text(Pos2::new(total.left() - 4.0, ty0), Align2::RIGHT_CENTER, human_bytes(disk_free), ty::label(), TEXT());
        icon(&p, Icon::Disk, Rect::from_center_size(Pos2::new(x0 + 8.0, ty0), Vec2::splat(16.0)), TEXT_SOFT());
        text_fit(&p, Pos2::new(x0 + 22.0, ty0), Align2::LEFT_CENTER, &volume_label(&root.path), ty::body(), TEXT_SOFT(), free.left() - x0 - 22.0 - sp::S);
        bar(&p, Rect::from_min_size(Pos2::new(x0, ty0 + 8.0 + 8.0), Vec2::new(x1 - x0, 4.0)), (used as f32 / disk_total as f32).clamp(0.01, 1.0), tint::DISK_MAP());
    }

    // ---- the map: bubbles (one level) or rings (three levels around the
    // folder), area or angle by size
    let sel_h = if app.lens_selected.is_empty() { 0.0 } else { 46.0 };
    let pa = Rect::from_min_max(area.min, Pos2::new(area.right(), area.bottom() - sel_h));
    // on the disk's own level, its free space is part of the map
    let volume_root = root.path == Path::new("/") || root.path.parent() == Some(Path::new("/Volumes"));
    let free = if node.path == root.path && volume_root && peek.is_none() { disk_space(&root.path) } else { None };
    if app.lens_morph.as_ref().is_none_or(|m| m.1 != node.path) {
        app.lens_morph = Some((None, node.path.clone(), now));
    }
    // a hovered AI Check row lights its folder on the map
    let lit_path = hover_path.clone().or_else(|| app.ai.hovered.as_ref().map(PathBuf::from));
    let out = match app.lens_view {
        1 => rings(app, ui, pa, root, node, &here, free, lit_path.as_deref(), can_up),
        2 => files(app, ui, pa, node),
        _ => bubbles(app, ui, pa, node, &here, free, lit_path.as_deref()),
    };
    if out.other {
        app.lens_other = true;
    }
    if let Some(h) = out.hover.clone() {
        hover_item = Some(h);
    }
    if out.up {
        nav = Some(Nav::Up);
    }
    if out.go.is_some() {
        go_to = out.go.clone();
    }
    if out.resp.secondary_clicked() {
        app.lens_menu = out.hover.clone();
    }
    if let Some((mp, mb)) = app.lens_menu.clone() {
        let drill = scan::find(root, &mp).is_some_and(|n| !n.children.is_empty());
        menu::context(&out.resp).show(|ui| {
            if let Some(a) = item_menu(ui, &mp, drill) {
                menu_act = Some((a, mp.clone(), mb));
            }
        });
    }
    app.lens_hover = out.hover.map(|h| h.0);
    // Space: Quick Look whatever is under the pointer
    if let Some((hp, _)) = &hover_item {
        if ui.input(|i| i.key_pressed(egui::Key::Space)) && !ctx.egui_wants_keyboard_input() {
            quick_look(hp);
        }
    }

    // ---- the selection bar, only while something is picked
    if !app.lens_selected.is_empty() {
        let sr = Rect::from_min_max(Pos2::new(area.left(), area.bottom() - 34.0), area.max);
        let n = app.lens_selected.len();
        let g = p.text(Pos2::new(sr.left(), sr.center().y), Align2::LEFT_CENTER, human_bytes(picked_total), bold(15.0), TEXT());
        p.text(Pos2::new(g.right() + 8.0, sr.center().y), Align2::LEFT_CENTER, tn!("lens.picked", n = n), font(13.0), TEXT_DIM());
        let bw = button_width(&p, ts!("lens.review_remove"), BTN_PRIMARY);
        let rb = Rect::from_min_size(Pos2::new(sr.right() - bw, sr.top()), Vec2::new(bw, 34.0));
        if pill_button(ui, rb, ts!("lens.review_remove"), BTN_PRIMARY, true).clicked() {
            app.lens_review = true;
        }
        let cw = button_width(&p, ts!("lens.clear"), BTN_SECONDARY);
        let cr = Rect::from_min_size(Pos2::new(rb.left() - 8.0 - cw, sr.top()), Vec2::new(cw, 34.0));
        if pill_button(ui, cr, ts!("lens.clear"), BTN_SECONDARY, true).clicked() {
            app.lens_selected.clear();
        }
    }

    if let Some(t) = toggle {
        pick(app, t);
    }
    // the sheet of everything in the folder ("Other" opened)
    if app.lens_other {
        if let Some(t) = other_sheet(app, ui, r, node) {
            go_to = Some((t, None));
        }
    }
    if menu_act.is_none() {
        menu_act = app.lens_menu_act.take();
    }
    if let Some((a, path, bytes)) = menu_act {
        match a {
            MenuAct::Open => go_to = Some((path, None)),
            MenuAct::QuickLook => quick_look(&path),
            MenuAct::Reveal => crate::kit::macos::reveal(&path),
            MenuAct::CopyPath => ctx.copy_text(path.display().to_string()),
            MenuAct::Trash => {
                app.lens_selected.retain(|(q, _)| !q.starts_with(&path));
                if !app.lens_selected.iter().any(|(q, _)| path.starts_with(q)) {
                    app.lens_selected.push((path, bytes));
                }
                app.lens_review = true;
            }
        }
    }
    let here_path = node.path.clone();
    match nav {
        Some(Nav::Back) if app.lens_back.is_empty() && peek.is_some() => return Act::Overview,
        Some(Nav::Up) if node.path == root.path && peek.is_some() => return Act::Overview,
        Some(Nav::Back) => {
            if let Some(t) = app.lens_back.pop() {
                app.lens_cursor = None;
                app.lens_fwd.push(here_path.clone());
                app.lens_path = t.clone();
                app.lens_morph = Some((None, t, now));
            }
        }
        Some(Nav::Forward) => {
            if let Some(t) = app.lens_fwd.pop() {
                app.lens_cursor = None;
                app.lens_back.push(here_path.clone());
                app.lens_path = t.clone();
                app.lens_morph = Some((None, t, now));
            }
        }
        Some(Nav::Up) => go_to = here_path.parent().filter(|_| can_up).map(|x| (x.to_path_buf(), None)),
        None => {}
    }
    if let Some((target, from)) = go_to {
        if peek.is_some() && !target.starts_with(&root.path) {
            return Act::Overview;
        }
        app.lens_cursor = None;
        if target != here_path {
            app.lens_back.push(here_path);
            app.lens_fwd.clear();
        }
        app.lens_path = target.clone();
        app.lens_morph = Some((from, target, now));
        ctx.request_repaint();
    }
    act
}

#[derive(Clone, Copy)]
enum Nav {
    Back,
    Forward,
    Up,
}

#[derive(Clone, Copy)]
pub(crate) enum MenuAct {
    Open,
    QuickLook,
    Reveal,
    CopyPath,
    Trash,
}

/// The right-click menu of a file or folder on the map or in the list.
fn item_menu(ui: &mut Ui, path: &Path, drill: bool) -> Option<MenuAct> {
    let mut out = None;
    if drill && menu::item(ui, ts!("lens.menu.open")).clicked() {
        out = Some(MenuAct::Open);
    }
    if menu::item(ui, ts!("lens.menu.quick_look")).clicked() {
        out = Some(MenuAct::QuickLook);
    }
    if menu::item(ui, ts!("lens.menu.reveal")).clicked() {
        out = Some(MenuAct::Reveal);
    }
    if menu::item(ui, ts!("lens.menu.copy_path")).clicked() {
        out = Some(MenuAct::CopyPath);
    }
    menu::separator(ui);
    if menu::danger(ui, ts!("lens.menu.trash"), trashable(path)).clicked() {
        out = Some(MenuAct::Trash);
    }
    if out.is_some() {
        ui.close();
    }
    out
}

/// An app's own data (its settings, caches it needs, documents in its
/// container): removing it resets or breaks the app, so it takes a second
/// confirmation.
pub(super) fn is_app_data(p: &Path) -> bool {
    let lib = home().join("Library");
    ["Application Support", "Containers", "Group Containers", "Preferences", "Saved Application State", "WebKit", "HTTPStorages", "Cookies"].iter().any(|d| {
        let base = lib.join(d);
        p.starts_with(&base) && p != base
    }) || p.starts_with("/Library/Application Support") && p != Path::new("/Library/Application Support")
}

/// Quick Look preview of a file or folder.
fn quick_look(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/qlmanage").arg("-p").arg(path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// The startup disk's free space for the map: (free, of it purgeable).
fn disk_space(root: &Path) -> Option<(u64, u64)> {
    let s = root.to_string_lossy();
    let (total, avail) = crate::kit::fs::volume_space(&s);
    (total > 0 && avail > 0).then(|| (avail, avail.saturating_sub(crate::kit::fs::df_free(&s))))
}

/// What a map view drew and what the pointer did on it.
struct MapOut {
    /// The area's response (clicks, right-click menu).
    resp: egui::Response,
    /// The file or folder under the pointer: (path, bytes).
    hover: Option<(PathBuf, u64)>,
    go: Option<(PathBuf, Option<Rect>)>,
    up: bool,
    /// "Other" was clicked: open the sheet of everything in the folder.
    other: bool,
}


/// The Move to Trash dialog, over everything (nothing under it reacts).
fn review(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    crate::kit::modal::layer(ui, "lens-review", |ui| review_body(app, ui, r, cx));
}

fn review_body(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    // swallow clicks behind the dialog
    let _ = ui.interact(r, ui.scope_id().with("review-scrim"), Sense::click());
    p.rect_filled(r, CornerRadius { ne: 12, se: 12, ..Default::default() }, crate::kit::glass::SCRIM());
    let n = app.lens_selected.len();
    let rows = n.min(8);
    let h = 22.0 + 20.0 + 14.0 + rows as f32 * 34.0 + 8.0 + 18.0 + 30.0 + 22.0;
    let m = Rect::from_center_size(r.center(), Vec2::new(520.0, h));
    crate::kit::glass::float_shadow(&p, m, 14.0, 1.0);
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, crate::kit::glass::POPOVER_EDGE()), StrokeKind::Inside);
    let total: u64 = app.lens_selected.iter().map(|x| x.1).sum();
    let title = tn!("lens.confirm", n = n, size = human_bytes(total));
    text_fit(&p, m.min + Vec2::new(22.0, 22.0), Align2::LEFT_TOP, &title, bold(15.0), TEXT(), m.width() - 44.0);
    let list = Rect::from_min_size(m.min + Vec2::new(22.0, 56.0), Vec2::new(m.width() - 44.0, rows as f32 * 34.0 + 8.0));
    p.rect_filled(list, 10.0, Color32::from_black_alpha(46));
    let mut drop: Option<usize> = None;
    for (i, (path, b)) in app.lens_selected.iter().enumerate().take(rows) {
        let rr = Rect::from_min_size(Pos2::new(list.left(), list.top() + 4.0 + i as f32 * 34.0), Vec2::new(list.width(), 34.0));
        icon(&p, Icon::Folder, Rect::from_center_size(Pos2::new(rr.left() + 20.0, rr.center().y), Vec2::splat(16.0)), ACCENT());
        let xr = Rect::from_center_size(Pos2::new(rr.right() - 20.0, rr.center().y), Vec2::splat(22.0));
        let mut sr = p.text(Pos2::new(xr.left() - 8.0, rr.center().y), Align2::RIGHT_CENTER, human_bytes(*b), font(13.0), TEXT_SOFT());
        if is_app_data(path) {
            // an app's own data: its settings or documents go with it
            let g = p.layout_no_wrap(ts!("lens.app_data").into(), font(11.0), AMBER());
            let tr = Rect::from_min_size(Pos2::new(sr.left() - 10.0 - g.size().x - 12.0, rr.center().y - 9.0), Vec2::new(g.size().x + 12.0, 18.0));
            p.rect_filled(tr, 9.0, with_alpha(AMBER(), 0.16));
            p.galley(Pos2::new(tr.left() + 6.0, tr.center().y - g.size().y / 2.0), g, AMBER());
            sr = tr;
        }
        text_fit(&p, Pos2::new(rr.left() + 38.0, rr.center().y), Align2::LEFT_CENTER, &tilde(path), mono(12.0), TEXT(), sr.left() - rr.left() - 48.0);
        if round_button(ui, xr, Icon::Close, ts!("lens.remove_from_list")).clicked() {
            drop = Some(i);
        }
    }
    if let Some(i) = drop {
        app.lens_selected.remove(i);
    }
    let by = m.bottom() - 22.0 - 30.0;
    // app data takes a second click: the button turns into "Move anyway"
    let risky = app.lens_selected.iter().any(|(q, _)| is_app_data(q));
    if !risky {
        app.lens_confirm = false;
    }
    let ok_label = if risky && app.lens_confirm { ts!("lens.move_anyway") } else { ts!("lens.move_to_trash") };
    let ow = button_width(&p, ok_label, BTN_PRIMARY);
    let ok = Rect::from_min_size(Pos2::new(m.right() - 22.0 - ow, by), Vec2::new(ow, 30.0));
    let cw = button_width(&p, ts!("lens.cancel"), BTN_SECONDARY);
    let cancel = Rect::from_min_size(Pos2::new(ok.left() - 8.0 - cw, by), Vec2::new(cw, 30.0));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if pill_button(ui, cancel, ts!("lens.cancel"), BTN_SECONDARY, true).clicked() || esc || app.lens_selected.is_empty() {
        app.lens_review = false;
        app.lens_confirm = false;
    }
    let go = pill_button(ui, ok, ok_label, BTN_PRIMARY, !app.lens_selected.is_empty()).clicked();
    if go && risky && !app.lens_confirm {
        app.lens_confirm = true;
    } else if go {
        app.lens_confirm = false;
        app.lens_review = false;
        let items: Vec<(PathBuf, u64)> = app.lens_selected.drain(..).collect();
        crate::kit::loader::reset(ui.ctx(), LOADER_KEY);
        app.lens_trash = Some(TrashJob::start(items, ui.ctx().clone()));
    }
    let _ = cx;
}

const LOADER_KEY: &str = "diskmap-trash";

/// Picked items going to the Trash, one by one, off the UI thread (a big
/// folder on a slow disk takes seconds).
#[derive(Default)]
pub(crate) struct TrashJob {
    total: u64,
    done: u64,
    current: String,
    removed: Vec<PathBuf>,
    failed: usize,
    freed: u64,
    finished: bool,
    stopped: bool,
    /// The map and the counters took the result.
    applied: bool,
    cancel: bool,
}

impl TrashJob {
    fn start(items: Vec<(PathBuf, u64)>, ctx: egui::Context) -> std::sync::Arc<std::sync::Mutex<TrashJob>> {
        let job = std::sync::Arc::new(std::sync::Mutex::new(TrashJob { total: items.iter().map(|x| x.1).sum(), ..Default::default() }));
        let j = job.clone();
        std::thread::spawn(move || {
            for (path, b) in items {
                {
                    let mut g = j.lock_ok();
                    if g.cancel {
                        g.stopped = true;
                        break;
                    }
                    g.current = path.display().to_string();
                }
                ctx.request_repaint();
                crate::kit::loader::test_pause();
                let skip = !can_trash(&path);
                let ok = !skip && crate::kit::fs::move_to_trash(&path).is_ok();
                let mut g = j.lock_ok();
                g.done += b;
                if ok {
                    g.freed += b;
                    g.removed.push(path);
                } else {
                    g.failed += 1;
                }
            }
            j.lock_ok().finished = true;
            ctx.request_repaint();
        });
        job
    }

    fn status(&self) -> crate::kit::loader::Status {
        use crate::kit::loader::{grouped, Count, Status};
        let moved = t!("lens.moved", size = human_bytes(self.freed)).into_owned();
        if !self.finished {
            Status::Working { title: ts!("lens.moving").into(), count: Count::Bytes { done: self.done, total: self.total }, current: self.current.clone(), can_stop: true }
        } else if self.stopped {
            Status::Stopped { sub: moved }
        } else {
            let n = self.removed.len();
            let problem = (self.failed > 0).then(|| tn!("lens.not_moved", n = self.failed).into_owned());
            Status::Finished { title: moved, sub: tn!("lens.items", n = n, count = grouped(n)).into_owned(), problem, put_back: false }
        }
    }
}

/// The loader while picked items go to the Trash; the map drops them once done.
fn trash_loader(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let Some(job) = app.lens_trash.clone() else {
        return;
    };
    let (status, toast) = {
        let mut g = job.lock_ok();
        if g.finished && !g.applied {
            g.applied = true;
            match app.lens.result.write_ok().as_mut() {
                Some(root) => {
                    scan::prune(root, &g.removed);
                }
                // still mapping: taken off the map once it's done
                None => app.lens_trashed.extend(g.removed.iter().cloned()),
            }
            scan::save_later(&app.lens);
        }
        let toast =
            if g.failed == 0 { t!("lens.moved", size = human_bytes(g.freed)).into_owned() } else { t!("lens.moved_failed", size = human_bytes(g.freed), n = g.failed).into_owned() };
        (g.status(), toast)
    };
    use crate::kit::loader::Click;
    match crate::kit::loader::show(ui, r, LOADER_KEY, &status) {
        Some(Click::Stop) => job.lock_ok().cancel = true,
        Some(Click::Done | Click::PutBack) => app.lens_trash = None,
        // quick: the toast, as before
        Some(Click::Ended) => {
            cx.toast(toast);
            app.lens_trash = None;
        }
        None => {}
    }
}

/// A made-up home folder for screenshots (no personal paths).
pub fn demo_tree(app: &mut App) {
    const GB: f64 = 1_000_000_000.0;
    fn mk(parent: &Path, name: &str, gb: f64, kids: Vec<(&str, f64, Vec<(&str, f64)>)>) -> Node {
        let path = parent.join(name);
        let children: Vec<Node> = kids
            .into_iter()
            .map(|(n, g, sub)| mk(&path, n, g, sub.into_iter().map(|(a, b)| (a, b, vec![])).collect()))
            .collect();
        // a name with an extension and nothing inside is a file
        let file = children.is_empty() && name.contains('.');
        Node { name: name.into(), path: path.clone(), bytes: (gb * GB) as u64, items: if file { 1 } else { (gb * 2400.0) as u64 }, children, other: 0 }
    }
    let base = PathBuf::from("/Users");
    let root = mk(
        &base,
        "me",
        365.0,
        vec![
            ("Library", 182.0, vec![("Containers", 61.0), ("Application Support", 48.0), ("Caches", 34.0), ("Developer", 27.0), ("Mail", 7.0), ("Logs", 5.0)]),
            ("Projects", 64.2, vec![("web-app", 22.0), ("cmdmeow", 18.0), ("api-server", 14.0), ("notes", 10.2)]),
            ("Downloads", 38.4, vec![("Xcode_16.xip", 12.0), ("old-movies.zip", 9.4), ("Installers", 8.0), ("Archive", 9.0)]),
            ("Movies", 31.0, vec![("Screen recordings", 15.2), ("Final Cut", 12.0), ("trip-2024.mov", 3.8)]),
            ("Pictures", 22.5, vec![("Photos Library", 21.0), ("Wallpapers", 1.5)]),
            ("Documents", 14.1, vec![]),
            ("Music", 6.3, vec![]),
            ("Desktop", 2.1, vec![]),
        ],
    );
    let mut root = root;
    root.other = (4.4 * GB) as u64;
    app.lens_root = root.path.clone();
    app.lens_path = root.path.clone();
    app.lens_focus = Some(root.path.join("Movies"));
    app.lens_selected = vec![(root.path.join("Downloads/Xcode_16.xip"), (12.0 * GB) as u64), (root.path.join("Movies"), (31.0 * GB) as u64)];
    *app.lens.result.write_ok() = Some(root);
}

/// Mapping in progress, for screenshots: some folders done, others still
/// counting.
pub fn demo_live(app: &mut App) {
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;
    demo_tree(app);
    let root = app.lens.result.write_ok().take().unwrap();
    let live: Vec<scan::LiveChild> = root
        .children
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let done = matches!(k, 1 | 3 | 5 | 7);
            let part = if done { c.bytes } else { (c.bytes as f64 * [0.55, 0.0, 0.8, 0.0, 0.35, 0.0, 0.6][k.min(6)]) as u64 };
            scan::LiveChild { name: c.name.clone(), path: c.path.clone(), bytes: Arc::new(AtomicU64::new(part)), done, node: done.then(|| Arc::new(c.clone())) }
        })
        .collect();
    app.lens.bytes.store(live.iter().map(|l| l.bytes.load(Ordering::Relaxed)).sum(), Ordering::Relaxed);
    app.lens.files.store(1_284_311, Ordering::Relaxed);
    *app.lens.current.lock_ok() = root.path.join("Library/Containers/com.apple.mail/Data").display().to_string();
    *app.lens.live.lock_ok() = Some((root.path.clone(), live));
    app.lens.running.store(true, Ordering::SeqCst);
    app.lens_selected.clear();
}

/// Folders worth a look: big ones up to four levels down.
fn big_folders(root: &Node) -> Vec<&Node> {
    fn walk<'a>(n: &'a Node, depth: usize, min: u64, out: &mut Vec<&'a Node>) {
        for k in &n.children {
            if k.bytes >= min && !k.children.is_empty() {
                out.push(k);
                if depth < 4 {
                    walk(k, depth + 1, min, out);
                }
            }
        }
    }
    let min = (root.bytes / 100).max(1_000_000_000);
    let mut out = vec![];
    walk(root, 1, min, &mut out);
    out.sort_by_key(|n| std::cmp::Reverse(n.bytes));
    out.truncate(40);
    out
}

/// [`can_trash`], remembered per path: rows and menus ask every frame, and
/// the answer looks at the disk.
fn trashable(p: &Path) -> bool {
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<PathBuf, bool>> = Default::default();
    }
    SEEN.with(|m| *m.borrow_mut().entry(p.to_path_buf()).or_insert_with(|| can_trash(p)))
}

/// Whether the map would offer "Move to Trash…" for `p`: never for
/// essential folders or a protected folder holding a git repo.
fn can_trash(p: &Path) -> bool {
    !(scan::is_essential(p) || crate::kit::fs::never_remove(p) || crate::kit::own::is_kept(p) || crate::kit::fs::is_protected(p).is_some() && p.join(".git").exists())
}

/// The AI Check action to run: a Remove on a folder the map would never
/// trash opens it on the map instead.
fn ai_action<'a>(action: &'a str, p: &Path) -> &'a str {
    match action {
        "open" | "reveal" => action,
        _ if can_trash(p) => "trash",
        _ => "reveal",
    }
}

/// What AI Check gets: the big folders on the map with their biggest parts.
fn map_ask(root: &Node) -> aicheck::Ask {
    let items = big_folders(root)
        .into_iter()
        .map(|n| {
            let mut kids: Vec<&Node> = n.children.iter().collect();
            kids.sort_by_key(|k| std::cmp::Reverse(k.bytes));
            let top: Vec<String> = kids.iter().take(4).map(|k| format!("{} {}", k.name, human_bytes(k.bytes))).collect();
            aicheck::Item {
                key: n.path.display().to_string(),
                line: format!("{} · {} · {} items · biggest inside: {}{}", n.path.display(), human_bytes(n.bytes), n.items, top.join(", "), if can_trash(&n.path) { "" } else { " · system folder: show in Finder only, never trash" }),
                icon: None,
                name: n.name.clone(),
            }
        })
        .collect();
    let mut facts = vec![format!("Mapped: {} ({})", root.path.display(), human_bytes(root.bytes))];
    if let Some((free, total)) = disk_space(&root.path) {
        facts.push(format!("Disk: {} free of {}", human_bytes(free), human_bytes(total)));
    }
    aicheck::Ask {
        task: "This is Disk Map: the biggest folders on this Mac. Point out the few biggest wins: what is safe to remove or can be made smaller (caches, old simulators, VM and container disks, downloads, duplicates), and say how. Skip system folders and the user's own documents unless clearly stale.".into(),
        actions: vec![
            ("trash", "put it in the remove list (the user reviews and confirms before anything moves to the Trash)", ts!("lens.remove")),
            ("reveal", "show it in Finder (for what shouldn't be removed here, such as a Photos library or a system folder; the user sees every item on the map by clicking the tip)", ts!("lens.show_in_finder")),
        ],
        items,
        facts,
        answer: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(path: &str, bytes: u64, items: u64, other: u64, children: Vec<Node>) -> Node {
        Node { name: path.rsplit('/').next().unwrap().into(), path: PathBuf::from(path), bytes, items, children, other }
    }

    #[test]
    fn ai_check_never_trashes_essential_folders() {
        assert_eq!(ai_action("trash", Path::new("/Applications")), "reveal");
        assert_eq!(ai_action("trash", Path::new("/System/Library")), "reveal");
        assert_eq!(ai_action("trash", Path::new("/Applications/Foo.app")), "trash");
        assert_eq!(ai_action("reveal", Path::new("/Applications")), "reveal");
        // the agent is told which folders are open/reveal only
        let root = n("/", 400_000_000_000, 10, 0, vec![n("/Applications", 50_000_000_000, 5, 0, vec![n("/Applications/Foo.app", 1, 1, 0, vec![])])]);
        let ask = map_ask(&root);
        let apps = ask.items.iter().find(|i| i.key == "/Applications").unwrap();
        assert!(apps.line.contains("never trash"));
    }

    #[test]
    fn biggest_files_come_from_every_level() {
        let t = n(
            "/r",
            0,
            9,
            0,
            vec![
                n("/r/a", 0, 5, 1, vec![n("/r/a/big.iso", 900, 1, 0, vec![]), n("/r/a/sub", 0, 2, 0, vec![n("/r/a/sub/mid.mov", 500, 1, 0, vec![])])]),
                n("/r/top.zip", 700, 1, 0, vec![]),
                // a folder holding one small file: not a file itself
                n("/r/tiny", 10, 1, 10, vec![]),
            ],
        );
        let got: Vec<&str> = biggest_files(&t, 10).iter().map(|x| x.name.as_str()).collect();
        assert_eq!(got, vec!["big.iso", "top.zip", "mid.mov"]);
        assert_eq!(biggest_files(&t, 2).len(), 2);
    }

    #[test]
    fn app_data_needs_a_second_look() {
        let h = home();
        assert!(is_app_data(&h.join("Library/Application Support/Slack")));
        assert!(is_app_data(&h.join("Library/Containers/com.apple.mail/Data")));
        assert!(!is_app_data(&h.join("Library/Application Support")));
        assert!(!is_app_data(&h.join("Downloads/x.dmg")));
        assert!(!is_app_data(&h.join("Library/Caches/foo")));
    }

    #[test]
    fn mapped_folders_are_remembered_newest_first() {
        let home = Path::new("/Users/me");
        let mut r = vec![];
        assert!(!remember_folder(&mut r, Path::new("/"), home));
        assert!(!remember_folder(&mut r, Path::new("/Volumes/Backup"), home));
        assert!(!remember_folder(&mut r, home, home));
        for n in 1..=6 {
            assert!(remember_folder(&mut r, &home.join(format!("p{n}")), home));
        }
        assert!(remember_folder(&mut r, &home.join("p3"), home));
        let names: Vec<_> = r.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["p3", "p6", "p5", "p4", "p2"]);
    }

    fn press(at: Pos2, down: bool) -> egui::Event {
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: down, modifiers: Default::default() }
    }

    /// A folder is mapped, then "Macintosh HD" is picked in the switcher:
    /// the page goes back to the disk. It used to freeze for good, clearing
    /// the map while the page still held the map's lock.
    #[test]
    fn switching_from_a_folder_back_to_the_disk_does_not_freeze() {
        use std::sync::mpsc::{channel, RecvTimeoutError};
        let (done, wait) = channel::<()>();
        std::thread::spawn(move || {
            if let Err(RecvTimeoutError::Timeout) = wait.recv_timeout(std::time::Duration::from_secs(60)) {
                eprintln!("Disk Map froze switching from a folder back to the disk");
                std::process::exit(1);
            }
        });
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = App::new(&crate::core::tool::ToolEnv { ctx: ctx.clone(), demo: true });
        // a mapped folder, not the disk
        demo_tree(&mut app);
        app.lens_selected.clear();
        assert_ne!(app.lens_root, Path::new("/"));
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 720.0));
        let r = Rect::from_min_size(Pos2::new(220.0, 0.0), Vec2::new(880.0, 720.0));
        let mut t = 0.0;
        let mut pass = |app: &mut App, events: Vec<egui::Event>| {
            t += 0.1;
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(t), events, ..Default::default() }, |ui| {
                let mut cx = Cx::new();
                page(app, ui, r, &mut cx);
            });
            out.textures_delta.clear();
            out.platform_output.accesskit_update
        };
        // open the switcher (the folder's name, top left)
        // (after the "Disk Map" title, back and forward: SP17)
        let switcher = Pos2::new(r.left() + 215.0, r.top() + HEADER_H / 2.0);
        pass(&mut app, vec![]);
        pass(&mut app, vec![egui::Event::PointerMoved(switcher)]);
        pass(&mut app, vec![press(switcher, true)]);
        pass(&mut app, vec![press(switcher, false)]);
        let mut disk = None;
        for _ in 0..3 {
            let Some(tree) = pass(&mut app, vec![]) else { continue };
            disk = tree.nodes.iter().find(|(_, n)| n.label() == Some("Macintosh HD")).and_then(|(_, n)| n.bounds()).map(|b| Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32));
            if disk.is_some() {
                break;
            }
        }
        let disk = disk.expect("the switcher's menu lists Macintosh HD");
        pass(&mut app, vec![egui::Event::PointerMoved(disk)]);
        pass(&mut app, vec![press(disk, true)]);
        pass(&mut app, vec![press(disk, false)]);
        done.send(()).unwrap();
        // the disk's last map opens next (none here: the folder's is gone)
        assert_eq!(app.lens_root, Path::new("/"));
        assert!(app.lens.result.read_ok().is_none());
    }

    #[test]
    fn picking_a_folder_replaces_what_is_inside() {
        let mut app = App::new(&crate::core::tool::ToolEnv { ctx: egui::Context::default(), demo: true });
        pick(&mut app, (PathBuf::from("/r/a/x"), 1));
        pick(&mut app, (PathBuf::from("/r/a"), 5));
        assert_eq!(app.lens_selected, vec![(PathBuf::from("/r/a"), 5)]);
        pick(&mut app, (PathBuf::from("/r/a/y"), 1));
        assert_eq!(app.lens_selected.len(), 1);
        pick(&mut app, (PathBuf::from("/r/a"), 5));
        assert!(app.lens_selected.is_empty());
    }
}

#[cfg(test)]
mod trash_job_tests {
    use super::*;
    use crate::kit::loader::Status;

    #[test]
    fn a_stopped_job_says_what_went_and_a_finished_one_what_failed() {
        let mut j = TrashJob { total: 10, done: 4, freed: 4, finished: false, ..Default::default() };
        assert!(matches!(j.status(), Status::Working { can_stop: true, .. }));
        j.finished = true;
        j.stopped = true;
        assert!(matches!(j.status(), Status::Stopped { .. }));
        j.stopped = false;
        j.removed = vec![PathBuf::from("/a")];
        j.failed = 2;
        match j.status() {
            Status::Finished { sub, problem, put_back, .. } => {
                assert_eq!(sub, "1 item");
                assert_eq!(problem.as_deref(), Some("2 items couldn't be moved"));
                assert!(!put_back);
            }
            s => panic!("{s:?}"),
        }
    }
}
