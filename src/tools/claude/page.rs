//! The AI usage page in the full window. One provider: its limits with
//! pace and forecast, what the use would cost at API prices (what the plan
//! saved, or the month's budget for an API key), a day chart to hover, and
//! use by model and project. "All": a card per provider side by side.

use super::backend::{self, All};
use super::parts::{self, Measure};
use super::ui::CLEANUP;
use crate::core::tool::Cx;
use crate::kit::ai::{self, settings, Billing, Day, UsageInfo};
use crate::kit::claude_code::{self as cc, Availability};
use crate::kit::glass::*;
use crate::kit::sync::LockExt;
use chrono::Datelike;
use egui::{Align2, Pos2, Rect, Ui, Vec2};

pub use parts::usage_color;

const GAP: f32 = sp::L;

/// The provider switch, right-aligned in `area`: "All" and every provider
/// that's on. Returns its left edge.
fn picker(demo: bool, ui: &mut Ui, area: Rect) -> f32 {
    let set = settings::get();
    let mut idx: Vec<usize> = vec![ai::ALL];
    idx.extend((0..ai::ALL).filter(|i| set.is_on(ai::PROVIDERS[*i].id)));
    let labels: Vec<&str> = idx.iter().map(|i| ai::PROVIDERS.get(*i).map(|m| m.name).unwrap_or("All")).collect();
    let w = 72.0 * labels.len() as f32;
    let rect = Rect::from_min_max(Pos2::new(area.right() - w, area.top()), area.max);
    let cur = idx.iter().position(|i| *i == ai::selected()).unwrap_or(0);
    if let Some(k) = segmented(ui, rect, ui.scope_id().with("ai-provider"), &labels, cur) {
        backend::select(idx[k], demo);
    }
    rect.left()
}

/// Header: title, status, refresh, the provider switch. Returns the left
/// edge of the controls.
fn header(ui: &mut Ui, r: Rect, sub: &str, sub_col: egui::Color32, loading: bool, demo: bool, cx: &mut Cx) -> f32 {
    let p = ui.painter().clone();
    let btns = page_header(&p, r, "AI Usage", sub, sub_col);
    let rb = Rect::from_center_size(Pos2::new(btns.right() - 13.0, btns.center().y), Vec2::splat(26.0));
    if loading || backend::refreshing() {
        spinner(&p, rb.center(), 6.0, ui.ctx().input(|i| i.time), TEXT_DIM());
        animate(ui.ctx());
    } else if round_button(ui, rb, Icon::Refresh, "Refresh").clicked() {
        backend::request_refresh();
        cx.toast("Checking your AI tools…");
    }
    picker(demo, ui, Rect::from_min_max(btns.min, Pos2::new(rb.left() - sp::M, btns.bottom())))
}

/// The page when the assistant is missing or signed out.
fn not_set_up(a: Availability, cl: &UsageInfo, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let m = ai::meta(cl.provider);
    let short = if a == Availability::SignedOut { format!("{} signed out", m.name) } else { format!("{} not installed", m.name) };
    header(ui, r, &short, TEXT_DIM(), cl.loading, demo, cx);
    let colw = 480.0f32.min(r.width() - 48.0);
    let x = r.center().x - colw / 2.0;
    let mut y = r.top() + PAGE_HEADER_H + 56.0;
    tool_chip(&p, Rect::from_center_size(Pos2::new(r.center().x, y + 28.0), Vec2::splat(56.0)), Icon::Gauge, tint::CLAUDE());
    y += 56.0 + 20.0;
    let title = if a == Availability::SignedOut { format!("{} isn't signed in", m.name) } else { format!("{} isn't installed", m.name) };
    p.text(Pos2::new(r.center().x, y), Align2::CENTER_TOP, title, bold(22.0), TEXT());
    y += 32.0 + 12.0;
    let steps: &[&str] = if a == Availability::SignedOut { m.sign_in_steps } else { m.install_steps };
    let card = Rect::from_min_size(Pos2::new(x, y), Vec2::new(colw, 12.0 + steps.len() as f32 * 34.0));
    glass_card(&p, card, R_CARD, 0.0, false);
    for (i, s) in steps.iter().enumerate() {
        let cy = card.top() + 6.0 + 17.0 + i as f32 * 34.0;
        let c = Pos2::new(card.left() + 24.0, cy);
        p.circle_filled(c, 10.0, white(18));
        p.text(c, Align2::CENTER_CENTER, format!("{}", i + 1), bold(11.0), TEXT());
        text_fit(&p, Pos2::new(card.left() + 44.0, cy), Align2::LEFT_CENTER, s, font(13.0), TEXT_SOFT(), colw - 58.0);
    }
    y = card.bottom() + 20.0;
    let primary = "Check again";
    let get = format!("Get {}", if m.id == "claude" { "Claude Code" } else { m.name });
    let second = if a == Availability::SignedOut { "Open Terminal" } else { get.as_str() };
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
        cx.toast(format!("Looking for {}…", m.name));
    }
}

