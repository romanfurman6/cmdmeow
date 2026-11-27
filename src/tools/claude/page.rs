//! The AI usage page in the full window. A header with the provider menu
//! (All providers, or one), the tips chip and two tabs. Limits (first):
//! a section per provider, a card per limit with how much is left, when it
//! is full again and the even-pace tick; hover shows the details. Usage, in
//! $ or tokens (a switch next to the tabs): the last 30 days per provider,
//! the usage chart (a line per provider) and the breakdown by model,
//! project or day.

use super::backend::{self, All};
use super::parts::{self, Measure, Period};
use crate::core::tool::Cx;
use crate::kit::aicheck::{self, ui::Event, Ask, Check, Item, Rec};
use crate::kit::ai::{self, pace, settings, Billing, Day, Limit, Share, UsageInfo};
use crate::kit::claude_code::{self as cc, Availability};
use crate::kit::glass::*;
use crate::kit::menu;
use crate::kit::sync::LockExt;
use chrono::{Datelike, NaiveDate};
use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Shape, Stroke, Ui, UiBuilder, Vec2};


const GAP: f32 = sp::L;
/// SP22: a limit card's left column (name, how much is left, when it comes back).
const LIMIT_LEFT_W: f32 = 184.0;
/// SP22: a limit card: 16, title 16, 8, hero 28, 6, caption 14, 16.
const LIMIT_H: f32 = 2.0 * sp::L + 16.0 + sp::S + 28.0 + 6.0 + 14.0;
/// Two limit cards side by side: 16, title 16, 8, hero 28, 14, bar 8, 10,
/// caption 14, 16.
const LIMIT_V_H: f32 = 2.0 * sp::L + 16.0 + sp::S + 28.0 + 14.0 + 8.0 + 10.0 + 14.0;
/// The "This week" card (SP25): 16, head 16, 16, plot 150, 8, day labels 14, 16.
const WEEK_H: f32 = 2.0 * sp::L + 16.0 + sp::L + 150.0 + 8.0 + 14.0;
/// The right gutter of the week chart, for "100%" and "50%".
const WEEK_GUTTER: f32 = 40.0;
/// A provider's section row (glyph 16, name, plan).
const SECTION_H: f32 = 20.0;
/// A one-line card in a section (checking, signed out, an error).
const NOTE_H: f32 = 52.0;
/// The tokens-only card (a provider without plan limits).
const TOKENS_H: f32 = 2.0 * sp::L + 22.0 + sp::XS + 14.0;
/// An API key's month budget card.
const BUDGET_H: f32 = 140.0;
/// The Cost / Tokens tab's top row (30 days, the chart).
const COST_ROW_H: f32 = 300.0;
/// The Cost / Tokens tab's second row (Totals, by type): two lines of stats.
const KINDS_H: f32 = 2.0 * sp::L + 16.0 + sp::M + 2.0 * (22.0 + sp::XS + 14.0) + sp::L;
/// Most rows in the breakdown.
const BREAKDOWN_ROWS: usize = 8;

/// What the page shows: Limits, or the Usage tab in $ (Cost) or tokens.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Limits,
    Cost,
    Tokens,
}

fn tab_id() -> egui::Id {
    egui::Id::unique("ai-usage-tab")
}

/// Switch the page to `tab` (a tip's "See Limits", AI Check's Show).
pub fn set_tab(ctx: &egui::Context, tab: Tab) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), tab));
}

/// The tab shown. Cost needs cost in $ (Settings › AI Usage); without it
/// the page has Limits and Tokens. Screenshots: CMDMEOW_AI_TAB=cost|tokens.
fn tab(ctx: &egui::Context, show_usd: bool) -> Tab {
    let first = match std::env::var("CMDMEOW_AI_TAB").as_deref() {
        Ok("cost") => Tab::Cost,
        Ok("tokens") => Tab::Tokens,
        _ => Tab::Limits,
    };
    let t = ctx.data(|d| d.get_temp::<Tab>(tab_id())).unwrap_or(first);
    if t == Tab::Cost && !show_usd { Tab::Tokens } else { t }
}

/// The tabs: Limits and Usage.
fn tabs() -> [&'static str; 2] {
    [ts!("ai.limits"), ts!("ai.usage_tab")]
}

/// The Usage tab in the unit last picked ($ unless tokens, or no $ at all).
fn usage_tab(show_usd: bool) -> Tab {
    if show_usd && !settings::get().usage_tokens { Tab::Cost } else { Tab::Tokens }
}

/// The Usage tab's unit switch: "$ | Tokens".
fn units() -> [&'static str; 2] {
    [ts!("ai.unit_usd"), ts!("ai.unit_tokens")]
}

/// The unit switch: 28 tall (SP18), its two segments as wide as the tabs'.
const UNIT_H: f32 = 28.0;

/// The providers on the page: the one picked, or every one that's on
/// (tools that aren't installed get no section).
fn shown(all: &All) -> Vec<UsageInfo> {
    if ai::shows_all() {
        backend::enabled(all).into_iter().filter(|i| i.unavailable() != Some(Availability::NotInstalled)).collect()
    } else {
        vec![backend::with_accounts(backend::current(all).lock_ok().clone(), &settings::get())]
    }
}

// ------------------------------------------------------------------ header

/// The provider menu button's label: "All providers" or the provider.
fn menu_label() -> &'static str {
    if ai::shows_all() { ts!("ai.all_providers") } else { ai::selected_meta().name }
}

/// SP17 menu button: 32 tall, a label and a chevron, Secondary.
fn menu_button_width(p: &egui::Painter, label: &str) -> f32 {
    sp::M + p.layout_no_wrap(label.to_owned(), font(13.0), TEXT()).size().x + sp::S + 8.0 + sp::M
}

fn menu_button(ui: &mut Ui, r: Rect, label: &str) -> egui::Response {
    let (resp, face) = pill_button_at(ui, r, "", BTN_SECONDARY, true);
    let p = ui.painter();
    let t = p.text(Pos2::new(face.left() + sp::M, face.center().y), Align2::LEFT_CENTER, label, font(13.0), TEXT());
    let c = Pos2::new(t.right() + sp::S + 4.0, face.center().y);
    let s = Stroke::new(1.5, TEXT_DIM());
    p.line_segment([c + Vec2::new(-4.0, -2.0), c + Vec2::new(0.0, 2.0)], s);
    p.line_segment([c + Vec2::new(0.0, 2.0), c + Vec2::new(4.0, -2.0)], s);
    resp
}

/// Header (SP16/SP17): "AI Usage", the provider menu; on the right from
/// the edge ↻ Refresh, 12, the tabs, 12, then the tips chip on Limits or
/// the "$ | Tokens" switch on Usage. Returns the tab shown.
#[allow(clippy::too_many_arguments)]
fn header(all: &All, infos: &[UsageInfo], check: &mut Check, ui: &mut Ui, r: Rect, (sub, sub_col): (&str, Color32), tip: &str, demo: bool, cx: &mut Cx) -> Tab {
    let p = ui.painter().clone();
    let show_usd = settings::get().show_cost;
    let labels = tabs();
    let tw = tabs_width(&p, &labels);
    let rw = icon_pill_width(&p, ts!("ai.refresh"), BTN_SECONDARY);
    let has_data = infos.iter().any(has_usage);
    let on_limits = tab(ui.ctx(), show_usd) == Tab::Limits;
    let uw = tabs_width(&p, &units());
    // the chip on Limits, the unit switch on Usage (only with $ on)
    let cw = if on_limits { aicheck::ui::chip_width(check, ui, has_data) } else if show_usd { uw } else { 0.0 };
    let label = menu_label();
    let mw = menu_button_width(&p, label);
    let sub_right = r.right() - sp::PAGE_EDGE - rw - sp::M - tw - sp::M - if cw > 0.0 { cw + sp::M } else { 0.0 };
    let (btns, at) = page_header_room(&p, r, ts!("ai.name"), sub, sub_col, mw, sub_right);
    let (tr, rb) = header_controls(btns, tw, rw);

    // the provider menu, right after the title
    let mr = Rect::from_min_size(Pos2::new(at.x, at.y - sp::HEADER_CONTROL_H / 2.0), Vec2::new(mw, sp::HEADER_CONTROL_H));
    let resp = menu_button(ui, mr, label);
    let set = settings::get();
    let on: Vec<usize> = set.ordered().into_iter().filter(|i| set.is_on(ai::PROVIDERS[*i].id)).collect();
    // H10: drag a provider to reorder; the widgets show the first ("In widgets")
    let pick = if on.len() > 1 { backend::widget_pick(all) } else { None };
    menu::popup(&resp).align(egui::RectAlign::BOTTOM_START).show(|ui| {
        if menu::choice(ui, ts!("ai.all_providers"), ai::shows_all()).clicked() {
            backend::select(ai::ALL, demo);
        }
        menu::separator(ui);
        let mut rows = Vec::with_capacity(on.len());
        for i in &on {
            let chip = if pick == Some(*i) { ts!("ai.in_widgets") } else { "" };
            let r = menu::drag_choice(ui, ai::PROVIDERS[*i].name, !ai::shows_all() && ai::selected() == *i, chip);
            if r.clicked() {
                backend::select(*i, demo);
            }
            rows.push(r);
        }
        if let Some((from, gap)) = crate::kit::reorder::rows(ui, &rows, menu::INSET) {
            settings::update(!demo, |s| s.reorder(&on, from, gap));
        }
        // another Claude Code account (its own config folder)
        if set.is_on("claude") {
            menu::separator(ui);
            if menu::item(ui, ts!("ai.acc.add_menu")).clicked() {
                super::accounts::open(ui.ctx());
            }
        }
    });

    // ↻ at the right edge; when it last updated sits in its tip (SP17 H5)
    let busy = infos.iter().any(|i| i.loading) || backend::refreshing();
    if busy {
        crate::kit::glass::poll(ui.ctx());
    }
    let resp = icon_pill(ui, rb, Icon::Refresh, ts!("ai.refresh"), BTN_SECONDARY, !busy);
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    if resp.clicked() {
        backend::request_refresh();
        cx.toast(ts!("ai.checking_tools"));
    }

    let cur = tab(ui.ctx(), show_usd);
    let at_tab = usize::from(cur != Tab::Limits);
    if let Some(k) = segmented(ui, tr, ui.scope_id().with("ai-tab"), &labels, at_tab) {
        set_tab(ui.ctx(), if k == 0 { Tab::Limits } else { usage_tab(show_usd) });
    }

    // Usage: the unit switch, 12 left of the tabs
    if !on_limits {
        if show_usd {
            let ur = Rect::from_min_size(Pos2::new(tr.left() - sp::M - uw, tr.center().y - UNIT_H / 2.0), Vec2::new(uw, UNIT_H));
            if let Some(k) = segmented(ui, ur, ui.scope_id().with("ai-unit"), &units(), usize::from(cur == Tab::Tokens)) {
                settings::update(!demo, |s| s.usage_tokens = k == 1);
                set_tab(ui.ctx(), if k == 1 { Tab::Tokens } else { Tab::Cost });
            }
        }
        let _ = all;
        return tab(ui.ctx(), show_usd);
    }

    // the tips chip, 12 left of the tabs
    let chip_at = Pos2::new(tr.left() - sp::M - cw, tr.center().y);
    let now = ui.ctx().input(|i| i.time);
    match aicheck::ui::chip(check, ui, chip_at, has_data) {
        Some(Event::Start) => check.start(ui.ctx(), ask(infos)),
        Some(Event::Act(rec)) => {
            match rec.action.as_str() {
                "copy" => {
                    let cmd = rec.key.trim_start_matches("cmd:").to_string();
                    ui.ctx().copy_text(cmd.clone());
                    cx.toast(t!("ai.copied", cmd = cmd));
                }
                "clean" => cx.open_page(super::ui::CLEANUP, "claude"),
                _ => {
                    set_tab(ui.ctx(), Tab::Limits);
                    check.show(&rec.key, now);
                }
            }
            check.done(&rec);
        }
        Some(Event::Show(_)) => set_tab(ui.ctx(), Tab::Limits),
        None => {}
    }
    let _ = all;
    tab(ui.ctx(), show_usd)
}

