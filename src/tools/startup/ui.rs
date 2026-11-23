//! Startup on screen: the window page (suggestions, Undo, the list by
//! where things come from) and the panel widget at 1:1, 1:2 and 2:4.

use super::extra::{BtmItem, CronJob, SysExt};
use super::items::{Job, LoginApp, Scope, Why};
use super::sign::Flag;
use super::{plural, trust_key, Filter, Gone, Startup};
use crate::core::tool::{Cx, Size};
use crate::kit::aicheck::{self, ui::Event};
use crate::kit::glass::*;
use crate::kit::menu;
use crate::kit::{macos, tile};
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2};
use std::path::{Path, PathBuf};

const ROW_H: f32 = sp::ROW2_H;
/// Rows of a list touch: one card per list, hairlines between (S4).
const ROW_GAP: f32 = 0.0;
const HEAD_H: f32 = sp::SECTION_H;
/// The suggestion / Undo card above the list.
const TOP_CARD_H: f32 = 64.0;
/// Icons are loaded a few per frame so a long list doesn't stall.
const ICONS_PER_FRAME: usize = 4;
/// Faint red behind suggested rows and the suggestion card.
fn red_wash(a: f32) -> Color32 {
    with_alpha(tint::STARTUP(), a)
}
#[allow(non_snake_case)]
fn SUGGEST_TEXT() -> Color32 { AMBER() }
#[allow(non_snake_case)]
fn ORPHAN_TEXT() -> Color32 { AMBER() }


/// The owning app's icon (macOS), else its letter tile.
#[allow(clippy::too_many_arguments)]
fn app_icon(s: &mut Startup, ctx: &egui::Context, p: &egui::Painter, r: Rect, path: Option<&Path>, name: &str, dim: bool, budget: &mut usize) {
    let a = if dim { 0.5 } else { 1.0 };
    if let Some(path) = path {
        // an app in Downloads etc. isn't read (nor cached) before Full Disk Access
        if !s.icons.contains_key(path) && *budget > 0 && !s.demo && !crate::kit::fs::behind_folder_prompt(path) {
            *budget -= 1;
            let tex = macos::app_icon(path, 64).map(|(n, rgba)| ctx.load_texture(format!("startup-icon:{}", path.display()), egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba), egui::TextureOptions::LINEAR));
            s.icons.insert(path.to_path_buf(), tex);
            ctx.request_repaint();
        }
        if let Some(Some(t)) = s.icons.get(path) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), with_alpha(Color32::WHITE, a));
            return;
        }
    }
    letter_tile(p, r, name, a);
}

fn flag_color(f: Flag) -> Color32 {
    if f.strong() { RED() } else { AMBER() }
}

/// "3h ago", "2 days ago".
fn ago(at: u64) -> String {
    let s = super::now_secs().saturating_sub(at);
    match s {
        0..=89 => "just now".into(),
        90..=5399 => format!("{} min ago", (s + 30) / 60),
        5400..=129_599 => format!("{}h ago", (s + 1800) / 3600),
        _ => format!("{} days ago", (s + 43_200) / 86_400),
    }
}

fn why_color(w: Why) -> Color32 {
    if w == Why::Removed { ORPHAN_TEXT() } else { SUGGEST_TEXT() }
}

/// The outermost app around a program.
fn owner_of(p: &Path) -> Option<&Path> {
    p.ancestors().filter(|a| a.extension().is_some_and(|e| e == "app")).last()
}

/// An app in /Applications that the Uninstaller can show.
fn uninstallable(owner: &Option<PathBuf>) -> Option<&PathBuf> {
    owner.as_ref().filter(|o| {
        let home_apps = crate::kit::fs::home().join("Applications");
        o.starts_with("/Applications") || o.starts_with(&home_apps)
    })
}

// ---------------------------------------------------------------- page

#[derive(Clone, Copy, PartialEq)]
enum HeadLink {
    None,
    /// "Open in System Settings"
    Settings,
    /// "Show all": read the Background Items list (password)
    ShowAll,
}

