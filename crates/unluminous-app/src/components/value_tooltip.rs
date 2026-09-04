//! The value under the pointer: a small tree hanging off a name in the source while the program is
//! paused.
//!
//! `task-1696`, and `tasks/task-1696-value-tooltip-tdd.md` is the design. It is the reference editor's value
//! tooltip: rest the pointer on a name and its value appears, a structure opens into its fields, and
//! a value can be typed over.
//!
//! ## It is an `egui::Area`, and that is the same decision four times now
//!
//! egui keeps at most one popup open at a time — the rule that turned the text options panel's three
//! line spacings into three buttons, that put the colour wheel *inside* the text menu, and that
//! shaped `components::completion`. This list has to coexist with all of them and must never take
//! the keyboard, so it is an [`egui::Area`] on the foreground order, positioned by the window from
//! geometry the pane recorded, and drawn **after the pane loop** — so it is never under a divider
//! and never drawn twice in a split view.
//!
//! ## One row means one thing
//!
//! The rows are [`debug_panel::show_row`], the same function the debug tile draws its variables
//! with, so a disclosure triangle, a type and a changed value look the same wherever they are.
//! What this file owns is the frame, where it hangs, and how wide it is.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::debug::HoverValue;
use crate::components::debug_panel::{self, RowOutcome};
use crate::theme::{color, size};

/// How tall one row is. The style guide's list row, which is what the tile uses.
pub const ROW: f32 = size::ROW;
/// The frame's own margin, matching `components::completion`'s.
const PADDING: f32 = 6.0;
/// How far below the word the popup hangs, so it never touches the letters it is about.
pub const GAP: f32 = 4.0;
/// The narrowest it is drawn, so a one-character value still reads as a panel rather than a chip.
const MIN_WIDTH: f32 = 220.0;
/// The widest, before a long value is left to the eliding `show_row` already does.
const MAX_WIDTH: f32 = 560.0;
/// How many rows are drawn before the rest scroll. A popup as tall as the window is a panel, and the
/// debug tile is where a panel belongs.
const MAX_ROWS: usize = 12;

/// What happened in the popup this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// A row was opened or closed.
    pub toggle_row: Option<String>,
    /// A row was given a new value.
    pub set_value: Option<(String, String)>,
    /// Where it was drawn, which is half of what decides whether the pointer has left it. `None`
    /// when there was nothing to draw at all.
    pub area: Option<Rect>,
    /// Which side of the word it went, so the next frame puts it on the same one. See
    /// [`where_it_goes`].
    pub above: Option<bool>,
}

/// Draw the popup. `word` is the box of the letters it is about and `pane` the editing area they are
/// in, which it is flipped and clamped inside.
///
/// `can_set_root` and `can_set_child` are separate because the two rows are changed by two different
/// requests: the root by `setExpression`, which not every adapter offers, and a child by
/// `setVariable`, which the tile already sends. A control whose capability is absent is absent, so
/// an adapter offering only the second still lets every field of a struct be typed over.
pub fn show(
    ui: &mut egui::Ui,
    hover: &HoverValue,
    editing: &mut Option<(String, String)>,
    can_set_root: bool,
    can_set_child: bool,
    word: Rect,
    pane: Rect,
    above: Option<bool>,
) -> Outcome {
    let mut outcome = Outcome::default();
    let message = waiting_or_refusal(hover);
    let rows = match message.is_some() {
        true => 1,
        false => hover.rows.len(),
    };
    if rows == 0 {
        return outcome;
    }
    let width = how_wide(ui, hover, message.as_deref());
    let above = above.or_else(|| Some(goes_above(rows, word, pane)));
    let area = where_it_goes(rows, width, word, pane, above);
    outcome.area = Some(area);
    outcome.above = above;
    egui::Area::new(egui::Id::new("unluminous-value-tooltip"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            // Reserved so egui knows where the popup is for the pointer's sake; the drawing itself is
            // at absolute positions, as everything in Unluminous is.
            ui.allocate_space(area.size());
            frame(ui, area);
            if let Some(said) = message {
                return sentence(ui, area, &said);
            }
            let inner = area.shrink(PADDING);
            let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            scroll.set_clip_rect(ui.painter().clip_rect().intersect(inner));
            egui::ScrollArea::vertical().id_salt("value-tooltip-rows").show(
                &mut scroll,
                |ui| {
                    // No gap between the rows: the popup's height is `rows * ROW` exactly, and
                    // egui's default spacing between allocated widgets would push the last row out
                    // of a box measured without it.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut what = RowOutcome::default();
                    for row in &hover.rows {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(inner.width(), ROW), Sense::click());
                        let can_set = match row.depth {
                            0 => can_set_root,
                            _ => can_set_child,
                        };
                        debug_panel::show_row(
                            ui, rect, response, row, editing, can_set, "Value", &mut what,
                        );
                    }
                    outcome.toggle_row = what.toggle_row;
                    outcome.set_value = what.set_value;
                },
            );
        });
    outcome
}

