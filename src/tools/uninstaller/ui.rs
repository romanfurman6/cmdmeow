//! Uninstaller on screen: the window page (Apps, Leftovers), the
//! review sheet, the result bar with Undo, the panel widget, the drop zone
//! that appears while an app is dragged, and the note shown when an app was
//! thrown away elsewhere.

use crate::kit::sync::LockExt;
use super::apps::Source;
use super::leftovers::{Item, Kind};
use super::remove::Target;
use super::{mac, Drop, Filter, Origin, Phase, Review, Sort, Tab, Uninstaller, ID, TINT, UNDO_FOR, UNUSED_DAYS};
use crate::core::tool::Cx;
use crate::kit::aicheck::{self, ui::Event};
use crate::kit::glass::*;
use crate::kit::tile;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, ViewportBuilder, ViewportId};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WIN_R: f32 = 12.0;
const ROW_H: f32 = sp::ROW_H;
const ASIDE_W: f32 = 330.0;
/// The Size column: room for "999.9 MB".
const SIZE_W: f32 = 72.0;
/// Icons are loaded a few per frame so a long list doesn't stall.
const ICONS_PER_FRAME: usize = 4;

// ---------------------------------------------------------------- shared bits

/// "today", "yesterday", "5 days", "4 months", "2 years".
pub fn ago(now: i64, t: Option<i64>) -> String {
    let Some(t) = t else { return "—".into() };
    let days = (now - t).max(0) / 86400;
    match days {
        0 => ts!("un.ago.today").into(),
        1 => ts!("un.ago.yesterday").into(),
        2..=44 => t!("un.ago.days", n = days).into_owned(),
        45..=364 => t!("un.ago.months", n = (days + 15) / 30).into_owned(),
        _ => {
            let y = days / 365;
            if y == 1 { ts!("un.ago.year").into() } else { t!("un.ago.years", n = y).into_owned() }
        }
    }
}

/// The app's own icon (macOS), else its letter tile.
fn app_icon(u: &mut Uninstaller, ctx: &egui::Context, p: &egui::Painter, r: Rect, path: Option<&Path>, name: &str, budget: &mut usize) {
    if let Some(path) = path {
        // an app in Downloads etc. isn't read (nor cached) before Full Disk Access
        if !u.icons.contains_key(path) && *budget > 0 && !u.demo && !crate::kit::fs::behind_folder_prompt(path) {
            *budget -= 1;
            let tex = mac::app_icon(path, 64).map(|(n, rgba)| ctx.load_texture(format!("app-icon:{}", path.display()), egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba), egui::TextureOptions::LINEAR));
            u.icons.insert(path.to_path_buf(), tex);
            ctx.request_repaint();
        }
        if let Some(Some(t)) = u.icons.get(path) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    letter_tile(p, r, name, 1.0);
}

/// An item's path as people read it: relative to ~/Library, else as is.
pub fn item_label(it: &Item, user_library: &Path) -> String {
    if it.kind == Kind::App {
        return it.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    }
    match it.path.strip_prefix(user_library) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => it.path.display().to_string(),
    }
}

/// "App + 8 items · 17.3 GB".
pub fn target_line(t: &Target) -> String {
    let sel: Vec<&Item> = t.items.iter().filter(|i| i.selected && !(t.reset && i.kind.is_part())).collect();
    let bytes: u64 = sel.iter().map(|i| i.bytes).sum();
    let files = sel.iter().filter(|i| i.kind != Kind::App).count();
    let has_app = sel.iter().any(|i| i.kind == Kind::App);
    let what = match (has_app, files) {
        (true, 0) => ts!("un.the_app").to_string(),
        (true, n) => tn!("un.app_plus", n = n).into_owned(),
        (false, n) => tn!("un.items", n = n).into_owned(),
    };
    format!("{what} · {}", human_bytes(bytes))
}

fn target_bytes(t: &Target) -> u64 {
    t.items.iter().filter(|i| i.selected && !(t.reset && i.kind.is_part())).map(|i| i.bytes).sum()
}

fn is_running(u: &mut Uninstaller, id: &str) -> bool {
    if u.demo {
        return id == "com.docker.docker" || id == "us.zoom.xos";
    }
    let fresh = u.running.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(2));
    if !fresh {
        let ids = crate::kit::native::running_apps().0;
        u.running = Some((ids, Instant::now()));
    }
    u.running.as_ref().is_some_and(|(ids, _)| ids.iter().any(|x| x == id))
}

/// The app the page opens on: the first listed one that isn't running and
/// can be removed. `rows`: (path, bundle id, locked), in list order.
fn default_pick(rows: &[(PathBuf, String, bool)], mut running: impl FnMut(&str) -> bool) -> Option<PathBuf> {
    rows.iter().find(|(_, id, locked)| !*locked && !running(id)).map(|(p, _, _)| p.clone())
}

fn scrim(ui: &mut Ui, r: Rect, salt: &str) {
    let _ = ui.interact(r, ui.scope_id().with(salt), Sense::click());
    ui.painter().rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, crate::kit::glass::SCRIM());
}

fn modal_card(p: &egui::Painter, m: Rect) {
    // opaque: nothing of the page behind may show through
    crate::kit::glass::float_shadow(p, m, 14.0, 1.0);
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, crate::kit::glass::POPOVER_EDGE()), StrokeKind::Inside);
}

/// Paths of apps among dropped (or hovering) files.
fn files_in(ctx: &egui::Context, dropped: bool) -> Vec<PathBuf> {
    ctx.input(|i| {
        if dropped {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        } else {
            i.raw.hovered_files.iter().filter_map(|f| f.path.clone()).collect()
        }
    })
}

// ---------------------------------------------------------------- page

pub fn page(u: &mut Uninstaller, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let (idle, seen) = {
        let g = u.st.lock_ok();
        (g.phase == Phase::Idle, g.page_seen || (u.demo && !u.demo_fresh))
    };
    // the scan reads other apps' data: never before Full Disk Access
    // (the page shows its lock till then, and macOS would ask on its own)
    let fda = || crate::kit::perms::Perm::FullDisk.status() == crate::kit::perms::Status::Allowed;
    // nothing scanned yet: a start screen, the scan only on a click
    // (an app dropped on it goes straight to the page and its review)
    if !seen {
        if files_in(&ctx, true).is_empty() {
            page_header(&p, r, ts!("un.name"), "", TEXT_DIM());
            let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
            if fresh_start(u, ui, body) && fda() {
                u.st.lock_ok().page_seen = true;
                u.rescan();
            }
            return;
        }
        u.st.lock_ok().page_seen = true;
    }
    // the listing scan in the background leaves the page idle: measure now
    if idle && fda() {
        u.rescan();
    }
    let (n_apps, total, phase, orphan_bytes, shown) = {
        let g = u.st.lock_ok();
        // "measuring N of M" counts the rows that show a size, so it matches
        // the list (the scan goes out of order: the app being looked at first)
        let removable = g.apps.iter().filter(|a| a.locked.is_none());
        let shown = (removable.clone().filter(|a| a.sized).count(), removable.count());
        (g.apps.len(), g.apps.iter().map(|a| a.total()).sum::<u64>(), g.phase, g.orphans.iter().map(|o| o.bytes()).sum::<u64>(), shown)
    };
    let sub = match (u.tab, phase) {
        (Tab::Leftovers, _) if orphan_bytes > 0 => human_bytes(orphan_bytes),
        (Tab::Leftovers, _) => String::new(),
        (_, Phase::Idle) => String::new(),
        (_, Phase::Listing) => ts!("un.looking_apps").to_string(),
        (_, Phase::Sizing { .. }) => t!("un.measuring", n = n_apps, done = shown.0, total = shown.1).into_owned(),
        _ => t!("un.apps_size", n = n_apps, size = human_bytes(total)).into_owned(),
    };
    // AI Check: a chip left of the tabs (SP17), on Leftovers
    let ai_data = u.tab == Tab::Leftovers && orphan_bytes > 0;
    let cw = if u.tab == Tab::Leftovers { aicheck::ui::chip_width(&u.ai, ui, ai_data) } else { 0.0 };
    // the status ends before the tabs and Rescan (SP16)
    let rw = icon_pill_width(&p, ts!("un.rescan"), BTN_SECONDARY);
    let labels_owned = [ts!("un.tab.apps").to_string(), ts!("un.tab.leftovers").to_string()];
    let labels: Vec<&str> = labels_owned.iter().map(|s| s.as_str()).collect();
    let tw = crate::kit::glass::tabs_width(&p, &labels);
    let sub_right = r.right() - sp::PAGE_EDGE - rw - sp::M - tw - sp::M - if cw > 0.0 { cw + sp::M } else { 0.0 };
    let (btns, _) = page_header_room(&p, r, ts!("un.name"), &sub, TEXT_DIM(), 0.0, sub_right);
    let (tr, rr) = header_controls(btns, tw, rw);
    let chip_at = Pos2::new(tr.left() - sp::M - cw, tr.center().y);
    let cur = match u.tab {
        Tab::Apps => 0,
        Tab::Leftovers => 1,
    };
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("un-tab"), &labels, cur) {
        u.tab = [Tab::Apps, Tab::Leftovers][i];
    }
    // scan again (apps and leftovers): always there, dimmed until the last scan is done
    if icon_pill(ui, rr, Icon::Refresh, ts!("un.rescan"), BTN_SECONDARY, phase == Phase::Ready).on_hover_text(ts!("un.rescan_tip")).clicked() {
        u.rescan();
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    // a dialog on top (the review, or the move under way) holds the page
    // still: its lists don't scroll along with the dialog's
    let modal = u.review.is_some() || u.st.lock_ok().working.is_some();
    want_first(u);
    ui.scope(|ui| {
        if modal {
            ui.disable();
        }
        match u.tab {
            Tab::Apps => apps_tab(u, ui, body),
            Tab::Leftovers => leftovers_tab(u, ui, body, chip_at),
        }
    });

    // a drop on the window
    let hovering = files_in(&ctx, false).iter().any(|f| f.extension().is_some_and(|e| e == "app"));
    if let Some(f) = files_in(&ctx, true).into_iter().next() {
        u.origin = Origin::Page;
        u.dropped(f);
    }
    if u.origin == Origin::Page {
        let d = u.st.lock_ok().drop.clone();
        match d {
            Drop::Confirm(t) => {
                u.st.lock_ok().drop = Drop::Idle;
                u.selected = t.app.clone();
                u.tab = Tab::Apps;
                u.review = Some(Review::new(vec![t], [0].into()));
            }
            Drop::Refused(name, why) => {
                u.st.lock_ok().drop = Drop::Idle;
                cx.toast(format!("{name}: {why}"));
            }
            _ => {}
        }
    }

    if u.review.is_some() {
        review(u, ui, r);
    }
    if u.st.lock_ok().loader {
        removal_loader(u, ui, r);
    } else {
        working(u, ui, r);
        result_bar(u, ui, r);
    }
    if hovering {
        let pr = r.shrink(16.0);
        p.rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, Color32::from_black_alpha(140));
        p.rect_filled(pr, 16.0, with_alpha(TINT(), 0.18));
        dashed_rect(&p, pr, 16.0, with_alpha(GREEN(), 0.9));
        tool_chip(&p, Rect::from_center_size(pr.center() - Vec2::new(0.0, 30.0), Vec2::splat(56.0)), Icon::AppRemove, TINT());
        p.text(pr.center() + Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, ts!("un.drop_to_uninstall"), bold(22.0), TEXT());
    }
}