/// An outage chip (the provider's status page, not the person's usage),
/// with a hover that says so.
fn incident(ui: &mut Ui, pos: Pos2, align: Align2, cl: &UsageInfo) -> Option<Rect> {
    let p = ui.painter().clone();
    let r = parts::incident_chip(&p, pos, align, cl)?;
    let m = ai::meta(cl.provider);
    let host = m.status_url.trim_start_matches("https://").split('/').next().unwrap_or("");
    let what = if cl.incident.as_deref() == Some("Outage") { "an outage" } else { "degraded service" };
    ui.interact(r, ui.scope_id().with(("ai-incident", cl.provider)), egui::Sense::hover()).on_hover_text(format!("{host} reports {what} right now. It's about {}'s servers, not your usage.", m.name));
    Some(r)
}

pub fn page(all: &All, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    if ai::shows_all() {
        all_page(all, demo, ui, r, cx);
        return;
    }
    let cl = backend::current(all).lock_ok().clone();
    // the first check isn't done: say so, rather than cards of zeros that
    // turn into "No usage yet" a moment later
    if !cl.scanned {
        let left = header(ui, r, "Checking…", TEXT_DIM(), true, demo, cx);
        incident(ui, Pos2::new(left - sp::M, r.top() + PAGE_HEADER_H / 2.0), Align2::RIGHT_CENTER, &cl);
        checking(&cl, ui, r);
        return;
    }
    if let Some(a) = cl.unavailable() {
        not_set_up(a, &cl, demo, ui, r, cx);
        return;
    }
    let meta = ai::meta(cl.provider);
    let p = ui.painter().clone();
    let (fresh, fresh_col) = parts::fresh(&cl);
    let mut sub = cl.plan_title();
    if cl.is_api() {
        sub.push_str(" · API key");
    }
    if !fresh.is_empty() {
        sub.push_str(&format!(" · {fresh}"));
    }
    let left = header(ui, r, &sub, if cl.stale { AMBER() } else { TEXT_DIM() }, cl.loading, demo, cx);
    incident(ui, Pos2::new(left - sp::M, r.top() + PAGE_HEADER_H / 2.0), Align2::RIGHT_CENTER, &cl);
    // nothing to show at all: one empty state instead of cards of zeros. It
    // stays up while a refresh runs, so the page doesn't flash cards
    if !has_usage(&cl) {
        no_usage(&cl, ui, r);
        return;
    }
    let _ = fresh_col;

    let is_claude = meta.id == "claude";
    let show_usd = settings::get().show_cost;
    let x0 = r.left() + sp::PAGE_EDGE;
    let x1 = r.right() - sp::PAGE_EDGE;
    let y = r.top() + PAGE_HEADER_H + 1.0 + sp::L;
    let cw = (x1 - x0 - 3.0 * GAP) / 4.0;
    let ch = 158.0;
    let slot = |k: usize, span: usize| Rect::from_min_size(Pos2::new(x0 + k as f32 * (cw + GAP), y), Vec2::new(cw * span as f32 + GAP * (span as f32 - 1.0), ch));

    // row 1
    if cl.is_api() {
        budget_card(ui, slot(0, 2), &cl);
        today_card(&p, slot(2, 1), &cl, show_usd);
        avg_card(&p, slot(3, 1), &cl);
    } else if !cl.limits.is_empty() {
        let n = cl.limits.len().min(3);
        for (k, l) in cl.limits.iter().take(n).enumerate() {
            limit_card(&p, slot(k, 1), l);
        }
        let rest = Rect::from_min_max(slot(n, 1).min, Pos2::new(x1, y + ch));
        if show_usd && cl.billing == Billing::Subscription && cl.plan_usd().is_some() {
            saved_card(&p, rest, &cl);
        } else {
            today_card(&p, rest, &cl, show_usd);
        }
    } else if meta.limits && (cl.loading || cl.error.is_some()) {
        let lr = slot(0, 3);
        glass_card(&p, lr, R_CARD, 0.0, false);
        let msg = cl.error.clone().unwrap_or_else(|| format!("Asking {} for your limits…", cl.provider));
        p.text(lr.min + Vec2::splat(sp::L), Align2::LEFT_TOP, "Plan limits", ty::title(), TEXT());
        paragraph(&p, lr.min + Vec2::new(sp::L, sp::L + 16.0 + sp::M), &msg, ty::body(), TEXT_SOFT(), lr.width() - 2.0 * sp::L);
        today_card(&p, slot(3, 1), &cl, show_usd);
    } else {
        // tokens only
        let l = &cl.local;
        stat_card(&p, slot(0, 1), "Today", &ai::human_tokens(l.tokens_today), &money_line(show_usd, l.usd_today, l.requests_today));
        stat_card(&p, slot(1, 1), "7 days", &ai::human_tokens(l.tokens_7d), &if show_usd { format!("{} at API prices", parts::usd(l.usd_7d)) } else { String::new() });
        let t30: u64 = l.days.iter().rev().take(30).map(|d| d.tokens).sum();
        stat_card(&p, slot(2, 1), "30 days", &ai::human_tokens(t30), &if show_usd { format!("{} at API prices", parts::usd(l.usd_30d)) } else { String::new() });
        stat_card(&p, slot(3, 1), "Last 5 hours", &ai::human_tokens(l.tokens_5h), "");
    }

    // row 2: the chart, then models over projects
    let y2 = y + ch + GAP;
    let bottom = r.bottom() - if is_claude { PAGE_FOOTER_H } else { 0.0 } - sp::L;
    let chart = Rect::from_min_max(Pos2::new(x0, y2), Pos2::new(x0 + cw * 2.0 + GAP, bottom));
    chart_card(ui, chart, &cl, meta.id, show_usd, demo);
    let col = Rect::from_min_max(Pos2::new(chart.right() + GAP, y2), Pos2::new(x1, bottom));
    let half = (col.height() - GAP) / 2.0;
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    for (k, (title, rows)) in [("Models", &cl.local.models), ("Projects", &cl.local.projects)].into_iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(col.left(), col.top() + k as f32 * (half + GAP)), Vec2::new(col.width(), half));
        glass_card(&p, cr, R_CARD, 0.0, false);
        let inner = cr.shrink(sp::L);
        p.text(inner.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
        p.text(inner.right_top(), Align2::RIGHT_TOP, "30 days", ty::caption(), ty::caption_color());
        let list = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 16.0 + sp::M), inner.max);
        if rows.is_empty() {
            empty_note(&p, list, &format!("No {} in the last 30 days", title.to_lowercase()));
        }
        parts::share_rows(&p, list, rows, show_usd, &home);
    }

    if is_claude {
        let f = page_footer(&p, r, 12.0);
        if cl.data_bytes > 0 {
            text_fit(&p, Pos2::new(f.left() + 24.0, f.center().y), Align2::LEFT_CENTER, &format!("{} data on this Mac {}", cl.provider, human_bytes(cl.data_bytes)), ty::label(), TEXT_DIM(), f.width() / 2.0);
        }
        let bl = "Review in Cleanup";
        let bw = button_width(&p, bl, BTN_PRIMARY);
        let br = Rect::from_min_size(Pos2::new(f.right() - 24.0 - bw, f.center().y - 16.0), Vec2::new(bw, 32.0));
        if pill_button(ui, br, bl, BTN_PRIMARY, true).clicked() {
            cx.open_page(CLEANUP, "claude");
        }
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
        animate(ui.ctx());
        return false;
    };
    let bw = button_width(&p, label, BTN_SECONDARY);
    let br = Rect::from_center_size(below, Vec2::new(bw, 30.0));
    pill_button(ui, br, label, BTN_SECONDARY, !backend::refreshing()).clicked()
}

