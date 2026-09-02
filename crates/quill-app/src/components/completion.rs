//! The dropdown of suggestions, drawn under the caret.
//!
//! It reports what was clicked and decides nothing, which is the rule every component in Quill
//! follows: what is offered and what accepting means are both `app::completion`'s.
//!
//! ## It is not an `egui::Popup`, and that is the whole of its shape
//!
//! egui keeps at most one popup open at a time — the rule that already turned the text options
//! panel's three line spacings into three buttons and that puts the colour wheel *inside* the text
//! menu rather than over it. This list has to coexist with nothing at all, but it also must never
//! take the keyboard: the document keeps it, typing flows into the file underneath, and the
//! dropdown is a picture of an offer rather than a control being used. So it is an
//! [`egui::Area`] on the foreground order, positioned by the window from the caret's own geometry
//! every frame, exactly as cheap to draw as the menu it resembles — and it neither opens nor closes
//! anything else.
//!
//! ## Where it goes
//!
//! Under the caret's own line, flipped **above** it when the rows would cross the bottom of the
//! pane, and clamped inside the pane horizontally. The caret's box is the same arithmetic the caret
//! itself is painted with, handed over by the pane that drew it, so the list follows the writing
//! rather than being placed at a remembered point.
//!
//! Up to eight rows, and more scroll: the list is drawn from `CompletionState::shown`, which the
//! window keeps in step with the chosen row.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::completion::CompletionState;
use crate::components::controls;
use crate::theme::{color, icon, size};

/// One row. A menu row, which is what `design/style-guide.md` gives a list of things to choose
/// between — the same 24 points the menu bar, the context menus and the text menu all use.
pub const ROW: f32 = 24.0;
/// How wide the list is. Wide enough for a long identifier and a file name beside it, and narrow
/// enough that it reads as a list hanging off a word rather than as a panel.
const WIDTH: f32 = 360.0;
/// The frame's own margin, matching `components::context_menu`'s.
const PADDING: f32 = 6.0;
/// How far below the caret's line the list hangs, so it never touches the letters it is about.
const GAP: f32 = 4.0;
/// How wide the column holding the kind glyph is.
const GLYPH: f32 = 20.0;

/// What happened in the list this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// A row was clicked, by name. The window takes it exactly as `Enter` would — the click never
    /// reaches the editing area, because the list's own `Area` is in front of it and takes the hit.
    pub accepted: Option<String>,
}

/// Draw the list. `caret` is the caret's box on the screen and `pane` is the editing area it is in.
pub fn show(ui: &mut egui::Ui, state: &CompletionState, caret: Rect, pane: Rect) -> Outcome {
    let mut outcome = Outcome::default();
    let shown = state.shown();
    if shown.is_empty() {
        return outcome;
    }
    let area = where_it_goes(shown.len(), caret, pane);
    egui::Area::new(egui::Id::new("quill-completion"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            // Reserve the whole of it, so egui knows where the list is for the pointer's sake; the
            // drawing itself is at absolute positions, as everything in Quill is.
            ui.allocate_space(area.size());
            frame(ui, area);
            for (offset, index) in shown.clone().enumerate() {
                let row = Rect::from_min_size(
                    Pos2::new(area.left() + PADDING, area.top() + PADDING + offset as f32 * ROW),
                    Vec2::new(area.width() - PADDING * 2.0, ROW),
                );
                if draw_row(ui, row, &state.rows[index], index == state.chosen) {
                    outcome.accepted = Some(state.rows[index].name.clone());
                }
            }
        });
    outcome
}

/// Where the list is drawn: under the caret, flipped above it near the bottom of the pane, and
/// clamped inside the pane's left and right edges.
///
/// A pure function of four numbers, so the flip and the clamp can be checked with no window.
pub fn where_it_goes(rows: usize, caret: Rect, pane: Rect) -> Rect {
    let height = rows as f32 * ROW + PADDING * 2.0;
    let width = WIDTH.min(pane.width().max(120.0));
    let below = caret.bottom() + GAP;
    // Above the caret's own line when the rows would cross the bottom of the pane, and only then:
    // under the word is where the eye already is.
    let top = if below + height > pane.bottom() && caret.top() - GAP - height >= pane.top() {
        caret.top() - GAP - height
    } else {
        below
    };
    let top = top.min((pane.bottom() - height).max(pane.top())).max(pane.top());
    let left = caret.left().min(pane.right() - width).max(pane.left());
    Rect::from_min_size(Pos2::new(left, top), Vec2::new(width, height))
}

