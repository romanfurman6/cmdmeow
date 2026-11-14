//! AI Check as a chip in the page header, after the title: "Check with
//! Claude" before a check, "Checking… Stop" while it runs, "3 tips" (or "All
//! good") after. Clicking the tips chip opens a popover with one row per
//! recommendation. The page itself never moves. The chip shows only when the
//! tool has found something, the main agent can run and the page isn't locked.

use super::{Check, Phase, Rec};
use crate::kit::agent::Agent;
use crate::kit::glass::*;
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const CHIP_H: f32 = 26.0;
const POP_W: f32 = 560.0;
const POP_HEAD_H: f32 = 44.0;
const POP_SEC_H: f32 = 28.0;

/// What the person pressed.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Run the check now: the tool builds its [`super::Ask`].
    Start,
    /// A recommendation's button.
    Act(Rec),
}

/// Not on a locked page (a missing permission disables it), and only once the
/// tool has something to check and the main agent can run (screenshots: always).
pub fn shown(c: &Check, ui: &Ui, has_data: bool) -> bool {
    ui.is_enabled() && (c.demo || (has_data && crate::kit::agent::main().ready()))
}

fn agent_of(c: &Check) -> Agent {
    if c.demo { Agent::Claude } else { crate::kit::agent::main() }
}

/// The chip's text for the current phase.
fn chip_label(c: &Check) -> String {
    match c.phase() {
        Phase::Idle => format!("Check with {}", agent_of(c).name()),
        Phase::Running => "Checking…".into(),
        Phase::Failed(_) => "Check failed".into(),
        Phase::Done if c.inline => match c.tips().len() {
            0 => format!("Checked by {}", agent_of(c).name()),
            1 => format!("Checked by {} · 1 tip", agent_of(c).name()),
            n => format!("Checked by {} · {n} tips", agent_of(c).name()),
        },
        Phase::Done => {
            let recs = c.recs();
            let a = recs.iter().filter(|r| c.is_answer(r)).count();
            let t = recs.len() - a;
            let tips = |n: usize| if n == 1 { "1 tip".to_string() } else { format!("{n} tips") };
            match (a, t) {
                (0, 0) => "All good".into(),
                (0, t) => tips(t),
                (1, 0) => "1 answer".into(),
                (a, 0) => format!("{a} answers"),
                (a, t) => format!("{a} answer{} · {}", if a == 1 { "" } else { "s" }, tips(t)),
            }
        }
    }
}

/// Width the chip takes in the header (0 when it doesn't show): pass it to
/// [`page_header_room`], then draw it with [`chip`] at the point that returns.
pub fn chip_width(c: &Check, ui: &Ui, has_data: bool) -> f32 {
    if !shown(c, ui, has_data) {
        return 0.0;
    }
    let p = ui.painter();
    let mut w = 10.0 + 14.0 + 6.0 + p.layout_no_wrap(chip_label(c), bold(12.0), TEXT()).size().x + 12.0;
    if c.phase() == Phase::Running {
        w += 8.0 + p.layout_no_wrap("Stop".into(), font(12.0), TEXT()).size().x;
    }
    w
}

