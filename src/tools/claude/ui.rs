//! The AI Usage widgets (W13): what's left, as on the page's limit cards
//! (SP22), with a bar of what's left and no even-pace tick. Small, half
//! and wide show the first provider in the person's order, named with its
//! mark; the tall one shows the providers that are on, in order, up to 3.

use super::backend;
use super::parts;
use crate::core::tool::Size;
use crate::kit::ai::{alerts, pace, Limit, UsageInfo};
use crate::kit::glass::*;
use crate::kit::tile;
use egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

/// The Cleanup tool, linked to by id only.
pub const CLEANUP: &str = "cleanup";

/// The AI Usage widget at `size`: `cl` is the provider the small sizes
/// show, `all` the providers that are on, in order.
pub fn widget(ui: &mut Ui, r: Rect, size: Size, cl: &UsageInfo, all: &[UsageInfo], sel: bool) -> egui::Response {
    let id = ui.scope_id().with("w-claude");
    let lim = main_limit(cl);
    match size {
        Size::S11 => small(ui, r, id, cl, lim, sel),
        Size::S12 => half(ui, r, id, cl, lim, sel),
        Size::S14 => wide(ui, r, id, cl, lim, sel),
        _ => tall(ui, r, id, cl, all, sel),
    }
}

/// The limit that runs out first: the session, else the first one.
fn main_limit(i: &UsageInfo) -> Option<&Limit> {
    i.session().or(i.limits.first())
}

fn left_of(l: &Limit) -> f32 {
    (100.0 - l.pct).clamp(0.0, 100.0)
}

/// When this pace empties `l`, if before it resets.
fn out_at(l: &Limit) -> Option<i64> {
    pace::of_limit(l, parts::now()).and_then(|p| p.out_at)
}

fn fill(l: &Limit) -> Color32 {
    if out_at(l).is_some() { RED() } else { tint::CLAUDE() }
}

/// "81%" (red when it runs out first) or "–".
fn value(l: Option<&Limit>) -> (String, Color32) {
    match l {
        Some(l) => (format!("{:.0}%", left_of(l)), if out_at(l).is_some() { RED() } else { TEXT() }),
        None => ("–".into(), TEXT()),
    }
}

/// "+19% at 18:04", red "Runs out 16:10", "Not started", or the status.
fn caption(i: &UsageInfo, l: Option<&Limit>) -> (String, Color32) {
    let Some(l) = l else { return (status(i), ty::caption_color()) };
    let reset = parts::reset_when(l);
    if l.pct <= 0.0 && reset.is_empty() {
        (ts!("ai.not_started").into(), ty::caption_color())
    } else if let Some(at) = out_at(l) {
        (t!("ai.runs_out", when = alerts::when(at, parts::now())).into_owned(), RED())
    } else if !reset.is_empty() {
        (t!("ai.restores_at", pct = l.pct : {:.0}, when = reset).into_owned(), ty::caption_color())
    } else {
        (parts::forecast(l), ty::caption_color())
    }
}

/// The 4 pt bar of what's left.
fn left_bar(p: &egui::Painter, r: Rect, l: Option<&Limit>) {
    match l {
        Some(l) => {
            let left = left_of(l);
            bar(p, r, (left / 100.0).max(if left > 0.0 { 0.01 } else { 0.0 }), fill(l));
        }
        None => bar(p, r, 0.0, tint::CLAUDE()),
    }
}

/// The provider's mark (12) and name at `pos` (left centre). Returns its rect.
fn name_line(p: &egui::Painter, pos: Pos2, i: &UsageInfo, f: egui::FontId, col: Color32, w: f32) -> Rect {
    let mr = Rect::from_min_size(Pos2::new(pos.x, pos.y - 6.0), Vec2::splat(12.0));
    super::page::provider_mark(p, mr, i.provider);
    let t = text_fit(p, Pos2::new(mr.right() + 5.0, pos.y), Align2::LEFT_CENTER, i.provider, f, col, w - 17.0);
    mr.union(t)
}

/// Small (1:1): the provider, "81%", the bar. The caption is in the tip.
fn small(ui: &mut Ui, r: Rect, id: egui::Id, cl: &UsageInfo, lim: Option<&Limit>, sel: bool) -> egui::Response {
    let resp = tile::card(ui, r, id, sel);
    let p = ui.painter();
    let x = r.left() + tile::SMALL_SIDE;
    let w = r.width() - 2.0 * tile::SMALL_SIDE;
    // name 15, 2, value 20, 6, bar 4: centred
    let top = r.center().y - (15.0 + 2.0 + 20.0 + 6.0 + 4.0) / 2.0;
    name_line(p, Pos2::new(x, top + 7.5), cl, ty::label(), ty::caption_color(), w);
    let (v, vc) = value(lim);
    p.text(Pos2::new(x, top + 17.0 + 10.0), Align2::LEFT_CENTER, v, bold(15.0), vc);
    left_bar(p, Rect::from_min_size(Pos2::new(x, top + 43.0), Vec2::new(w, 4.0)), lim);
    let (cap, _) = caption(cl, lim);
    resp.on_hover_text(cap)
}