/// The first check after launch is still running.
fn checking(cl: &UsageInfo, ui: &mut Ui, r: Rect) {
    let m = ai::meta(cl.provider);
    empty_state(ui, r, &format!("Checking {}…", m.name), &format!("Reading {}'s logs and limits on this Mac.", m.name), None);
}

/// A provider that's set up but hasn't been used on this Mac yet. While a
/// check runs the button says so and waits; the page stays as it is.
fn no_usage(cl: &UsageInfo, ui: &mut Ui, r: Rect) {
    let m = ai::meta(cl.provider);
    let msg = format!("Use {} on this Mac and its tokens, cost, models and projects show up here. Nothing in the last 30 days so far.", m.name);
    let label = if cl.loading || backend::refreshing() { "Checking…" } else { "Check again" };
    if empty_state(ui, r, &format!("No {} usage yet", m.name), &msg, Some(label)) {
        backend::request_refresh();
    }
}

fn money_line(show_usd: bool, usd: f64, req: u64) -> String {
    let mut parts_: Vec<String> = Vec::new();
    if show_usd && usd > 0.0 {
        parts_.push(parts::usd(usd));
    }
    if req > 0 {
        parts_.push(format!("{req} requests"));
    }
    parts_.join(" · ")
}

/// A plain number card: title, big value, one line under it.
fn stat_card(p: &egui::Painter, r: Rect, title: &str, value: &str, line: &str) {
    glass_card(p, r, R_CARD, 0.0, false);
    let i = r.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    text_fit(p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, value, ty::hero(), TEXT(), i.width());
    text_fit(p, i.left_bottom(), Align2::LEFT_BOTTOM, line, ty::label(), TEXT_DIM(), i.width());
}