/// The chip, its left edge centred on `at`, and its popover when open.
pub fn chip(c: &mut Check, ui: &mut Ui, at: Pos2, has_data: bool) -> Option<Event> {
    if !shown(c, ui, has_data) {
        c.hovered = None;
        c.open = false;
        return None;
    }
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let ai = tint::HEALTH();
    let phase = c.phase();
    let w = chip_width(c, ui, has_data);
    let r = Rect::from_min_size(Pos2::new(at.x, at.y - CHIP_H / 2.0), Vec2::new(w, CHIP_H));
    let id = ui.scope_id().with(("ai-chip", c.tool));
    let resp = ui.interact(r, id, Sense::click());
    let h = ctx.animate_bool(id.with("h"), resp.hovered());
    let filled = matches!(phase, Phase::Done) && !(if c.inline { c.tips() } else { c.recs() }).is_empty() || c.open;
    let fill = if filled { with_alpha(ai, 0.12 + 0.06 * h) } else { white((8.0 * h) as u8) };
    p.rect_filled(r, CHIP_H / 2.0, fill);
    p.rect_stroke(r, CHIP_H / 2.0, Stroke::new(1.0, with_alpha(ai, if filled { 0.45 } else { 0.3 })), StrokeKind::Inside);
    let gx = r.left() + 10.0 + 7.0;
    if phase == Phase::Running {
        spinner(&p, Pos2::new(gx, r.center().y), 5.5, ui.input(|i| i.time), ai);
        crate::kit::glass::poll(&ctx);
    } else if phase == Phase::Idle {
        agent_mark(&p, Rect::from_center_size(Pos2::new(gx, r.center().y), Vec2::splat(12.0)), agent_of(c), None);
    } else {
        sparkle(&p, Pos2::new(gx, r.center().y), 14.0, if matches!(phase, Phase::Failed(_)) { AMBER() } else { ai });
    }
    let t = p.text(Pos2::new(gx + 7.0 + 6.0, r.center().y), Align2::LEFT_CENTER, chip_label(c), bold(12.0), TEXT());
    if phase == Phase::Running {
        p.text(Pos2::new(t.right() + 8.0, r.center().y), Align2::LEFT_CENTER, "Stop", font(12.0), if resp.hovered() { TEXT() } else { TEXT_DIM() });
    }
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut ev = None;
    if resp.clicked() {
        match phase {
            Phase::Idle => ev = Some(Event::Start),
            Phase::Running => c.stop(),
            Phase::Done | Phase::Failed(_) => c.open = !c.open,
        }
    }
    if c.open && matches!(phase, Phase::Done | Phase::Failed(_)) {
        if let Some(e) = popover(c, ui, r) {
            ev = Some(e);
        }
    } else {
        c.open = false;
        c.hovered = None;
    }
    ev
}

