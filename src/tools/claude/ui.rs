//! The Claude card for the panel and the limits sheet beside it (also the
//! left half of the Claude page).

use crate::kit::sync::LockExt;
use super::backend;
use crate::kit::ai::UsageInfo;
use super::ID;
use crate::core::tool::{Cx, Size};
use crate::kit::tile;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

/// The Cleanup tool, linked to by id only.
pub const CLEANUP: &str = "cleanup";
const GAP: f32 = 12.0;

/// The Claude widget at `size`.
pub fn widget(ui: &mut Ui, r: Rect, size: Size, cl: &UsageInfo, all: &[UsageInfo], sel: bool) -> egui::Response {
    let id = ui.scope_id().with("w-claude");
    let t = ui.ctx().input(|i| i.time);
    let session = cl.session().cloned();
    let pct = session.as_ref().map(|l| l.pct).unwrap_or(0.0);
    let col = super::page::usage_color(pct);
    let value = session.as_ref().map(|_| format!("{pct:.0}%")).unwrap_or_else(|| "–".into());
    match size {
        Size::S11 => tile::small_ring(ui, r, id, Icon::Gauge, pct / 100.0, col, &format!("Session {value}"), sel),
        Size::S12 => {
            let st = status(cl);
            let bar_or = if session.is_some() { Ok(((pct / 100.0).max(0.01), col)) } else { Err(st.as_str()) };
            tile::wide(ui, r, id, Icon::Gauge, tint::CLAUDE(), &format!("{} · session", cl.provider), &value, bar_or, sel)
        }
        Size::S14 => {
            let resp = tile::card(ui, r, id, sel);
            card(ui.painter(), r, cl, t);
            resp
        }
        _ if all.iter().filter(|i| !i.limits.is_empty()).count() >= 2 => {
            let resp = tile::card(ui, r, id, sel);
            providers(ui.painter(), r, all);
            resp
        }
        _ => {
            let resp = tile::card(ui, r, id, sel);
            let p = ui.painter();
            let body = tile::head(p, r, Icon::Gauge, tint::CLAUDE(), "AI Usage", &status(cl));
            for (k, (label, l)) in [("Session", cl.session()), ("Week", cl.weekly())].into_iter().enumerate() {
                let lr = Rect::from_min_size(Pos2::new(body.left(), body.top() + k as f32 * 22.0), Vec2::new(body.width(), 22.0));
                let v = l.map(|l| format!("{:.0}% · {}", l.pct, short_reset(&l.resets).trim_start_matches("resets "))).unwrap_or_default();
                tile::bar_line(p, lr, label, l.map(|l| ((l.pct / 100.0).max(0.01), super::page::usage_color(l.pct))), &v);
            }
            let lr = Rect::from_min_max(Pos2::new(body.left(), body.top() + 52.0), body.max);
            p.text(lr.left_top(), Align2::LEFT_TOP, "Tokens · 7 days", font(11.0), TEXT_FAINT());
            super::parts::day_bars(p, Rect::from_min_max(lr.left_top() + Vec2::new(0.0, 17.0), lr.max), &cl.local.daily, 5.0);
            resp
        }
    }
}

