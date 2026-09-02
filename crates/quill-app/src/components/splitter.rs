//! The draggable divider between two panes.
//!
//! Every pane in Quill is resized by dragging its edge, and every one of them uses this. A later pane
//! must use it too rather than growing its own: the grab width, the highlight, the pointer shape and the
//! double click that puts the pane back to its usual size are decided here once.
//!
//! The divider is drawn as a one pixel line, which is what the design shows, but it is grabbed over
//! [`GRAB`] pixels centred on that line, because a one pixel target cannot be hit with a mouse.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::theme::color;

/// How wide the invisible grab area is, centred on the line.
pub const GRAB: f32 = 8.0;

/// Which way the divider runs, and so which way dragging it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// An upright line between two panes side by side. Dragging it changes a width.
    Upright,
    /// A flat line between two panes above and below. Dragging it changes a height.
    Flat,
}

/// What a divider was asked to do this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Drag {
    /// How far the pointer moved along the axis since the last frame, in points.
    pub delta: f32,
    /// The divider was double clicked, which means put the pane back to its usual size.
    pub reset: bool,
}

/// Draw a divider and report the drag.
///
/// `line` is the one pixel line to draw: for an upright divider a rectangle one point wide running the
/// height of the panes, and for a flat one a rectangle one point tall running their width. The grab area
/// is worked out from it.
pub fn show(ui: &mut egui::Ui, line: Rect, id: &str, axis: Axis) -> Drag {
    let hit = match axis {
        Axis::Upright => Rect::from_min_max(
            Pos2::new(line.center().x - GRAB / 2.0, line.top()),
            Pos2::new(line.center().x + GRAB / 2.0, line.bottom()),
        ),
        Axis::Flat => Rect::from_min_max(
            Pos2::new(line.left(), line.center().y - GRAB / 2.0),
            Pos2::new(line.right(), line.center().y + GRAB / 2.0),
        ),
    };
    let response = ui.interact(hit, ui.id().with(("splitter", id)), Sense::click_and_drag());
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(match axis {
            Axis::Upright => egui::CursorIcon::ResizeHorizontal,
            Axis::Flat => egui::CursorIcon::ResizeVertical,
        });
    }

    // The line itself, brighter while it is being pointed at so it is clear it can be moved.
    let colour = if active { color::accent() } else { color::divider() };
    let drawn = if active {
        match axis {
            Axis::Upright => Rect::from_center_size(
                line.center(),
                Vec2::new(2.0, line.height()),
            ),
            Axis::Flat => Rect::from_center_size(line.center(), Vec2::new(line.width(), 2.0)),
        }
    } else {
        line
    };
    ui.painter().rect_filled(drawn, CornerRadius::ZERO, colour);

    let delta = if response.dragged() {
        match axis {
            Axis::Upright => response.drag_delta().x,
            Axis::Flat => response.drag_delta().y,
        }
    } else {
        0.0
    };
    // A divider is a control, so it is named for the tests and for assistive technology.
    let name = format!("Resize {id}");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), name.clone())
    });
    Drag { delta, reset: response.double_clicked() }
}

/// Draw a plain divider with nothing draggable about it.
pub fn line(painter: &egui::Painter, from: Pos2, to: Pos2) {
    painter.line_segment([from, to], Stroke::new(1.0, color::divider()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_divider_reports_no_movement_when_nothing_happens() {
        assert_eq!(Drag::default(), Drag { delta: 0.0, reset: false });
    }
}