/// The recommendations under the chip, above the page. Closes on a click
/// outside it or Escape.
fn popover(c: &mut Check, ui: &mut Ui, chip: Rect) -> Option<Event> {
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let phase = c.phase();
    // inline: the page shows the answers and tips, the popover only the headline
    let recs = if phase == Phase::Done && !c.inline { c.recs() } else { vec![] };
    let w = POP_W.min(screen.width() - 2.0 * sp::L);
    let answers: Vec<Rec> = recs.iter().filter(|r| c.is_answer(r)).cloned().collect();
    let others: Vec<Rec> = recs.iter().filter(|r| !c.is_answer(r)).cloned().collect();
    let sections = if answers.is_empty() { 0.0 } else if others.is_empty() { 1.0 } else { 2.0 };
    let (line, col) = match &phase {
        Phase::Failed(e) => (e.clone(), AMBER()),
        _ => {
            let hl = c.headline();
            (if recs.is_empty() && hl.is_empty() { "Nothing to do here".to_string() } else { hl }, TEXT_DIM())
        }
    };
    // the agent's words wrap, never cut (R10): rows grow to fit them
    let pm = ui.painter().clone();
    let line_h = wrapped_h(&pm, &line, ty::body(), w - 28.0, 3);
    let head_h = POP_HEAD_H + line_h + sp::M;
    let row_hs: Vec<f32> = answers.iter().chain(others.iter()).map(|r| rec_height(&pm, c, r, w)).collect();
    let h = head_h + row_hs.iter().sum::<f32>() + sections * POP_SEC_H;
    let x = chip.left().min(screen.right() - sp::L - w).max(screen.left() + sp::L);
    let rect = Rect::from_min_size(Pos2::new(x, chip.bottom() + 6.0), Vec2::new(w, h));
    let mut ev = None;
    let area = egui::Area::new(ui.scope_id().with(("ai-pop", c.tool))).order(egui::Order::Foreground).fixed_pos(rect.min).show(&ctx, |ui| {
        let (rr, _) = ui.allocate_exact_size(rect.size(), Sense::click());
        let p = ui.painter().clone();
        p.rect_filled(rr, 12.0, POPOVER());
        p.rect_stroke(rr, 12.0, Stroke::new(1.0, white(30)), StrokeKind::Inside);
        // header: sparkle, AI Check, Check again; the headline (or the error) under it
        let cy = rr.top() + POP_HEAD_H / 2.0;
        sparkle(&p, Pos2::new(rr.left() + 14.0 + 7.0, cy), 14.0, tint::HEALTH());
        p.text(Pos2::new(rr.left() + 14.0 + 20.0, cy), Align2::LEFT_CENTER, "AI Check", ty::title(), TEXT());
        let again = if matches!(phase, Phase::Failed(_)) { "Try again" } else { "Check again" };
        let aw = p.layout_no_wrap(again.into(), ty::body(), TEXT()).size().x;
        let ar = Rect::from_min_size(Pos2::new(rr.right() - 14.0 - aw, cy - 10.0), Vec2::new(aw, 20.0));
        if ghost(ui, ar, again) {
            ev = Some(Event::Start);
        }
        text_wrap(&p, Pos2::new(rr.left() + 14.0, rr.top() + POP_HEAD_H - sp::XS), Align2::LEFT_TOP, &line, ty::body(), col, rr.width() - 28.0, 3);
        c.hovered = None;
        let mut y = rr.top() + head_h;
        let mut k = 0;
        // what waited on the user first, under its own label; then the rest
        let groups: [(&str, &Vec<Rec>); 2] = [("Needs your OK", &answers), ("Also", &others)];
        for (label, list) in groups {
            if list.is_empty() {
                continue;
            }
            if sections > 0.0 {
                hairline(&p, Pos2::new(rr.left(), y), Pos2::new(rr.right(), y));
                let t = if label == "Also" { label.to_string() } else { format!("{label} · {}", list.len()) };
                p.text(Pos2::new(rr.left() + 14.0, y + POP_SEC_H / 2.0), Align2::LEFT_CENTER, t, ty::label(), TEXT_FAINT());
                y += POP_SEC_H;
            }
            for r in list.iter() {
                let row = Rect::from_min_size(Pos2::new(rr.left(), y), Vec2::new(rr.width(), row_hs[k]));
                if let Some(e) = rec_row(c, ui, row, r) {
                    ev = Some(e);
                }
                y += row_hs[k];
                k += 1;
            }
        }
    });
    // close on a click elsewhere (the chip toggles itself) or Escape
    let clicked_out = ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| !area.response.rect.contains(pos) && !chip.contains(pos));
    if clicked_out || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        c.open = false;
        c.hovered = None;
    }
    if let Some(Event::Act(_)) = ev {
        c.open = false;
    }
    ev
}

/// Width of an [`agent_button`] with `label`.
pub fn agent_button_width(ui: &Ui, label: &str) -> f32 {
    10.0 + 14.0 + 6.0 + ui.painter().layout_no_wrap(label.to_owned(), bold(12.0), TEXT()).size().x + 12.0
}

/// A chip in AI Check's idle look (agent mark + label) that does one
/// thing, its left edge centred on `at`: "Ask Claude to set up".
pub fn agent_button(ui: &mut Ui, at: Pos2, id: egui::Id, a: Agent, label: &str) -> egui::Response {
    let p = ui.painter().clone();
    let ai = tint::HEALTH();
    let r = Rect::from_min_size(Pos2::new(at.x, at.y - CHIP_H / 2.0), Vec2::new(agent_button_width(ui, label), CHIP_H));
    let resp = ui.interact(r, id, Sense::click());
    let h = ui.ctx().animate_bool(id.with("h"), resp.hovered());
    p.rect_filled(r, CHIP_H / 2.0, white((8.0 * h) as u8));
    p.rect_stroke(r, CHIP_H / 2.0, Stroke::new(1.0, with_alpha(ai, 0.3 + 0.15 * h)), StrokeKind::Inside);
    let gx = r.left() + 10.0 + 7.0;
    agent_mark(&p, Rect::from_center_size(Pos2::new(gx, r.center().y), Vec2::splat(12.0)), a, None);
    p.text(Pos2::new(gx + 7.0 + 6.0, r.center().y), Align2::LEFT_CENTER, label, bold(12.0), TEXT());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The agent's mark: Claude's orange burst, Codex's prompt square.
pub fn agent_mark(p: &egui::Painter, r: Rect, a: Agent, ink: Option<Color32>) {
    match a {
        Agent::Claude => {
            let c = r.center();
            let rad = r.width() * 0.46;
            let col = ink.unwrap_or(Color32::from_rgb(0xD9, 0x77, 0x57));
            for k in 0..4 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                let d = Vec2::angled(ang) * rad;
                p.line_segment([c - d, c + d], Stroke::new((r.width() * 0.13).max(1.3), col));
            }
        }
        Agent::Codex => {
            p.rect_filled(r, r.width() * 0.24, ink.unwrap_or(Color32::BLACK));
            p.rect_stroke(r, r.width() * 0.24, Stroke::new(1.0, white(70)), egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, ">_", mono(r.width() * 0.5), if ink.is_some() { BG() } else { Color32::WHITE });
        }
    }
}