/// One plan limit: ring with the even-use arc, pace, forecast.
fn limit_card(p: &egui::Painter, cr: Rect, l: &ai::Limit) {
    glass_card(p, cr, R_CARD, 0.0, false);
    let inner = cr.shrink(sp::L);
    split_line(p, inner.left_top(), inner.right_top(), egui::Align::Min, &backend::short_label(&l.label), ty::title(), TEXT(), &parts::reset_when(l), ty::label(), TEXT_DIM());
    let rr = 34.0;
    let c = Pos2::new(inner.left() + rr + 5.0, inner.bottom() - rr - 5.0);
    parts::pace_ring(p, c, rr, 6.0, l);
    p.text(c, Align2::CENTER_CENTER, format!("{:.0}%", l.pct), ty::lead(), TEXT());
    let tx = c.x + rr + sp::L;
    let w = inner.right() - tx;
    // pace over the forecast, the block centred on the ring; the forecast
    // wraps ("Out tomorrow" / "23:40") rather than being cut
    let fc = parts::forecast(l);
    let fg = p.layout(fc.clone(), ty::label(), TEXT_DIM(), w);
    let pace = parts::pace_label(l);
    let ph = if pace.is_some() { line_h(p, &ty::title()) + sp::XS } else { 0.0 };
    let top = c.y - (ph + fg.size().y) / 2.0;
    if let Some((s, col)) = pace {
        text_fit(p, Pos2::new(tx, top), Align2::LEFT_TOP, s, ty::title(), col, w);
    }
    text_wrap(p, Pos2::new(tx, top + ph), Align2::LEFT_TOP, &fc, ty::label(), TEXT_DIM(), w, 3);
}

