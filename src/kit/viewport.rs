//! Native windows drawn inline in the current pass (egui "immediate"
//! viewports), opened safely while the menu panel is hidden.

use egui::{Context, Ui, ViewportBuilder, ViewportClass, ViewportId};
use std::sync::atomic::{AtomicBool, Ordering};

/// A window was drawn in this pass.
static SHOWN: AtomicBool = AtomicBool::new(false);

/// Call once at the end of each pass. eframe sleeps 10 ms after every pass
/// of a hidden main window; while our own windows (recorder, editor,
/// screenshot tools) are drawn in that pass, the sleep only costs them
/// frames, so it is off then.
pub fn end_pass() {
    eframe::skip_hidden_sleep(SHOWN.swap(false, Ordering::Relaxed));
}

/// Like `ctx.show_viewport_immediate`, but never crashes when the window
/// doesn't exist yet. Returns None on the pass that only asks for the window.
///
/// eframe can create an immediate viewport's window only while it runs inside
/// a winit event. With the panel hidden, the main window is painted from the
/// event loop's timer instead; eframe then has no event loop at hand, skips
/// our ui, and egui panics ("the user callback was never called"). That was
/// the crash right after a capture. So a new window is first registered as a
/// deferred viewport with an empty ui: eframe creates those at the start of
/// its next pass, with the event loop. Once the window is there, it is drawn
/// as an immediate viewport as before.
pub fn show<T>(ctx: &Context, id: ViewportId, builder: ViewportBuilder, mut ui: impl FnMut(&mut Ui, ViewportClass) -> T) -> Option<T> {
    SHOWN.store(true, Ordering::Relaxed);
    if ctx.embed_viewports() || has_window(ctx, id) {
        return Some(ctx.show_viewport_immediate(id, builder, |u, class| {
            let out = ui(u, class);
            // turning fan blades drawn in this window
            crate::kit::spin::flush(u.ctx());
            out
        }));
    }
    ctx.show_viewport_deferred(id, builder, |_, _| {});
    ctx.request_repaint();
    None
}

/// eframe fills a viewport's native scale only once it has a window.
fn has_window(ctx: &Context, id: ViewportId) -> bool {
    ctx.input(|i| i.raw.viewports.get(&id).is_some_and(|v| v.native_pixels_per_point.is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_counts_once_eframe_has_filled_its_scale() {
        let id = ViewportId::from_hash_of("t");
        let mut input = egui::RawInput::default();
        input.viewports.insert(id, egui::ViewportInfo::default());
        let ctx = Context::default();
        let mut seen = (true, true);
        ctx.run_ui(input.clone(), |ui| seen.0 = has_window(ui.ctx(), id)).textures_delta.clear();
        input.viewports.get_mut(&id).unwrap().native_pixels_per_point = Some(2.0);
        ctx.run_ui(input, |ui| seen.1 = has_window(ui.ctx(), id)).textures_delta.clear();
        assert_eq!(seen, (false, true));
    }
}