/// The popup frame: the menu fill and the one point border every menu in Quill is drawn with.
fn frame(ui: &egui::Ui, area: Rect) {
    ui.painter().rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::menu(),
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
}

/// One row: the kind glyph, the name with its matched letters picked out, and the quiet suffix.
///
/// Named `Completion draw_frame`, because the screenshot tests find controls by name and a control
/// with no name cannot be tested at all.
fn draw_row(ui: &mut egui::Ui, area: Rect, row: &quill_core::completion::Row, chosen: bool) -> bool {
    let name = format!("Completion {}", row.name);
    let response = ui.interact(area, ui.id().with(("completion", &row.name)), Sense::click());
    let painter = ui.painter();
    // One pill, drawn one way: the same `SELECTED_ROW` fill the explorer's open file and every menu
    // row's hover already use.
    if chosen {
        painter.rect_filled(area, CornerRadius::same(4), color::selected_row());
    } else if response.hovered() {
        painter.rect_filled(area, CornerRadius::same(4), color::control());
    }
    if let Some(kind) = row.kind {
        icon::symbol_kind(
            painter,
            Pos2::new(area.left() + GLYPH / 2.0 + 2.0, area.center().y),
            kind,
            color::text_dim(),
        );
    }
    let tint = if chosen { color::text_strong() } else { color::text_control() };
    // The matched letters in the accent colour, which is how `Find in Files` and `Go to File`
    // already answer "why is this row here".
    let galley = controls::marked_text(
        painter,
        &row.name,
        &row.matched,
        tint,
        egui::FontId::proportional(12.5),
    );
    let left = area.left() + GLYPH + 6.0;
    painter.galley(Pos2::new(left, area.center().y - galley.size().y / 2.0), galley, tint);
    if !row.detail.is_empty() {
        let suffix = painter.layout_no_wrap(
            format!("\u{00B7} {}", row.detail),
            egui::FontId::proportional(11.0),
            color::text_faint(),
        );
        painter.galley(
            Pos2::new(area.right() - 6.0 - suffix.size().x, area.center().y - suffix.size().y / 2.0),
            suffix,
            color::text_faint(),
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, &name));
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn the_list_hangs_under_the_caret_and_stays_inside_the_pane() {
        let caret = Rect::from_min_size(Pos2::new(300.0, 200.0), Vec2::new(2.0, 18.0));
        let area = where_it_goes(5, caret, pane());
        assert!(area.top() > caret.bottom(), "under the caret's own line");
        assert_eq!(area.left(), caret.left(), "and lined up with it");
        assert!(pane().contains_rect(area), "{area:?} is outside {:?}", pane());
    }

    #[test]
    fn a_caret_near_the_bottom_of_the_pane_puts_the_list_above_it() {
        // Scenario 22: the rows would cross the bottom, so the list flips.
        let caret = Rect::from_min_size(Pos2::new(300.0, 620.0), Vec2::new(2.0, 18.0));
        let area = where_it_goes(8, caret, pane());
        assert!(area.bottom() < caret.top(), "above the caret: {area:?}");
        assert!(pane().contains_rect(area), "and still on the screen");
    }

    #[test]
    fn a_caret_near_the_right_hand_edge_clamps_the_list_inside_the_pane() {
        let caret = Rect::from_min_size(Pos2::new(880.0, 200.0), Vec2::new(2.0, 18.0));
        let area = where_it_goes(3, caret, pane());
        assert!(area.right() <= pane().right() + 0.01, "{area:?}");
        assert!(area.left() >= pane().left() - 0.01);
    }

    #[test]
    fn a_pane_too_short_to_hold_the_list_either_way_still_puts_it_on_the_screen() {
        // A pane dragged down to nothing is not a reason to draw a list off the window.
        let short = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(300.0, 60.0));
        let caret = Rect::from_min_size(Pos2::new(120.0, 90.0), Vec2::new(2.0, 18.0));
        let area = where_it_goes(8, caret, short);
        assert_eq!(area.top(), short.top(), "clamped to the top rather than drawn off the pane");
        assert!(area.width() <= short.width() + 0.01, "and no wider than the pane");
    }
}