/// Two or more providers with limits: a row each, session and week side
/// by side with the even-use tick, and the dollars of all of them today.
fn providers(p: &egui::Painter, r: Rect, all: &[UsageInfo]) {
    let usd: f64 = all.iter().map(|i| i.local.usd_today).sum();
    let note = if crate::kit::ai::settings::get().show_cost { format!("{} today", super::parts::usd(usd)) } else { String::new() };
    let body = tile::head(p, r, Icon::Gauge, tint::CLAUDE(), "AI limits", &note);
    let rows: Vec<&UsageInfo> = all.iter().filter(|i| !i.limits.is_empty()).take(2).collect();
    let rh = (body.height() / rows.len().max(1) as f32).min(48.0);
    for (k, info) in rows.iter().enumerate() {
        let y = body.top() + k as f32 * rh;
        let mut x = body.left();
        let name = text_fit(p, Pos2::new(x, y), Align2::LEFT_TOP, &info.plan_title(), bold(12.0), TEXT(), body.width() * 0.5);
        super::parts::incident_chip(p, Pos2::new(name.right() + 6.0, name.center().y), Align2::LEFT_CENTER, info);
        let half = (body.width() - 12.0) / 2.0;
        for l in [info.session(), info.weekly()].into_iter().flatten() {
            let lr = Rect::from_min_size(Pos2::new(x, y + 18.0), Vec2::new(half, 22.0));
            let v = p.text(Pos2::new(lr.right(), lr.top()), Align2::RIGHT_TOP, format!("{:.0}%", l.pct), font(12.0), TEXT());
            text_fit(p, lr.left_top(), Align2::LEFT_TOP, &backend::short_label(&l.label), font(11.0), TEXT_FAINT(), v.left() - lr.left() - 4.0);
            super::parts::pace_bar(p, Rect::from_min_size(Pos2::new(lr.left(), lr.top() + 17.0), Vec2::new(half, 4.0)), l);
            x += half + 12.0;
        }
    }
}

fn status(cl: &UsageInfo) -> String {
    if (cl.loading || !cl.scanned) && cl.limits.is_empty() {
        "Checking…".to_string()
    } else if !cl.installed {
        "Not installed".to_string()
    } else if cl.signed_out && cl.limits.is_empty() {
        "Signed out".to_string()
    } else if cl.limits.is_empty() {
        cl.error.clone().unwrap_or_else(|| "No data yet".into())
    } else {
        format!("{} tokens today", backend::human_tokens(cl.local.tokens_today))
    }
}

/// The panel card: plan, tokens today, then Session and Week bars.
pub fn card(p: &egui::Painter, r: Rect, cl: &UsageInfo, t: f64) {
    let x0 = r.left() + 12.0;
    let x1 = r.right() - 12.0;
    let hy = r.top() + 17.0;
    let status = status(cl);
    let (fresh, fc) = super::parts::fresh(cl);
    // the time only when it is old: the rows already carry their own times
    let (status, scol) = if cl.limits.is_empty() || !cl.stale || fresh.is_empty() { (status, white(166)) } else { (fresh, fc) };
    let sw = text_fit(p, Pos2::new(x1, hy), Align2::RIGHT_CENTER, &status, ty::label(), scol, 170.0).width();
    text_fit(p, Pos2::new(x0, hy), Align2::LEFT_CENTER, "AI Usage", ty::title(), TEXT(), (x1 - sw - 12.0) - x0);
    if cl.limits.is_empty() && (cl.loading || !cl.scanned) {
        spinner(p, Pos2::new(x1 - 8.0, r.top() + 56.0), 7.0, t, TEXT_DIM());
    }
    // columns sized by what they hold this frame: the forecast keeps its
    // full width ("Runs out tomorrow 23:40"), the bar takes what is left
    let rows = [("Session", cl.session().cloned()), ("Week", cl.weekly().cloned())];
    let w = |s: &str| p.layout_no_wrap(s.to_string(), font(12.0), TEXT()).size().x;
    let fc: Vec<String> = rows.iter().map(|(_, l)| l.as_ref().map(super::parts::forecast).unwrap_or_default()).collect();
    let fw = fc.iter().map(|f| w(f)).fold(0.0, f32::max);
    let pw = w("100%");
    let lw = rows.iter().map(|(k, _)| w(k)).fold(0.0, f32::max);
    let pct_r = x1 - fw - if fw > 0.0 { sp::M } else { 0.0 };
    let bar_l = x0 + lw + sp::M;
    let bar_r = pct_r - pw - sp::M;
    for (k, (label, l)) in rows.iter().enumerate() {
        let yy = r.top() + 42.0 + k as f32 * 22.0;
        p.text(Pos2::new(x0, yy), Align2::LEFT_CENTER, *label, font(12.0), TEXT_DIM());
        let br = Rect::from_min_max(Pos2::new(bar_l, yy - 2.0), Pos2::new(bar_r.max(bar_l), yy + 2.0));
        let show_bar = br.width() >= 32.0;
        match l {
            Some(l) => {
                if show_bar {
                    super::parts::pace_bar(p, br, l);
                }
                p.text(Pos2::new(pct_r, yy), Align2::RIGHT_CENTER, format!("{:.0}%", l.pct), font(12.0), TEXT());
                let col = if super::parts::forecast(l).starts_with("Runs out") { RED() } else { TEXT_FAINT() };
                p.text(Pos2::new(x1, yy), Align2::RIGHT_CENTER, &fc[k], font(12.0), col);
            }
            None if show_bar => bar(p, br, 0.0, TEXT_DIM()),
            None => {}
        }
    }
}

