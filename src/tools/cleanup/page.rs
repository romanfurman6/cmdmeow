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
use std::path::PathBuf;

/// Window corner radius (the footer rounds its bottom-right corner to match).
const WIN_R: f32 = 12.0;

pub fn page(app: &mut App, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let phase = app.clean.lock_ok().phase.clone();
    match phase {
        Phase::Intro => intro(app, ui, r, cx),
        Phase::Scanning | Phase::Judging | Phase::Cleaning => working(app, ui, r, phase),
        Phase::Results => results(app, ui, r),
        Phase::Done => done(app, ui, r),
    }
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

fn intro(app: &mut App, ui: &mut Ui, r: Rect, _cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    page_header(&p, r, "Cleanup", "", TEXT_DIM());
    let body = Rect::from_min_max(Pos2::new(r.left() + sp::PAGE_EDGE, r.top() + PAGE_HEADER_H + 1.0 + sp::L), Pos2::new(r.right() - sp::PAGE_EDGE, r.bottom() - sp::L));

    // top: one toolbar with the disk, the AI switch and Scan; below it the
    // groups as columns sized by what the last scan found in each
    let (total, free) = crate::kit::fs::volume_space("/");
    let used = if total > 0 { 1.0 - free as f32 / total as f32 } else { 0.0 };
    let bar_card = Rect::from_min_size(body.min, Vec2::new(body.width(), 56.0));
    glass_card(&p, bar_card, R_CARD, 0.0, false);
    let di = bar_card.shrink2(Vec2::new(sp::L, 0.0));
    let cy = di.center().y;
    // right to left: Scan, a divider, the free space (AI Check runs after the scan, in its strip)
    // never scanned: the big Scan in the middle is the only one
    let last = if app.demo && !app.demo_fresh { Some(super::engine::LastScan { when: chrono::Utc::now().timestamp() - 2 * 86400, groups: vec![14_200_000_000, 9_800_000_000, 21_600_000_000, 6_100_000_000, 2_200_000_000, 1_300_000_000, 3_400_000_000] }) } else { super::engine::last_scan() };
    let hist = if app.demo_fresh { Vec::new() } else { super::engine::history() };
    let fresh = last.is_none() && hist.is_empty();
    let right = if fresh {
        di.right()
    } else {
        let scan_w = button_width(&p, "Scan", BTN_PRIMARY).max(96.0);
        let scan_r = Rect::from_min_size(Pos2::new(di.right() + 4.0 - scan_w, cy - 17.0), Vec2::new(scan_w, 34.0));
        if pill_button(ui, scan_r, "Scan", BTN_PRIMARY, true).clicked() {
            cleanup::start_scan(app.clean.clone(), ctx.clone(), ai_agent(app));
        }
        let div = scan_r.left() - sp::L;
        p.line_segment([Pos2::new(div, cy - 14.0), Pos2::new(div, cy + 14.0)], Stroke::new(1.0, CARD_EDGE()));
        div - sp::L
    };
    icon(&p, Icon::Disk, Rect::from_center_size(Pos2::new(di.left() + 9.0, cy), Vec2::splat(18.0)), TEXT_SOFT());
    let name = p.text(Pos2::new(di.left() + 18.0 + sp::M, cy), Align2::LEFT_CENTER, "Macintosh HD", ty::title(), TEXT());
    let free_s = if total > 0 { format!("{} free of {}", human_bytes(free), human_bytes(total)) } else { String::new() };
    let fr = p.text(Pos2::new(right, cy), Align2::RIGHT_CENTER, &free_s, ty::body(), TEXT_SOFT());
    let bar_r = Rect::from_min_max(Pos2::new(name.right() + sp::L, cy - 3.0), Pos2::new(fr.left() - sp::L, cy + 3.0));
    if bar_r.width() > 40.0 {
        bar(&p, bar_r, used, BLUE());
    }

    // a note when Claude is missing, at the bottom
    let chart_bottom = body.bottom();

    // never scanned: what a scan looks through, and one big Scan
    if fresh {
        let area = Rect::from_min_max(Pos2::new(body.left(), bar_card.bottom() + sp::M), Pos2::new(body.right(), chart_bottom));
        if fresh_start(ui, area) {
            cleanup::start_scan(app.clean.clone(), ctx.clone(), ai_agent(app));
        }
        return;
    }

    // the chart card
    let card = Rect::from_min_max(Pos2::new(body.left(), bar_card.bottom() + sp::M), Pos2::new(body.right(), chart_bottom));
    glass_card(&p, card, R_CARD, 0.0, false);
    let ci = card.shrink(sp::XL);
    let sizes: Vec<u64> = Group::ALL.iter().enumerate().map(|(i, _)| last.as_ref().and_then(|l| l.groups.get(i).copied()).unwrap_or(0)).collect();
    let found: u64 = sizes.iter().sum();
    let mut top = ci.top();
    if let Some(l) = &last {
        p.text(Pos2::new(ci.left(), top), Align2::LEFT_TOP, format!("Last scan {}", super::engine::days_ago(l.when)), ty::label(), TEXT_DIM());
        p.text(Pos2::new(ci.left(), top + 20.0), Align2::LEFT_TOP, human_bytes(found), ty::hero(), TEXT());
        top += 20.0 + 34.0;
    }
    if !hist.is_empty() {
        let freed: u64 = hist.iter().map(|h| h.1).sum();
        p.text(Pos2::new(ci.right(), ci.top()), Align2::RIGHT_TOP, format!("{} freed in {} clean{}", human_bytes(freed), hist.len(), if hist.len() == 1 { "" } else { "s" }), ty::label(), TEXT_DIM());
        top = top.max(ci.top() + 20.0);
    }
    if top > ci.top() {
        top += sp::L;
    }

    // columns, biggest first; the colour follows the rank
    let mut order: Vec<usize> = (0..Group::ALL.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let max = sizes.iter().copied().max().unwrap_or(0).max(1) as f32;
    let label_h = 18.0;
    let base = ci.bottom() - label_h - sp::M;
    let col_top = top + 22.0;
    let n = order.len() as f32;
    let gap = sp::L;
    let cw = (ci.width() - gap * (n - 1.0)) / n;
    let full = (base - col_top).max(20.0);
    for (k, &i) in order.iter().enumerate() {
        let g = Group::ALL[i];
        let x = ci.left() + k as f32 * (cw + gap);
        let col = junk_ramp(k);
        let track = Rect::from_min_max(Pos2::new(x, col_top), Pos2::new(x + cw, base));
        p.rect_filled(track, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, with_alpha(col, 0.06));
        if sizes[i] > 0 {
            let h = (full * sizes[i] as f32 / max).max(8.0);
            let fill = Rect::from_min_max(Pos2::new(x, base - h), Pos2::new(x + cw, base));
            p.rect_filled(fill, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, col);
            p.text(Pos2::new(fill.center().x, fill.top() - 6.0), Align2::CENTER_BOTTOM, human_bytes(sizes[i]), ty::title(), TEXT());
        }
        // icon + name under the baseline
        let ny = ci.bottom() - label_h / 2.0;
        // the whole name; the icon goes first when both don't fit the column
        let nw = p.layout_no_wrap(g.title().to_string(), ty::body(), TEXT()).size().x;
        // (a label may run into the gap between columns)
        let avail = cw + gap - sp::XS;
        let with_icon = nw + 22.0 <= avail;
        let lx = if with_icon { x + (cw - nw - 22.0) / 2.0 } else { x + (cw - nw.min(avail)) / 2.0 - 22.0 };
        let ir = Rect::from_center_size(Pos2::new(lx + 8.0, ny), Vec2::splat(16.0));
        if with_icon {
            group_icon(&p, g, ir, col);
        }
        text_fit(&p, Pos2::new(lx + 22.0, ny), Align2::LEFT_CENTER, g.title(), ty::body(), TEXT_SOFT(), avail);
    }
    p.line_segment([Pos2::new(ci.left(), base), Pos2::new(ci.right(), base)], Stroke::new(1.0, CARD_EDGE()));
}

/// A group's glyph (Developer: the SF hammer when there is one).
fn group_icon(p: &egui::Painter, g: Group, r: Rect, col: Color32) {
    if matches!(g, Group::Developer) && symbol(p, "hammer", r, col) {
        return;
    }
    let ic = match g {
        Group::AiTools => Icon::Gauge,
        Group::AppCaches => Icon::Folder,
        Group::Developer => Icon::Gear,
        Group::Projects => Icon::Folder,
        Group::Installers => Icon::Down,
        Group::Logs => Icon::Doc,
        Group::Trash => Icon::Trash,
    };
    icon(p, ic, r, col);
}

/// Before the first scan: every group a scan looks through as a round icon
/// with its name, in one row (two when narrow), and a big Scan under them,
/// all centred in `area`. True when Scan was clicked.
fn fresh_start(ui: &mut Ui, area: Rect) -> bool {
    let p = ui.painter().clone();
    let n = Group::ALL.len();
    let cell = 112.0;
    let per_row = if area.width() >= cell * n as f32 { n } else { n.div_ceil(2) };
    let rows = n.div_ceil(per_row);
    let cell_h = 92.0;
    let btn = Vec2::new(200.0, 44.0);
    let block_h = rows as f32 * cell_h + sp::XXL + btn.y;
    let top = area.center().y - block_h / 2.0;
    for (k, g) in Group::ALL.into_iter().enumerate() {
        let row = k / per_row;
        let in_row = if row + 1 == rows { n - row * per_row } else { per_row };
        let col_i = k % per_row;
        let x0 = area.center().x - in_row as f32 * cell / 2.0;
        let c = Pos2::new(x0 + (col_i as f32 + 0.5) * cell, top + row as f32 * cell_h + 26.0);
        let col = junk_ramp(k);
        p.circle_filled(c, 26.0, with_alpha(col, 0.14));
        group_icon(&p, g, Rect::from_center_size(c, Vec2::splat(22.0)), col);
        text_fit(&p, Pos2::new(c.x, c.y + 40.0), Align2::CENTER_CENTER, g.title(), ty::body(), TEXT_SOFT(), cell - 8.0);
    }
    let br = Rect::from_center_size(Pos2::new(area.center().x, top + rows as f32 * cell_h + sp::XXL + btn.y / 2.0), btn);
    let resp = ui.interact(br, ui.scope_id().with("fresh-scan"), Sense::click());
    let hov = ui.ctx().animate_bool(resp.id, resp.hovered());
    liquid_capsule(&p, br, Some(BLUE()), if resp.is_pointer_button_down_on() { 1.6 } else { hov }, 1.0);
    p.text(br.center(), Align2::CENTER_CENTER, "Scan", bold(16.0), on_color(BLUE()));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
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
        Phase::Judging => format!("Checking with {who}…"),
        Phase::Cleaning => "Cleaning…".into(),
        _ => "Scanning…".into(),
    };
    let btns = page_header(&p, r, "Cleanup", &sub, TEXT_DIM());
    // Stop: back to the start; Skip: the results now, by the built-in rules
    let stop = match phase {
        Phase::Scanning => Some("Stop"),
        Phase::Judging => Some("Skip"),
        _ => None,
    };
    if let Some(label) = stop {
        let w = button_width(&p, label, BTN_SECONDARY).max(64.0);
        let sr = Rect::from_min_max(Pos2::new(btns.right() - w, btns.top()), btns.max);
        if pill_button(ui, sr, label, BTN_SECONDARY, true).clicked() {
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
        (human_bytes(st.cleaned_bytes), format!("cleaned so far · {} items", st.cleaned_count))
    } else if phase == Phase::Scanning {
        // the items arrive at the end; the live tallies grow as each place is measured
        let bytes: u64 = st.live.iter().map(|l| l.bytes).sum();
        let n: usize = st.live.iter().map(|l| l.items).sum();
        (human_bytes(bytes), format!("found so far · {n} items"))
    } else {
        (human_bytes(st.items.iter().filter(|i| i.verdict != Verdict::Keep).map(|i| i.bytes).sum()), format!("found so far · {} items", st.items.len()))
    };
    let g = p.layout_no_wrap(big, bold(28.0), TEXT());
    let gw = g.size().x;
    p.galley(Pos2::new(x0, y), g, TEXT());
    p.text(Pos2::new(x0 + gw + 10.0, y + 22.0), Align2::LEFT_BOTTOM, caption, font(13.0), TEXT_DIM());
    if st.progress >= 0.0 {
        p.text(Pos2::new(x1, y + 22.0), Align2::RIGHT_BOTTOM, format!("{:.0}%", st.progress.clamp(0.0, 1.0) * 100.0), font(13.0), TEXT_DIM());
    }
    y += 40.0;
    // progress bar (an indeterminate sweep while the scan can't count)
    let track = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(x1 - x0, 6.0));
    if st.progress >= 0.0 {
        bar(&p, track, st.progress, BLUE());
    } else {
        p.rect_filled(track, 3.0, white(20));
        let w = track.width() * 0.25;
        let f = ((t * 0.6).fract() as f32) * (track.width() + w) - w;
        let seg = Rect::from_min_size(Pos2::new(track.left() + f, track.top()), Vec2::new(w, 6.0)).intersect(track);
        if seg.width() > 0.0 {
            p.rect_filled(seg, 3.0, BLUE());
        }
    }
    y += 16.0;
    // what it's doing right now
    match phase {
        Phase::Judging => {
            let line = if st.ai_status.is_empty() { format!("Asking {who}…") } else { st.ai_status.clone() };
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
    section_header(&p, Pos2::new(cols[1], y), Align2::LEFT_TOP, "Group");
    section_header(&p, Pos2::new(cols[2], y), Align2::LEFT_TOP, "Items");
    section_header(&p, Pos2::new(cols[3], y), Align2::RIGHT_TOP, if phase == Phase::Cleaning { "Selected" } else { "Found" });
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
            spinner(&p, mark, 7.0, t, BLUE());
        } else if n > 0 {
            p.circle_filled(mark, 8.0, tint::HEALTH());
            icon(&p, Icon::Check, Rect::from_center_size(mark, Vec2::splat(11.0)), ON_ACTION);
        } else {
            crate::kit::glass::stroke_circle(&p, mark, 7.5, Stroke::new(1.5, white(64)));
        }
        p.text(Pos2::new(cols[1], row.center().y), Align2::LEFT_CENTER, grp.title(), font(13.0), TEXT());
        let (status, sc) = if n > 0 { (format!("{n} found"), GREEN()) } else { ("—".to_string(), TEXT_FAINT()) };
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
    let (total, pending, ai_used, ai_error, selected) = {
        let g = app.clean.lock_ok();
        (g.total(), g.pending_questions(), g.ai_used, g.ai_error.clone(), g.selected_bytes())
    };
    let mut sub = format!("{} found", human_bytes(total));
    if let Some(e) = &ai_error {
        sub = format!("{sub} · {e}");
    }
    let _ = ai_used;
    // AI Check: a chip after the title
    let ai_n = app.clean.lock_ok().items.iter().filter(|i| i.verdict != Verdict::Keep).count();
    let cw = aicheck::ui::chip_width(&app.ai, ui, ai_n > 0);
    let (btns, chip_at) = page_header_room(&p, r, "Cleanup", &sub, if ai_error.is_some() { AMBER() } else { TEXT_DIM() }, cw, r.right() - 260.0);
    let w = button_width(&p, "Scan again", BTN_SECONDARY);
    let sr = Rect::from_min_max(Pos2::new(btns.right() - w, btns.top()), btns.max);
    if pill_button(ui, sr, "Scan again", BTN_SECONDARY, true).clicked() {
        // back to the start screen, where the scan can be set up and started
        cleanup::stop_scan(&app.clean);
        app.ai.reset();
        return;
    }
    let hidden = app.clean.lock_ok().hidden_kept;
    if hidden > 0 {
        let label = format!("Show {hidden} kept");
        let kw = button_width(&p, &label, BTN_SECONDARY);
        let kr = Rect::from_min_max(Pos2::new(sr.left() - 8.0 - kw, btns.top()), Pos2::new(sr.left() - 8.0, btns.bottom()));
        if pill_button(ui, kr, &label, BTN_SECONDARY, true).on_hover_text("You chose to keep these before, so they are hidden. Show them again and scan.").clicked() {
            cleanup::forget_kept();
            cleanup::start_scan(app.clean.clone(), ctx.clone(), ai_agent(app));
            return;
        }
    }

    if total == 0 && pending == 0 {
        let c = Pos2::new(r.center().x, r.top() + PAGE_HEADER_H + 150.0);
        p.circle_filled(c, 28.0, tint::HEALTH());
        icon(&p, Icon::Check, Rect::from_center_size(c, Vec2::splat(28.0)), ON_ACTION);
        p.text(c + Vec2::new(0.0, 56.0), Align2::CENTER_CENTER, "Nothing to clean", bold(22.0), TEXT());
        p.text(c + Vec2::new(0.0, 84.0), Align2::CENTER_CENTER, "No caches, logs or leftovers worth removing were found.", font(13.0), TEXT_DIM());
        let dr = Rect::from_center_size(c + Vec2::new(0.0, 130.0), Vec2::new(96.0, 32.0));
        if pill_button(ui, dr, "Done", BTN_PRIMARY, true).clicked() {
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
                let i = app.clean.lock_ok().items.iter().position(|it| it.path.display().to_string() == rec.key);
                if let Some(i) = i {
                    answer_items(app, &[i], if rec.action == "keep" { Verdict::Keep } else { Verdict::Safe });
                }
                app.ai.done(&rec);
            }
            None => {}
        }
    }

    // the groups as tabs: All, then each group that found something
    let mut tabs: Vec<(Option<Group>, String, u64, usize, Color32)> = vec![];
    {
        let g = app.clean.lock_ok();
        let asks = |f: &dyn Fn(&Item) -> bool| g.items.iter().filter(|i| f(i) && i.verdict == Verdict::Ask && i.by != Who::You).count();
        tabs.push((None, "All".into(), total, asks(&|_| true), TEXT_DIM()));
        for (k, grp) in Group::ALL.iter().enumerate() {
            if g.items.iter().any(|i| i.group == *grp) {
                tabs.push((Some(*grp), grp.title().into(), g.group_bytes(*grp), asks(&|i| i.group == *grp), junk_ramp(k)));
            }
        }
    }
    if !tabs.iter().any(|t| t.0 == app.clean_group) {
        app.clean_group = None;
    }
    let mut x = body.left();
    let tab_h = 30.0;
    for (grp, title, bytes, q, dot) in &tabs {
        let size = human_bytes(*bytes);
        let tw = p.layout_no_wrap(title.clone(), ty::body(), TEXT()).size().x;
        let sw = p.layout_no_wrap(size.clone(), ty::body(), TEXT()).size().x;
        let bw = if *q > 0 { (q.to_string().len() as f32 * 7.0 + 14.0).max(20.0) + sp::S } else { 0.0 };
        let w = 12.0 + 8.0 + sp::S + tw + sp::S + sw + bw + 12.0;
        let tr = Rect::from_min_size(Pos2::new(x, body.top()), Vec2::new(w, tab_h));
        let sel = app.clean_group == *grp;
        let id = ui.scope_id().with(("tab", title.as_str()));
        let resp = ui.interact(tr, id, Sense::click());
        let hv = ctx.animate_bool(id, resp.hovered());
        if sel {
            p.rect_filled(tr, R_BUTTON, with_alpha(BLUE(), 0.16));
            p.rect_stroke(tr, R_BUTTON, Stroke::new(1.0, with_alpha(BLUE(), 0.45)), StrokeKind::Inside);
        } else {
            p.rect_filled(tr, R_BUTTON, lerp_color(CARD(), RAISED(), hv));
            p.rect_stroke(tr, R_BUTTON, Stroke::new(1.0, CARD_EDGE()), StrokeKind::Inside);
        }
        let mut cx_ = tr.left() + 12.0;
        p.circle_filled(Pos2::new(cx_ + 4.0, tr.center().y), 4.0, *dot);
        cx_ += 8.0 + sp::S;
        let t = p.text(Pos2::new(cx_, tr.center().y), Align2::LEFT_CENTER, title, ty::body(), if sel { ACCENT() } else { TEXT() });
        let s = p.text(Pos2::new(t.right() + sp::S, tr.center().y), Align2::LEFT_CENTER, &size, ty::body(), TEXT_DIM());
        if *q > 0 {
            badge(&p, Pos2::new(s.right() + sp::S, tr.center().y), Align2::LEFT_CENTER, &q.to_string());
        }
        let resp = match grp {
            Some(g) => resp.on_hover_text(g.blurb()),
            None => resp,
        };
        if resp.clicked() {
            app.clean_group = *grp;
        }
        if resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        x = tr.right() + sp::S;
    }

    // the chosen tab's items, split three ways
    let (asks, safe, kept): (Vec<usize>, Vec<usize>, Vec<usize>) = {
        let g = app.clean.lock_ok();
        let mut mine: Vec<usize> = g.items.iter().enumerate().filter(|(_, it)| app.clean_group.is_none_or(|gr| it.group == gr)).map(|(i, _)| i).collect();
        mine.sort_by_key(|&i| std::cmp::Reverse(g.items[i].bytes));
        let asks = mine.iter().copied().filter(|&i| g.items[i].verdict == Verdict::Ask && g.items[i].by != Who::You).collect();
        let safe = mine.iter().copied().filter(|&i| g.items[i].verdict == Verdict::Safe || (g.items[i].verdict == Verdict::Ask && g.items[i].by == Who::You)).collect();
        let kept = mine.iter().copied().filter(|&i| g.items[i].verdict == Verdict::Keep).collect();
        (asks, safe, kept)
    };
    let shared = app.clean.clone();
    let sum = |v: &[usize]| -> u64 {
        let g = shared.lock_ok();
        v.iter().map(|&i| g.items[i].bytes).sum()
    };
    let cols = Rect::from_min_max(Pos2::new(body.left(), body.top() + tab_h + sp::L), body.max);
    let (left, right) = if asks.is_empty() {
        (None, cols)
    } else if safe.is_empty() && kept.is_empty() {
        (Some(cols), Rect::NOTHING)
    } else {
        let lw = ((cols.width() - sp::L) * 0.56).round();
        (
            Some(Rect::from_min_size(cols.min, Vec2::new(lw, cols.height()))),
            Rect::from_min_max(Pos2::new(cols.left() + lw + sp::L, cols.top()), cols.max),
        )
    };
    const LABEL_H: f32 = 20.0;

    // left: questions, one card each
    if let Some(lc) = left {
        let lab = section_label(&p, lc.left_top(), "Needs your OK", AMBER(), asks.len(), sum(&asks));
        let mut answer: Option<Verdict> = None;
        let a = link(ui, Pos2::new(lc.right(), lab.center().y), Align2::RIGHT_CENTER, "Keep all", 12.0);
        if a.clicked() {
            answer = Some(Verdict::Keep);
        }
        if link(ui, Pos2::new(a.rect.left() - sp::M, lab.center().y), Align2::RIGHT_CENTER, "Clean all", 12.0).clicked() {
            answer = Some(Verdict::Safe);
        }
        if let Some(v) = answer {
            answer_items(app, &asks, v);
        }
        let area = Rect::from_min_max(Pos2::new(lc.left(), lc.top() + LABEL_H + sp::S), lc.max);
        scroll_rows(ui, area, format!("asks-{:?}", app.clean_group), asks.len(), |_, _| ASK_H, sp::S, |ui, k, rr| ask_card(app, ui, asks[k], rr));
    }

    // AI Check tips go under the right column; a tip about a Safe item sits
    // on that item's row instead, so the list and the tip never disagree
    let tips: Vec<aicheck::Rec> = if right.is_positive() {
        let g = app.clean.lock_ok();
        app.ai.tips().into_iter().filter(|t| !safe.iter().any(|&i| g.items[i].path.display().to_string() == t.key)).collect()
    } else {
        Vec::new()
    };
    let tip_w = right.width();
    let tip_hs: Vec<f32> = tips.iter().map(|t| tip_height(&p, t, tip_w)).collect();
    // at most a third of the column, so the lists always keep room
    let tips_h: f32 = if tips.is_empty() { 0.0 } else { (sp::M + tip_hs.iter().sum::<f32>() + sp::S * (tips.len() - 1) as f32).min(right.height() / 3.0) };
    let right = if right.is_positive() { Rect::from_min_max(right.min, Pos2::new(right.right(), right.bottom() - tips_h)) } else { right };

    // right: what will go (Safe) and what stays (Kept)
    if right.is_positive() {
        // a reason that doesn't fit on one line wraps and makes its row taller
        let row_tips = app.ai.tips();
        let items: Vec<Item> = app.clean.lock_ok().items.iter().map(|it| with_row_tip(it, &row_tips)).collect();
        let row_h = |idx: &[usize], k: usize, w: f32| row_height(&p, &items[idx[k]], w);
        let need = |idx: &[usize]| if idx.is_empty() { 0.0 } else { LABEL_H + sp::S + (0..idx.len()).map(|k| row_h(idx, k, right.width())).sum::<f32>() + 8.0 };
        let (mut safe_h, mut kept_h) = (need(&safe), need(&kept));
        let gap = if safe_h > 0.0 && kept_h > 0.0 { sp::L } else { 0.0 };
        if safe_h + kept_h + gap > right.height() {
            kept_h = kept_h.min(if safe_h > 0.0 { right.height() * 0.4 } else { right.height() });
            safe_h = (right.height() - kept_h - gap).max(0.0);
        }
        let safe_r = Rect::from_min_size(right.min, Vec2::new(right.width(), safe_h));
        if !safe.is_empty() {
            let lab = section_label(&p, safe_r.left_top(), "Safe", ACCENT(), safe.len(), sum(&safe));
            let all_on = {
                let g = app.clean.lock_ok();
                safe.iter().all(|&i| g.items[i].selected || g.items[i].hard_keep)
            };
            if link(ui, Pos2::new(safe_r.right(), lab.center().y), Align2::RIGHT_CENTER, if all_on { "None" } else { "Select all" }, 12.0).clicked() {
                let mut g = app.clean.lock_ok();
                for &i in &safe {
                    let it = &mut g.items[i];
                    it.selected = !all_on && !it.hard_keep && !it.permanent;
                }
            }
            let card = Rect::from_min_max(Pos2::new(safe_r.left(), safe_r.top() + LABEL_H + sp::S), safe_r.max);
            glass_card(&p, card, R_CARD, 0.0, false);
            scroll_rows(ui, card.shrink2(Vec2::new(0.0, 4.0)), format!("safe-{:?}", app.clean_group), safe.len(), |k, w| row_h(&safe, k, w), 0.0, |ui, k, rr| safe_row(app, ui, safe[k], rr));
        }
        if !kept.is_empty() {
            let kr = Rect::from_min_size(Pos2::new(right.left(), safe_r.bottom() + gap), Vec2::new(right.width(), kept_h));
            section_label(&p, kr.left_top(), "Kept", TEXT_DIM(), kept.len(), sum(&kept));
            let card = Rect::from_min_max(Pos2::new(kr.left(), kr.top() + LABEL_H + sp::S), kr.max);
            glass_card(&p, card, R_CARD, 0.0, false);
            scroll_rows(ui, card.shrink2(Vec2::new(0.0, 4.0)), format!("kept-{:?}", app.clean_group), kept.len(), |k, w| row_h(&kept, k, w), 0.0, |ui, k, rr| kept_row(app, ui, kept[k], rr));
        }
        // under the lists, right after the last one
        let mut ty_ = right.top() + safe_h + gap + kept_h + sp::M;
        let mut act = None;
        for (t, h) in tips.iter().zip(&tip_hs) {
            if ty_ + h > right.bottom() + tips_h {
                break;
            }
            if let Some(e) = tip_row(ui, Rect::from_min_size(Pos2::new(right.left(), ty_), Vec2::new(tip_w, *h)), t) {
                act = Some(e);
            }
            ty_ += h + sp::S;
        }
        if let Some(rec) = act {
            let i = app.clean.lock_ok().items.iter().position(|it| it.path.display().to_string() == rec.key);
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

    // footer
    let f = page_footer(&p, r, WIN_R);
    let fy = f.center().y;
    // the segmented control already says where it goes; only warn when it can't come back
    let ty_ = if app.delete_now { fy - 9.0 } else { fy };
    p.text(Pos2::new(f.left() + 24.0, ty_), Align2::LEFT_CENTER, format!("{} selected", human_bytes(selected)), bold(13.0), TEXT());
    if app.delete_now {
        p.text(Pos2::new(f.left() + 24.0, fy + 10.0), Align2::LEFT_CENTER, "Can't be undone", font(12.0), AMBER());
    }
    let clean_label = format!("Clean {}", human_bytes(selected));
    let cw = button_width(&p, &clean_label, BTN_PRIMARY).max(120.0);
    let cr = Rect::from_min_size(Pos2::new(f.right() - 24.0 - cw, fy - 16.0), Vec2::new(cw, 32.0));
    if pill_button(ui, cr, &clean_label, BTN_PRIMARY, selected > 0).clicked() {
        app.clean_confirm = true;
    }
    let seg = Rect::from_min_size(Pos2::new(cr.left() - 16.0 - 200.0, fy - 14.0), Vec2::new(200.0, 28.0));
    if let Some(k) = segmented(ui, seg, ui.scope_id().with("removal"), &["Move to Trash", "Delete now"], app.delete_now as usize) {
        app.delete_now = k == 1;
    }
    if app.clean_confirm {
        confirm(app, ui, r);
    }
}

/// "Needs your OK  5 · 29.8 GB": a section title with its count and size.
fn section_label(p: &egui::Painter, at: Pos2, title: &str, col: Color32, n: usize, bytes: u64) -> Rect {
    let t = p.text(Pos2::new(at.x, at.y + 10.0), Align2::LEFT_CENTER, title, ty::title(), col);
    let s = p.text(Pos2::new(t.right() + sp::S, t.center().y), Align2::LEFT_CENTER, format!("{n} · {}", human_bytes(bytes)), ty::label(), TEXT_DIM());
    t.union(s)
}

/// A scrolling list of `n` rows in `area`, row `k` `row_h(k, width)` tall;
/// only the rows in view are drawn.
fn scroll_rows(ui: &mut Ui, area: Rect, salt: String, n: usize, row_h: impl Fn(usize, f32) -> f32, gap: f32, mut f: impl FnMut(&mut Ui, usize, Rect)) {
    if n == 0 || !area.is_positive() {
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(area.intersect(ui.clip_rect()));
    // when it scrolls, the rows stop short of the scroll bar (and may wrap
    // more in the narrower width)
    let w = area.width() - if scrolls(area.height(), (0..n).map(|k| row_h(k, area.width())), gap) { sp::M } else { 0.0 };
    ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing.y = gap;
        for k in 0..n {
            // never past the scroll gutter (sp::SCROLL_GUTTER) either
            let rw = w.min(ui.available_width());
            let (rect, _) = ui.allocate_exact_size(Vec2::new(rw, row_h(k, rw)), Sense::hover());
            if ui.is_rect_visible(rect) {
                f(ui, k, rect);
            }
        }
    });
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
            // AI Check's answer stays: Undo brings the card back with it
            if v == Verdict::Keep {
                kept.push(it.path.clone());
            }
            it.verdict = v;
            it.by = Who::You;
            it.selected = v == Verdict::Safe;
            it.reason = if v == Verdict::Safe { "You said clean".into() } else { "You said keep".into() };
        }
    }
    if !kept.is_empty() {
        cleanup::remember_kept(kept);
    }
}

