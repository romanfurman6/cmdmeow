use std::{sync::Arc, time::Instant};

use winit::{
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

use egui::ViewportId;
#[cfg(feature = "accesskit")]
use egui_winit::accesskit_winit;

/// Returns `true` if the window is invisible or minimized.
///
/// These windows don't receive `RedrawRequested` events on Windows,
/// so they need special handling to keep processing viewport commands.
pub fn is_invisible_or_minimized(window: &Window) -> bool {
    window.is_visible() == Some(false) || window.is_minimized() == Some(true)
}

/// Cmd+Meow patch: each viewport's winit window, so the app can reach its
/// native view (Core Animation layers over the egui surface).
pub static WINDOWS: std::sync::Mutex<Vec<(egui::ViewportId, std::sync::Weak<winit::window::Window>)>> = std::sync::Mutex::new(Vec::new());

pub fn remember_window(id: egui::ViewportId, window: &std::sync::Arc<winit::window::Window>) {
    let mut w = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    w.retain(|(v, win)| *v != id && win.strong_count() > 0);
    w.push((id, std::sync::Arc::downgrade(window)));
}

/// On Mac, a minimized window uses up all CPU:
/// <https://github.com/emilk/egui/issues/325>
///
/// On Windows, an invisible window also uses up all CPU:
/// <https://github.com/emilk/egui/issues/7776>
///
/// Cmd+Meow patch: the app draws overlays (recording countdown, frame,
/// control bar) as immediate viewports while its root window is hidden.
/// They present with vsync themselves, so this sleep only cost them frames.
/// The app turns it off while such overlays are up.
pub static SKIP_HIDDEN_SLEEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn sleep_if_invisible_or_minimized(window: Option<&Window>) {
    if !SKIP_HIDDEN_SLEEP.load(std::sync::atomic::Ordering::Relaxed) && window.is_some_and(is_invisible_or_minimized) {
        profiling::scope!("minimized_sleep");
        std::thread::sleep(core::time::Duration::from_millis(10));
    }
}

/// Cmd+Meow patch: what a viewport last put on screen. egui runs a pass for
/// every mouse move and every timer, and most of those passes come out
/// exactly like the frame already on screen (the pointer moved over nothing
/// that reacts). Then tessellation and the GPU frame are skipped: the window
/// keeps showing the same pixels. Still painted at least once a second, so
/// a window can't stay stale for long whatever macOS did with its layer.
/// A skipped pass still takes a display frame (see [`LastPaint::pace`]).
/// `CMDMEOW_PAINT_ALWAYS=1` turns this off.
#[derive(Default)]
pub struct LastPaint {
    shown: Option<(Vec<egui::epaint::ClippedShape>, f32, [u32; 2], Instant)>,
    /// When the last pass (painted or not) of this viewport ended.
    last_pass: Option<Instant>,
    /// The display's refresh rate and when it was looked up.
    refresh: Option<(u32, Instant)>,
    /// Since when the window has been on screen (None: hidden, or not told).
    on_screen_since: Option<Instant>,
}

/// How long a window that just came on screen is painted every pass: frames
/// painted while it was hidden, or while macOS was ordering it in, may never
/// reach the screen, so a skip right then left it blank for up to a second.
const SETTLE: core::time::Duration = core::time::Duration::from_millis(500);

impl LastPaint {
    /// Whether this pass draws what is already on screen (same shapes, scale
    /// and size, no texture changes). Remembers the pass either way.
    pub fn same(&mut self, shapes: &[egui::epaint::ClippedShape], pixels_per_point: f32, size: [u32; 2], textures_idle: bool) -> bool {
        if paint_always() || painting_everything() {
            return false;
        }
        // a window that went without passes for a while may have been hidden
        // or closed and shown again, and macOS may have dropped its pixels
        let recent = self.last_pass.is_some_and(|t| t.elapsed() < core::time::Duration::from_millis(250));
        let settled = self.on_screen_since.is_none_or(|t| t.elapsed() >= SETTLE);
        let same = textures_idle
            && recent
            && settled
            && self.shown.as_ref().is_some_and(|(s, p, z, at)| at.elapsed().as_secs_f32() < 1.0 && *p == pixels_per_point && *z == size && s.as_slice() == shapes);
        if !same {
            self.shown = Some((shapes.to_vec(), pixels_per_point, size, Instant::now()));
        }
        PAINTS[usize::from(same)].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        same
    }

    /// The window was hidden (or not drawn): paint the next pass whatever it shows.
    pub fn forget(&mut self) {
        self.shown = None;
    }

    /// Whether the window is on screen now (visible, not minimized). Once
    /// it comes on screen, every pass is painted for [`SETTLE`].
    pub fn on_screen(&mut self, on: bool) {
        if !on {
            self.forget();
            self.on_screen_since = None;
        } else if self.on_screen_since.is_none() {
            self.on_screen_since = Some(Instant::now());
        }
    }

    /// Call after each pass (after [`Self::same`]). A painted pass waits for the display (vsync);
    /// a skipped one would not, and egui asks for the next pass right away
    /// while the pointer moves, so passes would run back to back (thousands
    /// a second). A skipped pass waits out the rest of a display frame
    /// (`refresh_hz`, looked up every few seconds, 60 when unknown)
    /// instead, as painting would have.
    pub fn pace(&mut self, skipped: bool, refresh_hz: impl FnOnce() -> Option<u32>) {
        if skipped && let Some(last) = self.last_pass {
            let hz = match self.refresh {
                Some((hz, at)) if at.elapsed().as_secs() < 3 => hz,
                _ => {
                    let hz = refresh_hz().filter(|h| (24..=240).contains(h)).unwrap_or(60);
                    self.refresh = Some((hz, Instant::now()));
                    hz
                }
            };
            let frame = core::time::Duration::from_secs_f64(1.0 / f64::from(hz));
            let gone = last.elapsed();
            if gone < frame {
                std::thread::sleep(frame - gone);
            }
        }
        self.last_pass = Some(Instant::now());
    }
}

/// Until when every pass of every window is painted (see [`paint_everything_for`]).
static PAINT_ALL_UNTIL: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Paint every pass of every window for a while, whatever it draws: after a
/// change that may leave a frame on screen drawn half with the old state (a
/// theme switch rebuilds the fonts mid-pass), so the next passes replace it.
pub fn paint_everything_for(d: core::time::Duration) {
    if let Ok(mut u) = PAINT_ALL_UNTIL.lock() {
        *u = Some(Instant::now() + d);
    }
}

fn painting_everything() -> bool {
    PAINT_ALL_UNTIL.lock().ok().and_then(|u| *u).is_some_and(|t| Instant::now() < t)
}

fn paint_always() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CMDMEOW_PAINT_ALWAYS").is_some_and(|v| v != "0"))
}