/// A subscription: the 30 days at API prices against the plan's price.
fn saved_card(p: &egui::Painter, r: Rect, cl: &UsageInfo) {
    glass_card(p, r, R_CARD, 0.0, false);
    let i = r.shrink(sp::L);
    let plan = cl.plan_usd().unwrap_or(0.0);
    let api = cl.local.usd_30d;
    let saved = api - plan;
    p.text(i.left_top(), Align2::LEFT_TOP, "Saved · 30 days", ty::title(), TEXT());
    let hero = if saved > 0.0 { parts::usd(saved) } else { "$0".into() };
    let hr = text_fit(p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, &hero, ty::hero(), if saved > 0.0 { GREEN() } else { TEXT() }, i.width());
    if plan > 0.0 {
        text_fit(p, Pos2::new(hr.right() + sp::S, hr.bottom() - 4.0), Align2::LEFT_BOTTOM, &format!("{:.1}× plan", api / plan), ty::label(), TEXT_DIM(), i.right() - hr.right());
    }
    let rows = [("At API prices", parts::usd(api)), ("You paid", parts::usd(plan))];
    for (k, (a, b)) in rows.iter().enumerate() {
        let yy = i.bottom() - (1 - k) as f32 * 20.0;
        p.text(Pos2::new(i.left(), yy), Align2::LEFT_BOTTOM, *a, ty::label(), TEXT_DIM());
        p.text(Pos2::new(i.right(), yy), Align2::RIGHT_BOTTOM, b, ty::label(), TEXT());
    }
}

/// Tokens and dollars today.
fn today_card(p: &egui::Painter, r: Rect, cl: &UsageInfo, show_usd: bool) {
    let l = &cl.local;
    let mut line = vec![format!("{} in 7 days", ai::human_tokens(l.tokens_7d))];
    if l.requests_today > 0 {
        line.push(format!("{} req", l.requests_today));
    }
    let value = if show_usd && cl.is_api() { parts::usd(l.usd_today) } else { ai::human_tokens(l.tokens_today) };
    let title = if show_usd && !cl.is_api() && l.usd_today > 0.0 { format!("Today · {}", parts::usd(l.usd_today)) } else { "Today".into() };
    stat_card(p, r, &title, &value, &line.join(" · "));
}

