//! The AI usage page in the full window. One provider: its limits with
//! pace and forecast, what the use would cost at API prices (what the plan
//! saved, or the month's budget for an API key), a day chart to hover, and
//! use by model and project. "All": a card per provider side by side.

use super::backend::{self, All};
use super::parts::{self, Measure};
use crate::core::tool::Cx;
use crate::kit::ai::{self, settings, Billing, Day, UsageInfo};
use crate::kit::claude_code::{self as cc, Availability};
use crate::kit::glass::*;
use crate::kit::sync::LockExt;
use chrono::Datelike;
use egui::{Align2, Pos2, Rect, Ui, Vec2};

pub use parts::usage_color;

const GAP: f32 = sp::L;
/// R15: the grid fills the width but not the height. The limits row and
/// the chart with models and projects stop growing here, as on Mac Health.
const GRID_MAX_H: f32 = 2.0 * 340.0 + sp::L;
/// The top row: title, 12, a limit's ring (SP2 at [`RING_D`]).
const ROW1_H: f32 = 2.0 * sp::L + 16.0 + sp::M + RING_D;
/// One ring size for the page (SP2 `stat_ring`, one size per page).
const RING_D: f32 = 112.0;

/// The provider switch, right-aligned in `area`: "All" and every provider
/// that's on. Returns its left edge.
fn picker(demo: bool, ui: &mut Ui, area: Rect) -> f32 {
    let set = settings::get();
    let mut idx: Vec<usize> = vec![ai::ALL];
    idx.extend((0..ai::ALL).filter(|i| set.is_on(ai::PROVIDERS[*i].id)));
    let labels: Vec<&str> = idx.iter().map(|i| ai::PROVIDERS.get(*i).map(|m| m.name).unwrap_or(ts!("ai.all"))).collect();
    let w = crate::kit::glass::tabs_width(ui.painter(), &labels);
    let rect = Rect::from_min_max(Pos2::new(area.right() - w, area.top()), area.max);
    let cur = idx.iter().position(|i| *i == ai::selected()).unwrap_or(0);
    if let Some(k) = segmented(ui, rect, ui.scope_id().with("ai-provider"), &labels, cur) {
        backend::select(idx[k], demo);
    }
    rect.left()
}

/// Header: title, status, refresh, the provider switch. Returns the left
/// edge of the controls.
fn header(ui: &mut Ui, r: Rect, (sub, sub_col): (&str, egui::Color32), tip: &str, loading: bool, demo: bool, cx: &mut Cx) -> f32 {
    let p = ui.painter().clone();
    // SP16: Refresh at the right edge, the provider switch next to it
    let rw = icon_pill_width(&p, ts!("ai.refresh"), BTN_SECONDARY);
    let set = settings::get();
    let mut names: Vec<&str> = vec![ts!("ai.all")];
    names.extend((0..ai::ALL).filter(|i| set.is_on(ai::PROVIDERS[*i].id)).map(|i| ai::PROVIDERS[i].name));
    let tw = crate::kit::glass::tabs_width(&p, &names);
    let btns = page_header_room(&p, r, ts!("ai.name"), sub, sub_col, 0.0, r.right() - sp::PAGE_EDGE - rw - sp::M - tw - sp::M).0;
    let (tr, rb) = header_controls(btns, tw, rw);
    let busy = loading || backend::refreshing();
    if busy {
        crate::kit::glass::poll(ui.ctx());
    }
    // SP17 H5: when it last updated sits in the tip, not the status
    let resp = icon_pill(ui, rb, Icon::Refresh, ts!("ai.refresh"), BTN_SECONDARY, !busy);
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    if resp.clicked() {
        backend::request_refresh();
        cx.toast(ts!("ai.checking_tools"));
    }
    picker(demo, ui, tr)
}

