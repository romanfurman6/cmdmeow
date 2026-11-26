//! The AI usage page in the full window. A header with the provider menu
//! (All providers, or one), the tips chip and three tabs. Limits (first):
//! a section per provider, a card per limit with how much is left, when it
//! comes back and the even-pace tick; hover shows the details. Cost and
//! Tokens: the last 30 days per provider, the usage chart (a line per
//! provider) and the breakdown by model, project or day.

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
/// A provider's section row (glyph 16, name, plan, outage chip).
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

/// The page's tabs.
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

fn tabs(show_usd: bool) -> Vec<(Tab, &'static str)> {
    let mut v = vec![(Tab::Limits, ts!("ai.limits"))];
    if show_usd {
        v.push((Tab::Cost, ts!("ai.cost")));
    }
    v.push((Tab::Tokens, ts!("ai.unit_tokens")));
    v
}

/// The providers on the page: the one picked, or every one that's on
/// (tools that aren't installed get no section).
fn shown(all: &All) -> Vec<UsageInfo> {
    if ai::shows_all() {
        backend::enabled(all).into_iter().filter(|i| i.unavailable() != Some(Availability::NotInstalled)).collect()
    } else {
        vec![backend::current(all).lock_ok().clone()]
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
/// the edge ↻ Refresh, 12, the tabs, 12, the tips chip. Returns the tab shown.
#[allow(clippy::too_many_arguments)]
fn header(all: &All, infos: &[UsageInfo], check: &mut Check, ui: &mut Ui, r: Rect, (sub, sub_col): (&str, Color32), tip: &str, demo: bool, cx: &mut Cx) -> Tab {
    let p = ui.painter().clone();
    let show_usd = settings::get().show_cost;
    let tabs = tabs(show_usd);
    let labels: Vec<&str> = tabs.iter().map(|t| t.1).collect();
    let tw = tabs_width(&p, &labels);
    let rw = icon_pill_width(&p, ts!("ai.refresh"), BTN_SECONDARY);
    let has_data = infos.iter().any(has_usage);
    let cw = aicheck::ui::chip_width(check, ui, has_data);
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
    let at_tab = tabs.iter().position(|t| t.0 == cur).unwrap_or(0);
    if let Some(k) = segmented(ui, tr, ui.scope_id().with("ai-tab"), &labels, at_tab) {
        set_tab(ui.ctx(), tabs[k].0);
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
    if !info.limits.is_empty() {
        return info.limits.iter().map(Block::Limit).collect();
    }
    if meta.limits && (info.loading || info.error.is_some()) {
        return vec![Block::Note(info.error.clone().unwrap_or_else(|| t!("ai.asking_limits", provider = info.provider).into_owned()))];
    }
    vec![Block::Tokens]
}

fn block_h(b: &Block) -> f32 {
    match b {
        Block::Limit(_) => LIMIT_H,
        Block::Note(_) => NOTE_H,
        Block::Tokens => TOKENS_H,
        Block::Budget => BUDGET_H,
    }
}

fn limits_height(infos: &[UsageInfo]) -> f32 {
    let mut h = 0.0;
    for (k, info) in infos.iter().enumerate() {
        if k > 0 {
            h += sp::XL;
        }
        h += SECTION_H;
        for b in blocks(info) {
            h += GAP + block_h(&b);
        }
    }
    h
}

/// Every provider shown: its name row, then a card per limit.
fn limits_tab(infos: &[UsageInfo], check: &mut Check, ui: &mut Ui, a: Rect) {
    let p = ui.painter().clone();
    let mut y = a.top();
    for (k, info) in infos.iter().enumerate() {
        if k > 0 {
            y += sp::XL;
        }
        section_row(ui, Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), SECTION_H)), info);
        y += SECTION_H;
        for b in blocks(info) {
            y += GAP;
            let cr = Rect::from_min_size(Pos2::new(a.left(), y), Vec2::new(a.width(), block_h(&b)));
            match b {
                Block::Limit(l) => {
                    let key = limit_key(info, l);
                    if check.take_scroll(&key) {
                        ui.scroll_to_rect(cr, Some(egui::Align::Center));
                    }
                    let ring = check.ring(ui.ctx(), &key);
                    limit_card(ui, cr, info, l, ring);
                }
                Block::Note(msg) => {
                    data_card(&p, cr);
                    text_fit(&p, Pos2::new(cr.left() + sp::L, cr.center().y), Align2::LEFT_CENTER, &msg, ty::body(), TEXT_DIM(), cr.width() - 2.0 * sp::L);
                }
                Block::Tokens => tokens_card(&p, cr, info),
                Block::Budget => budget_card(ui, cr, info),
            }
            y += cr.height();
        }
    }
}

