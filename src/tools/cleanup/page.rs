//! Cleanup screens: start → scanning → Claude reviewing paths → results (with
//! "needs your OK" questions) → cleaning → done.

use crate::kit::sync::LockExt;
use super::engine::{self as cleanup, Group, Item, Phase, Verdict, Who};
use super::agent::Agent;
use super::Cleanup as App;
use crate::core::tool::Cx;
use crate::kit::aicheck::{self, ui::Event};
use crate::kit::claude_code::{self as cc, Availability};
use crate::kit::glass::{self, *};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use crate::kit::native::OpenApp;
use std::path::PathBuf;

/// Window corner radius (the footer rounds its bottom-right corner to match).
const WIN_R: f32 = 12.0;

pub fn page(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    if app.auto_tab {
        super::auto_page::tab(app, ui, r);
        return;
    }
    let phase = app.clean.lock_ok().phase.clone();
    page_body(app, ui, r, cx, &phase);
    // the Clean · Auto tabs left of the page action; under the clean loader they stay put
    if !(phase == Phase::Cleaning || (phase == Phase::Done && app.clean_loading)) {
        super::auto_page::header(app, ui, r, &phase);
    }
}

fn page_body(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx, phase: &Phase) {
    match phase.clone() {
        Phase::Intro => intro(app, ui, r, cx),
        Phase::Scanning | Phase::Judging => working(app, ui, r, phase.clone()),
        Phase::Results => results(app, ui, r),
        // the results stay under the loader; rows leave as they go
        Phase::Cleaning => {
            results(app, ui, r);
            clean_loader(app, ui, r);
        }
        Phase::Done => {
            done(app, ui, r);
            if app.clean_loading {
                clean_loader(app, ui, r);
            }
        }
    }
}

/// The removal loader over the window while cleaning, and its result.
fn clean_loader(app: &mut App, ui: &mut Ui, r: Rect) {
    use crate::kit::loader::{self, Click};
    let status = {
        let g = app.clean.lock_ok();
        clean_status(&g)
    };
    match loader::show(ui, r, "cleanup", &status) {
        Some(Click::Stop) => cleanup::stop_clean(&app.clean),
        Some(Click::Done | Click::PutBack | Click::Ended) => app.clean_loading = false,
        None => {}
    }
}

/// What the loader says for the clean as it stands.
fn clean_status(g: &cleanup::State) -> crate::kit::loader::Status {
    use crate::kit::loader::{Count, Status};
    if g.phase == Phase::Cleaning {
        // Stop pressed: a deletion stops at the next file, a Trash move or command finishes first
        let stopping = g.cancel.load(std::sync::atomic::Ordering::SeqCst);
        let title = if stopping { ts!("cleanup.stopping") } else { ts!("cleanup.cleaning") };
        return Status::Working { title: title.into(), count: Count::Bytes { done: g.clean_done, total: g.clean_total }, current: g.current.clone(), can_stop: !stopping };
    }
    let freed = g.cleaned_bytes.saturating_sub(g.trashed_bytes);
    let what = if g.trashed_bytes == 0 {
        t!("cleanup.deleted", size = human_bytes(g.cleaned_bytes)).into_owned()
    } else if freed == 0 {
        t!("cleanup.moved_trash", size = human_bytes(g.trashed_bytes)).into_owned()
    } else {
        t!("cleanup.cleaned", size = human_bytes(g.cleaned_bytes)).into_owned()
    };
    if g.stopped {
        return Status::Stopped { sub: what };
    }
    let n = g.failed.len();
    // nothing went: no ✓, and why
    if g.cleaned_count == 0 && n > 0 {
        let problem = if n == 1 { g.failed[0].1.clone() } else { tn!("cleanup.failed", n = n).into_owned() };
        return Status::Failed { title: ts!("cleanup.nothing_removed").into(), problem };
    }
    let problem = (n > 0).then(|| tn!("cleanup.failed", n = n).into_owned());
    let items = tn!("cleanup.items", n = g.cleaned_count, count = crate::kit::loader::grouped(g.cleaned_count)).into_owned();
    Status::Finished { title: what, sub: items, problem, put_back: false }
}

/// Whether Claude can judge paths now (always in the demo).
fn claude_now(app: &App) -> Availability {
    if app.demo {
        app.demo_claude.unwrap_or(Availability::Ready)
    } else {
        cc::availability()
    }
}

/// The agent that will check the paths, if it can right now.
fn ai_agent(app: &App) -> Option<Agent> {
    let ok = match app.agent {
        Agent::Claude => claude_now(app).ready(),
        a => app.demo || a.ready(),
    };
    (app.use_ai && ok).then_some(app.agent)
}

fn home_str() -> String {
    dirs::home_dir().unwrap_or_default().display().to_string()
}

fn short_path(p: &std::path::Path) -> String {
    p.display().to_string().replacen(&home_str(), "~", 1)
}

// ---------------------------------------------------------------- start

/// The start screen, no cards (R13): on the left the disk ring and Scan, on
/// the right every group a scan looks through, one 44 pt row each. After a
/// scan the rows carry what the last one found, as a bar and a size.
fn intro(app: &mut App, ui: &mut Ui, r: Rect, _cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    page_header(&p, r, ts!("cleanup.name"), "", TEXT_DIM());
    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, r.top() + PAGE_HEADER_H + 1.0 + sp::L), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - sp::L));
    let (total, free) = crate::kit::fs::volume_space("/");
    let last = if app.demo && !app.demo_fresh { Some(super::engine::LastScan { when: chrono::Utc::now().timestamp() - 2 * 86400, groups: vec![14_200_000_000, 9_800_000_000, 21_600_000_000, 6_100_000_000, 2_200_000_000, 6_400_000_000, 1_300_000_000, 3_400_000_000] }) } else { super::engine::last_scan() };
    let sizes: Option<Vec<u64>> = last.as_ref().filter(|l| l.groups.len() == Group::COUNT).map(|l| Group::ALL.iter().enumerate().map(|(i, _)| l.groups.get(i).copied().unwrap_or(0)).collect());

    // the block: ring column, a hairline, the rows; centred in the page
    const LEFT_W: f32 = 240.0;
    const GAP: f32 = 48.0;
    const BTN: Vec2 = Vec2::new(200.0, 44.0);
    let left_h = DISK_RING_H + sp::XXL + BTN.y;
    let head_h = if sizes.is_some() { sp::SECTION_H } else { 0.0 };
    let rows_h = head_h + Group::ALL.len() as f32 * sp::ROW_H;
    let block_w = body.width().min(840.0);
    let block_h = left_h.max(rows_h);
    let top = (body.center().y - block_h / 2.0).max(body.top());
    let block = Rect::from_min_size(Pos2::new(body.center().x - block_w / 2.0, top), Vec2::new(block_w, block_h));

    // left: the disk and Scan
    let lc = block.left() + LEFT_W / 2.0;
    let ly = block.top() + (block_h - left_h) / 2.0;
    disk_ring(&p, Pos2::new(lc, ly), ts!("health.macintosh_hd"), total, free, tint::CLEANUP());
    let br = Rect::from_center_size(Pos2::new(lc, ly + DISK_RING_H + sp::XXL + BTN.y / 2.0), BTN);
    // an auto run is cleaning: a scan now would race it
    if scan_button(ui, br) && !app.auto_run.busy {
        cleanup::start_scan(app.clean.clone(), ctx.clone(), ai_agent(app));
    }

    // a hairline between the columns
    let dx = block.left() + LEFT_W + GAP;
    hairline(&p, Pos2::new(dx, block.top()), Pos2::new(dx, block.bottom()));

    // right: the groups
    let x0 = dx + 1.0 + GAP;
    let x1 = block.right();
    let mut y = block.top() + (block_h - rows_h) / 2.0;
    if let (Some(l), Some(s)) = (&last, &sizes) {
        let cy = section_title(&p, x0, y, &t!("cleanup.last_scan", when = super::engine::days_ago(l.when)));
        crate::kit::glass::section_value(&p, x1, cy, &human_bytes(s.iter().sum()));
        y += head_h;
    }
    let max = sizes.as_ref().and_then(|s| s.iter().copied().max()).unwrap_or(0).max(1) as f32;
    // the name column fits the longest group name: app text is never cut (R10)
    let name_w = Group::ALL.iter().map(|g| p.layout_no_wrap(g.label().to_string(), ty::body(), TEXT()).size().x).fold(96.0, f32::max);
    for (k, g) in Group::ALL.into_iter().enumerate() {
        let rr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + sp::ROW_H));
        let resp = ui.interact(rr, ui.scope_id().with(("cleanup-group", k)), Sense::hover());
        if resp.hovered() {
            crate::kit::glass::start_row_fill(&p, rr);
        }
        if k > 0 || sizes.is_some() {
            crate::kit::glass::row_divider(ui, &p, rr.left_top(), rr.right_top(), resp.hovered(), rr.bottom());
        }
        let col = junk_ramp(k);
        let cy = rr.center().y;
        // H9: content 12 in from the row's edges
        let (x0, x1) = (x0 + crate::kit::glass::START_PAD, x1 - crate::kit::glass::START_PAD);
        let ic = Pos2::new(x0 + 14.0, cy);
        p.circle_filled(ic, 14.0, with_alpha(col, 0.14));
        group_icon(&p, g, Rect::from_center_size(ic, Vec2::splat(16.0)), col);
        let nx = x0 + 28.0 + sp::M;
        p.text(Pos2::new(nx, cy), Align2::LEFT_CENTER, g.label(), ty::body(), TEXT());
        if let Some(s) = &sizes {
            let sz = p.text(Pos2::new(x1, cy), Align2::RIGHT_CENTER, human_bytes(s[k]), ty::title(), TEXT());
            let bar_r = Rect::from_min_max(Pos2::new(nx + name_w + sp::M, cy - 2.0), Pos2::new(sz.left().min(x1 - 72.0) - sp::M, cy + 2.0));
            if bar_r.width() > 20.0 {
                bar(&p, bar_r, s[k] as f32 / max, col);
            }
        }
        // P7: beside the list, never over the next row
        crate::kit::glass::tip_beside(resp, g.blurb(), rr.right());
        y += sp::ROW_H;
    }
}

/// A group's glyph (Developer: the SF hammer when there is one).
pub(super) fn group_icon(p: &egui::Painter, g: Group, r: Rect, col: Color32) {
    if matches!(g, Group::Developer) && symbol(p, "hammer", r, col) {
        return;
    }
    let ic = match g {
        Group::AiTools => Icon::Gauge,
        Group::AppCaches => Icon::Folder,
        Group::Developer => Icon::Gear,
        Group::Projects => Icon::Folder,
        Group::Installers => Icon::Down,
        Group::Captures => Icon::Camera,
        Group::Logs => Icon::Doc,
        Group::Trash => Icon::Trash,
    };
    icon(p, ic, r, col);
}

/// The big lime Scan of the start screen. True when clicked.
fn scan_button(ui: &mut Ui, br: Rect) -> bool {
    let id = ui.scope_id().with("fresh-scan");
    let resp = crate::kit::empty::hero_button(ui, br.center(), id, ts!("cleanup.scan"), None, crate::kit::empty::Lead::None, Some(ACTION()), true, br.width());
    resp.clicked()
}

// ---------------------------------------------------------------- scanning / judging / cleaning

fn working(app: &mut App, ui: &mut Ui, r: Rect, phase: Phase) {
    let ctx = ui.ctx().clone();
    let st = app.clean.lock_ok().clone();
    let p = ui.painter().clone();
    let t = ctx.input(|i| i.time);
    let who = st.agent.map(|a| a.name()).unwrap_or("AI");
    let sub = match phase {
        Phase::Judging => t!("cleanup.checking_with", who = who).into_owned(),
        Phase::Cleaning => ts!("cleanup.cleaning").into(),
        _ => ts!("cleanup.scanning").into(),
    };
    // SP17 H5: no "Scanning…" in the header; Stop with its spinner says it
    let btns = page_header(&p, r, ts!("cleanup.name"), "", TEXT_DIM());
    // Stop: back to the start; Skip: the results now, by the built-in rules
    let stop = match phase {
        Phase::Scanning => Some(ts!("cleanup.stop")),
        Phase::Judging => Some(ts!("cleanup.skip")),
        _ => None,
    };
    if let Some(label) = stop {
        let w = [ts!("cleanup.scan_again"), ts!("cleanup.stop"), ts!("cleanup.skip")].iter().map(|l| crate::kit::glass::icon_pill_width(&p, l, BTN_SECONDARY)).fold(0.0, f32::max);
        let sr = Rect::from_min_max(Pos2::new(btns.right() - w, btns.top()), btns.max);
        if crate::kit::glass::busy_pill(ui, sr, label, tint::CLEANUP()).on_hover_text(&sub).clicked() {
            if phase == Phase::Judging {
                cleanup::skip_ai(&app.clean);
            } else {
                cleanup::stop_scan(&app.clean);
            }
            ctx.request_repaint();
            return;
        }
    }

    let x0 = r.left() + 24.0;
    let x1 = r.right() - 24.0;
    let mut y = r.top() + PAGE_HEADER_H + 20.0;
    // headline number
    let (big, caption) = if phase == Phase::Cleaning {
        (human_bytes(st.cleaned_bytes), t!("cleanup.cleaned_so_far", items = items_n(st.cleaned_count)).into_owned())
    } else if phase == Phase::Scanning {
        // the items arrive at the end; the live tallies grow as each place is measured
        let bytes: u64 = st.live.iter().map(|l| l.bytes).sum();
        let n: usize = st.live.iter().map(|l| l.items).sum();
        (human_bytes(bytes), t!("cleanup.found_so_far", items = items_n(n)).into_owned())
    } else {
        (human_bytes(st.items.iter().filter(|i| i.verdict != Verdict::Keep).map(|i| i.bytes).sum()), t!("cleanup.found_so_far", items = items_n(st.items.len())).into_owned())
    };
    let g = p.layout_no_wrap(big, bold(28.0), TEXT());
    let gw = g.size().x;
    p.galley(Pos2::new(x0, y), g, TEXT());
    p.text(Pos2::new(x0 + gw + 10.0, y + 22.0), Align2::LEFT_BOTTOM, caption, font(13.0), TEXT_DIM());
    // while the scan is still listing places the bar creeps (never still)
    let progress = st.shown_progress();
    if progress >= 0.0 {
        p.text(Pos2::new(x1, y + 22.0), Align2::RIGHT_BOTTOM, format!("{:.0}%", progress.clamp(0.0, 1.0) * 100.0), font(13.0), TEXT_DIM());
    }
    y += 40.0;
    // progress bar (an indeterminate sweep while the scan can't count); a
    // native layer that eases between values, so the creep costs no frames
    let track = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, 6.0));
    if progress >= 0.0 {
        crate::kit::spin::bar(&p, "cleanup-progress", track, progress, ACTION());
    } else {
        p.rect_filled(track, 3.0, white(20));
        let w = track.width() * 0.25;
        let f = ((t * 0.6).fract() as f32) * (track.width() + w) - w;
        let seg = Rect::from_min_size(Pos2::new(track.left() + f, track.top()), Vec2::new(w, 6.0)).intersect(track);
        if seg.width() > 0.0 {
            p.rect_filled(seg, 3.0, ACTION());
        }
    }
    y += 16.0;
    // what it's doing right now
    match phase {
        Phase::Judging => {
            let line = if st.ai_status.is_empty() { t!("cleanup.asking", who = who).into_owned() } else { st.ai_status.clone() };
            text_fit(&p, Pos2::new(x0, y + 7.0), Align2::LEFT_CENTER, &line, font(12.0), TEXT_FAINT(), x1 - x0);
        }
        _ => {
            let line = st.current.replacen(&home_str(), "~", 1);
            text_fit(&p, Pos2::new(x0, y + 7.0), Align2::LEFT_CENTER, &middle_ellipsis(&line, 110), mono(11.0), TEXT_FAINT(), x1 - x0);
        }
    }
    y += 34.0;

    // per-group table
    let cols = [x0 + 12.0, x0 + 48.0, x1 - 250.0, x1 - 12.0];
    section_header(&p, Pos2::new(cols[1], y), Align2::LEFT_TOP, ts!("cleanup.col.group"));
    section_header(&p, Pos2::new(cols[2], y), Align2::LEFT_TOP, ts!("cleanup.col.items"));
    section_header(&p, Pos2::new(cols[3], y), Align2::RIGHT_TOP, if phase == Phase::Cleaning { ts!("cleanup.col.selected") } else { ts!("cleanup.col.found") });
    y += 22.0;
    for grp in Group::ALL {
        let row = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 38.0));
        if row.bottom() > r.bottom() - 8.0 {
            break;
        }
        let (n, bytes, finished) = if phase == Phase::Scanning {
            let l = st.live[Group::ALL.iter().position(|g| *g == grp).unwrap_or(0)];
            (l.items, l.bytes, st.listed && l.done >= l.total)
        } else {
            let items: Vec<&Item> = st.items.iter().filter(|i| i.group == grp).collect();
            (items.len(), items.iter().filter(|i| i.verdict != Verdict::Keep).map(|i| i.bytes).sum(), true)
        };
        let mark = Pos2::new(cols[0] + 8.0, row.center().y);
        if !finished {
            spinner(&p, mark, 7.0, t, ACTION());
        } else if n > 0 {
            p.circle_filled(mark, 8.0, tint::HEALTH());
            icon(&p, Icon::Check, Rect::from_center_size(mark, Vec2::splat(11.0)), ON_ACTION);
        } else {
            crate::kit::glass::stroke_circle(&p, mark, 7.5, Stroke::new(1.5, white(64)));
        }
        p.text(Pos2::new(cols[1], row.center().y), Align2::LEFT_CENTER, grp.label(), font(13.0), TEXT());
        let (status, sc) = if n > 0 { (t!("cleanup.n_found", n = n).into_owned(), GREEN()) } else { ("—".to_string(), TEXT_FAINT()) };
        p.text(Pos2::new(cols[2], row.center().y), Align2::LEFT_CENTER, status, font(12.0), sc);
        p.text(Pos2::new(cols[3], row.center().y), Align2::RIGHT_CENTER, if n > 0 { human_bytes(bytes) } else { "—".into() }, font(13.0), if n > 0 { TEXT() } else { TEXT_FAINT() });
        y += 40.0;
    }
    ctx.request_repaint();
}