enum Row {
    Head(&'static str, HeadLink),
    App(LoginApp),
    Job(Job),
    Cron(CronJob),
    Ext(SysExt),
    Btm(BtmItem),
    Gone(Gone),
    Note(String),
    /// System Events access is missing: the login apps can't be read.
    Allow,
}

pub fn page(s: &mut Startup, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    trash_loader(s, ui, r);
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    s.load_apps();
    let events_ok = s.events_allowed();
    let (loaded, jobs, apps, apps_loaded, apps_err, new_labels, changed_labels, undo, busy, notify) = {
        let g = s.state();
        (g.jobs_loaded, g.jobs.clone(), g.apps.clone(), g.apps_loaded, g.apps_error.clone(), g.new_labels.clone(), g.changed_labels.clone(), g.undoable(), g.busy.clone(), g.settings.notify_new)
    };
    let (cron, new_cron, sysext, btm, btm_busy, gone, trusted) = {
        let g = s.state();
        (g.cron.clone(), g.new_cron.clone(), g.sysext.clone(), g.btm.clone(), g.btm_busy, g.settings.gone.clone(), g.settings.trusted.clone())
    };
    let is_trusted = |j: &Job| trusted.get(&trust_key(j)).is_some_and(|fp| *fp == j.fp);
    let flags_of = |j: &Job| if is_trusted(j) { Vec::new() } else { j.flags() };
    let is_suggested = |j: &Job| j.suggested() && !is_trusted(j);
    let total = jobs.len() + apps.len() + cron.len() + sysext.len();
    let running = jobs.iter().filter(|j| j.pid.is_some()).count();
    let suggested: Vec<&Job> = jobs.iter().filter(|j| is_suggested(j)).collect();
    let flagged = jobs.iter().filter(|j| !flags_of(j).is_empty()).count() + cron.iter().filter(|c| !c.flags().is_empty()).count();
    if flagged == 0 && s.filter == Filter::Flagged && loaded {
        s.filter = Filter::All;
    }
    let off = jobs.iter().filter(|j| !j.on).count() + apps.iter().filter(|a| !a.on).count() + cron.iter().filter(|c| !c.on).count();
    let sub = if !loaded {
        "Reading what starts with your Mac…".to_string()
    } else {
        format!("{} · {running} running", plural(total, "item"))
    };
    // AI Check: a chip after the title
    let ai_n = jobs.iter().filter(|j| j.on).count() + apps.iter().filter(|a| a.on).count();
    let ai_data = loaded && ai_n > 0;
    let cw = aicheck::ui::chip_width(&s.ai, ui, ai_data);
    // the status ends where search starts (tabs, then a 190 pt field)
    let n_tabs = if flagged > 0 { 4.0 } else { 3.0 };
    let sub_right = r.right() - sp::PAGE_EDGE - 100.0 * n_tabs - 12.0 - 190.0 - 12.0;
    let (btns, chip_at) = page_header_room(&p, r, "Startup", &sub, TEXT_DIM(), cw, sub_right);

    // filter + search on the right of the header
    let mut tabs = vec![(Filter::All, format!("All {total}")), (Filter::Suggested, format!("Suggested {}", suggested.len()))];
    if flagged > 0 {
        tabs.push((Filter::Flagged, format!("Flagged {flagged}")));
    }
    tabs.push((Filter::Off, format!("Off {off}")));
    let labels: Vec<&str> = tabs.iter().map(|(_, x)| x.as_str()).collect();
    let tr = Rect::from_min_max(Pos2::new(btns.right() - 100.0 * tabs.len() as f32, btns.top()), btns.max);
    let cur = tabs.iter().position(|(f, _)| *f == s.filter).unwrap_or(0);
    if let Some(i) = segmented(ui, tr, ui.scope_id().with("su-filter"), &labels, cur) {
        s.filter = tabs[i].0;
    }
    let sr = Rect::from_min_max(Pos2::new(tr.left() - sp::M - 180.0, tr.top()), Pos2::new(tr.left() - sp::M, tr.bottom()));
    let mut q = std::mem::take(&mut s.search);
    search_field(ui, sr, &mut q, "Search", false);
    s.search = q;

    let body = Rect::from_min_max(Pos2::new(r.left() + 24.0, r.top() + PAGE_HEADER_H + 1.0), Pos2::new(r.right() - 24.0, r.bottom()));
    if loaded {
        if ai_n > 0 {
            // again only when something new starts at login, not after a turn off or remove
            let on = jobs.iter().filter(|j| j.on).map(|j| j.label.clone()).chain(apps.iter().filter(|a| a.on).map(|a| format!("app:{}", a.name)));
            s.ai.auto_new(&ctx, on, || startup_ask(&jobs, &apps));
        }
        match aicheck::ui::chip(&mut s.ai, ui, chip_at, ai_data) {
            Some(Event::Start) => {
                let ask = startup_ask(&jobs, &apps);
                s.ai.start(&ctx, ask);
            }
            Some(Event::Act(rec)) => {
                if let Some(name) = rec.key.strip_prefix("app:") {
                    if let Some(a) = apps.iter().find(|a| a.name == name && a.on) {
                        s.toggle_app(a);
                    }
                } else if let Some(j) = jobs.iter().find(|j| j.label == rec.key) {
                    if rec.action == "remove" && j.orphan && !j.bundled {
                        s.trash(j);
                    } else if j.on {
                        s.toggle_job(j);
                    }
                }
                s.ai.done(&rec);
            }
            // the row may be behind a tab or the search: show the whole list
            Some(Event::Show(_)) => {
                s.filter = Filter::All;
                s.search.clear();
            }
            None => {}
        }
    }
    if !loaded {
        spinner(&p, Pos2::new(body.left() + 8.0, body.top() + 30.0), 7.0, ctx.input(|i| i.time), ACCENT());
        p.text(Pos2::new(body.left() + 24.0, body.top() + 30.0), Align2::LEFT_CENTER, "Reading what starts with your Mac…", font(13.0), TEXT_DIM());
        crate::kit::glass::poll(&ctx);
        return;
    }

    // what passes the filter and the search
    let needle = s.search.trim().to_lowercase();
    let hit = |name: &str, label: &str| needle.is_empty() || name.to_lowercase().contains(&needle) || label.to_lowercase().contains(&needle);
    let job_ok = |j: &Job| {
        (hit(&j.name, &j.label) || hit(&j.command(), ""))
            && match s.filter {
                Filter::All => true,
                Filter::Suggested => is_suggested(j),
                Filter::Flagged => !flags_of(j).is_empty(),
                Filter::Off => !j.on,
            }
    };
    let app_ok = |a: &LoginApp| {
        hit(&a.name, &a.path.to_string_lossy())
            && match s.filter {
                Filter::All => true,
                Filter::Suggested | Filter::Flagged => false,
                Filter::Off => !a.on,
            }
    };
    let cron_ok = |c: &CronJob| {
        hit(&c.name(), &c.command)
            && match s.filter {
                Filter::All => true,
                Filter::Suggested => false,
                Filter::Flagged => !c.flags().is_empty(),
                Filter::Off => !c.on,
            }
    };
    let all = s.filter == Filter::All;
    let mut rows = Vec::new();
    if all || s.filter == Filter::Off {
        let a: Vec<&LoginApp> = apps.iter().filter(|a| app_ok(a)).collect();
        if !a.is_empty() || (all && needle.is_empty()) {
            rows.push(Row::Head("Open at login", HeadLink::Settings));
            match (&apps_err, apps_loaded) {
                _ if !events_ok => rows.push(Row::Allow),
                (Some(_), _) => rows.push(Row::Allow),
                (None, false) => rows.push(Row::Note("Reading…".into())),
                (None, true) if a.is_empty() => rows.push(Row::Note("None".into())),
                _ => rows.extend(a.into_iter().cloned().map(Row::App)),
            }
        }
    }
    for (scopes, title) in [(&[Scope::User, Scope::AllUsers][..], "Background agents"), (&[Scope::System][..], "System daemons")] {
        let js: Vec<&Job> = jobs.iter().filter(|j| scopes.contains(&j.scope) && job_ok(j)).collect();
        if js.is_empty() {
            continue;
        }
        rows.push(Row::Head(title, HeadLink::None));
        rows.extend(js.into_iter().cloned().map(Row::Job));
    }
    let cs: Vec<&CronJob> = cron.iter().filter(|c| cron_ok(c)).collect();
    if !cs.is_empty() {
        rows.push(Row::Head("Cron jobs", HeadLink::None));
        rows.extend(cs.into_iter().cloned().map(Row::Cron));
    }
    if all {
        let xs: Vec<&SysExt> = sysext.iter().filter(|x| hit(&x.name, &x.bundle_id)).collect();
        if !xs.is_empty() {
            rows.push(Row::Head("System extensions", HeadLink::Settings));
            rows.extend(xs.into_iter().cloned().map(Row::Ext));
        }
        // the full list from macOS, minus what's already above
        let labels: Vec<&str> = jobs.iter().map(|j| j.label.as_str()).collect();
        let programs: Vec<PathBuf> = jobs.iter().filter_map(|j| j.program.clone()).collect();
        let app_paths: Vec<PathBuf> = apps.iter().map(|a| a.path.clone()).collect();
        match &btm {
            None if needle.is_empty() => {
                rows.push(Row::Head("Background items", if btm_busy { HeadLink::None } else { HeadLink::ShowAll }));
                if btm_busy {
                    rows.push(Row::Note("Reading…".into()));
                }
            }
            None => {}
            Some(list) => {
                let extra: Vec<BtmItem> = super::extra::btm_extra(list, macos::uid(), &labels, &programs, &app_paths).into_iter().filter(|b| hit(&b.name, &b.developer)).collect();
                if !extra.is_empty() || needle.is_empty() {
                    rows.push(Row::Head("Background items", HeadLink::Settings));
                    if extra.is_empty() {
                        rows.push(Row::Note("Nothing else".into()));
                    }
                    rows.extend(extra.into_iter().map(Row::Btm));
                }
            }
        }
        let gs: Vec<&Gone> = gone.iter().rev().filter(|g| hit(&g.name, &g.label)).collect();
        if !gs.is_empty() {
            rows.push(Row::Head("Removed", HeadLink::None));
            rows.extend(gs.into_iter().cloned().map(Row::Gone));
        }
    }
    if rows.is_empty() {
        rows.push(Row::Note(match s.filter {
            Filter::Suggested => "Nothing to turn off".into(),
            Filter::Flagged => "Nothing looks wrong".into(),
            Filter::Off => "Nothing turned off".into(),
            Filter::All => "No matches".into(),
        }));
    }

    let top_h = if busy.is_some() || undo.is_some() || (!suggested.is_empty() && matches!(s.filter, Filter::All | Filter::Suggested)) { TOP_CARD_H } else { 0.0 };
    let list_h: f32 = rows.iter().map(|r| match r {
        Row::Head(..) => HEAD_H,
        Row::Note(_) => 40.0,
        Row::Allow => 62.0,
        _ => ROW_H + ROW_GAP,
    }).sum::<f32>()
        + 56.0;
    let area = Rect::from_min_max(Pos2::new(body.left(), body.top() + 12.0), Pos2::new(body.right(), body.bottom() - 12.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    let mut flip_app: Option<LoginApp> = None;
    let mut flip_job: Option<Job> = None;
    let mut flip_cron: Option<CronJob> = None;
    let mut trust: Option<(Job, bool)> = None;
    let mut vt: Option<Job> = None;
    let mut show_btm = false;
    let mut trash: Option<Job> = None;
    let mut turn_off_all = false;
    let mut do_undo = false;
    let mut retry = false;
    let mut set_notify: Option<bool> = None;
    let mut budget = ICONS_PER_FRAME;
    ScrollArea::vertical().id_salt("su-list").auto_shrink([false, false]).show(&mut child, |ui| {
        let (a, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), top_h + list_h), Sense::hover());
        let p = ui.painter().clone();
        let mut y = a.top();

        // the card on top: working / Undo / suggestion
        if top_h > 0.0 {
            let cr = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), TOP_CARD_H));
            let (y1, y2) = two_lines_y(&p, cr.center().y, &bold(13.0), &font(12.0));
            if let Some(line) = &busy {
                glass_card(&p, cr, R_CARD, 0.0, false);
                spinner(&p, Pos2::new(cr.left() + 24.0, cr.center().y), 7.0, ctx.input(|i| i.time), ACCENT());
                p.text(Pos2::new(cr.left() + 42.0, cr.center().y), Align2::LEFT_CENTER, line, font(13.0), TEXT_SOFT());
            } else if let Some(n) = undo {
                glass_card(&p, cr, R_CARD, 0.0, false);
                let c = Pos2::new(cr.left() + 30.0, cr.center().y);
                p.circle_filled(c, 17.0, with_alpha(GREEN(), 0.16));
                icon(&p, Icon::Check, Rect::from_center_size(c, Vec2::splat(18.0)), GREEN());
                let br = Rect::from_min_size(Pos2::new(cr.right() - 14.0 - 76.0, cr.center().y - sp::CONTROL_H / 2.0), Vec2::new(76.0, sp::CONTROL_H));
                if pill_button(ui, br, "Undo", BTN_SECONDARY, true).clicked() {
                    do_undo = true;
                }
                p.text(Pos2::new(cr.left() + 60.0, y1), Align2::LEFT_CENTER, format!("{} turned off", plural(n, "item")), bold(13.0), TEXT());
                text_fit(&p, Pos2::new(cr.left() + 60.0, y2), Align2::LEFT_CENTER, "The apps still open", font(12.0), TEXT_DIM(), br.left() - cr.left() - 70.0);
            } else {
                glass_card(&p, cr, R_CARD, 0.0, false);
                tool_chip(&p, Rect::from_center_size(Pos2::new(cr.left() + 30.0, cr.center().y), Vec2::splat(34.0)), Icon::Power, tint::STARTUP());
                let label = format!("Turn off {}", suggested.len());
                let bw = button_width(&p, &label, BTN_PRIMARY);
                let br = Rect::from_min_size(Pos2::new(cr.right() - 14.0 - bw, cr.center().y - sp::CONTROL_H / 2.0), Vec2::new(bw, sp::CONTROL_H));
                if pill_button(ui, br, &label, BTN_PRIMARY, true).clicked() {
                    turn_off_all = true;
                }
                let title = if suggested.len() == 1 { "1 item you likely don't need".to_string() } else { format!("{} items you likely don't need", suggested.len()) };
                p.text(Pos2::new(cr.left() + 60.0, y1), Align2::LEFT_CENTER, title, bold(13.0), TEXT());
                text_fit(&p, Pos2::new(cr.left() + 60.0, y2), Align2::LEFT_CENTER, &why_line(&suggested), font(12.0), TEXT_SOFT(), br.left() - cr.left() - 70.0);
            }
            y += top_h;
        }