/// "✦ Tip: title. why" wrapped, with its action as a link on the right.
fn tip_text(t: &aicheck::Rec, w: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append("Tip: ", 0.0, egui::TextFormat { font_id: bold(12.0), color: tint::HEALTH(), ..Default::default() });
    let title = t.title.trim_end_matches('.');
    let body = if t.why.is_empty() { format!("{title}.") } else { format!("{title}. {}", t.why) };
    job.append(&body, 0.0, egui::TextFormat { font_id: ty::label(), color: TEXT_SOFT(), ..Default::default() });
    job.wrap.max_width = w.max(1.0);
    job.wrap.max_rows = 3;
    job
}

const TIP_LINK_W: f32 = 64.0;

fn tip_height(p: &egui::Painter, t: &aicheck::Rec, w: f32) -> f32 {
    p.layout_job(tip_text(t, w - 18.0 - TIP_LINK_W)).size().y.max(16.0)
}

fn tip_row(ui: &mut Ui, r: Rect, t: &aicheck::Rec) -> Option<aicheck::Rec> {
    let p = ui.painter().clone();
    icon(&p, Icon::Sparkle, Rect::from_center_size(Pos2::new(r.left() + 6.0, r.top() + 8.0), Vec2::splat(12.0)), tint::HEALTH());
    let g = p.layout_job(tip_text(t, r.width() - 18.0 - TIP_LINK_W));
    p.galley(Pos2::new(r.left() + 18.0, r.top()), g, TEXT());
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

/// A question card: name and size, what it is, the path, then a strip with
/// the agent's answer and Clean / Keep.
const ASK_H: f32 = 144.0;
const STRIP_H: f32 = 48.0;

/// Hover: a light highlight, and the path when the row doesn't show it;
/// double click: show in Finder.
fn row_extras(ui: &mut Ui, rr: Rect, it: &Item, path_tip: bool) {
    let resp = ui.interact(rr, ui.scope_id().with(("row", &it.path)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rr, R_SMALL, white(6));
    }
    let resp = if path_tip { resp.on_hover_text(short_path(&it.path)) } else { resp };
    if resp.double_clicked() {
        let _ = std::process::Command::new("open").arg("-R").arg(&it.path).spawn();
    }
}

/// Each line a wrapped reason adds to a Safe / Kept row.
const REASON_LINE: f32 = 16.0;

/// The room for the reason in a Safe / Kept row `w` wide: after the
/// checkbox (or lock), before the size, the "Permanent" chip and Undo.
fn reason_room(p: &egui::Painter, it: &Item, w: f32) -> f32 {
    let size_w = p.layout_no_wrap(human_bytes(it.bytes), ty::title(), TEXT()).size().x;
    let chip_w = if it.permanent { p.layout_no_wrap("Permanent".into(), bold(11.0), TEXT()).size().x + 16.0 + sp::S } else { 0.0 };
    let lead = 4.0 + sp::M + 16.0 + sp::M;
    let undo = if undoable(it) { UNDO_W } else { 0.0 };
    (w - 4.0 - sp::M - size_w - sp::M - chip_w - lead - undo).max(60.0)
}

/// Room left for "Undo" after the reason.
const UNDO_W: f32 = 60.0;

/// The reason wrapped to the row's room: the app's own words are never cut.
fn reason_galley(p: &egui::Painter, it: &Item, room: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    p.layout(it.reason.clone(), ty::label(), color, room)
}

/// A Safe / Kept row's height for width `w`: taller by a line for each line
/// the reason wraps onto.
fn row_height(p: &egui::Painter, it: &Item, w: f32) -> f32 {
    if it.reason.is_empty() {
        return sp::ROW2_H;
    }
    let lines = reason_galley(p, it, reason_room(p, it, w), TEXT_DIM()).rows.len().max(1);
    sp::ROW2_H + (lines - 1) as f32 * REASON_LINE
}

/// Title and wrapped reason of a Safe / Kept row, centred in `rr`; returns
/// the reason's rect (Undo goes after its last line).
fn row_text(p: &egui::Painter, rr: Rect, x: f32, title_right: f32, it: &Item, title_col: Color32, reason_col: Color32) -> Option<Rect> {
    let room = reason_room(p, it, rr.width() + 8.0);
    let g = (!it.reason.is_empty()).then(|| reason_galley(p, it, room, reason_col));
    let th = line_h(p, &ty::body());
    let gh = g.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let block = th + if g.is_some() { sp::LINE_GAP + gh } else { 0.0 };
    let top = rr.center().y - block / 2.0;
    text_fit(p, Pos2::new(x, top + th / 2.0), Align2::LEFT_CENTER, &it.title, ty::body(), title_col, title_right - x);
    g.map(|g| {
        let at = Pos2::new(x, top + th + sp::LINE_GAP);
        let r = Rect::from_min_size(at, g.size());
        p.galley(at, g, reason_col);
        r
    })
}

/// One question: name and size, what it is, the path; under them a strip
/// with the agent's answer (its button lit) and Clean / Keep.
fn ask_card(app: &mut App, ui: &mut Ui, i: usize, rr: Rect) {
    let it = app.clean.lock_ok().items[i].clone();
    let key = it.path.display().to_string();
    let p = ui.painter().clone();
    glass_card(&p, rr, R_CARD, 0.0, false);
    // the AI Check answer under the pointer points at its card
    if app.ai.hovered.as_deref() == Some(key.as_str()) {
        p.rect_stroke(rr, R_CARD, Stroke::new(2.0, with_alpha(tint::HEALTH(), 0.7)), StrokeKind::Inside);
    }
    row_extras(ui, rr, &it, false);
    let tx = rr.left() + sp::L;
    let tw = rr.right() - sp::L - tx;
    // name … size
    let mut y = rr.top() + sp::L;
    let sz = p.text(Pos2::new(rr.right() - sp::L, y), Align2::RIGHT_TOP, human_bytes(it.bytes), ty::title(), TEXT());
    let mut right = sz.left() - sp::M;
    if it.permanent {
        right = chip(&p, Pos2::new(right, y - 2.0), Align2::RIGHT_TOP, "Permanent", CHIP_RISK()).left() - sp::S;
    }
    let t = text_fit(&p, Pos2::new(tx, y), Align2::LEFT_TOP, &it.title, ty::title(), TEXT(), right - tx);
    y = t.bottom() + sp::LINE_GAP;
    // what it is (the rule's words), then where
    let what = if it.rule_reason.is_empty() { it.kind.to_string() } else { it.rule_reason.clone() };
    let w = text_fit(&p, Pos2::new(tx, y), Align2::LEFT_TOP, &what, ty::label(), TEXT_SOFT(), tw);
    y = w.bottom() + sp::LINE_GAP;
    text_fit(&p, Pos2::new(tx, y), Align2::LEFT_TOP, &middle_ellipsis(&short_path(&it.path), 90), mono(11.0), TEXT_FAINT(), tw);

    // the strip: the agent's answer, Clean and Keep
    let strip = Rect::from_min_max(Pos2::new(rr.left() + sp::M, rr.bottom() - sp::M - STRIP_H), Pos2::new(rr.right() - sp::M, rr.bottom() - sp::M));
    let ans = app.ai.answer_for(&key);
    let agent = if app.demo { Agent::Claude.name() } else { super::agent::main().name() };
    let ai = tint::HEALTH();
    p.rect_filled(strip, R_SMALL, if ans.is_some() { with_alpha(ai, 0.08) } else { white(5) });
    let keep_lit = ans.as_ref().is_some_and(|a| a.action == "keep");
    let clean_lit = !keep_lit;
    let style = |lit: bool| if lit { BTN_PRIMARY } else { BTN_SECONDARY };
    let cy = strip.center().y;
    let kw = button_width(&p, "Keep", style(keep_lit));
    let cw = button_width(&p, "Clean", style(clean_lit));
    let kr = Rect::from_min_size(Pos2::new(strip.right() - sp::S - kw, cy - sp::CONTROL_H / 2.0), Vec2::new(kw, sp::CONTROL_H));
    let cr = Rect::from_min_size(Pos2::new(kr.left() - sp::S - cw, kr.top()), Vec2::new(cw, sp::CONTROL_H));
    let lx = strip.left() + sp::M;
    let lw = cr.left() - sp::M - lx;
    let running = app.ai.phase() == aicheck::Phase::Running && app.ai.answer.contains(&key);
    if let Some(a) = &ans {
        icon(&p, Icon::Sparkle, Rect::from_center_size(Pos2::new(lx + 6.0, cy), Vec2::splat(12.0)), ai);
        let verdict = if keep_lit { "keep" } else { "clean" };
        let mut job = egui::text::LayoutJob::default();
        job.append(&format!("{}: {verdict}. ", agent), 0.0, egui::TextFormat { font_id: bold(12.0), color: ai, ..Default::default() });
        job.append(&a.why, 0.0, egui::TextFormat { font_id: ty::label(), color: TEXT(), ..Default::default() });
        job.wrap.max_width = (lw - 18.0).max(1.0);
        job.wrap.max_rows = 2;
        let g = p.layout_job(job);
        let gr = Align2::LEFT_CENTER.anchor_size(Pos2::new(lx + 18.0, cy), g.size());
        p.galley(gr.min, g, TEXT());
    } else if running {
        spinner(&p, Pos2::new(lx + 6.0, cy), 5.0, ui.input(|i| i.time), ai);
        p.text(Pos2::new(lx + 18.0, cy), Align2::LEFT_CENTER, format!("{} is checking…", agent), ty::label(), TEXT_DIM());
        ui.ctx().request_repaint();
    }
    if pill_button(ui, cr, "Clean", style(clean_lit), true).clicked() {
        answer_items(app, &[i], Verdict::Safe);
    }
    if pill_button(ui, kr, "Keep", style(keep_lit), true).clicked() {
        answer_items(app, &[i], Verdict::Keep);
    }
}

/// One safe item: checkbox, name, size.
/// The AI Check tip about this item, if any (matched by path).
fn row_tip(it: &Item, tips: &[aicheck::Rec]) -> Option<aicheck::Rec> {
    let key = it.path.display().to_string();
    tips.iter().find(|t| t.key == key).cloned()
}

/// The item with its reason replaced by the agent's tip ("Xcode: keep. You
/// built MyApp today"), so heights and drawing agree.
fn with_row_tip(it: &Item, tips: &[aicheck::Rec]) -> Item {
    let mut it = it.clone();
    if let Some(t) = row_tip(&it, tips) {
        let verdict = if t.action == "keep" { "keep" } else { "clean" };
        it.reason = if t.why.is_empty() { format!("{}: {verdict}", t.name) } else { format!("{}: {verdict}. {}", t.name, t.why) };
    }
    it
}

fn safe_row(app: &mut App, ui: &mut Ui, i: usize, rr: Rect) {
    let tips = app.ai.tips();
    let raw = app.clean.lock_ok().items[i].clone();
    let tip = row_tip(&raw, &tips);
    let it = with_row_tip(&raw, &tips);
    let p = ui.painter().clone();
    let rr = rr.shrink2(Vec2::new(4.0, 0.0));
    row_extras(ui, rr, &it, true);
    if app.ai.hovered.as_deref() == Some(it.path.display().to_string().as_str()) {
        p.rect_stroke(rr, R_SMALL, Stroke::new(2.0, with_alpha(tint::HEALTH(), 0.7)), StrokeKind::Inside);
    }
    let can = !it.hard_keep;
    let cb = Rect::from_center_size(Pos2::new(rr.left() + sp::M + 8.0, rr.center().y), Vec2::splat(16.0));
    if checkbox(ui, cb, ui.scope_id().with(("sel", &it.path)), it.selected, can).clicked() && can {
        let mut g = app.clean.lock_ok();
        g.items[i].selected = !g.items[i].selected;
    }
    let sz = p.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), ty::title(), TEXT());
    let mut right = sz.left() - sp::M;
    if it.permanent {
        right = chip(&p, Pos2::new(right, rr.center().y), Align2::RIGHT_CENTER, "Permanent", CHIP_RISK()).left() - sp::S;
    }
    let x = cb.right() + sp::M;
    let reason = row_text(&p, rr, x, right, &it, TEXT(), if tip.is_some() { tint::HEALTH() } else { TEXT_DIM() });
    match (tip, reason) {
        // the tip's action after its last line, like Undo
        (Some(t), Some(r)) => {
            let last_w = p.layout(it.reason.clone(), ty::label(), TEXT_DIM(), r.width().max(1.0) + 0.5).rows.last().map(|row| row.rect().width()).unwrap_or(0.0);
            let ly = r.bottom() - line_h(&p, &ty::label()) / 2.0;
            if link(ui, Pos2::new(x + last_w + sp::S, ly), Align2::LEFT_CENTER, &t.label, 12.0).clicked() {
                if t.action == "keep" {
                    answer_items(app, &[i], Verdict::Keep);
                } else {
                    let mut g = app.clean.lock_ok();
                    let it = &mut g.items[i];
                    it.selected = !it.hard_keep && !it.permanent;
                }
                app.ai.done(&t);
            }
        }
        _ => undo_link(app, ui, i, &it, x, reason),
    }
}

