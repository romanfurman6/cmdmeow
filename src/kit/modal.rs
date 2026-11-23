//! While something floats over a window (a menu, a popover, a dialog, a
//! sheet), what's under it stays still: no hover highlight, no tip, no
//! click. One empty layer over the whole window, just under the floating
//! ones, takes the pointer; egui's modal layer keeps every layer below it
//! out of hit testing and keyboard focus.

use egui::{Area, Context, Id, Order, Pos2, Rect, Sense, Ui};

fn shield_id() -> Id {
    Id::unique("cmdmeow-shield")
}

/// Cover the window for this pass: everything on the page layer goes still.
/// A click on the cover lands nowhere (an open menu still closes on it).
/// Safe to call more than once in a pass.
pub fn shield(ctx: &Context) {
    let pass = ctx.cumulative_pass_nr();
    let id = shield_id().with(ctx.viewport_id());
    if ctx.data(|d| d.get_temp::<u64>(id)) == Some(pass) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, pass));
    let screen = ctx.content_rect();
    let area = Area::new(id).order(Order::Middle).fixed_pos(screen.min).constrain(false).fade_in(false).movable(false);
    ctx.memory_mut(|m| m.set_modal_layer(area.layer()));
    area.show(ctx, |ui| {
        ui.set_clip_rect(screen);
        let _ = ui.allocate_rect(screen, Sense::click_and_drag());
    });
}

/// Call once at the end of each window's pass: an open egui menu (context
/// menu, drop-down) covers the rest of the window.
pub fn guard_popups(ctx: &Context) {
    if egui::Popup::is_any_open(ctx) {
        shield(ctx);
    }
}

/// Draw a dialog or sheet with `f` on its own layer over the page and cover
/// everything under it. `f` draws with absolute rects as on the page.
pub fn layer<R>(ui: &Ui, salt: &str, f: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    shield(&ctx);
    let screen = ctx.content_rect();
    Area::new(Id::unique(("cmdmeow-modal", salt)))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .fade_in(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.set_clip_rect(screen);
            // the layer spans the window, so the pointer is on it anywhere
            let _ = ui.allocate_rect(screen, Sense::hover());
            f(ui)
        })
        .inner
}

/// The pointer over `ui`'s layer, or None while something covers it: for
/// views that hit-test the pointer themselves instead of using responses.
pub fn pointer(ui: &Ui) -> Option<Pos2> {
    let ctx = ui.ctx();
    ctx.pointer_hover_pos().filter(|_| ctx.memory(|m| m.allows_interaction(ui.layer_id())))
}

/// Same as [`pointer`], for a rect: true when the pointer is in `r` and
/// nothing covers `ui`'s layer.
#[allow(dead_code)]
pub fn over(ui: &Ui, r: Rect) -> bool {
    pointer(ui).is_some_and(|q| r.contains(q))
}

/// A card's details over the page (Mac Health, AI Usage): a scrim over the
/// window and a popover, centred when it opens, `w` wide and `h` tall, at most the window
/// less a margin. `f` draws into the popover's rect with absolute rects.
/// Returns what `f` returned and true when Esc or a click outside asks to
/// close it.
pub fn details<R>(ctx: &Context, id: Id, w: f32, h: f32, f: impl FnOnce(&mut Ui, Rect) -> R) -> (R, bool) {
    use crate::kit::glass::{float_shadow, POPOVER, POPOVER_EDGE, SCRIM};
    shield(ctx);
    let full = ctx.content_rect();
    Area::new(id.with("area"))
        .order(Order::Foreground)
        .fixed_pos(full.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_min_size(full.size());
            let scrim = ui.interact(full, id.with("scrim"), Sense::click());
            ui.painter().rect_filled(full, 12.0, SCRIM());
            let size = egui::Vec2::new(w.min(full.width() - 32.0), h.min(full.height() - 48.0));
            // the top stays where it opened while the content changes height
            // (a process list growing must not move the row under the
            // pointer); it is centred again on the next open or a resize
            let key = id.with("top");
            let pass = ctx.cumulative_pass_nr();
            let kept = ctx.data(|d| d.get_temp::<(f32, u64, Rect)>(key)).filter(|(_, last, was)| *last + 1 >= pass && *was == full).map(|(t, _, _)| t);
            let top = kept.unwrap_or(full.center().y - size.y / 2.0).min(full.bottom() - 24.0 - size.y).max(full.top() + 24.0);
            ctx.data_mut(|d| d.insert_temp(key, (top, pass, full)));
            let m = Rect::from_min_size(egui::Pos2::new(full.center().x - size.x / 2.0, top), size);
            let card = ui.interact(m, id.with("card"), Sense::click());
            float_shadow(ui.painter(), m, 16.0, 1.0);
            ui.painter().rect_filled(m, 16.0, POPOVER());
            ui.painter().rect_stroke(m, 16.0, egui::Stroke::new(1.0, POPOVER_EDGE()), egui::StrokeKind::Inside);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(m).id_salt(id.with("body")));
            child.set_clip_rect(m);
            let r = f(&mut child, m);
            let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            (r, esc || (scrim.clicked() && !card.contains_pointer()))
        })
        .inner
}