/// The page when the assistant is missing or signed out.
fn not_set_up(a: Availability, cl: &UsageInfo, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    let p = ui.painter().clone();
    let m = ai::meta(cl.provider);
    let short = if a == Availability::SignedOut { t!("ai.signed_out_short", name = m.name).into_owned() } else { t!("ai.not_installed_short", name = m.name).into_owned() };
    header(ui, r, (&short, TEXT_DIM()), "", cl.loading, demo, cx);
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

/// An outage chip (the provider's status page, not the person's usage),
/// with a hover that says so.
fn incident(ui: &mut Ui, pos: Pos2, align: Align2, cl: &UsageInfo) -> Option<Rect> {
    let p = ui.painter().clone();
    let r = parts::incident_chip(&p, pos, align, cl)?;
    let m = ai::meta(cl.provider);
    let host = m.status_url.trim_start_matches("https://").split('/').next().unwrap_or("");
    let tip = if cl.incident.as_deref() == Some("Outage") { t!("ai.incident_outage", host = host, name = m.name) } else { t!("ai.incident_degraded", host = host, name = m.name) };
    ui.interact(r, ui.scope_id().with(("ai-incident", cl.provider)), egui::Sense::hover()).on_hover_text(tip);
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
        let left = header(ui, r, (ts!("ai.checking"), TEXT_DIM()), "", true, demo, cx);
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
        sub.push_str(ts!("ai.api_key_suffix"));
    }
    if !fresh.is_empty() && cl.stale {
        sub.push_str(&format!(" · {fresh}"));
    }
    let left = header(ui, r, (&sub, if cl.stale { AMBER() } else { TEXT_DIM() }), &fresh, cl.loading, demo, cx);
    incident(ui, Pos2::new(left - sp::M, r.top() + PAGE_HEADER_H / 2.0), Align2::RIGHT_CENTER, &cl);
    // nothing to show at all: one empty state instead of cards of zeros. It
    // stays up while a refresh runs, so the page doesn't flash cards
    if !has_usage(&cl) {
        no_usage(&cl, ui, r);
        return;
    }
    let _ = fresh_col;

    let show_usd = settings::get().show_cost;
    let x0 = r.left() + sp::PAGE_EDGE;
    let x1 = r.right() - sp::PAGE_EDGE;
    let y = r.top() + PAGE_HEADER_H + 1.0 + sp::L;
    let cw = (x1 - x0 - 3.0 * GAP) / 4.0;
    let ch = ROW1_H;
    let slot = |k: usize, span: usize| Rect::from_min_size(Pos2::new(x0 + k as f32 * (cw + GAP), y), Vec2::new(cw * span as f32 + GAP * (span as f32 - 1.0), ch));

    // row 1; a click anywhere on a limit card opens the limits popup
    let mut limit_rects: Vec<Rect> = Vec::new();
    if cl.is_api() {
        budget_card(ui, slot(0, 2), &cl);
        today_card(&p, slot(2, 1), &cl, show_usd);
        avg_card(&p, slot(3, 1), &cl);
    } else if !cl.limits.is_empty() {
        let n = cl.limits.len().min(3);
        for (k, l) in cl.limits.iter().take(n).enumerate() {
            limit_card(&p, slot(k, 1), l);
            limit_rects.push(slot(k, 1));
        }
        let rest = Rect::from_min_max(slot(n, 1).min, Pos2::new(x1, y + ch));
        if show_usd && cl.billing == Billing::Subscription && cl.plan_usd().is_some() {
            saved_card(&p, rest, &cl);
        } else {
            today_card(&p, rest, &cl, show_usd);
        }
    } else if meta.limits && (cl.loading || cl.error.is_some()) {
        let lr = slot(0, 3);
        data_card(&p, lr);
        let msg = cl.error.clone().unwrap_or_else(|| t!("ai.asking_limits", provider = cl.provider).into_owned());
        p.text(lr.min + Vec2::splat(sp::L), Align2::LEFT_TOP, ts!("ai.plan_limits"), ty::title(), TEXT());
        paragraph(&p, lr.min + Vec2::new(sp::L, sp::L + 16.0 + sp::M), &msg, ty::body(), TEXT_SOFT(), lr.width() - 2.0 * sp::L);
        today_card(&p, slot(3, 1), &cl, show_usd);
    } else {
        // tokens only
        let l = &cl.local;
        stat_card(&p, slot(0, 1), ts!("ai.today"), &ai::human_tokens(l.tokens_today), &money_line(show_usd, l.usd_today, l.requests_today));
        stat_card(&p, slot(1, 1), ts!("ai.days_7"), &ai::human_tokens(l.tokens_7d), &if show_usd { t!("ai.at_api_prices", usd = parts::usd(l.usd_7d)).into_owned() } else { String::new() });
        let t30: u64 = l.days.iter().rev().take(30).map(|d| d.tokens).sum();
        stat_card(&p, slot(2, 1), ts!("ai.days_30"), &ai::human_tokens(t30), &if show_usd { t!("ai.at_api_prices", usd = parts::usd(l.usd_30d)).into_owned() } else { String::new() });
        stat_card(&p, slot(3, 1), ts!("ai.last_5h"), &ai::human_tokens(l.tokens_5h), "");
    }

    // row 2: the chart, then models over projects
    let y2 = y + ch + GAP;
    let bottom = (r.bottom() - sp::L).min(y + GRID_MAX_H).max(y2 + 200.0);
    let chart = Rect::from_min_max(Pos2::new(x0, y2), Pos2::new(x0 + cw * 2.0 + GAP, bottom));
    chart_card(ui, chart, &cl, meta.id, show_usd, demo);
    let col = Rect::from_min_max(Pos2::new(chart.right() + GAP, y2), Pos2::new(x1, bottom));
    let half = (col.height() - GAP) / 2.0;
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    for (k, (title, empty, rows)) in [(ts!("ai.models"), ts!("ai.no_models"), &cl.local.models), (ts!("ai.projects"), ts!("ai.no_projects"), &cl.local.projects)].into_iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(col.left(), col.top() + k as f32 * (half + GAP)), Vec2::new(col.width(), half));
        data_card(&p, cr);
        let inner = cr.shrink(sp::L);
        p.text(inner.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
        p.text(inner.right_top(), Align2::RIGHT_TOP, ts!("ai.days_30"), ty::caption(), ty::caption_color());
        let list = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 16.0 + sp::M), inner.max);
        if rows.is_empty() {
            empty_note(&p, list, empty);
        }
        parts::share_lines(&p, list, rows, show_usd, &home);
    }
    // last, so nothing drawn after takes the card's clicks (the whole card, S2)
    for (k, lr) in limit_rects.into_iter().enumerate() {
        let resp = ui.interact(lr, ui.scope_id().with(("ai-limit", k)), egui::Sense::click());
        if resp.hovered() {
            p.rect_filled(lr, crate::kit::tile::R_TILE, white(6));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            open_limits(ui.ctx(), true);
        }
    }
    limits_popup(all, ui, cx);
}