/// "Undo" after the reason of an answered question (after its last line).
fn undo_link(app: &mut App, ui: &mut Ui, i: usize, it: &Item, x: f32, reason: Option<Rect>) {
    if !undoable(it) {
        return;
    }
    let Some(r) = reason else { return };
    let p = ui.painter().clone();
    let last_w = p.layout(it.reason.clone(), ty::label(), TEXT_DIM(), r.width().max(1.0) + 0.5).rows.last().map(|row| row.rect().width()).unwrap_or(0.0);
    let ly = r.bottom() - line_h(&p, &ty::label()) / 2.0;
    if link(ui, Pos2::new(x + last_w + sp::S, ly), Align2::LEFT_CENTER, "Undo", 12.0).clicked() {
        undo_item(app, i);
    }
}

/// One kept item: a lock, the name, the size, dimmed.
fn kept_row(app: &mut App, ui: &mut Ui, i: usize, rr: Rect) {
    let it = app.clean.lock_ok().items[i].clone();
    let p = ui.painter().clone();
    let rr = rr.shrink2(Vec2::new(4.0, 0.0));
    row_extras(ui, rr, &it, true);
    icon(&p, Icon::Lock, Rect::from_center_size(Pos2::new(rr.left() + sp::M + 8.0, rr.center().y), Vec2::splat(14.0)), TEXT_FAINT());
    let sz = p.text(Pos2::new(rr.right() - sp::M, rr.center().y), Align2::RIGHT_CENTER, human_bytes(it.bytes), ty::body(), TEXT_DIM());
    let x = rr.left() + sp::M + 16.0 + sp::M;
    let reason = row_text(&p, rr, x, sz.left() - sp::M, &it, TEXT_DIM(), TEXT_FAINT());
    undo_link(app, ui, i, &it, x, reason);
}

