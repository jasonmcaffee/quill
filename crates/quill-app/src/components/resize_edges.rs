//! The eight places the window itself is resized from.
//!
//! Quill draws its own title bar, which means the window is created with `with_decorations(false)`, and
//! an undecorated window has no frame for the operating system to offer a resize grip on. `task-1658`
//! is what that cost: the window could be resized from the top, where the title bar's own drag happened
//! to land on something the platform still handled, and from nowhere else.
//!
//! So the eight grips are drawn here — four edges and four corners — as invisible strips inside the
//! window's own rectangle. Each one sets the pointer to the arrow for the direction it moves and, when it
//! is dragged, sends `ViewportCommand::BeginResize`, which hands the drag to the window manager. Nothing
//! is painted: the window already has its rounded rectangle, and a visible frame is exactly what turning
//! the decorations off was for.
//!
//! **They are added to the `Ui` last**, after every pane, for the reason `components::splitter` records
//! about dividers: a widget added earlier sits underneath one added later, and the editing area, the
//! explorer and the status bar all take drags over the whole of their rectangles. Added first, the grips
//! never saw a pointer.
//!
//! A corner is a square [`CORNER`] points on a side and an edge is [`EDGE`] points wide. The corner has
//! to win where the two overlap, so the corners are added after the edges.

use egui::{Rect, Sense, Vec2};
use egui::viewport::ResizeDirection;

/// How far in from an edge the window can be grabbed.
///
/// Six points, for the reason `splitter::GRAB` is eight: a one point target cannot be hit with a mouse.
/// Six rather than eight because these grips are added last and so sit **over** everything at the
/// window's edge, and six is what the activity bar's buttons are inset by — so a button and a grip never
/// want the same point.
pub const EDGE: f32 = 6.0;
/// How far along each edge a corner reaches, which is roughly what a window manager offers.
pub const CORNER: f32 = 16.0;

/// Add the eight grips over `window`, and report a direction when one of them was dragged.
///
/// The caller sends the viewport command, so this component changes nothing itself, which is the rule
/// every component in Quill follows.
pub fn show(ui: &mut egui::Ui, window: Rect) -> Option<ResizeDirection> {
    let mut started = None;
    // The four edges first, then the four corners over them, so a grab in a corner resizes both ways.
    let edges: [(&str, ResizeDirection, Rect, egui::CursorIcon); 4] = [
        (
            "top",
            ResizeDirection::North,
            Rect::from_min_size(window.left_top(), Vec2::new(window.width(), EDGE)),
            egui::CursorIcon::ResizeNorth,
        ),
        (
            "bottom",
            ResizeDirection::South,
            Rect::from_min_size(
                egui::pos2(window.left(), window.bottom() - EDGE),
                Vec2::new(window.width(), EDGE),
            ),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            "left",
            ResizeDirection::West,
            Rect::from_min_size(window.left_top(), Vec2::new(EDGE, window.height())),
            egui::CursorIcon::ResizeWest,
        ),
        (
            "right",
            ResizeDirection::East,
            Rect::from_min_size(
                egui::pos2(window.right() - EDGE, window.top()),
                Vec2::new(EDGE, window.height()),
            ),
            egui::CursorIcon::ResizeEast,
        ),
    ];
    let corners: [(&str, ResizeDirection, egui::Pos2, egui::CursorIcon); 4] = [
        (
            "top left",
            ResizeDirection::NorthWest,
            window.left_top(),
            egui::CursorIcon::ResizeNorthWest,
        ),
        (
            "top right",
            ResizeDirection::NorthEast,
            window.right_top() - Vec2::new(CORNER, 0.0),
            egui::CursorIcon::ResizeNorthEast,
        ),
        (
            "bottom left",
            ResizeDirection::SouthWest,
            window.left_bottom() - Vec2::new(0.0, CORNER),
            egui::CursorIcon::ResizeSouthWest,
        ),
        (
            "bottom right",
            ResizeDirection::SouthEast,
            window.right_bottom() - Vec2::splat(CORNER),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ];

    for (name, direction, area, cursor) in edges {
        if grip(ui, area, name, cursor) {
            started = Some(direction);
        }
    }
    for (name, direction, at, cursor) in corners {
        let area = Rect::from_min_size(at, Vec2::splat(CORNER));
        if grip(ui, area, name, cursor) {
            started = Some(direction);
        }
    }
    started
}

/// One grip: invisible, named, and reporting the frame the drag began on.
///
/// The drag is reported once, when it starts, rather than on every frame of it. `BeginResize` hands the
/// whole drag to the window manager, which then owns the pointer until it is let go — sending it again
/// on the next frame would ask for a second resize inside the first.
fn grip(ui: &mut egui::Ui, area: Rect, name: &str, cursor: egui::CursorIcon) -> bool {
    let response = ui.interact(area, ui.id().with(("resize-window", name)), Sense::drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(cursor);
    }
    // Every control in Quill has a plain name, so a test can find this one without a pointer.
    let label = format!("Resize window: {name}");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), label.clone())
    });
    response.drag_started()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_reaches_further_than_an_edge_so_it_can_win_where_they_overlap() {
        assert!(CORNER > EDGE);
    }
}