// -------------------------------------------------------------------- tips

/// What Claude Code's `/usage` says uses the limits, as the chip's own
/// tips (verbatim, its words): the leading "76%" is the value, a line with
/// a slash command can copy it. Also the popover's title and Claude's
/// "Last 24h · 115 requests" after it.
fn usage_tips(infos: &[UsageInfo]) -> (Vec<Rec>, Option<(String, String)>) {
    let Some(cl) = infos.iter().find(|i| i.provider == ai::claude::NAME) else { return (Vec::new(), None) };
    let Some(c) = cl.contrib.first() else { return (Vec::new(), None) };
    let mut out: Vec<Rec> = Vec::new();
    for line in &c.lines {
        let (value, text) = split_pct(line);
        // a line without its own number explains the one before it
        if value.is_empty() {
            if let Some(last) = out.last_mut() {
                last.why = if last.why.is_empty() { text.to_string() } else { format!("{} {text}", last.why) };
                set_command(last);
                continue;
            }
        }
        let mut r = Rec { key: format!("usage:{}", out.len()), title: text.to_string(), value: value.to_string(), name: "Claude Code".into(), ..Default::default() };
        set_command(&mut r);
        out.push(r);
    }
    (out, Some((ts!("ai.what_uses").to_string(), c.title.clone())))
}

/// A /usage line's leading number ("76%") and the whole line: the sentence
/// stays Claude's own, the number also shows in the value column.
fn split_pct(line: &str) -> (&str, &str) {
    let t = line.trim();
    let n = t.find('%').filter(|n| *n > 0 && t[..*n].chars().all(|c| c.is_ascii_digit() || c == '.'));
    match n {
        Some(n) => (&t[..=n], t),
        None => ("", t),
    }
}

/// A tip that names a slash command ("run /compact") copies it; one about
/// MCP servers copies `/mcp`, where they're turned off.
fn set_command(r: &mut Rec) {
    let text = format!("{} {}", r.title, r.why);
    let cmd = text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/' && c != '-')).find(|w| w.len() > 1 && w.starts_with('/') && w[1..].chars().all(|c| c.is_ascii_lowercase() || c == '-'));
    let cmd = cmd.map(str::to_owned).or_else(|| text.contains("MCP").then(|| "/mcp".to_string()));
    if let Some(cmd) = cmd {
        r.key = format!("cmd:{cmd}");
        r.action = "copy".into();
        r.label = ts!("ai.copy_command").into();
    }
}

/// The AI Check ask: every limit with its pace, the models of 7 days, and
/// Claude's caches (Cleanup can clean them).
fn ask(infos: &[UsageInfo]) -> Ask {
    let now = parts::now();
    let mut items = Vec::new();
    let mut facts = Vec::new();
    for info in infos {
        for l in &info.limits {
            let pc = pace::of_limit(l, now);
            let mut line = format!("{} · {}: {:.0}% used, {:.0}% left", info.provider, backend::short_label(&l.label), l.pct, 100.0 - l.pct);
            if let Some(pc) = &pc {
                line.push_str(&format!(", even use would be at {:.0}% by now", pc.expected));
                if let Some(at) = pc.out_at {
                    line.push_str(&format!(", runs out {} at this rate", ai::alerts::when(at, now)));
                }
            }
            let reset = parts::reset_when(l);
            if !reset.is_empty() {
                line.push_str(&format!(", resets {reset}"));
            }
            items.push(Item { key: limit_key(info, l), line, icon: None, name: info.provider.into() });
        }
        let t7: u64 = info.local.days.iter().rev().take(7).map(|d| d.tokens).sum();
        facts.push(format!("{}: {} tokens in 7 days, {} today, {} requests today", info.provider, ai::human_tokens(t7), ai::human_tokens(info.local.tokens_today), info.local.requests_today));
        let today = chrono::Local::now().date_naive();
        let week: Vec<_> = info.local.archive.iter().filter(|d| (today - d.date).num_days() < 7).collect();
        for m in ai::shares_of(week.iter().copied(), true).iter().take(4) {
            facts.push(format!("{} model {}: {} tokens in 7 days", info.provider, m.name, ai::human_tokens(m.tokens)));
        }
        for c in &info.contrib {
            facts.push(format!("{} reports: {}", info.provider, c.title));
            facts.extend(c.lines.iter().map(|l| format!("  {l}")));
        }
    }
    if infos.iter().any(|i| i.provider == ai::claude::NAME) {
        items.push(Item { key: "caches".into(), line: "Claude Code's caches and old logs on this Mac (Cleanup lists them)".into(), icon: None, name: ai::claude::NAME.into() });
    }
    Ask {
        task: "This is AI Usage: the plan limits of the person's AI coding assistants (how much is left, when it comes back) and what used them. \
               Look for a limit that runs out before it resets, a window used far faster than even use, or a model or habit that spends most of it, and say what to change. \
               The reports lines are the assistant's own breakdown of the last day; the page already shows them, so don't repeat them as tips."
            .into(),
        actions: vec![("show", "Show that limit on the page (limit items only)", ts!("ai.ac.show")), ("clean", "Open Claude Code's caches in Cleanup (the caches item only)", ts!("ai.ac.clean"))],
        items,
        facts,
        answer: Vec::new(),
    }
}

fn limit_key(info: &UsageInfo, l: &Limit) -> String {
    format!("limit:{}:{}", info.provider, l.label)
}

// -------------------------------------------------------------------- page

pub fn page(all: &All, check: &mut Check, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    super::accounts::dialog(ui, demo);
    let infos = shown(all);
    let (pinned, title) = usage_tips(&infos);
    if !check.demo || !pinned.is_empty() {
        check.pinned = pinned;
        check.pinned_label = ts!("ai.usage_source").into();
        check.title = title;
    }
    check.locate = true;
    // when the limits were last checked: in the refresh tip; amber in the
    // status only when that check failed (SP17 H5)
    let stale = infos.iter().find(|i| i.stale);
    let tip = infos.iter().filter_map(|i| i.fetched).max().map(|f| t!("ai.updated", time = f.format("%H:%M")).into_owned()).unwrap_or_default();
    let (sub, sub_col) = match stale {
        Some(i) => (parts::fresh(i).0, AMBER()),
        None => (String::new(), TEXT_DIM()),
    };

    // one provider that isn't ready: one state for the whole page
    if !ai::shows_all() {
        let cl = &infos[0];
        if !cl.scanned {
            header(all, &infos, check, ui, r, ("", TEXT_DIM()), "", demo, cx);
            checking(cl, ui, r);
            return;
        }
        if let Some(a) = cl.unavailable() {
            header(all, &infos, check, ui, r, ("", TEXT_DIM()), "", demo, cx);
            not_set_up(a, cl, ui, r, cx);
            return;
        }
        if !has_usage(cl) {
            header(all, &infos, check, ui, r, ("", TEXT_DIM()), &tip, demo, cx);
            no_usage(cl, ui, r);
            return;
        }
    }
    let tab = header(all, &infos, check, ui, r, (&sub, sub_col), &tip, demo, cx);
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + PAGE_HEADER_H + 1.0), r.max);
    let w = body.width() - 2.0 * sp::PAGE_EDGE;
    let h = match tab {
        Tab::Limits => limits_height(&infos),
        Tab::Cost | Tab::Tokens => cost_height(),
    };
    let mut child = ui.new_child(UiBuilder::new().max_rect(body).id_salt("ai-page"));
    ScrollArea::vertical().id_salt(("ai-scroll", tab as u8)).auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + 2.0 * sp::L), Sense::hover());
        let a = Rect::from_min_size(Pos2::new(area.left() + sp::PAGE_EDGE, area.top() + sp::L), Vec2::new(w, h));
        match tab {
            Tab::Limits => limits_tab(&infos, check, ui, a),
            Tab::Cost => cost_tab(&infos, ui, a, Measure::Usd, demo),
            Tab::Tokens => cost_tab(&infos, ui, a, Measure::Tokens, demo),
        }
    });
}

// ------------------------------------------------------------------ limits

/// What a provider's section holds under its name.
enum Block<'a> {
    Limit(&'a Limit),
    /// One limit across every account (by its label).
    Multi(String),
    Note(String),
    Tokens,
    Budget,
}

fn blocks(info: &UsageInfo) -> Vec<Block<'_>> {
    let meta = ai::meta(info.provider);
    if !info.scanned {
        return vec![Block::Note(t!("ai.checking_name", name = meta.name).into_owned())];
    }
    match info.unavailable() {
        Some(Availability::SignedOut) => return vec![Block::Note(t!("ai.not_signed_in_title", name = meta.name).into_owned())],
        Some(_) => return vec![Block::Note(t!("ai.not_installed_title", name = meta.name).into_owned())],
        None => {}
    }
    if info.is_api() {
        return vec![Block::Budget];
    }
    if !info.accounts.is_empty() {
        // a card per limit any account has, in the order they come
        let mut labels: Vec<String> = Vec::new();
        for a in info.each_account() {
            for l in &a.limits {
                if !labels.contains(&l.label) {
                    labels.push(l.label.clone());
                }
            }
        }
        if !labels.is_empty() {
            return labels.into_iter().map(Block::Multi).collect();
        }
    }
    if !info.limits.is_empty() {
        return info.limits.iter().map(Block::Limit).collect();
    }
    if meta.limits && (info.loading || info.error.is_some()) {
        return vec![Block::Note(info.error.clone().unwrap_or_else(|| t!("ai.asking_limits", provider = info.provider).into_owned()))];
    }
    vec![Block::Tokens]
}

fn block_h(b: &Block, info: &UsageInfo) -> f32 {
    match b {
        Block::Limit(_) => LIMIT_H,
        Block::Multi(_) => multi_h(info.accounts.len() + 1),
        Block::Note(_) => NOTE_H,
        Block::Tokens => TOKENS_H,
        Block::Budget => BUDGET_H,
    }
}