/// "81%" and a dim "left" after it, at `pos` (left centre). Returns its rect.
fn value_left(p: &egui::Painter, pos: Pos2, lim: Option<&Limit>) -> Rect {
    let (v, vc) = value(lim);
    let vr = p.text(pos, Align2::LEFT_CENTER, v, ty::lead(), vc);
    if lim.is_none() {
        return vr;
    }
    vr.union(p.text(Pos2::new(vr.right() + 4.0, vr.bottom() - 2.0), Align2::LEFT_BOTTOM, ts!("ai.left"), ty::label(), TEXT_DIM()))
}

/// Half (1:2): the glyph; the provider, "81% left" and the bar beside it.
fn half(ui: &mut Ui, r: Rect, id: egui::Id, cl: &UsageInfo, lim: Option<&Limit>, sel: bool) -> egui::Response {
    let resp = tile::card(ui, r, id, sel);
    let p = ui.painter();
    let x0 = tile::visual(p, r, Icon::Gauge, tint::CLAUDE(), None);
    let w = r.right() - tile::PAD - x0;
    // name 15, 2, value 20, 6, bar 4
    let top = r.center().y - (15.0 + 2.0 + 20.0 + 6.0 + 4.0) / 2.0;
    name_line(p, Pos2::new(x0, top + 7.5), cl, ty::label(), ty::caption_color(), w);
    value_left(p, Pos2::new(x0, top + 27.0), lim);
    left_bar(p, Rect::from_min_size(Pos2::new(x0, top + 43.0), Vec2::new(w, 4.0)), lim);
    resp
}

/// One limit as a line: its name and "81%" over a bar of what's left.
fn limit_line(p: &egui::Painter, r: Rect, label: &str, l: Option<&Limit>, suffix: bool) {
    let (v, vc) = value(l);
    let v = if suffix && l.is_some() { format!("{v} {}", ts!("ai.left")) } else { v };
    let vr = p.text(Pos2::new(r.right(), r.top()), Align2::RIGHT_TOP, v, ty::label(), vc);
    text_fit(p, r.left_top(), Align2::LEFT_TOP, label, ty::label(), TEXT_DIM(), vr.left() - r.left() - 6.0);
    left_bar(p, Rect::from_min_size(Pos2::new(r.left(), r.top() + 20.0), Vec2::new(r.width(), 4.0)), l);
}

/// The two limits a provider shows side by side: session and week, else
/// its first two.
fn two_limits(i: &UsageInfo) -> Vec<(String, &Limit)> {
    let mut v: Vec<&Limit> = [i.session(), i.weekly()].into_iter().flatten().collect();
    if v.is_empty() {
        v = i.limits.iter().take(2).collect();
    }
    v.into_iter().map(|l| (backend::short_label(&l.label), l)).collect()
}

/// Wide (1:4): the provider's main limit on the left ("81% left",
/// "+19% at 18:04"); session and week as lines on the right.
fn wide(ui: &mut Ui, r: Rect, id: egui::Id, cl: &UsageInfo, lim: Option<&Limit>, sel: bool) -> egui::Response {
    let resp = tile::card(ui, r, id, sel);
    let p = ui.painter();
    let split = r.center().x.round();
    let x0 = tile::visual(p, r, Icon::Gauge, tint::CLAUDE(), None);
    let w = split - tile::PAD - x0;
    let top = r.center().y - (15.0 + 2.0 + 20.0 + 2.0 + 14.0) / 2.0;
    name_line(p, Pos2::new(x0, top + 7.5), cl, ty::label(), ty::caption_color(), w);
    value_left(p, Pos2::new(x0, top + 27.0), lim);
    let (cap, cc) = caption(cl, lim);
    text_fit(p, Pos2::new(x0, top + 39.0 + 7.0), Align2::LEFT_CENTER, &cap, ty::caption(), cc, w);
    let inset = r.height() * 0.06;
    p.line_segment([Pos2::new(split, r.top() + inset), Pos2::new(split, r.bottom() - inset)], egui::Stroke::new(1.0, white(HAIRLINE_STRONG)));
    let right = Rect::from_min_max(Pos2::new(split + tile::PAD, r.top()), Pos2::new(r.right() - tile::PAD, r.bottom()));
    let lines = two_limits(cl);
    if lines.is_empty() {
        tile::empty_note(p, right, &status(cl));
        return resp;
    }
    // lines 24 tall, 9 apart, centred
    let h = lines.len() as f32 * 24.0 + (lines.len() - 1) as f32 * 9.0;
    for (k, (label, l)) in lines.iter().enumerate() {
        let lr = Rect::from_min_size(Pos2::new(right.left(), right.center().y - h / 2.0 + k as f32 * 33.0), Vec2::new(right.width(), 24.0));
        limit_line(p, lr, label, Some(l), true);
    }
    resp
}

