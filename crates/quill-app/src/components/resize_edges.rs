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
//!
//! ## A maximised window has no grips at all, and that is not a nicety
//!
//! `task-1693` reported a window that could not be resized. Driven with real mouse input, a freshly
//! started Quill resizes from all four edges and all four corners, and in `egui_kittest` all eight
//! report `drag_started`. What is broken is what happens when the window manager **refuses** the
//! request.
//!
//! `ViewportCommand::BeginResize` becomes winit's `handle_os_dragging`, which latches a private
//! `dragging` flag, posts `WM_NCLBUTTONDOWN`, and returns early from **every later call** until that
//! flag is cleared. The only place in winit that clears it is `WM_EXITSIZEMOVE` — the end of a modal
//! size or move loop. A posted `WM_NCLBUTTONDOWN` that never starts such a loop therefore latches
//! the flag for the life of the process, and a **maximised** window is exactly that case: Windows
//! turns the hit test into `SC_SIZE`, and `Size` is disabled on a maximised window. One refused edge
//! drag and the window can no longer be resized **or moved** at all, because the title bar's own
//! `StartDrag` goes through the same latch. Measured twice on this machine: after a single edge drag
//! on a maximised window, every later drag did nothing, and a freshly started Quill worked at once.
//!
//! So **no grip is added while the window is maximised**. That is Quill's own rule for a control
//! that can never apply — a maximised window has no size to change — and it is what makes it
//! impossible to send a request the window manager will throw away. The title bar's `StartDrag` is
//! left alone, because Windows *does* handle dragging a maximised window: it restores it and moves
//! it, which is a real modal loop and therefore an honest `WM_EXITSIZEMOVE`.

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
///
/// `maximized` says whether the window is maximised, and when it is **nothing is added at all** — no
/// grip, no cursor, no request. See the note at the top of this file: a resize the window manager
/// refuses does not merely fail, it wedges every later move and resize as well.
pub fn show(ui: &mut egui::Ui, window: Rect, maximized: bool) -> Option<ResizeDirection> {
    if maximized {
        return None;
    }
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

    /// `task-1693`: a maximised window adds no grips, so no resize the window manager would refuse
    /// is ever asked for. See the note at the top of this file for what one refused request costs.
    /// That nothing is *drawn* either is checked in the screenshot tests, which can ask the window
    /// for a control by name.
    #[test]
    fn a_maximised_window_asks_for_no_resize() {
        let context = egui::Context::default();
        let mut answer = Some(ResizeDirection::North);
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let window = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(800.0, 600.0));
            answer = show(ui, window, true);
        });
        // egui insists a pass's texture changes are taken or cleared before the output is dropped.
        output.textures_delta.clear();
        assert_eq!(answer, None, "a maximised window has no size to change");
    }
}