/// The start screen before the first scan, no cards (SP1): the tool icon and
/// Scan for apps on the left, a hairline, and on the right what the scan
/// looks through, with counts the app listing already knows. True when Scan
/// for apps was clicked.
fn fresh_start(u: &mut Uninstaller, ui: &mut Ui, area: Rect) -> bool {
    let p = ui.painter().clone();
    let body = Rect::from_min_max(Pos2::new(area.left() + sp::PAGE_EDGE, area.top() + sp::L), Pos2::new(area.right() - sp::PAGE_EDGE, area.bottom() - sp::L));
    const LEFT_W: f32 = 240.0;
    const GAP: f32 = 48.0;
    const BTN: Vec2 = Vec2::new(200.0, 44.0);
    const CHIP: f32 = 96.0;
    let now = u.demo_now.unwrap_or_else(|| chrono::Utc::now().timestamp());
    // counts only once the background listing has found apps (never "0")
    let (n_all, n_unused, n_store) = {
        let g = u.st.lock_ok();
        let unused = |a: &super::AppRow| a.locked.is_none() && a.last_used.is_some_and(|t| now - t > UNUSED_DAYS * 86400);
        (g.apps.len(), g.apps.iter().filter(|a| unused(a)).count(), g.apps.iter().filter(|a| a.source == Source::AppStore).count())
    };
    let n_rows = 5;
    let left_h = CHIP + sp::XXL + BTN.y;
    let head_h = sp::SECTION_H;
    let rows_h = head_h + n_rows as f32 * ROW_H;
    let block_w = body.width().min(840.0);
    let block_h = left_h.max(rows_h);
    let top = (body.center().y - block_h / 2.0).max(body.top());
    let block = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, top), Vec2::new(block_w, block_h));
    let lc = block.left() + LEFT_W / 2.0;
    let ly = block.top() + (block_h - left_h) / 2.0;
    tool_chip(&p, Rect::from_min_size(Pos2::new(lc - CHIP / 2.0, ly), Vec2::splat(CHIP)), Icon::AppRemove, TINT());
    let br = Rect::from_center_size(Pos2::new(lc, ly + CHIP + sp::XXL + BTN.y / 2.0), BTN);
    let id = ui.scope_id().with("un-fresh-scan");
    let clicked = crate::kit::empty::hero_button(ui, br.center(), id, ts!("un.scan_apps"), None, crate::kit::empty::Lead::None, Some(ACTION()), true, br.width()).clicked();
    let dx = block.left() + LEFT_W + GAP;
    hairline(&p, Pos2::new(dx, block.top()), Pos2::new(dx, block.bottom()));
    let x0 = dx + 1.0 + GAP;
    let x1 = block.right();
    let mut y = block.top() + (block_h - rows_h) / 2.0;
    let listed = n_all > 0;
    let cy = section_title(&p, x0, y, ts!("un.tab.apps"));
    if listed {
        crate::kit::glass::section_value(&p, x1, cy, &format!("{n_all}"));
    }
    y += head_h;
    {
        let rows: [(&str, Icon, Option<usize>); 5] = [
            (ts!("un.f.unused"), Icon::Clock, Some(n_unused)),
            (ts!("un.f.large"), Icon::Disk, None),
            (ts!("un.f.store"), Icon::Down, Some(n_store)),
            (ts!("un.f.other"), Icon::Window, Some(n_all.saturating_sub(n_store))),
            (ts!("un.tab.leftovers"), Icon::Folder, None),
        ];
        for (k, (name, ic, n)) in rows.into_iter().enumerate() {
            let rr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + ROW_H));
            // a line under the header too, like Cleanup's start list
            hairline(&p, rr.left_top(), rr.right_top());
            let cy = rr.center().y;
            // H9: content 12 in from the row's edges, like the other start lists
            let (x0, x1) = (x0 + crate::kit::glass::START_PAD, x1 - crate::kit::glass::START_PAD);
            let icp = Pos2::new(x0 + 14.0, cy);
            p.circle_filled(icp, 14.0, with_alpha(TINT(), 0.14));
            icon(&p, ic, Rect::from_center_size(icp, Vec2::splat(16.0)), TINT());
            p.text(Pos2::new(x0 + 28.0 + sp::M, cy), Align2::LEFT_CENTER, name, ty::body(), TEXT());
            // every row has a value: the count, or a faint dash until the scan knows it
            match n.filter(|_| listed) {
                Some(n) => p.text(Pos2::new(x1, cy), Align2::RIGHT_CENTER, format!("{n}"), ty::title(), TEXT()),
                None => p.text(Pos2::new(x1, cy), Align2::RIGHT_CENTER, "—", ty::title(), TEXT_FAINT()),
            };
            y += ROW_H;
        }
    }
    clicked
}

// ---------------------------------------------------------------- Apps

fn filters() -> [(&'static str, Filter); 4] {
    [(ts!("un.f.all"), Filter::All), (ts!("un.f.unused"), Filter::Unused), (ts!("un.f.large"), Filter::Large), (ts!("un.f.store"), Filter::Source(Source::AppStore))]
}

fn chip_row(ui: &mut Ui, p: &egui::Painter, x: f32, y: f32, cur: &mut Filter) -> f32 {
    filter_chips(ui, p, x, y, &filters(), cur, "un-filter", |_| None)
}

fn apps_tab(u: &mut Uninstaller, ui: &mut Ui, body: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let x0 = body.left() + 24.0;
    let x1 = body.right() - 24.0;
    let fy = body.top() + 12.0;
    let sr = Rect::from_min_size(Pos2::new(x0, fy - 1.0), Vec2::new(190.0, 28.0));
    let mut q = std::mem::take(&mut u.search);
    search_field(ui, sr, &mut q, ts!("un.search"), false);
    u.search = q;
    let mut f = u.filter;
    chip_row(ui, &p, sr.right() + 10.0, fy, &mut f);
    u.filter = f;
    // sort: click to cycle
    let label = match u.sort {
        Sort::Size => ts!("un.col.size"),
        Sort::Name => ts!("un.col.name"),
        Sort::LastUsed => ts!("un.col.last_used"),
    };
    let g = p.layout_no_wrap(label.to_string(), font(12.0), TEXT_SOFT());
    let sr = Rect::from_min_max(Pos2::new(x1 - g.size().x - 30.0, fy), Pos2::new(x1, fy + 26.0));
    let resp = ui.interact(sr, ui.scope_id().with("un-sort"), Sense::click()).on_hover_text(ts!("un.sort_tip"));
    p.rect_filled(sr, 6.0, white(if resp.hovered() { 22 } else { 15 }));
    p.text(Pos2::new(sr.left() + 10.0, sr.center().y), Align2::LEFT_CENTER, label, font(12.0), TEXT_SOFT());
    icon(&p, Icon::Down, Rect::from_center_size(Pos2::new(sr.right() - 11.0, sr.center().y), Vec2::splat(10.0)), TEXT_SOFT());
    if resp.clicked() {
        u.sort = match u.sort {
            Sort::Size => Sort::Name,
            Sort::Name => Sort::LastUsed,
            Sort::LastUsed => Sort::Size,
        };
    }

    let area = Rect::from_min_max(Pos2::new(x0, fy + 40.0), Pos2::new(x1, body.bottom() - 20.0));
    let mut list = Rect::from_min_max(area.min, Pos2::new(area.right() - ASIDE_W - 16.0, area.bottom()));
    let aside = Rect::from_min_max(Pos2::new(list.right() + 16.0, area.top()), area.max);
    u.checked.retain(|c| u.st.lock_ok().apps.iter().any(|a| &a.path == c && a.locked.is_none()));
    if !u.checked.is_empty() {
        let bar = Rect::from_min_max(Pos2::new(list.left(), list.bottom() - 52.0), list.max);
        list.max.y = bar.top() - 10.0;
        batch_bar(u, ui, bar);
    }
    let ids = u.visible();
    let keep = u.selected.as_ref().is_some_and(|s| u.st.lock_ok().apps.iter().any(|a| &a.path == s));
    if !keep {
        // never open on an app that's running or can't be removed: its
        // main button would be "Quit and uninstall"
        let rows: Vec<(PathBuf, String, bool)> = {
            let g = u.st.lock_ok();
            ids.iter().map(|&i| (g.apps[i].path.clone(), g.apps[i].id.clone(), g.apps[i].locked.is_some())).collect()
        };
        u.selected = default_pick(&rows, |id| is_running(u, id));
    }
    glass_card(&p, list, R_CARD, 0.0, false);
    // column heads
    let hy = list.top() + sp::TABLE_HEAD_H / 2.0;
    // rows live in a scroll area that keeps a gutter on the right for the
    // bar (R12): the columns line up with the rows, not with the card edge
    let c_size = list.right() - sp::SCROLL_GUTTER - sp::L;
    let c_used = c_size - SIZE_W - sp::L;
    let c_name = list.left() + sp::L + 16.0 + 10.0 + 26.0 + 10.0;
    section_header(&p, Pos2::new(c_name, hy), Align2::LEFT_CENTER, ts!("un.col.name"));
    section_header(&p, Pos2::new(c_used, hy), Align2::RIGHT_CENTER, ts!("un.col.last_used"));
    section_header(&p, Pos2::new(c_size, hy), Align2::RIGHT_CENTER, ts!("un.col.size"));
    hairline(&p, Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H), Pos2::new(list.right(), list.top() + sp::TABLE_HEAD_H));
    let rows = Rect::from_min_max(Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H + 1.0), Pos2::new(list.right(), list.bottom() - 2.0));
    if ids.is_empty() {
        let (msg, hint) = match u.st.lock_ok().phase {
            // Idle = not scanned (no Full Disk Access yet: the lock says so)
            Phase::Idle => ("", ""),
            Phase::Listing => (ts!("un.looking_apps"), ""),
            _ if u.search.is_empty() && u.filter == Filter::All => (ts!("un.no_apps"), ts!("un.no_apps_hint")),
            _ => (ts!("un.no_match"), ts!("un.no_match_hint")),
        };
        p.text(rows.center() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, msg, bold(15.0), TEXT_SOFT());
        p.text(rows.center() + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, hint, font(13.0), TEXT_DIM());
    } else {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rows));
        child.set_clip_rect(rows);
        let now = u.now();
        let mut budget = ICONS_PER_FRAME;
        ScrollArea::vertical().id_salt("un-apps").auto_shrink([false, false]).show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ids.len() as f32 * ROW_H), Sense::hover());
            let p = ui.painter().clone();
            let clip = ui.clip_rect();
            for (k, &i) in ids.iter().enumerate() {
                let r = Rect::from_min_size(Pos2::new(area.left(), area.top() + k as f32 * ROW_H), Vec2::new(area.width(), ROW_H));
                if !r.intersects(clip) {
                    continue;
                }
                // the row without its file list, which can be long
                let (a, total) = {
                    let g = u.st.lock_ok();
                    (g.apps[i].without_items(), g.apps[i].total())
                };
                let resp = ui.interact(r, ui.scope_id().with(("un-row", &a.path)), Sense::click());
                let sel = u.selected.as_ref() == Some(&a.path);
                // a card list: selected and hover sit inside the card (H2, H3)
                if sel {
                    crate::kit::glass::selected_fill(&p, crate::kit::glass::lit_rect(r), crate::kit::glass::LIT_R);
                } else if resp.hovered() {
                    crate::kit::glass::row_hover_fill(&p, r);
                }
                if resp.clicked() {
                    u.selected = Some(a.path.clone());
                }
                let dim = a.locked.is_some();
                // H9: 12 in from the lit fill
                let cb = Rect::from_center_size(Pos2::new(r.left() + sp::L + 8.0, r.center().y), Vec2::splat(15.0));
                let on = u.checked.contains(&a.path);
                if checkbox(ui, cb, ui.scope_id().with(("un-check", &a.path)), on, !dim).clicked() && !dim {
                    if on {
                        u.checked.retain(|c| c != &a.path);
                    } else {
                        u.checked.push(a.path.clone());
                    }
                }
                let ir = Rect::from_center_size(Pos2::new(cb.right() + 10.0 + 13.0, r.center().y), Vec2::splat(26.0));
                app_icon(u, &ctx, &p, ir, Some(&a.path), &a.name, &mut budget);
                let tc = if dim { TEXT_FAINT() } else if sel { ACCENT() } else { TEXT() };
                if a.locked.is_some() || a.sized {
                    let size = if a.locked.is_some() { "—".to_string() } else { human_bytes(total) };
                    p.text(Pos2::new(c_size, r.center().y), Align2::RIGHT_CENTER, size, font(13.0), tc);
                } else {
                    // still measuring: a quiet placeholder, not a cut "…"
                    let ph = Rect::from_min_max(Pos2::new(c_size - 28.0, r.center().y - 2.0), Pos2::new(c_size, r.center().y + 2.0));
                    p.rect_filled(ph, 2.0, white(26));
                }
                let stale = a.last_used.is_some_and(|t| now - t > UNUSED_DAYS * 86400) && !dim;
                p.text(Pos2::new(c_used, r.center().y), Align2::RIGHT_CENTER, ago(now, a.last_used), font(12.0), if stale { AMBER() } else { TEXT_FAINT() });
                let room = c_used - 80.0 - c_name;
                text_fit(&p, Pos2::new(c_name, r.center().y), Align2::LEFT_CENTER, &a.name, font(13.0), tc, room);
            }
        });
    }
    glass_card(&p, aside, R_CARD, 0.0, false);
    if let Some(sel) = u.selected.clone() {
        app_detail(u, ui, aside, &sel);
    }
}

