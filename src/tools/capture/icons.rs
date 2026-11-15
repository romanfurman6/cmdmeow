//! Capture's own icons: Apple's SF Symbols when available, otherwise simple
//! stroke drawings on a 24-unit grid (the same shapes as the design).

use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ico {
    Area,
    Window,
    Screen,
    ScanText,
    Timer,
    Select,
    Arrow,
    Rect,
    Ellipse,
    Pen,
    Frame,
    Text,
    Highlight,
    Blur,
    Eraser,
    Steps,
    Crop,
    Eyedropper,
    Ruler,
    Pin,
    Copy,
    Save,
    Edit,
    Undo,
    Redo,
    Trash,
    Folder,
    History,
    Close,
    Check,
    Keyboard,
    Record,
    Mic,
    MicOff,
    Camera,
    Speaker,
    Pause,
    Play,
    Restart,
    Gear,
    Scissors,
    Link,
}

fn symbol(i: Ico) -> &'static str {
    match i {
        Ico::Area => "rectangle.dashed",
        Ico::Window => "macwindow",
        Ico::Screen => "display",
        Ico::ScanText => "text.viewfinder",
        Ico::Timer => "timer",
        Ico::Select => "cursorarrow",
        Ico::Arrow => "arrow.up.right",
        Ico::Rect => "rectangle",
        Ico::Ellipse => "circle",
        Ico::Pen => "scribble",
        Ico::Frame => "photo.artframe",
        Ico::Text => "textformat",
        Ico::Highlight => "highlighter",
        Ico::Blur => "square.grid.3x3",
        Ico::Eraser => "eraser",
        Ico::Steps => "1.circle",
        Ico::Crop => "crop",
        Ico::Eyedropper => "eyedropper",
        Ico::Ruler => "ruler",
        Ico::Pin => "pin",
        Ico::Copy => "doc.on.doc",
        Ico::Save => "square.and.arrow.down",
        Ico::Edit => "pencil",
        Ico::Undo => "arrow.uturn.backward",
        Ico::Redo => "arrow.uturn.forward",
        Ico::Trash => "trash",
        Ico::Folder => "folder",
        Ico::History => "clock.arrow.circlepath",
        Ico::Close => "xmark",
        Ico::Check => "checkmark",
        Ico::Keyboard => "keyboard",
        Ico::Record => "record.circle",
        Ico::Mic => "mic",
        Ico::MicOff => "mic.slash",
        Ico::Camera => "video",
        Ico::Speaker => "speaker.wave.2",
        Ico::Pause => "pause",
        Ico::Play => "play.fill",
        Ico::Restart => "arrow.counterclockwise",
        Ico::Gear => "gearshape",
        Ico::Scissors => "scissors",
        Ico::Link => "link",
    }
}

