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
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=44 => format!("{days} days"),
        45..=364 => format!("{} months", (days + 15) / 30),
        _ => {
            let y = days / 365;
            if y == 1 { "1 year".into() } else { format!("{y} years") }
        }
    }
}

fn letter_color(name: &str) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(0x1F, 0x7A, 0xE0),
        Color32::from_rgb(0x8E, 0x44, 0xAD),
        Color32::from_rgb(0x24, 0x8A, 0x3D),
        Color32::from_rgb(0xC7, 0x80, 0x1F),
        Color32::from_rgb(0xC0, 0x39, 0x2B),
        Color32::from_rgb(0x2E, 0x9E, 0x8F),
        Color32::from_rgb(0x5B, 0x2C, 0x6F),
        Color32::from_rgb(0x48, 0x48, 0x4E),
    ];
    let h = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    PALETTE[(h % PALETTE.len() as u32) as usize]
}

/// The app's own icon (macOS), else a colored square with its first letter.
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
    let rad = (r.width() * 0.25).round();
    p.rect_filled(r, rad, letter_color(name));
    let l: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(r.center(), Align2::CENTER_CENTER, l, bold(r.width() * 0.5), Color32::WHITE);
}

fn source_chip(p: &egui::Painter, pos: Pos2, s: Source) -> Option<Rect> {
    let label = s.tag()?;
    let col = match s {
        Source::AppStore => ACCENT(),
        Source::Homebrew => AMBER(),
        Source::Setapp => Color32::from_rgb(0xC9, 0xA7, 0xFF),
        _ => TEXT_FAINT(),
    };
    let g = p.layout_no_wrap(label.to_string(), font(11.0), col);
    let r = Align2::LEFT_CENTER.anchor_size(pos, g.size() + Vec2::new(10.0, 2.0));
    // quiet text, no box: a tag is meta, not a button
    p.galley(r.center() - g.size() / 2.0, g, col);
    Some(r)
}

fn outline_chip(p: &egui::Painter, pos: Pos2, align: Align2, label: &str, col: Color32) -> Rect {
    let g = p.layout_no_wrap(label.to_string(), font(11.0), col);
    let r = align.anchor_size(pos, g.size() + Vec2::new(10.0, 2.0));
    // quiet text, no box: a tag is meta, not a button
    p.galley(r.center() - g.size() / 2.0, g, col);
    r
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
        (true, 0) => "The app".to_string(),
        (true, n) => format!("App + {n} item{}", if n == 1 { "" } else { "s" }),
        (false, n) => format!("{n} item{}", if n == 1 { "" } else { "s" }),
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

fn scrim(ui: &mut Ui, r: Rect, salt: &str) {
    let _ = ui.interact(r, ui.scope_id().with(salt), Sense::click());
    ui.painter().rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, Color32::from_black_alpha(115));
}