fn batch_bar(u: &mut Uninstaller, ui: &mut Ui, bar: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(bar, R_CARD, RAISED());
    p.rect_stroke(bar, R_CARD, Stroke::new(1.0, white(20)), StrokeKind::Inside);
    let (n, bytes) = {
        let g = u.st.lock_ok();
        let sel: Vec<_> = g.apps.iter().filter(|a| u.checked.contains(&a.path)).collect();
        (sel.len(), sel.iter().map(|a| a.total()).sum::<u64>())
    };
    let t = p.text(Pos2::new(bar.left() + 14.0, bar.center().y), Align2::LEFT_CENTER, tn!("un.selected", n = n), bold(13.0), TEXT());
    p.text(Pos2::new(t.right() + 10.0, bar.center().y), Align2::LEFT_CENTER, t!("un.with_data", size = human_bytes(bytes)), font(13.0), TEXT_DIM());
    let label = tn!("un.review", n = n).into_owned();
    let bw = button_width(&p, &label, BTN_PRIMARY);
    let br = Rect::from_min_size(Pos2::new(bar.right() - 12.0 - bw, bar.center().y - 15.0), Vec2::new(bw, 30.0));
    if pill_button(ui, br, &label, BTN_PRIMARY, true).clicked() {
        let g = u.st.lock_ok();
        let targets: Vec<Target> = g.apps.iter().filter(|a| u.checked.contains(&a.path) && a.locked.is_none()).map(|a| a.target(false)).collect();
        drop(g);
        u.review = Some(Review::new(targets, Default::default()));
    }
    let cw = button_width(&p, ts!("un.clear"), BTN_SECONDARY);
    let cr = Rect::from_min_size(Pos2::new(br.left() - 8.0 - cw, br.top()), Vec2::new(cw, 30.0));
    if pill_button(ui, cr, ts!("un.clear"), BTN_SECONDARY, true).clicked() {
        u.checked.clear();
    }
}

fn app_detail(u: &mut Uninstaller, ui: &mut Ui, r: Rect, sel: &Path) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let Some(a) = u.st.lock_ok().apps.iter().find(|a| a.path == sel).cloned() else { return };
    let x0 = r.left() + 14.0;
    let x1 = r.right() - 14.0;
    let mut y = r.top() + 14.0;
    let mut budget = ICONS_PER_FRAME;
    app_icon(u, &ctx, &p, Rect::from_min_size(Pos2::new(x0, y), Vec2::splat(44.0)), Some(&a.path), &a.name, &mut budget);
    let running = is_running(u, &a.id);
    let mut right = x1;
    if running {
        let g = p.layout_no_wrap(ts!("un.running").into(), font(11.0), GREEN());
        let br = Rect::from_min_max(Pos2::new(x1 - g.size().x - 22.0, y + 12.0), Pos2::new(x1, y + 32.0));
        p.rect_filled(br, 10.0, with_alpha(GREEN(), 0.16));
        p.circle_filled(Pos2::new(br.left() + 9.0, br.center().y), 3.0, GREEN());
        p.galley(Pos2::new(br.left() + 16.0, br.center().y - g.size().y / 2.0), g, GREEN());
        right = br.left() - 8.0;
    }
    text_fit(&p, Pos2::new(x0 + 56.0, y + 4.0), Align2::LEFT_TOP, &a.name, bold(15.0), TEXT(), right - x0 - 56.0);
    let ver = if a.version.is_empty() { a.id.clone() } else { format!("{} · {}", a.version, a.id) };
    text_fit(&p, Pos2::new(x0 + 56.0, y + 24.0), Align2::LEFT_TOP, &ver, font(12.0), TEXT_FAINT(), right - x0 - 56.0);
    y += 60.0;

    if let Some(why) = a.locked {
        icon(&p, Icon::Lock, Rect::from_min_size(Pos2::new(x0, y), Vec2::splat(16.0)), TEXT_FAINT());
        p.text(Pos2::new(x0 + 24.0, y + 8.0), Align2::LEFT_CENTER, why, bold(13.0), TEXT_SOFT());
        return;
    }
    if !a.sized {
        spinner(&p, Pos2::new(x0 + 8.0, y + 10.0), 7.0, ctx.input(|i| i.time), ACCENT());
        p.text(Pos2::new(x0 + 24.0, y + 10.0), Align2::LEFT_CENTER, ts!("un.finding"), font(13.0), TEXT_DIM());
        crate::kit::glass::poll(&ctx);
        return;
    }
    // size
    let total = a.total();
    let app_b = a.app_bytes();
    let tr = p.text(Pos2::new(x0, y), Align2::LEFT_TOP, human_bytes(total), bold(22.0), TEXT());
    p.text(Pos2::new(tr.right() + 6.0, tr.bottom() - 3.0), Align2::LEFT_BOTTOM, ts!("un.in_total"), font(13.0), TEXT_DIM());
    y = tr.bottom() + 8.0;
    // two Clean series, not amber: data isn't a warning (C4, C5)
    let (app_col, data_col) = (series::clean(0), series::clean(3));
    let tf = total.max(1) as f32;
    stacked_bar(&p, Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, 6.0)), &[(app_b as f32 / tf, app_col), ((total - app_b) as f32 / tf, data_col)]);
    y += 12.0;
    let mut lx = x0;
    for (k, (label, col)) in [(t!("un.app_size", size = human_bytes(app_b)).into_owned(), app_col), (t!("un.data_size", size = human_bytes(total - app_b)).into_owned(), data_col)].into_iter().enumerate() {
        if k > 0 {
            lx += 12.0;
        }
        p.rect_filled(Rect::from_min_size(Pos2::new(lx, y + 3.0), Vec2::splat(7.0)), 2.0, col);
        lx = p.text(Pos2::new(lx + 12.0, y), Align2::LEFT_TOP, label, font(11.0), TEXT_DIM()).right();
    }
    y += 26.0;

    // files
    let n_sel = a.items.iter().filter(|i| i.selected).count();
    section_header(&p, Pos2::new(x0, y), Align2::LEFT_TOP, &t!("un.moves_to_trash", sel = n_sel, all = a.items.len()));
    if link(ui, Pos2::new(x1, y), Align2::RIGHT_TOP, ts!("un.show_in_finder"), 11.0).clicked() {
        mac::reveal(&a.path);
    }
    y += 20.0;
    let btn_top = r.bottom() - 14.0 - 30.0;
    let files = Rect::from_min_max(Pos2::new(x0 - 6.0, y), Pos2::new(x1 + 6.0, btn_top - 10.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(files));
    child.set_clip_rect(files);
    let lib = u.roots.user_library.clone();
    ScrollArea::vertical().id_salt(("un-files", &a.path)).auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), a.items.len() as f32 * 26.0), Sense::hover());
        let p = ui.painter().clone();
        for (k, it) in a.items.iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(area.left(), area.top() + k as f32 * 26.0), Vec2::new(area.width(), 26.0));
            if !ui.is_rect_visible(rr) {
                continue;
            }
            let cb = Rect::from_center_size(Pos2::new(rr.left() + 6.0 + 7.0, rr.center().y), Vec2::splat(13.0));
            let resp = checkbox(ui, cb, ui.scope_id().with(("un-file", &it.path)), it.selected, true);
            let sr = p.text(Pos2::new(rr.right(), rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), font(12.0), TEXT_DIM());
            let mut right = sr.left() - 8.0;
            if it.admin {
                right = tag(&p, Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, ts!("un.admin_tag"), TEXT_FAINT()).left() - 6.0;
            }
            // a long path gives way in the middle, so its file name stays
            let w = right - cb.right() - 12.0;
            let label = fit_middle(&p, &item_label(it, &lib), &font(12.0), w);
            let lr = text_fit(&p, Pos2::new(cb.right() + 8.0, rr.center().y), Align2::LEFT_CENTER, &label, font(12.0), if it.selected { TEXT() } else { TEXT_FAINT() }, right - cb.right() - 12.0);
            ui.interact(lr, ui.scope_id().with(("un-tip", &it.path)), Sense::hover()).on_hover_text(format!("{} · {}", it.kind.label(), it.path.display()));
            if resp.clicked() {
                let mut g = u.st.lock_ok();
                if let Some(x) = g.apps.iter_mut().find(|x| x.path == a.path).and_then(|x| x.items.iter_mut().find(|i| i.path == it.path)) {
                    x.selected = !x.selected;
                }
            }
        }
    });
    let gap = 6.0;
    let w = x1 - x0;
    let rw = (w - gap) * 0.42;
    let reset_r = Rect::from_min_size(Pos2::new(x0, btn_top), Vec2::new(rw, 30.0));
    let main_r = Rect::from_min_max(Pos2::new(reset_r.right() + gap, btn_top), Pos2::new(x1, btn_top + 30.0));
    let has_data = a.items.iter().any(|i| !i.kind.is_part());
    if pill_button(ui, reset_r, ts!("un.reset_btn"), BTN_SECONDARY, has_data).on_hover_text(ts!("un.reset_tip")).clicked() {
        u.review = Some(Review::new(vec![a.target(true)], [0].into()));
    }
    let label = if running { ts!("un.quit_uninstall") } else { ts!("un.uninstall_btn") };
    if pill_button(ui, main_r, label, BTN_PRIMARY, n_sel > 0).clicked() {
        u.review = Some(Review::new(vec![a.target(false)], [0].into()));
    }
}

// ---------------------------------------------------------------- review sheet

fn review(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    crate::kit::modal::layer(ui, "un-review", |ui| review_body(u, ui, r));
}