        // one card behind each run of item rows (S4)
        {
            let mut yy = y;
            let mut run: Option<f32> = None;
            for row in rows.iter().map(Some).chain(std::iter::once(None)) {
                let item = matches!(row, Some(Row::App(_) | Row::Job(_) | Row::Cron(_) | Row::Ext(_) | Row::Btm(_) | Row::Gone(_)));
                match (item, run) {
                    (true, None) => run = Some(yy),
                    (false, Some(top)) => {
                        glass_card(&p, Rect::from_min_max(Pos2::new(a.left(), top), Pos2::new(a.right(), yy)), R_CARD, 0.0, false);
                        run = None;
                    }
                    _ => {}
                }
                yy += match row {
                    Some(Row::Head(..)) => HEAD_H,
                    Some(Row::Note(_)) => 40.0,
                    Some(Row::Allow) => 62.0,
                    Some(_) => ROW_H + ROW_GAP,
                    None => 0.0,
                };
            }
        }
        let mut prev_item = false;
        for row in &rows {
            let item = matches!(row, Row::App(_) | Row::Job(_) | Row::Cron(_) | Row::Ext(_) | Row::Btm(_) | Row::Gone(_));
            let first = !prev_item;
            prev_item = item;
            match row {
                Row::Head(t, l) => {
                    let cy = section_title(&p, a.left() + 4.0, y, t);
                    let at = Pos2::new(a.right() - 4.0, cy);
                    match l {
                        HeadLink::Settings => {
                            if link(ui, at, Align2::RIGHT_CENTER, "Open in System Settings", 12.0).clicked() {
                                super::items::open_settings();
                            }
                        }
                        HeadLink::ShowAll => {
                            if link(ui, at, Align2::RIGHT_CENTER, "Show all", 12.0).on_hover_text("Asks for your password").clicked() {
                                show_btm = true;
                            }
                        }
                        HeadLink::None => {}
                    }
                    y += HEAD_H;
                }
                Row::Cron(c) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    let fl = c.flags();
                    row_line(ui, &p, r, first, fl.iter().any(|f| f.strong()));
                    if fl.iter().any(|f| f.strong()) {
                        p.rect_filled(crate::kit::glass::lit_rect(r), crate::kit::glass::LIT_R, red_wash(0.05));
                    }
                    let cols = columns(&p, r, true);
                    let name = c.name();
                    app_icon(s, &ctx, &p, cols.icon, None, &name, !c.on, &mut budget);
                    let mut tags: Vec<(&str, Color32)> = fl.iter().map(|f| (f.label(), flag_color(*f))).collect();
                    if new_cron.contains(&c.line) {
                        tags.push(("new", ACCENT()));
                    }
                    name_line(&p, &cols, &name, if c.on { TEXT() } else { TEXT_DIM() }, &tags);
                    let dr = detail_line(&p, &cols, &c.command);
                    ui.interact(dr, ui.scope_id().with(("su-cron-tip", &c.line)), Sense::hover()).on_hover_text(&c.line);
                    when_cell(&p, &cols, r.center().y, Icon::Clock, &c.when);
                    if !c.on {
                        state_cell(&p, cols.state, r.center().y, Some("off"), false);
                    }
                    let sw = cols.sw;
                    let mr = icon_dots(ui, cols.more, ui.scope_id().with(("su-cron-more", &c.line)));
                    menu::popup(&mr).show(|ui| {
                        menu::note(ui, &c.line, true);
                        menu::separator(ui);
                        if menu::item(ui, "Copy command").clicked() {
                            ui.ctx().copy_text(c.command.clone());
                        }
                    });
                    if switch(ui, sw, ui.scope_id().with(("su-cron", &c.line)), c.on, busy.is_none()).on_hover_text(if c.on { "Turn off" } else { "Turn on" }).clicked() {
                        flip_cron = Some(c.clone());
                    }
                    y += ROW_H + ROW_GAP;
                }
                Row::Ext(x) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    row_line(ui, &p, r, first, false);
                    let cols = columns(&p, r, false);
                    app_icon(s, &ctx, &p, cols.icon, None, &x.name, !x.on, &mut budget);
                    name_line(&p, &cols, &x.name, if x.on { TEXT() } else { TEXT_DIM() }, &[]);
                    detail_line(&p, &cols, &format!("{} · {}", x.bundle_id, x.team));
                    when_cell(&p, &cols, r.center().y, Icon::Shield, &x.kind);
                    let state = if x.state.contains("waiting") { Some("waiting") } else if !x.on { Some("off") } else if x.active { None } else { Some("idle") };
                    state_cell(&p, cols.state, r.center().y, state, x.on && x.active && state.is_none());
                    let mr = icon_dots(ui, cols.more, ui.scope_id().with(("su-ext-more", &x.bundle_id)));
                    menu::popup(&mr).show(|ui| {
                        menu::note(ui, &x.state, false);
                        menu::separator(ui);
                        if menu::item(ui, "Open in System Settings").clicked() {
                            super::items::open_settings();
                        }
                        if menu::item(ui, "Copy bundle ID").clicked() {
                            ui.ctx().copy_text(x.bundle_id.clone());
                        }
                    });
                    y += ROW_H + ROW_GAP;
                }
                Row::Btm(b) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    row_line(ui, &p, r, first, false);
                    let cols = columns(&p, r, false);
                    let icon_path = b.path.as_deref().filter(|p| p.extension().is_some_and(|e| e == "app")).or(b.program.as_deref().and_then(|p| owner_of(p)));
                    app_icon(s, &ctx, &p, cols.icon, icon_path, &b.name, !b.on, &mut budget);
                    name_line(&p, &cols, &b.name, if b.on { TEXT() } else { TEXT_DIM() }, &[]);
                    let detail = if b.developer.is_empty() { b.label().to_string() } else { format!("{} · {}", b.developer, b.label()) };
                    detail_line(&p, &cols, &detail);
                    when_cell(&p, &cols, r.center().y, Icon::Power, &b.kind);
                    if !b.on {
                        state_cell(&p, cols.state, r.center().y, Some("off"), false);
                    }
                    let mr = icon_dots(ui, cols.more, ui.scope_id().with(("su-btm-more", &b.identifier)));
                    menu::popup(&mr).show(|ui| {
                        if menu::item(ui, "Open in System Settings").clicked() {
                            super::items::open_settings();
                        }
                        if let Some(f) = b.program.as_ref().or(b.path.as_ref()) {
                            if menu::item(ui, "Show in Finder").clicked() {
                                macos::reveal(f);
                            }
                        }
                    });
                    y += ROW_H + ROW_GAP;
                }
                Row::Gone(g) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    row_line(ui, &p, r, first, false);
                    let cols = columns(&p, r, false);
                    app_icon(s, &ctx, &p, cols.icon, None, &g.name, true, &mut budget);
                    name_line(&p, &cols, &g.name, TEXT_DIM(), &[]);
                    detail_line(&p, &cols, &g.label);
                    when_cell(&p, &cols, r.center().y, Icon::Clock, &ago(g.at));
                    state_cell(&p, cols.state, r.center().y, Some("removed"), false);
                    y += ROW_H + ROW_GAP;
                }
                Row::Note(t) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), 36.0));
                    glass_card(&p, r, R_CARD, 0.0, false);
                    text_fit(&p, Pos2::new(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, t, font(12.0), TEXT_DIM(), r.width() - 28.0);
                    y += 40.0;
                }
                Row::Allow => {
                    use crate::kit::perms::{Perm, Status};
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), 58.0));
                    glass_card(&p, r, R_CARD, 0.0, false);
                    icon(&p, Icon::Script, Rect::from_center_size(Pos2::new(r.left() + 24.0, r.center().y), Vec2::splat(18.0)), TEXT_SOFT());
                    let denied = s.events.0 == Status::Denied;
                    let label = if denied { "Open Settings" } else { "Allow access" };
                    let bw = button_width(&p, label, BTN_PRIMARY);
                    let br = Rect::from_min_size(Pos2::new(r.right() - 12.0 - bw, r.center().y - 14.0), Vec2::new(bw, 28.0));
                    let tw = br.left() - r.left() - 44.0 - 12.0;
                    let (ly1, ly2) = two_lines_y(&p, r.center().y, &bold(13.0), &font(12.0));
                    text_fit(&p, Pos2::new(r.left() + 44.0, ly1), Align2::LEFT_CENTER, "Can't see the apps that open at login", bold(13.0), TEXT(), tw);
                    let why = if denied {
                        "Turn on System Events for Cmd+Meow in Privacy & Security › Automation"
                    } else {
                        "macOS lists them through System Events; allow it once to see and switch them"
                    };
                    text_fit(&p, Pos2::new(r.left() + 44.0, ly2), Align2::LEFT_CENTER, why, font(12.0), TEXT_FAINT(), tw);
                    if pill_button(ui, br, label, BTN_PRIMARY, true).clicked() {
                        Perm::SystemEvents.ask();
                        retry = true;
                    }
                    if !events_ok {
                        // repaint now and then so the row goes once access lands
                        ctx.request_repaint_after(std::time::Duration::from_secs(1));
                    }
                    y += 62.0;
                }
                Row::App(la) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    let key = format!("app:{}", la.name);
                    if s.ai.take_scroll(&key) {
                        ui.scroll_to_rect(r, Some(egui::Align::Center));
                    }
                    let ring = s.ai.ring(&ctx, &key);
                    row_line(ui, &p, r, first, ring > 0.0);
                    reveal::row(&p, r, ring);
                    let cols = columns(&p, r, true);
                    app_icon(s, &ctx, &p, cols.icon, Some(&la.path), &la.name, !la.on, &mut budget);
                    name_line(&p, &cols, &la.name, if la.on { TEXT() } else { TEXT_DIM() }, &[]);
                    detail_line(&p, &cols, if la.hidden { "Opens hidden" } else { "Opens its window" });
                    when_cell(&p, &cols, r.center().y, Icon::Power, "at login");
                    if !la.on {
                        state_cell(&p, cols.state, r.center().y, Some("off"), false);
                    }
                    let sw = cols.sw;
                    if switch(ui, sw, ui.scope_id().with(("su-app", &la.path)), la.on, busy.is_none()).on_hover_text(if la.on { "Don't open at login" } else { "Open at login" }).clicked() {
                        flip_app = Some(la.clone());
                    }
                    y += ROW_H + ROW_GAP;
                }
                Row::Job(j) => {
                    let r = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), ROW_H));
                    let trusted_now = is_trusted(j);
                    let why = j.why().filter(|_| j.on && !trusted_now);
                    let fl = flags_of(j);
                    if s.ai.take_scroll(&j.label) {
                        ui.scroll_to_rect(r, Some(egui::Align::Center));
                    }
                    let ring = s.ai.ring(&ctx, &j.label);
                    let washed = why.is_some() || fl.iter().any(|f| f.strong());
                    row_line(ui, &p, r, first, ring > 0.0 || washed);
                    if washed {
                        p.rect_filled(crate::kit::glass::lit_rect(r), crate::kit::glass::LIT_R, red_wash(0.05));
                    }
                    reveal::row(&p, r, ring);
                    let mut cols = columns(&p, r, true);
                    app_icon(s, &ctx, &p, cols.icon, j.owner.as_deref(), &j.name, !j.on, &mut budget);
                    let (sw, more) = (cols.sw, cols.more);
                    let mut right_btn = more.left() - sp::S;
                    if j.orphan {
                        let rb = Rect::from_min_max(Pos2::new(right_btn - 74.0, r.center().y - 12.0), Pos2::new(right_btn, r.center().y + 12.0));
                        if pill_button(ui, rb, "Remove", BTN_SECONDARY, busy.is_none()).on_hover_text("Stop it and move its file to the Trash").clicked() {
                            trash = Some(j.clone());
                        }
                        right_btn = rb.left() - sp::COL_GAP;
                        // the button pushes the state and schedule columns left;
                        // the schedule goes when the name would get too little
                        if cols.state + STATE_W > right_btn {
                            cols.state = right_btn - STATE_W;
                            let when = cols.state - sp::COL_GAP - WHEN_W;
                            cols.when = (when - sp::COL_GAP - cols.text_x >= 160.0).then_some(when);
                            cols.name_max = cols.when.unwrap_or(cols.state) - sp::COL_GAP - cols.text_x;
                        }
                    }
                    let mut tags: Vec<(&str, Color32)> = fl.iter().map(|f| (f.label(), flag_color(*f))).collect();
                    if let Some(w) = why {
                        tags.push((if w == Why::Removed { "app removed" } else { "suggested" }, why_color(w)));
                    } else if j.orphan {
                        tags.push(("app removed", ORPHAN_TEXT()));
                    }
                    if !trusted_now && changed_labels.contains(&j.label) {
                        tags.push(("changed", AMBER()));
                    } else if !trusted_now && new_labels.contains(&j.label) {
                        tags.push(("new", ACCENT()));
                    }
                    if j.admin() {
                        tags.push(("admin", TEXT_DIM()));
                    }
                    name_line(&p, &cols, &j.name, if j.on { TEXT() } else { TEXT_DIM() }, &tags);
                    let detail = match (j.owner_name(), why) {
                        (_, Some(w)) if w != Why::Removed => format!("{} · {}", j.label, w.short()),
                        (Some(o), _) if o != j.name => format!("{} · {o}", j.label),
                        _ => j.label.clone(),
                    };
                    let dr = detail_line(&p, &cols, &detail);
                    let tip = match &j.program {
                        Some(pr) => format!("{}\nRuns {}", j.plist.display(), pr.display()),
                        None => j.plist.display().to_string(),
                    };
                    ui.interact(dr, ui.scope_id().with(("su-tip", &j.label, j.scope as u8)), Sense::hover()).on_hover_text(tip);
                    when_cell(&p, &cols, r.center().y, when_icon(j), &j.when.label(j.scope));
                    state_cell(&p, cols.state, r.center().y, if !j.on { Some("off") } else if j.pid.is_some() { None } else { Some("idle") }, j.on && j.pid.is_some());
                    let mr = icon_dots(ui, more, ui.scope_id().with(("su-more", &j.label, j.scope as u8)));
                    menu::popup(&mr).show(|ui| {
                        if let Some(sg) = &j.sig {
                            menu::note(ui, &sg.line(), false);
                        }
                        let cmd = j.command();
                        if !cmd.is_empty() {
                            menu::note(ui, &cmd, true);
                        }
                        menu::separator(ui);
                        if let Some(app) = uninstallable(&j.owner) {
                            let n = app.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                            if menu::item(ui, &format!("Open {n} in Uninstaller")).clicked() {
                                cx.open_page("uninstaller", &format!("app:{}", app.display()));
                            }
                        }
                        if menu::item(ui, "Show file in Finder").clicked() {
                            macos::reveal(&j.plist);
                        }
                        if menu::item(ui, "Copy label").clicked() {
                            ui.ctx().copy_text(j.label.clone());
                        }
                        if !cmd.is_empty() && menu::item(ui, "Copy command").clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                        if j.vt_file().is_some_and(|f| f.is_file()) && menu::item(ui, "Check on VirusTotal").on_hover_text("Opens virustotal.com with the file's hash; the file isn't uploaded").clicked() {
                            vt = Some(j.clone());
                        }
                        if trusted_now {
                            if menu::item(ui, "Don't trust").clicked() {
                                trust = Some((j.clone(), false));
                            }
                        } else if (!j.flags().is_empty() || j.why().is_some() || new_labels.contains(&j.label) || changed_labels.contains(&j.label)) && menu::item(ui, "Trust").clicked() {
                            trust = Some((j.clone(), true));
                        }
                        // a bundled job's file is part of its app
                        if !j.bundled {
                            menu::separator(ui);
                            if menu::danger(ui, "Move file to Trash…", busy.is_none()).clicked() {
                                trash = Some(j.clone());
                            }
                        }
                    });
                    if switch(ui, sw, ui.scope_id().with(("su-job", &j.label, j.scope as u8)), j.on, busy.is_none()).on_hover_text(if j.on { "Turn off" } else { "Turn on" }).clicked() {
                        flip_job = Some(j.clone());
                    }
                    y += ROW_H + ROW_GAP;
                }
            }
        }

        // the notification switch, a settings row in its own card
        y += sp::L;
        let nr = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), sp::SETTINGS_ROW_H));
        glass_card(&p, nr, R_CARD, 0.0, false);
        p.text(Pos2::new(nr.left() + sp::L, nr.center().y), Align2::LEFT_CENTER, "Tell me when an app adds itself to startup", font(13.0), TEXT());
        let sw = Rect::from_min_size(Pos2::new(nr.right() - 42.0, nr.center().y - 9.0), Vec2::new(30.0, 18.0));
        if switch(ui, sw, ui.scope_id().with("su-notify"), crate::kit::notify::shown(notify), true).clicked() {
            let mut v = notify;
            crate::kit::notify::flip(&mut v);
            set_notify = Some(v);
        }
    });
    if turn_off_all {
        s.turn_off_suggested();
    }
    if retry {
        s.retry_apps();
    }
    if do_undo {
        s.undo();
    }
    if let Some(la) = flip_app {
        s.toggle_app(&la);
    }
    if let Some(j) = flip_job {
        s.toggle_job(&j);
    }
    if let Some(c) = flip_cron {
        s.toggle_cron(&c);
    }
    if let Some((j, on)) = trust {
        s.set_trust(&j, on);
    }
    if let Some(j) = vt {
        s.virustotal(&j);
    }
    if show_btm {
        s.load_btm();
    }
    if let Some(j) = trash {
        s.trash(&j);
    }
    if let Some(on) = set_notify {
        s.set_notify(on);
    }
}