/// A provider's name row: its mark, name, plan (dim) and outage chip.
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
    let x = if plan.is_empty() { t.right() } else { p.text(Pos2::new(t.right() + sp::S, r.center().y), Align2::LEFT_CENTER, plan, ty::body(), TEXT_DIM()).right() };
    incident(ui, Pos2::new(x + sp::S, r.center().y), Align2::LEFT_CENTER, info);
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

/// SP22: one plan limit as a card. Left: its name, "48% left", and when
/// it comes back ("↻ +52% in 2d 14h"; red "Runs out 16:10" when this pace
/// empties it first). Right: when it resets over an 8 pt bar of what's
/// left, with a tick where even use would leave it by now. Hover: the
/// details (P1 popover).
fn limit_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, l: &Limit, ring: f32) {
    let p = ui.painter().clone();
    data_card(&p, cr);
    if ring > 0.0 {
        p.rect_stroke(cr, R_CARD, Stroke::new(2.0, with_alpha(AI(), ring)), egui::StrokeKind::Inside);
    }
    let i = cr.shrink(sp::L);
    let now = parts::now();
    let pc = pace::of_limit(l, now);
    let out = pc.as_ref().and_then(|p| p.out_at);
    let left = left_of(l);
    // left column
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &backend::short_label(&l.label), ty::title(), TEXT(), LIMIT_LEFT_W);
    let hero_col = if out.is_some() { RED() } else { TEXT() };
    let hr = p.text(Pos2::new(i.left(), tr.bottom() + sp::S), Align2::LEFT_TOP, format!("{left:.0}%"), ty::hero(), hero_col);
    p.text(Pos2::new(hr.right() + 6.0, hr.bottom() - 3.0), Align2::LEFT_BOTTOM, ts!("ai.left"), ty::label(), TEXT_DIM());
    let (cap, cap_col) = if idle(l) {
        (ts!("ai.not_started").to_string(), TEXT_DIM())
    } else if let Some(at) = out {
        (t!("ai.runs_out", when = ai::alerts::when(at, now)).into_owned(), RED())
    } else if let Some(at) = l.resets_at {
        (t!("ai.restores_in", pct = l.pct : {:.0}, time = until(at)).into_owned(), TEXT_DIM())
    } else {
        (parts::forecast(l), TEXT_DIM())
    };
    text_fit(&p, Pos2::new(i.left(), hr.bottom() + 6.0), Align2::LEFT_TOP, &cap, ty::label(), cap_col, LIMIT_LEFT_W);
    // right: reset over the bar
    let bx = i.left() + LIMIT_LEFT_W + sp::L;
    let br = Rect::from_min_max(Pos2::new(bx, i.center().y + 16.0 - 8.0), Pos2::new(i.right(), i.center().y + 16.0));
    let reset = parts::reset_when(l);
    let head = if !reset.is_empty() {
        t!("ai.resets", when = reset).into_owned()
    } else if l.window_secs > 0 {
        t!("ai.window_h", h = l.window_secs / 3600).into_owned()
    } else {
        String::new()
    };
    text_fit(&p, Pos2::new(br.right(), br.top() - 10.0), Align2::RIGHT_BOTTOM, &head, ty::label(), TEXT_DIM(), br.width());
    let fill = if out.is_some() { RED() } else { tint::CLAUDE() };
    bar(&p, br, (left / 100.0).max(if left > 0.0 { 0.01 } else { 0.0 }), fill);
    let even = pc.as_ref().map(|pc| (100.0 - pc.expected).clamp(0.0, 100.0));
    if let Some(e) = even {
        let x = br.left() + br.width() * e / 100.0;
        p.line_segment([Pos2::new(x, br.top() - 4.0), Pos2::new(x, br.bottom() + 4.0)], Stroke::new(1.5, TEXT()));
    }
    // hover: the details
    let resp = ui.interact(cr, ui.scope_id().with(("ai-limit", info.provider, &l.label)), Sense::hover());
    let mut tip = egui::Tooltip::for_enabled(&resp);
    tip.popup = tip.popup.align(egui::RectAlign::BOTTOM_END).gap(6.0);
    let updated = info.fetched.map(|f| t!("ai.updated", time = f.format("%H:%M")).into_owned()).unwrap_or_default();
    tip.show(|ui| limit_tip(ui, l, left, even, &updated));
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
            Series { name: ai::meta(info.provider).name.to_string(), color: series::monitor(k), days }
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
    by_type(&p, Rect::from_min_size(Pos2::new(a.left() + half + GAP, ky), Vec2::new(half, KINDS_H)), &kinds, m);
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
    let tokens: u64 = all.iter().flat_map(|s| s.days.iter()).map(|d| d.tokens).sum();
    let requests: u64 = all.iter().flat_map(|s| s.days.iter()).map(|d| d.requests).sum();
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
fn by_type(p: &egui::Painter, r: Rect, k: &ai::Kinds, m: Measure) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    let title = if m == Measure::Usd { ts!("ai.cost_by_type") } else { ts!("ai.tokens_by_type") };
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    let parts_: [(&str, f64, f64, Color32); 4] = [
        (ts!("ai.kind.input"), k.input as f64, k.usd_input, series::monitor(2)),
        (ts!("ai.kind.cache_read"), k.cache_read as f64, k.usd_cache_read, with_alpha(series::monitor(0), 0.45)),
        (ts!("ai.kind.cache_write"), k.cache_write as f64, k.usd_cache_write, series::monitor(0)),
        (ts!("ai.kind.output"), k.output as f64, k.usd_output, series::monitor(1)),
    ];
    let val = |t: f64, u: f64| if m == Measure::Usd { u } else { t };
    let total: f64 = parts_.iter().map(|(_, t, u, _)| val(*t, *u)).sum();
    let body = Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::M), i.max);
    if total <= 0.0 {
        empty_note(p, body, ts!("ai.no_usage_30"));
        return;
    }
    let segs: Vec<(f32, Color32)> = parts_.iter().map(|(_, t, u, c)| ((val(*t, *u) / total) as f32, *c)).collect();
    stacked_bar(p, Rect::from_min_size(Pos2::new(body.left(), body.top() + sp::S), Vec2::new(body.width(), 8.0)), &segs);
    // the legend: two to a line
    let ly = body.top() + sp::S + 8.0 + sp::L;
    let cw = body.width() / 2.0;
    for (n, (lab, t, u, c)) in parts_.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(body.left() + (n % 2) as f32 * cw - 12.0, ly + (n / 2) as f32 * sp::LEGEND_H), Vec2::new(cw + 12.0, sp::LEGEND_H));
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


