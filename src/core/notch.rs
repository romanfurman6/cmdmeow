//! The notch island (Settings › General "Where CmdMeow lives": Notch).
//! It takes the menu bar panel's place: the title, Quit and Open sit either
//! side of the notch, the widgets under it. Closed, a window the size of the notch sits over it and draws
//! nothing. The pointer resting on it for a moment opens a black island
//! under the notch with the panel's widgets, the same ones, sizes and cards
//! as the menu bar panel; only the background is black, so the island and
//! the notch read as one. The pointer leaving it, Esc, or a widget's action
//! closes it. On a Mac without a notch a thin strip at the top centre of the
//! main display does the same.
//!
//! Nothing here repaints while the island is closed: the window's tracking
//! area wakes the app when the pointer comes in.

use super::panel;
use super::shell::App;
use super::tool::Cx;
use crate::kit::glass::{self, Theme};
use crate::kit::mechanics as m;
use egui::{Color32, Pos2, Rect, Shape, Vec2, ViewportBuilder, ViewportId};
use std::cell::RefCell;
use std::time::{Duration, Instant};

pub const TITLE: &str = "CmdMeow Notch";
/// How long the pointer rests on the notch before the island opens.
const OPEN_AFTER: Duration = Duration::from_millis(150);
/// How long the pointer can be off the island before it closes.
const CLOSE_AFTER: Duration = Duration::from_millis(300);
/// Bottom corner radius of the open island.
const R_OPEN: f32 = 26.0;
/// The concave corners that join the island to the menu bar.
const EAR: f32 = 10.0;
/// Width of the hot strip on a display without a notch, and its height.
const STRIP: Vec2 = Vec2::new(200.0, 3.0);

/// Where the notch is, top-left origin, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    /// The notch, or the hot strip at the top centre when there is none.
    pub notch: Rect,
    /// The whole display it is on.
    pub screen: Rect,
    pub real: bool,
}

