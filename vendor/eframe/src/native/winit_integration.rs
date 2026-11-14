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
/// `CMDMEOW_PAINT_ALWAYS=1` turns this off.
#[derive(Default)]
pub struct LastPaint(Option<(Vec<egui::epaint::ClippedShape>, f32, [u32; 2], Instant)>);

impl LastPaint {
    /// Whether this pass draws what is already on screen (same shapes, scale
    /// and size, no texture changes). Remembers the pass either way.
    pub fn same(&mut self, shapes: &[egui::epaint::ClippedShape], pixels_per_point: f32, size: [u32; 2], textures_idle: bool) -> bool {
        if paint_always() {
            return false;
        }
        let same = textures_idle
            && self.0.as_ref().is_some_and(|(s, p, z, at)| at.elapsed().as_secs_f32() < 1.0 && *p == pixels_per_point && *z == size && s.as_slice() == shapes);
        if !same {
            self.0 = Some((shapes.to_vec(), pixels_per_point, size, Instant::now()));
        }
        PAINTS[usize::from(same)].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        same
    }

    /// The window was hidden (or not drawn): paint the next pass whatever it shows.
    pub fn forget(&mut self) {
        self.0 = None;
    }
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