fn modal_card(p: &egui::Painter, m: Rect) {
    let shadow = egui::epaint::Shadow { offset: [0, 20], blur: 60, spread: 0, color: Color32::from_black_alpha(128) };
    p.add(shadow.as_shape(m, CornerRadius::same(14)));
    // opaque: nothing of the page behind may show through
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, white(31)), StrokeKind::Inside);
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
        (g.phase == Phase::Idle, g.page_seen || u.demo)
    };
    // the scan reads other apps' data: never before Full Disk Access
    // (the page shows its lock till then, and macOS would ask on its own)
    let fda = || crate::kit::perms::Perm::FullDisk.status() == crate::kit::perms::Status::Allowed;
    // nothing scanned yet: a start screen, the scan only on a click
    // (an app dropped on it goes straight to the page and its review)
    if !seen {
        if files_in(&ctx, true).is_empty() {
            page_header(&p, r, "Uninstaller", "", TEXT_DIM());
            let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
            if fresh_start(ui, body) && fda() {
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
    let (n_apps, total, phase, orphan_bytes) = {
        let g = u.st.lock_ok();
        (g.apps.len(), g.apps.iter().map(|a| a.total()).sum::<u64>(), g.phase, g.orphans.iter().map(|o| o.bytes()).sum::<u64>())
    };
    let sub = match (u.tab, phase) {
        (Tab::Leftovers, _) => "Data left behind by apps that are no longer installed".to_string(),
        (_, Phase::Idle) => String::new(),
        (_, Phase::Listing) => "Looking for apps…".to_string(),
        (_, Phase::Sizing { done, total: t }) => format!("{n_apps} apps · measuring {done} of {t}…"),
        _ => format!("{n_apps} apps · {} · drop any app on this window to remove it", human_bytes(total)),
    };
    // AI Check: a chip after the title, on Leftovers
    let ai_data = u.tab == Tab::Leftovers && orphan_bytes > 0;
    let cw = if u.tab == Tab::Leftovers { aicheck::ui::chip_width(&u.ai, ui, ai_data) } else { 0.0 };
    // the status ends before Rescan (once a scan is done) and the tabs
    let rescan_room = if phase == Phase::Ready { 60.0 } else { 0.0 };
    let sub_right = r.right() - sp::PAGE_EDGE - 260.0 - 16.0 - rescan_room;
    let (btns, chip_at) = page_header_room(&p, r, "Uninstaller", &sub, TEXT_DIM(), cw, sub_right);
    let labels_owned = [
        "Apps".to_string(),
        if orphan_bytes > 0 { format!("Leftovers · {}", human_bytes(orphan_bytes)) } else { "Leftovers".into() },
    ];
    let labels: Vec<&str> = labels_owned.iter().map(|s| s.as_str()).collect();
    let tw = 260.0;
    let tr = Rect::from_min_max(Pos2::new(btns.right() - tw, btns.top()), btns.max);
    let cur = match u.tab {
        Tab::Apps => 0,
        Tab::Leftovers => 1,
    };
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("un-tab"), &labels, cur) {
        u.tab = [Tab::Apps, Tab::Leftovers][i];
    }
    // scan again (apps and leftovers), once the last scan is done
    if phase == Phase::Ready && link(ui, Pos2::new(tr.left() - 16.0, tr.center().y), Align2::RIGHT_CENTER, "Rescan", 12.0).on_hover_text("Look for apps and leftovers again").clicked() {
        u.rescan();
    }
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    // a dialog on top (the review, or the move under way) holds the page
    // still: its lists don't scroll along with the dialog's
    let modal = u.review.is_some() || u.st.lock_ok().working.is_some();
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
                u.review = Some(Review { targets: vec![t], open: [0].into() });
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
    working(u, ui, r);
    result_bar(u, ui, r);
    if hovering {
        let pr = r.shrink(16.0);
        p.rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, Color32::from_black_alpha(140));
        p.rect_filled(pr, 16.0, with_alpha(TINT(), 0.18));
        dashed_rect(&p, pr, 16.0, with_alpha(GREEN(), 0.9));
        tool_chip(&p, Rect::from_center_size(pr.center() - Vec2::new(0.0, 30.0), Vec2::splat(56.0)), Icon::AppRemove, TINT());
        p.text(pr.center() + Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, "Drop to uninstall", bold(22.0), TEXT());
    }
}

/// The start screen before the first scan: what the scan finds, and Scan for
/// apps. Returns true when it was clicked.
fn fresh_start(ui: &mut Ui, area: Rect) -> bool {
    let p = ui.painter().clone();
    let c = area.center() - Vec2::new(0.0, 40.0);
    tool_chip(&p, Rect::from_center_size(c - Vec2::new(0.0, 70.0), Vec2::splat(64.0)), Icon::AppRemove, TINT());
    p.text(Pos2::new(c.x, c.y - 18.0), Align2::CENTER_CENTER, "Remove apps and what they leave behind", bold(18.0), TEXT());
    let w = area.width().min(440.0);
    text_wrap(&p, Pos2::new(c.x, c.y + 6.0), Align2::CENTER_TOP, "The scan lists your apps with their size and files, and finds data left by apps that are gone. Nothing is removed until you choose.", ty::body(), TEXT_DIM(), w, 3);
    let btn = Vec2::new(200.0, 44.0);
    let br = Rect::from_center_size(Pos2::new(c.x, c.y + 104.0), btn);
    let resp = ui.interact(br, ui.scope_id().with("un-fresh-scan"), Sense::click());
    let hov = ui.ctx().animate_bool(resp.id, resp.hovered());
    liquid_capsule(&p, br, Some(BLUE()), if resp.is_pointer_button_down_on() { 1.6 } else { hov }, 1.0);
    p.text(br.center(), Align2::CENTER_CENTER, "Scan for apps", bold(16.0), on_color(BLUE()));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    p.text(Pos2::new(c.x, br.bottom() + 22.0), Align2::CENTER_CENTER, "Or drop an app on this window to remove it", font(12.0), TEXT_FAINT());
    resp.clicked()
}

// ---------------------------------------------------------------- Apps

const FILTERS: [(&str, Filter); 6] = [
    ("All", Filter::All),
    ("Unused 90+ days", Filter::Unused),
    ("Large", Filter::Large),
    ("App Store", Filter::Source(Source::AppStore)),
    ("Homebrew", Filter::Source(Source::Homebrew)),
    ("Setapp", Filter::Source(Source::Setapp)),
];

