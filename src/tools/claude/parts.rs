//! Pieces the AI usage page, card and sheet share: colours, pace marks,
//! money and freshness labels, and the interactive day chart.

use crate::kit::ai::{alerts, pace, Day, Limit, UsageInfo};
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

/// Bar colour for a usage percentage: the tool's tint, amber from 70%,
/// red from 90%.
pub fn usage_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        RED()
    } else if pct >= 70.0 {
        AMBER()
    } else {
        CLAUDE()
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Colour of a limit: red when ahead of pace or nearly full.
pub fn limit_color(l: &Limit) -> Color32 {
    match pace::of_limit(l, now()) {
        Some(p) if p.state == pace::State::Ahead && l.pct >= 20.0 => RED(),
        _ => usage_color(l.pct),
    }
}

/// "Ahead of pace" / "On pace" / "Room to spare", with its colour.
pub fn pace_label(l: &Limit) -> Option<(&'static str, Color32)> {
    let p = pace::of_limit(l, now())?;
    Some(match p.state {
        pace::State::Ahead => ("Ahead of pace", RED()),
        pace::State::OnPace => ("On pace", TEXT()),
        pace::State::Room => ("Room to spare", GREEN()),
    })
}

/// "Out Sat 14:00" when the current rate runs out before the reset, else
/// when it resets.
pub fn forecast(l: &Limit) -> String {
    let now = now();
    match pace::of_limit(l, now).and_then(|p| p.out_at) {
        Some(at) => format!("Out {}", alerts::when(at, now)),
        None => l.resets_at.map(|r| format!("Resets {}", alerts::when(r, now))).unwrap_or_else(|| reset_short(&l.resets)),
    }
}

/// "21:10" / "Mon 05:00": when a limit resets.
pub fn reset_when(l: &Limit) -> String {
    l.resets_at.map(|r| alerts::when(r, now())).unwrap_or_else(|| l.resets.split(" (").next().unwrap_or("").to_string())
}

fn reset_short(s: &str) -> String {
    let s = s.split(" (").next().unwrap_or(s);
    if s.is_empty() { String::new() } else { format!("Resets {s}") }
}

/// A thin ring: a faint arc up to where even use would be by now, the
/// used share on top in the limit's colour. Running ahead of pace shows as
/// colour past the faint arc; no extra marks.
pub fn pace_ring(p: &egui::Painter, c: Pos2, r: f32, w: f32, l: &Limit) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    // solid colours mixed over the card, so overlapping caps don't
    // double up the way translucent strokes would
    let base = {
        let [r_, g, b, a] = CARD().to_srgba_unmultiplied();
        lerp_color(BG(), Color32::from_rgb(r_, g, b), a as f32 / 255.0)
    };
    let ink = |k: f32| lerp_color(base, TEXT(), k);
    stroke_circle(p, c, r, Stroke::new(w, ink(0.08)));
    let arc = |f: f32, col: Color32| {
        let f = f.clamp(0.0, 1.0);
        if f <= 0.005 {
            return;
        }
        let a0 = -FRAC_PI_2;
        if f >= 0.999 {
            stroke_circle(p, c, r, Stroke::new(w, col));
            return;
        }
        let n = ((f * 96.0) as usize).max(4);
        p.add(Shape::line(arc_points(c, r, a0, a0 + f * TAU, n), Stroke::new(w, col)));
        for q in [c + Vec2::angled(a0) * r, c + Vec2::angled(a0 + f * TAU) * r] {
            p.circle_filled(q, w / 2.0, col);
        }
    };
    if let Some(pc) = pace::of_limit(l, now()) {
        // lighter in light mode, where the same mix reads almost solid
        arc(pc.expected / 100.0, ink(if is_light() { 0.13 } else { 0.22 }));
    }
    arc(l.pct / 100.0, limit_color(l));
}