pub fn draw(p: &Painter, i: Ico, rect: Rect, color: Color32) {
    if crate::kit::glass::symbol(p, symbol(i), rect, color) {
        return;
    }
    let c = rect.center();
    let u = rect.width().min(rect.height()) / 24.0;
    let pt = |x: f32, y: f32| c + Vec2::new((x - 12.0) * u, (y - 12.0) * u);
    let w = (1.8 * u).max(1.1);
    let line = |pts: &[(f32, f32)]| {
        p.add(PathShape::line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), PathStroke::new(w, color)));
    };
    let closed = |pts: &[(f32, f32)]| {
        p.add(PathShape::closed_line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), PathStroke::new(w, color)));
    };
    let boxr = |x0: f32, y0: f32, x1: f32, y1: f32| {
        p.rect_stroke(Rect::from_min_max(pt(x0, y0), pt(x1, y1)), 1.5 * u, Stroke::new(w, color), StrokeKind::Middle);
    };
    let circle = |x: f32, y: f32, r: f32| {
        crate::kit::glass::stroke_circle(&p, pt(x, y), r * u, Stroke::new(w, color));
    };
    match i {
        Ico::Area => {
            line(&[(4.0, 8.0), (4.0, 4.0), (8.0, 4.0)]);
            line(&[(16.0, 4.0), (20.0, 4.0), (20.0, 8.0)]);
            line(&[(20.0, 16.0), (20.0, 20.0), (16.0, 20.0)]);
            line(&[(8.0, 20.0), (4.0, 20.0), (4.0, 16.0)]);
        }
        Ico::Window => {
            boxr(3.0, 5.0, 21.0, 19.0);
            line(&[(3.0, 9.0), (21.0, 9.0)]);
        }
        Ico::Screen => {
            boxr(3.0, 4.0, 21.0, 16.0);
            line(&[(8.0, 20.0), (16.0, 20.0)]);
            line(&[(12.0, 16.0), (12.0, 20.0)]);
        }
        Ico::ScanText => {
            draw(p, Ico::Area, rect, color);
            line(&[(8.0, 10.0), (16.0, 10.0)]);
            line(&[(8.0, 14.0), (13.0, 14.0)]);
        }
        Ico::Timer => {
            circle(12.0, 13.0, 8.0);
            line(&[(12.0, 9.0), (12.0, 13.0), (15.0, 15.0)]);
            line(&[(9.0, 2.5), (15.0, 2.5)]);
        }
        Ico::Select => closed(&[(5.0, 3.0), (19.0, 10.0), (13.0, 12.0), (11.0, 18.0)]),
        Ico::Arrow => {
            line(&[(5.0, 19.0), (19.0, 5.0)]);
            line(&[(10.0, 5.0), (19.0, 5.0), (19.0, 14.0)]);
        }
        Ico::Rect => boxr(4.0, 6.0, 20.0, 18.0),
        Ico::Ellipse => circle(12.0, 12.0, 8.0),
        Ico::Pen => line(&[(3.0, 17.0), (6.0, 11.0), (9.0, 15.0), (13.0, 7.0), (16.0, 14.0), (21.0, 6.0)]),
        Ico::Frame => {
            boxr(3.0, 3.0, 21.0, 21.0);
            boxr(7.5, 7.5, 16.5, 16.5);
        }
        Ico::Text => {
            line(&[(5.0, 6.0), (5.0, 4.0), (19.0, 4.0), (19.0, 6.0)]);
            line(&[(12.0, 4.0), (12.0, 20.0)]);
            line(&[(9.0, 20.0), (15.0, 20.0)]);
        }
        Ico::Highlight => {
            line(&[(4.0, 20.0), (12.0, 20.0)]);
            closed(&[(14.0, 4.0), (20.0, 10.0), (12.0, 18.0), (8.0, 18.0), (8.0, 14.0)]);
        }
        Ico::Blur => {
            for (x, y) in [(4.0, 4.0), (14.0, 4.0), (4.0, 14.0), (14.0, 14.0)] {
                boxr(x, y, x + 6.0, y + 6.0);
            }
        }
        Ico::Eraser => {
            line(&[(7.0, 20.0), (20.0, 20.0)]);
            closed(&[(4.0, 15.0), (13.0, 6.0), (19.0, 12.0), (11.0, 20.0), (8.0, 20.0)]);
            line(&[(9.0, 10.0), (15.0, 16.0)]);
        }
        Ico::Steps => {
            circle(12.0, 12.0, 9.0);
            line(&[(10.5, 9.0), (12.5, 8.0), (12.5, 16.0)]);
        }
        Ico::Crop => {
            line(&[(6.0, 2.0), (6.0, 18.0), (22.0, 18.0)]);
            line(&[(2.0, 6.0), (18.0, 6.0), (18.0, 22.0)]);
        }
        Ico::Eyedropper => {
            line(&[(15.0, 4.0), (20.0, 9.0)]);
            closed(&[(13.0, 6.0), (18.0, 11.0), (9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
        }
        Ico::Ruler => {
            closed(&[(3.0, 17.0), (17.0, 3.0), (21.0, 7.0), (7.0, 21.0)]);
            for (x, y) in [(7.0, 13.0), (10.0, 10.0), (13.0, 7.0)] {
                line(&[(x, y), (x + 2.0, y + 2.0)]);
            }
        }
        Ico::Pin => {
            closed(&[(9.0, 3.0), (15.0, 3.0), (14.0, 10.0), (18.0, 13.0), (6.0, 13.0), (10.0, 10.0)]);
            line(&[(12.0, 13.0), (12.0, 21.0)]);
        }
        Ico::Copy => {
            boxr(8.0, 8.0, 20.0, 20.0);
            line(&[(4.0, 16.0), (4.0, 4.0), (16.0, 4.0)]);
        }
        Ico::Save => {
            line(&[(12.0, 4.0), (12.0, 15.0)]);
            line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            line(&[(4.0, 20.0), (20.0, 20.0)]);
        }
        Ico::Edit => closed(&[(4.0, 20.0), (8.0, 20.0), (19.0, 9.0), (15.0, 5.0), (4.0, 16.0)]),
        Ico::Undo => {
            line(&[(9.0, 14.0), (4.0, 9.0), (9.0, 4.0)]);
            p.add(PathShape::line(arc(pt(15.0, 14.0), 5.0 * u, -90.0, 90.0, pt(4.0, 9.0)), PathStroke::new(w, color)));
        }
        Ico::Redo => {
            line(&[(15.0, 14.0), (20.0, 9.0), (15.0, 4.0)]);
            p.add(PathShape::line(arc(pt(9.0, 14.0), 5.0 * u, 270.0, 90.0, pt(20.0, 9.0)), PathStroke::new(w, color)));
        }
        Ico::Trash => {
            line(&[(4.0, 6.0), (20.0, 6.0)]);
            line(&[(9.0, 6.0), (9.0, 3.5), (15.0, 3.5), (15.0, 6.0)]);
            line(&[(6.0, 6.0), (7.0, 20.5), (17.0, 20.5), (18.0, 6.0)]);
        }
        Ico::Folder => closed(&[(3.0, 6.0), (9.5, 6.0), (11.5, 8.0), (21.0, 8.0), (21.0, 19.0), (3.0, 19.0)]),
        Ico::History => {
            circle(12.0, 12.0, 8.5);
            line(&[(12.0, 8.0), (12.0, 12.0), (15.0, 14.0)]);
        }
        Ico::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Ico::Check => line(&[(5.0, 12.0), (10.0, 17.0), (19.0, 7.0)]),
        Ico::Keyboard => {
            boxr(3.0, 6.0, 21.0, 18.0);
            line(&[(7.0, 14.0), (17.0, 14.0)]);
            for x in [7.0, 11.0, 15.0] {
                p.circle_filled(pt(x, 10.0), u, color);
            }
        }
        Ico::Record => {
            circle(12.0, 12.0, 9.0);
            p.circle_filled(pt(12.0, 12.0), 4.5 * u, color);
        }
        Ico::Mic | Ico::MicOff => {
            boxr(9.0, 3.0, 15.0, 13.0);
            line(&[(5.0, 11.0), (6.0, 15.0), (12.0, 18.0), (18.0, 15.0), (19.0, 11.0)]);
            line(&[(12.0, 18.0), (12.0, 21.0)]);
            if i == Ico::MicOff {
                line(&[(3.0, 3.0), (21.0, 21.0)]);
            }
        }
        Ico::Camera => {
            boxr(3.0, 7.0, 15.0, 17.0);
            closed(&[(15.0, 10.0), (21.0, 7.0), (21.0, 17.0), (15.0, 14.0)]);
        }
        Ico::Speaker => {
            closed(&[(4.0, 9.0), (8.0, 9.0), (13.0, 5.0), (13.0, 19.0), (8.0, 15.0), (4.0, 15.0)]);
            line(&[(16.0, 9.0), (17.5, 12.0), (16.0, 15.0)]);
            line(&[(19.0, 6.0), (21.5, 12.0), (19.0, 18.0)]);
        }
        Ico::Pause => {
            line(&[(9.0, 5.0), (9.0, 19.0)]);
            line(&[(15.0, 5.0), (15.0, 19.0)]);
        }
        Ico::Play => {
            p.add(PathShape::convex_polygon(vec![pt(7.0, 4.0), pt(20.0, 12.0), pt(7.0, 20.0)], color, Stroke::NONE));
        }
        Ico::Restart => {
            line(&[(4.0, 4.0), (4.0, 9.0), (9.0, 9.0)]);
            p.add(PathShape::line(arc(pt(12.0, 12.0), 8.0 * u, 200.0, 160.0, pt(4.0, 9.0)), PathStroke::new(w, color)));
        }
        Ico::Scissors => {
            circle(7.0, 17.0, 3.0);
            circle(17.0, 17.0, 3.0);
            line(&[(9.0, 15.0), (18.0, 4.0)]);
            line(&[(15.0, 15.0), (6.0, 4.0)]);
        }
        Ico::Link => {
            line(&[(10.0, 14.0), (14.0, 10.0)]);
            line(&[(11.0, 7.0), (13.5, 4.5), (16.5, 4.5), (19.5, 7.5), (19.5, 10.5), (17.0, 13.0)]);
            line(&[(13.0, 17.0), (10.5, 19.5), (7.5, 19.5), (4.5, 16.5), (4.5, 13.5), (7.0, 11.0)]);
        }
        Ico::Gear => {
            circle(12.0, 12.0, 3.0);
            circle(12.0, 12.0, 7.5);
            for k in 0..8 {
                let a = (k as f32 * 45.0f32).to_radians();
                line(&[(12.0 + 7.5 * a.cos(), 12.0 + 7.5 * a.sin()), (12.0 + 10.0 * a.cos(), 12.0 + 10.0 * a.sin())]);
            }
        }
    }
}

/// A half-circle arc for undo/redo, joined to `start`.
fn arc(center: Pos2, r: f32, from_deg: f32, to_deg: f32, start: Pos2) -> Vec<Pos2> {
    let mut pts = vec![start, Pos2::new(center.x, center.y - r)];
    let n = 12;
    let span = if to_deg >= from_deg { to_deg - from_deg } else { to_deg + 360.0 - from_deg };
    for k in 0..=n {
        let a = (from_deg + span * k as f32 / n as f32).to_radians();
        pts.push(Pos2::new(center.x + r * a.cos(), center.y + r * a.sin()));
    }
    pts
}