fn chip_row(ui: &mut Ui, p: &egui::Painter, x: f32, y: f32, cur: &mut Filter) -> f32 {
    let mut x = x;
    for (i, (label, f)) in FILTERS.iter().enumerate() {
        let g = p.layout_no_wrap(label.to_string(), font(12.0), TEXT());
        let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(g.size().x + 22.0, 26.0));
        let resp = ui.interact(r, ui.scope_id().with(("un-filter", i)), Sense::click());
        let on = *cur == *f;
        p.rect_filled(r, 13.0, if on { with_alpha(BLUE(), 0.22) } else if resp.hovered() { white(10) } else { Color32::TRANSPARENT });
        p.rect_stroke(r, 13.0, Stroke::new(1.0, if on { with_alpha(ACCENT(), 0.45) } else { white(26) }), StrokeKind::Inside);
        p.text(r.center(), Align2::CENTER_CENTER, *label, font(12.0), if on { TEXT() } else { TEXT_DIM() });
        if resp.clicked() {
            *cur = *f;
        }
        x = r.right() + 6.0;
    }
    x
}

fn apps_tab(u: &mut Uninstaller, ui: &mut Ui, body: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let x0 = body.left() + 24.0;
    let x1 = body.right() - 24.0;
    let fy = body.top() + 12.0;
    let sr = Rect::from_min_size(Pos2::new(x0, fy - 1.0), Vec2::new(190.0, 28.0));
    let mut q = std::mem::take(&mut u.search);
    search_field(ui, sr, &mut q, "Search apps", false);
    u.search = q;
    let mut f = u.filter;
    chip_row(ui, &p, sr.right() + 10.0, fy, &mut f);
    u.filter = f;
    // sort: click to cycle
    let label = match u.sort {
        Sort::Size => "Size",
        Sort::Name => "Name",
        Sort::LastUsed => "Last used",
    };
    let g = p.layout_no_wrap(label.to_string(), font(12.0), TEXT_SOFT());
    let sr = Rect::from_min_max(Pos2::new(x1 - g.size().x - 30.0, fy), Pos2::new(x1, fy + 26.0));
    let resp = ui.interact(sr, ui.scope_id().with("un-sort"), Sense::click()).on_hover_text("Sort by size, name or last used");
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
        u.selected = ids.first().map(|&i| u.st.lock_ok().apps[i].path.clone());
    }
    glass_card(&p, list, R_CARD, 0.0, false);
    // column heads
    let hy = list.top() + sp::TABLE_HEAD_H / 2.0;
    // rows live in a scroll area that keeps a gutter on the right for the
    // bar (R12): the columns line up with the rows, not with the card edge
    let c_size = list.right() - sp::SCROLL_GUTTER - sp::L;
    let c_used = c_size - SIZE_W - sp::L;
    let c_name = list.left() + 14.0 + 16.0 + 10.0 + 26.0 + 10.0;
    section_header(&p, Pos2::new(c_name, hy), Align2::LEFT_CENTER, "Name");
    section_header(&p, Pos2::new(c_used, hy), Align2::RIGHT_CENTER, "Last used");
    section_header(&p, Pos2::new(c_size, hy), Align2::RIGHT_CENTER, "Size");
    hairline(&p, Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H), Pos2::new(list.right(), list.top() + sp::TABLE_HEAD_H));
    let rows = Rect::from_min_max(Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H + 1.0), Pos2::new(list.right(), list.bottom() - 2.0));
    if ids.is_empty() {
        let (msg, hint) = match u.st.lock_ok().phase {
            // Idle = not scanned (no Full Disk Access yet: the lock says so)
            Phase::Idle => ("", ""),
            Phase::Listing => ("Looking for apps…", ""),
            _ if u.search.is_empty() && u.filter == Filter::All => ("No apps found", "Apps in /Applications and ~/Applications show here."),
            _ => ("Nothing matches", "Try another filter or search."),
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
                if sel {
                    p.rect_filled(r.shrink2(Vec2::new(4.0, 1.0)), 7.0, with_alpha(BLUE(), 0.30));
                    p.rect_stroke(r.shrink2(Vec2::new(4.0, 1.0)), 7.0, Stroke::new(1.0, with_alpha(ACCENT(), 0.55)), StrokeKind::Inside);
                } else if resp.hovered() {
                    p.rect_filled(r.shrink2(Vec2::new(4.0, 1.0)), 7.0, white(8));
                }
                if resp.clicked() {
                    u.selected = Some(a.path.clone());
                }
                let dim = a.locked.is_some();
                let cb = Rect::from_center_size(Pos2::new(r.left() + 14.0 + 8.0, r.center().y), Vec2::splat(15.0));
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
                let tc = if dim { TEXT_FAINT() } else { TEXT() };
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
                // the name gets the room first; the source chip only shows
                // when both fit
                let room = c_used - 80.0 - c_name;
                let name_w = p.layout_no_wrap(a.name.clone(), font(13.0), tc).size().x;
                let src = if (dim && a.locked == Some("This is Cmd+Meow")) || name_w + 70.0 > room { None } else { Some(a.source) };
                let name_r = text_fit(&p, Pos2::new(c_name, r.center().y), Align2::LEFT_CENTER, &a.name, font(13.0), tc, room);
                if let Some(s) = src {
                    source_chip(&p, Pos2::new(name_r.right() + 7.0, r.center().y), s);
                }
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
    let t = p.text(Pos2::new(bar.left() + 14.0, bar.center().y), Align2::LEFT_CENTER, format!("{n} app{} selected", if n == 1 { "" } else { "s" }), bold(13.0), TEXT());
    p.text(Pos2::new(t.right() + 10.0, bar.center().y), Align2::LEFT_CENTER, format!("{} with their data", human_bytes(bytes)), font(13.0), TEXT_DIM());
    let label = format!("Review {n} app{}…", if n == 1 { "" } else { "s" });
    let bw = button_width(&p, &label, BTN_PRIMARY);
    let br = Rect::from_min_size(Pos2::new(bar.right() - 12.0 - bw, bar.center().y - 15.0), Vec2::new(bw, 30.0));
    if pill_button(ui, br, &label, BTN_PRIMARY, true).clicked() {
        let g = u.st.lock_ok();
        let targets: Vec<Target> = g.apps.iter().filter(|a| u.checked.contains(&a.path) && a.locked.is_none()).map(|a| a.target(false)).collect();
        drop(g);
        u.review = Some(Review { targets, open: Default::default() });
    }
    let cw = button_width(&p, "Clear", BTN_SECONDARY);
    let cr = Rect::from_min_size(Pos2::new(br.left() - 8.0 - cw, br.top()), Vec2::new(cw, 30.0));
    if pill_button(ui, cr, "Clear", BTN_SECONDARY, true).clicked() {
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
        let g = p.layout_no_wrap("Running".into(), font(11.0), GREEN());
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
        p.text(Pos2::new(x0 + 24.0, y + 10.0), Align2::LEFT_CENTER, "Finding its files…", font(13.0), TEXT_DIM());
        crate::kit::glass::animate(&ctx);
        return;
    }
    // size
    let total = a.total();
    let app_b = a.app_bytes();
    let tr = p.text(Pos2::new(x0, y), Align2::LEFT_TOP, human_bytes(total), bold(22.0), TEXT());
    p.text(Pos2::new(tr.right() + 6.0, tr.bottom() - 3.0), Align2::LEFT_BOTTOM, "in total", font(13.0), TEXT_DIM());
    y = tr.bottom() + 8.0;
    let data_col = AMBER();
    let tf = total.max(1) as f32;
    stacked_bar(&p, Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, 6.0)), &[(app_b as f32 / tf, TINT()), ((total - app_b) as f32 / tf, data_col)]);
    y += 12.0;
    let l1 = p.text(Pos2::new(x0, y), Align2::LEFT_TOP, format!("App {}", human_bytes(app_b)), font(11.0), TEXT_DIM());
    p.text(Pos2::new(l1.right() + 12.0, y), Align2::LEFT_TOP, format!("Data {}", human_bytes(total - app_b)), font(11.0), data_col);
    y += 26.0;

    // files
    let n_sel = a.items.iter().filter(|i| i.selected).count();
    section_header(&p, Pos2::new(x0, y), Align2::LEFT_TOP, &format!("Moves to Trash · {n_sel} of {}", a.items.len()));
    if link(ui, Pos2::new(x1, y), Align2::RIGHT_TOP, "Show in Finder", 11.0).clicked() {
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
                right = outline_chip(&p, Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, "admin", TEXT_FAINT()).left() - 6.0;
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
    if pill_button(ui, reset_r, "Reset…", BTN_SECONDARY, has_data).on_hover_text("Remove its settings and data, keep the app").clicked() {
        u.review = Some(Review { targets: vec![a.target(true)], open: [0].into() });
    }
    let label = if running { "Quit and uninstall" } else { "Uninstall…" };
    if pill_button(ui, main_r, label, BTN_PRIMARY, n_sel > 0).clicked() {
        u.review = Some(Review { targets: vec![a.target(false)], open: [0].into() });
    }
}