// ---------------------------------------------------------------- results

fn results(app: &mut App, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let (total, pending, ai_error, selected) = {
        let g = app.clean.lock_ok();
        (g.total(), g.pending_questions(), g.ai_error.clone(), g.selected_in(app.scope()))
    };
    // after cleaning a group: what's left (the rail shows what went)
    let back = !app.clean.lock_ok().cleaned_groups.is_empty();
    let mut sub = if back { t!("cleanup.n_left", n = human_bytes(total)) } else { t!("cleanup.n_found", n = human_bytes(total)) }.into_owned();
    if let Some(e) = &ai_error {
        sub = format!("{sub} · {e}");
    }
    // AI Check: a chip left of the page action (SP17)
    let ai_n = app.clean.lock_ok().items.iter().filter(|i| i.verdict != Verdict::Keep).count();
    let cw = aicheck::ui::chip_width(&app.ai, ui, ai_n > 0);
    let w = [ts!("cleanup.scan_again"), ts!("cleanup.stop"), ts!("cleanup.skip")].iter().map(|l| crate::kit::glass::icon_pill_width(&p, l, BTN_SECONDARY)).fold(0.0, f32::max);
    let tabs = super::auto_page::reserve(&p);
    let (btns, _) = page_header_room(&p, r, ts!("cleanup.name"), &sub, if ai_error.is_some() { AMBER() } else { TEXT_DIM() }, 0.0, r.right() - sp::PAGE_EDGE - w - sp::M - tabs - cw - sp::M);
    let sr = Rect::from_min_max(Pos2::new(btns.right() - w, btns.top()), btns.max);
    let chip_at = Pos2::new(sr.left() - sp::M - tabs - cw, sr.center().y);
    if crate::kit::glass::icon_pill(ui, sr, Icon::Refresh, ts!("cleanup.scan_again"), BTN_SECONDARY, true).clicked() {
        // back to the start screen, where the scan can be set up and started
        cleanup::stop_scan(&app.clean);
        app.ai.reset();
        return;
    }

    // kept by the person only: the list still shows them under Kept
    let you_kept = app.clean.lock_ok().items.iter().any(|i| i.verdict == Verdict::Keep && i.by == Who::You);
    if total == 0 && pending == 0 && !you_kept {
        let c = Pos2::new(r.center().x, r.top() + PAGE_HEADER_H + 150.0);
        p.circle_filled(c, 28.0, tint::HEALTH());
        icon(&p, Icon::Check, Rect::from_center_size(c, Vec2::splat(28.0)), ON_ACTION);
        p.text(c + Vec2::new(0.0, 56.0), Align2::CENTER_CENTER, ts!("cleanup.nothing_to_clean"), bold(22.0), TEXT());
        p.text(c + Vec2::new(0.0, 84.0), Align2::CENTER_CENTER, ts!("cleanup.nothing_found"), font(13.0), TEXT_DIM());
        let dr = Rect::from_center_size(c + Vec2::new(0.0, 130.0), Vec2::new(96.0, 32.0));
        if pill_button(ui, dr, ts!("cleanup.done"), BTN_PRIMARY, true).clicked() {
            app.clean.lock_ok().phase = Phase::Intro;
        }
        return;
    }

    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, r.top() + PAGE_HEADER_H + 1.0 + sp::L), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - PAGE_FOOTER_H - sp::L));

    {
        // one automatic check per scan (the total changes with every Keep,
        // so it can't name the scan)
        let scan = app.clean.lock_ok().run.to_string();
        let shared = app.clean.clone();
        app.ai.auto(&ctx, &scan, || check_ask(&shared));
        match aicheck::ui::chip(&mut app.ai, ui, chip_at, ai_n > 0) {
            Some(Event::Start) => {
                let ask = check_ask(&app.clean);
                app.ai.start(&ctx, ask);
            }
            Some(Event::Act(rec)) => {
                let idx = bundle_for_key(&app.clean.lock_ok().items, &rec.key);
                if !idx.is_empty() {
                    answer_items(app, &idx, if rec.action == "keep" { Verdict::Keep } else { Verdict::Safe });
                }
                app.ai.done(&rec);
            }
            // Cleanup shows its tips on the items themselves (no locate)
            Some(Event::Show(_)) | None => {}
        }
    }

    // the groups on the left, what the chosen one found on the right
    let rail = Rect::from_min_size(body.min, Vec2::new(RAIL_W, body.height()));
    group_rail(app, ui, rail);
    // the list runs down to the footer, so rows scroll straight under it;
    // its own last gap keeps the end clear of the footer (result_list)
    let list = Rect::from_min_max(Pos2::new(rail.right() + sp::L, body.top()), Pos2::new(body.right(), r.bottom() - PAGE_FOOTER_H));
    result_list(app, ui, list);

    // footer: on a group it cleans only that group (the rest stays ticked)
    let f = page_footer(&p, r, WIN_R);
    let fy = f.center().y;
    // the segmented control already says where it goes; only warn when it can't come back
    let ty_ = if app.delete_now { fy - 9.0 } else { fy };
    p.text(Pos2::new(f.left() + 24.0, ty_), Align2::LEFT_CENTER, t!("cleanup.selected", size = human_bytes(selected)), bold(13.0), TEXT());
    if app.delete_now {
        p.text(Pos2::new(f.left() + 24.0, fy + 10.0), Align2::LEFT_CENTER, ts!("cleanup.cant_undo"), font(12.0), AMBER());
    }
    let clean_label = match app.clean_group {
        Some(_) if !app.clean_tool.is_empty() => t!("cleanup.clean_x", what = app.clean_tool).into_owned(),
        Some(g) => t!("cleanup.clean_x", what = g.label()).into_owned(),
        None => t!("cleanup.clean_x", what = human_bytes(selected)).into_owned(),
    };
    let cw = button_width(&p, &clean_label, BTN_PRIMARY).max(120.0);
    let cr = Rect::from_min_size(Pos2::new(f.right() - 24.0 - cw, fy - 16.0), Vec2::new(cw, 32.0));
    if pill_button(ui, cr, &clean_label, BTN_PRIMARY, selected > 0).clicked() {
        open_confirm(app);
    }
    let seg = Rect::from_min_size(Pos2::new(cr.left() - 16.0 - 200.0, fy - 14.0), Vec2::new(200.0, 28.0));
    if let Some(k) = segmented(ui, seg, ui.scope_id().with("removal"), &[ts!("cleanup.move_to_trash"), ts!("cleanup.delete_now")], app.delete_now as usize) {
        app.delete_now = k == 1;
    }
    if app.clean_confirm {
        confirm(app, ui, r);
    }
}

/// The group list on the left of the results.
const RAIL_W: f32 = 232.0;
/// A row of that list.
const GROUP_ROW_H: f32 = 40.0;
/// A tool's row under its group (SP3a).
const TOOL_ROW_H: f32 = 32.0;
/// A section title ("Needs your OK  5 · 29.8 GB") above its card.
const LABEL_H: f32 = 20.0;
/// The size column of a question.
const SIZE_W: f32 = 72.0;
/// Rows of a bundle sit this far in from the bundle's own row.
const MEMBER_INDENT: f32 = 28.0;
/// From this many alike items on, they are offered as one row.
const BUNDLE_MIN: usize = 3;

/// A question the person hasn't answered yet.
fn is_question(it: &Item) -> bool {
    it.verdict == Verdict::Ask && it.by != Who::You
}

/// Items of one kind that are offered together (dozens of macOS caches,
/// Claude Code's per-session files, build folders of recent projects), so
/// one answer covers them all. The name is the bundle's title.
/// Whether `p` sits right inside a folder named `name`.
fn parent_is(p: &std::path::Path, name: &str) -> bool {
    p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == name)
}

/// The bundle an item folds into, and from how many alike items on.
fn bundle_of(it: &Item) -> Option<(String, usize)> {
    let apple = it.path.components().any(|c| c.as_os_str().to_string_lossy().starts_with("com.apple."));
    let fixed = match it.group {
        Group::AppCaches if apple => Some(ts!("cleanup.bundle.macos")),
        Group::AiTools if parent_is(&it.path, "file-history") => Some(ts!("cleanup.bundle.edit_history")),
        Group::AiTools if parent_is(&it.path, "uploads") => Some(ts!("cleanup.bundle.claude_uploads")),
        Group::AiTools if it.kind == "Old Sessions" && parent_is(&it.path, "projects") => Some(ts!("cleanup.bundle.old_chats")),
        Group::Projects if it.rule_verdict == Verdict::Ask => Some(ts!("cleanup.bundle.recent_builds")),
        Group::Captures => super::captures::bundle(it.kind),
        _ => None,
    };
    match fixed {
        Some(b) => Some((b.to_string(), BUNDLE_MIN)),
        // a tool's parts with one name ("Cursor · Web cache", "Codex · Logs")
        // are one row from two on (SP4)
        None if !it.tool.is_empty() => Some((it.title.clone(), 2)),
        None => None,
    }
}

/// One row of a results list: one item, or a bundle of alike items.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// The bundle's title; `None` for a single item.
    name: Option<String>,
    /// Its items, biggest first.
    idx: Vec<usize>,
    bytes: u64,
}