/// A thin bar with the even-use tick.
pub fn pace_bar(p: &egui::Painter, r: Rect, l: &Limit) {
    bar(p, r, (l.pct / 100.0).max(0.01), limit_color(l));
    if let Some(pc) = pace::of_limit(l, now()) {
        let x = r.left() + r.width() * pc.expected / 100.0;
        p.line_segment([Pos2::new(x, r.top() - 3.0), Pos2::new(x, r.bottom() + 3.0)], Stroke::new(1.5, TEXT()));
    }
}

/// "$38.40", "$664".
pub fn usd(v: f64) -> String {
    if v >= 100.0 { format!("${v:.0}") } else { format!("${v:.2}") }
}

/// "14:20" when the limits were last checked, or "As of 13:52" (amber)
/// when the last check failed and these are the last ones known.
pub fn fresh(i: &UsageInfo) -> (String, Color32) {
    let Some(f) = i.fetched else { return (String::new(), TEXT_FAINT()) };
    let t = f.format("%H:%M");
    if i.stale {
        (format!("As of {t}"), AMBER())
    } else {
        (t.to_string(), TEXT_FAINT())
    }
}

/// A small chip for an outage: "Degraded" (amber) or "Outage" (red).
pub fn incident_chip(p: &egui::Painter, pos: Pos2, align: Align2, i: &UsageInfo) -> Option<Rect> {
    let s = i.incident.as_deref()?;
    let c = if s == "Outage" { RED() } else { AMBER() };
    Some(chip(p, pos, align, s, (with_alpha(c, 0.16), c)))
}

/// What the chart shows per day.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Tokens,
    Usd,
}

/// Bars per day with a hover readout above them: date, $, tokens and
/// requests of the day under the pointer (today when none). `pace` draws a
/// dashed line at that many dollars a day (a budget's even share).
pub fn day_chart(ui: &mut Ui, r: Rect, days: &[Day], m: Measure, pace: Option<f64>, salt: &str) {
    let p = ui.painter().clone();
    if days.is_empty() {
        return;
    }
    let head = 18.0;
    let foot = 16.0;
    let plot = Rect::from_min_max(Pos2::new(r.left(), r.top() + head + sp::S), Pos2::new(r.right(), r.bottom() - foot));
    let val = |d: &Day| match m {
        Measure::Tokens => d.tokens as f64,
        Measure::Usd => d.usd,
    };
    let top = days.iter().map(val).fold(pace.unwrap_or(0.0), f64::max).max(1e-9);
    let n = days.len() as f32;
    let gap = if n > 14.0 { 3.0 } else { 6.0 };
    let bw = ((plot.width() - gap * (n - 1.0)) / n).max(1.0);
    let resp = ui.interact(plot.expand2(Vec2::new(0.0, head)), ui.scope_id().with(("ai-chart", salt)), Sense::hover());
    let hover = resp.hover_pos().map(|q| (((q.x - plot.left()) / (bw + gap)) as usize).min(days.len() - 1));
    let sel = hover.unwrap_or(days.len() - 1);
    for (i, d) in days.iter().enumerate() {
        let h = (plot.height() * (val(d) / top) as f32).max(2.0);
        let x = plot.left() + i as f32 * (bw + gap);
        let br = Rect::from_min_max(Pos2::new(x, plot.bottom() - h), Pos2::new(x + bw, plot.bottom()));
        let over = pace.is_some_and(|pc| val(d) > pc);
        let col = if i == sel {
            CLAUDE()
        } else if over {
            with_alpha(RED(), 0.55)
        } else {
            with_alpha(CLAUDE(), 0.28)
        };
        p.rect_filled(br, 2.0, col);
    }
    if let Some(pc) = pace {
        let y = plot.bottom() - plot.height() * (pc / top) as f32;
        p.extend(Shape::dashed_line(&[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, TEXT_DIM()), 4.0, 3.0));
    }
    // readout
    let d = &days[sel];
    let today = days.last().map(|l| l.date) == Some(d.date);
    let date = if today { "Today".to_string() } else { d.date.format("%a %b %-d").to_string() };
    // one line in two sizes on one baseline: laid out as a single job, so
    // the small grey parts don't ride up next to the bold ones
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = r.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    for (s, f, c) in [
        (date, ty::title(), TEXT()),
        (usd(d.usd), ty::title(), TEXT()),
        (format!("{} tok", crate::kit::ai::human_tokens(d.tokens)), ty::label(), TEXT_DIM()),
        (if d.requests > 0 { format!("{} req", d.requests) } else { String::new() }, ty::label(), TEXT_DIM()),
    ] {
        if s.is_empty() {
            continue;
        }
        let lead = if job.sections.is_empty() { 0.0 } else { sp::M };
        job.append(&s, lead, egui::TextFormat { font_id: f, color: c, valign: egui::Align::BOTTOM, ..Default::default() });
    }
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(Pos2::new(r.left(), r.top() + (head - h) / 2.0), g, TEXT());
    // first and last day under the bars
    let lab = |d: &Day| d.date.format("%b %-d").to_string();
    p.text(Pos2::new(plot.left(), r.bottom()), Align2::LEFT_BOTTOM, lab(&days[0]), ty::caption(), ty::caption_color());
    p.text(Pos2::new(plot.right(), r.bottom()), Align2::RIGHT_BOTTOM, "Today", ty::caption(), ty::caption_color());
}