// ---------------------------------------------------------------- review sheet

fn review(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
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
        (true, _) => format!("Reset {}?", rv.targets[0].name),
        (false, 1) => format!("Uninstall {}?", rv.targets[0].name),
        (false, n) => format!("Uninstall {n} apps?"),
    };
    let title = if rv.targets.iter().all(|t| t.app.is_none()) && !reset { if n == 1 { format!("Remove what {} left?", rv.targets[0].name) } else { format!("Remove what {n} apps left?") } } else { title };
    p.text(Pos2::new(x0, y), Align2::LEFT_TOP, title, bold(15.0), TEXT());
    y += 30.0;
    let sub = if reset { "Settings, caches and data go to the Trash. The app stays" } else { "Everything goes to the Trash" };
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
            let sz = p.text(Pos2::new(row.right() - 12.0, row.center().y), Align2::RIGHT_CENTER, human_bytes(target_bytes(t)), font(13.0), TEXT());
            let (ly1, ly2) = two_lines_y(&p, row.center().y, &bold(13.0), &font(11.0));
            text_fit(&p, Pos2::new(ir.right() + 10.0, ly1), Align2::LEFT_CENTER, &t.name, bold(13.0), TEXT(), sz.left() - ir.right() - 20.0);
            let open_now = t.app.is_some() && running.contains(&t.name);
            let line = if open_now { format!("Open now · {}", target_line(t)) } else { target_line(t) };
            text_fit(&p, Pos2::new(ir.right() + 10.0, ly2), Align2::LEFT_CENTER, &line, font(11.0), if open_now { AMBER() } else { TEXT_FAINT() }, sz.left() - ir.right() - 20.0);
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
                        right = outline_chip(&p, Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, "admin", TEXT_FAINT()).left() - 6.0;
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
        text_fit(&p, Pos2::new(x0 + 22.0, y + 9.0), Align2::LEFT_CENTER, &format!("{admin} item{} your password. macOS asks once.", if admin == 1 { " needs" } else { "s need" }), font(12.0), TEXT_DIM(), x1 - x0 - 22.0);
    }
    let Some(rv) = u.review.as_ref() else { return };
    let bytes: u64 = rv.targets.iter().map(target_bytes).sum();
    let any = rv.targets.iter().any(|t| target_bytes(t) > 0 || visible_items(t).iter().any(|i| i.selected));
    let by = m.bottom() - 18.0 - 30.0;
    let ok_label = if reset { "Reset" } else { "Move to Trash" };
    let okw = button_width(&p, ok_label, BTN_PRIMARY);
    let ok = Rect::from_min_size(Pos2::new(x1 - okw, by), Vec2::new(okw, 30.0));
    let cw = button_width(&p, "Cancel", BTN_SECONDARY);
    let cancel = Rect::from_min_size(Pos2::new(ok.left() - 8.0 - cw, by), Vec2::new(cw, 30.0));
    let fr = p.text(Pos2::new(x0, ok.center().y), Align2::LEFT_CENTER, "Frees ", font(13.0), TEXT_DIM());
    let br = p.text(Pos2::new(fr.right(), ok.center().y), Align2::LEFT_CENTER, human_bytes(bytes), bold(13.0), TEXT());
    p.text(Pos2::new(br.right(), ok.center().y), Align2::LEFT_CENTER, " after you empty the Trash", font(13.0), TEXT_DIM());
    if pill_button(ui, cancel, "Cancel", BTN_SECONDARY, true).clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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