/// Passes painted (0) and skipped as unchanged (1), for the app's repaint log.
pub static PAINTS: [std::sync::atomic::AtomicU32; 2] = [std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0)];

/// What [`WinitApp::run_ui_and_paint`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassMode {
    /// Run [`crate::App::logic`], plus the ui and painting if the viewport is visible.
    Full,

    /// Run only [`crate::App::logic`]: no egui pass and no painting.
    ///
    /// Used when the window was asked to redraw, but the windowing system hasn't let it
    /// (e.g. a Wayland compositor withholding frame callbacks from a hidden window).
    LogicOnly,
}

/// Create an egui context, restoring it from storage if possible.
pub fn create_egui_context(storage: Option<&dyn crate::Storage>) -> egui::Context {
    profiling::function_scope!();

    pub const IS_DESKTOP: bool = cfg!(any(
        target_os = "freebsd",
        target_os = "linux",
        target_os = "macos",
        target_os = "openbsd",
        target_os = "windows",
    ));

    let egui_ctx = egui::Context::default();

    egui_ctx.set_embed_viewports(!IS_DESKTOP);

    egui_ctx.options_mut(|o| {
        // eframe supports multi-pass (Context::request_discard).
        #[expect(clippy::unwrap_used)]
        {
            o.max_passes = 2.try_into().unwrap();
        }
    });

    let memory = crate::native::epi_integration::load_egui_memory(storage).unwrap_or_default();
    egui_ctx.memory_mut(|mem| *mem = memory);

    egui_ctx
}