fn sparkle(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    icon(p, Icon::Sparkle, Rect::from_center_size(c, Vec2::splat(s)), col);
}

/// Text-only button: dim, brighter on hover. True when clicked.
fn ghost(ui: &mut Ui, r: Rect, label: &str) -> bool {
    let resp = ui.interact(r, ui.scope_id().with(("ai-ghost", label, r.min.x as i32, r.min.y as i32)), Sense::click());
    let col = if resp.hovered() { TEXT() } else { TEXT_DIM() };
    ui.painter().text(Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, ty::body(), col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Height of `text` wrapped to `w` on at most `rows` lines.
fn wrapped_h(p: &egui::Painter, text: &str, font: egui::FontId, w: f32, rows: usize) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, TEXT(), w.max(1.0));
    job.wrap.max_rows = rows;
    p.layout_job(job).size().y
}

/// A recommendation's buttons: the agent's pick, then Skip (an answer: the
/// other choice). Their labels and the width they take on the right.
fn rec_buttons(p: &egui::Painter, c: &Check, r: &Rec) -> (Option<(&'static str, &'static str)>, f32) {
    let alt = if c.is_answer(r) { c.actions.iter().find(|(id, _)| *id != r.action).copied() } else { None };
    let second = match alt {
        Some((_, label)) => button_width(p, label, BTN_SECONDARY),
        None => p.layout_no_wrap("Skip".into(), ty::body(), TEXT()).size().x,
    };
    (alt, 14.0 + second + sp::M + button_width(p, &r.label, BTN_PRIMARY))
}

/// Where a recommendation's text goes in a row `w` wide: its left edge
/// (from the row's) and its width.
fn rec_text_span(p: &egui::Painter, c: &Check, r: &Rec, w: f32) -> (f32, f32) {
    let (_, bw) = rec_buttons(p, c, r);
    let tx = 14.0 + 24.0 + sp::M;
    (tx, w - tx - bw - sp::L)
}

/// A row's height: title (with its value) over why, both wrapped.
fn rec_height(p: &egui::Painter, c: &Check, r: &Rec, w: f32) -> f32 {
    let (_, tw) = rec_text_span(p, c, r, w);
    let vw = if r.value.is_empty() { 0.0 } else { p.layout_no_wrap(r.value.clone(), ty::title(), TEXT()).size().x + sp::M };
    let th = wrapped_h(p, &r.title, ty::title(), tw - vw, 2);
    let wh = wrapped_h(p, &r.why, ty::label(), tw, 3);
    (2.0 * sp::M + th + if wh > 0.0 { sp::LINE_GAP + wh } else { 0.0 }).max(sp::ROW2_H)
}