/// Average per day this month and the budget's even share.
fn avg_card(p: &egui::Painter, r: Rect, cl: &UsageInfo) {
    let month = cl.local.month();
    let n = month.len().max(1) as f64;
    let avg = cl.local.usd_month / n;
    let line = budget(cl).map(|b| format!("Budget pace {}", parts::usd(b / days_in_month() as f64))).unwrap_or_default();
    stat_card(p, r, "Avg / day", &parts::usd(avg), &line);
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
    glass_card(&p, r, R_CARD, 0.0, false);
    let i = r.shrink(sp::L);
    let today = chrono::Local::now().date_naive();
    let spent = cl.local.usd_month;
    let b = budget(cl);
    p.text(i.left_top(), Align2::LEFT_TOP, format!("{} budget", today.format("%B")), ty::title(), TEXT());
    // the budget field, top right
    let fr = Rect::from_min_max(Pos2::new(i.right() - 84.0, i.top() + 8.0 - sp::CONTROL_H / 2.0), Pos2::new(i.right(), i.top() + 8.0 + sp::CONTROL_H / 2.0));
    let id = ui.scope_id().with(("ai-budget", cl.provider));
    let mut text = ui.ctx().data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| b.map(|v| format!("{v:.0}")).unwrap_or_default());
    let resp = text_input(ui, fr, id.with("field"), &mut text, "Budget", FieldStyle { right: true, ..Default::default() });
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
            ai::pace::State::Ahead => ("Ahead of pace", RED()),
            ai::pace::State::OnPace => ("On pace", TEXT()),
            ai::pace::State::Room => ("Room to spare", GREEN()),
        };
        p.text(Pos2::new(hr.right() + sp::M, hr.bottom() - 4.0), Align2::LEFT_BOTTOM, s, ty::title(), c);
    }
    let br = Rect::from_min_size(Pos2::new(i.left(), hr.bottom() + sp::M), Vec2::new(i.width(), 8.0));
    let frac = (spent / b) as f32;
    bar(&p, br, frac.clamp(0.01, 1.0), if frac >= 0.9 { RED() } else if frac >= 0.7 { AMBER() } else { CLAUDE() });
    let x = br.left() + br.width() * day as f32 / dim as f32;
    p.line_segment([Pos2::new(x, br.top() - 3.0), Pos2::new(x, br.bottom() + 3.0)], egui::Stroke::new(1.5, TEXT()));
    p.text(i.left_bottom(), Align2::LEFT_BOTTOM, format!("Day {day} of {dim}"), ty::label(), TEXT_DIM());
    let per_day = spent / day.max(1) as f64;
    let by_end = per_day * dim as f64;
    let mut f = format!("~{} by {}", parts::usd(by_end), today.format("%b ").to_string() + &dim.to_string());
    if per_day > 0.0 && by_end > b {
        let out_day = (b / per_day).ceil() as u32;
        f = format!("Budget out {} {out_day} · {f}", today.format("%b"));
    }
    text_fit(&p, i.right_bottom(), Align2::RIGHT_BOTTOM, &f, ty::label(), if by_end > b { RED() } else { TEXT_DIM() }, i.width() * 0.65);
}

/// History: the limit window as a line, or tokens / dollars per day.
fn chart_card(ui: &mut Ui, r: Rect, cl: &UsageInfo, id: &str, show_usd: bool, demo: bool) {
    let p = ui.painter().clone();
    glass_card(&p, r, R_CARD, 0.0, false);
    let i = r.shrink(sp::L);
    // one question per card: limits live in the top row; this card is always
    // daily usage, and Tokens | $ only changes the unit of the same bars
    let _ = demo;
    let units: Vec<&str> = if show_usd { vec!["Tokens", "$"] } else { vec![] };
    let key = ui.scope_id().with(("ai-chart-unit", id));
    let mut unit = ui.ctx().data_mut(|d| d.get_temp::<usize>(key)).unwrap_or(usize::from(cl.is_api() && show_usd)).min(units.len().saturating_sub(1));
    let title = if cl.is_api() { "Usage · this month" } else { "Usage · 30 days" };
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    if !units.is_empty() {
        let tw = 120.0;
        let tr = Rect::from_min_size(Pos2::new(i.right() - tw, i.top() - 6.0), Vec2::new(tw, sp::CONTROL_H));
        if let Some(k) = segmented(ui, tr, key.with("seg"), &units, unit) {
            unit = k;
            ui.ctx().data_mut(|d| d.insert_temp(key, k));
        }
    }
    let usd_unit = units.get(unit) == Some(&"$");
    let body = Rect::from_min_max(Pos2::new(i.left(), i.top() + 24.0 + sp::M), i.max);
    match if cl.is_api() && usd_unit { "Spend" } else if usd_unit { "API price" } else { "Tokens" } {
        "Spend" => {
            let pace = budget(cl).map(|b| b / days_in_month() as f64);
            let days: Vec<Day> = cl.local.month().to_vec();
            if days.iter().all(|d| d.tokens == 0) {
                return empty_note(&p, body, "No spend this month yet");
            }
            parts::day_chart(ui, body, &days, Measure::Usd, pace, id);
        }
        other => {
            let days: Vec<Day> = cl.local.days.iter().rev().take(30).rev().copied().collect();
            if days.iter().all(|d| d.tokens == 0) {
                return empty_note(&p, body, "No usage in the last 30 days");
            }
            let m = if other == "Tokens" { Measure::Tokens } else { Measure::Usd };
            parts::day_chart(ui, body, &days, m, None, id);
        }
    }
}