/// Tall (2:4): "AI limits" and today's dollars; then each provider that's
/// on, in order (up to 3): its mark, name, plan, and its two limits side by
/// side. One provider adds its tokens of the last 7 days.
fn tall(ui: &mut Ui, r: Rect, id: egui::Id, cl: &UsageInfo, all: &[UsageInfo], sel: bool) -> egui::Response {
    let resp = tile::card(ui, r, id, sel);
    let p = ui.painter();
    let usd: f64 = all.iter().map(|i| i.local.usd_today).sum();
    let note = if crate::kit::ai::settings::get().show_cost && usd > 0.0 { t!("ai.usd_today", usd = parts::usd(usd)).into_owned() } else { String::new() };
    let body = tile::head(p, r, Icon::Gauge, tint::CLAUDE(), ts!("ai.widget_title"), &note);
    let with: Vec<&UsageInfo> = all.iter().filter(|i| !i.limits.is_empty()).collect();
    let rows: Vec<&UsageInfo> = if with.is_empty() { vec![cl] } else { with };
    // a provider takes name 16, 6, a line 24; 10 between
    const ROW: f32 = 16.0 + 6.0 + 24.0;
    let n = rows.len().min(3).min(((body.height() + 10.0) / (ROW + 10.0)).floor().max(1.0) as usize);
    let half = (body.width() - 12.0) / 2.0;
    for (k, info) in rows.iter().take(n).enumerate() {
        let y = body.top() + k as f32 * (ROW + 10.0);
        let nr = name_line(p, Pos2::new(body.left(), y + 8.0), info, bold(12.0), TEXT(), body.width() * 0.5);
        let mut x = nr.right() + sp::S;
        let title = info.plan_title();
        let plan = title.strip_prefix(info.provider).unwrap_or("").trim();
        if !plan.is_empty() {
            x = p.text(Pos2::new(x, y + 8.0), Align2::LEFT_CENTER, plan, ty::label(), TEXT_DIM()).right() + sp::S;
        }
        parts::incident_chip(p, Pos2::new(x, y + 8.0), Align2::LEFT_CENTER, info);
        let lines = two_limits(info);
        if lines.is_empty() {
            p.text(Pos2::new(body.left(), y + 22.0), Align2::LEFT_TOP, status(info), ty::caption(), ty::caption_color());
        }
        for (j, (label, l)) in lines.iter().enumerate() {
            let lr = Rect::from_min_size(Pos2::new(body.left() + j as f32 * (half + 12.0), y + 22.0), Vec2::new(half, 24.0));
            limit_line(p, lr, label, Some(l), true);
        }
    }
    if n == 1 {
        let lr = Rect::from_min_max(Pos2::new(body.left(), body.top() + ROW + 14.0), body.max);
        if lr.height() > 40.0 {
            p.text(lr.left_top(), Align2::LEFT_TOP, ts!("ai.tokens_7d"), font(11.0), TEXT_FAINT());
            parts::day_bars(p, Rect::from_min_max(lr.left_top() + Vec2::new(0.0, 17.0), lr.max), &rows[0].local.daily, 5.0);
        }
    }
    resp
}

fn status(cl: &UsageInfo) -> String {
    if (cl.loading || !cl.scanned) && cl.limits.is_empty() {
        ts!("ai.checking").to_string()
    } else if !cl.installed {
        ts!("ai.not_installed").to_string()
    } else if cl.signed_out && cl.limits.is_empty() {
        ts!("ai.signed_out").to_string()
    } else if cl.limits.is_empty() {
        cl.error.clone().unwrap_or_else(|| ts!("ai.no_data").into())
    } else {
        t!("ai.tokens_today", n = backend::human_tokens(cl.local.tokens_today)).into_owned()
    }
}