/// A provider's cards as rows: two limits in a row side by side, the rest
/// one per row.
fn rows_of(bs: Vec<Block<'_>>) -> Vec<Vec<Block<'_>>> {
    let mut rows: Vec<Vec<Block>> = Vec::new();
    for b in bs {
        let pair = matches!(b, Block::Limit(_)) && rows.last().is_some_and(|r| r.len() == 1 && matches!(r[0], Block::Limit(_)));
        if pair {
            rows.last_mut().unwrap().push(b);
        } else {
            rows.push(vec![b]);
        }
    }
    rows
}

fn row_h(row: &[Block], info: &UsageInfo) -> f32 {
    if row.len() == 2 { LIMIT_V_H } else { block_h(&row[0], info) }
}

/// The weekly limit and the days since it last reset, when the page shows
/// one provider and it has both.
fn week_of(infos: &[UsageInfo]) -> Option<(&UsageInfo, &Limit)> {
    let [info] = infos else { return None };
    let l = info.weekly()?;
    // the same check the card draws with, so a card never comes out empty
    (l.pct > 0.0 && info.accounts.is_empty() && week_days(info, l).is_some()).then_some((info, l))
}

fn limits_height(infos: &[UsageInfo]) -> f32 {
    let mut h = 0.0;
    for (k, info) in infos.iter().enumerate() {
        if k > 0 {
            h += sp::XL;
        }
        h += SECTION_H;
        for row in rows_of(blocks(info)) {
            h += GAP + row_h(&row, info);
        }
    }
    if week_of(infos).is_some() {
        h += GAP + WEEK_H;
    }
    h
}

/// Every provider shown: its name row, then its limits, two to a row; with
/// one provider, its week by day under them.
fn limits_tab(infos: &[UsageInfo], check: &mut Check, ui: &mut Ui, a: Rect) {
    let p = ui.painter().clone();
    let mut y = a.top();
    for (k, info) in infos.iter().enumerate() {
        if k > 0 {
            y += sp::XL;
        }
        section_row(ui, Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), SECTION_H)), info);
        y += SECTION_H;
        for row in rows_of(blocks(info)) {
            y += GAP;
            let h = row_h(&row, info);
            let two = row.len() == 2;
            let w = if two { (a.width() - GAP) / 2.0 } else { a.width() };
            for (j, b) in row.into_iter().enumerate() {
                let cr = Rect::from_min_size(Pos2::new(a.left() + j as f32 * (w + GAP), y), Vec2::new(w, h));
                match b {
                    Block::Limit(l) => {
                        let key = limit_key(info, l);
                        if check.take_scroll(&key) {
                            ui.scroll_to_rect(cr, Some(egui::Align::Center));
                        }
                        let ring = check.ring(ui.ctx(), &key);
                        if two {
                            limit_card_v(ui, cr, info, l, ring);
                        } else {
                            limit_card(ui, cr, info, l, ring);
                        }
                    }
                    Block::Multi(label) => multi_card(ui, cr, info, &label),
                    Block::Note(msg) => {
                        data_card(&p, cr);
                        text_fit(&p, Pos2::new(cr.left() + sp::L, cr.center().y), Align2::LEFT_CENTER, &msg, ty::body(), TEXT_DIM(), cr.width() - 2.0 * sp::L);
                    }
                    Block::Tokens => tokens_card(&p, cr, info),
                    Block::Budget => budget_card(ui, cr, info),
                }
            }
            y += h;
        }
    }
    if let Some((info, l)) = week_of(infos) {
        y += GAP;
        week_card(ui, Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), WEEK_H)), info, l);
    }
}

/// A provider's name row: its mark, name and plan (dim).
fn section_row(ui: &mut Ui, r: Rect, info: &UsageInfo) {
    let p = ui.painter().clone();
    let g = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 8.0), Vec2::splat(16.0));
    provider_mark(&p, g, info.provider);
    let name = ai::meta(info.provider).name;
    let t = p.text(Pos2::new(g.right() + sp::S, r.center().y), Align2::LEFT_CENTER, name, ty::title(), TEXT());
    let full = info.plan_title();
    let mut plan = full.strip_prefix(info.provider).unwrap_or(&full).trim().to_string();
    if info.is_api() {
        plan = ts!("ai.api_key").into();
    }
    if !info.accounts.is_empty() {
        plan = t!("ai.acc.n", n = info.accounts.len() + 1).into_owned();
    }
    if !plan.is_empty() {
        p.text(Pos2::new(t.right() + sp::S, r.center().y), Align2::LEFT_CENTER, plan, ty::body(), TEXT_DIM());
    }
}

/// Claude's burst and Codex's prompt square, a letter tile for the rest.
pub(super) fn provider_mark(p: &egui::Painter, r: Rect, provider: &str) {
    use crate::kit::agent::Agent;
    match ai::meta(provider).id {
        "claude" => aicheck::ui::agent_mark(p, r, Agent::Claude, None),
        "codex" => aicheck::ui::agent_mark(p, r, Agent::Codex, None),
        _ => letter_tile(p, r, provider, 1.0),
    }
}

/// How much of a limit is left, in percent.
fn left_of(l: &Limit) -> f32 {
    (100.0 - l.pct).clamp(0.0, 100.0)
}

/// "2d 14h" until `at`.
fn until(at: i64) -> String {
    human_duration((at - parts::now()).max(60) as u64)
}

/// A session that hasn't started: nothing used, nothing to reset.
fn idle(l: &Limit) -> bool {
    l.pct <= 0.0 && parts::reset_when(l).is_empty()
}

/// What a limit card shows, both layouts: what's left, the caption under
/// it and its colour, the head over the bar, the bar's fill and colour,
/// and where even use would leave it by now.
struct LimitView {
    left: f32,
    red: bool,
    cap: String,
    cap_col: Color32,
    head: String,
    filled: f32,
    even: Option<f32>,
}

fn limit_view(l: &Limit) -> LimitView {
    let now = parts::now();
    let pc = pace::of_limit(l, now);
    let out = pc.as_ref().and_then(|p| p.out_at);
    let left = left_of(l);
    let (cap, cap_col) = if idle(l) {
        (ts!("ai.not_started").to_string(), TEXT_DIM())
    } else if let Some(wc) = parts::week_word(l) {
        wc
    } else if let Some(at) = out {
        (t!("ai.runs_out", when = ai::alerts::when(at, now)).into_owned(), RED())
    } else if let Some(at) = l.resets_at {
        (t!("ai.full_in", time = until(at)).into_owned(), TEXT_DIM())
    } else {
        (parts::forecast(l), TEXT_DIM())
    };
    let reset = parts::reset_when(l);
    let head = if reset.is_empty() { String::new() } else { t!("ai.resets", when = reset).into_owned() };
    // not started: an empty track, it fills from the first message
    let filled = if idle(l) { 0.0 } else { (left / 100.0).max(if left > 0.0 { 0.01 } else { 0.0 }) };
    let even = pc.as_ref().map(|pc| (100.0 - pc.expected).clamp(0.0, 100.0));
    LimitView { left, red: out.is_some(), cap, cap_col, head, filled, even }
}

/// The bar of what's left with the even-use tick.
fn limit_bar(p: &egui::Painter, br: Rect, v: &LimitView) {
    bar(p, br, v.filled, if v.red { RED() } else { tint::CLAUDE() });
    if let Some(e) = v.even {
        let x = br.left() + br.width() * e / 100.0;
        p.line_segment([Pos2::new(x, br.top() - 4.0), Pos2::new(x, br.bottom() + 4.0)], Stroke::new(1.5, TEXT()));
    }
}

/// Hover on a limit card: the details (P1 popover).
fn limit_hover(ui: &mut Ui, cr: Rect, info: &UsageInfo, l: &Limit, v: &LimitView) {
    let resp = ui.interact(cr, ui.scope_id().with(("ai-limit", info.provider, &l.label)), Sense::hover());
    let mut tip = egui::Tooltip::for_enabled(&resp);
    tip.popup = tip.popup.align(egui::RectAlign::BOTTOM_END).gap(6.0);
    let updated = info.fetched.map(|f| t!("ai.updated", time = f.format("%H:%M")).into_owned()).unwrap_or_default();
    tip.show(|ui| limit_tip(ui, l, v.left, v.even, &updated));
}

fn limit_ring(p: &egui::Painter, cr: Rect, ring: f32) {
    if ring > 0.0 {
        p.rect_stroke(cr, R_CARD, Stroke::new(2.0, with_alpha(AI(), ring)), egui::StrokeKind::Inside);
    }
}

/// SP22: one plan limit as a card. Left: its name, "48% left", and when
/// it's full again ("Full in 2d 14h"; red "Runs out 16:10" when this pace
/// empties it first). Right: when it resets over an 8 pt bar of what's
/// left, with a tick where even use would leave it by now. Hover: the
/// details (P1 popover).
fn limit_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, l: &Limit, ring: f32) {
    let p = ui.painter().clone();
    data_card(&p, cr);
    limit_ring(&p, cr, ring);
    let i = cr.shrink(sp::L);
    let v = limit_view(l);
    // left column
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &backend::short_label(&l.label), ty::title(), TEXT(), LIMIT_LEFT_W);
    let hr = p.text(Pos2::new(i.left(), tr.bottom() + sp::S), Align2::LEFT_TOP, format!("{:.0}%", v.left), ty::hero(), if v.red { RED() } else { TEXT() });
    p.text(Pos2::new(hr.right() + 6.0, hr.bottom() - 3.0), Align2::LEFT_BOTTOM, ts!("ai.left"), ty::label(), TEXT_DIM());
    text_fit(&p, Pos2::new(i.left(), hr.bottom() + 6.0), Align2::LEFT_TOP, &v.cap, ty::label(), v.cap_col, LIMIT_LEFT_W);
    // right: reset over the bar
    let bx = i.left() + LIMIT_LEFT_W + sp::L;
    let br = Rect::from_min_max(Pos2::new(bx, i.center().y + 16.0 - 8.0), Pos2::new(i.right(), i.center().y + 16.0));
    text_fit(&p, Pos2::new(br.right(), br.top() - 10.0), Align2::RIGHT_BOTTOM, &v.head, ty::label(), TEXT_DIM(), br.width());
    limit_bar(&p, br, &v);
    limit_hover(ui, cr, info, l, &v);
}

/// SP22, two to a row: name with the reset at its right, "40% left", the
/// bar, then when it's full again.
fn limit_card_v(ui: &mut Ui, cr: Rect, info: &UsageInfo, l: &Limit, ring: f32) {
    let p = ui.painter().clone();
    data_card(&p, cr);
    limit_ring(&p, cr, ring);
    let i = cr.shrink(sp::L);
    let v = limit_view(l);
    let hw = if v.head.is_empty() { 0.0 } else { p.layout_no_wrap(v.head.clone(), ty::label(), TEXT_DIM()).size().x };
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &backend::short_label(&l.label), ty::title(), TEXT(), i.width() - hw - sp::M);
    p.text(Pos2::new(i.right(), tr.center().y), Align2::RIGHT_CENTER, &v.head, ty::label(), TEXT_DIM());
    let hr = p.text(Pos2::new(i.left(), tr.bottom() + sp::S), Align2::LEFT_TOP, format!("{:.0}%", v.left), ty::hero(), if v.red { RED() } else { TEXT() });
    p.text(Pos2::new(hr.right() + 6.0, hr.bottom() - 3.0), Align2::LEFT_BOTTOM, ts!("ai.left"), ty::label(), TEXT_DIM());
    let br = Rect::from_min_size(Pos2::new(i.left(), hr.bottom() + 14.0), Vec2::new(i.width(), 8.0));
    limit_bar(&p, br, &v);
    text_fit(&p, Pos2::new(i.left(), br.bottom() + 10.0), Align2::LEFT_TOP, &v.cap, ty::label(), v.cap_col, i.width());
    limit_hover(ui, cr, info, l, &v);
}

/// What each day since the weekly reset took of the weekly limit: the
/// limit's used share split by that day's tokens. Every date the window
/// touches (8 when it doesn't start at midnight); days still to come are
/// `None`.
fn week_days(info: &UsageInfo, l: &Limit) -> Option<(i64, WeekDays)> {
    use chrono::TimeZone;
    let end = l.resets_at?;
    let start = end - week_len(l);
    let first = chrono::Local.timestamp_opt(start, 0).single()?.date_naive();
    let last = chrono::Local.timestamp_opt(end - 1, 0).single()?.date_naive();
    let today = chrono::Local::now().date_naive();
    let days: Vec<NaiveDate> = first.iter_days().take_while(|d| *d <= last).collect();
    let tok = |d: NaiveDate| info.local.days.iter().find(|x| x.date == d).map_or(0, |x| x.tokens);
    let sum: u64 = days.iter().filter(|d| **d <= today).map(|d| tok(*d)).sum();
    if sum == 0 {
        return None;
    }
    Some((start, days.into_iter().map(|d| (d, (d <= today).then(|| l.pct * tok(d) as f32 / sum as f32))).collect()))
}

/// A weekly window's length: its own, else 7 days.
fn week_len(l: &Limit) -> i64 {
    if l.window_secs > 0 { l.window_secs } else { 7 * 86_400 }
}