fn limits_id() -> egui::Id {
    egui::Id::unique("ai-limits-details")
}

/// Open or close the limits popup.
pub fn open_limits(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| {
        if open {
            d.insert_temp(limits_id(), true);
        } else {
            d.remove::<bool>(limits_id());
        }
    });
}

/// The limits popup: every limit, what uses them, Clean Claude caches.
/// Esc, its close button or a click outside closes it.
fn limits_popup(all: &All, ui: &mut Ui, cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    if !ctx.data(|d| d.get_temp::<bool>(limits_id())).unwrap_or(false) {
        return;
    }
    let cl = backend::current(all).lock_ok().clone();
    let w = 440.0f32.min(ctx.content_rect().width() - 32.0);
    let h = super::ui::details_height(ui, &cl, w);
    let (close_btn, close) = crate::kit::modal::details(&ctx, limits_id(), w, h, |ui, m| super::ui::details(all, ui, m, cx));
    if close_btn || close {
        open_limits(&ctx, false);
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

fn money_line(show_usd: bool, usd: f64, req: u64) -> String {
    let mut parts_: Vec<String> = Vec::new();
    if show_usd && usd > 0.0 {
        parts_.push(parts::usd(usd));
    }
    if req > 0 {
        parts_.push(t!("ai.n_requests", n = req).into_owned());
    }
    parts_.join(" · ")
}

/// A plain number card: title, big value, one line under it.
fn stat_card(p: &egui::Painter, r: Rect, title: &str, value: &str, line: &str) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, title, ty::title(), TEXT());
    text_fit(p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, value, ty::hero(), TEXT(), i.width());
    text_fit(p, i.left_bottom(), Align2::LEFT_BOTTOM, line, ty::label(), TEXT_DIM(), i.width());
}