/// "2 updaters that check in the background, Steam Cleaner, and 1 agent of an app you removed."
/// What AI Check gets: each login app and background job that's on.
/// The removal loader while a job's file goes to the Trash (it can wait on
/// the admin password).
fn trash_loader(s: &mut Startup, ui: &mut Ui, r: Rect) {
    use crate::kit::loader::{self, Click, Count, Status};
    let Some((name, file, res)) = s.state().trashing.clone() else {
        return;
    };
    let status = match res {
        None => Status::Working { title: format!("Removing {name}…"), count: Count::Unknown, current: file, can_stop: false },
        Some(Ok(())) => Status::Finished { title: format!("Removed {name}"), sub: "Moved to the Trash".into(), problem: None, put_back: false },
        Some(Err(e)) => Status::Failed { title: format!("{name} couldn't be removed"), problem: e },
    };
    if let Some(Click::Done | Click::Ended | Click::PutBack) = loader::show(ui, r, "startup-trash", &status) {
        s.state().trashing = None;
    }
}

fn startup_ask(jobs: &[Job], apps: &[LoginApp]) -> aicheck::Ask {
    let mut items: Vec<aicheck::Item> = apps
        .iter()
        .filter(|a| a.on)
        .map(|a| aicheck::Item { key: format!("app:{}", a.name), line: format!("login app {} ({}){}", a.name, a.path.display(), if a.hidden { ", opens hidden" } else { "" }), icon: Some(a.path.clone()), name: a.name.clone() })
        .collect();
    for j in jobs.iter().filter(|j| j.on && !j.apple()) {
        let kind = match j.scope {
            Scope::System => "system daemon (admin)",
            _ => "background agent",
        };
        let mut notes = Vec::new();
        if j.orphan {
            notes.push("its app is gone".to_string());
        }
        if j.pid.is_some() {
            notes.push("running now".into());
        }
        if let Some(w) = j.why() {
            notes.push(format!("rule says: {}", w.short()));
        }
        items.push(aicheck::Item {
            key: j.label.clone(),
            line: format!("{kind} {} · label {} · runs {} · {} · {}", j.name, j.label, j.command(), j.when.label(j.scope), notes.join(", ")),
            icon: j.owner.clone(),
            name: j.name.clone(),
        });
    }
    aicheck::Ask {
        task: "This is the Startup list: apps and background jobs that start with the user's Mac. Point out what can be turned off without breaking anything the user relies on (updaters that run when the app opens anyway, helpers of apps they rarely use, agents of removed apps). Say plainly when turning something off would break a feature.".into(),
        actions: vec![
            ("turn_off", "stop it starting with the Mac (the user can turn it back on)", "Turn off"),
            ("remove", "move the job's file to the Trash; only for jobs whose app is gone", "Remove"),
        ],
        items,
        facts: Vec::new(),
        answer: Vec::new(),
    }
}