/// The dates of a weekly window and what each took (`None`: still to come).
type WeekDays = Vec<(NaiveDate, Option<f32>)>;

/// "This week" (SP25): a burn-up of the weekly limit since its reset. x is
/// the window's 7 days, y 0–100% of the limit; the used line steps day by
/// day to a dot at now, a dashed diagonal is even use, and a dotted
/// forecast runs to the reset at the current rate (red to the run-out time
/// when it crosses 100%). Hover a day: what it took and the total so far.
fn week_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, l: &Limit) {
    use chrono::TimeZone;
    let p = ui.painter().clone();
    data_card(&p, cr);
    let i = cr.shrink(sp::L);
    let Some((start, days)) = week_days(info, l) else { return };
    let len = week_len(l);
    let end = start + len;
    let now = parts::now().clamp(start, end);
    let local = |t: i64| chrono::Local.timestamp_opt(t, 0).single();
    let since = local(start).map(|t| crate::kit::date::then(crate::kit::date::wd(&t), &t.format("%H:%M").to_string())).unwrap_or_default();
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.this_week"), ty::title(), TEXT());
    p.text(i.right_top(), Align2::RIGHT_TOP, t!("ai.week_since", when = since), ty::label(), TEXT_DIM());
    let plot = Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::L), Pos2::new(i.right() - WEEK_GUTTER, i.bottom() - 8.0 - 14.0));
    let x = |t: i64| plot.left() + plot.width() * ((t - start) as f32 / len as f32).clamp(0.0, 1.0);
    let y = |v: f32| plot.bottom() - plot.height() * (v / 100.0).clamp(0.0, 1.0);
    let ink = |a: f32| with_alpha(TEXT(), a);
    // scale: 100% hairline, a faint 50%, the base
    p.line_segment([Pos2::new(plot.left(), y(50.0)), Pos2::new(plot.right(), y(50.0))], Stroke::new(1.0, ink(0.07)));
    p.line_segment([Pos2::new(plot.left(), y(100.0)), Pos2::new(plot.right(), y(100.0))], Stroke::new(1.0, ink(0.3)));
    p.line_segment([Pos2::new(plot.left(), plot.bottom()), Pos2::new(plot.right(), plot.bottom())], Stroke::new(1.0, ink(0.12)));
    p.text(Pos2::new(i.right(), y(100.0)), Align2::RIGHT_CENTER, "100%", ty::caption(), TEXT_FAINT());
    p.text(Pos2::new(i.right(), y(50.0)), Align2::RIGHT_CENTER, "50%", ty::caption(), ink(0.4));
    // each day's span in the window, its share and the total by its end
    let today = chrono::Local::now().date_naive();
    let midnight = |d: NaiveDate| d.and_hms_opt(0, 0, 0).and_then(|t| chrono::Local.from_local_datetime(&t).earliest()).map_or(start, |t| t.timestamp());
    let mut cols = Vec::new();
    let mut total = 0.0;
    for (d, v) in &days {
        let a = midnight(*d).max(start);
        let b = midnight(*d + chrono::Days::new(1)).min(end);
        total += v.unwrap_or(0.0);
        cols.push((*d, a, b, *v, total));
    }
    for (_, a, _, _, _) in cols.iter().skip(1) {
        p.line_segment([Pos2::new(x(*a), y(100.0)), Pos2::new(x(*a), plot.bottom())], Stroke::new(1.0, ink(0.05)));
    }
    // the day under the pointer
    let mut hover = None;
    for (k, c) in cols.iter().enumerate().filter(|(_, c)| c.3.is_some()) {
        let col = Rect::from_min_max(Pos2::new(x(c.1), y(100.0)), Pos2::new(x(c.2), plot.bottom()));
        let r = ui.interact(col, ui.scope_id().with(("ai-week", k)), Sense::hover());
        if r.hovered() {
            hover = Some((c, r));
        }
    }
    if let Some(((_, a, b, _, _), _)) = &hover {
        p.rect_filled(Rect::from_min_max(Pos2::new(x(*a), y(100.0)), Pos2::new(x(*b), plot.bottom())), 6.0, ink(0.04));
    }
    for (d, a, b, v, _) in &cols {
        let (font, col) = if *d == today {
            (bold(12.0), TEXT())
        } else if v.is_none() {
            (ty::label(), ink(0.35))
        } else {
            (ty::label(), TEXT_FAINT())
        };
        text_fit(&p, Pos2::new((x(*a) + x(*b)) / 2.0, plot.bottom() + 8.0), Align2::CENTER_TOP, crate::kit::date::wd(d), font, col, x(*b) - x(*a));
    }
    // even use: 0 at the reset to 100% at the next
    p.extend(Shape::dashed_line(&[Pos2::new(plot.left(), plot.bottom()), Pos2::new(plot.right(), y(100.0))], Stroke::new(1.5, ink(0.4)), 4.0, 4.0));
    // used: the total at each past midnight, then now
    let mut pts = vec![Pos2::new(x(start), y(0.0))];
    for (d, _, b, _, sum) in &cols {
        if *d < today && *b <= now {
            pts.push(Pos2::new(x(*b), y(*sum)));
        }
    }
    let used = l.pct.clamp(0.0, 100.0);
    let at = Pos2::new(x(now), y(used));
    pts.push(at);
    let mut mesh = egui::Mesh::default();
    let fill = with_alpha(tint::CLAUDE(), 0.16);
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        for q in [w[0], w[1], Pos2::new(w[1].x, plot.bottom()), Pos2::new(w[0].x, plot.bottom())] {
            mesh.colored_vertex(q, fill);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(pts, Stroke::new(2.0, tint::CLAUDE())));
    // the hovered day's own stretch of the line
    if let Some(((_, a, b, Some(v), sum), _)) = &hover {
        let from = Pos2::new(x(*a), y(sum - v));
        let to = if *b > now { at } else { Pos2::new(x(*b), y(*sum)) };
        p.line_segment([from, to], Stroke::new(3.5, tint::CLAUDE()));
    }
    // forecast at the current rate, once there's a rate to tell
    if let Some(pc) = pace::of(l.pct, start, end, now) {
        if let Some(out) = pc.out_at.filter(|o| *o > now) {
            let o = Pos2::new(x(out), y(100.0));
            p.extend(Shape::dashed_line(&[at, o], Stroke::new(1.5, RED()), 2.0, 3.0));
            p.circle_filled(o, 3.5, RED());
            p.text(Pos2::new(o.x, o.y + 6.0), Align2::CENTER_TOP, ai::alerts::when(out, parts::now()), bold(12.0), RED());
        } else if pc.out_at.is_none() && l.pct > 0.0 && (now - start) as f32 >= len as f32 * 0.03 {
            let at_end = l.pct * len as f32 / (now - start) as f32;
            let e = Pos2::new(plot.right(), y(at_end));
            p.extend(Shape::dashed_line(&[at, e], Stroke::new(1.5, with_alpha(tint::CLAUDE(), 0.7)), 2.0, 3.0));
            // under the line's end, clear of the even-use diagonal and "100%"
            p.text(Pos2::new(e.x - 4.0, e.y + 6.0), Align2::RIGHT_TOP, format!("{at_end:.0}%"), ty::label(), TEXT_FAINT());
        }
    }
    p.circle(at, 4.5, tint::CLAUDE(), Stroke::new(2.0, BG()));
    p.text(Pos2::new(at.x - 8.0, at.y - 7.0), Align2::RIGHT_BOTTOM, format!("{used:.0}%"), bold(13.0), TEXT());
    // hover: the day's share and the total by its end
    if let Some(((d, _, _, Some(v), sum), resp)) = hover {
        // beside the day, its bottom on the plot's base: never over the day labels
        let mut tip = egui::Tooltip::for_enabled(&resp);
        tip.popup = tip.popup.align(egui::RectAlign::RIGHT_END).align_alternatives(&[egui::RectAlign::LEFT_END]).gap(6.0);
        let (d, v, sum) = (*d, *v, *sum);
        tip.show(|ui| day_tip(ui, info, d, v, sum));
    }
}

/// SP25 day hover: the date, what the day took of the week and the total
/// by its end; its tokens, cost (when shown) and requests; its top models.
/// Painted at a fixed width so the popover fits its content, not the window.
fn day_tip(ui: &mut Ui, info: &UsageInfo, d: NaiveDate, used: f32, week: f32) {
    const W: f32 = 208.0;
    const ROW: f32 = 16.0;
    let day = info.local.days.iter().find(|x| x.date == d);
    let rec = info.local.archive.iter().find(|x| x.date == d);
    let tokens = day.map(|x| x.tokens).or(rec.map(|x| x.tokens)).unwrap_or(0);
    let usd = day.map(|x| x.usd).or(rec.map(|x| x.usd)).unwrap_or(0.0);
    let requests = day.map(|x| x.requests).or(rec.map(|x| x.requests)).unwrap_or(0);
    let mut models = rec.map(|r| ai::shares_of(std::iter::once(r), true)).unwrap_or_default();
    models.sort_by_key(|m| std::cmp::Reverse(m.tokens));
    let mtotal: u64 = models.iter().map(|m| m.tokens).sum();
    let row = |ui: &mut Ui, k: &str, v: String| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(W, ROW), Sense::hover());
        let p = ui.painter();
        let vr = p.text(r.right_center(), Align2::RIGHT_CENTER, v, ty::label(), TEXT());
        text_fit(p, r.left_center(), Align2::LEFT_CENTER, k, ty::label(), TEXT_DIM(), vr.left() - r.left() - sp::M);
    };
    let rule = |ui: &mut Ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(W, 9.0), Sense::hover());
        ui.painter().line_segment([Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)], Stroke::new(1.0, white(HAIRLINE_STRONG)));
    };
    ui.spacing_mut().item_spacing.y = 2.0;
    let (r, _) = ui.allocate_exact_size(Vec2::new(W, 20.0), Sense::hover());
    text_fit(ui.painter(), r.left_center(), Align2::LEFT_CENTER, &crate::kit::date::day_title(&d), bold(12.5), TEXT(), W);
    row(ui, ts!("ai.wk.used"), format!("+{used:.0}%"));
    row(ui, ts!("ai.wk.week"), format!("{week:.0}%"));
    if tokens > 0 {
        rule(ui);
        row(ui, ts!("ai.kind.tokens"), ai::human_tokens(tokens));
        if settings::get().show_cost && usd > 0.0 {
            row(ui, ts!("ai.cost"), parts::usd(usd));
        }
        if requests > 0 {
            row(ui, ts!("ai.kind.requests"), requests.to_string());
        }
    }
    if mtotal > 0 {
        rule(ui);
        // name, then a 56×4 bar and the share right-aligned in a 32 column
        for m in models.iter().take(3) {
            let share = m.tokens as f32 / mtotal as f32;
            let (r, _) = ui.allocate_exact_size(Vec2::new(W, ROW), Sense::hover());
            let p = ui.painter();
            p.text(r.right_center(), Align2::RIGHT_CENTER, format!("{:.0}%", share * 100.0), ty::label(), TEXT());
            let br = Rect::from_min_size(Pos2::new(r.right() - 32.0 - sp::S - 56.0, r.center().y - 2.0), Vec2::new(56.0, 4.0));
            bar(p, br, share, tint::CLAUDE());
            text_fit(p, r.left_center(), Align2::LEFT_CENTER, &m.name, ty::label(), TEXT_DIM(), br.left() - r.left() - sp::S);
        }
    }
}

/// A row of the multi-account card: name 16, 8, bar 8, 16 between.
const ACC_ROW_H: f32 = 32.0;
const ACC_ROW_GAP: f32 = 12.0;