/// Rows of name, a bar of its share, tokens and dollars.
pub fn share_rows(p: &egui::Painter, r: Rect, rows: &[crate::kit::ai::Share], show_usd: bool, home: &str) {
    // R4: name and value on one line, a 4 pt bar 8 pt under them
    let rh = sp::BAR_ROW_H;
    // one measure for every bar: dollars when shown (a model without a price
    // is 0), else tokens; mixing them made the priced bars look empty
    let usd_bars = show_usd && rows.iter().any(|s| s.usd > 0.0);
    let value = |s: &crate::kit::ai::Share| if usd_bars { s.usd } else { s.tokens as f64 };
    let top = rows.iter().map(value).fold(0.0, f64::max).max(1e-9);
    for (k, s) in rows.iter().enumerate() {
        let y = r.top() + k as f32 * rh;
        if y + rh > r.bottom() + 1.0 {
            break;
        }
        let row = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), rh));
        let right = if show_usd { usd(s.usd) } else { crate::kit::ai::human_tokens(s.tokens) };
        let ty_ = row.top() + 8.0;
        let vr = p.text(Pos2::new(row.right(), ty_), Align2::RIGHT_CENTER, right, ty::label(), TEXT());
        let mut x1 = vr.left() - sp::S;
        if show_usd {
            let tr = p.text(Pos2::new(x1, ty_), Align2::RIGHT_CENTER, crate::kit::ai::human_tokens(s.tokens), ty::label(), TEXT_FAINT());
            x1 = tr.left() - sp::S;
        }
        let name = s.name.replacen(home, "~", 1);
        text_fit(p, Pos2::new(row.left(), ty_), Align2::LEFT_CENTER, &name, ty::body(), TEXT_SOFT(), x1 - row.left());
        let v = value(s);
        bar(p, Rect::from_min_size(Pos2::new(row.left(), ty_ + 8.0 + 8.0), Vec2::new(row.width(), 4.0)), (v / top) as f32, with_alpha(CLAUDE(), 0.7));
    }
}

/// Daily token bars, oldest first; the last (today) solid, the rest faded.
pub fn day_bars(p: &egui::Painter, r: Rect, daily: &[u64], gap: f32) {
    let maxd = daily.iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = daily.len().max(1) as f32;
    let bw = (r.width() - gap * (n - 1.0)) / n;
    for (i, v) in daily.iter().enumerate() {
        let h = (r.height() * (*v as f32 / maxd)).max(2.0);
        let x = r.left() + i as f32 * (bw + gap);
        let br = Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + bw, r.bottom()));
        let last = i + 1 == daily.len();
        p.rect_filled(br, 2.0, if last { CLAUDE() } else { white(46) });
    }
}