fn why_line(list: &[&Job]) -> String {
    let n = |w: Why| list.iter().filter(|j| j.why() == Some(w)).count();
    let mut parts = Vec::new();
    match n(Why::Updater) {
        0 => {}
        1 => parts.push("1 updater that checks in the background".to_string()),
        k => parts.push(format!("{k} updaters that check in the background")),
    }
    let extra: Vec<String> = list.iter().filter(|j| j.why() == Some(Why::NotNeeded)).map(|j| j.name.clone()).collect();
    if !extra.is_empty() {
        parts.push(extra.join(", "));
    }
    match n(Why::Removed) {
        0 => {}
        1 => parts.push("1 agent of an app you removed".to_string()),
        k => parts.push(format!("{k} agents of apps you removed")),
    }
    let s = match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        _ => format!("{}, and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    };
    let mut c = s.chars();
    let s: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    s
}

/// Where the parts of a list row go: icon, name and detail, the schedule
/// and state columns, ⋯ and the switch, [`sp::COL_GAP`] apart.
struct Cols {
    icon: Rect,
    text_x: f32,
    name_max: f32,
    /// None when the window is too narrow: names keep their room instead.
    when: Option<f32>,
    state: f32,
    more: Rect,
    sw: Rect,
    /// The two text lines' centres.
    y1: f32,
    y2: f32,
}

const WHEN_W: f32 = 116.0;
const STATE_W: f32 = 72.0;

fn columns(p: &egui::Painter, r: Rect, switch: bool) -> Cols {
    let cy = r.center().y;
    let sw = Rect::from_min_size(Pos2::new(r.right() - sp::L - 36.0, cy - 10.0), Vec2::new(36.0, 20.0));
    let right = if switch { sw.left() - sp::S } else { r.right() - sp::L };
    let more = Rect::from_min_size(Pos2::new(right - 24.0, cy - 12.0), Vec2::splat(24.0));
    let icon = Rect::from_center_size(Pos2::new(r.left() + sp::L + 14.0, cy), Vec2::splat(28.0));
    let text_x = icon.right() + sp::M;
    let state = more.left() - sp::COL_GAP - STATE_W;
    let when = state - sp::COL_GAP - WHEN_W;
    let (y1, y2) = two_lines_y(p, cy, &bold(13.0), &font(11.0));
    let (when, name_max) = if when - sp::COL_GAP - text_x >= 200.0 { (Some(when), when - sp::COL_GAP - text_x) } else { (None, state - sp::COL_GAP - text_x) };
    Cols { icon, text_x, name_max, when, state, more, sw, y1, y2 }
}

/// The name, then its tags on the same line; the name gives way to the tags.
fn name_line(p: &egui::Painter, c: &Cols, name: &str, col: Color32, tags: &[(&str, Color32)]) -> Rect {
    let tags_w: f32 = tags.iter().map(|(t, _)| p.layout_no_wrap(t.to_string(), font(11.0), TEXT()).size().x + 10.0 + sp::XS).sum();
    let max = if tags.is_empty() { c.name_max } else { (c.name_max - tags_w - sp::XS).max(c.name_max * 0.4) };
    let nr = text_fit(p, Pos2::new(c.text_x, c.y1), Align2::LEFT_CENTER, name, bold(13.0), col, max);
    let mut x = nr.right() + sp::XS;
    for (t, tc) in tags {
        if x > c.text_x + c.name_max {
            break;
        }
        x = tag(p, Pos2::new(x, c.y1), Align2::LEFT_CENTER, t, *tc).right() + sp::XS;
    }
    nr
}

/// The detail line under the name.
fn detail_line(p: &egui::Painter, c: &Cols, text: &str) -> Rect {
    text_fit(p, Pos2::new(c.text_x, c.y2), Align2::LEFT_CENTER, text, font(11.0), TEXT_FAINT(), c.name_max)
}

fn when_icon(j: &Job) -> Icon {
    use super::items::When;
    match j.when {
        When::KeepsRunning => Icon::Refresh,
        When::Every(_) | When::Scheduled => Icon::Clock,
        When::AtLogin => Icon::Power,
        When::OnDemand => Icon::Forward,
    }
}

/// The divider above an item row, inset 16; none next to a lit row (H2).
fn row_line(ui: &Ui, p: &egui::Painter, r: Rect, first: bool, lit: bool) {
    let (a, b) = (Pos2::new(r.left() + sp::L, r.top()), Pos2::new(r.right() - sp::L, r.top()));
    let stroke = if first { egui::Stroke::NONE } else { egui::Stroke::new(1.0, white(HAIRLINE)) };
    crate::kit::glass::row_divider_stroke(ui, p, a, b, lit, r.bottom(), stroke);
}

fn when_cell(p: &egui::Painter, c: &Cols, cy: f32, ic: Icon, label: &str) {
    let Some(x) = c.when else { return };
    icon(p, ic, Rect::from_center_size(Pos2::new(x + 6.0, cy), Vec2::splat(12.0)), TEXT_DIM());
    text_fit(p, Pos2::new(x + 12.0 + sp::S, cy), Align2::LEFT_CENTER, label, font(12.0), TEXT_DIM(), WHEN_W - 12.0 - sp::S);
}

/// `label` None = running (green dot).
fn state_cell(p: &egui::Painter, x: f32, cy: f32, label: Option<&str>, running: bool) {
    p.circle_filled(Pos2::new(x + 4.0, cy), 3.5, if running { GREEN() } else { white(56) });
    p.text(Pos2::new(x + 8.0 + sp::S, cy), Align2::LEFT_CENTER, label.unwrap_or("running"), font(12.0), TEXT_DIM());
}

/// The ⋯ button of a row.
fn icon_dots(ui: &mut Ui, r: Rect, id: egui::Id) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered() && !in_widget());
    let p = ui.painter();
    if h > 0.0 {
        p.rect_filled(r, 6.0, white((18.0 * h) as u8));
    }
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(r.center() + Vec2::new(dx, 0.0), 1.6, TEXT_FAINT());
    }
    resp.on_hover_text("More")
}