#[derive(Default)]
struct State {
    /// The pointer came onto the notch at.
    rest: Option<Instant>,
    /// The pointer left the open island at.
    left: Option<Instant>,
    open: bool,
    /// The window is set up (level, Spaces, tracking area).
    ready: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The island is open (its widgets are on screen: live numbers keep moving).
pub fn is_open() -> bool {
    STATE.with(|s| s.borrow().open)
}

/// The top band of the open island: as tall as the notch, and room for
/// the header's buttons where there is no notch.
pub fn band(notch_h: f32) -> f32 {
    notch_h.max(40.0)
}

/// The island's size when open: the header band, then the panel's grid.
pub fn open_size(notch_h: f32, content_h: f32, max_h: f32) -> Vec2 {
    let top = band(notch_h);
    let h = top + panel::GAP + content_h + panel::PAD;
    Vec2::new(m::PANEL_WIDTH + 2.0 * EAR, h.min(max_h).max(top + 40.0))
}

/// The window's rect: closed it covers only the notch (or the strip);
/// open, the island plus the ears, centred on the notch.
pub fn window_rect(g: &Geo, open: bool, content_h: f32) -> Rect {
    if !open {
        return g.notch;
    }
    let size = open_size(g.notch.height(), content_h, g.screen.height() * 0.8);
    let x = (g.notch.center().x - size.x / 2.0).clamp(g.screen.left(), g.screen.right() - size.x);
    Rect::from_min_size(Pos2::new(x, g.screen.top()), size)
}

/// Draw (or close) the island. Called every pass from the shell.
pub fn ui(app: &mut App, ctx: &egui::Context, cx: &mut Cx) {
    let want = super::place::get().notch() && app.setup.is_none() && super::onboard::is_done() && !app.demo;
    if !want {
        STATE.with(|s| *s.borrow_mut() = State::default());
        return;
    }
    let Some(g) = mac::geometry() else { return };
    let now = Instant::now();
    let content_h = panel::place(app, panel::inner_width()).1.max(40.0);
    let open = STATE.with(|s| s.borrow().open);
    let rect = window_rect(&g, open, content_h);
    // the pointer, from macOS: the window gets no moves while another app is in front
    let mouse = mac::mouse();
    let over_notch = mouse.is_some_and(|p| g.notch.expand2(Vec2::new(0.0, 1.0)).contains(p));
    let over_island = mouse.is_some_and(|p| rect.contains(p));
    let mut changed = false;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.open {
            if over_notch {
                let t = *s.rest.get_or_insert(now);
                if now - t >= OPEN_AFTER {
                    s.open = true;
                    s.left = None;
                    changed = true;
                }
            } else {
                s.rest = None;
            }
        } else if over_island {
            s.left = None;
        } else {
            let t = *s.left.get_or_insert(now);
            if now - t >= CLOSE_AFTER {
                s.open = false;
                s.rest = None;
                changed = true;
            }
        }
    });
    let open = STATE.with(|s| s.borrow().open);
    let rest = STATE.with(|s| s.borrow().rest.is_some());
    // follow the pointer only while something can change
    if open || rest {
        ctx.request_repaint_after(Duration::from_millis(60));
    }
    if changed {
        ctx.request_repaint();
    }
    // stays big while it shrinks back into the notch
    let opened = ctx.animate_bool_with_time(egui::Id::new("notch-open"), open, 0.28);
    let rect = window_rect(&g, open || opened > 0.01, content_h);
    let builder = ViewportBuilder::default()
        .with_title(TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false)
        .with_position(rect.min)
        .with_inner_size(rect.size());
    let id = ViewportId::from_hash_of(TITLE);
    let notch = g.notch;
    let real = g.real;
    let queued = cx.len();
    let mut close = false;
    crate::kit::viewport::show(ctx, id, builder, |ui, _| {
        if !STATE.with(|s| s.borrow().ready) {
            if mac::setup(id) {
                STATE.with(|s| s.borrow_mut().ready = true);
            }
        }
        let full = ui.max_rect();
        let p = ui.painter().clone();
        if !open && opened < 0.01 {
            // closed: the notch itself; without one, a faint strip
            if !real {
                p.rect_filled(full, full.height() / 2.0, Color32::from_black_alpha(120));
            }
            return;
        }
        // the island grows from the notch: the black shape eases out from
        // the notch's size to the whole window
        let nw = notch.width();
        let t = ease(opened);
        let w = egui::lerp(nw..=(full.width() - 2.0 * EAR).max(nw), t);
        let h = egui::lerp(notch.height()..=full.height(), t);
        let body = Rect::from_min_size(Pos2::new(full.center().x - w / 2.0, full.top()), Vec2::new(w, h));
        let r = egui::lerp(10.0..=R_OPEN, t);
        p.rect_filled(body, egui::CornerRadius { nw: 0, ne: 0, sw: r as u8, se: r as u8 }, Color32::BLACK);
        ears(&p, body, EAR * t);
        if t < 0.6 {
            return;
        }
        // the widgets: the panel's, drawn on the dark palette (the island is
        // black in every theme; Miami keeps its own)
        let was = glass::theme();
        if was == Theme::Light {
            glass::set_theme(Theme::Dark);
        }
        let top = band(notch.height());
        let grid = Rect::from_min_max(Pos2::new(body.left() + panel::PAD, body.top() + top + panel::GAP), Pos2::new(body.right() - panel::PAD, body.bottom() - panel::PAD));
        ui.set_opacity(((t - 0.6) / 0.4).clamp(0.0, 1.0));
        let (placed, need) = panel::place(app, grid.width());
        let mut draw = |ui: &mut egui::Ui, origin: Pos2| {
            for w in &placed {
                panel::draw_widget(app, ui, w, w.rect.translate(origin.to_vec2()), cx);
            }
        };
        if need > grid.height() + 0.5 {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid).layout(egui::Layout::top_down(egui::Align::Min)));
            child.set_clip_rect(grid.expand(4.0));
            egui::ScrollArea::vertical().id_salt("notch-widgets").auto_shrink([false, false]).show(&mut child, |ui| {
                let (origin, _) = ui.allocate_exact_size(Vec2::new(grid.width(), need), egui::Sense::hover());
                draw(ui, origin.min);
            });
        } else {
            draw(ui, grid.min);
        }
        // the panel's header either side of the notch: the title on the
        // left, Quit and Open on the right
        let nl = full.center().x - notch.width() / 2.0;
        let opened_app = app.app_open;
        let x0 = body.left() + 16.0;
        let title_right = if real { nl - 8.0 } else { body.center().x };
        panel::header_at(app, ui, x0, title_right, body.right() - 14.0, body.top() + top / 2.0);
        if app.app_open && !opened_app {
            close = true;
        }
        if was == Theme::Light {
            glass::set_theme(was);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    // a widget asked for something (a page, a capture): the island steps aside
    if close || cx.len() > queued {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.open {
                s.open = false;
                s.rest = None;
                s.left = None;
            }
        });
        ctx.request_repaint();
    }
}