/// Tell the scan which apps to measure first: the ones under review still
/// waiting for their files, the app shown, the ones ticked. An app left
/// unmeasured after a scan gets a new one.
fn want_first(u: &mut Uninstaller) {
    let mut want: Vec<PathBuf> = u.review.as_ref().map(|rv| rv.waiting.iter().cloned().collect()).unwrap_or_default();
    want.extend(u.selected.iter().cloned());
    want.extend(u.checked.iter().cloned());
    let mut g = u.st.lock_ok();
    want.retain(|p| g.apps.iter().any(|a| &a.path == p && !a.sized && a.locked.is_none()));
    let stuck = !want.is_empty() && g.phase == Phase::Ready;
    g.urgent = want;
    drop(g);
    if stuck {
        u.rescan();
    }
}

/// Apps opened for review before they were measured wait for their files
/// (only while a scan runs, which measures them first), then take them in.
fn refresh_review(u: &mut Uninstaller) {
    let g = u.st.lock_ok();
    let Some(rv) = u.review.as_mut() else { return };
    let scanning = matches!(g.phase, Phase::Listing | Phase::Sizing { .. });
    for t in rv.targets.iter_mut() {
        let Some(app) = t.app.clone() else { continue };
        match g.apps.iter().find(|a| a.path == app) {
            Some(a) if !a.sized && scanning && (t.items.is_empty() || rv.waiting.contains(&app)) => {
                rv.waiting.insert(app);
            }
            Some(a) if a.sized && rv.waiting.remove(&app) => t.items = a.target(t.reset).items,
            _ => {
                rv.waiting.remove(&app);
            }
        }
    }
}

fn review_body(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
    refresh_review(u);
    scrim(ui, r, "un-review-scrim");
    let p = ui.painter().clone();
    let Some(rv) = u.review.as_ref() else { return };
    let reset = rv.targets.iter().all(|t| t.reset);
    let n = rv.targets.len();
    let lib = u.roots.user_library.clone();
    // rows: target lines + open item lists
    let mut rows_h = 0.0;
    for (k, t) in rv.targets.iter().enumerate() {
        rows_h += 46.0;
        if rv.open.contains(&k) {
            rows_h += visible_items(t).len() as f32 * 22.0 + 8.0;
        }
    }
    let running: Vec<String> = rv.targets.iter().filter(|t| t.app.is_some()).map(|t| (t.name.clone(), t.id.clone())).collect::<Vec<_>>().into_iter().filter(|(_, id)| is_running(u, id)).map(|(n, _)| n).collect();
    let Some(rv) = u.review.as_ref() else { return };
    let admin = rv.targets.iter().flat_map(|t| visible_items(t)).filter(|i| i.selected && i.admin).count();
    let warn_h = (running.len().min(1) + (admin > 0) as usize) as f32 * 22.0;
    let w = 540.0;
    let list_max = (r.height() - 260.0 - warn_h).max(120.0);
    let list_h = rows_h.min(list_max);
    let h = 18.0 + 22.0 + 8.0 + 36.0 + 14.0 + list_h + 14.0 + warn_h + 14.0 + 30.0 + 18.0;
    let m = Rect::from_center_size(r.center(), Vec2::new(w, h));
    modal_card(&p, m);
    let x0 = m.left() + 18.0;
    let x1 = m.right() - 18.0;
    let mut y = m.top() + 18.0;
    let title = match (reset, n) {
        (true, _) => t!("un.t.reset", name = rv.targets[0].name).into_owned(),
        (false, 1) => t!("un.t.uninstall", name = rv.targets[0].name).into_owned(),
        (false, n) => t!("un.t.uninstall_n", n = n).into_owned(),
    };
    let title = if rv.targets.iter().all(|t| t.app.is_none()) && !reset { if n == 1 { t!("un.t.left", name = rv.targets[0].name).into_owned() } else { t!("un.t.left_n", n = n).into_owned() } } else { title };
    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, title, bold(15.0), TEXT());
    y += 30.0;
    let sub = if reset { ts!("un.sub.reset") } else { ts!("un.sub.all") };
    paragraph(&p, Pos2::new(x0, y), sub, font(13.0), TEXT_DIM(), x1 - x0);
    y += 36.0 + 8.0;
    let lr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, list_h));
    p.rect_filled(lr, 10.0, RAISED());
    p.rect_stroke(lr, 10.0, Stroke::new(1.0, white(HAIRLINE)), StrokeKind::Inside);
    let mut child = ui.new_child(UiBuilder::new().max_rect(lr));
    child.set_clip_rect(lr);
    let mut toggle_open: Option<usize> = None;
    let mut toggle_item: Option<(usize, PathBuf)> = None;
    let targets = rv.targets.clone();
    let opened = rv.open.clone();
    let waiting = rv.waiting.clone();
    let mut budget = ICONS_PER_FRAME;
    ScrollArea::vertical().id_salt("un-review").auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), rows_h), Sense::hover());
        let p = ui.painter().clone();
        let mut yy = area.top();
        for (k, t) in targets.iter().enumerate() {
            let row = Rect::from_min_size(Pos2::new(area.left(), yy), Vec2::new(area.width(), 46.0));
            let resp = ui.interact(row, ui.scope_id().with(("un-rv", k)), Sense::click());
            if resp.hovered() {
                p.rect_filled(row.shrink(2.0), 8.0, white(6));
            }
            let open = opened.contains(&k);
            icon(&p, if open { Icon::Down } else { Icon::Forward }, Rect::from_center_size(Pos2::new(row.left() + 16.0, row.center().y), Vec2::splat(11.0)), TEXT_FAINT());
            let ir = Rect::from_center_size(Pos2::new(row.left() + 44.0, row.center().y), Vec2::splat(28.0));
            app_icon(u, &ctx, &p, ir, t.app.as_deref(), &t.name, &mut budget);
            let measuring = t.app.as_ref().is_some_and(|a| waiting.contains(a));
            // still measuring: a spinner where the size goes, until the files are in
            let sz = if measuring {
                let c = Pos2::new(row.right() - 12.0 - 7.0, row.center().y);
                spinner(&p, c, 7.0, ctx.input(|i| i.time), ACCENT());
                Rect::from_center_size(c, Vec2::splat(14.0))
            } else {
                p.text(Pos2::new(row.right() - 12.0, row.center().y), Align2::RIGHT_CENTER, human_bytes(target_bytes(t)), font(13.0), TEXT())
            };
            let (ly1, ly2) = two_lines_y(&p, row.center().y, &bold(13.0), &font(11.0));
            text_fit(&p, Pos2::new(ir.right() + 10.0, ly1), Align2::LEFT_CENTER, &t.name, bold(13.0), TEXT(), sz.left() - ir.right() - 20.0);
            let open_now = t.app.is_some() && running.contains(&t.name);
            let line = if measuring {
                ts!("un.finding").to_string()
            } else if open_now {
                t!("un.open_now_line", line = target_line(t)).into_owned()
            } else {
                target_line(t)
            };
            text_fit(&p, Pos2::new(ir.right() + 10.0, ly2), Align2::LEFT_CENTER, &line, font(11.0), if open_now && !measuring { AMBER() } else { TEXT_FAINT() }, sz.left() - ir.right() - 20.0);
            if resp.clicked() {
                toggle_open = Some(k);
            }
            yy += 46.0;
            if open {
                for it in visible_items(t) {
                    let rr = Rect::from_min_size(Pos2::new(area.left() + 54.0, yy), Vec2::new(area.width() - 66.0, 22.0));
                    let cb = Rect::from_center_size(Pos2::new(rr.left() + 6.0, rr.center().y), Vec2::splat(12.0));
                    if checkbox(ui, cb, ui.scope_id().with(("un-rvi", k, &it.path)), it.selected, true).clicked() {
                        toggle_item = Some((k, it.path.clone()));
                    }
                    let sr = p.text(Pos2::new(rr.right(), rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), font(12.0), TEXT_FAINT());
                    let mut right = sr.left() - 8.0;
                    if it.admin {
                        right = tag(&p, Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, ts!("un.admin_tag"), TEXT_FAINT()).left() - 6.0;
                    }
                    text_fit(&p, Pos2::new(cb.right() + 8.0, rr.center().y), Align2::LEFT_CENTER, &item_label(it, &lib), font(12.0), if it.selected { TEXT_SOFT() } else { TEXT_FAINT() }, right - cb.right() - 12.0);
                    yy += 22.0;
                }
                yy += 8.0;
            }
            if k + 1 < targets.len() {
                hairline(&p, Pos2::new(area.left() + 12.0, yy), Pos2::new(area.right() - 12.0, yy));
            }
        }
    });
    if let Some(rv) = u.review.as_mut() {
        if let Some(k) = toggle_open {
            if !rv.open.remove(&k) {
                rv.open.insert(k);
            }
        }
        if let Some((k, path)) = toggle_item {
            if let Some(it) = rv.targets[k].items.iter_mut().find(|i| i.path == path) {
                it.selected = !it.selected;
            }
        }
    }
    y = lr.bottom() + 14.0;
    let amber = AMBER();
    if let Some(name) = running.first() {
        icon(&p, Icon::Info, Rect::from_min_size(Pos2::new(x0, y + 2.0), Vec2::splat(14.0)), amber);
        let more = if running.len() > 1 { format!(" (and {} more)", running.len() - 1) } else { String::new() };
        text_fit(&p, Pos2::new(x0 + 22.0, y + 9.0), Align2::LEFT_CENTER, &format!("{name}{more} is open. It will be asked to quit first; unsaved work stays its call."), font(12.0), amber, x1 - x0 - 22.0);
        y += 22.0;
    }
    if admin > 0 {
        icon(&p, Icon::Lock, Rect::from_min_size(Pos2::new(x0, y + 2.0), Vec2::splat(14.0)), TEXT_DIM());
        text_fit(&p, Pos2::new(x0 + 22.0, y + 9.0), Align2::LEFT_CENTER, &tn!("un.password", n = admin), font(12.0), TEXT_DIM(), x1 - x0 - 22.0);
    }
    let Some(rv) = u.review.as_ref() else { return };
    let bytes: u64 = rv.targets.iter().map(target_bytes).sum();
    let measuring = !rv.waiting.is_empty();
    if measuring {
        crate::kit::glass::poll(&ctx);
    }
    // nothing goes to the Trash on half-read files: wait for the scan,
    // which measures these apps before any other
    let any = !measuring && rv.targets.iter().any(|t| target_bytes(t) > 0 || visible_items(t).iter().any(|i| i.selected));
    let by = m.bottom() - 18.0 - 30.0;
    let ok_label = if reset { ts!("un.reset") } else { ts!("un.move_to_trash") };
    let okw = button_width(&p, ok_label, BTN_PRIMARY);
    let ok = Rect::from_min_size(Pos2::new(x1 - okw, by), Vec2::new(okw, 30.0));
    let cw = button_width(&p, ts!("un.cancel"), BTN_SECONDARY);
    let cancel = Rect::from_min_size(Pos2::new(ok.left() - 8.0 - cw, by), Vec2::new(cw, 30.0));
    if !measuring {
        let fr = p.text(Pos2::new(x0, ok.center().y), Align2::LEFT_CENTER, ts!("un.frees"), font(13.0), TEXT_DIM());
        let br = p.text(Pos2::new(fr.right(), ok.center().y), Align2::LEFT_CENTER, human_bytes(bytes), bold(13.0), TEXT());
        p.text(Pos2::new(br.right(), ok.center().y), Align2::LEFT_CENTER, ts!("un.after_empty"), font(13.0), TEXT_DIM());
    }
    if pill_button(ui, cancel, ts!("un.cancel"), BTN_SECONDARY, true).clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        u.review = None;
        return;
    }
    if pill_button(ui, ok, ok_label, BTN_PRIMARY, any).clicked() {
        let rv = u.review.take().unwrap();
        for t in &rv.targets {
            if let Some(a) = &t.app {
                u.checked.retain(|c| c != a);
            }
        }
        u.run(rv.targets);
    }
}