/// `idx` (biggest first) as rows: alike items bundled once there are
/// [`BUNDLE_MIN`] of them, biggest row first.
fn entries(items: &[Item], idx: &[usize]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut bundles: Vec<(String, usize, Vec<usize>)> = Vec::new();
    for &i in idx {
        match bundle_of(&items[i]) {
            Some((b, min)) => match bundles.iter_mut().find(|x| x.0 == b) {
                Some(x) => x.2.push(i),
                None => bundles.push((b, min, vec![i])),
            },
            None => out.push(Entry { name: None, idx: vec![i], bytes: items[i].bytes }),
        }
    }
    for (b, min, v) in bundles {
        if v.len() >= min {
            let bytes = v.iter().map(|&i| items[i].bytes).sum();
            out.push(Entry { name: Some(b), idx: v, bytes });
        } else {
            out.extend(v.into_iter().map(|i| Entry { name: None, idx: vec![i], bytes: items[i].bytes }));
        }
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    out
}

/// Bundles whose members are alike enough for one AI answer to cover them
/// all (system caches macOS rebuilds). Other bundles hold the person's own
/// files (chats, uploads, project builds) and are answered item by item.
fn one_answer(name: &str) -> bool {
    name == ts!("cleanup.bundle.macos")
}

/// The open questions an answer about `key` covers: the whole bundle when
/// `key` names the biggest item of a one-answer bundle (what AI Check is
/// asked about), else that item.
fn bundle_for_key(items: &[Item], key: &str) -> Vec<usize> {
    let mut asks: Vec<usize> = (0..items.len()).filter(|&i| is_question(&items[i])).collect();
    asks.sort_by_key(|&i| std::cmp::Reverse(items[i].bytes));
    for e in entries(items, &asks) {
        if e.name.as_deref().is_some_and(one_answer) && items[e.idx[0]].path.display().to_string() == key {
            return e.idx;
        }
    }
    items.iter().position(|it| it.path.display().to_string() == key).into_iter().collect()
}

/// The groups as a list: All, then each group that found something, with
/// its size; an amber badge counts a group's questions
/// (items, like the "Needs your OK" count).
/// The group rail; returns its card.
/// A rail row: group, tool ("" for the group itself), title, size,
/// questions, color.
type RailRow = (Option<Group>, &'static str, &'static str, u64, usize, Color32);

fn group_rail(app: &mut App, ui: &mut Ui, rail: Rect) -> Rect {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let mut rows: Vec<RailRow> = vec![];
    // groups cleaned since the scan with nothing left in them: a quiet row with a check
    let mut gone: Vec<(Group, u64)> = vec![];
    {
        let g = app.clean.lock_ok();
        let asks = |f: &dyn Fn(&Item) -> bool| g.items.iter().filter(|i| f(i) && is_question(i)).count();
        rows.push((None, "", ts!("cleanup.all"), g.total(), asks(&|_| true), TEXT_DIM()));
        for (k, grp) in Group::ALL.iter().enumerate() {
            if g.items.iter().any(|i| i.group == *grp) {
                rows.push((Some(*grp), "", grp.label(), g.group_bytes(*grp), asks(&|i| i.group == *grp), junk_ramp(k)));
                // SP3a: AI Tools, once picked, lists the tools its items come from
                if *grp == Group::AiTools && app.clean_group == Some(Group::AiTools) {
                    for (tool, bytes, q) in tools_of(&g.items) {
                        rows.push((Some(*grp), tool, tool, bytes, q, junk_ramp(k)));
                    }
                }
            } else if let Some(c) = g.cleaned_groups.iter().find(|c| c.0 == *grp) {
                gone.push(*c);
            }
        }
    }
    if !rows.iter().any(|t| t.0 == app.clean_group && t.1 == app.clean_tool) {
        if !rows.iter().any(|t| t.0 == app.clean_group) {
            app.clean_group = None;
        }
        app.clean_tool = "";
    }
    let row_h = |tool: &str| if tool.is_empty() { GROUP_ROW_H } else { TOOL_ROW_H };
    let h: f32 = rows.iter().map(|t| row_h(t.1)).sum();
    let card = Rect::from_min_size(rail.min, Vec2::new(rail.width(), (h + gone.len() as f32 * GROUP_ROW_H + 2.0 * sp::XS).min(rail.height())));
    glass_card(&p, card, R_CARD, 0.0, false);
    for (k, (grp, bytes)) in gone.iter().enumerate() {
        let rr = Rect::from_min_size(Pos2::new(card.left() + sp::XS, card.top() + sp::XS + h + k as f32 * GROUP_ROW_H), Vec2::new(card.width() - 2.0 * sp::XS, GROUP_ROW_H));
        if rr.bottom() > card.bottom() + 0.5 {
            break;
        }
        let c = Pos2::new(rr.left() + sp::M + 12.0, rr.center().y);
        p.circle_filled(c, 12.0, with_alpha(GREEN(), 0.14));
        icon(&p, Icon::Check, Rect::from_center_size(c, Vec2::splat(14.0)), GREEN());
        let size = p.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(*bytes), ty::body(), TEXT_FAINT());
        let x = c.x + 12.0 + sp::M;
        text_fit(&p, Pos2::new(x, rr.center().y), Align2::LEFT_CENTER, grp.label(), ty::body(), TEXT_DIM(), size.left() - sp::S - x);
    }
    let mut y = card.top() + sp::XS;
    for (grp, tool, title, bytes, q, col) in rows.iter() {
        let rr = Rect::from_min_size(Pos2::new(card.left() + sp::XS, y), Vec2::new(card.width() - 2.0 * sp::XS, row_h(tool)));
        y = rr.bottom();
        if rr.bottom() > card.bottom() + 0.5 {
            break;
        }
        let is_tool = !tool.is_empty();
        let sel = app.clean_group == *grp && app.clean_tool == *tool;
        let id = ui.scope_id().with(("group", *title, *tool));
        let resp = ui.interact(rr, id, Sense::click());
        let hv = ctx.animate_bool(id, resp.hovered());
        // lit rows never touch: LIT_INSET top and bottom (H1)
        let lit = rr.shrink2(Vec2::new(0.0, crate::kit::glass::LIT_INSET));
        if sel {
            // H3
            crate::kit::glass::selected_fill(&p, lit, 8.0);
        } else if hv > 0.0 {
            // ROW_HOVER (ui-rules H1), inset radius 8 in the rail
            p.rect_filled(lit, 8.0, white((ROW_HOVER as f32 * hv) as u8));
        }
        // H9: 12 from the lit edge, like the size on the right
        let c = Pos2::new(rr.left() + sp::M + 12.0, rr.center().y);
        if !is_tool {
            p.circle_filled(c, 12.0, with_alpha(*col, 0.14));
            let ir = Rect::from_center_size(c, Vec2::splat(14.0));
            match grp {
                Some(g) => group_icon(&p, *g, ir, *col),
                None => icon(&p, Icon::Disk, ir, *col),
            }
        }
        let size = p.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(*bytes), ty::body(), TEXT_DIM());
        // SP3a: a tool's name starts under its group's name (H6)
        let x = c.x + 12.0 + sp::M;
        let label = q.to_string();
        let bw = if *q > 0 { (p.layout_no_wrap(label.clone(), bold(11.0), BG()).size().x + 14.0).max(20.0) + sp::S } else { 0.0 };
        let ink = if sel { ACCENT() } else if is_tool { TEXT_DIM() } else { TEXT() };
        let t = text_fit(&p, Pos2::new(x, rr.center().y), Align2::LEFT_CENTER, title, ty::body(), ink, size.left() - sp::S - bw - x);
        if *q > 0 {
            badge(&p, Pos2::new(t.right() + sp::S, rr.center().y), Align2::LEFT_CENTER, &label);
        }
        let resp = match grp {
            // P7: beside the rail, never over the next row
            Some(g) if !is_tool => crate::kit::glass::tip_beside(resp, g.blurb(), card.right()),
            _ => resp,
        };
        if resp.clicked() {
            app.clean_group = *grp;
            app.clean_tool = tool;
        }
        if resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    card
}