fn working(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    let Some(line) = u.st.lock_ok().working.clone() else { return };
    let ctx = ui.ctx().clone();
    scrim(ui, r, "un-working-scrim");
    let p = ui.painter().clone();
    let m = Rect::from_center_size(r.center(), Vec2::new(360.0, 74.0));
    modal_card(&p, m);
    spinner(&p, Pos2::new(m.left() + 30.0, m.center().y), 9.0, ctx.input(|i| i.time), ACCENT());
    text_fit(&p, Pos2::new(m.left() + 52.0, m.center().y), Align2::LEFT_CENTER, &line, font(13.0), TEXT(), m.width() - 70.0);
    crate::kit::glass::animate(&ctx);
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

fn result_bar(u: &mut Uninstaller, ui: &mut Ui, r: Rect) {
    let fresh = u.st.lock_ok().last.as_ref().is_some_and(|(_, t)| t.elapsed() < UNDO_FOR);
    if !fresh || u.st.lock_ok().working.is_some() {
        return;
    }
    let Some(o) = result_text(u) else { return };
    let failed = o.failed;
    let p = ui.painter().clone();
    let w = 560.0f32.min(r.width() - 48.0);
    let bar = Rect::from_center_size(Pos2::new(r.center().x, r.bottom() - 46.0), Vec2::new(w, 48.0));
    let shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(110) };
    p.add(shadow.as_shape(bar, CornerRadius::same(12)));
    p.rect_filled(bar, 12.0, POPOVER());
    p.rect_stroke(bar, 12.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
    let c = Pos2::new(bar.left() + 25.0, bar.center().y);
    let ok_col = if failed > 0 { AMBER() } else { TINT() };
    p.circle_filled(c, 11.0, ok_col);
    icon(&p, if failed > 0 { Icon::Info } else { Icon::Check }, Rect::from_center_size(c, Vec2::splat(12.0)), ON_ACTION);
    let close = Rect::from_center_size(Pos2::new(bar.right() - 22.0, bar.center().y), Vec2::splat(22.0));
    if round_button(ui, close, Icon::Close, "Dismiss").clicked() {
        u.st.lock_ok().last = None;
        return;
    }
    let mut right = close.left() - 8.0;
    if o.can_undo {
        let uw = button_width(&p, "Undo", BTN_SECONDARY);
        let ur = Rect::from_min_size(Pos2::new(right - uw, bar.center().y - 15.0), Vec2::new(uw, 30.0));
        if pill_button(ui, ur, "Undo", BTN_SECONDARY, true).clicked() {
            u.undo();
            return;
        }
        right = ur.left() - 8.0;
    }
    if o.can_retry {
        let rw = button_width(&p, "Try again", BTN_PRIMARY);
        let rr = Rect::from_min_size(Pos2::new(right - rw, bar.center().y - 15.0), Vec2::new(rw, 30.0));
        if pill_button(ui, rr, "Try again", BTN_PRIMARY, true).clicked() {
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
                        u.review = Some(Review { targets: vec![o.target()], open: [0].into() });
                    } else {
                        let mut g = u.st.lock_ok();
                        for x in g.orphans.iter_mut() {
                            x.selected = x.id == o.id;
                        }
                    }
                }
                u.ai.done(&r);
            }
            None => {}
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
        if busy { "Looking for leftovers…".to_string() } else { "No leftovers found".to_string() }
    } else {
        format!("{} from {} removed app{}", human_bytes(total), orphans.len(), if orphans.len() == 1 { "" } else { "s" })
    };
    let label = format!("Move {} to Trash", sel.len());
    let bw = button_width(&p, &label, BTN_PRIMARY);
    let br = Rect::from_min_size(Pos2::new(hero.right() - 16.0 - bw, hero.center().y - 15.0), Vec2::new(bw, 30.0));
    p.text(Pos2::new(hero.left() + 70.0, hero.center().y), Align2::LEFT_CENTER, title, bold(15.0), TEXT());
    if !orphans.is_empty() && pill_button(ui, br, &label, BTN_PRIMARY, !sel.is_empty()).on_hover_text(format!("{} selected", human_bytes(sel_bytes))).clicked() {
        let targets: Vec<Target> = sel.iter().map(|o| o.target()).collect();
        u.review = Some(Review { targets, open: Default::default() });
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
        ("Offer cleanup when an app goes to the Trash", watch),
        ("Show a drop zone while dragging an app", zone),
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
    let c_name = list.left() + 14.0 + 16.0 + 12.0;
    let hy = list.top() + sp::TABLE_HEAD_H / 2.0;
    section_header(&p, Pos2::new(c_name, hy), Align2::LEFT_CENTER, "Removed app");
    section_header(&p, Pos2::new(c_where, hy), Align2::LEFT_CENTER, "Where");
    section_header(&p, Pos2::new(c_when, hy), Align2::RIGHT_CENTER, "Last touched");
    section_header(&p, Pos2::new(c_size, hy), Align2::RIGHT_CENTER, "Size");
    hairline(&p, Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H), Pos2::new(list.right(), list.top() + sp::TABLE_HEAD_H));
    let rows_r = Rect::from_min_max(Pos2::new(list.left(), list.top() + sp::TABLE_HEAD_H + 1.0), Pos2::new(list.right(), list.bottom() - sp::S));
    // whole rows only: a row cut in half by the card edge looks broken
    let rows_r = Rect::from_min_size(rows_r.min, Vec2::new(rows_r.width(), ((rows_r.height() / 40.0).floor() * 40.0).max(40.0).min(rows_r.height())));
    if orphans.is_empty() {
        let msg = if busy { "Still looking…" } else { "Nothing left behind" };
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
            // only rows in view: each one joins its paths for the tooltip
            if !ui.is_rect_visible(r) {
                continue;
            }
            let resp = ui.interact(r, ui.scope_id().with(("un-orow", &o.id)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r.shrink2(Vec2::new(4.0, 1.0)), 7.0, white(8));
            }
            let cb = Rect::from_center_size(Pos2::new(r.left() + 14.0 + 8.0, r.center().y), Vec2::splat(15.0));
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
            ("remove", "move that app's leftovers to the Trash (the user confirms in a review sheet first)", "Remove"),
            ("review", "select it in the list so the user can look at its files first", "Review"),
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
            text_fit(&p, Pos2::new(x0 + 34.0, rect.center().y), Align2::LEFT_CENTER, &format!("Finding {name}'s files…"), font(13.0), TEXT_DIM(), x1 - x0 - 34.0);
            crate::kit::glass::animate(&ctx);
        }
        Drop::Refused(name, why) => {
            icon(&p, Icon::Info, Rect::from_min_size(Pos2::new(x0, rect.top() + 12.0), Vec2::splat(16.0)), AMBER());
            text_fit(&p, Pos2::new(x0 + 24.0, rect.top() + 20.0), Align2::LEFT_CENTER, name, bold(12.0), TEXT(), x1 - x0 - 24.0);
            text_fit(&p, Pos2::new(x0 + 24.0, rect.top() + 38.0), Align2::LEFT_CENTER, why, font(11.0), TEXT_DIM(), x1 - x0 - 24.0);
            let ok = Rect::from_min_size(Pos2::new(x1 - 52.0, rect.bottom() - 34.0), Vec2::new(52.0, 26.0));
            if pill_button(ui, ok, "OK", BTN_SECONDARY, true).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
            }
        }
        Drop::Confirm(t) => {
            let ir = Rect::from_min_size(Pos2::new(x0, rect.top() + 9.0), Vec2::splat(28.0));
            app_icon(u, &ctx, &p, ir, t.app.as_deref(), &t.name, &mut budget);
            let running = is_running(u, &t.id);
            text_fit(&p, Pos2::new(ir.right() + 8.0, ir.top() + 6.0), Align2::LEFT_CENTER, &format!("Uninstall {}?", t.name), bold(12.0), TEXT(), x1 - ir.right() - 8.0);
            let line = if running { format!("{} · open now", target_line(t)) } else { target_line(t) };
            text_fit(&p, Pos2::new(ir.right() + 8.0, ir.top() + 22.0), Align2::LEFT_CENTER, &line, font(11.0), TEXT_DIM(), x1 - ir.right() - 8.0);
            let by = rect.bottom() - 8.0 - 26.0;
            let main = if running { "Quit and move to Trash" } else { "Move to Trash" };
            let mw = button_width(&p, main, BTN_PRIMARY) - 6.0;
            let mr = Rect::from_min_size(Pos2::new(x1 - mw, by), Vec2::new(mw, 26.0));
            let dw = button_width(&p, "Details…", BTN_SECONDARY) - 6.0;
            let dr = Rect::from_min_size(Pos2::new(mr.left() - 6.0 - dw, by), Vec2::new(dw, 26.0));
            let cr = Rect::from_min_size(Pos2::new(x0 - 4.0, by), Vec2::new(64.0, 26.0));
            if link(ui, Pos2::new(cr.left() + 6.0, cr.center().y), Align2::LEFT_CENTER, "Cancel", 12.0).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
            }
            if pill_button(ui, dr, "Details…", BTN_SECONDARY, true).clicked() {
                u.st.lock_ok().drop = Drop::Idle;
                u.selected = t.app.clone();
                u.tab = Tab::Apps;
                u.review = Some(Review { targets: vec![t.clone()], open: [0].into() });
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
                let uw = if can_retry { button_width(&p, "Try again", BTN_PRIMARY) } else { 64.0 };
                let ur = Rect::from_min_size(Pos2::new(r.right() - 14.0 - uw, r.center().y - 14.0), Vec2::new(uw, 28.0));
                let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(11.0));
                text_fit(&p, Pos2::new(c.x + 30.0, ly1), Align2::LEFT_CENTER, &msg, bold(13.0), TEXT(), ur.left() - c.x - 40.0);
                let sub = detail.unwrap_or_else(|| "Undo puts everything back".into());
                text_fit(&p, Pos2::new(c.x + 30.0, ly2), Align2::LEFT_CENTER, &sub, font(11.0), if failed > 0 { AMBER() } else { TEXT_DIM() }, ur.left() - c.x - 40.0);
                if can_retry {
                    if pill_button(ui, ur, "Try again", BTN_PRIMARY, true).clicked() {
                        retry(u);
                        u.done_until = None;
                    }
                } else if can_undo && pill_button(ui, ur, "Undo", BTN_SECONDARY, true).clicked() {
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
        let first = names.first().cloned().unwrap_or_else(|| "the app".into());
        tool_chip(&p, Rect::from_min_size(Pos2::new(r.left() + 16.0, r.center().y - 20.0), Vec2::splat(40.0)), Icon::AppRemove, TINT());
        let x = r.left() + 70.0;
        let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(12.0));
        text_fit(&p, Pos2::new(x, ly1), Align2::LEFT_CENTER, if over { "Release to review" } else { "Drop here to uninstall" }, bold(13.0), if over { GREEN() } else { TEXT() }, r.right() - x - 14.0);
        text_fit(&p, Pos2::new(x, ly2), Align2::LEFT_CENTER, &format!("{first} and its files, after you confirm"), font(12.0), TEXT_DIM(), r.right() - x - 14.0);
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
        p.text(Pos2::new(r.right() - 12.0, r.top() + 12.0), Align2::RIGHT_TOP, "now", font(11.0), TEXT_DIM());
        text_fit(&p, Pos2::new(tx, r.top() + 30.0), Align2::LEFT_TOP, &format!("{} was removed", n.target.name), bold(13.0), TEXT(), r.right() - tx - 12.0);
        let k = n.target.items.len();
        text_fit(&p, Pos2::new(tx, r.top() + 48.0), Align2::LEFT_TOP, &format!("{} of its data is still on your Mac ({k} item{}).", human_bytes(target_bytes(&n.target)), if k == 1 { "" } else { "s" }), font(12.0), TEXT_SOFT(), r.right() - tx - 12.0);
        let by = r.bottom() - 12.0 - 28.0;
        let w = (r.width() - 24.0 - 6.0) / 2.0;
        if pill_button(ui, Rect::from_min_size(Pos2::new(x0, by), Vec2::new(w, 28.0)), "Keep", BTN_SECONDARY, true).clicked() {
            act = Some(false);
        }
        if pill_button(ui, Rect::from_min_size(Pos2::new(x0 + w + 6.0, by), Vec2::new(w, 28.0)), "Review leftovers…", BTN_SECONDARY, true).clicked() {
            act = Some(true);
        }
    });
    match act {
        Some(true) => {
            u.st.lock_ok().notice = None;
            u.tab = Tab::Leftovers;
            u.review = Some(Review { targets: vec![n.target], open: [0].into() });
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
            app("Safari", "com.apple.Safari", "26.1", Source::Apple, 0, &[], Some(0), Some("Part of macOS")),
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
        u.selected = Some(PathBuf::from("/Applications/Docker.app"));
        match view {
            "uninstaller" | "uninstaller-apps" => {}
            "uninstaller-leftovers" => {
                u.tab = Tab::Leftovers;
                use crate::kit::aicheck::Check;
                u.ai.set_demo(
                    "Leftovers of 2 removed apps",
                    vec![
                        Check::demo_rec("remove", "Remove", "Remove Zoom leftovers", "412 MB", "Zoom was removed in March; nothing else uses these", "Zoom"),
                        Check::demo_rec("review", "Review", "Review Docker data", "6.8 GB", "Holds images and volumes; check before removing", "Docker"),
                    ],
                );
            }
            "uninstaller-review" => {
                u.filter = Filter::Unused;
                let g = u.st.lock_ok();
                let pick = ["Android Studio", "Final Cut Pro", "Zoom"];
                let targets: Vec<_> = g.apps.iter().filter(|a| pick.contains(&a.name.as_str())).map(|a| a.target(false)).collect();
                u.checked = g.apps.iter().filter(|a| pick.contains(&a.name.as_str())).map(|a| a.path.clone()).collect();
                drop(g);
                u.selected = Some(PathBuf::from("/Applications/Android Studio.app"));
                u.review = Some(Review { targets, open: [0].into() });
            }
            "uninstaller-done" => {
                let t = u.st.lock_ok().apps.iter().find(|a| a.name == "Android Studio").unwrap().target(false);
                u.run(vec![t]);
                u.st.lock_ok().last.as_mut().unwrap().1 = Instant::now();
            }
            "uninstaller-reset" => {
                let t = u.st.lock_ok().apps.iter().find(|a| a.name == "Docker").unwrap().target(true);
                u.review = Some(Review { targets: vec![t], open: [0].into() });
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