/// The height of a limit card for `n` accounts.
fn multi_h(n: usize) -> f32 {
    let rows = n as f32 * ACC_ROW_H + (n.saturating_sub(1)) as f32 * ACC_ROW_GAP;
    (2.0 * sp::L + rows).max(LIMIT_H)
}

/// What's left of a limit across accounts, given for each whether it
/// reported any limits and what it has left of this one: the mean over the
/// accounts that reported (one without this limit, e.g. Opus unused, has all
/// of it left), and how many that is. Accounts with no data don't count.
fn mean_left(rows: &[(bool, Option<f32>)]) -> (f32, f32) {
    let vals: Vec<f32> = rows.iter().filter(|(reported, _)| *reported).map(|(_, l)| l.unwrap_or(100.0)).collect();
    let n = vals.len().max(1) as f32;
    (vals.iter().sum::<f32>() / n, n)
}

/// An account row's word when it has no number: Checking, Signed out,
/// Not installed or No data.
fn account_status(a: &UsageInfo) -> &'static str {
    if a.loading || !a.scanned {
        ts!("ai.checking")
    } else if !a.installed {
        ts!("ai.not_installed_short")
    } else if a.signed_out || !a.logged_in {
        ts!("ai.signed_out")
    } else {
        ts!("ai.no_data")
    }
}

/// SP22 for several accounts: on the left the limit across them ("90%
/// left", the mean of what each has left) and when all are full again; on the
/// right a row per account with its folder, what it has left, when it
/// resets, and its bar with the even-use tick.
fn multi_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, label: &str) {
    let p = ui.painter().clone();
    data_card(&p, cr);
    let i = cr.shrink(sp::L);
    let now = parts::now();
    let accs = info.each_account();
    let rows: Vec<(&UsageInfo, Option<&Limit>)> = accs.iter().map(|a| (a, a.limits.iter().find(|l| l.label == label))).collect();
    let known: Vec<&Limit> = rows.iter().filter_map(|r| r.1).collect();
    let (left, _) = mean_left(&rows.iter().map(|(a, l)| (!a.limits.is_empty(), l.map(left_of))).collect::<Vec<_>>());
    let out = known.iter().filter_map(|l| pace::of_limit(l, now).and_then(|pc| pc.out_at)).min();
    // left column
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &backend::short_label(label), ty::title(), TEXT(), LIMIT_LEFT_W);
    let hr = p.text(Pos2::new(i.left(), tr.bottom() + sp::S), Align2::LEFT_TOP, format!("{left:.0}%"), ty::hero(), if out.is_some() { RED() } else { TEXT() });
    p.text(Pos2::new(hr.right() + 6.0, hr.bottom() - 3.0), Align2::LEFT_BOTTOM, ts!("ai.left"), ty::label(), TEXT_DIM());
    // full again once the last used account resets
    let full = known.iter().filter(|l| l.pct > 0.0).filter_map(|l| l.resets_at).max();
    let (cap, cap_col) = if let Some(at) = out {
        (t!("ai.runs_out", when = ai::alerts::when(at, now)).into_owned(), RED())
    } else if let Some(at) = full {
        (t!("ai.full_in", time = until(at)).into_owned(), TEXT_DIM())
    } else {
        (ts!("ai.not_started").to_string(), TEXT_DIM())
    };
    text_fit(&p, Pos2::new(i.left(), hr.bottom() + 6.0), Align2::LEFT_TOP, &cap, ty::label(), cap_col, LIMIT_LEFT_W);
    // right: a row per account
    let bx = i.left() + LIMIT_LEFT_W + sp::L;
    let total = rows.len() as f32 * ACC_ROW_H + (rows.len().saturating_sub(1)) as f32 * ACC_ROW_GAP;
    let mut y = i.center().y - total / 2.0;
    for (a, l) in &rows {
        let rr = Rect::from_min_max(Pos2::new(bx, y), Pos2::new(i.right(), y + ACC_ROW_H));
        let (v, vc, when) = match l {
            Some(l) => {
                let red = pace::of_limit(l, now).and_then(|pc| pc.out_at).is_some();
                let reset = parts::reset_when(l);
                let when = if idle(l) { ts!("ai.not_started").to_string() } else if reset.is_empty() { String::new() } else { t!("ai.resets", when = reset).into_owned() };
                (format!("{:.0}% {}", left_of(l), ts!("ai.left")), if red { RED() } else { TEXT() }, when)
            }
            // a word here (R-minimal); what to do is in the row's hover
            None => (account_status(a).to_string(), TEXT_DIM(), String::new()),
        };
        if l.is_none() {
            if let Some(err) = a.error.as_deref().filter(|e| !e.is_empty()) {
                let _ = ui.interact(rr, ui.scope_id().with(("ai-acc-row", &a.folder, label)), Sense::hover()).on_hover_text(err);
            }
        }
        let wr = if when.is_empty() { Rect::from_min_size(Pos2::new(rr.right(), rr.top()), Vec2::ZERO) } else { p.text(Pos2::new(rr.right(), rr.top()), Align2::RIGHT_TOP, format!(" · {when}"), ty::label(), TEXT_DIM()) };
        let vr = text_fit(&p, Pos2::new(wr.left(), rr.top()), Align2::RIGHT_TOP, &v, bold(12.0), vc, rr.width() * 0.6);
        let nr = text_fit(&p, rr.left_top(), Align2::LEFT_TOP, &a.account, bold(12.0), TEXT_SOFT(), vr.left() - rr.left() - sp::S);
        text_fit(&p, Pos2::new(nr.right() + sp::S, rr.top()), Align2::LEFT_TOP, &a.folder, ty::label(), TEXT_FAINT(), (vr.left() - nr.right() - 2.0 * sp::S).max(0.0));
        let br = Rect::from_min_size(Pos2::new(rr.left(), rr.bottom() - 8.0), Vec2::new(rr.width(), 8.0));
        match l {
            Some(l) => {
                let lf = left_of(l);
                let red = pace::of_limit(l, now).and_then(|pc| pc.out_at).is_some();
                bar(&p, br, (lf / 100.0).max(if lf > 0.0 { 0.01 } else { 0.0 }), if red { RED() } else { tint::CLAUDE() });
                if let Some(pc) = pace::of_limit(l, now) {
                    let x = br.left() + br.width() * (100.0 - pc.expected).clamp(0.0, 100.0) / 100.0;
                    p.line_segment([Pos2::new(x, br.top() - 4.0), Pos2::new(x, br.bottom() + 4.0)], Stroke::new(1.5, TEXT()));
                }
            }
            None => bar(&p, br, 0.0, tint::CLAUDE()),
        }
        y += ACC_ROW_H + ACC_ROW_GAP;
    }
}

/// The limit card's hover: Left, Even pace, Resets, Restores, then when
/// it was updated under a hairline.
fn limit_tip(ui: &mut Ui, l: &Limit, left: f32, even: Option<f32>, updated: &str) {
    use egui::RichText;
    let mut rows: Vec<(&str, String)> = vec![(ts!("ai.pop.left"), format!("{left:.0}%"))];
    if let Some(e) = even {
        rows.push((ts!("ai.pop.even"), t!("ai.pop.even_v", pct = e : {:.0}).into_owned()));
    }
    let reset = parts::reset_when(l);
    if !reset.is_empty() {
        let v = match l.resets_at {
            Some(at) => t!("ai.pop.resets_v", when = reset, time = until(at)).into_owned(),
            None => reset,
        };
        rows.push((ts!("ai.pop.resets"), v));
        if l.pct > 0.0 {
            rows.push((ts!("ai.pop.restores"), format!("+{:.0}%", l.pct)));
        }
    }
    egui::Grid::new("ai-limit-tip").num_columns(2).spacing([sp::M, 6.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).font(ty::label()).color(TEXT_DIM()));
            ui.label(RichText::new(v).font(ty::label()).color(TEXT()));
            ui.end_row();
        }
    });
    if !updated.is_empty() {
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);
        ui.label(RichText::new(updated).font(ty::label()).color(TEXT_FAINT()));
    }
}

/// A provider without plan limits: tokens today, in 7 days and in 5 hours.
fn tokens_card(p: &egui::Painter, cr: Rect, info: &UsageInfo) {
    data_card(p, cr);
    let i = cr.shrink(sp::L);
    let l = &info.local;
    for (k, (lab, v)) in [(ts!("ai.today"), l.tokens_today), (ts!("ai.days_7"), l.tokens_7d), (ts!("ai.hours_5"), l.tokens_5h)].iter().enumerate() {
        let x = i.left() + k as f32 * i.width() / 3.0;
        p.text(Pos2::new(x, i.top()), Align2::LEFT_TOP, ai::human_tokens(*v), ty::number(), TEXT());
        p.text(Pos2::new(x, i.top() + 22.0 + sp::XS), Align2::LEFT_TOP, *lab, ty::label(), TEXT_DIM());
    }
}

// -------------------------------------------------------------- cost/tokens

fn cost_height() -> f32 {
    COST_ROW_H + GAP + KINDS_H + GAP + breakdown_height()
}

fn breakdown_height() -> f32 {
    2.0 * sp::L + 16.0 + sp::M + (BREAKDOWN_ROWS + 1) as f32 * sp::LEGEND_H
}

/// One line on the chart: a provider's buckets over the period.
struct Series {
    name: String,
    color: Color32,
    days: Vec<Day>,
}

/// Every shown provider's buckets for `period`, the same dates for all.
fn series(infos: &[UsageInfo], period: Period) -> Vec<Series> {
    let today = chrono::Local::now().date_naive();
    infos
        .iter()
        .filter(|i| has_usage(i))
        .enumerate()
        .map(|(k, info)| {
            let days = match period {
                // the last 30 days as the logs and history merged them
                Period::D30 => {
                    let grid = parts::buckets(&[], Period::D30, today);
                    grid.into_iter().map(|g| info.local.days.iter().find(|d| d.date == g.date).copied().unwrap_or(g)).collect()
                }
                _ => parts::buckets(&info.local.archive, period, today),
            };
            Series { name: info.display_name(), color: series::monitor(k), days }
        })
        .collect()
}

fn value(d: &Day, m: Measure) -> f64 {
    match m {
        Measure::Tokens => d.tokens as f64,
        Measure::Usd => d.usd,
    }
}

fn fmt(v: f64, m: Measure) -> String {
    match m {
        Measure::Tokens => ai::human_tokens(v as u64),
        Measure::Usd => parts::usd(v),
    }
}