/// The tools AI Tools' items come from, biggest first: name, size (without
/// what's kept), open questions.
fn tools_of(items: &[Item]) -> Vec<(&'static str, u64, usize)> {
    let mut out: Vec<(&'static str, u64, usize)> = Vec::new();
    for it in items.iter().filter(|i| i.group == Group::AiTools && !i.tool.is_empty()) {
        let k = match out.iter().position(|t| t.0 == it.tool) {
            Some(k) => k,
            None => {
                out.push((it.tool, 0, 0));
                out.len() - 1
            }
        };
        if it.verdict != Verdict::Keep {
            out[k].1 += it.bytes;
        }
        out[k].2 += is_question(it) as usize;
    }
    // a tool with only kept items has nothing to pick
    out.retain(|t| items.iter().any(|i| i.group == Group::AiTools && i.tool == t.0 && i.verdict != Verdict::Keep));
    out.sort_by_key(|t| std::cmp::Reverse(t.1));
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sec {
    Ask,
    Safe,
    Kept,
}

/// A line of the results list.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Line {
    Label(Sec),
    Gap(f32),
    /// A row: the section and the entry's index.
    Entry(Sec, usize),
    /// An item of an open bundle: section, entry, item.
    Member(Sec, usize, usize),
    /// "Kept  34 · 5.91 GB", folds the kept items.
    KeptHead,
    /// An AI Check tip about nothing in the list.
    Tip(usize),
}

impl Line {
    fn card(self) -> Option<Sec> {
        match self {
            Line::Entry(s, _) | Line::Member(s, _, _) => Some(s),
            Line::KeptHead => Some(Sec::Kept),
            _ => None,
        }
    }
}

/// What a click in the list asks for; done after drawing.
enum Act {
    Answer(Vec<usize>, Verdict),
    /// Tick or untick; `true` also ticks items that can't be put back.
    Select(Vec<usize>, bool, bool),
    Undo(Vec<usize>),
    /// Kept items back to "Needs your OK".
    Unkeep(Vec<usize>),
    Fold(String),
    Tip(aicheck::Rec),
}

/// Which bundles (and Kept) are open, by key.
fn open_rows(ctx: &egui::Context) -> std::collections::HashSet<String> {
    ctx.data(|d| d.get_temp::<std::collections::HashSet<String>>(egui::Id::unique("cleanup-open")).unwrap_or_default())
}

fn fold(ctx: &egui::Context, key: String) {
    ctx.data_mut(|d| {
        let set = d.get_temp_mut_or_default::<std::collections::HashSet<String>>(egui::Id::unique("cleanup-open"));
        if !set.remove(&key) {
            set.insert(key);
        }
    });
}

fn fold_key(sec: Sec, e: &Entry) -> String {
    format!("{sec:?}:{}", e.name.as_deref().unwrap_or(""))
}

/// "Claude: keep. Large to download again…", wrapped to `w`.
fn answer_job(agent: &str, a: &aicheck::Rec, w: f32) -> egui::text::LayoutJob {
    let head = if a.action == "keep" { t!("cleanup.ans_keep", agent = agent) } else { t!("cleanup.ans_clean", agent = agent) };
    let mut job = egui::text::LayoutJob::default();
    job.append(&head, 0.0, egui::TextFormat { font_id: bold(12.0), color: AI(), ..Default::default() });
    job.append(&a.why, 0.0, egui::TextFormat { font_id: ty::label(), color: TEXT(), ..Default::default() });
    job.wrap.max_width = w.max(1.0);
    job
}

/// The agent's answer for a row: about its biggest item, which is what
/// AI Check is asked about for a bundle.
fn row_answer(app: &App, items: &[Item], e: &Entry) -> Option<aicheck::Rec> {
    e.idx.iter().find_map(|&i| app.ai.answer_for(&items[i].path.display().to_string()))
}

/// Room for a question's text in a row `w` wide.
fn ask_text_w(p: &egui::Painter, w: f32, bundle: bool) -> f32 {
    let btns = button_width(p, ts!("cleanup.clean"), BTN_PRIMARY) + sp::S + button_width(p, ts!("cleanup.keep"), BTN_PRIMARY);
    w - 2.0 * sp::L - if bundle { 20.0 } else { 0.0 } - SIZE_W - 2.0 * sp::COL_GAP - btns
}

/// What a question says under its name: the rule's words, or how many
/// items a bundle holds.
fn ask_detail(items: &[Item], e: &Entry) -> String {
    match e.name {
        Some(_) => items_n(e.idx.len()),
        None => {
            let it = &items[e.idx[0]];
            if it.rule_reason.is_empty() { super::engine::kind_label(it.kind).to_string() } else { it.rule_reason.clone() }
        }
    }
}

/// A question's height: name, what it is (wrapped, never cut), and the
/// agent's answer only once there is one.
fn ask_height(p: &egui::Painter, items: &[Item], e: &Entry, ans: Option<&aicheck::Rec>, agent: &str, w: f32) -> f32 {
    let tw = ask_text_w(p, w, e.name.is_some());
    let detail = p.layout(ask_detail(items, e), ty::label(), TEXT_SOFT(), tw.max(1.0)).size().y;
    let answer = ans.map(|a| sp::LINE_GAP + p.layout_job(answer_job(agent, a, tw)).size().y).unwrap_or(0.0);
    sp::M + line_h(p, &ty::title()) + sp::LINE_GAP + detail + answer + sp::M
}

/// A Safe row: taller when AI Check has a tip about it.
fn safe_height(p: &egui::Painter, tip: Option<&aicheck::Rec>, agent: &str, w: f32) -> f32 {
    match tip {
        Some(t) => sp::M + line_h(p, &ty::body()) + sp::LINE_GAP + p.layout_job(tip_job(agent, t, safe_tip_w(p, w))).size().y + sp::M,
        None => sp::ROW_H,
    }
}

fn safe_tip_w(p: &egui::Painter, w: f32) -> f32 {
    w - 2.0 * sp::L - 16.0 - sp::M - SIZE_W - sp::COL_GAP - p.layout_no_wrap(ts!("cleanup.keep").into(), bold(12.0), TEXT()).size().x - sp::COL_GAP
}

/// "Claude: keep. You built MyApp today", wrapped to `w`.
fn tip_job(agent: &str, t: &aicheck::Rec, w: f32) -> egui::text::LayoutJob {
    let head = if t.action == "keep" { t!("cleanup.ans_keep", agent = agent) } else { t!("cleanup.ans_clean", agent = agent) };
    let mut job = egui::text::LayoutJob::default();
    job.append(&head, 0.0, egui::TextFormat { font_id: bold(12.0), color: AI(), ..Default::default() });
    job.append(&t.why, 0.0, egui::TextFormat { font_id: ty::label(), color: TEXT_SOFT(), ..Default::default() });
    job.wrap.max_width = w.max(1.0);
    job
}

/// One list of what the chosen group found: the questions, what will go
/// (Safe), what stays (Kept, folded), then AI Check's other tips.
fn result_list(app: &mut App, ui: &mut Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let mut items: Vec<Item> = app.clean.lock_ok().items.clone();
    let scope = app.scope();
    // one tool's list: its rows drop the tool's name (the rail says it)
    if !scope.tool.is_empty() {
        for it in items.iter_mut().filter(|i| i.tool == scope.tool) {
            it.title = cleanup::without_tool(it).to_string();
        }
    }
    let mut mine: Vec<usize> = (0..items.len()).filter(|&i| cleanup::in_scope(&items[i], scope)).collect();
    mine.sort_by_key(|&i| std::cmp::Reverse(items[i].bytes));
    let asks: Vec<usize> = mine.iter().copied().filter(|&i| is_question(&items[i])).collect();
    let safe: Vec<usize> = mine.iter().copied().filter(|&i| items[i].verdict == Verdict::Safe || (items[i].verdict == Verdict::Ask && items[i].by == Who::You)).collect();
    let kept: Vec<usize> = mine.iter().copied().filter(|&i| items[i].verdict == Verdict::Keep).collect();
    let es = [entries(&items, &asks), entries(&items, &safe), entries(&items, &kept)];
    let sec_i = |s: Sec| match s {
        Sec::Ask => 0,
        Sec::Safe => 1,
        Sec::Kept => 2,
    };
    let mut open = open_rows(&ctx);
    if DEMO_HOVER.lock_ok().is_some() {
        open.insert("kept".into());
    }
    let agent = if app.demo { Agent::Claude.name() } else { super::agent::main().name() };
    let running = app.ai.phase() == aicheck::Phase::Running;
    let all_tips = app.ai.tips();
    let key_of = |i: usize| items[i].path.display().to_string();
    let row_tip = |e: &Entry| -> Option<aicheck::Rec> { if e.name.is_some() { None } else { all_tips.iter().find(|t| t.key == key_of(e.idx[0])).cloned() } };
    // a tip about a listed Safe item sits on its row; the rest go under the list
    let tips: Vec<aicheck::Rec> = all_tips.iter().filter(|t| !safe.iter().any(|&i| key_of(i) == t.key)).cloned().collect();
    let answers: Vec<Option<aicheck::Rec>> = es[0].iter().map(|e| row_answer(app, &items, e)).collect();

    let mut lines: Vec<Line> = Vec::new();
    let push_entries = |lines: &mut Vec<Line>, sec: Sec| {
        for (k, e) in es[sec_i(sec)].iter().enumerate() {
            lines.push(Line::Entry(sec, k));
            if e.name.is_some() && open.contains(&fold_key(sec, e)) {
                lines.extend((0..e.idx.len()).map(|m| Line::Member(sec, k, m)));
            }
        }
    };
    if !es[0].is_empty() {
        lines.extend([Line::Label(Sec::Ask), Line::Gap(sp::M)]);
        push_entries(&mut lines, Sec::Ask);
    }
    if !es[1].is_empty() {
        if !lines.is_empty() {
            lines.push(Line::Gap(sp::XL));
        }
        lines.extend([Line::Label(Sec::Safe), Line::Gap(sp::M)]);
        push_entries(&mut lines, Sec::Safe);
    }
    let kept_open = open.contains("kept");
    if !es[2].is_empty() {
        if !lines.is_empty() {
            lines.push(Line::Gap(sp::XL));
        }
        lines.push(Line::KeptHead);
        if kept_open {
            push_entries(&mut lines, Sec::Kept);
        }
    }
    for k in 0..tips.len() {
        lines.push(Line::Gap(if k == 0 { sp::L } else { sp::S }));
        lines.push(Line::Tip(k));
    }
    // the list reaches the footer; scrolled to the end it stops this far above it
    lines.push(Line::Gap(sp::L));

    let height = |l: Line, w: f32| -> f32 {
        match l {
            Line::Label(_) => LABEL_H,
            Line::Gap(g) => g,
            Line::Entry(Sec::Ask, k) => ask_height(&p, &items, &es[0][k], answers[k].as_ref(), agent, w),
            Line::Entry(Sec::Safe, k) => safe_height(&p, row_tip(&es[1][k]).as_ref(), agent, w),
            Line::Tip(k) => tip_height(&p, &tips[k], w),
            _ => sp::ROW_H,
        }
    };
    // rows stop short of the scroll bar only when the list scrolls
    let full: Vec<f32> = lines.iter().map(|&l| height(l, area.width())).collect();
    let w = if scrolls(area.height(), full.iter().copied(), 0.0) { area.width() - sp::SCROLL_GUTTER } else { area.width() };
    let hs: Vec<f32> = if w == area.width() { full } else { lines.iter().map(|&l| height(l, w)).collect() };
    let total: f32 = hs.iter().sum();

    let mut acts: Vec<Act> = Vec::new();
    let mut child = ui.new_child(UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(area.intersect(ui.clip_rect()));
    ScrollArea::vertical().id_salt(("results", scope.group, scope.tool)).auto_shrink([false, false]).show(&mut child, |ui| {
        let (all, _) = ui.allocate_exact_size(Vec2::new(w, total), Sense::hover());
        let p = ui.painter().clone();
        // the cards behind runs of rows
        let mut y = all.top();
        let mut run: Option<(Sec, f32)> = None;
        for (k, &l) in lines.iter().enumerate() {
            match (run, l.card()) {
                (Some((s, _)), Some(c)) if s == c => {}
                (prev, cur) => {
                    if let Some((_, top)) = prev {
                        glass_card(&p, Rect::from_min_max(Pos2::new(all.left(), top), Pos2::new(all.right(), y)), R_CARD, 0.0, false);
                    }
                    run = cur.map(|c| (c, y));
                }
            }
            y += hs[k];
        }
        if let Some((_, top)) = run {
            glass_card(&p, Rect::from_min_max(Pos2::new(all.left(), top), Pos2::new(all.right(), y)), R_CARD, 0.0, false);
        }
        // the rows
        let mut y = all.top();
        for (k, &l) in lines.iter().enumerate() {
            let rr = Rect::from_min_size(Pos2::new(all.left(), y), Vec2::new(w, hs[k]));
            y += hs[k];
            if !ui.is_rect_visible(rr) {
                continue;
            }
            // hairlines between the rows of a card, none next to a lit row (H2)
            let lit = match l {
                Line::Entry(sec, e) => ui.rect_contains_pointer(rr) || demo_hover(&items[es[sec_i(sec)][e].idx[0]].path.display().to_string()) || app.ai.hovered.as_deref() == Some(items[es[sec_i(sec)][e].idx[0]].path.display().to_string().as_str()),
                Line::Member(..) | Line::KeptHead => ui.rect_contains_pointer(rr),
                _ => false,
            };
            if k > 0 && l.card().is_some() && lines[k - 1].card() == l.card() {
                crate::kit::glass::row_divider_stroke(ui, &p, rr.left_top(), rr.right_top(), lit, rr.bottom(), Stroke::new(1.0, CARD_EDGE()));
            } else if lit {
                crate::kit::glass::row_divider_stroke(ui, &p, rr.left_top(), rr.left_top(), true, rr.bottom(), Stroke::NONE);
            }
            let act = match l {
                Line::Label(Sec::Ask) => {
                    let lab = section_label(&p, rr.left_top(), ts!("cleanup.needs_ok"), AMBER(), asks.len(), asks.iter().map(|&i| items[i].bytes).sum());
                    let a = link(ui, Pos2::new(rr.right(), lab.center().y), Align2::RIGHT_CENTER, ts!("cleanup.keep_all"), 12.0);
                    if a.clicked() {
                        Some(Act::Answer(asks.clone(), Verdict::Keep))
                    } else if link(ui, Pos2::new(a.rect.left() - sp::M, lab.center().y), Align2::RIGHT_CENTER, ts!("cleanup.clean_all"), 12.0).clicked() {
                        Some(Act::Answer(asks.clone(), Verdict::Safe))
                    } else {
                        None
                    }
                }
                Line::Label(_) => {
                    let lab = section_label(&p, rr.left_top(), ts!("cleanup.safe"), ACCENT(), safe.len(), safe.iter().map(|&i| items[i].bytes).sum());
                    let all_on = safe.iter().all(|&i| items[i].selected || items[i].hard_keep);
                    link(ui, Pos2::new(rr.right(), lab.center().y), Align2::RIGHT_CENTER, if all_on { ts!("cleanup.none") } else { ts!("cleanup.select_all") }, 12.0).clicked().then(|| Act::Select(safe.clone(), !all_on, false))
                }
                Line::Gap(_) => None,
                Line::Entry(Sec::Ask, e) => ask_row(app, ui, rr, &items, &es[0][e], answers[e].as_ref(), agent, running, open.contains(&fold_key(Sec::Ask, &es[0][e]))),
                Line::Member(Sec::Ask, e, m) => ask_member(app, ui, rr, &items, es[0][e].idx[m], agent),
                Line::Entry(sec, e) => list_row(ui, rr, &items, sec, &es[sec_i(sec)][e], row_tip(&es[sec_i(sec)][e]).as_ref(), agent, open.contains(&fold_key(sec, &es[sec_i(sec)][e])), app.ai.hovered.as_deref()),
                Line::Member(sec, e, m) => list_member(ui, rr, &items, sec, es[sec_i(sec)][e].idx[m]),
                Line::KeptHead => kept_head(ui, rr, kept.len(), kept.iter().map(|&i| items[i].bytes).sum(), kept_open),
                Line::Tip(t) => tip_row(ui, rr, &tips[t]).map(Act::Tip),
            };
            acts.extend(act);
        }
    });

    for act in acts {
        match act {
            Act::Answer(idx, v) => answer_items(app, &idx, v),
            Act::Select(idx, on, any) => {
                let mut g = app.clean.lock_ok();
                for i in idx {
                    let it = &mut g.items[i];
                    it.selected = on && !it.hard_keep && (any || !it.permanent);
                }
            }
            Act::Undo(idx) => {
                for i in idx {
                    undo_item(app, i);
                }
            }
            Act::Unkeep(idx) => unkeep_items(app, &idx),
            Act::Fold(key) => fold(&ctx, key),
            Act::Tip(rec) => {
                let i = items.iter().position(|it| it.path.display().to_string() == rec.key);
                if let Some(i) = i {
                    let ask = app.clean.lock_ok().items[i].verdict == Verdict::Ask;
                    if rec.action == "keep" {
                        answer_items(app, &[i], Verdict::Keep);
                    } else if ask {
                        answer_items(app, &[i], Verdict::Safe);
                    } else {
                        let mut g = app.clean.lock_ok();
                        let it = &mut g.items[i];
                        it.selected = !it.hard_keep && !it.permanent;
                    }
                }
                app.ai.done(&rec);
            }
        }
    }
}

/// "Needs your OK  5 · 29.8 GB": a section title with its count and size.
fn section_label(p: &egui::Painter, at: Pos2, title: &str, col: Color32, n: usize, bytes: u64) -> Rect {
    let t = p.text(Pos2::new(at.x, at.y + LABEL_H / 2.0), Align2::LEFT_CENTER, title, ty::title(), col);
    let s = p.text(Pos2::new(t.right() + sp::S, t.center().y), Align2::LEFT_CENTER, format!("{n} · {}", human_bytes(bytes)), ty::label(), TEXT_DIM());
    t.union(s)
}

/// A light highlight under the pointer; true when the row was clicked.
fn row_hover(ui: &mut Ui, rr: Rect, salt: impl std::hash::Hash + std::fmt::Debug) -> egui::Response {
    let resp = ui.interact(rr, ui.scope_id().with(("row", salt)), Sense::click());
    if resp.hovered() {
        // ROW_HOVER (H1), inside the card (H2)
        crate::kit::glass::row_hover_fill(ui.painter(), rr);
    }
    resp
}

/// A bundle's fold arrow at `x`, centred on `cy`.
fn fold_arrow(p: &egui::Painter, x: f32, cy: f32, open: bool) {
    icon(p, if open { Icon::Down } else { Icon::Forward }, Rect::from_center_size(Pos2::new(x + 6.0, cy), Vec2::splat(12.0)), TEXT_DIM());
}

/// Clean / Keep at the right of `rr`, the agent's pick lit; nothing lit
/// before it answered. Returns the answer clicked.
fn clean_keep(ui: &mut Ui, rr: Rect, ans: Option<&aicheck::Rec>) -> Option<Verdict> {
    let p = ui.painter().clone();
    let keep_lit = ans.is_some_and(|a| a.action == "keep");
    let clean_lit = ans.is_some_and(|a| a.action != "keep");
    let style = |lit: bool| if lit { BTN_PRIMARY } else { BTN_SECONDARY };
    let cy = rr.center().y;
    let kw = button_width(&p, ts!("cleanup.keep"), style(keep_lit));
    let cw = button_width(&p, ts!("cleanup.clean"), style(clean_lit));
    let kr = Rect::from_min_size(Pos2::new(rr.right() - sp::L - kw, cy - sp::CONTROL_H / 2.0), Vec2::new(kw, sp::CONTROL_H));
    let cr = Rect::from_min_size(Pos2::new(kr.left() - sp::S - cw, kr.top()), Vec2::new(cw, sp::CONTROL_H));
    if pill_button(ui, cr, ts!("cleanup.clean"), style(clean_lit), true).clicked() {
        return Some(Verdict::Safe);
    }
    if pill_button(ui, kr, ts!("cleanup.keep"), style(keep_lit), true).clicked() {
        return Some(Verdict::Keep);
    }
    None
}

/// The right edge of a question's size column.
fn ask_size_right(p: &egui::Painter, rr: Rect) -> f32 {
    rr.right() - sp::L - button_width(p, ts!("cleanup.keep"), BTN_PRIMARY) - sp::S - button_width(p, ts!("cleanup.clean"), BTN_PRIMARY) - sp::COL_GAP
}

/// One question: name, what it is, the agent's answer once it came, the
/// size, Clean and Keep. A bundle folds open to its items.
#[allow(clippy::too_many_arguments)]
fn ask_row(app: &App, ui: &mut Ui, rr: Rect, items: &[Item], e: &Entry, ans: Option<&aicheck::Rec>, agent: &str, running: bool, open: bool) -> Option<Act> {
    let p = ui.painter().clone();
    let first = &items[e.idx[0]];
    let key = first.path.display().to_string();
    let resp = row_hover(ui, rr, &key);
    // the AI Check answer under the pointer points at its row
    if app.ai.hovered.as_deref() == Some(key.as_str()) {
        p.rect_stroke(crate::kit::glass::lit_rect(rr), crate::kit::glass::LIT_R, Stroke::new(2.0, with_alpha(AI(), 0.7)), StrokeKind::Inside);
    }
    let mut act = None;
    if e.name.is_some() {
        if resp.clicked() {
            act = Some(Act::Fold(fold_key(Sec::Ask, e)));
        }
    } else {
        let resp = resp.on_hover_text(short_path(&first.path));
        if resp.double_clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(&first.path).spawn();
        }
    }
    let mut x = rr.left() + sp::L;
    let th = line_h(&p, &ty::title());
    let ty0 = rr.top() + sp::M + th / 2.0;
    if e.name.is_some() {
        fold_arrow(&p, x, ty0, open);
        x += 20.0;
    }
    let tw = ask_text_w(&p, rr.width(), e.name.is_some());
    let size_r = ask_size_right(&p, rr);
    p.text(Pos2::new(size_r, rr.center().y), Align2::RIGHT_CENTER, human_bytes(e.bytes), ty::title(), TEXT());
    // name, then the "Permanent" chip and the agent's spinner after it
    let mut right = x + tw;
    let perm = e.idx.iter().any(|&i| items[i].permanent);
    if perm {
        right -= p.layout_no_wrap(ts!("cleanup.permanent").into(), bold(11.0), TEXT()).size().x + 16.0 + sp::S;
    }
    let waiting = running && ans.is_none() && app.ai.answer.contains(&key);
    if waiting {
        right -= 10.0 + sp::S;
    }
    let t = text_fit(&p, Pos2::new(x, ty0), Align2::LEFT_CENTER, e.name.as_deref().unwrap_or(&first.title), ty::title(), TEXT(), right - x);
    let mut after = t.right() + sp::S;
    if perm {
        after = chip(&p, Pos2::new(after, ty0), Align2::LEFT_CENTER, ts!("cleanup.permanent"), CHIP_RISK()).right() + sp::S;
    }
    if waiting {
        spinner(&p, Pos2::new(after + 5.0, ty0), 5.0, ui.input(|i| i.time), AI());
        ui.ctx().request_repaint();
    }
    // what it is, wrapped
    let dy = rr.top() + sp::M + th + sp::LINE_GAP;
    let g = p.layout(ask_detail(items, e), ty::label(), TEXT_SOFT(), tw.max(1.0));
    let dh = g.size().y;
    p.galley(Pos2::new(x, dy), g, TEXT_SOFT());
    // the agent's answer, only once it gave one
    if let Some(a) = ans {
        let g = p.layout_job(answer_job(agent, a, tw));
        p.galley(Pos2::new(x, dy + dh + sp::LINE_GAP), g, TEXT());
    }
    if let Some(v) = clean_keep(ui, rr, ans) {
        act = Some(Act::Answer(e.idx.clone(), v));
    }
    act
}

/// An item of an open question bundle: name, size, its own Clean / Keep.
fn ask_member(app: &App, ui: &mut Ui, rr: Rect, items: &[Item], i: usize, agent: &str) -> Option<Act> {
    let p = ui.painter().clone();
    let it = &items[i];
    let key = it.path.display().to_string();
    let ans = app.ai.answer_for(&key);
    let tip = match &ans {
        Some(a) => format!("{}\n{}{}", short_path(&it.path), if a.action == "keep" { t!("cleanup.ans_keep", agent = agent) } else { t!("cleanup.ans_clean", agent = agent) }, a.why),
        None => short_path(&it.path),
    };
    // its own id: the bundle row above is keyed on the same (first) path
    let resp = row_hover(ui, rr, ("member", &key)).on_hover_text(tip);
    if resp.double_clicked() {
        let _ = std::process::Command::new("open").arg("-R").arg(&it.path).spawn();
    }
    let x = rr.left() + sp::L + MEMBER_INDENT;
    let size_r = ask_size_right(&p, rr);
    let s = p.text(Pos2::new(size_r, rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), ty::body(), TEXT_DIM());
    text_fit(&p, Pos2::new(x, rr.center().y), Align2::LEFT_CENTER, &it.title, ty::body(), TEXT_SOFT(), s.left() - sp::COL_GAP - x);
    clean_keep(ui, rr, ans.as_ref()).map(|v| Act::Answer(vec![i], v))
}

/// A Safe or Kept row: a checkbox (Safe) or room for the lock (Kept), the
/// name, Undo after a Clean / Keep given here, the size. A bundle shows how
/// many items it holds and folds open; AI Check's tip about an item sits
/// under its name.
#[allow(clippy::too_many_arguments)]
fn list_row(ui: &mut Ui, rr: Rect, items: &[Item], sec: Sec, e: &Entry, tip: Option<&aicheck::Rec>, agent: &str, open: bool, hovered: Option<&str>) -> Option<Act> {
    let p = ui.painter().clone();
    let first = &items[e.idx[0]];
    let key = first.path.display().to_string();
    let resp = row_hover(ui, rr, &key);
    if hovered == Some(key.as_str()) {
        p.rect_stroke(crate::kit::glass::lit_rect(rr), crate::kit::glass::LIT_R, Stroke::new(2.0, with_alpha(AI(), 0.7)), StrokeKind::Inside);
    }
    let mut act = None;
    if e.name.is_some() {
        if resp.clicked() {
            act = Some(Act::Fold(fold_key(sec, e)));
        }
    } else {
        let resp = resp.on_hover_text(format!("{}\n{}", first.reason, short_path(&first.path)));
        if resp.double_clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(&first.path).spawn();
        }
    }
    let dim = sec == Sec::Kept;
    let lh = line_h(&p, &ty::body());
    let cy = if tip.is_some() { rr.top() + sp::M + lh / 2.0 } else { rr.center().y };
    let mut x = rr.left() + sp::L;
    let hot = ui.rect_contains_pointer(rr) || demo_hover(&key);
    if demo_hover(&key) {
        crate::kit::glass::row_hover_fill(&p, rr);
    }
    let unkeep: Vec<usize> = if sec == Sec::Kept { e.idx.iter().copied().filter(|&i| unkeepable(&items[i])).collect() } else { Vec::new() };
    if sec == Sec::Safe {
        let can: Vec<usize> = e.idx.iter().copied().filter(|&i| !items[i].hard_keep).collect();
        let on = !can.is_empty() && can.iter().all(|&i| items[i].selected);
        let cb = Rect::from_center_size(Pos2::new(x + 8.0, cy), Vec2::splat(16.0));
        if checkbox(ui, cb, ui.scope_id().with(("sel", &key, &e.name)), on, !can.is_empty()).clicked() && !can.is_empty() {
            act = Some(Act::Select(e.idx.clone(), !on, e.name.is_none()));
        }
    }
    x += 16.0 + sp::M;
    if e.name.is_some() {
        fold_arrow(&p, x, cy, open);
        x += 20.0;
    }
    let s = p.text(Pos2::new(rr.right() - sp::L, cy), Align2::RIGHT_CENTER, human_bytes(e.bytes), if dim { ty::body() } else { ty::title() }, if dim { TEXT_DIM() } else { TEXT() });
    let mut right = s.left() - sp::COL_GAP;
    if sec == Sec::Kept {
        let (a, r) = unkeep_control(ui, Pos2::new(right, cy), &key, &unkeep, if e.name.is_none() { Some(&first.path) } else { None }, hot);
        act = act.or(a);
        right = r;
    }
    if let Some(t) = tip {
        let l = link(ui, Pos2::new(right, cy), Align2::RIGHT_CENTER, &t.label, 12.0);
        if l.clicked() {
            act = Some(Act::Tip(t.clone()));
        }
        right = l.rect.left() - sp::M;
    }
    let perm = e.name.is_none() && first.permanent;
    if perm {
        right = chip(&p, Pos2::new(right, cy), Align2::RIGHT_CENTER, ts!("cleanup.permanent"), CHIP_RISK()).left() - sp::S;
    }
    let undo = e.name.is_none() && sec != Sec::Kept && undoable(first);
    let undo_w = if undo { p.layout_no_wrap(ts!("cleanup.undo").into(), bold(12.0), TEXT()).size().x + sp::S } else { 0.0 };
    let title = e.name.as_deref().unwrap_or(&first.title);
    let t = text_fit(&p, Pos2::new(x, cy), Align2::LEFT_CENTER, title, ty::body(), if dim { TEXT_DIM() } else { TEXT() }, right - undo_w - x);
    if e.name.is_some() {
        p.text(Pos2::new(t.right() + sp::S, cy), Align2::LEFT_CENTER, items_n(e.idx.len()), ty::label(), TEXT_DIM());
    }
    if undo && link(ui, Pos2::new(t.right() + sp::S, cy), Align2::LEFT_CENTER, ts!("cleanup.undo"), 12.0).clicked() {
        act = Some(Act::Undo(vec![e.idx[0]]));
    }
    if let Some(t) = tip {
        let g = p.layout_job(tip_job(agent, t, safe_tip_w(&p, rr.width())));
        p.galley(Pos2::new(x, rr.top() + sp::M + lh + sp::LINE_GAP), g, TEXT());
    }
    act
}

