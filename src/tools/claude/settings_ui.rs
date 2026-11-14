//! Settings › AI Usage: which providers to read, notifications, and what
//! the menu bar shows.

use crate::kit::ai::{self, settings, UsageInfo};
use crate::kit::glass::*;
use egui::{Align2, Pos2, Rect, Ui, Vec2};

const ROW_H: f32 = sp::SETTINGS_ROW_H;
const THRESHOLDS: [u8; 3] = [50, 80, 95];
const MENU: [(&str, &str); 4] = [("closest", "Closest to limit"), ("claude", "Claude"), ("codex", "Codex"), ("off", "Off")];

/// A card of `n` rows under a section title. Returns the card and moves `y`
/// past it.
fn section(p: &egui::Painter, x: f32, w: f32, y: &mut f32, title: &str, n: usize) -> Rect {
    p.text(Pos2::new(x, *y), Align2::LEFT_TOP, title, bold(15.0), TEXT());
    *y += 28.0;
    let card = Rect::from_min_size(Pos2::new(x, *y), Vec2::new(w, n as f32 * ROW_H));
    glass_card(p, card, R_CARD, 0.0, false);
    for k in 1..n {
        let t = card.top() + k as f32 * ROW_H;
        hairline(p, Pos2::new(card.left() + 16.0, t), Pos2::new(card.right() - 16.0, t));
    }
    *y = card.bottom() + 28.0;
    card
}

fn row(card: Rect, k: usize) -> Rect {
    Rect::from_min_size(Pos2::new(card.left(), card.top() + k as f32 * ROW_H), Vec2::new(card.width(), ROW_H))
}

fn switch_at(r: Rect) -> Rect {
    Rect::from_min_size(Pos2::new(r.right() - 42.0, r.center().y - 9.0), Vec2::new(30.0, 18.0))
}

fn label(p: &egui::Painter, r: Rect, s: &str) {
    p.text(Pos2::new(r.left() + 16.0, r.center().y), Align2::LEFT_CENTER, s, font(13.0), TEXT());
}

/// The plan as people call it: "Max", "Plus", "API key", or nothing.
fn plan_name(i: &UsageInfo) -> String {
    if i.is_api() {
        return "API key".into();
    }
    let mut p = i.plan.trim().to_string();
    if let Some(f) = p.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    p
}