fn confirm(app: &mut App, ui: &mut Ui, r: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    // block clicks to the page underneath
    let _ = ui.interact(r, ui.scope_id().with("confirm-scrim"), Sense::click());
    p.rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: WIN_R as u8, se: WIN_R as u8 }, Color32::from_black_alpha(115));
    let (n, bytes, perm, trashed, rebuilt, emptied, tools) = {
        let g = app.clean.lock_ok();
        let sel: Vec<&Item> = g.items.iter().filter(|i| i.selected).collect();
        let trashed = sel.iter().filter(|i| !i.permanent && !i.rebuilds()).count();
        let rebuilt = sel.iter().filter(|i| !i.permanent && i.rebuilds()).count();
        let emptied = sel.iter().any(|i| i.group == Group::Trash);
        let tools = sel.iter().any(|i| i.by_command());
        (sel.len(), sel.iter().map(|i| i.bytes).sum::<u64>(), sel.iter().any(|i| i.permanent), trashed, rebuilt, emptied, tools)
    };
    let mut body = if app.delete_now {
        "They'll be deleted right away. This can't be undone.".to_string()
    } else {
        let mut parts: Vec<String> = Vec::new();
        if rebuilt > 0 {
            parts.push("Caches and logs are deleted; apps rebuild them.".into());
        }
        if trashed > 0 {
            parts.push(if rebuilt == 0 { "They go to the Trash, so you can put anything back.".into() } else { format!("The other {trashed} go to the Trash.") });
        }
        if emptied {
            parts.push("Emptying the Trash is permanent.".into());
        }
        if tools {
            parts.push("Simulators and Homebrew leftovers are removed by their own tools, for good.".into());
        }
        parts.join(" ")
    };
    body.push_str(" Quit the apps involved first for the best result.");
    let bw = 440.0 - 44.0;
    let body_h = p.layout(body.clone(), font(13.0), TEXT_SOFT(), bw).size().y;
    let m = Rect::from_center_size(r.center(), Vec2::new(440.0, 22.0 + 22.0 + 14.0 + body_h + 18.0 + 30.0 + 22.0));
    let shadow = egui::epaint::Shadow { offset: [0, 20], blur: 60, spread: 0, color: Color32::from_black_alpha(128) };
    p.add(shadow.as_shape(m, CornerRadius::same(14)));
    p.rect_filled(m, 14.0, POPOVER());
    p.rect_stroke(m, 14.0, Stroke::new(1.0, white(31)), StrokeKind::Inside);
    p.text(m.min + Vec2::new(22.0, 22.0), Align2::LEFT_TOP, format!("Clean {} items ({})?", n, human_bytes(bytes)), bold(15.0), TEXT());
    paragraph(&p, m.min + Vec2::new(22.0, 58.0), &body, font(13.0), TEXT_SOFT(), bw);
    let danger = app.delete_now || perm;
    let ok_label = if app.delete_now { "Delete now" } else if trashed == 0 { "Clean" } else { "Move to Trash" };
    let okw = button_width(&p, ok_label, BTN_PRIMARY);
    let ok = Rect::from_min_size(Pos2::new(m.right() - 22.0 - okw, m.bottom() - 22.0 - 30.0), Vec2::new(okw, 30.0));
    let cw = button_width(&p, "Cancel", BTN_SECONDARY);
    let cancel = Rect::from_min_size(Pos2::new(ok.left() - 8.0 - cw, ok.top()), Vec2::new(cw, 30.0));
    if pill_button(ui, cancel, "Cancel", BTN_SECONDARY, true).clicked() {
        app.clean_confirm = false;
    }
    if pill_button(ui, ok, ok_label, if danger { BTN_DANGER } else { BTN_PRIMARY }, true).clicked() {
        app.clean_confirm = false;
        cleanup::start_clean(app.clean.clone(), ctx, app.delete_now);
    }
}