/// One plan limit: the page's SP2 ring with the even-use arc under the
/// used one, and beside it when it runs out (red) and when it resets.
fn limit_card(p: &egui::Painter, cr: Rect, l: &ai::Limit) {
    data_card(p, cr);
    let inner = cr.shrink(sp::L);
    let tr = p.text(inner.left_top(), Align2::LEFT_TOP, backend::short_label(&l.label), ty::title(), TEXT());
    p.text(Pos2::new(tr.right() + sp::XS, tr.center().y), Align2::LEFT_CENTER, "›", ty::title(), TEXT_FAINT());
    let body = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 16.0 + sp::M), inner.max);
    let d = RING_D.min(body.height()).min(body.width() * 0.6);
    let c = Pos2::new(body.left() + d / 2.0, body.center().y);
    // no caption in the ring: the card title names the number (T14)
    stat_ring_paced(p, c, d, l.pct / 100.0, parts::pace_arc(l), parts::limit_color(l), &format!("{:.0}%", l.pct), "");
    // beside it, centred on the ring; each line wraps rather than being cut
    // (R10). No pace words (T9): the faint arc shows the pace
    let tx = c.x + d / 2.0 + sp::M;
    let w = inner.right() - tx;
    let lines = parts::limit_lines(l);
    let hs: Vec<f32> = lines.iter().map(|(t, col)| p.layout(t.clone(), ty::label(), *col, w).size().y).collect();
    let total: f32 = hs.iter().sum::<f32>() + sp::XS * (hs.len().max(1) - 1) as f32;
    let mut y = c.y - total / 2.0;
    for ((t, col), h) in lines.iter().zip(hs) {
        text_wrap(p, Pos2::new(tx, y), Align2::LEFT_TOP, t, ty::label(), *col, w, 3);
        y += h + sp::XS;
    }
}

/// A subscription: the 30 days at API prices against the plan's price.
fn saved_card(p: &egui::Painter, r: Rect, cl: &UsageInfo) {
    data_card(p, r);
    let i = r.shrink(sp::L);
    let plan = cl.plan_usd().unwrap_or(0.0);
    let api = cl.local.usd_30d;
    let saved = api - plan;
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.saved_30"), ty::title(), TEXT());
    let hero = if saved > 0.0 { parts::usd(saved) } else { "$0".into() };
    let hr = text_fit(p, Pos2::new(i.left(), i.top() + 16.0 + sp::M), Align2::LEFT_TOP, &hero, ty::hero(), TEXT(), i.width());
    if plan > 0.0 {
        text_fit(p, Pos2::new(hr.right() + sp::S, hr.bottom() - 4.0), Align2::LEFT_BOTTOM, &t!("ai.x_plan", x = api / plan : {:.1}), ty::label(), TEXT_DIM(), i.right() - hr.right());
    }
    let rows = [(ts!("ai.api_prices"), parts::usd(api)), (ts!("ai.you_paid"), parts::usd(plan))];
    for (k, (a, b)) in rows.iter().enumerate() {
        let yy = i.bottom() - (1 - k) as f32 * 20.0;
        p.text(Pos2::new(i.left(), yy), Align2::LEFT_BOTTOM, *a, ty::label(), TEXT_DIM());
        p.text(Pos2::new(i.right(), yy), Align2::RIGHT_BOTTOM, b, ty::label(), TEXT());
    }
}