pub fn short_reset(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = s.split(" (").next().unwrap_or(s);
    format!("resets {}", s)
}

/// The Claude sheet beside the panel: limits, local tokens, what uses the
/// limits, and links to the page and to Cleanup.
pub fn detail(all: &backend::All, ui: &mut Ui, rect: Rect, cx: &mut Cx) {
    let ctx = ui.ctx().clone();
    let cl = backend::current(all).lock_ok().clone();
    let p = ui.painter().clone();
    let t = ctx.input(|i| i.time);
    let busy = cl.loading || !cl.scanned || backend::refreshing();
    let sub = if busy { "Updating…".to_string() } else { super::parts::fresh(&cl).0 };
    if sheet_header(ui, rect, Icon::Gauge, tint::CLAUDE(), "AI Usage", &sub) {
        cx.close_detail();
    }
    let rb = Rect::from_center_size(Pos2::new(rect.right() - 27.0 - 34.0, rect.top() + 29.0), Vec2::splat(26.0));
    if busy {
        spinner(&p, rb.center(), 6.0, t, TEXT_DIM());
        crate::kit::glass::animate(&ctx);
    } else if round_button(ui, rb, Icon::Refresh, "Refresh limits").clicked() {
        backend::request_refresh();
        cx.toast(format!("Asking {} for your limits…", cl.provider));
    }
    let x0 = rect.left() + 16.0;
    let w = rect.width() - 32.0;
    let mut y = rect.top() + SHEET_HEADER_H + 16.0;

    // limits
    let n = cl.limits.len();
    let lim_h = if n == 0 { 64.0 } else { n as f32 * 58.0 + 13.0 };
    let lr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, lim_h));
    glass_card(&p, lr, R_CARD, 0.0, false);
    if n == 0 {
        let msg = cl.error.clone().unwrap_or_else(|| format!("Asking {} for your limits…", cl.provider));
        paragraph(&p, lr.min + Vec2::new(12.0, 12.0), &msg, font(13.0), TEXT_DIM(), w - 24.0);
    }
    for (i, l) in cl.limits.iter().enumerate() {
        let yy = lr.top() + 12.0 + i as f32 * 58.0;
        p.text(Pos2::new(lr.left() + 12.0, yy), Align2::LEFT_TOP, backend::short_label(&l.label), font(13.0), TEXT());
        p.text(Pos2::new(lr.right() - 12.0, yy), Align2::RIGHT_TOP, format!("{:.0}%", l.pct), bold(13.0), TEXT());
        super::parts::pace_bar(&p, Rect::from_min_size(Pos2::new(lr.left() + 12.0, yy + 20.0), Vec2::new(w - 24.0, 6.0)), l);
        let f = super::parts::forecast(l);
        let col = if f.starts_with("Runs out") { RED() } else { TEXT_FAINT() };
        let fr = text_fit(&p, Pos2::new(lr.left() + 12.0, yy + 32.0), Align2::LEFT_TOP, &f, font(12.0), col, w - 24.0);
        if let Some((s, c)) = super::parts::pace_label(l) {
            text_fit(&p, Pos2::new(lr.right() - 12.0, yy + 32.0), Align2::RIGHT_TOP, s, font(12.0), c, lr.right() - 24.0 - fr.right());
        }
    }
    y = lr.bottom() + GAP;

    // on this Mac
    let ar = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 184.0));
    glass_card(&p, ar, R_CARD, 0.0, false);
    p.text(ar.min + Vec2::new(12.0, 12.0), Align2::LEFT_TOP, "On this Mac", bold(13.0), TEXT());
    let stats = [("last 5 hours", cl.local.tokens_5h), ("today", cl.local.tokens_today), ("7 days", cl.local.tokens_7d)];
    for (i, (k, v)) in stats.iter().enumerate() {
        let x = ar.left() + 12.0 + i as f32 * ((w - 24.0) / 3.0);
        p.text(Pos2::new(x, ar.top() + 36.0), Align2::LEFT_TOP, backend::human_tokens(*v), bold(22.0), TEXT());
        p.text(Pos2::new(x, ar.top() + 60.0), Align2::LEFT_TOP, *k, font(11.0), TEXT_FAINT());
    }
    let week: Vec<crate::kit::ai::Day> = cl.local.days.iter().rev().take(7).rev().copied().collect();
    super::parts::day_chart(ui, Rect::from_min_max(Pos2::new(ar.left() + 12.0, ar.top() + 78.0), Pos2::new(ar.right() - 12.0, ar.bottom() - 10.0)), &week, super::parts::Measure::Tokens, None, "detail");
    y = ar.bottom() + GAP;

    // what uses your limits
    let by = rect.bottom() - 16.0 - 30.0;
    let ur_max = by - GAP;
    let mut lines: Vec<(String, Color32, f32)> = Vec::new();
    match cl.contrib.first() {
        Some(c) => {
            lines.push((c.title.clone(), TEXT_FAINT(), 12.0));
            for l in &c.lines {
                lines.push((l.clone(), TEXT_SOFT(), 13.0));
            }
        }
        None if !cl.note.is_empty() && cl.provider != crate::kit::ai::claude::NAME => lines.push((cl.note.clone(), TEXT_DIM(), 13.0)),
        None => lines.push((format!("{} reports this once you've used it on this Mac.", cl.provider), TEXT_DIM(), 13.0)),
    }
    let heights: Vec<f32> = lines.iter().map(|(l, c, s)| p.layout(l.clone(), font(*s), *c, w - 24.0).size().y).collect();
    let mut h = 12.0 + 18.0 + 6.0;
    let mut shown = 0;
    for hh in &heights {
        if y + h + hh + 6.0 + 12.0 > ur_max {
            break;
        }
        h += hh + 6.0;
        shown += 1;
    }
    let ur = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, h + 6.0));
    glass_card(&p, ur, R_CARD, 0.0, false);
    p.text(ur.min + Vec2::new(12.0, 12.0), Align2::LEFT_TOP, if cl.contrib.is_empty() && cl.provider != crate::kit::ai::claude::NAME { "About these limits" } else { "What uses your limits" }, bold(13.0), TEXT());
    let mut ly = ur.top() + 36.0;
    for ((l, c, s), hh) in lines.iter().zip(&heights).take(shown) {
        paragraph(&p, Pos2::new(ur.left() + 12.0, ly), l, font(*s), *c, w - 24.0);
        ly += hh + 6.0;
    }

    // buttons, bottom right (Cleanup knows Claude's caches only)
    let primary = "Clean Claude caches";
    let pw = button_width(&p, primary, BTN_PRIMARY);
    let mut pr = Rect::from_min_size(Pos2::new(rect.right() - 16.0 - pw, by), Vec2::new(pw, 30.0));
    if cl.provider == crate::kit::ai::claude::NAME {
        if pill_button(ui, pr, primary, BTN_PRIMARY, true).clicked() {
            cx.open_page(CLEANUP, "claude");
        }
    } else {
        pr = Rect::from_min_size(Pos2::new(rect.right() - 8.0, by), Vec2::new(0.0, 30.0));
    }
    let sw = button_width(&p, "Open page", BTN_SECONDARY);
    let sr = Rect::from_min_size(Pos2::new(pr.left() - 8.0 - sw, by), Vec2::new(sw, 30.0));
    if pill_button(ui, sr, "Open page", BTN_SECONDARY, true).clicked() {
        cx.open_page(ID, "");
    }
}