/// The one line drawn instead of a tree: the debugger's refusal, or that it has not answered yet.
///
/// **Its own words, never Unluminous's** — a debugger explains a name that does not resolve in this frame
/// far better than an editor could, which is the rule `unluminous-git` keeps about git's standard error.
fn waiting_or_refusal(hover: &HoverValue) -> Option<String> {
    if let Some(said) = hover.refusal() {
        return Some(said.to_owned());
    }
    hover.is_waiting().then(|| format!("{}\u{2026}", hover.expression))
}

/// Which side of the word the popup goes: under it, or above it when the rows would cross the bottom
/// of the pane and there is room above.
///
/// **Decided once, when the popup opens, and then kept** — which is [`where_it_goes`]'s `above`
/// argument. Opening a row makes the tree taller, and a popup that re-decided every frame would leap
/// from below the word to above it the moment somebody clicked a disclosure triangle: the rows the
/// pointer was walking down would move out from under it, and the popup would put itself away. Under
/// the word is where the eye already is, so that is the side asked for first.
pub fn goes_above(rows: usize, word: Rect, pane: Rect) -> bool {
    let height = height_of(rows);
    word.bottom() + GAP + height > pane.bottom() && word.top() - GAP - height >= pane.top()
}

/// Where the popup hangs, clamped inside the pane's edges.
///
/// A pure function of its arguments, so the side and the clamp can be checked with no window — which
/// is `completion::where_it_goes`'s own arrangement, and this is deliberately the same shape.
/// `above` is the side already settled on, or `None` to work it out from this many rows.
pub fn where_it_goes(
    rows: usize,
    width: f32,
    word: Rect,
    pane: Rect,
    above: Option<bool>,
) -> Rect {
    let above = above.unwrap_or_else(|| goes_above(rows, word, pane));
    // How tall it may be **on the side it is on**, so a tree that grew past the pane scrolls rather
    // than hanging off the end of it. One row always fits, or there would be nothing to look at.
    let room = match above {
        true => word.top() - GAP - pane.top(),
        false => pane.bottom() - word.bottom() - GAP,
    };
    let height = height_of(rows).min(room.max(ROW + PADDING * 2.0));
    let width = width.min(pane.width().max(120.0));
    let top = match above {
        true => word.top() - GAP - height,
        false => word.bottom() + GAP,
    };
    let top = top.min((pane.bottom() - height).max(pane.top())).max(pane.top());
    let left = word.left().min(pane.right() - width).max(pane.left());
    Rect::from_min_size(Pos2::new(left, top), Vec2::new(width, height))
}

/// How tall a tree of this many rows wants to be.
fn height_of(rows: usize) -> f32 {
    rows.min(MAX_ROWS) as f32 * ROW + PADDING * 2.0
}

