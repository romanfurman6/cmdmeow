//! Settings › AI Usage: which providers to read, notifications and display.

use crate::kit::agent::Agent;
use crate::kit::ai::{self, settings, UsageInfo};
use crate::kit::glass::*;
use egui::{Align2, Pos2, Rect, Ui, Vec2};

const ROW_H: f32 = sp::SETTINGS_ROW_H;
const THRESHOLDS: [u8; 3] = [50, 80, 95];
/// Where Claude's account sub-rows start (SP26).
const SUB_X: f32 = 44.0;

/// A card of `n` rows under a section title. Returns the card and moves `y`
/// past it.
fn section(p: &egui::Painter, x: f32, w: f32, y: &mut f32, title: &str, n: usize) -> Rect {
    crate::kit::glass::card_title(p, Pos2::new(x, *y), title);
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
        return ts!("ai.api_key").into();
    }
    let mut p = i.plan.trim().to_string();
    if let Some(f) = p.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    p
}

/// Draws the tab into `body`; returns the bottom of what it drew.
pub fn list(ui: &mut Ui, body: Rect, demo: bool) -> f32 {
    super::accounts::dialog(ui, demo);
    let p = ui.painter().clone();
    let set = settings::get();
    let w = 600.0f32.min(body.width() - 48.0);
    let x = body.center().x - w / 2.0;
    let mut y = body.top() + 24.0;
    let mut change: Option<Box<dyn FnOnce(&mut settings::Settings)>> = None;

    // SP26 providers: one switch per provider, Claude's accounts under it,
    // the ones not on this Mac last and dimmed
    let found: Vec<bool> = (0..ai::PROVIDERS.len()).map(|k| demo || ai::make(k).data_dir().is_some_and(|d| d.exists())).collect();
    // in the person's order (H10), the ones not on this Mac last
    let mut order: Vec<usize> = set.ordered();
    order.sort_by_key(|k| !found[*k]);
    let pick = if order.iter().filter(|k| set.is_on(ai::PROVIDERS[**k].id)).count() > 1 { super::backend::widget_provider() } else { None };
    // Claude's sub-rows: its accounts (the main one first) once there is
    // more than one, then "+ Add account"
    let claude_on = order.iter().any(|k| ai::PROVIDERS[*k].id == "claude" && found[*k]) && set.is_on("claude");
    let subs = if !claude_on {
        0
    } else if set.accounts.is_empty() {
        1
    } else {
        set.accounts.len() + 2
    };
    // found rows drag to reorder (a provider with its accounts); the
    // switches and buttons drawn after them win their clicks
    let mut drag_rows = Vec::new();
    crate::kit::glass::card_title(&p, Pos2::new(x, y), ts!("ai.set.providers"));
    y += 28.0;
    let card = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, (ai::PROVIDERS.len() + subs) as f32 * ROW_H));
    glass_card(&p, card, R_CARD, 0.0, false);
    let mut top = card.top();
    for (n, &k) in order.iter().enumerate() {
        let m = &ai::PROVIDERS[k];
        let r = Rect::from_min_size(Pos2::new(card.left(), top), Vec2::new(card.width(), ROW_H));
        let own_subs = if m.id == "claude" { subs } else { 0 };
        let block = Rect::from_min_size(r.min, Vec2::new(r.width(), ROW_H * (1 + own_subs) as f32));
        top = block.bottom();
        if n > 0 {
            hairline(&p, Pos2::new(r.left() + 16.0, r.top()), Pos2::new(r.right() - 16.0, r.top()));
        }
        let on = set.is_on(m.id);
        let main = set.main == m.id;
        let info = super::backend::snapshot(k).unwrap_or_default();
        if found[k] {
            let resp = ui.interact(block, ui.scope_id().with(("ai-row", m.id)), egui::Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                crate::kit::reorder::grip(&p, Pos2::new(r.left() + 8.0, r.center().y));
            }
            drag_rows.push(resp);
        }
        // the provider's mark where the status dot was; dimmed when not on this Mac
        let mark = Rect::from_center_size(Pos2::new(r.left() + 24.0, r.center().y), Vec2::splat(16.0));
        let mut mp = p.clone();
        if !found[k] {
            mp.multiply_opacity(0.45);
        }
        super::page::provider_mark(&mp, mark, m.name);
        let nr = p.text(Pos2::new(mark.right() + sp::S, r.center().y), Align2::LEFT_CENTER, m.name, font(13.0), if found[k] { TEXT() } else { TEXT_FAINT() });
        // Claude's live limits need its sign-in from the Keychain: nothing
        // reads it until the person connects it here or in setup
        if m.id == "claude" && !demo {
            super::keychain::refocus(ui.ctx());
        }
        if !found[k] {
            p.text(Pos2::new(r.right() - 16.0, r.center().y), Align2::RIGHT_CENTER, ts!("ai.not_installed"), ty::label(), TEXT_FAINT());
            continue;
        }
        let sr = switch_at(r);
        let mut after = nr.right() + sp::S;
        if m.id == "claude" && set.claude_keychain != "ok" && !demo && super::keychain::wanted(false) {
            let busy = super::keychain::busy();
            let bw = button_width(&p, ts!("ai.set.connect"), BTN_SECONDARY);
            let br = Rect::from_min_size(Pos2::new(sr.left() - sp::M - bw, r.center().y - 14.0), Vec2::new(bw, 28.0));
            if pill_button(ui, br, ts!("ai.set.connect"), BTN_SECONDARY, !busy).on_hover_text(ts!("ai.set.connect_tip")).clicked() {
                super::keychain::connect(Some(ui.ctx().clone()));
            }
            let err = super::keychain::error();
            if !err.is_empty() {
                p.text(Pos2::new(br.left() - sp::M, r.center().y), Align2::RIGHT_CENTER, err, ty::label(), RED());
            }
        } else if (on || main) && info.stale {
            // the last check failed: say so where the plan goes
            after = chip(&p, Pos2::new(after, r.center().y), Align2::LEFT_CENTER, ts!("ai.set.stale"), (with_alpha(AMBER(), 0.16), AMBER())).right() + sp::S;
        } else {
            let plan = plan_name(&info);
            if !plan.is_empty() {
                after = p.text(Pos2::new(after, r.center().y), Align2::LEFT_CENTER, plan, ty::label(), TEXT_DIM()).right() + sp::S;
            }
        }
        if pick == Some(k) {
            chip(&p, Pos2::new(after, r.center().y), Align2::LEFT_CENTER, ts!("ai.in_widgets"), (with_alpha(ACCENT(), 0.16), ACCENT()));
        }
        let sw = switch(ui, sr, ui.scope_id().with(("ai-on", m.id)), on || main, true);
        // the agent AI Check uses is always on
        let sw = if main { sw.on_hover_text(ts!("ai.set.main_tip")) } else { sw };
        if sw.clicked() && !main {
            let id = m.id.to_string();
            change = Some(Box::new(move |s| {
                s.off.retain(|x| *x != id);
                if on {
                    s.off.push(id);
                }
            }));
        }
        if own_subs == 0 {
            continue;
        }
        // Claude's accounts: name and folder, ✕ on hover (not the main one),
        // then "+ Add account"
        let sub = |j: usize| Rect::from_min_size(Pos2::new(r.left(), r.bottom() + j as f32 * ROW_H), Vec2::new(r.width(), ROW_H));
        let mut j = 0;
        if !set.accounts.is_empty() {
            let accounts = std::iter::once((set.main_account(), "~/.claude".to_string())).chain(set.accounts.iter().map(|a| (a.name.clone(), a.dir.clone())));
            for (idx, (name, dir)) in accounts.enumerate() {
                let ar = sub(j);
                j += 1;
                hairline(&p, Pos2::new(ar.left() + SUB_X, ar.top()), Pos2::new(ar.right() - 16.0, ar.top()));
                let nr = p.text(Pos2::new(ar.left() + SUB_X, ar.center().y), Align2::LEFT_CENTER, &name, font(13.0), TEXT());
                let xr = Rect::from_min_size(Pos2::new(ar.right() - 12.0 - 28.0, ar.center().y - 14.0), Vec2::splat(28.0));
                text_fit(&p, Pos2::new(nr.right() + sp::S, ar.center().y), Align2::LEFT_CENTER, &dir, ty::label(), TEXT_FAINT(), (xr.left() - nr.right() - 2.0 * sp::S).max(0.0));
                if idx > 0 && ui.rect_contains_pointer(ar) && icon_button(ui, xr, Icon::Close, ts!("ai.acc.remove")).clicked() {
                    change = Some(Box::new(move |s| s.accounts.retain(|x| x.dir != dir)));
                }
            }
        }
        let ar = sub(j);
        hairline(&p, Pos2::new(ar.left() + SUB_X, ar.top()), Pos2::new(ar.right() - 16.0, ar.top()));
        if link(ui, Pos2::new(ar.left() + SUB_X, ar.center().y), Align2::LEFT_CENTER, ts!("ai.acc.add_row"), 13.0).clicked() {
            super::accounts::open(ui.ctx());
        }
    }
    y = card.bottom() + 28.0;

    if let Some((from, gap)) = crate::kit::reorder::rows(ui, &drag_rows, 16.0) {
        let shown: Vec<usize> = order.iter().copied().filter(|k| found[*k]).collect();
        change = Some(Box::new(move |s| s.reorder(&shown, from, gap)));
    }

    // AI Check: the agent it asks, among the ones on this Mac that can check
    let agents: Vec<Agent> = Agent::ALL.into_iter().filter(|a| found[ai::index_of(a.id())]).collect();
    if !agents.is_empty() {
        let card = section(&p, x, w, &mut y, ts!("ai.set.ai_check"), 1);
        let r = row(card, 0);
        label(&p, r, ts!("ai.set.checks_with"));
        let cur = if demo { Agent::Claude } else { crate::kit::agent::main() };
        let bw = sp::M + 14.0 + sp::S + p.layout_no_wrap(cur.name().to_owned(), font(13.0), TEXT()).size().x + sp::S + 8.0 + sp::M;
        let br = Rect::from_min_size(Pos2::new(r.right() - 12.0 - bw, r.center().y - 14.0), Vec2::new(bw, 28.0));
        let (resp, face) = pill_button_at(ui, br, "", BTN_SECONDARY, true);
        let mr = Rect::from_center_size(Pos2::new(face.left() + sp::M + 7.0, face.center().y), Vec2::splat(14.0));
        crate::kit::aicheck::ui::agent_mark(&p, mr, cur, None);
        let t = p.text(Pos2::new(mr.right() + sp::S, face.center().y), Align2::LEFT_CENTER, cur.name(), font(13.0), TEXT());
        let c = Pos2::new(t.right() + sp::S + 4.0, face.center().y);
        let st = egui::Stroke::new(1.5, TEXT_DIM());
        p.line_segment([c + Vec2::new(-4.0, -2.0), c + Vec2::new(0.0, 2.0)], st);
        p.line_segment([c + Vec2::new(0.0, 2.0), c + Vec2::new(4.0, -2.0)], st);
        crate::kit::menu::popup(&resp).show(|ui| {
            for a in &agents {
                if crate::kit::menu::choice(ui, a.name(), *a == cur).clicked() && *a != cur {
                    let id = a.id().to_string();
                    change = Some(Box::new(move |s| {
                        s.off.retain(|x| *x != id);
                        s.main = id;
                    }));
                }
            }
        });
    }

    // notifications
    // alerts go out as notifications: while those aren't allowed the card
    // says so (R17), or the switches would look on and nothing would come
    let notif = crate::kit::perms::Perm::Notifications;
    let no_notif = !demo && notif.missing();
    let card = section(&p, x, w, &mut y, ts!("ai.set.notifications"), 3 + no_notif as usize);
    if no_notif {
        let r = row(card, 3);
        // the row's dot on the labels' column, its button on the switches' edge
        let at = Rect::from_min_max(Pos2::new(r.left() - 2.0, r.top()), Pos2::new(r.right() - 4.0, r.bottom()));
        perm_row(ui, &p, at, r.center().y, notif);
    }
    let r0 = row(card, 0);
    label(&p, r0, ts!("ai.set.limit_reaches"));
    let cw = 56.0;
    for (k, t) in THRESHOLDS.iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(r0.right() - 16.0 - (THRESHOLDS.len() - k) as f32 * (cw + 6.0) + 6.0, r0.center().y - 13.0), Vec2::new(cw, 26.0));
        let on = set.notify_at.contains(t);
        let resp = ui.interact(cr, ui.scope_id().with(("ai-th", *t)), egui::Sense::click());
        p.rect_filled(cr, R_SMALL, if on { with_alpha(ACTION(), 0.22) } else { white(15) });
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
        (ts!("ai.set.limit_reset"), set.notify_reset, (|s: &mut settings::Settings| s.notify_reset = !s.notify_reset) as fn(&mut settings::Settings)),
        (ts!("ai.set.weekly_unused"), set.notify_unused, |s: &mut settings::Settings| s.notify_unused = !s.notify_unused),
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
    let card = section(&p, x, w, &mut y, ts!("ai.set.display"), 1);
    let r = row(card, 0);
    label(&p, r, ts!("ai.set.cost"));
    if switch(ui, switch_at(r), ui.scope_id().with(("ai-d", 0)), set.show_cost, true).clicked() {
        change = Some(Box::new(|s: &mut settings::Settings| s.show_cost = !s.show_cost));
    }

    if let Some(f) = change {
        settings::update(!demo, f);
        crate::kit::claude_code::request_recheck();
    }
    y - 4.0
}