// ---------------------------------------------------------------- all

/// Every provider that's on, side by side, and the dollars of all of them.
fn all_page(all: &All, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    // tools that aren't installed get no card, only a line at the bottom
    let (infos, missing): (Vec<_>, Vec<_>) = backend::enabled(all).into_iter().partition(|i| i.unavailable() != Some(Availability::NotInstalled));
    let p = ui.painter().clone();
    let show_usd = settings::get().show_cost;
    let today: f64 = infos.iter().map(|i| i.local.usd_today).sum();
    let sub = if show_usd { format!("{} today", parts::usd(today)) } else { String::new() };
    let loading = infos.iter().any(|i| i.loading || !i.scanned);
    header(ui, r, &sub, TEXT_DIM(), loading, demo, cx);
    let x0 = r.left() + sp::PAGE_EDGE;
    let x1 = r.right() - sp::PAGE_EDGE;
    let mut y = r.top() + PAGE_HEADER_H + 1.0 + sp::L;
    let cols = if infos.len() > 2 { 2 } else { infos.len().max(1) };
    let cw = (x1 - x0 - GAP * (cols as f32 - 1.0)) / cols as f32;
    let ch = 168.0;
    // the chip only when the menu bar shows a number at all
    let menu = match settings::get().menu_bar.as_str() {
        "off" => None,
        "closest" => ai::alerts::closest(&infos).map(|(i, _)| i.provider),
        id => infos.iter().find(|i| ai::meta(i.provider).id == id).map(|i| i.provider),
    };
    for (k, info) in infos.iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(x0 + (k % cols) as f32 * (cw + GAP), y + (k / cols) as f32 * (ch + GAP)), Vec2::new(cw, ch));
        provider_card(ui, cr, info, menu == Some(info.provider), show_usd);
    }
    y += infos.len().div_ceil(cols) as f32 * (ch + GAP);
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|i| ai::meta(i.provider).name).collect();
        text_fit(&p, Pos2::new(x0, y - GAP + sp::M), Align2::LEFT_TOP, &format!("Not installed on this Mac: {}. Their cards show up once they're used here.", names.join(", ")), ty::label(), TEXT_FAINT(), x1 - x0);
        y += sp::M + 16.0;
    }
    // dollars of every provider per day
    let bottom = r.bottom() - sp::L;
    if bottom - y < 120.0 || !show_usd {
        return;
    }
    let cr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, bottom));
    glass_card(&p, cr, R_CARD, 0.0, false);
    let i = cr.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, "At API prices · 30 days", ty::title(), TEXT());
    let mut days: Vec<Day> = Vec::new();
    for info in &infos {
        for d in info.local.days.iter().rev().take(30) {
            match days.iter_mut().find(|x| x.date == d.date) {
                Some(x) => {
                    x.tokens += d.tokens;
                    x.usd += d.usd;
                    x.requests += d.requests;
                }
                None => days.push(*d),
            }
        }
    }
    days.sort_by_key(|d| d.date);
    parts::day_chart(ui, Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::M), i.max), &days, Measure::Usd, None, "all");
}