/// Tokens and dollars today.
fn today_card(p: &egui::Painter, r: Rect, cl: &UsageInfo, show_usd: bool) {
    let l = &cl.local;
    let mut line = vec![t!("ai.in_7_days", tokens = ai::human_tokens(l.tokens_7d)).into_owned()];
    if l.requests_today > 0 {
        line.push(t!("ai.n_req", n = l.requests_today).into_owned());
    }
    let value = if show_usd && cl.is_api() { parts::usd(l.usd_today) } else { ai::human_tokens(l.tokens_today) };
    let title = if show_usd && !cl.is_api() && l.usd_today > 0.0 { t!("ai.today_usd", usd = parts::usd(l.usd_today)).into_owned() } else { ts!("ai.today").into() };
    stat_card(p, r, &title, &value, &line.join(" · "));
}

/// Average per day this month and the budget's even share.
fn avg_card(p: &egui::Painter, r: Rect, cl: &UsageInfo) {
    let month = cl.local.month();
    let n = month.len().max(1) as f64;
    let avg = cl.local.usd_month / n;
    let line = budget(cl).map(|b| t!("ai.budget_pace", usd = parts::usd(b / days_in_month() as f64)).into_owned()).unwrap_or_default();
    stat_card(p, r, ts!("ai.avg_day"), &parts::usd(avg), &line);
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

/// History: the limit window as a line, or tokens / dollars per day.
fn chart_card(ui: &mut Ui, r: Rect, cl: &UsageInfo, id: &str, show_usd: bool, demo: bool) {
    let p = ui.painter().clone();
    data_card(&p, r);
    let i = r.shrink(sp::L);
    // one question per card: limits live in the top row; this card is always
    // daily usage, and Tokens | $ only changes the unit of the same bars
    let _ = demo;
    let units: Vec<&str> = if show_usd { vec![ts!("ai.unit_tokens"), "$"] } else { vec![] };
    let key = ui.scope_id().with(("ai-chart-unit", id));
    let mut unit = ui.ctx().data_mut(|d| d.get_temp::<usize>(key)).unwrap_or(usize::from(cl.is_api() && show_usd)).min(units.len().saturating_sub(1));
    let title = if cl.is_api() { ts!("ai.usage_month") } else { ts!("ai.usage_30") };
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
                return empty_note(&p, body, ts!("ai.no_spend"));
            }
            parts::day_chart(ui, body, &days, Measure::Usd, pace, show_usd, id);
        }
        other => {
            let days: Vec<Day> = cl.local.days.iter().rev().take(30).rev().copied().collect();
            if days.iter().all(|d| d.tokens == 0) {
                return empty_note(&p, body, ts!("ai.no_usage_30"));
            }
            let m = if other == "Tokens" { Measure::Tokens } else { Measure::Usd };
            parts::day_chart(ui, body, &days, m, None, show_usd, id);
        }
    }
}


// ---------------------------------------------------------------- all

/// Every provider that's on, side by side, and the dollars of all of them.
fn all_page(all: &All, demo: bool, ui: &mut Ui, r: Rect, cx: &mut Cx) {
    // tools that aren't installed get no card, only a line at the bottom
    let infos: Vec<_> = backend::enabled(all).into_iter().filter(|i| i.unavailable() != Some(Availability::NotInstalled)).collect();
    let p = ui.painter().clone();
    let show_usd = settings::get().show_cost;
    let today: f64 = infos.iter().map(|i| i.local.usd_today).sum();
    let sub = if show_usd { t!("ai.usd_today", usd = parts::usd(today)).into_owned() } else { String::new() };
    let loading = infos.iter().any(|i| i.loading || !i.scanned);
    header(ui, r, (&sub, TEXT_DIM()), "", loading, demo, cx);
    let x0 = r.left() + sp::PAGE_EDGE;
    let x1 = r.right() - sp::PAGE_EDGE;
    let mut y = r.top() + PAGE_HEADER_H + 1.0 + sp::L;
    let cols = if infos.len() > 2 { 2 } else { infos.len().max(1) };
    let cw = (x1 - x0 - GAP * (cols as f32 - 1.0)) / cols as f32;
    let ch = 168.0;
    for (k, info) in infos.iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(x0 + (k % cols) as f32 * (cw + GAP), y + (k / cols) as f32 * (ch + GAP)), Vec2::new(cw, ch));
        provider_card(ui, cr, info, show_usd);
    }
    y += infos.len().div_ceil(cols) as f32 * (ch + GAP);
    // dollars of every provider per day
    let bottom = r.bottom() - sp::L;
    if bottom - y < 120.0 || !show_usd {
        return;
    }
    let cr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, bottom));
    data_card(&p, cr);
    let i = cr.shrink(sp::L);
    p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.api_prices_30"), ty::title(), TEXT());
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
    parts::day_chart(ui, Rect::from_min_max(Pos2::new(i.left(), i.top() + 16.0 + sp::M), i.max), &days, Measure::Usd, None, true, "all");
}