/// An item of an open Safe or Kept bundle.
fn list_member(ui: &mut Ui, rr: Rect, items: &[Item], sec: Sec, i: usize) -> Option<Act> {
    let p = ui.painter().clone();
    let it = &items[i];
    let key = it.path.display().to_string();
    // its own id: the bundle row above is keyed on the same (first) path
    let resp = row_hover(ui, rr, ("member", &key)).on_hover_text(format!("{}\n{}", it.reason, short_path(&it.path)));
    if resp.double_clicked() {
        let _ = std::process::Command::new("open").arg("-R").arg(&it.path).spawn();
    }
    let mut act = None;
    let x0 = rr.left() + sp::L + MEMBER_INDENT;
    if sec == Sec::Safe {
        let cb = Rect::from_center_size(Pos2::new(x0 + 8.0, rr.center().y), Vec2::splat(16.0));
        if checkbox(ui, cb, ui.scope_id().with(("sel", &key)), it.selected, !it.hard_keep).clicked() && !it.hard_keep {
            act = Some(Act::Select(vec![i], !it.selected, true));
        }
    }
    let can = sec == Sec::Kept && unkeepable(it);
    let x = x0 + 16.0 + sp::M;
    let s = p.text(Pos2::new(rr.right() - sp::L, rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), ty::body(), TEXT_DIM());
    let mut right = s.left() - sp::COL_GAP;
    if sec == Sec::Kept {
        let hot = ui.rect_contains_pointer(rr) || demo_hover(&key);
        let (a, r) = unkeep_control(ui, Pos2::new(right, rr.center().y), &format!("m:{key}"), if can { std::slice::from_ref(&i) } else { &[] }, Some(&it.path), hot);
        act = act.or(a);
        right = r;
    }
    let undo = sec != Sec::Kept && undoable(it);
    let undo_w = if undo { p.layout_no_wrap(ts!("cleanup.undo").into(), bold(12.0), TEXT()).size().x + sp::S } else { 0.0 };
    let t = text_fit(&p, Pos2::new(x, rr.center().y), Align2::LEFT_CENTER, &it.title, ty::body(), TEXT_SOFT(), right - undo_w - x);
    if undo && link(ui, Pos2::new(t.right() + sp::S, rr.center().y), Align2::LEFT_CENTER, ts!("cleanup.undo"), 12.0).clicked() {
        act = Some(Act::Undo(vec![i]));
    }
    act
}

/// "🔒 Kept  34 · 5.91 GB  ›": the kept items, folded until clicked.
fn kept_head(ui: &mut Ui, rr: Rect, n: usize, bytes: u64, open: bool) -> Option<Act> {
    let p = ui.painter().clone();
    let resp = row_hover(ui, rr, "kept-head");
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let cy = rr.center().y;
    icon(&p, Icon::Lock, Rect::from_center_size(Pos2::new(rr.left() + sp::L + 8.0, cy), Vec2::splat(14.0)), TEXT_FAINT());
    let t = p.text(Pos2::new(rr.left() + sp::L + 16.0 + sp::M, cy), Align2::LEFT_CENTER, ts!("cleanup.kept"), ty::title(), TEXT_DIM());
    p.text(Pos2::new(t.right() + sp::S, cy), Align2::LEFT_CENTER, n.to_string(), ty::label(), TEXT_DIM());
    let ax = rr.right() - sp::L - 12.0;
    fold_arrow(&p, ax, cy, open);
    p.text(Pos2::new(ax - sp::M, cy), Align2::RIGHT_CENTER, human_bytes(bytes), ty::body(), TEXT_DIM());
    resp.clicked().then(|| Act::Fold("kept".into()))
}

/// Whether rows `hs` tall, `gap` apart, are taller than `h`.
fn scrolls(h: f32, hs: impl Iterator<Item = f32>, gap: f32) -> bool {
    let (n, sum) = hs.fold((0usize, 0.0f32), |(n, s), x| (n + 1, s + x));
    sum + n.saturating_sub(1) as f32 * gap > h + 0.5
}

/// Answer questions: Clean ticks them, Keep remembers them as kept.
fn answer_items(app: &mut App, idx: &[usize], v: Verdict) {
    let mut kept = Vec::new();
    {
        let mut g = app.clean.lock_ok();
        for &i in idx {
            let it = &mut g.items[i];
            // AI Check's answer stays: Undo brings the question back with it
            if v == Verdict::Keep {
                kept.push(it.path.clone());
            }
            it.verdict = v;
            it.by = Who::You;
            it.selected = v == Verdict::Safe;
            it.reason = if v == Verdict::Safe { ts!("cleanup.you_said_clean").into() } else { ts!("cleanup.you_said_keep").into() };
        }
    }
    if !kept.is_empty() {
        cleanup::remember_kept(kept);
    }
}

/// "Tip: title. why" wrapped, with its action as a link on the right.
fn tip_text(t: &aicheck::Rec, w: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(ts!("cleanup.tip"), 0.0, egui::TextFormat { font_id: bold(12.0), color: AI(), ..Default::default() });
    let title = t.title.trim_end_matches('.');
    let body = if t.why.is_empty() { format!("{title}.") } else { format!("{title}. {}", t.why) };
    job.append(&body, 0.0, egui::TextFormat { font_id: ty::label(), color: TEXT_SOFT(), ..Default::default() });
    job.wrap.max_width = w.max(1.0);
    job
}

const TIP_LINK_W: f32 = 64.0;

fn tip_height(p: &egui::Painter, t: &aicheck::Rec, w: f32) -> f32 {
    p.layout_job(tip_text(t, w - TIP_LINK_W)).size().y.max(16.0)
}

fn tip_row(ui: &mut Ui, r: Rect, t: &aicheck::Rec) -> Option<aicheck::Rec> {
    let p = ui.painter().clone();
    let g = p.layout_job(tip_text(t, r.width() - TIP_LINK_W));
    p.galley(r.left_top(), g, TEXT());
    link(ui, Pos2::new(r.right(), r.top() + 8.0), Align2::RIGHT_CENTER, &t.label, 12.0).clicked().then(|| t.clone())
}

/// Undo the person's Clean / Keep: the item goes back to what the rules
/// said (a question goes back to "Needs your OK").
fn undo_item(app: &mut App, i: usize) {
    let mut g = app.clean.lock_ok();
    if undo(&mut g.items[i]) {
        cleanup::forget_kept_one(&g.items[i].path);
    }
}

/// Put an item back to what the rules said; true when it had been kept.
fn undo(it: &mut Item) -> bool {
    let was_kept = it.verdict == Verdict::Keep;
    if it.kind == "Projects" && it.rule_verdict == Verdict::Keep {
        it.hard_keep = true;
    }
    it.verdict = it.rule_verdict;
    it.by = Who::Rule;
    it.selected = it.rule_verdict == Verdict::Safe && !it.permanent && !it.hard_keep;
    it.reason = it.rule_reason.clone();
    was_kept
}

/// Whether the person decided this item here (it can go back).
fn undoable(it: &Item) -> bool {
    it.by == Who::You
}

/// A kept item the person may take back: theirs, what the rules or Claude
/// kept, and Claude Code projects in use (their chats are the person's
/// call). Protected folders, kept caches and worktrees with work in them
/// stay locked, also when the person kept them too.
fn unkeepable(it: &Item) -> bool {
    it.verdict == Verdict::Keep && (!it.hard_keep || it.kind == "Projects")
}

/// Remove from Kept: the items go to "Needs your OK" (Clean / Keep), and
/// what the person had kept is no longer remembered.
fn unkeep_items(app: &mut App, idx: &[usize]) {
    let forget = unkeep(&mut app.clean.lock_ok().items, idx);
    for p in forget {
        cleanup::forget_kept_one(&p);
    }
}

/// [`unkeep_items`] on the list: returns the paths the person had kept.
fn unkeep(items: &mut [Item], idx: &[usize]) -> Vec<PathBuf> {
    let mut forget = Vec::new();
    for &i in idx {
        let it = &mut items[i];
        if !unkeepable(it) {
            continue;
        }
        if it.by == Who::You {
            forget.push(it.path.clone());
        }
        it.verdict = Verdict::Ask;
        it.by = Who::Rule;
        it.hard_keep = false;
        it.selected = false;
        // "You used this project yesterday — kept" asks without the verdict
        it.reason = it.rule_reason.trim_end_matches(" — kept").to_string();
    }
    forget
}

/// Screenshots only: the Kept row drawn as if under the pointer (and its
/// menu open).
pub(crate) static DEMO_HOVER: std::sync::Mutex<Option<(String, bool)>> = std::sync::Mutex::new(None);

fn demo_hover(key: &str) -> bool {
    DEMO_HOVER.lock_ok().as_ref().is_some_and(|(k, _)| k == key)
}

/// A Kept row's "⋯" on hover (MN1–MN6): Remove from Kept (when it may be
/// taken back), Show in Finder (one item). Returns the act and the x left
/// of the button.
fn unkeep_control(ui: &mut Ui, at: Pos2, key: &str, idx: &[usize], path: Option<&PathBuf>, hot: bool) -> (Option<Act>, f32) {
    if idx.is_empty() && path.is_none() {
        return (None, at.x);
    }
    let r = Rect::from_center_size(Pos2::new(at.x - 14.0, at.y), Vec2::splat(28.0));
    let id = ui.scope_id().with(("unkeep-more", key));
    let menu_id = id.with("popup");
    let demo_open = DEMO_HOVER.lock_ok().as_ref().is_some_and(|(k, m)| *m && k == key);
    if demo_open && !egui::Popup::is_id_open(ui.ctx(), menu_id) {
        egui::Popup::open_id(ui.ctx(), menu_id);
    }
    let open = egui::Popup::is_id_open(ui.ctx(), menu_id);
    if !hot && !open {
        return (None, at.x);
    }
    let resp = ui.interact(r, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered() || open);
    let p = ui.painter();
    if h > 0.0 {
        p.rect_filled(r, 6.0, white((18.0 * h) as u8));
    }
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(r.center() + Vec2::new(dx, 0.0), 1.6, TEXT_DIM());
    }
    let mut act = None;
    // near the bottom of the list it opens upward, clear of the footer
    let up = r.bottom() + 2.0 * crate::kit::menu::ROW_H + 16.0 > ui.clip_rect().bottom();
    let popup = crate::kit::menu::popup(&resp);
    let popup = if up { popup.align(egui::RectAlign::TOP_END) } else { popup };
    popup.show(|ui| {
        if !idx.is_empty() && crate::kit::menu::item(ui, ts!("cleanup.remove_from_kept")).clicked() {
            act = Some(Act::Unkeep(idx.to_vec()));
        }
        if let Some(p) = path {
            if crate::kit::menu::item(ui, ts!("cleanup.show_in_finder")).clicked() {
                let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
            }
        }
    });
    (act, r.left() - sp::S)
}