// ---------------------------------------------------------------- done

fn done(app: &mut App, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    let st = app.clean.lock_ok().clone();
    page_header(&p, r, "Cleanup", "Finished", TEXT_DIM());
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
    let left_h = 2.0 * sp::L + 44.0 + sp::L + 34.0 + sp::XS + 16.0 + sp::XL + 34.0 + if ok { 0.0 } else { 16.0 + sp::S + st.failed.len().min(4) as f32 * 18.0 + 20.0 + sp::L };
    let right_h = 2.0 * sp::L + 34.0 + sp::S + rows.len().max(1) as f32 * sp::ROW2_H;
    let h = if app.clean_log { body.height() } else { left_h.max(right_h).min(body.height()) };
    let left = Rect::from_min_size(body.min, Vec2::new(340.0f32.min(body.width() * 0.4), h));
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
    p.text(Pos2::new(tx, ly1), Align2::LEFT_CENTER, "Cleanup finished", ty::title(), TEXT());
    p.text(Pos2::new(tx, ly2), Align2::LEFT_CENTER, format!("{} of {} items", st.cleaned_count, total), ty::caption(), ty::caption_color());
    let mut y = li.top() + 44.0 + sp::L;
    let freed = st.cleaned_bytes.saturating_sub(st.trashed_bytes);
    let g = p.layout_no_wrap(human_bytes(if freed > 0 { freed } else { st.cleaned_bytes }), ty::hero(), TEXT());
    let gw = g.size().x;
    p.galley(Pos2::new(li.left(), y), g, TEXT());
    let what = if st.trashed_bytes == 0 {
        "freed".to_string()
    } else if freed == 0 {
        "to the Trash".to_string()
    } else {
        format!("freed · {} to the Trash", human_bytes(st.trashed_bytes))
    };
    p.text(Pos2::new(li.left() + gw + 6.0, y + 26.0), Align2::LEFT_BOTTOM, what, ty::body(), ty::caption_color());
    y += 34.0 + sp::XS;
    y += sp::L;
    if !ok {
        let n = st.failed.len();
        p.text(Pos2::new(li.left(), y), Align2::LEFT_TOP, format!("{n} item{} couldn't be removed", if n == 1 { "" } else { "s" }), ty::title(), AMBER());
        y += 16.0 + sp::S;
        for (name, err) in st.failed.iter().take(4) {
            text_fit(&p, Pos2::new(li.left(), y), Align2::LEFT_TOP, &format!("{name}: {err}"), mono(11.0), TEXT_SOFT(), li.width());
            y += 18.0;
        }
        text_fit(&p, Pos2::new(li.left(), y + sp::XS), Align2::LEFT_TOP, "Quit the apps and scan again", ty::caption(), ty::caption_color(), li.width());
    }
    let db = Rect::from_min_size(Pos2::new(li.left(), li.bottom() - 34.0), Vec2::new(li.width(), 34.0));
    if pill_button(ui, db, "Done", BTN_PRIMARY, true).clicked() {
        app.clean.lock_ok().phase = Phase::Intro;
        app.clean_log = false;
    }

    // right: what was cleaned, or the log
    let ri = right.shrink(sp::L);
    let log_label = if app.clean_log { "Hide log" } else { "Show log" };
    let lw = button_width(&p, log_label, BTN_SECONDARY);
    let lb = Rect::from_min_size(Pos2::new(ri.right() - lw, ri.top() - 4.0), Vec2::new(lw, 26.0));
    if pill_button(ui, lb, log_label, BTN_SECONDARY, true).clicked() {
        app.clean_log = !app.clean_log;
    }
    let list = Rect::from_min_max(Pos2::new(ri.left(), ri.top() + 16.0 + sp::M), ri.max);
    if app.clean_log {
        p.text(ri.left_top(), Align2::LEFT_TOP, "Cleanup log", ty::title(), TEXT());
        let mut child = ui.new_child(UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(list);
        ScrollArea::vertical().id_salt("log").auto_shrink([false, false]).show(&mut child, |ui| {
            for l in &st.log {
                let col = if l.starts_with("Couldn't") { AMBER() } else { TEXT_DIM() };
                ui.label(egui::RichText::new(l.replacen(&home_str(), "~", 1)).font(glass::mono(11.0)).color(col));
            }
        });
        return;
    }
    p.text(ri.left_top(), Align2::LEFT_TOP, "What was cleaned", ty::title(), TEXT());
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
        text_fit(&p, Pos2::new(x, ly1), Align2::LEFT_CENTER, g.title(), ty::body(), TEXT(), vr.left() - x - sp::S);
        text_fit(&p, Pos2::new(x, ly2), Align2::LEFT_CENTER, &format!("{n} item{}", if *n == 1 { "" } else { "s" }), ty::caption(), ty::caption_color(), vr.left() - x - sp::S);
    }
    if rows.is_empty() {
        text_fit(&p, Pos2::new(list.left(), list.top() + sp::M), Align2::LEFT_TOP, "Nothing selected", ty::body(), TEXT_DIM(), list.width());
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
    st.log = sel.iter().map(|i| format!("Moved to Trash: {}", i.path.display())).collect();
    st
}

/// What AI Check gets: the biggest things found, with what the rules or the
/// path check said about each.
fn check_ask(clean: &cleanup::Shared) -> aicheck::Ask {
    let g = clean.lock_ok();
    let mut idx: Vec<usize> = (0..g.items.len()).filter(|&i| g.items[i].verdict != Verdict::Keep).collect();
    // what waits on the user first (all of it is sent), then by size
    idx.sort_by_key(|&i| (g.items[i].verdict != Verdict::Ask, std::cmp::Reverse(g.items[i].bytes)));
    let answer: Vec<String> = idx.iter().filter(|&&i| g.items[i].verdict == Verdict::Ask && g.items[i].by != Who::You).take(aicheck::MAX_ANSWERS).map(|&i| g.items[i].path.display().to_string()).collect();
    let items = idx
        .into_iter()
        .take(40)
        .map(|i| {
            let it = &g.items[i];
            let now = match it.verdict {
                Verdict::Safe => "marked safe",
                Verdict::Ask => "needs the user's OK",
                Verdict::Keep => "kept",
            };
            let why = if it.reason.is_empty() { String::new() } else { format!(" ({})", it.reason) };
            aicheck::Item {
                key: it.path.display().to_string(),
                line: format!("{} · {} · {} · {} · untouched {} days · {}{}{}", it.group.title(), it.title, short_path(&it.path), human_bytes(it.bytes), it.age_days.round(), now, why, if it.selected { " · ticked for cleaning" } else { "" }),
                icon: None,
                name: it.title.clone(),
            }
        })
        .collect();
    aicheck::Ask {
        task: "This is Cleanup's results: caches, logs and leftovers found on this Mac. Some items need the user's OK: say for each whether to clean it (select) or keep it. Then point out anything ticked for cleaning that the user would likely miss (documents, settings, data that cannot be downloaded again), or a big clearly safe win.".into(),
        actions: vec![
            ("select", "clean it: tick it for cleaning (the user still presses Clean and confirms)", "Clean"),
            ("keep", "untick it and remember it as kept", "Keep"),
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
}