/// The custom even `eframe` uses with the [`winit`] event loop.
#[derive(Debug)]
pub enum UserEvent {
    /// A repaint is requested.
    RequestRepaint {
        /// What to repaint.
        viewport_id: ViewportId,

        /// When to repaint.
        when: Instant,

        /// What the cumulative pass number was when the repaint was _requested_.
        cumulative_pass_nr: u64,
    },

    /// A request related to [`accesskit`](https://accesskit.dev/).
    #[cfg(feature = "accesskit")]
    AccessKitActionRequest(accesskit_winit::Event),
}

#[cfg(feature = "accesskit")]
impl From<accesskit_winit::Event> for UserEvent {
    fn from(inner: accesskit_winit::Event) -> Self {
        Self::AccessKitActionRequest(inner)
    }
}

pub trait WinitApp {
    fn egui_ctx(&self) -> Option<&egui::Context>;

    fn window(&self, window_id: WindowId) -> Option<Arc<Window>>;

    fn window_id_from_viewport_id(&self, id: ViewportId) -> Option<WindowId>;

    fn save(&mut self);

    fn save_and_destroy(&mut self);

    fn run_ui_and_paint(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        mode: PassMode,
    ) -> crate::Result<EventResult>;

    fn suspended(&mut self, event_loop: &ActiveEventLoop) -> crate::Result<EventResult>;

    fn resumed(&mut self, event_loop: &ActiveEventLoop) -> crate::Result<EventResult>;

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) -> crate::Result<EventResult>;

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) -> crate::Result<EventResult>;

    #[cfg(feature = "accesskit")]
    fn on_accesskit_event(&mut self, event: accesskit_winit::Event) -> crate::Result<EventResult>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    Wait,

    /// Causes a synchronous repaint inside the event handler. This should only
    /// be used in special situations if the window must be repainted while
    /// handling a specific event. This occurs on Windows when handling resizes.
    ///
    /// `RepaintNow` creates a new frame synchronously, and should therefore
    /// only be used for extremely urgent repaints.
    RepaintNow(WindowId),

    /// Queues a repaint for once the event loop handles its next redraw. Exists
    /// so that multiple input events can be handled in one frame. Does not
    /// cause any delay like `RepaintNow`.
    RepaintNext(WindowId),

    RepaintAt(WindowId, Instant),

    /// Causes a save of the client state when the persistence feature is enabled.
    Save,

    /// Starts the process of ending eframe execution whilst allowing for proper
    /// clean up of resources.
    ///
    /// # Warning
    /// This event **must** occur before [`Exit`] to correctly exit eframe code.
    /// If in doubt, return this event.
    ///
    /// [`Exit`]: [EventResult::Exit]
    CloseRequested,

    /// The event loop will exit, now.
    /// The correct circumstance to return this event is in response to a winit "Destroyed" event.
    ///
    /// # Warning
    /// The [`CloseRequested`] **must** occur before this event to ensure that winit
    /// is able to remove any open windows. Otherwise the window(s) will remain open
    /// until the program terminates.
    ///
    /// [`CloseRequested`]: EventResult::CloseRequested
    Exit,
}

#[cfg(feature = "accesskit")]
pub(crate) fn on_accesskit_window_event(
    egui_winit: &mut egui_winit::State,
    window_id: WindowId,
    event: &accesskit_winit::WindowEvent,
) -> EventResult {
    match event {
        accesskit_winit::WindowEvent::InitialTreeRequested => {
            egui_winit.egui_ctx().enable_accesskit();
            // Because we can't provide the initial tree synchronously
            // (because that would require the activation handler to access
            // the same mutable state as the winit event handler), some
            // AccessKit platform adapters will use a placeholder tree
            // until we send the first tree update. To minimize the possible
            // bad effects of that workaround, repaint and send the tree
            // immediately.
            EventResult::RepaintNow(window_id)
        }
        accesskit_winit::WindowEvent::ActionRequested(request) => {
            egui_winit.on_accesskit_action_request(request.clone());
            // As a form of user input, accessibility actions should cause
            // a repaint, but not until the next regular frame.
            EventResult::RepaintNext(window_id)
        }
        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
            egui_winit.egui_ctx().disable_accesskit();
            // Disabling AccessKit support should have no visible effect,
            // so there's no need to repaint.
            EventResult::Wait
        }
    }
}

