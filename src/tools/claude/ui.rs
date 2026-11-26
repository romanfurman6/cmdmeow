//! The AI Usage card for the panel.

use super::backend;
use crate::kit::ai::UsageInfo;
use crate::core::tool::Size;
use crate::kit::tile;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

/// The Cleanup tool, linked to by id only.
pub const CLEANUP: &str = "cleanup";

/// The Claude widget at `size`.
pub fn widget(ui: &mut Ui, r: Rect, size: Size, cl: &UsageInfo, all: &[UsageInfo], sel: bool) -> egui::Response {
    let id = ui.scope_id().with("w-claude");
    let session = cl.session().cloned();
    let pct = session.as_ref().map(|l| l.pct).unwrap_or(0.0);
    let col = super::page::usage_color(pct);
    let value = session.as_ref().map(|_| format!("{pct:.0}%")).unwrap_or_else(|| "–".into());
    match size {
        // W4: the ring and the number; Session and its reset in the tip
        Size::S11 => {
            let tip = session.as_ref().map(|l| t!("ai.session_resets", when = super::parts::reset_when(l)).into_owned()).unwrap_or_else(|| status(cl));
            tile::small_ring(ui, r, id, Icon::Gauge, pct / 100.0, col, &value, sel).on_hover_text(tip)
        }
        // W3, W8: "AI Usage", the session %, when it resets
        Size::S12 | Size::S14 => {
            let ring_ = session.as_ref().map(|_| ((pct / 100.0).max(0.01), col));
            let caption = match &session {
                Some(l) => {
                    // the same time as the page and the sheet ("Resets 20:29",
                    // C6); another day reads "Resets Sun 08:52": where that
                    // doesn't fit beside the ring, the day alone (the hour
                    // is in the sheet)
                    let when = super::parts::reset_when(l);
                    let full = t!("ai.resets", when = when).into_owned();
                    let room = if size == Size::S12 { r.width() } else { tile::ROW_SPLIT } - (tile::PAD + 50.0 + tile::PAD) - tile::PAD;
                    let fits = ui.painter().layout_no_wrap(full.clone(), ty::caption(), TEXT()).size().x <= room;
                    if when.is_empty() { ts!("ai.not_started").to_string() } else if fits { full } else { t!("ai.resets", when = when.split(' ').next().unwrap_or(&when)).into_owned() }
                }
                None => status(cl),
            };
            let value = if session.is_some() { value } else { "–".to_string() };
            if size == Size::S12 {
                return tile::wide_full(ui, r, id, Icon::Gauge, tint::CLAUDE(), ring_, ts!("ai.name"), &value, &caption, ty::caption_color(), sel);
            }
            // the right part adds what the left doesn't have: the week and today's tokens
            let (resp, right) = tile::row_split_full(ui, r, id, Icon::Gauge, tint::CLAUDE(), ring_, ts!("ai.name"), &value, &caption, ty::caption_color(), sel);
            let mut lines: Vec<(String, Option<(f32, Color32)>, String)> = Vec::new();
            if let Some(w) = cl.weekly() {
                lines.push((ts!("ai.week").into(), Some(((w.pct / 100.0).max(0.01), super::page::usage_color(w.pct))), format!("{:.0}%", w.pct)));
            }
            if cl.local.tokens_today > 0 {
                lines.push((ts!("ai.today").into(), None, t!("ai.n_tokens", n = backend::human_tokens(cl.local.tokens_today)).into_owned()));
            }
            tile::lines(ui.painter(), right, &lines, &status(cl));
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
            let body = tile::head(p, r, Icon::Gauge, tint::CLAUDE(), ts!("ai.name"), &status(cl));
            let rows: Vec<(&str, _, String)> = [(ts!("ai.session"), cl.session()), (ts!("ai.week"), cl.weekly())]
                .into_iter()
                .map(|(label, l)| (label, l, l.map(|l| format!("{:.0}% · {}", l.pct, super::parts::reset_when(l))).unwrap_or_default()))
                .collect();
            let cols = tile::bar_cols(p, &rows.iter().map(|r| (r.0, r.2.as_str())).collect::<Vec<_>>());
            for (k, (label, l, v)) in rows.iter().enumerate() {
                let lr = Rect::from_min_size(Pos2::new(body.left(), body.top() + k as f32 * 22.0), Vec2::new(body.width(), 22.0));
                tile::bar_line(p, lr, label, l.map(|l| ((l.pct / 100.0).max(0.01), super::page::usage_color(l.pct))), v, cols);
            }
            let lr = Rect::from_min_max(Pos2::new(body.left(), body.top() + 52.0), body.max);
            p.text(lr.left_top(), Align2::LEFT_TOP, ts!("ai.tokens_7d"), font(11.0), TEXT_FAINT());
            super::parts::day_bars(p, Rect::from_min_max(lr.left_top() + Vec2::new(0.0, 17.0), lr.max), &cl.local.daily, 5.0);
            resp
        }
    }
}

/// Two or more providers with limits: a row each, session and week side
/// by side with the even-use tick, and the dollars of all of them today.
fn providers(p: &egui::Painter, r: Rect, all: &[UsageInfo]) {
    let usd: f64 = all.iter().map(|i| i.local.usd_today).sum();
    let note = if crate::kit::ai::settings::get().show_cost { t!("ai.usd_today", usd = super::parts::usd(usd)).into_owned() } else { String::new() };
    let body = tile::head(p, r, Icon::Gauge, tint::CLAUDE(), ts!("ai.widget_title"), &note);
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