/// Cost or Tokens: the 30 days per provider and the chart over the
/// period (SP18) on top, the breakdown under them.
fn cost_tab(infos: &[UsageInfo], ui: &mut Ui, a: Rect, m: Measure, demo: bool) {
    let _ = demo;
    // each account of a provider is its own line and row
    let flat: Vec<UsageInfo> = infos.iter().flat_map(|i| i.each_account()).collect();
    let infos = flat.as_slice();
    let p = ui.painter().clone();
    let cw = (a.width() - 3.0 * GAP) / 4.0;
    let top = Rect::from_min_size(a.min, Vec2::new(cw, COST_ROW_H));
    let chart = Rect::from_min_max(Pos2::new(top.right() + GAP, a.top()), Pos2::new(a.right(), a.top() + COST_ROW_H));
    let pkey = egui::Id::unique("ai-chart-period");
    let mut period = ui.ctx().data(|d| d.get_temp::<Period>(pkey)).unwrap_or_default();
    let all = series(infos, Period::D30);
    totals_card(&p, top, &all, infos, m);
    // the chart card: title, the period switch, the readout and the lines
    data_card(&p, chart);
    let i = chart.shrink(sp::L);
    let title = match (m, period.grain()) {
        (Measure::Usd, parts::Grain::Day) => ts!("ai.daily_cost"),
        (Measure::Usd, parts::Grain::Week) => ts!("ai.weekly_cost"),
        (Measure::Usd, parts::Grain::Month) => ts!("ai.monthly_cost"),
        (Measure::Tokens, parts::Grain::Day) => ts!("ai.daily_tokens"),
        (Measure::Tokens, parts::Grain::Week) => ts!("ai.weekly_tokens"),
        (Measure::Tokens, parts::Grain::Month) => ts!("ai.monthly_tokens"),
    };
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    let labels: Vec<&str> = Period::EVERY.iter().map(|p| p.short()).collect();
    let pr = Rect::from_min_size(Pos2::new(i.right() - 176.0, i.top() - 6.0), Vec2::new(176.0, sp::CONTROL_H));
    let at = Period::EVERY.iter().position(|x| *x == period).unwrap_or(0);
    if let Some(k) = segmented(ui, pr, pkey.with("seg"), &labels, at) {
        period = Period::EVERY[k];
        ui.ctx().data_mut(|d| d.insert_temp(pkey, period));
    }
    let body = Rect::from_min_max(Pos2::new(i.left(), i.top() + 24.0 + sp::M), i.max);
    let lines = series(infos, period);
    if lines.iter().all(|s| s.days.iter().all(|d| d.tokens == 0)) {
        empty_note(&p, body, if period == Period::D30 { ts!("ai.no_usage_30") } else { ts!("ai.no_usage_period") });
    } else {
        line_chart(ui, body, &lines, m, period.grain());
    }
    // Totals and by type, the last 30 days like the card on the left
    let kinds = infos.iter().filter(|i| has_usage(i)).fold(ai::Kinds::default(), |k, i| k.plus(&i.local.kinds_30d));
    let ky = a.top() + COST_ROW_H + GAP;
    let half = (a.width() - GAP) / 2.0;
    kinds_totals(&p, Rect::from_min_size(Pos2::new(a.left(), ky), Vec2::new(half, KINDS_H)), &all, &kinds);
    by_type(&p, Rect::from_min_size(Pos2::new(a.left() + half + GAP, ky), Vec2::new(half, KINDS_H)), &kinds, &all, m);
    let br = Rect::from_min_size(Pos2::new(a.left(), ky + KINDS_H + GAP), Vec2::new(a.width(), breakdown_height()));
    breakdown(ui, br, infos, &lines, m, period);
}

/// "Totals" of the last 30 days: tokens, cached and uncached input,
/// output, what the cache saved (with dollars on) and requests; three
/// stats to a line.
fn kinds_totals(p: &egui::Painter, r: Rect, all: &[Series], k: &ai::Kinds) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.totals"), ty::title(), TEXT());
    // tokens and requests as the "30 days" card counts them (with the days
    // kept only in the app's history); the kinds come from the logs
    let tokens: u64 = all.iter().flat_map(|s| s.days.iter()).map(|d| d.tokens).sum::<u64>().max(k.tokens());
    let requests: u64 = all.iter().flat_map(|s| s.days.iter()).map(|d| d.requests).sum::<u64>().max(k.requests);
    let mut stats: Vec<(String, &str)> = vec![
        (ai::human_tokens(tokens), ts!("ai.kind.tokens")),
        (ai::human_tokens(k.cache_read), ts!("ai.kind.cached_input")),
        (ai::human_tokens(k.input + k.cache_write), ts!("ai.kind.uncached_input")),
        (ai::human_tokens(k.output), ts!("ai.kind.output")),
    ];
    if settings::get().show_cost {
        stats.push((parts::usd(k.savings), ts!("ai.kind.savings")));
    }
    stats.push((requests.to_string(), ts!("ai.kind.requests")));
    let top = i.top() + 16.0 + sp::M;
    for (n, (v, lab)) in stats.iter().enumerate() {
        let x = i.left() + (n % 3) as f32 * i.width() / 3.0;
        let y = top + (n / 3) as f32 * (22.0 + sp::XS + 14.0 + sp::L);
        let w = i.width() / 3.0 - sp::M;
        text_fit(p, Pos2::new(x, y), Align2::LEFT_TOP, v, ty::number(), TEXT(), w);
        text_fit(p, Pos2::new(x, y + 22.0 + sp::XS), Align2::LEFT_TOP, lab, ty::label(), TEXT_DIM(), w);
    }
}

/// "Cost by type" (or "Tokens by type"): input, cache reads, cache writes
/// and output as one stacked bar, a legend with each under it.
fn by_type(p: &egui::Painter, r: Rect, k: &ai::Kinds, all: &[Series], m: Measure) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    let title = if m == Measure::Usd { ts!("ai.cost_by_type") } else { ts!("ai.tokens_by_type") };
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    let mut parts_: Vec<(&str, f64, f64, Color32)> = vec![
        (ts!("ai.kind.input"), k.input as f64, k.usd_input, series::monitor(2)),
        (ts!("ai.kind.cache_read"), k.cache_read as f64, k.usd_cache_read, with_alpha(series::monitor(0), 0.45)),
        (ts!("ai.kind.cache_write"), k.cache_write as f64, k.usd_cache_write, series::monitor(0)),
        (ts!("ai.kind.output"), k.output as f64, k.usd_output, series::monitor(1)),
    ];
    let val = |t: f64, u: f64| if m == Measure::Usd { u } else { t };
    // days the app kept after the logs were cleaned up have totals but no
    // split: one more part, so the parts add up to the "30 days" card
    let card: f64 = all.iter().flat_map(|s| s.days.iter()).map(|d| value(d, m)).sum();
    let known: f64 = parts_.iter().map(|(_, t, u, _)| val(*t, *u)).sum();
    if card - known > card * 0.005 {
        let rest = card - known;
        parts_.push((ts!("ai.no_details"), rest, rest, white(46)));
    }
    let total: f64 = parts_.iter().map(|(_, t, u, _)| val(*t, *u)).sum();
    let body = Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::M), i.max);
    if total <= 0.0 {
        empty_note(p, body, ts!("ai.no_usage_30"));
        return;
    }
    let segs: Vec<(f32, Color32)> = parts_.iter().map(|(_, t, u, c)| ((val(*t, *u) / total) as f32, *c)).collect();
    stacked_bar(p, Rect::from_min_size(Pos2::new(body.left(), body.top() + sp::S), Vec2::new(body.width(), 8.0)), &segs);
    // the legend: two to a line, three when there's a fifth part
    let ly = body.top() + sp::S + 8.0 + sp::L;
    let per = if parts_.len() > 4 { 3 } else { 2 };
    let cw = body.width() / per as f32;
    for (n, (lab, t, u, c)) in parts_.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(body.left() + (n % per) as f32 * cw - 12.0, ly + (n / per) as f32 * sp::LEGEND_H), Vec2::new(cw + 12.0, sp::LEGEND_H));
        let v = val(*t, *u);
        legend_row(p, lr, *c, lab, &fmt(v, m), TEXT());
    }
}

/// "30 days": the total at API prices (or in tokens), what the plans cost,
/// and an R4 row per provider with its share.
fn totals_card(p: &egui::Painter, r: Rect, all: &[Series], infos: &[UsageInfo], m: Measure) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.days_30"), ty::title(), TEXT());
    let sums: Vec<f64> = all.iter().map(|s| s.days.iter().map(|d| value(d, m)).sum()).collect();
    let total: f64 = sums.iter().sum();
    let hr = text_fit(p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, &fmt(total, m), ty::hero(), TEXT(), i.width());
    let cap = match m {
        Measure::Usd => {
            let plans: f64 = infos.iter().filter(|i| i.billing == Billing::Subscription).filter_map(|i| i.plan_usd()).sum();
            if plans > 0.0 { t!("ai.plans_usd", usd = parts::usd(plans)).into_owned() } else { ts!("ai.api_prices").to_string() }
        }
        Measure::Tokens => {
            let req: u64 = all.iter().flat_map(|s| s.days.iter()).map(|d| d.requests).sum();
            t!("ai.n_requests", n = req).into_owned()
        }
    };
    let cr = text_fit(p, Pos2::new(i.left(), hr.bottom() + sp::XS), Align2::LEFT_TOP, &cap, ty::label(), TEXT_DIM(), i.width());
    let mut y = cr.bottom() + sp::L;
    let topv = sums.iter().copied().fold(0.0, f64::max).max(1e-9);
    for (s, v) in all.iter().zip(&sums) {
        if y + sp::BAR_ROW_H > i.bottom() + 1.0 {
            break;
        }
        let share = if total > 0.0 { v / total * 100.0 } else { 0.0 };
        let dot = Rect::from_min_size(Pos2::new(i.left(), y + 2.0), Vec2::splat(8.0));
        p.rect_filled(dot, 2.0, s.color);
        let vr = p.text(Pos2::new(i.right(), y), Align2::RIGHT_TOP, format!("{} · {share:.0}%", fmt(*v, m)), ty::label(), TEXT());
        text_fit(p, Pos2::new(dot.right() + 6.0, y), Align2::LEFT_TOP, &s.name, ty::label(), TEXT_SOFT(), vr.left() - dot.right() - 6.0 - sp::S);
        bar(p, Rect::from_min_size(Pos2::new(i.left(), y + 20.0), Vec2::new(i.width(), 4.0)), ((v / topv) as f32).max(0.01), s.color);
        y += sp::BAR_ROW_H;
    }
}