/// The items a target will touch (a reset leaves the app and its jobs).
fn visible_items(t: &Target) -> Vec<&Item> {
    t.items.iter().filter(|i| !(t.reset && i.kind.is_part())).collect()
}

// ---------------------------------------------------------------- progress and result

/// The removal loader over the window while apps or leftovers go, then the
/// result with Put back. A removal over before the loader showed leaves the
/// result to the bar, as before.
fn removal_loader(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    use crate::kit::loader::{self, Click, Count, Status};
    let status = {
        let g = u.st.lock_ok();
        match (&g.working, &g.last) {
            (Some(line), _) => Status::Working {
                title: g.work_title.clone(),
                count: Count::Bytes { done: g.work_done.load(std::sync::atomic::Ordering::Relaxed), total: g.work_total },
                current: line.clone(),
                can_stop: false,
            },
            (None, Some((rep, _))) => loader_result(rep),
            (None, None) => Status::Finished { title: g.work_title.trim_end_matches('…').to_string(), sub: String::new(), problem: None, put_back: false },
        }
    };
    match loader::show(ui, r, "uninstaller", &status) {
        Some(Click::PutBack) => u.undo(),
        Some(Click::Done) => {
            let mut g = u.st.lock_ok();
            g.loader = false;
            // the bar stays only to offer Try again
            if g.last.as_ref().is_some_and(|(rep, _)| rep.retry.is_empty()) {
                g.last = None;
            }
        }
        Some(Click::Ended) => u.st.lock_ok().loader = false,
        Some(Click::Stop) | None => {}
    }
}

/// The loader's last card for a removal.
pub fn loader_result(rep: &super::remove::Report) -> crate::kit::loader::Status {
    let size = human_bytes(rep.bytes());
    let who = match rep.names.as_slice() {
        [n] => n.clone(),
        ns => t!("un.n_apps", n = ns.len()).into_owned(),
    };
    let (_, detail) = rep.summary();
    if rep.moved.is_empty() {
        return crate::kit::loader::Status::Failed { title: t!("un.sum.cant", who = who).into_owned(), problem: detail.unwrap_or_default() };
    }
    let gone = rep.apps.len().saturating_sub(rep.kept.len());
    let (title, sub) = if rep.reset {
        (t!("un.done.reset", who = who).into_owned(), t!("un.done.moved_size", size = size).into_owned())
    } else if rep.apps.is_empty() {
        (t!("un.done.moved", size = size).into_owned(), t!("un.done.from", who = who).into_owned())
    } else if gone == rep.apps.len() {
        (t!("un.done.removed", who = who).into_owned(), t!("un.done.moved_size", size = size).into_owned())
    } else {
        (tn!("un.done.some", n = rep.apps.len(), g = gone).into_owned(), t!("un.done.moved_size", size = size).into_owned())
    };
    crate::kit::loader::Status::Finished { title, sub, problem: detail, put_back: !rep.moved.is_empty() }
}

fn working(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    if u.st.lock_ok().working.is_some() {
        crate::kit::modal::layer(ui, "un-working", |ui| working_body(u, ui, r));
    }
}

fn working_body(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    let Some(line) = u.st.lock_ok().working.clone() else { return };
    let ctx = ui.ctx().clone();
    scrim(ui, r, "un-working-scrim");
    let p = ui.painter().clone();
    let m = Rect::from_center_size(r.center(), Vec2::new(360.0, 74.0));
    modal_card(&p, m);
    spinner(&p, Pos2::new(m.left() + 30.0, m.center().y), 9.0, ctx.input(|i| i.time), ACCENT());
    text_fit(&p, Pos2::new(m.left() + 52.0, m.center().y), Align2::LEFT_CENTER, &line, font(13.0), TEXT(), m.width() - 70.0);
    crate::kit::glass::poll(&ctx);
}

/// The last removal, for the bar and the drop zone.
pub struct Outcome {
    pub title: String,
    /// What stayed, when something did.
    pub detail: Option<String>,
    pub can_undo: bool,
    pub can_retry: bool,
    pub failed: usize,
}

pub fn result_text(u: &Uninstaller) -> Option<Outcome> {
    let g = u.st.lock_ok();
    let (rep, _) = g.last.as_ref()?;
    let (title, detail) = rep.summary();
    Some(Outcome { title, detail, can_undo: !rep.moved.is_empty(), can_retry: !rep.retry.is_empty(), failed: rep.failed.len() })
}

/// Try again with what stayed last time.
fn retry(u: &mut Uninstaller) {
    let again = u.st.lock_ok().last.take().map(|(r, _)| r.retry).unwrap_or_default();
    u.run(again);
}

/// How long a result that went fine stays up. One with problems (Try
/// again, files that stayed) keeps UNDO_FOR.
const TOAST_FOR: Duration = Duration::from_secs(10);

fn result_bar(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    if u.st.lock_ok().working.is_some() {
        return;
    }
    let Some(o) = result_text(u) else { return };
    let failed = o.failed;
    let life = if failed > 0 || o.can_retry { UNDO_FOR } else { TOAST_FOR };
    // on Apps the toast keeps to the list column, clear of the detail
    // card's buttons, and sits above the batch bar when one is up
    let (lane_l, lane_r, lift) = if u.tab == Tab::Apps {
        (r.left() + 24.0, r.right() - 24.0 - ASIDE_W - 16.0, if u.checked.is_empty() { 0.0 } else { 52.0 + 10.0 })
    } else {
        (r.left(), r.right(), 0.0)
    };
    let w = 560.0f32.min(lane_r - lane_l - 48.0);
    let bar = Rect::from_center_size(Pos2::new((lane_l + lane_r) / 2.0, r.bottom() - 46.0 - lift), Vec2::new(w, 48.0));
    {
        let mut g = u.st.lock_ok();
        let Some((_, at)) = g.last.as_mut() else { return };
        // the pointer on it holds it: the countdown starts again on leaving
        if ui.rect_contains_pointer(bar) {
            *at = Instant::now();
        }
        let left = life.saturating_sub(at.elapsed());
        if left.is_zero() {
            return;
        }
        // gone on time even with no input to wake the window
        ui.ctx().request_repaint_after(left);
    }
    let p = ui.painter().clone();
    crate::kit::glass::float_shadow(&p, bar, 12.0, 1.0);
    p.rect_filled(bar, 12.0, POPOVER());
    p.rect_stroke(bar, 12.0, Stroke::new(1.0, crate::kit::glass::POPOVER_EDGE()), StrokeKind::Inside);
    let c = Pos2::new(bar.left() + 25.0, bar.center().y);
    let ok_col = if failed > 0 { AMBER() } else { TINT() };
    p.circle_filled(c, 11.0, ok_col);
    icon(&p, if failed > 0 { Icon::Info } else { Icon::Check }, Rect::from_center_size(c, Vec2::splat(12.0)), ON_ACTION);
    let close = Rect::from_center_size(Pos2::new(bar.right() - 22.0, bar.center().y), Vec2::splat(22.0));
    if round_button(ui, close, Icon::Close, ts!("un.dismiss")).clicked() {
        u.st.lock_ok().last = None;
        return;
    }
    let mut right = close.left() - 8.0;
    if o.can_undo {
        let uw = button_width(&p, ts!("un.undo"), BTN_SECONDARY);
        let ur = Rect::from_min_size(Pos2::new(right - uw, bar.center().y - 15.0), Vec2::new(uw, 30.0));
        if pill_button(ui, ur, ts!("un.undo"), BTN_SECONDARY, true).clicked() {
            u.undo();
            return;
        }
        right = ur.left() - 8.0;
    }
    if o.can_retry {
        let rw = button_width(&p, ts!("un.try_again"), BTN_PRIMARY);
        let rr = Rect::from_min_size(Pos2::new(right - rw, bar.center().y - 15.0), Vec2::new(rw, 30.0));
        if pill_button(ui, rr, ts!("un.try_again"), BTN_PRIMARY, true).clicked() {
            retry(u);
            return;
        }
        right = rr.left() - 8.0;
    }
    let two = o.detail.is_some();
    let (ly1, ly2) = if two { two_lines_y(&p, bar.center().y, &font(13.0), &font(11.0)) } else { (bar.center().y, 0.0) };
    let tr = text_fit(&p, Pos2::new(c.x + 20.0, ly1), Align2::LEFT_CENTER, &o.title, font(13.0), TEXT(), right - c.x - 28.0);
    if let Some(d) = &o.detail {
        let fr = text_fit(&p, Pos2::new(c.x + 20.0, ly2), Align2::LEFT_CENTER, d, font(11.0), if failed > 0 { AMBER() } else { TEXT_DIM() }, right - c.x - 28.0);
        if failed > 0 {
            let why: String = u.st.lock_ok().last.as_ref().map(|(r, _)| r.failed.iter().take(6).map(|(a, b)| format!("{}: {b}", short_path(a))).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            ui.interact(fr.union(tr), ui.scope_id().with("un-fail-tip"), Sense::hover()).on_hover_text(why);
        }
    }
}

/// A failed path as its last part ("ClearVPN.app"); names stay as they are.
fn short_path(s: &str) -> String {
    Path::new(s).file_name().filter(|_| s.starts_with('/')).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| s.to_string())
}

// ---------------------------------------------------------------- Leftovers