// ---------------------------------------------------------------- confirm

/// An open app whose files are in the selection, on the confirm sheet.
#[derive(Clone, Debug)]
pub(crate) struct QuitRow {
    pub app: OpenApp,
    /// Selected items that belong to it.
    pub idx: Vec<usize>,
    pub bytes: u64,
    pub state: Quit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Quit {
    Open,
    /// Asked to quit at this time (seconds, egui clock).
    Quitting(f64),
    Closed,
    /// Didn't quit in time (it may be asking to save).
    Stuck,
}

/// How long an app gets to quit before its row says "Still open".
const QUIT_WAIT: f64 = 10.0;
const SHEET_W: f32 = 440.0;
const SHEET_PAD: f32 = 24.0;
const SHEET_ROWS_MAX: usize = 6;

/// Cleaning these while their app runs would be skipped or rebuilt at once.
fn quit_kind(kind: &str) -> bool {
    matches!(kind, "Caches" | "Temp Files" | "Build Data")
}

/// The item's files belong to this app: its bundle id is a folder on the
/// path, or the title is the app's name (or starts with it).
fn belongs(app: &OpenApp, it: &Item) -> bool {
    let id = app.id.to_lowercase();
    if !id.is_empty() && it.path.components().any(|c| c.as_os_str().to_string_lossy().to_lowercase() == id) {
        return true;
    }
    let name = app.name.to_lowercase();
    let title = it.title.to_lowercase();
    !name.is_empty() && (title == name || title.starts_with(&format!("{name} ")))
}

/// Open apps with selected files, largest first. Each item counts once.
/// With `only`, just that group's files count (the clean is limited to it).
pub(crate) fn quit_rows(items: &[Item], apps: Vec<OpenApp>, only: impl Into<cleanup::Scope>) -> Vec<QuitRow> {
    let only = only.into();
    let mut taken = vec![false; items.len()];
    let mut rows: Vec<QuitRow> = Vec::new();
    for app in apps {
        let idx: Vec<usize> = (0..items.len()).filter(|&i| !taken[i] && items[i].selected && cleanup::in_scope(&items[i], only) && quit_kind(items[i].kind) && belongs(&app, &items[i])).collect();
        if idx.is_empty() {
            continue;
        }
        for &i in &idx {
            taken[i] = true;
        }
        let bytes = idx.iter().map(|&i| items[i].bytes).sum();
        rows.push(QuitRow { app, idx, bytes, state: Quit::Open });
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.bytes));
    rows
}

/// Open the confirm sheet for the current selection.
fn open_confirm(app: &mut App) {
    let apps = if app.demo { demo_apps() } else { crate::kit::native::open_apps() };
    app.quit = quit_rows(&app.clean.lock_ok().items, apps, app.scope());
    app.quit_then_clean = false;
    app.clean_confirm = true;
}

fn quit_one(app: &mut App, k: usize, now: f64) {
    let row = &mut app.quit[k];
    if app.demo {
        row.state = Quit::Quitting(now);
        return;
    }
    let asked = crate::kit::native::quit_app(row.app.pid);
    row.state = if !asked && !crate::kit::native::app_alive(row.app.pid) { Quit::Closed } else { Quit::Quitting(now) };
}

/// Move quitting rows on: closed once the app is gone, stuck after a while.
fn poll_quits(app: &mut App, ctx: &egui::Context, now: f64) {
    let demo = app.demo;
    let mut waiting = false;
    let mut stuck = false;
    for row in &mut app.quit {
        match row.state {
            Quit::Quitting(t) => {
                let gone = if demo { now - t > 0.9 } else { !crate::kit::native::app_alive(row.app.pid) };
                row.state = if gone {
                    Quit::Closed
                } else if now - t > QUIT_WAIT {
                    Quit::Stuck
                } else {
                    waiting = true;
                    Quit::Quitting(t)
                };
            }
            // an app slow to quit (or quit from the Dock) still turns Closed
            Quit::Stuck if !demo => {
                if crate::kit::native::app_alive(row.app.pid) {
                    stuck = true;
                } else {
                    row.state = Quit::Closed;
                }
            }
            _ => {}
        }
    }
    if waiting {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    } else if stuck {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn start(app: &mut App, ctx: egui::Context) {
    app.clean_confirm = false;
    app.quit_then_clean = false;
    app.quit.clear();
    app.clean_loading = true;
    crate::kit::loader::reset(&ctx, "cleanup");
    cleanup::start_clean(app.clean.clone(), ctx, app.delete_now, app.scope());
}

/// "1 item", "3 items".
fn items_n(n: usize) -> String {
    tn!("cleanup.items", n = n, count = n).into_owned()
}

fn confirm(app: &mut App, ui: &mut Ui, r: Rect) {
    crate::kit::modal::layer(ui, "cleanup-confirm", |ui| confirm_body(app, ui, r));
}

fn confirm_body(app: &mut App, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
    let now = ui.input(|i| i.time);
    poll_quits(app, &ctx, now);
    let open_left = app.quit.iter().filter(|q| q.state != Quit::Closed).count();
    let busy = app.quit.iter().any(|q| matches!(q.state, Quit::Quitting(_)));
    // "Quit all and clean": go as soon as every app is closed
    if app.quit_then_clean && open_left == 0 {
        start(app, ctx);
        return;
    }
    let p = ui.painter().clone();
    // block clicks to the page underneath
    let dim = ui.interact(r, ui.scope_id().with("confirm-scrim"), Sense::click());
    p.rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, SCRIM());
    let (n, bytes, perm, trashed) = {
        let g = app.clean.lock_ok();
        let sel: Vec<&Item> = g.items.iter().filter(|i| i.selected && cleanup::in_scope(i, app.scope())).collect();
        let trashed = sel.iter().filter(|i| !i.permanent && !i.rebuilds()).count();
        let perm = sel.iter().any(|i| i.permanent || i.group == Group::Trash || i.by_command());
        (sel.len(), sel.iter().map(|i| i.bytes).sum::<u64>(), perm, trashed)
    };
    let warn = if app.delete_now {
        Some(ts!("cleanup.cant_undo"))
    } else if perm {
        Some(ts!("cleanup.some_permanent"))
    } else {
        None
    };
    let rows_shown = app.quit.len().min(SHEET_ROWS_MAX);
    let card_h = if app.quit.is_empty() { 0.0 } else { sp::L + rows_shown as f32 * sp::ROW_H };
    let warn_h = if warn.is_some() { sp::L + 16.0 } else { 0.0 };
    let h = SHEET_PAD + 20.0 + card_h + warn_h + sp::XL + sp::CONTROL_H + SHEET_PAD;
    let m = Rect::from_center_size(r.center(), Vec2::new(SHEET_W.min(r.width() - 2.0 * sp::XL), h));
    float_shadow(&p, m, 14.0, 1.0);
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, POPOVER_EDGE()), StrokeKind::Inside);
    let inner = m.shrink(SHEET_PAD);
    p.text(Pos2::new(inner.left(), inner.top() + 10.0), Align2::LEFT_CENTER, t!("cleanup.confirm_title", items = items_n(n), size = human_bytes(bytes)), bold(15.0), TEXT());
    let mut y = inner.top() + 20.0;
    let mut act: Option<usize> = None;
    if !app.quit.is_empty() {
        let card = Rect::from_min_size(Pos2::new(inner.left(), y + sp::L), Vec2::new(inner.width(), rows_shown as f32 * sp::ROW_H));
        glass_card(&p, card, R_CARD, 0.0, false);
        let mut child = ui.new_child(UiBuilder::new().max_rect(card));
        child.set_clip_rect(card.intersect(ui.clip_rect()));
        ScrollArea::vertical().id_salt("confirm-apps").auto_shrink([false, false]).show(&mut child, |ui| {
            let (all, _) = ui.allocate_exact_size(Vec2::new(card.width(), app.quit.len() as f32 * sp::ROW_H), Sense::hover());
            for k in 0..app.quit.len() {
                let rr = Rect::from_min_size(Pos2::new(all.left(), all.top() + k as f32 * sp::ROW_H), Vec2::new(all.width(), sp::ROW_H));
                if k > 0 {
                    ui.painter().line_segment([rr.left_top(), rr.right_top()], Stroke::new(1.0, CARD_EDGE()));
                }
                if quit_row(app, ui, rr, k) {
                    act = Some(k);
                }
            }
        });
        y = card.bottom();
    }
    if let Some(k) = act {
        quit_one(app, k, now);
    }
    if let Some(w) = warn {
        p.text(Pos2::new(inner.left(), y + sp::L + 8.0), Align2::LEFT_CENTER, w, font(12.0), AMBER());
    }
    // buttons, right to left
    let by = inner.bottom() - sp::CONTROL_H;
    let danger = app.delete_now || perm;
    let done_label = if app.delete_now { ts!("cleanup.delete_now") } else if trashed == 0 { ts!("cleanup.clean") } else { ts!("cleanup.move_to_trash") };
    let ok_label = if open_left > 0 { ts!("cleanup.quit_all_clean") } else { done_label };
    let okw = button_width(&p, ok_label, BTN_PRIMARY);
    let ok = Rect::from_min_size(Pos2::new(inner.right() - okw, by), Vec2::new(okw, sp::CONTROL_H));
    let mut left = ok.left();
    let rest = if open_left > 0 {
        let w = button_width(&p, ts!("cleanup.clean_rest"), BTN_SECONDARY);
        let b = Rect::from_min_size(Pos2::new(left - sp::S - w, by), Vec2::new(w, sp::CONTROL_H));
        left = b.left();
        Some(b)
    } else {
        None
    };
    let cw = button_width(&p, ts!("cleanup.cancel"), BTN_SECONDARY);
    let cancel = Rect::from_min_size(Pos2::new(left - sp::S - cw, by), Vec2::new(cw, sp::CONTROL_H));
    // P4: Esc closes unless apps are quitting; a click on the dim closes unless destructive
    let outside = dim.interact_pointer_pos().is_some_and(|q| !m.contains(q));
    let dim_closes = dim.clicked() && outside && !danger && !busy && !app.quit_then_clean;
    if pill_button(ui, cancel, ts!("cleanup.cancel"), BTN_SECONDARY, true).clicked() || dim_closes || (!busy && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        app.clean_confirm = false;
        app.quit_then_clean = false;
        app.quit.clear();
        return;
    }
    if let Some(b) = rest {
        if pill_button(ui, b, ts!("cleanup.clean_rest"), BTN_SECONDARY, !busy).clicked() {
            // leave the open apps' files alone (and the apps open)
            let keep: Vec<usize> = app.quit.iter().filter(|q| q.state != Quit::Closed).flat_map(|q| q.idx.clone()).collect();
            let any_left = {
                let mut g = app.clean.lock_ok();
                for i in keep {
                    if let Some(it) = g.items.get_mut(i) {
                        it.selected = false;
                    }
                }
                g.items.iter().any(|i| i.selected && cleanup::in_scope(i, app.scope()))
            };
            if any_left {
                start(app, ctx);
            } else {
                app.clean_confirm = false;
                app.quit.clear();
            }
            return;
        }
    }
    let kind = if danger && open_left == 0 { BTN_DANGER } else { BTN_PRIMARY };
    if pill_button(ui, ok, ok_label, kind, !busy).clicked() {
        if open_left == 0 {
            start(app, ctx);
        } else {
            app.quit_then_clean = true;
            for k in 0..app.quit.len() {
                if matches!(app.quit[k].state, Quit::Open | Quit::Stuck) {
                    quit_one(app, k, now);
                }
            }
        }
    }
}

/// One open app on the confirm sheet; true when its Quit was clicked.
fn quit_row(app: &mut App, ui: &mut Ui, rr: Rect, k: usize) -> bool {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let row = app.quit[k].clone();
    let closed = row.state == Quit::Closed;
    let a = if closed { 0.55 } else { 1.0 };
    let ic = Rect::from_center_size(Pos2::new(rr.left() + sp::L + 12.0, rr.center().y), Vec2::splat(24.0));
    app_tile(app, &ctx, &p, ic, &row.app, a);
    let x = ic.right() + sp::M;
    let name = p.layout_no_wrap(row.app.name.clone(), font(13.0), with_alpha(TEXT(), a));
    let nw = name.size().x;
    p.galley(Pos2::new(x, rr.center().y - name.size().y / 2.0), name, TEXT());
    p.text(Pos2::new(x + nw + sp::M, rr.center().y), Align2::LEFT_CENTER, human_bytes(row.bytes), font(13.0), with_alpha(TEXT_DIM(), a));
    let right = rr.right() - sp::L;
    let quit_btn = |ui: &mut Ui, right: f32| {
        let w = button_width(&p, ts!("cleanup.quit"), BTN_SECONDARY);
        let b = Rect::from_min_size(Pos2::new(right - w, rr.center().y - sp::CONTROL_H / 2.0), Vec2::new(w, sp::CONTROL_H));
        (pill_button(ui, b, ts!("cleanup.quit"), BTN_SECONDARY, true).clicked(), b.left())
    };
    match row.state {
        Quit::Open => quit_btn(ui, right).0,
        Quit::Quitting(_) => {
            spinner(&p, Pos2::new(right - 6.0, rr.center().y), 5.0, 0.0, TEXT_DIM());
            false
        }
        Quit::Stuck => {
            let (hit, l) = quit_btn(ui, right);
            p.text(Pos2::new(l - sp::S, rr.center().y), Align2::RIGHT_CENTER, ts!("cleanup.still_open"), font(12.0), AMBER());
            hit
        }
        Quit::Closed => {
            icon(&p, Icon::Check, Rect::from_center_size(Pos2::new(right - 8.0, rr.center().y), Vec2::splat(16.0)), ACCENT());
            false
        }
    }
}

/// The app's own icon, else a tile with its first letter.
fn app_tile(app: &mut App, ctx: &egui::Context, p: &egui::Painter, r: Rect, a: &OpenApp, alpha: f32) {
    if let Some(path) = &a.bundle {
        if !app.icons.contains_key(path) {
            let tex = crate::kit::macos::app_icon(path, 64).map(|(n, rgba)| ctx.load_texture(format!("cleanup-icon:{}", path.display()), egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba), egui::TextureOptions::LINEAR));
            app.icons.insert(path.clone(), tex);
        }
        if let Some(Some(t)) = app.icons.get(path) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), with_alpha(Color32::WHITE, alpha));
            return;
        }
    }
    p.rect_filled(r, 6.0, with_alpha(TEXT_DIM(), 0.35 * alpha));
    let l: String = a.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(r.center(), Align2::CENTER_CENTER, l, bold(12.0), with_alpha(TEXT(), alpha));
}

/// Screenshots: apps "open" over the demo results.
pub(crate) fn demo_apps() -> Vec<OpenApp> {
    let app = |pid, id: &str, name: &str| OpenApp { pid, id: id.into(), name: name.into(), bundle: None };
    vec![app(1, "com.spotify.client", "Spotify"), app(2, "com.google.chrome", "Google Chrome"), app(3, "com.anthropic.claudefordesktop", "Claude")]
}