/// SP15 lines chart: an area line per provider, the readout above it
/// (the date, each provider, the total), the bucket under the pointer
/// marked (today when none), dashed guides with their values on the right.
fn line_chart(ui: &mut Ui, r: Rect, lines: &[Series], m: Measure, grain: parts::Grain) {
    let p = ui.painter().clone();
    let n = lines.iter().map(|s| s.days.len()).max().unwrap_or(0);
    if n == 0 {
        return;
    }
    let head = 20.0 + 14.0;
    let foot = 16.0;
    let guide_w = 44.0;
    let plot = Rect::from_min_max(Pos2::new(r.left(), r.top() + head + sp::M), Pos2::new(r.right() - guide_w, r.bottom() - foot));
    let top = lines.iter().flat_map(|s| s.days.iter().map(|d| value(d, m))).fold(0.0, f64::max).max(1e-9);
    let x = |k: usize| plot.left() + if n > 1 { plot.width() * k as f32 / (n - 1) as f32 } else { plot.width() / 2.0 };
    let y = |v: f64| plot.bottom() - plot.height() * (v / top) as f32;
    // three dashed guides with their values
    for g in 1..=3 {
        let v = top * g as f64 / 3.0;
        let gy = y(v);
        p.extend(Shape::dashed_line(&[Pos2::new(plot.left(), gy), Pos2::new(plot.right(), gy)], Stroke::new(1.0, with_alpha(TEXT(), 0.16)), 3.0, 4.0));
        p.text(Pos2::new(r.right(), gy), Align2::RIGHT_CENTER, fmt(v, m), ty::caption(), ty::caption_color());
    }
    // areas first, then the lines on top
    for (k, s) in lines.iter().enumerate() {
        let pts: Vec<Pos2> = s.days.iter().enumerate().map(|(j, d)| Pos2::new(x(j), y(value(d, m)))).collect();
        if pts.len() >= 2 {
            let mut mesh = egui::Mesh::default();
            let fill = with_alpha(s.color, if k == 0 { 0.16 } else { 0.10 });
            for w in pts.windows(2) {
                let base = mesh.vertices.len() as u32;
                for q in [w[0], w[1], Pos2::new(w[1].x, plot.bottom()), Pos2::new(w[0].x, plot.bottom())] {
                    mesh.colored_vertex(q, fill);
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            p.add(Shape::mesh(mesh));
        }
    }
    for (k, s) in lines.iter().enumerate() {
        let pts: Vec<Pos2> = s.days.iter().enumerate().map(|(j, d)| Pos2::new(x(j), y(value(d, m)))).collect();
        p.add(Shape::line(pts, Stroke::new(if k == 0 { 2.0 } else { 1.5 }, s.color)));
    }
    // the bucket under the pointer
    let resp = ui.interact(plot.expand2(Vec2::new(0.0, head)), ui.scope_id().with("ai-lines"), Sense::hover());
    let sel = resp.hover_pos().map(|q| if n > 1 { (((q.x - plot.left()) / plot.width() * (n - 1) as f32).round().max(0.0) as usize).min(n - 1) } else { 0 }).unwrap_or(n - 1);
    let sx = x(sel);
    p.line_segment([Pos2::new(sx, plot.top()), Pos2::new(sx, plot.bottom())], Stroke::new(1.0, with_alpha(TEXT(), 0.5)));
    for s in lines {
        if let Some(d) = s.days.get(sel) {
            p.circle_filled(Pos2::new(sx, y(value(d, m))), 3.5, s.color);
        }
    }
    // readout: the date, each provider, the total
    let date = lines.iter().find_map(|s| s.days.get(sel)).map(|d| d.date).unwrap_or_default();
    let last = sel + 1 == n;
    let (when, cap) = bucket_label(date, last, grain);
    let mut stats: Vec<(String, String)> = vec![(when, cap.to_string())];
    let mut total = 0.0;
    for s in lines {
        let v = s.days.get(sel).map(|d| value(d, m)).unwrap_or(0.0);
        total += v;
        stats.push((fmt(v, m), s.name.clone()));
    }
    if lines.len() > 1 {
        stats.push((fmt(total, m), ts!("ai.total").to_string()));
    }
    let mut xx = r.left();
    for (v, c) in stats {
        let vr = p.text(Pos2::new(xx, r.top()), Align2::LEFT_TOP, v, ty::lead(), TEXT());
        let cr = p.text(Pos2::new(xx, r.top() + 20.0), Align2::LEFT_TOP, c, ty::caption(), TEXT_DIM());
        xx = vr.right().max(cr.right()) + sp::XL;
    }
    // first and last bucket under the plot
    let first = lines.iter().find_map(|s| s.days.first()).map(|d| d.date).unwrap_or_default();
    p.text(Pos2::new(plot.left(), r.bottom()), Align2::LEFT_BOTTOM, bucket_label(first, false, grain).0, ty::caption(), ty::caption_color());
    p.text(Pos2::new(plot.right(), r.bottom()), Align2::RIGHT_BOTTOM, bucket_label(first, true, grain).0, ty::caption(), ty::caption_color());
}

/// A bucket's name ("Fri 21 Nov", "Week of Nov 17", "Oct 2026"; "Today",
/// "This week", "This month" for the last) and its caption (Day, Week, Month).
fn bucket_label(d: NaiveDate, last: bool, grain: parts::Grain) -> (String, &'static str) {
    use parts::Grain;
    match (grain, last) {
        (Grain::Day, true) => (ts!("ai.today").into(), ts!("ai.cap.day")),
        (Grain::Day, false) => (crate::kit::date::wmd(&d), ts!("ai.cap.day")),
        (Grain::Week, true) => (ts!("ai.this_week").into(), ts!("ai.cap.week")),
        (Grain::Week, false) => (t!("ai.week_of", date = crate::kit::date::md(&d)).into_owned(), ts!("ai.cap.week")),
        (Grain::Month, true) => (ts!("ai.this_month").into(), ts!("ai.cap.month")),
        (Grain::Month, false) => (t!("ai.month_year", mon = crate::kit::date::mon(&d), year = d.year()).into_owned(), ts!("ai.cap.month")),
    }
}

/// What the breakdown lists.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum By {
    #[default]
    Model,
    Project,
    Day,
}

/// Models, projects or buckets of the period as W5 lines: name, bar, the
/// measure, its share, the other measure (when cost shows).
fn breakdown(ui: &mut Ui, r: Rect, infos: &[UsageInfo], lines: &[Series], m: Measure, period: Period) {
    let p = ui.painter().clone();
    data_card(&p, r);
    let i = r.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.breakdown"), ty::title(), TEXT());
    let key = egui::Id::unique("ai-breakdown");
    let mut by = ui.ctx().data(|d| d.get_temp::<By>(key)).unwrap_or_default();
    let labels = [ts!("ai.by.model"), ts!("ai.by.project"), ts!("ai.by.day")];
    let sw = tabs_width(&p, &labels).min(i.width() / 2.0);
    let sr = Rect::from_min_size(Pos2::new(i.right() - sw, i.top() - 6.0), Vec2::new(sw, sp::CONTROL_H));
    if let Some(k) = segmented(ui, sr, key.with("seg"), &labels, by as usize) {
        by = [By::Model, By::Project, By::Day][k];
        ui.ctx().data_mut(|d| d.insert_temp(key, by));
    }
    let today = chrono::Local::now().date_naive();
    let rows: Vec<Share> = match by {
        By::Model | By::Project => {
            let mut all: Vec<Share> = Vec::new();
            // days kept from before models and projects were saved, or whose
            // logs are gone, have totals without the split: they show as one
            // row so the rows add up to the period's total
            let mut rest = Share { name: ts!("ai.no_details").into(), ..Default::default() };
            for info in infos {
                for r in parts::period_days(&info.local.archive, period, today) {
                    let split = if by == By::Model { &r.models } else { &r.projects };
                    rest.tokens += r.tokens.saturating_sub(split.iter().map(|s| s.tokens).sum());
                    rest.usd += (r.usd - split.iter().map(|s| s.usd).sum::<f64>()).max(0.0);
                }
                for s in ai::shares_of(parts::period_days(&info.local.archive, period, today), by == By::Model) {
                    match all.iter_mut().find(|x| x.name == s.name) {
                        Some(x) => {
                            x.tokens += s.tokens;
                            x.usd += s.usd;
                        }
                        None => all.push(s),
                    }
                }
            }
            if rest.tokens > 0 || rest.usd >= 0.01 {
                all.push(rest);
            }
            all
        }
        By::Day => {
            let n = lines.iter().map(|s| s.days.len()).max().unwrap_or(0);
            (0..n)
                .rev()
                .filter_map(|k| {
                    let date = lines.iter().find_map(|s| s.days.get(k))?.date;
                    let (tokens, usd) = lines.iter().filter_map(|s| s.days.get(k)).fold((0, 0.0), |(t, u), d| (t + d.tokens, u + d.usd));
                    (tokens > 0).then(|| Share { name: bucket_label(date, k + 1 == n, period.grain()).0, tokens, usd })
                })
                .collect()
        }
    };
    let measure = |s: &Share| match m {
        Measure::Tokens => s.tokens as f64,
        Measure::Usd => s.usd,
    };
    let mut rows = rows;
    if by != By::Day {
        rows.sort_by(|a, b| measure(b).total_cmp(&measure(a)).then(a.name.cmp(&b.name)));
    }
    let list = Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::M), i.max);
    if rows.is_empty() {
        empty_note(&p, list, ts!("ai.no_usage_period"));
        return;
    }
    let show_usd = settings::get().show_cost;
    let total: f64 = rows.iter().map(measure).sum::<f64>().max(1e-9);
    let topv = rows.iter().map(measure).fold(0.0, f64::max).max(1e-9);
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    let paths: Vec<&str> = rows.iter().take(BREAKDOWN_ROWS).map(|s| s.name.as_str()).collect();
    let names = if by == By::Project { parts::project_names(&paths, &home) } else { paths.iter().map(|s| s.to_string()).collect() };
    // columns: name, bar, the measure, share, the other measure
    let (c1, c2, c3) = (72.0, 48.0, 64.0);
    let other = match m {
        Measure::Usd => Some(ts!("ai.unit_tokens")),
        Measure::Tokens if show_usd => Some(ts!("ai.cost")),
        Measure::Tokens => None,
    };
    let x3 = i.right();
    let x2 = if other.is_some() { x3 - c3 - sp::M } else { x3 };
    let x1 = x2 - c2 - sp::M;
    let name_w = (i.width() * 0.3).min(220.0);
    let bar_l = i.left() + name_w + sp::M;
    let bar_r = x1 - c1 - sp::M;
    let row = |k: usize| Rect::from_min_size(Pos2::new(i.left(), list.top() + k as f32 * sp::LEGEND_H), Vec2::new(i.width(), sp::LEGEND_H));
    let hr = row(0);
    let first = if m == Measure::Usd { ts!("ai.cost") } else { ts!("ai.unit_tokens") };
    p.text(Pos2::new(x1, hr.center().y), Align2::RIGHT_CENTER, first, ty::label(), TEXT_FAINT());
    p.text(Pos2::new(x2, hr.center().y), Align2::RIGHT_CENTER, ts!("ai.col.share"), ty::label(), TEXT_FAINT());
    if let Some(o) = other {
        p.text(Pos2::new(x3, hr.center().y), Align2::RIGHT_CENTER, o, ty::label(), TEXT_FAINT());
    }
    for (k, s) in rows.iter().take(BREAKDOWN_ROWS).enumerate() {
        let rr = row(k + 1);
        let cy = rr.center().y;
        text_fit(&p, Pos2::new(rr.left(), cy), Align2::LEFT_CENTER, &names[k], ty::label(), TEXT_SOFT(), name_w);
        if bar_r > bar_l + 8.0 {
            bar(&p, Rect::from_min_max(Pos2::new(bar_l, cy - 2.0), Pos2::new(bar_r, cy + 2.0)), ((measure(s) / topv) as f32).max(0.01), series::monitor(0));
        }
        p.text(Pos2::new(x1, cy), Align2::RIGHT_CENTER, fmt(measure(s), m), ty::label(), TEXT());
        p.text(Pos2::new(x2, cy), Align2::RIGHT_CENTER, format!("{:.0}%", measure(s) / total * 100.0), ty::label(), TEXT_DIM());
        if other.is_some() {
            let o = match m {
                Measure::Usd => ai::human_tokens(s.tokens),
                Measure::Tokens => parts::usd(s.usd),
            };
            p.text(Pos2::new(x3, cy), Align2::RIGHT_CENTER, o, ty::label(), TEXT_DIM());
        }
    }
}

// ------------------------------------------------------------------ states

/// The page when the assistant is missing or signed out.
fn not_set_up(a: Availability, cl: &UsageInfo, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let m = ai::meta(cl.provider);
    let colw = 480.0f32.min(r.width() - 48.0);
    let x = r.center().x - colw / 2.0;
    let mut y = r.top() + PAGE_HEADER_H + 56.0;
    tool_chip(&p, Rect::from_center_size(Pos2::new(r.center().x, y + 28.0), Vec2::splat(56.0)), Icon::Gauge, tint::CLAUDE());
    y += 56.0 + 20.0;
    let title = if a == Availability::SignedOut { t!("ai.not_signed_in_title", name = m.name).into_owned() } else { t!("ai.not_installed_title", name = m.name).into_owned() };
    p.text(Pos2::new(r.center().x, y), Align2::CENTER_TOP, title, bold(22.0), TEXT());
    y += 32.0 + 12.0;
    let steps = if a == Availability::SignedOut { m.sign_in_steps() } else { m.install_steps() };
    let card = Rect::from_min_size(Pos2::new(x, y), Vec2::new(colw, 12.0 + steps.len() as f32 * 34.0));
    data_card(&p, card);
    for (i, s) in steps.iter().enumerate() {
        let cy = card.top() + 6.0 + 17.0 + i as f32 * 34.0;
        let c = Pos2::new(card.left() + 24.0, cy);
        p.circle_filled(c, 10.0, white(18));
        p.text(c, Align2::CENTER_CENTER, format!("{}", i + 1), bold(11.0), TEXT());
        text_fit(&p, Pos2::new(card.left() + 44.0, cy), Align2::LEFT_CENTER, s, font(13.0), TEXT_SOFT(), colw - 58.0);
    }
    y = card.bottom() + 20.0;
    let primary = ts!("ai.check_again");
    let get = t!("ai.get", name = if m.id == "claude" { "Claude Code" } else { m.name }).into_owned();
    let second = if a == Availability::SignedOut { ts!("ai.open_terminal") } else { get.as_str() };
    let pw = button_width(&p, primary, BTN_PRIMARY);
    let sw = button_width(&p, second, BTN_SECONDARY);
    let left = r.center().x - (pw + 8.0 + sw) / 2.0;
    let sr = Rect::from_min_size(Pos2::new(left, y), Vec2::new(sw, 32.0));
    let pr = Rect::from_min_size(Pos2::new(sr.right() + 8.0, y), Vec2::new(pw, 32.0));
    if pill_button(ui, sr, second, BTN_SECONDARY, true).clicked() {
        if a == Availability::SignedOut {
            cc::open_terminal();
        } else {
            let _ = std::process::Command::new("open").arg(m.install_url).spawn();
        }
    }
    if pill_button(ui, pr, primary, BTN_PRIMARY, true).clicked() {
        backend::request_refresh();
        cx.toast(t!("ai.looking_for", name = m.name));
    }
}