fn leftovers_tab(u: &mut Uninstaller, ui: &mut Ui, body: Rect, chip_at: Pos2) {
    let p = ui.painter().clone();
    let (orphans, phase) = {
        let g = u.st.lock_ok();
        (g.orphans.clone(), g.phase)
    };
    let x1 = body.right() - 24.0;
    let mut y = body.top() + 16.0;
    {
        if !orphans.is_empty() {
            let scan = format!("{}:{}", orphans.len(), orphans.iter().map(|o| o.bytes()).sum::<u64>());
            let now = u.now();
            u.ai.auto(ui.ctx(), &scan, || leftovers_ask(now, &orphans));
        }
        match aicheck::ui::chip(&mut u.ai, ui, chip_at, !orphans.is_empty()) {
            Some(Event::Start) => {
                let ask = leftovers_ask(u.now(), &orphans);
                u.ai.start(ui.ctx(), ask);
            }
            Some(Event::Act(r)) => {
                if let Some(o) = orphans.iter().find(|o| o.id == r.key) {
                    if r.action == "remove" {
                        u.review = Some(Review::new(vec![o.target()], [0].into()));
                    } else {
                        // an older answer's "review": show the row instead
                        u.ai.show(&o.id, ui.ctx().input(|i| i.time));
                    }
                }
                u.ai.done(&r);
            }
            // the list scrolls to the row and rings it (below)
            Some(Event::Show(_)) | None => {}
        }
    }
    let x0 = body.left() + 24.0;
    let sel: Vec<&super::Orphan> = orphans.iter().filter(|o| o.selected).collect();
    let total: u64 = orphans.iter().map(|o| o.bytes()).sum();
    let sel_bytes: u64 = sel.iter().map(|o| o.bytes()).sum();
    // hero
    let hero = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, 68.0));
    glass_card(&p, hero, R_CARD, 0.0, false);
    tool_chip(&p, Rect::from_min_size(Pos2::new(hero.left() + 16.0, hero.center().y - 20.0), Vec2::splat(40.0)), Icon::AppRemove, TINT());
    let busy = !matches!(phase, Phase::Ready);
    let title = if orphans.is_empty() {
        if busy { ts!("un.looking_left").to_string() } else { ts!("un.no_left").to_string() }
    } else {
        tn!("un.from_removed", n = orphans.len(), size = human_bytes(total)).into_owned()
    };
    let label = t!("un.move_n", n = sel.len()).into_owned();
    let bw = button_width(&p, &label, BTN_PRIMARY);
    let br = Rect::from_min_size(Pos2::new(hero.right() - 16.0 - bw, hero.center().y - 15.0), Vec2::new(bw, 30.0));
    p.text(Pos2::new(hero.left() + 70.0, hero.center().y), Align2::LEFT_CENTER, title, bold(15.0), TEXT());
    if !orphans.is_empty() && pill_button(ui, br, &label, BTN_PRIMARY, !sel.is_empty()).on_hover_text(t!("un.n_selected", size = human_bytes(sel_bytes))).clicked() {
        let targets: Vec<Target> = sel.iter().map(|o| o.target()).collect();
        u.review = Some(Review::new(targets, Default::default()));
    }
    y = hero.bottom() + 14.0;

    // settings card at the bottom
    let sc = Rect::from_min_max(Pos2::new(x0, body.bottom() - 20.0 - 2.0 * 44.0), Pos2::new(x1, body.bottom() - 20.0));
    glass_card(&p, sc, R_CARD, 0.0, false);
    let (watch, zone) = {
        let g = u.st.lock_ok();
        (g.settings.watch_removed, g.settings.drop_zone)
    };
    let rows = [
        (ts!("un.set.watch"), watch),
        (ts!("un.set.zone"), zone),
    ];
    for (k, (t, on)) in rows.iter().enumerate() {
        let rr = Rect::from_min_size(Pos2::new(sc.left(), sc.top() + k as f32 * 44.0), Vec2::new(sc.width(), 44.0));
        if k > 0 {
            hairline(&p, Pos2::new(rr.left() + 14.0, rr.top()), Pos2::new(rr.right() - 14.0, rr.top()));
        }
        p.text(Pos2::new(rr.left() + 16.0, rr.center().y), Align2::LEFT_CENTER, *t, font(13.0), TEXT());
        let sw = Rect::from_center_size(Pos2::new(rr.right() - 34.0, rr.center().y), Vec2::new(36.0, 20.0));
        if switch(ui, sw, ui.scope_id().with(("un-set", k)), *on, true).clicked() {
            let mut g = u.st.lock_ok();
            if k == 0 {
                g.settings.watch_removed = !g.settings.watch_removed;
            } else {
                g.settings.drop_zone = !g.settings.drop_zone;
            }
            if !u.demo {
                g.settings.save();
            }
        }
    }

    // list
    let list = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, sc.top() - sp::L));
    glass_card(&p, list, R_CARD, 0.0, false);
    let c_size = list.right() - sp::SCROLL_GUTTER - sp::L;
    let c_when = c_size - SIZE_W - sp::L;
    let c_where = c_when - 90.0 - 16.0 - 250.0;
    let c_name = list.left() + sp::L + 16.0 + 12.0;
    let hy = list.top() + sp::TABLE_HEAD_H / 2.0;
    section_header(&p, Pos2::new(c_name, hy), Align2::LEFT_CENTER, ts!("un.col.removed"));
    section_header(&p, Pos2::new(c_where, hy), Align2::LEFT_CENTER, ts!("un.col.where"));
    section_header(&p, Pos2::new(c_when, hy), Align2::RIGHT_CENTER, ts!("un.col.touched"));
    section_header(&p, Pos2::new(c_size, hy), Align2::RIGHT_CENTER, ts!("un.col.size"));
    hairline(&p, Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H), Pos2::new(list.right(), list.top() + sp::TABLE_HEAD_H));
    // rows scroll to the card's bottom edge, as in the apps list
    let rows_r = Rect::from_min_max(Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H + 1.0), Pos2::new(list.right(), list.bottom() - 2.0));
    if orphans.is_empty() {
        let msg = if busy { ts!("un.still_looking") } else { ts!("un.nothing_left") };
        p.text(rows_r.center(), Align2::CENTER_CENTER, msg, font(13.0), TEXT_DIM());
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rows_r));
    child.set_clip_rect(rows_r);
    let now = u.now();
    ScrollArea::vertical().id_salt("un-orphans").auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), orphans.len() as f32 * sp::ROW2_H), Sense::hover());
        let p = ui.painter().clone();
        for (k, o) in orphans.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(area.left(), area.top() + k as f32 * sp::ROW2_H), Vec2::new(area.width(), sp::ROW2_H));
            // an AI Check tip was clicked for this row: bring it into view
            if u.ai.take_scroll(&o.id) {
                ui.scroll_to_rect(r, Some(egui::Align::Center));
            }
            // only rows in view: each one joins its paths for the tooltip
            if !ui.is_rect_visible(r) {
                continue;
            }
            let resp = ui.interact(r, ui.scope_id().with(("un-orow", &o.id)), Sense::click());
            if resp.hovered() {
                crate::kit::glass::row_hover_fill(&p, r);
            }
            reveal::row(&p, r, u.ai.ring(ui.ctx(), &o.id));
            let cb = Rect::from_center_size(Pos2::new(r.left() + sp::L + 8.0, r.center().y), Vec2::splat(15.0));
            if checkbox(ui, cb, ui.scope_id().with(("un-ocheck", &o.id)), o.selected, true).clicked() || resp.clicked() {
                let mut g = u.st.lock_ok();
                if let Some(x) = g.orphans.iter_mut().find(|x| x.id == o.id) {
                    x.selected = !x.selected;
                }
            }
            p.text(Pos2::new(c_size, r.center().y), Align2::RIGHT_CENTER, human_bytes(o.bytes()), font(13.0), TEXT());
            p.text(Pos2::new(c_when, r.center().y), Align2::RIGHT_CENTER, ago(now, o.touched), font(12.0), TEXT_FAINT());
            let mut kinds: Vec<&str> = Vec::new();
            for it in &o.items {
                let l = it.kind.label();
                if !kinds.contains(&l) {
                    kinds.push(l);
                }
            }
            let place = if kinds.len() > 3 { format!("{} +{}", kinds[..2].join(", "), kinds.len() - 2) } else { kinds.join(", ") };
            let wr = text_fit(&p, Pos2::new(c_where, r.center().y), Align2::LEFT_CENTER, &place, font(12.0), TEXT_DIM(), c_when - 100.0 - c_where);
            let paths = o.items.iter().map(|i| i.path.display().to_string()).collect::<Vec<_>>().join("\n");
            ui.interact(wr, ui.scope_id().with(("un-otip", &o.id)), Sense::hover()).on_hover_text(paths);
            let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(11.0));
            text_fit(&p, Pos2::new(c_name, ly1), Align2::LEFT_CENTER, &o.name, bold(13.0), TEXT(), c_where - c_name - 16.0);
            text_fit(&p, Pos2::new(c_name, ly2), Align2::LEFT_CENTER, &o.id, font(11.0), TEXT_FAINT(), c_where - c_name - 16.0);
        }
    });
}

fn plural_n(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// What AI Check gets: one line per removed app's leftovers.
fn leftovers_ask(now: i64, orphans: &[super::Orphan]) -> aicheck::Ask {
    let items = orphans
        .iter()
        .map(|o| {
            let paths: Vec<String> = o.items.iter().take(6).map(|i| format!("{} ({})", short_path(&i.path.display().to_string()), human_bytes(i.bytes))).collect();
            aicheck::Item {
                key: o.id.clone(),
                line: format!("{} ({}) · {} total · last touched {} · {}", o.name, o.id, human_bytes(o.bytes()), ago(now, o.touched), paths.join("; ")),
                icon: None,
                name: o.name.clone(),
            }
        })
        .collect();
    aicheck::Ask {
        task: "This is the Uninstaller's Leftovers list: data left behind by apps that are no longer installed. Point out what is worth removing (big, old, clearly orphaned) and anything that needs a look first, such as documents, recordings or data another installed app may share.".into(),
        actions: vec![
            ("remove", "move that app's leftovers to the Trash (the user confirms in a review sheet first, and sees the row in the list by clicking the tip)", ts!("un.remove")),
        ],
        items,
        facts: Vec::new(),
        answer: Vec::new(),
    }
}

// ---------------------------------------------------------------- panel widget

/// The confirm row inside the 1:4 widget and the drop zone.
fn inline_confirm(u: &mut Uninstaller, ui: &mut Ui, rect: Rect, d: &Drop) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    tile::paint_card(&p, rect, 0.0, false);
    let x0 = rect.left() + 10.0;
    let x1 = rect.right() - 10.0;
    let mut budget = ICONS_PER_FRAME;
    match d {
        Drop::Idle => {}
        Drop::Reading(path) => {
            spinner(&p, Pos2::new(x0 + 14.0, rect.center().y), 8.0, ctx.input(|i| i.time), ACCENT());
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            text_fit(&p, Pos2::new(x0 + 34.0, rect.center().y), Align2::LEFT_CENTER, &t!("un.finding_name", name = name), font(13.0), TEXT_DIM(), x1 - x0 - 34.0);
            crate::kit::glass::animate(&ctx);
        }
        Drop::Refused(name, why) => {
            icon(&p, Icon::Info, Rect::from_min_size(Pos2::new(x0, rect.top() + 12.0), Vec2::splat(16.0)), AMBER());
            text_fit(&p, Pos2::new(x0 + 24.0, rect.top() + 20.0), Align2::LEFT_CENTER, name, bold(12.0), TEXT(), x1 - x0 - 24.0);
            text_fit(&p, Pos2::new(x0 + 24.0, rect.top() + 38.0), Align2::LEFT_CENTER, why, font(11.0), TEXT_DIM(), x1 - x0 - 24.0);
            let ok = Rect::from_min_size(Pos2::new(x1 - 52.0, rect.bottom() - 34.0), Vec2::new(52.0, 26.0));
            if pill_button(ui, ok, ts!("un.ok"), BTN_SECONDARY, true).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
            }
        }
        Drop::Confirm(t) => {
            let ir = Rect::from_min_size(Pos2::new(x0, rect.top() + 9.0), Vec2::splat(28.0));
            app_icon(u, &ctx, &p, ir, t.app.as_deref(), &t.name, &mut budget);
            let running = is_running(u, &t.id);
            text_fit(&p, Pos2::new(ir.right() + 8.0, ir.top() + 6.0), Align2::LEFT_CENTER, &t!("un.t.uninstall", name = t.name), bold(12.0), TEXT(), x1 - ir.right() - 8.0);
            let line = if running { t!("un.line_open_now", line = target_line(t)).into_owned() } else { target_line(t) };
            text_fit(&p, Pos2::new(ir.right() + 8.0, ir.top() + 22.0), Align2::LEFT_CENTER, &line, font(11.0), TEXT_DIM(), x1 - ir.right() - 8.0);
            let by = rect.bottom() - 8.0 - 26.0;
            let main = if running { ts!("un.quit_move") } else { ts!("un.move_to_trash") };
            let mw = button_width(&p, main, BTN_PRIMARY) - 6.0;
            let mr = Rect::from_min_size(Pos2::new(x1 - mw, by), Vec2::new(mw, 26.0));
            let dw = button_width(&p, ts!("un.details"), BTN_SECONDARY) - 6.0;
            let dr = Rect::from_min_size(Pos2::new(mr.left() - 6.0 - dw, by), Vec2::new(dw, 26.0));
            let cr = Rect::from_min_size(Pos2::new(x0 - 4.0, by), Vec2::new(64.0, 26.0));
            if link(ui, Pos2::new(cr.left() + 6.0, cr.center().y), Align2::LEFT_CENTER, ts!("un.cancel"), 12.0).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
            }
            if pill_button(ui, dr, ts!("un.details"), BTN_SECONDARY, true).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
                u.selected = t.app.clone();
                u.tab = Tab::Apps;
                u.review = Some(Review::new(vec![t.clone()], [0].into()));
                u.want_page = true;
            }
            if pill_button(ui, mr, main, BTN_PRIMARY, true).clicked() {
                let t = t.clone();
                u.st.lock_ok().drop = Drop::Idle;
                u.done_until = Some(Instant::now() + Duration::from_secs(10));
                u.run(vec![t]);
            }
        }
    }
}