/// One provider in the "All" view.
fn provider_card(ui: &mut Ui, cr: Rect, info: &UsageInfo, show_usd: bool) {
    let p = ui.painter().clone();
    let resp = ui.interact(cr, ui.scope_id().with(("ai-all", info.provider)), egui::Sense::click());
    crate::kit::tile::paint_card(&p, cr, if resp.hovered() { 1.0 } else { 0.0 }, false);
    if resp.clicked() {
        backend::select(ai::index_of(ai::meta(info.provider).id), false);
    }
    let i = cr.shrink(sp::L);
    let (f, fc) = parts::fresh(info);
    let fr = p.text(i.right_top(), Align2::RIGHT_TOP, &f, ty::label(), fc);
    // the time keeps its width; the plan name gives way (and a chip's room)
    let chips_w = if info.incident.is_some() { 150.0 } else { 0.0 };
    let tr = text_fit(&p, i.left_top(), Align2::LEFT_TOP, &info.plan_title(), ty::title(), TEXT(), (fr.left() - sp::M - i.left() - chips_w).max(i.width() * 0.3));
    let x = tr.right() + sp::S;
    incident(ui, Pos2::new(x, tr.center().y), Align2::LEFT_CENTER, info);
    let mut y = i.top() + 16.0 + sp::M;
    // the same words as the provider's own tab
    let state = if !info.scanned {
        Some(ts!("ai.checking").to_string())
    } else if let Some(e) = info.unavailable() {
        Some(if e == Availability::SignedOut { ts!("ai.signed_out") } else { ts!("ai.not_installed") }.to_string())
    } else if !has_usage(info) {
        Some(t!("ai.no_usage_yet", name = ai::meta(info.provider).name).into_owned())
    } else {
        None
    };
    if let Some(s) = state {
        p.text(Pos2::new(i.left(), y), Align2::LEFT_TOP, s, ty::body(), TEXT_DIM());
        return;
    }
    if info.limits.is_empty() {
        let l = &info.local;
        for (k, (lab, v)) in [(ts!("ai.today"), l.tokens_today), (ts!("ai.days_7"), l.tokens_7d), (ts!("ai.hours_5"), l.tokens_5h)].iter().enumerate() {
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
        let col = if parts::runs_out(l) { RED() } else { TEXT_DIM() };
        text_fit(&p, Pos2::new(row.right() - 44.0, row.top() + 1.0), Align2::RIGHT_TOP, &fc, ty::label(), col, row.width() - 170.0);
        parts::pace_bar(&p, Rect::from_min_size(Pos2::new(row.left(), row.top() + 22.0), Vec2::new(row.width(), 5.0)), l);
        y += 38.0;
    }
    let l = &info.local;
    let mut line = Vec::new();
    if show_usd {
        line.push(t!("ai.usd_today", usd = parts::usd(l.usd_today)).into_owned());
    }
    line.push(t!("ai.n_tok", n = ai::human_tokens(l.tokens_today)).into_owned());
    if show_usd {
        line.push(t!("ai.usd_7d", usd = parts::usd(l.usd_7d)).into_owned());
    }
    text_fit(&p, i.left_bottom(), Align2::LEFT_BOTTOM, &line.join(" · "), ty::label(), TEXT_DIM(), i.width());
}