/// A card's body with nothing in it: one quiet line in the middle.
fn empty_note(p: &egui::Painter, r: Rect, msg: &str) {
    text_fit(p, Pos2::new(r.center().x, r.center().y - 8.0), Align2::CENTER_CENTER, msg, ty::body(), TEXT_FAINT(), r.width());
}

/// Limits, or any tokens in the last 30 days.
fn has_usage(cl: &UsageInfo) -> bool {
    !cl.limits.is_empty() || cl.local.days.iter().any(|d| d.tokens > 0) || !cl.local.models.is_empty()
}

/// The centred empty state (R6): chip, title, a dim body, and under it
/// either a spinner (`None`) or one secondary button. True when it's clicked.
fn empty_state(ui: &mut Ui, r: Rect, title: &str, msg: &str, button: Option<&str>) -> bool {
    let p = ui.painter().clone();
    let c = Pos2::new(r.center().x, r.top() + PAGE_HEADER_H + sp::XXL + (r.height() - PAGE_HEADER_H) * 0.3);
    tool_chip(&p, Rect::from_center_size(c, Vec2::splat(56.0)), Icon::Gauge, tint::CLAUDE());
    p.text(c + Vec2::new(0.0, 52.0), Align2::CENTER_CENTER, title, bold(15.0), TEXT());
    let w = (r.width() - 48.0).min(400.0);
    let mut job = egui::text::LayoutJob::simple(msg.to_string(), ty::body(), TEXT_DIM(), w);
    job.halign = egui::Align::Center;
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(c + Vec2::new(0.0, 74.0), g, TEXT_DIM());
    let below = Pos2::new(c.x, c.y + 74.0 + h + 30.0);
    let Some(label) = button else {
        spinner(&p, below, 8.0, ui.ctx().input(|i| i.time), TEXT_DIM());
        crate::kit::glass::poll(ui.ctx());
        return false;
    };
    let bw = button_width(&p, label, BTN_SECONDARY);
    let br = Rect::from_center_size(below, Vec2::new(bw, 30.0));
    pill_button(ui, br, label, BTN_SECONDARY, !backend::refreshing()).clicked()
}

/// The first check after launch is still running.
fn checking(cl: &UsageInfo, ui: &mut Ui, r: Rect) {
    let m = ai::meta(cl.provider);
    empty_state(ui, r, &t!("ai.checking_name", name = m.name), ts!("ai.reading_logs"), None);
}

/// A provider that's set up but hasn't been used on this Mac yet. While a
/// check runs the button says so and waits; the page stays as it is.
fn no_usage(cl: &UsageInfo, ui: &mut Ui, r: Rect) {
    let m = ai::meta(cl.provider);
    let msg = ts!("ai.no_usage_msg");
    let label = if cl.loading || backend::refreshing() { ts!("ai.checking") } else { ts!("ai.check_again") };
    if empty_state(ui, r, &t!("ai.no_usage_yet", name = m.name), msg, Some(label)) {
        backend::request_refresh();
    }
}


fn budget(cl: &UsageInfo) -> Option<f64> {
    settings::get().budgets.get(cl.provider).copied().filter(|b| *b > 0.0)
}

fn days_in_month() -> u32 {
    let t = chrono::Local::now().date_naive();
    let next = if t.month() == 12 { chrono::NaiveDate::from_ymd_opt(t.year() + 1, 1, 1) } else { chrono::NaiveDate::from_ymd_opt(t.year(), t.month() + 1, 1) };
    next.and_then(|n| n.pred_opt()).map(|d| d.day()).unwrap_or(30)
}

/// An API key: this month's spend against the budget, with pace and when
/// it runs out. The budget is typed right on the card.
fn budget_card(ui: &mut Ui, r: Rect, cl: &UsageInfo) {
    let p = ui.painter().clone();
    data_card(&p, r);
    let i = r.shrink(sp::L);
    let today = chrono::Local::now().date_naive();
    let spent = cl.local.usd_month;
    let b = budget(cl);
    p.text(i.left_top(), Align2::LEFT_TOP, t!("ai.month_budget", month = crate::kit::date::month(&today)), ty::title(), TEXT());
    // the budget field, top right
    let fr = Rect::from_min_max(Pos2::new(i.right() - 84.0, i.top() + 8.0 - sp::CONTROL_H / 2.0), Pos2::new(i.right(), i.top() + 8.0 + sp::CONTROL_H / 2.0));
    let id = ui.scope_id().with(("ai-budget", cl.provider));
    let mut text = ui.ctx().data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| b.map(|v| format!("{v:.0}")).unwrap_or_default());
    let resp = text_input(ui, fr, id.with("field"), &mut text, ts!("ai.budget"), FieldStyle { right: true, ..Default::default() });
    p.text(Pos2::new(fr.left() + 10.0, fr.center().y), Align2::LEFT_CENTER, "$", ty::body(), TEXT_DIM());
    if resp.changed() {
        ui.ctx().data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if resp.lost_focus() {
        let v = text.trim().trim_start_matches('$').parse::<f64>().unwrap_or(0.0).max(0.0);
        let name = cl.provider.to_string();
        settings::update(true, |s| {
            if v > 0.0 {
                s.budgets.insert(name, v);
            } else {
                s.budgets.remove(&name);
            }
        });
        ui.ctx().data_mut(|d| d.remove::<String>(id));
    }
    let hr = text_fit(&p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, &parts::usd(spent), ty::hero(), TEXT(), i.width() / 2.0);
    let dim = days_in_month();
    let day = today.day();
    let Some(b) = b else { return };
    let start = chrono::Local::now().timestamp() - (day as i64 - 1) * 86_400;
    let pc = ai::pace::of((spent / b * 100.0) as f32, start, start + dim as i64 * 86_400, chrono::Local::now().timestamp());
    if let Some(pc) = pc {
        let (s, c) = match pc.state {
            ai::pace::State::Ahead => (ts!("ai.pace.ahead"), RED()),
            ai::pace::State::OnPace => (ts!("ai.pace.on"), TEXT()),
            ai::pace::State::Room => (ts!("ai.pace.room"), GREEN()),
        };
        p.text(Pos2::new(hr.right() + sp::M, hr.bottom() - 4.0), Align2::LEFT_BOTTOM, s, ty::title(), c);
    }
    let br = Rect::from_min_size(Pos2::new(i.left(), hr.bottom() + sp::M), Vec2::new(i.width(), 8.0));
    let frac = (spent / b) as f32;
    bar(&p, br, frac.clamp(0.01, 1.0), if frac >= 0.9 { RED() } else if frac >= 0.7 { AMBER() } else { tint::CLAUDE() });
    let x = br.left() + br.width() * day as f32 / dim as f32;
    p.line_segment([Pos2::new(x, br.top() - 3.0), Pos2::new(x, br.bottom() + 3.0)], egui::Stroke::new(1.5, TEXT()));
    p.text(i.left_bottom(), Align2::LEFT_BOTTOM, t!("ai.day_of", day = day, dim = dim), ty::label(), TEXT_DIM());
    let per_day = spent / day.max(1) as f64;
    let by_end = per_day * dim as f64;
    let mut f = t!("ai.by_end", usd = parts::usd(by_end), date = t!("kit.date.md", mon = crate::kit::date::mon(&today), day = dim)).into_owned();
    if per_day > 0.0 && by_end > b {
        let out_day = (b / per_day).ceil() as u32;
        f = t!("ai.budget_out", month = crate::kit::date::mon(&today), day = out_day, rest = f).into_owned();
    }
    text_fit(&p, i.right_bottom(), Align2::RIGHT_BOTTOM, &f, ty::label(), if by_end > b { RED() } else { TEXT_DIM() }, i.width() * 0.65);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_days_cover_every_date_of_a_window_off_midnight() {
        use chrono::TimeZone;
        let today = chrono::Local::now().date_naive();
        let reset = chrono::Local.from_local_datetime(&(today + chrono::Days::new(3)).and_hms_opt(5, 0, 0).unwrap()).earliest().unwrap().timestamp();
        let mut info = UsageInfo::default();
        info.local.days = vec![Day { date: today, tokens: 100, ..Default::default() }];
        let l = Limit { label: "Current week (all models)".into(), pct: 40.0, resets_at: Some(reset), window_secs: 7 * 86_400, ..Default::default() };
        let (start, days) = week_days(&info, &l).unwrap();
        // 05:00 four days ago to 05:00 in three: 8 dates, the last one to come
        assert_eq!(start, reset - 7 * 86_400);
        assert_eq!(days.len(), 8);
        assert_eq!(days.first().unwrap().0, today - chrono::Days::new(4));
        assert_eq!(days.last().unwrap(), &(today + chrono::Days::new(3), None));
    }

    #[test]
    fn no_week_card_without_tokens_since_the_reset() {
        let today = chrono::Local::now().date_naive();
        let mut info = UsageInfo::default();
        info.local.days = (0..30).map(|k| Day { date: today - chrono::Days::new(29 - k), ..Default::default() }).collect();
        // tokens 20 days ago only, the week reset 3 days ago
        info.local.days[9].tokens = 1000;
        let l = Limit { label: "Current week (all models)".into(), pct: 40.0, resets_at: Some(parts::now() + 4 * 86_400), ..Default::default() };
        assert!(week_days(&info, &l).is_none());
        info.local.days[29].tokens = 500;
        let (_, days) = week_days(&info, &l).unwrap();
        assert_eq!(days.iter().filter_map(|d| d.1).sum::<f32>().round(), 40.0);
    }

    #[test]
    fn the_mean_skips_accounts_without_data() {
        // Personal 50% left, Work signed out: 50%, not 75%
        assert_eq!(mean_left(&[(true, Some(50.0)), (false, None)]), (50.0, 1.0));
        // Work reported but never used Opus: all of it left
        assert_eq!(mean_left(&[(true, Some(50.0)), (true, None)]), (75.0, 2.0));
    }

    #[test]
    fn accounts_have_their_own_names() {
        let mut main = UsageInfo::new(ai::claude::NAME);
        assert_eq!(main.display_name(), "Claude");
        main.account = "Personal".into();
        main.accounts = vec![UsageInfo { account: "Work".into(), ..UsageInfo::new(ai::claude::NAME) }];
        let each: Vec<String> = main.each_account().iter().map(|i| i.display_name()).collect();
        assert_eq!(each, vec!["Claude · Personal", "Claude · Work"]);
        assert!(main.each_account()[0].accounts.is_empty());
    }
}