/// How wide it has to be to hold what is in it, measured rather than guessed at.
///
/// The alternative is a fixed width, which the completion list can have because every row in it is
/// one identifier. A value is anything at all — `3`, `Vec(2)`, a whole struct printed by the
/// debugger — so a fixed width would be far too wide for the first and far too narrow for the last.
fn how_wide(ui: &egui::Ui, hover: &HoverValue, message: Option<&str>) -> f32 {
    let name_font = egui::FontId::monospace(11.5);
    let kind_font = egui::FontId::monospace(10.5);
    let measure = |text: String, font: egui::FontId| {
        ui.painter().layout_no_wrap(text, font, color::text()).size().x
    };
    if let Some(said) = message {
        let wanted = measure(said.to_owned(), name_font) + PADDING * 2.0 + 24.0;
        return wanted.clamp(MIN_WIDTH, MAX_WIDTH);
    }
    let mut widest: f32 = 0.0;
    for row in &hover.rows {
        // The same pen positions `show_row` walks, so the number is what will really be drawn.
        let mut pen = 12.0 + row.depth as f32 * debug_panel::INDENT + 16.0;
        pen += measure(row.name.clone(), name_font.clone()) + 10.0;
        if let Some(kind) = &row.kind {
            pen += measure(kind.clone(), kind_font.clone()) + 10.0;
        }
        pen += measure(debug_panel::elide(&row.value), name_font.clone()) + 12.0;
        widest = widest.max(pen);
    }
    (widest + PADDING * 2.0).clamp(MIN_WIDTH, MAX_WIDTH)
}

/// The popup frame: the menu fill and the one point border every menu in Unluminous is drawn with.
fn frame(ui: &egui::Ui, area: Rect) {
    ui.painter().rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::menu(),
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
}

/// One line of words in the middle of the popup, for a question with no tree behind it.
fn sentence(ui: &egui::Ui, area: Rect, said: &str) {
    let galley = ui.painter().layout_no_wrap(
        said.to_owned(),
        egui::FontId::monospace(11.5),
        color::text_dim(),
    );
    ui.painter().galley(
        Pos2::new(area.left() + PADDING + 12.0, area.center().y - galley.size().y / 2.0),
        galley,
        color::text_dim(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(600.0, 400.0))
    }

    #[test]
    fn it_hangs_under_the_word_it_is_about() {
        let word = Rect::from_min_size(Pos2::new(160.0, 120.0), Vec2::new(40.0, 16.0));
        let area = where_it_goes(3, 300.0, word, pane(), None);
        assert_eq!(area.left(), 160.0, "aligned with the name");
        assert_eq!(area.top(), 140.0, "the word's bottom plus the gap");
    }

    /// Near the bottom of the pane it goes above the word rather than being pushed up over it,
    /// which is what keeps the name it is about visible.
    #[test]
    fn near_the_bottom_it_goes_above_the_word() {
        let word = Rect::from_min_size(Pos2::new(160.0, 420.0), Vec2::new(40.0, 16.0));
        assert!(goes_above(8, word, pane()));
        let area = where_it_goes(8, 300.0, word, pane(), None);
        assert!(area.bottom() <= 432.0, "above the word: {area:?}");
    }

    /// **The side is settled once and then kept.** Opening a row makes the tree taller, and a popup
    /// that re-decided every frame would leap from under the word to above it the moment somebody
    /// clicked a disclosure triangle — taking the rows out from under the pointer that was walking
    /// down them, which then puts the popup away. Measured on a real window before it was fixed.
    #[test]
    fn a_row_being_opened_does_not_move_the_popup_to_the_other_side() {
        let word = Rect::from_min_size(Pos2::new(160.0, 300.0), Vec2::new(40.0, 16.0));
        let below = goes_above(3, word, pane());
        assert!(!below, "three rows fit under it");
        let small = where_it_goes(3, 300.0, word, pane(), Some(below));
        let grown = where_it_goes(9, 300.0, word, pane(), Some(below));
        assert_eq!(small.top(), grown.top(), "the same side, and the same top edge");
        assert!(grown.bottom() <= pane().bottom() + 0.01, "and still inside the pane");
    }

    #[test]
    fn it_is_clamped_inside_the_panes_right_edge() {
        let word = Rect::from_min_size(Pos2::new(650.0, 120.0), Vec2::new(30.0, 16.0));
        let area = where_it_goes(2, 300.0, word, pane(), None);
        assert!(area.right() <= pane().right() + 0.01, "inside the pane: {area:?}");
    }

    /// A popup as tall as the window is a panel, and the debug tile is where a panel belongs.
    #[test]
    fn a_deep_tree_scrolls_rather_than_growing_for_ever() {
        let word = Rect::from_min_size(Pos2::new(160.0, 60.0), Vec2::new(40.0, 16.0));
        let many = where_it_goes(200, 300.0, word, pane(), Some(false));
        let capped = where_it_goes(MAX_ROWS, 300.0, word, pane(), Some(false));
        assert_eq!(many.height(), capped.height());
    }
}