/// One recommendation under a hairline: icon, title and value, why under
/// them (both wrap), then its button and Skip.
fn rec_row(c: &mut Check, ui: &mut Ui, rr: Rect, r: &Rec) -> Option<Event> {
    let p = ui.painter().clone();
    let cy = rr.center().y;
    hairline(&p, rr.left_top(), rr.right_top());
    if ui.rect_contains_pointer(rr) {
        c.hovered = Some(r.key.clone());
        p.rect_filled(rr.shrink2(Vec2::new(4.0, 2.0)), R_SMALL, white(7));
    }
    let ic = Rect::from_center_size(Pos2::new(rr.left() + 14.0 + 12.0, cy), Vec2::splat(24.0));
    rec_icon(c, ui.ctx(), &p, ic, r);
    let mut ev = None;
    let (alt, _) = rec_buttons(&p, c, r);
    let sr = if let Some((id, label)) = alt {
        let aw = button_width(&p, label, BTN_SECONDARY);
        let ar = Rect::from_min_size(Pos2::new(rr.right() - 14.0 - aw, cy - sp::CONTROL_H / 2.0), Vec2::new(aw, sp::CONTROL_H));
        if pill_button(ui, ar, label, BTN_SECONDARY, true).clicked() {
            ev = Some(Event::Act(Rec { action: id.to_string(), label: label.to_string(), ..r.clone() }));
        }
        ar
    } else {
        let sw = p.layout_no_wrap("Skip".into(), ty::body(), TEXT()).size().x;
        let sr = Rect::from_min_size(Pos2::new(rr.right() - 14.0 - sw, cy - 10.0), Vec2::new(sw, 20.0));
        if ghost(ui, sr, "Skip") {
            c.skip(r);
        }
        sr
    };
    let bw = button_width(&p, &r.label, BTN_PRIMARY);
    let br = Rect::from_min_size(Pos2::new(sr.left() - sp::M - bw, cy - sp::CONTROL_H / 2.0), Vec2::new(bw, sp::CONTROL_H));
    if pill_button(ui, br, &r.label, BTN_PRIMARY, true).clicked() {
        ev = Some(Event::Act(r.clone()));
    }
    let (tx, tw) = rec_text_span(&p, c, r, rr.width());
    let tx = rr.left() + tx;
    let vw = if r.value.is_empty() { 0.0 } else { p.layout_no_wrap(r.value.clone(), ty::title(), TEXT()).size().x + sp::M };
    let th = wrapped_h(&p, &r.title, ty::title(), tw - vw, 2);
    let wh = wrapped_h(&p, &r.why, ty::label(), tw, 3);
    let top = cy - (th + if wh > 0.0 { sp::LINE_GAP + wh } else { 0.0 }) / 2.0;
    let t = text_wrap(&p, Pos2::new(tx, top), Align2::LEFT_TOP, &r.title, ty::title(), TEXT(), tw - vw, 2);
    if !r.value.is_empty() {
        p.text(Pos2::new(tx + tw, top), Align2::RIGHT_TOP, &r.value, ty::title(), tint::HEALTH());
    }
    if wh > 0.0 {
        text_wrap(&p, Pos2::new(tx, t.bottom() + sp::LINE_GAP), Align2::LEFT_TOP, &r.why, ty::label(), TEXT_DIM(), tw, 3);
    }
    ev
}

fn rec_icon(c: &mut Check, ctx: &egui::Context, p: &egui::Painter, r: Rect, rec: &Rec) {
    if let Some(path) = &rec.icon {
        if !c.icons.contains_key(path) && !c.demo {
            let tex = crate::kit::macos::app_icon(path, 64).map(|(n, rgba)| ctx.load_texture(format!("aicheck-icon:{}", path.display()), egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba), egui::TextureOptions::LINEAR));
            c.icons.insert(path.clone(), tex);
        }
        if let Some(Some(t)) = c.icons.get(path) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    let name = if rec.name.is_empty() { &rec.title } else { &rec.name };
    p.rect_filled(r, 7.0, letter_color(name));
    let l: String = name.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
    p.text(r.center(), Align2::CENTER_CENTER, l, bold(14.0), Color32::WHITE);
}

fn letter_color(name: &str) -> Color32 {
    const PALETTE: [Color32; 6] = [
        Color32::from_rgb(0x4C, 0x8B, 0xF5),
        Color32::from_rgb(0x2E, 0xA0, 0x6B),
        Color32::from_rgb(0xC0, 0x60, 0x3E),
        Color32::from_rgb(0x8A, 0x5C, 0xD6),
        Color32::from_rgb(0x2B, 0x8C, 0xA0),
        Color32::from_rgb(0xB0, 0x7A, 0x1E),
    ];
    let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    PALETTE[(h % PALETTE.len() as u32) as usize]
}