/// An outage chip (the provider's status page, not the person's usage).
/// Hover shows the open incidents (name, state and since when, what it
/// touches); a click opens the incident on the status page.
fn incident(ui: &mut Ui, pos: Pos2, align: Align2, cl: &UsageInfo) -> Option<Rect> {
    let p = ui.painter().clone();
    let r = parts::incident_chip(&p, pos, align, cl)?;
    let m = ai::meta(cl.provider);
    let host = m.status_url.trim_start_matches("https://").split('/').next().unwrap_or("").to_owned();
    let resp = ui.interact(r, ui.scope_id().with(("ai-incident", cl.provider)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        let link = cl.incidents.iter().map(|i| i.link.as_str()).find(|l| l.starts_with("https://")).map(str::to_owned).unwrap_or_else(|| format!("https://{host}"));
        let _ = std::process::Command::new("open").arg(link).spawn();
    }
    let level = if cl.incident.as_deref() == Some("Outage") { RED() } else { AMBER() };
    let mut t = egui::Tooltip::for_enabled(&resp);
    t.popup = t.popup.align(egui::RectAlign::BOTTOM).gap(8.0);
    t.show(|ui| incident_tip(ui, &cl.incidents, &host, level));
    Some(r)
}

/// The badge's hover: each open incident split by a hairline, then the
/// status page's host.
fn incident_tip(ui: &mut Ui, incidents: &[ai::status::Incident], host: &str, level: egui::Color32) {
    use egui::RichText;
    ui.set_width(300.0 - 2.0 * sp::M);
    ui.spacing_mut().item_spacing.y = 4.0;
    for (n, i) in incidents.iter().enumerate() {
        if n > 0 {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        ui.add(egui::Label::new(RichText::new(&i.name).font(bold(13.0)).color(TEXT())).wrap());
        let state = parts::incident_state(&i.state);
        let since = i.since.map(parts::incident_since);
        if state.is_some() || since.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if let Some(s) = &state {
                    ui.label(RichText::new(s.as_str()).font(bold(12.0)).color(level));
                }
                if let Some(w) = &since {
                    let w = if state.is_some() { format!(" · {w}") } else { w.clone() };
                    ui.label(RichText::new(w).font(font(12.0)).color(TEXT_DIM()));
                }
            });
        }
        if !i.affects.is_empty() {
            ui.add(egui::Label::new(RichText::new(i.affects.join(" · ")).font(font(12.0)).color(TEXT_FAINT())).wrap());
        }
    }
    if !incidents.is_empty() {
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(host).font(font(12.0)).color(TEXT_FAINT()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("↗").font(font(12.0)).color(TEXT_FAINT()));
        });
    });
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
    empty_state(ui, r, &t!("ai.checking_name", name = m.name), &t!("ai.reading_logs", name = m.name), None);
}

/// A provider that's set up but hasn't been used on this Mac yet. While a
/// check runs the button says so and waits; the page stays as it is.
fn no_usage(cl: &UsageInfo, ui: &mut Ui, r: Rect) {
    let m = ai::meta(cl.provider);
    let msg = t!("ai.no_usage_msg", name = m.name);
    let label = if cl.loading || backend::refreshing() { ts!("ai.checking") } else { ts!("ai.check_again") };
    if empty_state(ui, r, &t!("ai.no_usage_yet", name = m.name), &msg, Some(label)) {
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