/// One provider in the "All" view.
fn provider_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, menu: bool, show_usd: bool) {
    let p = ui.painter().clone();
    let resp = ui.interact(cr, ui.scope_id().with(("ai-all", info.provider)), egui::Sense::click());
    glass_card(&p, cr, R_CARD, if resp.hovered() { 1.0 } else { 0.0 }, false);
    if resp.clicked() {
        backend::select(ai::index_of(ai::meta(info.provider).id), false);
    }
    let i = cr.shrink(sp::L);
    let (f, fc) = parts::fresh(info);
    let fr = p.text(i.right_top(), Align2::RIGHT_TOP, &f, ty::label(), fc);
    // the time keeps its width; the plan name gives way (and a chip's room)
    let chips_w = if menu { 90.0 } else { 0.0 } + if info.incident.is_some() { 80.0 } else { 0.0 };
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &info.plan_title(), ty::title(), TEXT(), (fr.left() - sp::M - i.left() - chips_w).max(i.width() * 0.3));
    let mut x = tr.right() + sp::S;
    if menu {
        // this one's limit is the number next to the icon in the menu bar
        let cr = chip(&p, Pos2::new(x, tr.center().y), Align2::LEFT_CENTER, "In menu bar", (with_alpha(BLUE(), 0.16), ACCENT()));
        ui.interact(cr, ui.scope_id().with(("ai-menu-chip", info.provider)), egui::Sense::hover()).on_hover_text("Its usage is the number shown next to the Cmd+Meow icon in the menu bar. Change it in Settings › AI Usage.");
        x = cr.right() + sp::S;
    }
    incident(ui, Pos2::new(x, tr.center().y), Align2::LEFT_CENTER, info);
    let mut y = i.top() + 16.0 + sp::M;
    // the same words as the provider's own tab
    let state = if !info.scanned {
        Some("Checking…".to_string())
    } else if let Some(e) = info.unavailable() {
        Some(if e == Availability::SignedOut { "Signed out" } else { "Not installed" }.to_string())
    } else if !has_usage(info) {
        Some(format!("No {} usage yet", ai::meta(info.provider).name))
    } else {
        None
    };
    if let Some(s) = state {
        p.text(Pos2::new(i.left(), y), Align2::LEFT_TOP, s, ty::body(), TEXT_DIM());
        return;
    }
    if info.limits.is_empty() {
        let l = &info.local;
        for (k, (lab, v)) in [("Today", l.tokens_today), ("7 days", l.tokens_7d), ("5 hours", l.tokens_5h)].iter().enumerate() {
            let xx = i.left() + k as f32 * i.width() / 3.0;
            p.text(Pos2::new(xx, y), Align2::LEFT_TOP, ai::human_tokens(*v), ty::number(), TEXT());
            p.text(Pos2::new(xx, y + 28.0), Align2::LEFT_TOP, *lab, ty::caption(), ty::caption_color());
        }
    }
    for l in info.limits.iter().take(2) {
        let row = Rect::from_min_size(Pos2::new(i.left(), y), Vec2::new(i.width(), 38.0));
        text_fit(&p, row.left_top(), Align2::LEFT_TOP, &backend::short_label(&l.label), ty::label(), TEXT_SOFT(), 120.0);
        p.text(Pos2::new(row.right(), row.top()), Align2::RIGHT_TOP, format!("{:.0}%", l.pct), ty::title(), TEXT());
        let fc = parts::forecast(l);
        let col = if fc.starts_with("Out") { RED() } else { TEXT_DIM() };
        text_fit(&p, Pos2::new(row.right() - 44.0, row.top() + 1.0), Align2::RIGHT_TOP, &fc, ty::label(), col, row.width() - 170.0);
        parts::pace_bar(&p, Rect::from_min_size(Pos2::new(row.left(), row.top() + 22.0), Vec2::new(row.width(), 5.0)), l);
        y += 38.0;
    }
    let l = &info.local;
    let mut line = Vec::new();
    if show_usd {
        line.push(format!("{} today", parts::usd(l.usd_today)));
    }
    line.push(format!("{} tok", ai::human_tokens(l.tokens_today)));
    if show_usd {
        line.push(format!("{} / 7d", parts::usd(l.usd_7d)));
    }
    text_fit(&p, i.left_bottom(), Align2::LEFT_BOTTOM, &line.join(" · "), ty::label(), TEXT_DIM(), i.width());
}