#[cfg(test)]
mod last_paint_tests {
    use super::{LastPaint, SETTLE};
    use egui::epaint::{ClippedShape, Shape};
    use egui::{Color32, Pos2, Rect};

    fn shapes(x: f32) -> Vec<ClippedShape> {
        let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        vec![ClippedShape::new(clip, Shape::rect_filled(Rect::from_min_max(Pos2::new(x, 0.0), Pos2::new(50.0, 50.0)), 2.0, Color32::RED))]
    }

    #[test]
    fn a_pass_that_draws_the_same_is_skipped_anything_else_is_painted() {
        let mut lp = LastPaint::default();
        assert!(!lp.same(&shapes(0.0), 2.0, [200, 200], true), "first pass paints");
        lp.pace(false, || None);
        assert!(lp.same(&shapes(0.0), 2.0, [200, 200], true), "same frame is skipped");
        assert!(!lp.same(&shapes(1.0), 2.0, [200, 200], true), "a changed shape paints");
        assert!(!lp.same(&shapes(1.0), 1.0, [200, 200], true), "a new scale paints");
        assert!(!lp.same(&shapes(1.0), 1.0, [201, 200], true), "a new size paints");
        assert!(!lp.same(&shapes(1.0), 1.0, [201, 200], false), "texture changes paint");
        assert!(lp.same(&shapes(1.0), 1.0, [201, 200], true));
        lp.forget();
        assert!(!lp.same(&shapes(1.0), 1.0, [201, 200], true), "after forget it paints");
    }

    #[test]
    fn a_frame_shown_over_a_second_ago_is_painted_again() {
        let mut lp = LastPaint::default();
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true));
        lp.pace(false, || None);
        if let Some(shown) = lp.shown.as_mut() {
            shown.3 -= std::time::Duration::from_millis(1100);
        }
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true));
    }

    #[test]
    fn a_window_that_had_no_passes_for_a_while_is_painted_again() {
        let mut lp = LastPaint::default();
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true));
        lp.pace(false, || None);
        // closed and reopened: no passes in between
        lp.last_pass = lp.last_pass.map(|t| t - std::time::Duration::from_millis(400));
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true));
    }

    #[test]
    fn a_window_that_just_came_on_screen_is_painted_every_pass_for_a_moment() {
        let mut lp = LastPaint::default();
        // painted while hidden (the first frame of a reopened window)
        lp.on_screen(false);
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true));
        lp.on_screen(true);
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true), "just shown: painted");
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true), "still settling: painted");
        lp.on_screen_since = lp.on_screen_since.map(|t| t - SETTLE);
        lp.on_screen(true);
        lp.pace(false, || None);
        assert!(lp.same(&shapes(0.0), 2.0, [10, 10], true), "settled: skipped again");
        lp.on_screen(false);
        lp.on_screen(true);
        assert!(!lp.same(&shapes(0.0), 2.0, [10, 10], true), "hidden and shown again: painted");
    }

    #[test]
    fn only_skipped_passes_wait_for_the_display() {
        use std::time::{Duration, Instant};
        let mut lp = LastPaint::default();
        lp.pace(false, || Some(30));
        let t = Instant::now();
        lp.pace(false, || Some(30));
        assert!(t.elapsed() < Duration::from_millis(10), "a painted pass doesn't sleep");
        let t = Instant::now();
        lp.pace(true, || Some(30));
        assert!(t.elapsed() >= Duration::from_millis(25), "a skipped pass waits out a 30 Hz frame");
        let t = Instant::now();
        lp.pace(true, || None);
        // the 30 Hz looked up a moment ago is kept for a few seconds
        assert!(t.elapsed() >= Duration::from_millis(25));
    }
}