// ---------------------------------------------------------------- floating windows

pub fn windows(u: &mut Uninstaller, ctx: &egui::Context, cx: &mut Cx) {
    if u.want_page {
        u.want_page = false;
        cx.open_page(ID, "");
    }
    drop_zone(u, ctx);
    notice(u, ctx, cx);
}

fn screen(ctx: &egui::Context) -> Vec2 {
    ctx.input(|i| i.viewport().monitor_size).filter(|s| s.x > 100.0).unwrap_or(Vec2::new(1512.0, 982.0))
}

/// The drop target that appears while an app is dragged, and the confirm
/// and result that follow a drop on it.
fn drop_zone(u: &mut Uninstaller, ctx: &egui::Context) {
    let (dragging, d, working) = {
        let g = u.st.lock_ok();
        (!g.drag.apps.is_empty(), g.drop.clone(), g.working.is_some())
    };
    let mine = u.origin == Origin::Zone;
    let showing_drop = mine && !matches!(d, Drop::Idle);
    let showing_done = mine && u.done_until.is_some_and(|t| Instant::now() < t);
    if !(dragging || showing_drop || showing_done || (mine && working)) {
        if u.done_until.is_some_and(|t| Instant::now() >= t) {
            u.done_until = None;
        }
        return;
    }
    let size = Vec2::new(360.0, 84.0);
    let pos = Pos2::new((screen(ctx).x - size.x) / 2.0, 34.0);
    let title = format!("{}uninstaller-drop", crate::kit::native::PLAIN_WINDOW);
    let vid = ViewportId::from_hash_of("uninstaller-drop");
    let b = ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_position(pos)
        .with_inner_size(size)
        .with_active(false)
        .with_drag_and_drop(true);
    crate::kit::viewport::show(ctx, vid, b, |ui, _| {
        let r = ui.max_rect();
        let ictx = ui.ctx().clone();
        let p = ui.painter().clone();
        let dropped = files_in(&ictx, true);
        if let Some(f) = dropped.into_iter().next() {
            u.origin = Origin::Zone;
            u.dropped(f);
            crate::kit::native::activate();
            u.st.lock_ok().drag = Default::default();
        }
        let over = !files_in(&ictx, false).is_empty();
        let d = u.st.lock_ok().drop.clone();
        if u.origin == Origin::Zone && !matches!(d, Drop::Idle) {
            p.rect_filled(r, 16.0, POPOVER());
            p.rect_stroke(r, 16.0, Stroke::new(1.0, white(36)), StrokeKind::Inside);
            inline_confirm(u, ui, r.shrink(2.0), &d);
            return;
        }
        if u.st.lock_ok().working.is_some() {
            p.rect_filled(r, 16.0, POPOVER());
            spinner(&p, Pos2::new(r.left() + 30.0, r.center().y), 9.0, ictx.input(|i| i.time), ACCENT());
            let line = u.st.lock_ok().working.clone().unwrap_or_default();
            text_fit(&p, Pos2::new(r.left() + 52.0, r.center().y), Align2::LEFT_CENTER, &line, font(13.0), TEXT(), r.width() - 70.0);
            crate::kit::glass::animate(&ictx);
            return;
        }
        if showing_done && !dragging {
            p.rect_filled(r, 16.0, POPOVER());
            p.rect_stroke(r, 16.0, Stroke::new(1.0, white(36)), StrokeKind::Inside);
            if let Some(Outcome { title: msg, detail, can_undo, can_retry, failed }) = result_text(u) {
                let c = Pos2::new(r.left() + 34.0, r.center().y);
                p.circle_filled(c, 20.0, with_alpha(if failed > 0 { AMBER() } else { TINT() }, 0.25));
                icon(&p, if failed > 0 { Icon::Info } else { Icon::Check }, Rect::from_center_size(c, Vec2::splat(18.0)), GREEN());
                let uw = if can_retry { button_width(&p, ts!("un.try_again"), BTN_PRIMARY) } else { 64.0 };
                let ur = Rect::from_min_size(Pos2::new(r.right() - 14.0 - uw, r.center().y - 14.0), Vec2::new(uw, 28.0));
                let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(11.0));
                text_fit(&p, Pos2::new(c.x + 30.0, ly1), Align2::LEFT_CENTER, &msg, bold(13.0), TEXT(), ur.left() - c.x - 40.0);
                let sub = detail.unwrap_or_else(|| ts!("un.undo_hint").into());
                text_fit(&p, Pos2::new(c.x + 30.0, ly2), Align2::LEFT_CENTER, &sub, font(11.0), if failed > 0 { AMBER() } else { TEXT_DIM() }, ur.left() - c.x - 40.0);
                if can_retry {
                    if pill_button(ui, ur, ts!("un.try_again"), BTN_PRIMARY, true).clicked() {
                        retry(u);
                        u.done_until = None;
                    }
                } else if can_undo && pill_button(ui, ur, ts!("un.undo"), BTN_SECONDARY, true).clicked() {
                    u.undo();
                    u.done_until = None;
                }
            }
            ictx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        // waiting for the drop
        let green = GREEN();
        let fill = if over { lerp_color(POPOVER(), GREEN(), 0.2) } else { POPOVER() };
        p.rect_filled(r, 16.0, fill);
        if over {
            p.rect_stroke(r, 16.0, Stroke::new(2.0, green), StrokeKind::Inside);
        } else {
            dashed_rect(&p, r.shrink(2.0), 14.0, white(90));
        }
        let names: Vec<String> = u.st.lock_ok().drag.apps.iter().filter_map(|a| a.file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
        let first = names.first().cloned().unwrap_or_else(|| ts!("un.the_app_lc").into());
        tool_chip(&p, Rect::from_min_size(Pos2::new(r.left() + 16.0, r.center().y - 20.0), Vec2::splat(40.0)), Icon::AppRemove, TINT());
        let x = r.left() + 70.0;
        let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(12.0));
        text_fit(&p, Pos2::new(x, ly1), Align2::LEFT_CENTER, if over { ts!("un.release") } else { ts!("un.drop_here") }, bold(13.0), if over { GREEN() } else { TEXT() }, r.right() - x - 14.0);
        text_fit(&p, Pos2::new(x, ly2), Align2::LEFT_CENTER, &t!("un.after_confirm", name = first), font(12.0), TEXT_DIM(), r.right() - x - 14.0);
        ictx.request_repaint_after(Duration::from_millis(100));
    });
}

/// "Zoom was removed, its data is still here" note.
fn notice(u: &mut Uninstaller, ctx: &egui::Context, cx: &mut Cx) {
    let Some(n) = u.st.lock_ok().notice.clone() else { return };
    if n.at.elapsed() > Duration::from_secs(90) {
        u.st.lock_ok().notice = None;
        return;
    }
    let size = Vec2::new(360.0, 118.0);
    let pos = Pos2::new(screen(ctx).x - size.x - 16.0, 38.0);
    let title = format!("{}uninstaller-notice", crate::kit::native::PLAIN_WINDOW);
    let vid = ViewportId::from_hash_of("uninstaller-notice");
    let b = ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_position(pos)
        .with_inner_size(size)
        .with_active(false);
    let mut act: Option<bool> = None;
    crate::kit::viewport::show(ctx, vid, b, |ui, _| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(r, 16.0, POPOVER());
        p.rect_stroke(r, 16.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        let x0 = r.left() + 12.0;
        tool_chip(&p, Rect::from_min_size(Pos2::new(x0, r.top() + 12.0), Vec2::splat(32.0)), Icon::AppRemove, TINT());
        let tx = x0 + 42.0;
        p.text(Pos2::new(tx, r.top() + 12.0), Align2::LEFT_TOP, "Cmd+Meow", bold(12.0), TEXT_SOFT());
        p.text(Pos2::new(r.right() - 12.0, r.top() + 12.0), Align2::RIGHT_TOP, ts!("un.now"), font(11.0), TEXT_DIM());
        text_fit(&p, Pos2::new(tx, r.top() + 30.0), Align2::LEFT_TOP, &t!("un.was_removed", name = n.target.name), bold(13.0), TEXT(), r.right() - tx - 12.0);
        let k = n.target.items.len();
        text_fit(&p, Pos2::new(tx, r.top() + 48.0), Align2::LEFT_TOP, &tn!("un.data_left", n = k, size = human_bytes(target_bytes(&n.target))), font(12.0), TEXT_SOFT(), r.right() - tx - 12.0);
        let by = r.bottom() - 12.0 - 28.0;
        let w = (r.width() - 24.0 - 6.0) / 2.0;
        if pill_button(ui, Rect::from_min_size(Pos2::new(x0, by), Vec2::new(w, 28.0)), ts!("un.keep"), BTN_SECONDARY, true).clicked() {
            act = Some(false);
        }
        if pill_button(ui, Rect::from_min_size(Pos2::new(x0 + w + 6.0, by), Vec2::new(w, 28.0)), ts!("un.review_left"), BTN_SECONDARY, true).clicked() {
            act = Some(true);
        }
    });
    match act {
        Some(true) => {
            u.st.lock_ok().notice = None;
            u.tab = Tab::Leftovers;
            u.review = Some(Review::new(vec![n.target], [0].into()));
            cx.open_page(ID, "leftovers");
        }
        Some(false) => u.st.lock_ok().notice = None,
        None => ctx.request_repaint_after(Duration::from_millis(500)),
    }
}

// ---------------------------------------------------------------- sample data

pub mod demo {
    use super::super::apps::Source;
    use super::super::leftovers::{Item, Kind};
    use super::super::{AppRow, Filter, Orphan, Phase, Review, Tab, Uninstaller};
    use crate::core::tool::View;
    use std::path::PathBuf;
    use crate::kit::sync::LockExt;
    use std::time::Instant;

    const MB: u64 = 1_000_000;
    /// Nov 9 2026, noon UTC.
    const NOW: i64 = 1_794_225_600;

    #[allow(clippy::too_many_arguments)]
    fn app(name: &str, id: &str, ver: &str, src: Source, app_b: u64, data: &[(&str, Kind, u64, bool)], used_days: Option<i64>, locked: Option<&'static str>) -> AppRow {
        let path = PathBuf::from(format!("/Applications/{name}.app"));
        let lib = crate::kit::fs::home().join("Library");
        let mut items = vec![Item { path: path.clone(), kind: Kind::App, bytes: app_b, admin: false, selected: true }];
        for (rel, kind, b, admin) in data {
            let p = if rel.starts_with('/') { PathBuf::from(rel) } else { lib.join(rel) };
            items.push(Item { path: p, kind: *kind, bytes: *b, admin: *admin, selected: true });
        }
        AppRow { path, name: name.into(), id: id.into(), version: ver.into(), source: src, locked, last_used: used_days.map(|d| NOW - d * 86400 - 3600), items, sized: true, alt: Vec::new(), sign: Default::default(), pkgs: Vec::new() }
    }