fn ease(t: f32) -> f32 {
    // ease out, a touch of overshoot like a spring
    let t = t.clamp(0.0, 1.0);
    let c = 1.2;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

/// The two concave corners where the island meets the menu bar, `e` wide.
fn ears(p: &egui::Painter, body: Rect, e: f32) {
    if e < 0.5 {
        return;
    }
    for left in [true, false] {
        // the corner point on the menu bar, and the arc from the body's side down to it
        let (corner, side) = if left { (Pos2::new(body.left() - e, body.top()), body.left()) } else { (Pos2::new(body.right() + e, body.top()), body.right()) };
        let centre = Pos2::new(corner.x, body.top() + e);
        let mut pts = vec![Pos2::new(side, body.top())];
        let n = 8;
        for i in 0..=n {
            // from the side (pointing at the body) round to the top
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
            let (dx, dy) = (a.cos() * e, -a.sin() * e);
            pts.push(Pos2::new(if left { centre.x + dx } else { centre.x - dx }, centre.y + dy));
        }
        // a fan from the body's top corner fills the space outside the arc
        let mut mesh = egui::epaint::Mesh::default();
        mesh.colored_vertex(pts[0], Color32::BLACK);
        for q in &pts[1..] {
            mesh.colored_vertex(*q, Color32::BLACK);
        }
        for i in 1..pts.len() as u32 - 1 {
            mesh.add_triangle(0, i, i + 1);
        }
        p.add(Shape::mesh(mesh));
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Geo;
    use egui::{Pos2, Rect, Vec2};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow, NSWindowCollectionBehavior};
    use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSRect};

    fn top(mtm: MainThreadMarker) -> f64 {
        NSScreen::screens(mtm).firstObject().map(|s| s.frame().size.height).unwrap_or(0.0)
    }

    fn flip(top: f64, f: NSRect) -> Rect {
        Rect::from_min_size(Pos2::new(f.origin.x as f32, (top - f.origin.y - f.size.height) as f32), Vec2::new(f.size.width as f32, f.size.height as f32))
    }

    /// The built-in display's notch; without one, a strip at the top centre
    /// of the display with the menu bar.
    pub fn geometry() -> Option<Geo> {
        let mtm = MainThreadMarker::new()?;
        let t = top(mtm);
        let screens = NSScreen::screens(mtm);
        for s in screens.iter() {
            let has: bool = unsafe { msg_send![&*s, respondsToSelector: sel!(auxiliaryTopLeftArea)] };
            if !has {
                continue;
            }
            let ins: NSEdgeInsets = unsafe { msg_send![&*s, safeAreaInsets] };
            if ins.top <= 0.0 {
                continue;
            }
            let frame = s.frame();
            let l: NSRect = unsafe { msg_send![&*s, auxiliaryTopLeftArea] };
            let r: NSRect = unsafe { msg_send![&*s, auxiliaryTopRightArea] };
            let screen = flip(t, frame);
            let x0 = screen.left() + l.size.width as f32;
            let x1 = screen.right() - r.size.width as f32;
            if x1 - x0 < 40.0 {
                continue;
            }
            let notch = Rect::from_min_max(Pos2::new(x0, screen.top()), Pos2::new(x1, screen.top() + ins.top as f32));
            return Some(Geo { notch, screen, real: true });
        }
        let s = screens.firstObject()?;
        let screen = flip(t, s.frame());
        let notch = Rect::from_center_size(Pos2::new(screen.center().x, screen.top() + super::STRIP.y / 2.0), super::STRIP);
        Some(Geo { notch, screen, real: false })
    }

    /// The pointer, top-left origin.
    pub fn mouse() -> Option<Pos2> {
        let mtm = MainThreadMarker::new()?;
        let p = NSEvent::mouseLocation();
        Some(Pos2::new(p.x as f32, (top(mtm) - p.y) as f32))
    }

    /// Above the menu bar, on every Space and over full-screen apps, and a
    /// tracking area that wakes the app when the pointer comes in even while
    /// another app is in front. True once done.
    pub fn setup(id: egui::ViewportId) -> bool {
        let Some(_mtm) = MainThreadMarker::new() else { return false };
        let Some(view) = eframe::viewport_ns_view(id) else { return false };
        let Some(view) = (unsafe { (view as *const AnyObject).as_ref() }) else { return false };
        let window: Option<Retained<NSWindow>> = unsafe { msg_send![view, window] };
        let Some(window) = window else { return false };
        // NSMainMenuWindowLevel + 3: over the menu bar, as the notch is
        window.setLevel(27);
        window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle);
        window.setHasShadow(false);
        unsafe {
            // NSTrackingMouseEnteredAndExited | MouseMoved | ActiveAlways | InVisibleRect
            let opts: usize = 0x01 | 0x02 | 0x80 | 0x200;
            let bounds: NSRect = msg_send![view, bounds];
            let area: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSTrackingArea), alloc];
            let area: Option<Retained<AnyObject>> = msg_send![area, initWithRect: bounds, options: opts, owner: view, userInfo: std::ptr::null_mut::<AnyObject>()];
            if let Some(area) = area {
                let _: () = msg_send![view, addTrackingArea: &*area];
            }
        }
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::Geo;
    pub fn geometry() -> Option<Geo> {
        None
    }
    pub fn mouse() -> Option<egui::Pos2> {
        None
    }
    pub fn setup(_id: egui::ViewportId) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo() -> Geo {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1512.0, 982.0));
        Geo { notch: Rect::from_min_max(Pos2::new(662.0, 0.0), Pos2::new(850.0, 32.0)), screen, real: true }
    }

    #[test]
    fn closed_covers_only_the_notch() {
        let g = geo();
        assert_eq!(window_rect(&g, false, 300.0), g.notch);
    }

    #[test]
    fn open_is_the_panel_grid_centred_under_the_notch() {
        let g = geo();
        let r = window_rect(&g, true, 200.0);
        assert_eq!(r.top(), 0.0);
        assert!((r.center().x - g.notch.center().x).abs() < 0.01);
        assert_eq!(r.width(), m::PANEL_WIDTH + 2.0 * EAR);
        assert_eq!(r.height(), band(32.0) + panel::GAP + 200.0 + panel::PAD);
        // never taller than most of the display
        assert!(window_rect(&g, true, 5000.0).height() <= g.screen.height() * 0.8 + 0.01);
    }

    #[test]
    fn ease_ends_where_it_should() {
        assert!(ease(0.0).abs() < 1e-5);
        assert!((ease(1.0) - 1.0).abs() < 1e-5);
    }
}