// ---------------------------------------------------------------- done

/// Failures listed on the Done card; "Show all" opens the log for the rest.
const FAILED_SHOWN: usize = 3;

fn done(app: &mut App, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let st = app.clean.lock_ok().clone();
    page_header(&p, r, ts!("cleanup.name"), ts!("cleanup.finished"), TEXT_DIM());
    let ok = st.failed.is_empty();
    // removed items leave the list; the engine keeps what each one was
    let mut rows: Vec<(Group, usize, u64)> = Group::ALL
        .iter()
        .map(|g| {
            let it: Vec<u64> = st.cleaned.iter().filter(|c| c.0 == *g).map(|c| c.1).collect();
            (*g, it.len(), it.iter().sum())
        })
        .filter(|r| r.1 > 0)
        .collect();
    rows.sort_by(|a, b| b.2.cmp(&a.2));
    // cards fit their content (no stretched empty boxes); the log gets the full height
    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, r.top() + PAGE_HEADER_H + 1.0 + sp::L), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - sp::L));
    // the failures wrap, so the card measures them at its own width
    let left_w = 340.0f32.min(body.width() * 0.4);
    let fail_w = left_w - 2.0 * sp::L;
    let fails_h: f32 = st.failed.iter().take(FAILED_SHOWN).map(|(name, err)| p.layout(name.clone(), ty::body(), TEXT(), fail_w).size().y + p.layout(err.clone(), ty::caption(), TEXT(), fail_w).size().y + sp::S).sum();
    let left_h = 2.0 * sp::L + 44.0 + sp::L + 34.0 + sp::XS + 16.0 + sp::XL + 34.0 + if ok { 0.0 } else { 16.0 + sp::S + fails_h + if st.failed.len() > FAILED_SHOWN { 20.0 } else { 0.0 } + sp::L };
    let right_h = 2.0 * sp::L + 34.0 + sp::S + rows.len().max(1) as f32 * sp::ROW2_H;
    let h = if app.clean_log { body.height() } else { left_h.max(right_h).min(body.height()) };
    let left = Rect::from_min_size(body.min, Vec2::new(left_w, h));
    let right = Rect::from_min_size(Pos2::new(left.right() + sp::L, body.top()), Vec2::new(body.right() - left.right() - sp::L, h));
    glass_card(&p, left, R_CARD, 0.0, false);
    glass_card(&p, right, R_CARD, 0.0, false);

    // left: the result
    let li = left.shrink(sp::L);
    let c = Pos2::new(li.left() + 22.0, li.top() + 22.0);
    let col = if ok { GREEN() } else { AMBER() };
    crate::kit::tile::ring(&p, c, 19.0, 4.0, 1.0, col);
    icon(&p, if ok { Icon::Check } else { Icon::Info }, Rect::from_center_size(c, Vec2::splat(18.0)), col);
    let tx = c.x + 22.0 + sp::M;
    let total = st.cleaned_count + st.failed.len();
    let (ly1, ly2) = two_lines_y(&p, c.y, &ty::title(), &ty::caption());
    p.text(Pos2::new(tx, ly1), Align2::LEFT_CENTER, ts!("cleanup.cleanup_finished"), ty::title(), TEXT());
    p.text(Pos2::new(tx, ly2), Align2::LEFT_CENTER, t!("cleanup.n_of", n = st.cleaned_count, items = items_n(total)), ty::caption(), ty::caption_color());
    let mut y = li.top() + 44.0 + sp::L;
    let freed = st.cleaned_bytes.saturating_sub(st.trashed_bytes);
    let g = p.layout_no_wrap(human_bytes(if freed > 0 { freed } else { st.cleaned_bytes }), ty::hero(), TEXT());
    let gw = g.size().x;
    p.galley(Pos2::new(li.left(), y), g, TEXT());
    let what = if st.trashed_bytes == 0 {
        ts!("cleanup.freed").to_string()
    } else if freed == 0 {
        ts!("cleanup.to_trash").to_string()
    } else {
        t!("cleanup.freed_trash", size = human_bytes(st.trashed_bytes)).into_owned()
    };
    p.text(Pos2::new(li.left() + gw + 6.0, y + 26.0), Align2::LEFT_BOTTOM, what, ty::body(), ty::caption_color());
    y += 34.0 + sp::XS;
    y += sp::L;
    if !ok {
        let n = st.failed.len();
        p.text(Pos2::new(li.left(), y), Align2::LEFT_TOP, tn!("cleanup.failed", n = n), ty::title(), AMBER());
        y += 16.0 + sp::S;
        // name over its reason, both whole (never cut with "…"); the rest
        // is one click away in the log
        for (name, err) in st.failed.iter().take(FAILED_SHOWN) {
            let g = p.layout(name.clone(), ty::body(), TEXT(), li.width());
            let h = g.size().y;
            p.galley(Pos2::new(li.left(), y), g, TEXT());
            y += h;
            let g = p.layout(err.clone(), ty::caption(), ty::caption_color(), li.width());
            let h = g.size().y;
            p.galley(Pos2::new(li.left(), y), g, ty::caption_color());
            y += h + sp::S;
        }
        if n > FAILED_SHOWN && !app.clean_log && crate::kit::glass::link(ui, Pos2::new(li.left(), y), Align2::LEFT_TOP, &t!("cleanup.show_all_n", n = n), 12.0).clicked() {
            app.clean_log = true;
        }
    }
    let db = Rect::from_min_size(Pos2::new(li.left(), li.bottom() - 34.0), Vec2::new(li.width(), 34.0));
    // after one group: the other groups are still waiting in the results
    let back = st.clean_only.is_some() && st.total() > 0;
    if pill_button(ui, db, if back { ts!("cleanup.back_to_results") } else { ts!("cleanup.done") }, BTN_PRIMARY, true).clicked() {
        app.clean.lock_ok().phase = if back { Phase::Results } else { Phase::Intro };
        app.clean_log = false;
    }

    // right: what was cleaned, or the log
    let ri = right.shrink(sp::L);
    let log_label = if app.clean_log { ts!("cleanup.hide_log") } else { ts!("cleanup.show_log") };
    let lw = button_width(&p, log_label, BTN_SECONDARY);
    let lb = Rect::from_min_size(Pos2::new(ri.right() - lw, ri.top() - 4.0), Vec2::new(lw, 26.0));
    if pill_button(ui, lb, log_label, BTN_SECONDARY, true).clicked() {
        app.clean_log = !app.clean_log;
    }
    let list = Rect::from_min_max(Pos2::new(ri.left(), ri.top() + 16.0 + sp::M), ri.max);
    if app.clean_log {
        p.text(ri.left_top(), Align2::LEFT_TOP, ts!("cleanup.log"), ty::title(), TEXT());
        // lines scroll to the card's bottom edge; the padding below the last
        // line sits inside the scroll
        let list = Rect::from_min_max(list.min, Pos2::new(list.right(), right.bottom() - 1.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(list);
        ScrollArea::vertical().id_salt("log").auto_shrink([false, false]).show(&mut child, |ui| {
            for (l, warn) in &st.log {
                let col = if *warn { AMBER() } else { TEXT_DIM() };
                ui.label(egui::RichText::new(l.replacen(&home_str(), "~", 1)).font(glass::mono(11.0)).color(col));
            }
            ui.add_space(sp::L - 1.0 - ui.spacing().item_spacing.y);
        });
        return;
    }
    p.text(ri.left_top(), Align2::LEFT_TOP, ts!("cleanup.what_cleaned"), ty::title(), TEXT());
    for (k, (g, n, b)) in rows.iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(list.left(), list.top() + k as f32 * sp::ROW2_H), Vec2::new(list.width(), sp::ROW2_H));
        if row.bottom() > list.bottom() {
            break;
        }
        if k > 0 {
            p.line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, white(15)));
        }
        icon(&p, Icon::Check, Rect::from_center_size(Pos2::new(row.left() + 7.0, row.center().y), Vec2::splat(14.0)), GREEN());
        let vr = p.text(Pos2::new(row.right(), row.center().y), Align2::RIGHT_CENTER, human_bytes(*b), ty::body(), white(166));
        let x = row.left() + 14.0 + sp::M;
        let (ly1, ly2) = two_lines_y(&p, row.center().y, &ty::body(), &ty::caption());
        text_fit(&p, Pos2::new(x, ly1), Align2::LEFT_CENTER, g.label(), ty::body(), TEXT(), vr.left() - x - sp::S);
        text_fit(&p, Pos2::new(x, ly2), Align2::LEFT_CENTER, &tn!("cleanup.items", n = *n, count = n), ty::caption(), ty::caption_color(), vr.left() - x - sp::S);
    }
    if rows.is_empty() {
        text_fit(&p, Pos2::new(list.left(), list.top() + sp::M), Align2::LEFT_TOP, ts!("cleanup.nothing_selected"), ty::body(), TEXT_DIM(), list.width());
    }
}

/// Example results for screenshots (no personal paths).
pub fn demo_results() -> cleanup::State {
    let mk = |group, title: &str, path: &str, bytes: u64, kind, verdict, reason: &str, by| Item {
        group,
        title: title.into(),
        path: PathBuf::from(path),
        bytes,
        age_days: 12.0,
        kind,
        verdict,
        reason: reason.into(),
        rule_reason: reason.into(),
        rule_verdict: verdict,
        by,
        selected: verdict == Verdict::Safe,
        hard_keep: false,
        permanent: false,
        app_running: false,
        action: cleanup::Action::Files,
        tool: "",
    };
    let items = vec![
        mk(Group::AiTools, "Claude Desktop cache", "~/Library/Application Support/Claude/Cache", 678_000_000, "Caches", Verdict::Safe, "Web view cache, rebuilt automatically", Who::Ai),
        mk(Group::AiTools, "Claude Desktop VM image", "~/Library/Application Support/Claude/vm_bundles", 9_000_000_000, "Downloads", Verdict::Ask, "Agent sandbox image, large to download again", Who::Ai),
        mk(Group::AiTools, "Project · old-experiment", "~/.claude/projects/-Users-me-old-experiment", 412_000_000, "Old Sessions", Verdict::Ask, "Chat history for a folder that no longer exists", Who::Ai),
        mk(Group::AiTools, "Project · cmdmeow", "~/.claude/projects/-Users-me-Projects-cmdmeow", 96_000_000, "Projects", Verdict::Keep, "You used this project today", Who::Rule),
        mk(Group::AiTools, "Claude Desktop logs", "~/Library/Logs/Claude", 42_000_000, "Logs", Verdict::Safe, "Old app logs", Who::Ai),
        mk(Group::AiTools, "Claude CLI logs", "~/Library/Caches/claude-cli-nodejs", 18_000_000, "Logs", Verdict::Safe, "Old CLI logs", Who::Ai),
        mk(Group::AppCaches, "Spotify", "~/Library/Caches/com.spotify.client", 1_200_000_000, "Caches", Verdict::Safe, "Streaming cache, rebuilt automatically", Who::Ai),
        mk(Group::AppCaches, "Google Chrome", "~/Library/Caches/Google", 820_000_000, "Caches", Verdict::Safe, "Browser cache, rebuilt automatically", Who::Ai),
        mk(Group::Developer, "Derived Data · MyApp", "~/Library/Developer/Xcode/DerivedData/MyApp-abc", 3_400_000_000, "Build Data", Verdict::Safe, "Build products and index, Xcode rebuilds them", Who::Rule),
        mk(Group::AppCaches, "Adobe Media Cache", "~/Library/Application Support/Adobe/Common/Media Cache Files", 2_100_000_000, "Caches", Verdict::Safe, "Premiere/After Effects media cache", Who::Rule),
        mk(Group::Developer, "npm cache", "~/.npm/_cacache", 640_000_000, "Caches", Verdict::Safe, "npm downloads, fetched again when needed", Who::Ai),
        mk(Group::Projects, "old-site · node_modules", "~/Code/old-site/node_modules", 870_000_000, "Build Data", Verdict::Safe, "Node build output, idle 94 days, rebuilds itself", Who::Rule),
    ];
    let mut items = items;
    // a question already answered here: "You said clean · Undo"
    let mut tg = mk(Group::AppCaches, "Telegram cache", "~/Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram/stable/postbox/media", 2_300_000_000, "Caches", Verdict::Safe, "You said clean", Who::You);
    tg.rule_verdict = Verdict::Ask;
    tg.rule_reason = "Media downloaded from chats".into();
    items.push(tg);
    items.push(mk(Group::Developer, "Gradle caches", "~/.gradle/caches", 1_100_000_000, "Caches", Verdict::Safe, "Gradle downloads, fetched again when needed", Who::Rule));
    items.push(mk(Group::Developer, "CocoaPods cache", "~/Library/Caches/CocoaPods", 310_000_000, "Caches", Verdict::Safe, "Pod downloads, fetched again when needed", Who::Rule));
    // dozens of alike questions show as one row
    for (name, bytes) in [("com.apple.Safari", 640_000_000), ("com.apple.Music", 410_000_000), ("com.apple.helpd", 120_000_000), ("com.apple.Maps", 96_000_000)] {
        items.push(mk(Group::AppCaches, &name.replace("com.apple.", ""), &format!("~/Library/Caches/{name}"), bytes, "Caches", Verdict::Ask, "macOS system cache, usually safe, rebuilt by macOS", Who::Rule));
    }
    // old captures: screenshots and recordings to the Trash, uploads deleted for good
    for (k, b) in [2_400_000u64, 3_100_000, 1_900_000, 2_700_000].into_iter().enumerate() {
        items.push(mk(Group::Captures, &format!("Capture 2026-09-0{} at 10.1{k}.04.png", k + 1), &format!("~/Desktop/Capture 2026-09-0{} at 10.1{k}.04.png", k + 1), b, super::captures::SCREENSHOTS, Verdict::Ask, "Taken 70 days ago", Who::Rule));
    }
    for (k, b) in [1_900_000_000u64, 2_900_000_000, 1_400_000_000].into_iter().enumerate() {
        items.push(mk(Group::Captures, &format!("Recording 2026-09-1{k} at 15.02.40.mp4"), &format!("~/Desktop/Recording 2026-09-1{k} at 15.02.40.mp4"), b, super::captures::RECORDINGS, Verdict::Ask, "Recorded 60 days ago", Who::Rule));
    }
    for (k, b) in [1_200_000_000u64, 900_000_000, 600_000_000].into_iter().enumerate() {
        let mut up = mk(Group::Captures, &format!("Link · Recording 2026-08-2{k} at 11.40.02.mp4"), &format!("https://share.example.com/v/{k}"), b, super::captures::UPLOADS, Verdict::Ask, "Shared 80 days ago; the link stops working", Who::Rule);
        up.permanent = true;
        up.selected = false;
        items.push(up);
    }
    for (k, b) in [3_800_000u64, 4_200_000, 2_900_000].into_iter().enumerate() {
        items.push(mk(Group::Captures, &format!("Capture 2026-09-2{k} at 09.30.12"), &format!("~/Library/Application Support/cmdmeow/capture/history/175900000000{k}.png"), b, super::captures::HISTORY, Verdict::Safe, "History copy from 50 days ago", Who::Rule));
    }
    cleanup::State { phase: Phase::Results, items, ai_used: true, ..Default::default() }
}