    pub fn apps() -> Vec<AppRow> {
        use Kind::*;
        vec![
            app("Xcode", "com.apple.dt.Xcode", "26.1", Source::AppStore, 12_100 * MB, &[("Developer/Xcode/DerivedData", Caches, 2_400 * MB, false), ("Caches/com.apple.dt.Xcode", Caches, 300 * MB, false)], Some(0), None),
            app(
                "Docker",
                "com.docker.docker",
                "4.36.0",
                Source::Direct,
                2_100 * MB,
                &[
                    ("Containers/com.docker.docker", Containers, 14_600 * MB, false),
                    ("Caches/com.docker.docker", Caches, 310 * MB, false),
                    ("Group Containers/group.com.docker", GroupContainers, 220 * MB, false),
                    ("Application Support/com.docker.docker", Support, 48 * MB, false),
                    ("Logs/Docker Desktop", Logs, 12 * MB, false),
                    ("Saved Application State/com.docker.docker.savedState", SavedState, 2 * MB, false),
                    ("Preferences/com.docker.docker.plist", Preferences, 4_000, false),
                    ("/Library/LaunchDaemons/com.docker.vmnetd.plist", Launch, 4_000, true),
                ],
                Some(0),
                None,
            ),
            app(
                "Android Studio",
                "com.google.android.studio",
                "2024.2",
                Source::Direct,
                2_400 * MB,
                &[("Application Support/Google/AndroidStudio2024.2", Support, 1_900 * MB, false), ("Caches/Google/AndroidStudio2024.2", Caches, 4_800 * MB, false), ("Logs/Google/AndroidStudio2024.2", Logs, 310 * MB, false), ("Preferences/com.google.android.studio.plist", Preferences, 8_000, false)],
                Some(214),
                None,
            ),
            app("Final Cut Pro", "com.apple.FinalCut", "11.0", Source::AppStore, 5_900 * MB, &[("Containers/com.apple.FinalCut", Containers, 700 * MB, false), ("Caches/com.apple.FinalCut", Caches, 90 * MB, false)], Some(390), None),
            app("Figma", "com.figma.Desktop", "125.4", Source::Direct, 480 * MB, &[("Application Support/Figma", Support, 1_300 * MB, false), ("Caches/com.figma.Desktop", Caches, 120 * MB, false)], Some(1), None),
            app("Slack", "com.tinyspeck.slackmacgap", "4.41", Source::Direct, 520 * MB, &[("Application Support/Slack", Support, 820 * MB, false), ("Caches/com.tinyspeck.slackmacgap", Caches, 60 * MB, false)], Some(0), None),
            app(
                "Zoom",
                "us.zoom.xos",
                "6.2.5",
                Source::Homebrew,
                410 * MB,
                &[("Application Support/zoom.us", Support, 640 * MB, false), ("Caches/us.zoom.xos", Caches, 30 * MB, false), ("Preferences/us.zoom.xos.plist", Preferences, 20_000, false), ("/Library/LaunchDaemons/us.zoom.ZoomDaemon.plist", Launch, 4_000, true)],
                Some(121),
                None,
            ),
            app("Spotify", "com.spotify.client", "1.2.50", Source::Direct, 340 * MB, &[("Caches/com.spotify.client", Caches, 640 * MB, false)], Some(2), None),
            app("CleanShot X", "pl.maketheweb.cleanshotx-setapp", "4.7", Source::Setapp, 180 * MB, &[("Application Support/CleanShot", Support, 230 * MB, false)], Some(150), None),
            app("VLC", "org.videolan.vlc", "3.0.21", Source::Homebrew, 160 * MB, &[("Preferences/org.videolan.vlc", Preferences, 30 * MB, false)], Some(420), None),
            app("Safari", "com.apple.Safari", "26.1", Source::Apple, 0, &[], Some(0), Some(ts!("un.locked.macos"))),
        ]
    }

    pub fn orphans() -> Vec<Orphan> {
        let lib = crate::kit::fs::home().join("Library");
        let o = |name: &str, id: &str, parts: &[(&str, Kind, u64)], days: i64| Orphan {
            name: name.into(),
            id: id.into(),
            items: parts.iter().map(|(rel, k, b)| Item { path: lib.join(rel), kind: *k, bytes: *b, admin: false, selected: true }).collect(),
            touched: Some(NOW - days * 86400),
            selected: true,
        };
        use Kind::*;
        vec![
            o("Parallels Desktop", "com.parallels.desktop.console", &[("Application Support/com.parallels.desktop.console", Support, 3_200 * MB), ("Caches/com.parallels.desktop.console", Caches, 800 * MB), ("Logs/com.parallels.desktop.console", Logs, 100 * MB), ("Preferences/com.parallels.desktop.console.plist", Preferences, 20_000)], 95),
            o("Unity Hub", "com.unity3d.unityhub", &[("Application Support/com.unity3d.unityhub", Support, 1_700 * MB), ("Caches/com.unity3d.unityhub", Caches, 500 * MB)], 150),
            o("Microsoft Teams", "com.microsoft.teams2", &[("Containers/com.microsoft.teams2", Containers, 1_500 * MB), ("Saved Application State/com.microsoft.teams2.savedState", SavedState, 100 * MB)], 180),
            o("Discord", "com.hnc.Discord", &[("Application Support/com.hnc.Discord", Support, 700 * MB), ("Caches/com.hnc.Discord", Caches, 80 * MB)], 240),
            o("Postman", "com.postmanlabs.mac", &[("Application Support/com.postmanlabs.mac", Support, 400 * MB), ("Saved Application State/com.postmanlabs.mac.savedState", SavedState, 10 * MB)], 270),
            o("Skype", "com.skype.skype", &[("Application Support/com.skype.skype", Support, 160 * MB), ("Preferences/com.skype.skype.plist", Preferences, 30_000)], 400),
            o("Raycast", "com.raycast.macos", &[("Application Support/com.raycast.macos", Support, 80 * MB), ("HTTPStorages/com.raycast.macos", WebData, 16 * MB)], 420),
            o("TeamViewer", "com.teamviewer.TeamViewer", &[("Preferences/com.teamviewer.TeamViewer.plist", Preferences, MB)], 500),
        ]
    }

    /// Load sample data for `view` and say what to show.
    pub fn fill(u: &mut Uninstaller, view: &str) -> Option<View> {
        if !view.starts_with("uninstaller") {
            return None;
        }
        u.demo_now = Some(NOW);
        {
            let mut g = u.st.lock_ok();
            g.apps = apps();
            g.orphans = orphans();
            g.phase = Phase::Ready;
            g.last = None;
        }
        u.tab = Tab::Apps;
        u.filter = Filter::All;
        u.review = None;
        u.checked.clear();
        u.demo_fresh = false;
        u.selected = Some(PathBuf::from("/Applications/Docker.app"));
        match view {
            "uninstaller" | "uninstaller-apps" => {}
            "uninstaller-fresh" => u.demo_fresh = true,
            "uninstaller-leftovers" | "uninstaller-leftovers-shown" => {
                u.tab = Tab::Leftovers;
                use crate::kit::aicheck::Check;
                let ids: Vec<String> = u.st.lock_ok().orphans.iter().map(|o| o.id.clone()).collect();
                let id = |k: usize| ids.get(k).cloned().unwrap_or_default();
                let last = ids.len().saturating_sub(1);
                u.ai.set_demo(
                    "Leftovers of 2 removed apps",
                    vec![
                        Check::demo_rec_for(&id(last), "remove", "Remove", "Remove TeamViewer leftovers", "1.00 MB", "TeamViewer was removed in March; nothing else uses these", "TeamViewer"),
                        Check::demo_rec_for(&id(0), "remove", "Remove", "Remove Parallels data", "4.10 GB", "Holds virtual machine settings; check the files first", "Parallels"),
                    ],
                );
                if view == "uninstaller-leftovers-shown" {
                    u.ai.demo_show(&id(last));
                }
            }
            "uninstaller-review" => {
                u.filter = Filter::Unused;
                let g = u.st.lock_ok();
                let pick = ["Android Studio", "Final Cut Pro", "Zoom"];
                let targets: Vec<_> = g.apps.iter().filter(|a| pick.contains(&a.name.as_str())).map(|a| a.target(false)).collect();
                u.checked = g.apps.iter().filter(|a| pick.contains(&a.name.as_str())).map(|a| a.path.clone()).collect();
                drop(g);
                u.selected = Some(PathBuf::from("/Applications/Android Studio.app"));
                u.review = Some(Review::new(targets, [0].into()));
            }
            "uninstaller-done" => {
                let t = u.st.lock_ok().apps.iter().find(|a| a.name == "Android Studio").unwrap().target(false);
                u.run(vec![t]);
                u.st.lock_ok().last.as_mut().unwrap().1 = Instant::now();
            }
            "uninstaller-reset" => {
                let t = u.st.lock_ok().apps.iter().find(|a| a.name == "Docker").unwrap().target(true);
                u.review = Some(Review::new(vec![t], [0].into()));
            }
            _ => return None,
        }
        Some(View::Page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_naturally() {
        let now = 1_000_000_000;
        assert_eq!(ago(now, None), "—");
        assert_eq!(ago(now, Some(now - 100)), "today");
        assert_eq!(ago(now, Some(now - 86400 - 5)), "yesterday");
        assert_eq!(ago(now, Some(now - 5 * 86400)), "5 days");
        assert_eq!(ago(now, Some(now - 121 * 86400)), "4 months");
        assert_eq!(ago(now, Some(now - 400 * 86400)), "1 year");
        assert_eq!(ago(now, Some(now - 800 * 86400)), "2 years");
    }

    #[test]
    fn opens_on_an_app_that_is_not_running() {
        let row = |p: &str, id: &str, locked: bool| (PathBuf::from(p), id.to_string(), locked);
        let rows = vec![row("/A/Claude.app", "claude", false), row("/A/Safari.app", "safari", true), row("/A/Steam.app", "steam", false)];
        assert_eq!(default_pick(&rows, |id| id == "claude"), Some(PathBuf::from("/A/Steam.app")));
        assert_eq!(default_pick(&rows, |_| false), Some(PathBuf::from("/A/Claude.app")));
        // everything running or protected: nothing picked
        assert_eq!(default_pick(&rows, |_| true), None);
        assert_eq!(default_pick(&[], |_| false), None);
    }

    #[test]
    fn target_lines() {
        let apps = demo::apps();
        let docker = apps.iter().find(|a| a.name == "Docker").unwrap();
        assert_eq!(target_line(&docker.target(false)), "App + 8 items · 17.3 GB");
        // a reset leaves the app and its launch jobs
        assert!(target_line(&docker.target(true)).starts_with("7 items"));
    }

    #[test]
    fn demo_views_load() {
        let env = crate::core::tool::ToolEnv { ctx: egui::Context::default(), demo: true };
        let mut u = Uninstaller::new(&env);
        for v in ["uninstaller", "uninstaller-review", "uninstaller-done", "uninstaller-reset", "uninstaller-leftovers"] {
            assert!(demo::fill(&mut u, v).is_some(), "{v}");
        }
        assert!(demo::fill(&mut u, "clipboard").is_none());
    }
}

/// `s` cut in the middle ("/Library/…/com.docker.vmnetd.plist") until it
/// fits `max_w`; the end of a path is what tells files apart.
fn fit_middle(p: &egui::Painter, s: &str, f: &egui::FontId, max_w: f32) -> String {
    let fits = |t: &str| p.layout_no_wrap(t.to_string(), f.clone(), TEXT()).size().x <= max_w;
    if fits(s) {
        return s.to_string();
    }
    let mut n = s.chars().count();
    while n > 8 {
        n -= 1;
        let t = middle_ellipsis(s, n);
        if fits(&t) {
            return t;
        }
    }
    s.to_string()
}