/// Draws the tab into `body`; returns the bottom of what it drew.
pub fn list(ui: &mut Ui, body: Rect, demo: bool) -> f32 {
    let p = ui.painter().clone();
    let set = settings::get();
    let w = 600.0f32.min(body.width() - 48.0);
    let x = body.center().x - w / 2.0;
    let mut y = body.top() + 24.0;
    let mut change: Option<Box<dyn FnOnce(&mut settings::Settings)>> = None;

    // providers: the ones on this Mac first, the rest dim below them
    let found: Vec<bool> = (0..ai::PROVIDERS.len()).map(|k| demo || ai::make(k).data_dir().is_some_and(|d| d.exists())).collect();
    let mut order: Vec<usize> = (0..ai::PROVIDERS.len()).collect();
    order.sort_by_key(|k| !found[*k]);
    let card = section(&p, x, w, &mut y, "Providers", ai::PROVIDERS.len());
    for (n, &k) in order.iter().enumerate() {
        let m = &ai::PROVIDERS[k];
        let r = row(card, n);
        let on = set.is_on(m.id);
        let main = set.main == m.id;
        let info = super::backend::snapshot(k).unwrap_or_default();
        // green: up to date, amber: the last check failed, dim: not on this Mac or off
        let dot = if !found[k] || !(on || main) {
            white(51)
        } else if info.stale {
            AMBER()
        } else {
            GREEN()
        };
        p.circle_filled(Pos2::new(r.left() + 20.0, r.center().y), 4.0, dot);
        p.text(Pos2::new(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, m.name, font(13.0), if found[k] { TEXT() } else { TEXT_FAINT() });
        let rr = Rect::from_center_size(Pos2::new(r.right() - 58.0 - 8.0, r.center().y), Vec2::splat(16.0));
        let resp = radio(ui, rr, ui.scope_id().with(("ai-main", m.id)), main, true).on_hover_text("Main agent");
        if resp.clicked() && !main {
            let id = m.id.to_string();
            change = Some(Box::new(move |s| {
                s.off.retain(|x| *x != id);
                s.main = id;
            }));
        }
        // Claude's live limits need its sign-in from the Keychain: nothing
        // reads it until the person connects it here or in setup
        if m.id == "claude" && !demo {
            super::keychain::refocus(ui.ctx());
        }
        if m.id == "claude" && found[k] && set.claude_keychain != "ok" && !demo && super::keychain::wanted(false) {
            let busy = super::keychain::busy();
            let bw = button_width(&p, "Connect", BTN_SECONDARY);
            let br = Rect::from_min_size(Pos2::new(rr.left() - sp::M - bw, r.center().y - 14.0), Vec2::new(bw, 28.0));
            if pill_button(ui, br, "Connect", BTN_SECONDARY, !busy).on_hover_text("Claude limits from your account").clicked() {
                super::keychain::connect(Some(ui.ctx().clone()));
            }
            let err = super::keychain::error();
            if !err.is_empty() {
                p.text(Pos2::new(br.left() - sp::M, r.center().y), Align2::RIGHT_CENTER, err, ty::label(), RED());
            }
        } else if found[k] {
            p.text(Pos2::new(rr.left() - sp::M, r.center().y), Align2::RIGHT_CENTER, plan_name(&info), ty::label(), TEXT_DIM());
        }
        let sw = switch(ui, switch_at(r), ui.scope_id().with(("ai-on", m.id)), on || main, true);
        // the main agent (Cleanup checks with it) is always on
        let sw = if main { sw.on_hover_text("The main agent is always tracked") } else { sw };
        if sw.clicked() && !main {
            let id = m.id.to_string();
            change = Some(Box::new(move |s| {
                s.off.retain(|x| *x != id);
                if on {
                    s.off.push(id);
                }
            }));
        }
    }

    // notifications
    let card = section(&p, x, w, &mut y, "Notifications", 3);
    let r0 = row(card, 0);
    label(&p, r0, "Limit reaches");
    let cw = 56.0;
    for (k, t) in THRESHOLDS.iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(r0.right() - 16.0 - (THRESHOLDS.len() - k) as f32 * (cw + 6.0) + 6.0, r0.center().y - 13.0), Vec2::new(cw, 26.0));
        let on = set.notify_at.contains(t);
        let resp = ui.interact(cr, ui.scope_id().with(("ai-th", *t)), egui::Sense::click());
        p.rect_filled(cr, R_SMALL, if on { with_alpha(BLUE(), 0.22) } else { white(15) });
        p.text(cr.center(), Align2::CENTER_CENTER, format!("{t}%"), font(12.0), if on { ACCENT() } else { TEXT_DIM() });
        if resp.clicked() {
            let t = *t;
            change = Some(Box::new(move |s| {
                if on {
                    s.notify_at.retain(|x| *x != t);
                } else {
                    s.notify_at.push(t);
                    s.notify_at.sort_unstable();
                }
            }));
        }
    }
    for (k, (name, on, f)) in [
        ("Limit reset", set.notify_reset, (|s: &mut settings::Settings| s.notify_reset = !s.notify_reset) as fn(&mut settings::Settings)),
        ("Weekly unused, day before reset", set.notify_unused, |s: &mut settings::Settings| s.notify_unused = !s.notify_unused),
    ]
    .into_iter()
    .enumerate()
    {
        let r = row(card, k + 1);
        label(&p, r, name);
        if switch(ui, switch_at(r), ui.scope_id().with(("ai-n", k)), on, true).clicked() {
            change = Some(Box::new(f));
        }
    }

    // display
    let card = section(&p, x, w, &mut y, "Display", 3);
    let r0 = row(card, 0);
    label(&p, r0, "Menu bar");
    let labels: Vec<&str> = MENU.iter().map(|m| m.1).collect();
    let seg = Rect::from_min_size(Pos2::new(r0.right() - 16.0 - 380.0, r0.center().y - 13.0), Vec2::new(380.0, 26.0));
    let cur = MENU.iter().position(|m| m.0 == set.menu_bar).unwrap_or(0);
    if let Some(i) = segmented(ui, seg, ui.scope_id().with("ai-menu"), &labels, cur) {
        change = Some(Box::new(move |s| s.menu_bar = MENU[i].0.into()));
    }
    for (k, (name, on, f)) in [
        ("Cost in $", set.show_cost, (|s: &mut settings::Settings| s.show_cost = !s.show_cost) as fn(&mut settings::Settings)),
        ("Provider outages", set.show_outages, |s: &mut settings::Settings| s.show_outages = !s.show_outages),
    ]
    .into_iter()
    .enumerate()
    {
        let r = row(card, k + 1);
        label(&p, r, name);
        if switch(ui, switch_at(r), ui.scope_id().with(("ai-d", k)), on, true).clicked() {
            change = Some(Box::new(f));
        }
    }

    if let Some(f) = change {
        settings::update(!demo, f);
        crate::kit::claude_code::request_recheck();
    }
    y - 4.0
}