// ---------------------------------------------------------------- panel widget

pub fn widget(s: &mut Startup, size: Size, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let id = ui.scope_id().with("w-startup");
    let (loaded, total, running, suggested, undo, busy) = {
        let g = s.state();
        (g.jobs_loaded, g.total(), g.running(), g.suggested(), g.undoable(), g.busy.is_some())
    };
    let n = suggested.len();
    match size {
        // W4, W3, W7: the count and what can go; no button in the panel, the card opens Startup
        Size::S11 | Size::S12 => {
            let value = if loaded { plural(total, "item") } else { "…".into() };
            let (cap, cap_col) = match undo {
                Some(k) => (format!("{k} turned off"), GREEN()),
                None if n > 0 => (format!("{n} suggested"), AMBER()),
                None => (format!("{running} running"), ty::caption_color()),
            };
            let resp = if size == Size::S11 {
                let resp = tile::card(ui, rect, id, false);
                let p = ui.painter();
                icon(p, Icon::Power, Rect::from_min_size(rect.min + Vec2::new(tile::SMALL_SIDE, tile::PAD), Vec2::splat(20.0)), lift(tint::STARTUP()));
                // what can go matters more than the count; the count is in the tip then
                if n > 0 && undo.is_none() {
                    tile::small_text(p, rect, "Startup", &cap, AMBER());
                    resp.on_hover_text(value)
                } else {
                    tile::small_text(p, rect, "Startup", &value, TEXT_DIM());
                    resp.on_hover_text(cap)
                }
            } else {
                tile::wide_full(ui, rect, id, Icon::Power, tint::STARTUP(), None, "Startup", &value, &cap, cap_col, false)
            };
            if resp.clicked() {
                cx.open_page(super::ID, if n > 0 { "suggested" } else { "" });
            }
        }
        _ => {
            let resp = tile::card(ui, rect, id, false);
            let p = ui.painter().clone();
            let note = if loaded { format!("{} · {running} running", plural(total, "item")) } else { "Reading…".into() };
            let (x0, x1) = (rect.left() + tile::PAD, rect.right() - tile::PAD);
            let cy = rect.top() + tile::PAD + 10.0;
            let tr = p.text(Pos2::new(x0, cy), Align2::LEFT_CENTER, "Startup", ty::title(), TEXT());
            let mut right = x1;
            if undo.is_none() && n > 0 {
                let label = format!("Turn off {n}");
                let bw = button_width(&p, &label, BTN_SECONDARY) - 6.0;
                let br = Rect::from_min_max(Pos2::new(x1 - bw, cy - 12.0), Pos2::new(x1, cy + 12.0));
                // W7: a quiet button; no lime in the panel
                if pill_button(ui, br, &label, BTN_SECONDARY, !busy).clicked() {
                    s.turn_off_suggested();
                }
                right = br.left() - 8.0;
            }
            text_fit(&p, Pos2::new(tr.right() + 8.0, cy), Align2::LEFT_CENTER, &note, ty::caption(), ty::caption_color(), right - tr.right() - 8.0);
            let body = Rect::from_min_max(Pos2::new(x0, rect.top() + tile::PAD + 20.0 + sp::S), Pos2::new(x1, rect.bottom() - tile::PAD));
            if let Some(k) = undo {
                let c = Pos2::new(body.left() + 20.0, body.center().y);
                p.circle_filled(c, 18.0, with_alpha(GREEN(), 0.16));
                icon(&p, Icon::Check, Rect::from_center_size(c, Vec2::splat(18.0)), GREEN());
                let br = Rect::from_min_max(Pos2::new(body.right() - 64.0, c.y - 13.0), Pos2::new(body.right(), c.y + 13.0));
                if pill_button(ui, br, "Undo", BTN_SECONDARY, !busy).clicked() {
                    s.undo();
                }
                let (y1, y2) = two_lines_y(&p, c.y, &bold(13.0), &font(12.0));
                p.text(Pos2::new(c.x + 30.0, y1), Align2::LEFT_CENTER, format!("{} turned off", plural(k, "item")), bold(13.0), TEXT());
                text_fit(&p, Pos2::new(c.x + 30.0, y2), Align2::LEFT_CENTER, "The apps still open", font(12.0), TEXT_DIM(), br.left() - c.x - 40.0);
            } else if n == 0 {
                let msg = if loaded { "Nothing to turn off" } else { "Reading what starts with your Mac…" };
                tile::empty_note(&p, body.shrink2(Vec2::new(0.0, 6.0)), msg);
            } else {
                let lh = 28.0;
                let fit = ((body.height() - 14.0) / lh).floor().max(1.0) as usize;
                let show = if n > fit { fit.saturating_sub(0).min(n) } else { n };
                let mut budget = ICONS_PER_FRAME;
                let ctx = ui.ctx().clone();
                for (k, j) in suggested.iter().take(show.min(3)).enumerate() {
                    let r = Rect::from_min_size(Pos2::new(body.left(), body.top() + k as f32 * lh), Vec2::new(body.width(), lh));
                    let ir = Rect::from_center_size(Pos2::new(r.left() + 10.0, r.center().y), Vec2::splat(20.0));
                    app_icon(s, &ctx, &p, ir, j.owner.as_deref(), &j.name, false, &mut budget);
                    let why = j.why().map(|w| w.short()).unwrap_or("");
                    let wr = p.text(Pos2::new(r.right() - 42.0, r.center().y), Align2::RIGHT_CENTER, why, font(11.0), TEXT_FAINT());
                    text_fit(&p, Pos2::new(ir.right() + 8.0, r.center().y), Align2::LEFT_CENTER, &j.name, font(12.0), TEXT(), wr.left() - ir.right() - 16.0);
                    let sw = Rect::from_center_size(Pos2::new(r.right() - 15.0, r.center().y), Vec2::new(30.0, 18.0));
                    if switch(ui, sw, id.with(("w-sw", &j.label)), true, !busy).clicked() {
                        s.toggle_job(j);
                    }
                }
                if n > 3 {
                    p.text(Pos2::new(body.left(), body.bottom() - 4.0), Align2::LEFT_BOTTOM, format!("+{} more · open Startup", n - 3), font(11.0), TEXT_FAINT());
                }
            }
            if resp.clicked() {
                cx.open_page(super::ID, if n > 0 { "suggested" } else { "" });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn why_line_reads_well() {
        let mut g = super::super::State::default();
        super::super::demo(&mut g, false);
        let list = g.suggested();
        let refs: Vec<&Job> = list.iter().collect();
        assert_eq!(why_line(&refs), "2 updaters that check in the background, Steam Cleaner, and 1 agent of an app you removed");
    }

    /// Every demo view renders without a panic.
    #[test]
    fn demo_views_draw() {
        use crate::core::tool::{Tool, ToolEnv};
        let ctx = egui::Context::default();
        let mut s = Startup::new(&ToolEnv { ctx: ctx.clone(), demo: true });
        for v in ["startup", "startup-done", "startup-suggested"] {
            assert!(s.demo_view(v).is_some());
            ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 700.0))), ..Default::default() }, |ui| {
                let mut cx = Cx::new(None);
                page(&mut s, ui, Rect::from_min_size(Pos2::new(220.0, 0.0), Vec2::new(880.0, 700.0)), &mut cx);
                for z in [Size::S11, Size::S12, Size::S24] {
                    widget(&mut s, z, ui, Rect::from_min_size(Pos2::ZERO, Vec2::new(356.0, 174.0)), &mut cx);
                }
            })
            .textures_delta
            .clear();
        }
    }
}