/// The Done screen with sample results (screenshots).
pub fn demo_done() -> cleanup::State {
    let mut st = demo_results();
    st.phase = Phase::Done;
    let sel: Vec<&Item> = st.items.iter().filter(|i| i.selected).collect();
    st.cleaned_count = sel.len();
    st.cleaned_bytes = sel.iter().map(|i| i.bytes).sum();
    st.cleaned = sel.iter().map(|i| (i.group, i.bytes)).collect();
    st.trashed_bytes = sel.iter().filter(|i| !i.permanent && !i.rebuilds()).map(|i| i.bytes).sum();
    st.log = sel.iter().map(|i| (format!("{}: {}", ts!("cleanup.how.trash"), i.path.display()), false)).collect();
    st
}

/// The Done screen after cleaning one group, others still waiting (screenshots).
pub fn demo_done_group() -> cleanup::State {
    let mut st = demo_results();
    let g = Group::AppCaches;
    st.phase = Phase::Done;
    st.clean_only = Some(g).into();
    let sel: Vec<Item> = st.items.iter().filter(|i| i.selected && i.group == g).cloned().collect();
    st.cleaned_count = sel.len();
    st.cleaned_bytes = sel.iter().map(|i| i.bytes).sum();
    st.cleaned = sel.iter().map(|i| (i.group, i.bytes)).collect();
    st.trashed_bytes = sel.iter().filter(|i| !i.permanent && !i.rebuilds()).map(|i| i.bytes).sum();
    st.log = sel.iter().map(|i| (format!("{}: {}", ts!("cleanup.how.trash"), i.path.display()), false)).collect();
    st.items.retain(|i| !(i.selected && i.group == g));
    st
}

/// What AI Check gets: the biggest things found, with what the rules or the
/// path check said about each.
fn check_ask(clean: &cleanup::Shared) -> aicheck::Ask {
    let g = clean.lock_ok();
    let mut idx: Vec<usize> = (0..g.items.len()).filter(|&i| g.items[i].verdict != Verdict::Keep).collect();
    // what waits on the user first (all of it is sent), then by size
    idx.sort_by_key(|&i| (g.items[i].verdict != Verdict::Ask, std::cmp::Reverse(g.items[i].bytes)));
    // a one-answer bundle is asked once, by its biggest item, and the answer
    // covers the bundle (see `bundle_for_key`); other bundles item by item
    let asks: Vec<usize> = idx.iter().copied().filter(|&i| is_question(&g.items[i])).collect();
    let mut stands_for: std::collections::HashMap<usize, (String, usize, u64)> = std::collections::HashMap::new();
    let mut covered: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let questions = entries(&g.items, &asks);
    for e in &questions {
        if let Some(name) = e.name.clone().filter(|n| one_answer(n)) {
            stands_for.insert(e.idx[0], (name, e.idx.len(), e.bytes));
            covered.extend(e.idx[1..].iter().copied());
        }
    }
    let answer: Vec<String> = questions
        .iter()
        .flat_map(|e| if e.name.as_deref().is_some_and(one_answer) { vec![e.idx[0]] } else { e.idx.clone() })
        .take(aicheck::MAX_ANSWERS)
        .map(|i| g.items[i].path.display().to_string())
        .collect();
    let items = idx
        .into_iter()
        .filter(|i| !covered.contains(i))
        .take(40)
        .map(|i| {
            let it = &g.items[i];
            let bundle = stands_for.get(&i).map(|(name, n, bytes)| format!(" · stands for {n} alike items ({name}, {} in all): the answer covers them all", human_bytes(*bytes))).unwrap_or_default();
            let now = match it.verdict {
                Verdict::Safe => "marked safe",
                Verdict::Ask => "needs the user's OK",
                Verdict::Keep => "kept",
            };
            let why = if it.reason.is_empty() { String::new() } else { format!(" ({})", it.reason) };
            aicheck::Item {
                key: it.path.display().to_string(),
                line: format!("{} · {} · {} · {} · untouched {} days · {}{}{}{}", it.group.title(), it.title, short_path(&it.path), human_bytes(it.bytes), it.age_days.round(), now, why, if it.selected { " · ticked for cleaning" } else { "" }, bundle),
                icon: None,
                name: it.title.clone(),
            }
        })
        .collect();
    aicheck::Ask {
        task: "This is Cleanup's results: caches, logs and leftovers found on this Mac. Some items need the user's OK: say for each whether to clean it (select) or keep it. Then point out anything ticked for cleaning that the user would likely miss (documents, settings, data that cannot be downloaded again), or a big clearly safe win.".into(),
        actions: vec![
            ("select", "clean it: tick it for cleaning (the user still presses Clean and confirms)", ts!("cleanup.ai.clean")),
            ("keep", "untick it and remember it as kept", ts!("cleanup.ai.keep")),
        ],
        items,
        facts: Vec::new(),
        answer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_from_kept_asks_again_but_locks_hold() {
        let mut st = demo_results();
        let base = st.items[0].clone();
        let mk = |kind: &'static str, by: Who, hard: bool| Item { kind, by, hard_keep: hard, verdict: Verdict::Keep, rule_verdict: Verdict::Keep, rule_reason: "You used this project today — kept".into(), path: PathBuf::from(format!("/k/{kind}/{hard}")), ..base.clone() };
        st.items = vec![
            mk("Projects", Who::Rule, true),  // in-use project: may go
            mk("Caches", Who::You, false),    // the person's own Keep
            mk("Caches", Who::Rule, true),    // kept cache: locked
            mk("Worktrees", Who::You, true),  // protected, also kept by the person: locked
        ];
        let ok: Vec<bool> = st.items.iter().map(unkeepable).collect();
        assert_eq!(ok, [true, true, false, false]);
        let forget = unkeep(&mut st.items, &[0, 1, 2, 3]);
        assert_eq!(forget, vec![PathBuf::from("/k/Caches/false")]);
        for it in &st.items[..2] {
            assert_eq!((it.verdict, it.by, it.hard_keep, it.selected), (Verdict::Ask, Who::Rule, false, false));
            assert_eq!(it.reason, "You used this project today");
        }
        for it in &st.items[2..] {
            assert!(it.verdict == Verdict::Keep && it.hard_keep);
        }
        // Undo on a project brings its lock back
        let mut p = st.items[0].clone();
        p.verdict = Verdict::Safe;
        p.by = Who::You;
        undo(&mut p);
        assert!(p.verdict == Verdict::Keep && p.hard_keep);
    }

    #[test]
    fn undo_puts_a_question_back() {
        let mut st = demo_results();
        let it = st.items.iter_mut().find(|i| i.verdict == Verdict::Ask).unwrap();
        let rule_reason = it.rule_reason.clone();
        // the person said keep
        it.verdict = Verdict::Keep;
        it.by = Who::You;
        it.reason = "You said keep".into();
        assert!(undoable(it));
        assert!(undo(it));
        assert_eq!((it.verdict, it.by, it.selected), (Verdict::Ask, Who::Rule, false));
        assert_eq!(it.reason, rule_reason);
        assert!(!undoable(it));
        // a safe item the person kept goes back ticked
        let it = st.items.iter_mut().find(|i| i.rule_verdict == Verdict::Safe && !i.permanent).unwrap();
        it.verdict = Verdict::Keep;
        it.by = Who::You;
        it.selected = false;
        assert!(undo(it));
        assert!(it.selected && it.verdict == Verdict::Safe);
    }

    #[test]
    fn rows_leave_room_for_the_scroll_bar_only_when_they_scroll() {
        let rows = |n: usize| std::iter::repeat_n(84.0, n);
        assert!(!scrolls(200.0, rows(2), 8.0));
        assert!(!scrolls(176.0, rows(2), 8.0));
        assert!(scrolls(200.0, rows(3), 8.0));
        assert!(!scrolls(0.0, rows(0), 8.0));
        assert!(scrolls(100.0, [52.0, 68.0].into_iter(), 0.0), "a wrapped row counts its extra line");
    }

    #[test]
    fn alike_questions_become_one_bundle() {
        let st = demo_results();
        let items = &st.items;
        let asks: Vec<usize> = (0..items.len()).filter(|&i| is_question(&items[i])).collect();
        let es = entries(items, &asks);
        let mac = es.iter().find(|e| e.name.as_deref() == Some("macOS caches")).expect("macOS caches bundle");
        assert_eq!(mac.idx.len(), 4);
        assert!(mac.idx.windows(2).all(|w| items[w[0]].bytes >= items[w[1]].bytes), "biggest first");
        assert_eq!(mac.bytes, mac.idx.iter().map(|&i| items[i].bytes).sum::<u64>());
        // everything else stays on its own
        assert!(es.iter().filter(|e| e.name.is_none()).all(|e| e.idx.len() == 1));
        assert_eq!(es.iter().map(|e| e.idx.len()).sum::<usize>(), asks.len());
        // the AI's answer for the biggest member covers the bundle; any other key is one item
        let key = items[mac.idx[0]].path.display().to_string();
        assert_eq!(bundle_for_key(items, &key).len(), 4);
        let key = items[mac.idx[1]].path.display().to_string();
        assert_eq!(bundle_for_key(items, &key), vec![mac.idx[1]]);
        // the person's own files: one answer never covers the others
        let mut items = items.clone();
        let base = items.iter().position(|i| i.group == Group::AiTools && is_question(i)).unwrap();
        for (k, mb) in [300u64, 200, 100].into_iter().enumerate() {
            let mut it = items[base].clone();
            it.title = format!("Uploaded files · chat {k}");
            it.path = PathBuf::from(format!("/Users/me/.claude/uploads/chat-{k}"));
            it.bytes = mb * 1_000_000;
            items.push(it);
        }
        let asks: Vec<usize> = (0..items.len()).filter(|&i| is_question(&items[i])).collect();
        let up = entries(&items, &asks).into_iter().find(|e| e.name.as_deref() == Some("Claude Code uploads")).expect("uploads bundle");
        assert_eq!(up.idx.len(), 3, "still shown as one row");
        let key = items[up.idx[0]].path.display().to_string();
        assert_eq!(bundle_for_key(&items, &key), vec![up.idx[0]]);
    }

    #[test]
    fn one_group_cleans_only_its_own_files() {
        let st = demo_results();
        let g = Some(Group::AppCaches);
        let mine: u64 = st.items.iter().filter(|i| i.selected && i.group == Group::AppCaches).map(|i| i.bytes).sum();
        assert_eq!(st.selected_in(g), mine);
        assert!(st.selected_in(g) < st.selected_bytes());
        // Claude's files are in AI Tools: Claude isn't asked to quit
        let names: Vec<String> = quit_rows(&st.items, demo_apps(), g).into_iter().map(|r| r.app.name).collect();
        assert_eq!(names, ["Spotify", "Google Chrome"]);
        // the Done screen goes back to what's left
        let done = demo_done_group();
        assert!(done.items.iter().all(|i| !(i.selected && i.group == Group::AppCaches)));
        assert!(done.total() > 0 && done.clean_only == g.into());
    }

    #[test]
    fn open_apps_claim_their_selected_files() {
        let mut st = demo_results();
        let rows = quit_rows(&st.items, demo_apps(), None);
        let names: Vec<&str> = rows.iter().map(|r| r.app.name.as_str()).collect();
        assert_eq!(names, ["Spotify", "Google Chrome", "Claude"], "biggest first");
        let claude = &rows[2];
        assert!(claude.idx.iter().all(|&i| st.items[i].title.starts_with("Claude ") && quit_kind(st.items[i].kind)));
        // not selected, not counted
        for it in st.items.iter_mut().filter(|i| i.title == "Spotify") {
            it.selected = false;
        }
        assert!(quit_rows(&st.items, demo_apps(), None).iter().all(|r| r.app.name != "Spotify"));
        // a name inside a word doesn't match
        let app = OpenApp { pid: 9, id: "com.example.music".into(), name: "Music".into(), bundle: None };
        let mut it = st.items[0].clone();
        it.title = "Musicbox cache".into();
        assert!(!belongs(&app, &it));
        it.title = "Music".into();
        assert!(belongs(&app, &it));
    }

    #[test]
    fn item_counts_are_singular_for_one() {
        assert_eq!(items_n(1), "1 item");
        assert_eq!(items_n(0), "0 items");
        assert_eq!(items_n(12), "12 items");
    }

    /// SP4/SP3a: a tool's parts with one title are one row from two on, and
    /// a tool picked in the rail limits the list, the total and the clean.
    #[test]
    fn tool_parts_fold_and_scope_by_tool() {
        let base = demo_results().items.into_iter().find(|i| i.group == Group::AiTools).unwrap();
        let part = |tool: &'static str, title: &str, n: u64| {
            let mut it = base.clone();
            it.tool = tool;
            it.title = title.into();
            it.kind = "Caches";
            it.verdict = Verdict::Safe;
            it.selected = true;
            it.bytes = n;
            it.path = PathBuf::from(format!("/Users/me/{tool}/{n}"));
            it
        };
        let items = vec![part("Cursor", "Cursor · Web cache", 10), part("Cursor", "Cursor · Web cache", 5), part("Codex", "Codex · Web cache", 7), part("Codex", "Codex · Logs", 3)];
        let es = entries(&items, &[0, 2, 1, 3]);
        assert_eq!(es.len(), 3, "Cursor's two parts are one row, Codex's stay apart");
        assert_eq!(es[0].name.as_deref(), Some("Cursor · Web cache"));
        assert_eq!(es[0].bytes, 15);
        let scope = cleanup::Scope { group: Some(Group::AiTools), tool: "Codex" };
        let st = cleanup::State { items: items.clone(), ..Default::default() };
        assert_eq!(st.selected_in(scope), 10);
        assert_eq!(st.selected_in(Some(Group::AiTools)), 25);
        assert_eq!(cleanup::without_tool(&items[3]), "Logs");
        let mut items = items;
        let mut kept = part("Worktrees", "app · main", 4);
        kept.verdict = Verdict::Keep;
        items.push(kept);
        let tools = tools_of(&items);
        assert_eq!(tools.iter().map(|t| t.0).collect::<Vec<_>>(), ["Cursor", "Codex"], "only-kept tools get no row");
    }

    /// Bundles go by folder and kinds show as words: neither reads the
    /// shown title, so a translated title can't break them.
    #[test]
    fn bundles_and_kinds_do_not_read_titles() {
        let mut it = demo_results().items.into_iter().find(|i| i.group == Group::AiTools).unwrap();
        it.title = "anything".into();
        it.kind = "Old Sessions";
        for (dir, want) in [("file-history", ts!("cleanup.bundle.edit_history")), ("uploads", ts!("cleanup.bundle.claude_uploads")), ("projects", ts!("cleanup.bundle.old_chats"))] {
            it.path = PathBuf::from(format!("/Users/me/.claude/{dir}/x"));
            assert_eq!(bundle_of(&it), Some((want.to_string(), BUNDLE_MIN)), "{dir}");
        }
        it.path = PathBuf::from("/Users/me/.claude/other/x");
        assert_eq!(bundle_of(&it), None);
        assert_eq!(super::super::engine::kind_label("Old Sessions"), "Old Sessions");
        assert_eq!(super::super::engine::kind_label("Capture History"), "Capture History");
        assert_eq!(super::super::engine::kind_label("unknown"), "unknown");
    }
}
