//! The Find and Replace bar, drawn over the top right of the editing area.
//!
//! `task-1804` §3.1. A bar rather than a modal, and that is the one design decision here worth
//! writing down: `Go to File` and `Find in Files` are modals because they are about the *project*
//! and their answer is a list you read. Find in the current file is about the text you are looking
//! at, and a modal over it would cover the thing being searched. Every editor draws this as a strip
//! in the corner for that reason, and it is why the bar is inset from the top right rather than
//! centred.
//!
//! Nothing here decides what matches. [`crate::services::find::Find`] holds the needle, the matches
//! and which one is current, and is a unit test with no window; this file draws it and reports what
//! was pressed. That is `go_to_file`'s arrangement and `find_in_files`'s.
//!
//! The bar is drawn **after** the editing area and takes the pointer over its own rectangle, so a
//! click on it is not also a click into the text. The band behind every other match is painted by
//! `editor_view::paint`, because the matches are ranges in the laid out text and that is the file
//! that knows where a byte is on the screen.

use egui::{Pos2, Rect, Vec2};

use crate::components::{controls, modal};
use crate::services::find::{Field, Find};
use crate::theme::{color, icon, size};

/// How wide the bar is, and how tall each of its two rows is.
pub const WIDTH: f32 = 460.0;
pub const ROW: f32 = 34.0;
/// How far in from the top right corner of the editing area it sits.
const INSET: f32 = 10.0;

/// What was pressed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Go to the next match, or the previous one.
    pub next: bool,
    pub previous: bool,
    /// Replace the current match, or every match.
    pub replace: bool,
    pub replace_all: bool,
    /// Send every match to `Find in Files`' results, so they can be replaced across the project.
    pub in_files: bool,
    /// Put the bar away.
    pub close: bool,
}

/// How tall the bar is, which the window needs before it draws it so the editing area can be told.
pub fn height(find: &Find) -> f32 {
    if find.replacing { ROW * 2.0 + 8.0 } else { ROW + 8.0 }
}

/// Where the bar sits over an editing area of `area`.
pub fn rect_over(area: Rect, find: &Find) -> Rect {
    let width = WIDTH.min(area.width() - INSET * 2.0);
    Rect::from_min_size(
        Pos2::new(area.right() - INSET - width, area.top() + INSET),
        Vec2::new(width, height(find)),
    )
}

/// Draw the bar and say what was pressed.
pub fn show(ui: &mut egui::Ui, area: Rect, find: &mut Find) -> Outcome {
    let mut outcome = Outcome::default();
    let bar = rect_over(area, find);
    // The pointer belongs to the bar over its own rectangle. Without this a click on Replace All
    // also lands in the text underneath and moves the caret, so the replacement happens somewhere
    // else -- which is the shape of fault the browser pane's `is_any_open` test exists for.
    ui.interact(bar, ui.id().with("find-bar"), egui::Sense::click_and_drag());
    let painter = ui.painter_at(bar);
    painter.rect(
        bar,
        egui::CornerRadius::same(size::CONTROL_CORNER),
        color::menu(),
        egui::Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );

    let first = Rect::from_min_size(
        Pos2::new(bar.left() + 4.0, bar.top() + 4.0),
        Vec2::new(bar.width() - 8.0, ROW),
    );
    outcome.close |= find_row(ui, first, find, &mut outcome);
    if find.replacing {
        let second = Rect::from_min_size(
            Pos2::new(bar.left() + 4.0, first.bottom()),
            Vec2::new(bar.width() - 8.0, ROW),
        );
        replace_row(ui, second, find, &mut outcome);
    }
    outcome
}

/// The Find row: the disclosure, the box, the tally, the two toggles, previous, next and close.
fn find_row(ui: &mut egui::Ui, row: Rect, find: &mut Find, outcome: &mut Outcome) -> bool {
    let mut pen = row.left();
    // The disclosure that opens the Replace row, which is where every editor puts it and is what
    // makes Replace discoverable without knowing `Ctrl+H`.
    let toggle = Rect::from_min_size(Pos2::new(pen, row.top()), Vec2::splat(ROW));
    // Two function pointers rather than one closure over `find.replacing`, because `icon_button`
    // takes a `fn` -- a drawing is a fixed mark rather than something a caller builds.
    let mark = if find.replacing { pointing_down } else { pointing_right };
    if controls::icon_button(ui, toggle, "Replace", mark) {
        find.replacing = !find.replacing;
    }
    pen += ROW;

    // Right to left from here, because the buttons are fixed widths and the box takes what is left.
    let mut right = row.right();
    let close = Rect::from_min_size(Pos2::new(right - ROW, row.top()), Vec2::splat(ROW));
    if controls::icon_button(ui, close, "Close find", icon::cross) {
        return true;
    }
    right -= ROW;
    let next = Rect::from_min_size(Pos2::new(right - ROW, row.top()), Vec2::splat(ROW));
    outcome.next |= controls::icon_button(ui, next, "Next match", icon::chevron_down);
    right -= ROW;
    let previous = Rect::from_min_size(Pos2::new(right - ROW, row.top()), Vec2::splat(ROW));
    outcome.previous |= controls::icon_button(ui, previous, "Previous match", icon::chevron_up);
    right -= ROW + 4.0;

    // The two toggles, drawn as the words every editor draws: `Aa` for case, `ab` underlined for a
    // whole word. Words rather than drawn marks because these two are conventions a person already
    // knows, and a mark of our own would have to be learnt.
    let word = Rect::from_min_size(Pos2::new(right - 30.0, row.top() + 3.0), Vec2::new(30.0, ROW - 6.0));
    if controls::choice_button_named(ui, word, "ab", "Whole word", find.whole_word) {
        find.whole_word = !find.whole_word;
    }
    right -= 34.0;
    let case = Rect::from_min_size(Pos2::new(right - 30.0, row.top() + 3.0), Vec2::new(30.0, ROW - 6.0));
    if controls::choice_button_named(ui, case, "Aa", "Match case", find.match_case) {
        find.match_case = !find.match_case;
    }
    right -= 34.0;

    // The tally sits between the box and the toggles, right aligned against them, so the box does
    // not resize as the count changes.
    if let Some(tally) = find.tally() {
        let painter = ui.painter_at(row);
        let galley = painter.layout_no_wrap(
            tally,
            egui::FontId::proportional(11.0),
            color::text_faint(),
        );
        painter.galley(
            Pos2::new(right - 6.0 - galley.size().x, row.center().y - galley.size().y / 2.0),
            galley.clone(),
            color::text_faint(),
        );
        right -= galley.size().x + 12.0;
    }

    let box_rect = Rect::from_min_size(
        Pos2::new(pen, row.top() + 3.0),
        Vec2::new((right - pen).max(60.0), ROW - 6.0),
    );
    // Named `Find text` rather than `Find`, because the menu bar has a word `Find` in it now and a
    // name that matches two things in one window is a name neither a test nor a screen reader can
    // use to mean one of them.
    let entry = modal::field(ui, box_rect, "Find text", &mut find.needle);
    // The box has the keyboard from the moment the bar opens, for `go_to_file`'s reason: a search
    // box that has to be clicked before it can be typed into is a search box that gets typed past.
    if find.field == Field::Find && !entry.has_focus() {
        entry.request_focus();
    }
    if entry.gained_focus() {
        find.field = Field::Find;
    }
    false
}

/// The disclosure open, which is what the Replace row showing looks like.
fn pointing_down(painter: &egui::Painter, centre: Pos2, colour: egui::Color32) {
    icon::disclosure(painter, centre, true, colour);
}

/// And shut.
fn pointing_right(painter: &egui::Painter, centre: Pos2, colour: egui::Color32) {
    icon::disclosure(painter, centre, false, colour);
}

/// The Replace row: the box, Replace, Replace All, and the button that sends it to the project.
fn replace_row(ui: &mut egui::Ui, row: Rect, find: &mut Find, outcome: &mut Outcome) {
    let pen = row.left() + ROW;
    let mut right = row.right();

    // The three buttons are words rather than marks: "Replace All" is a destructive thing to press
    // by accident and a drawn icon for it would be a guess.
    for (label, announced, pressed) in [
        ("In Files", "Replace in files", &mut outcome.in_files),
        ("All", "Replace all", &mut outcome.replace_all),
        ("Replace", "Replace", &mut outcome.replace),
    ] {
        let width = if label == "Replace" { 64.0 } else { 56.0 };
        let button =
            Rect::from_min_size(Pos2::new(right - width, row.top() + 3.0), Vec2::new(width, ROW - 6.0));
        *pressed |= controls::choice_button_named(ui, button, label, announced, false);
        right -= width + 4.0;
    }

    let box_rect = Rect::from_min_size(
        Pos2::new(pen, row.top() + 3.0),
        Vec2::new((right - pen).max(60.0), ROW - 6.0),
    );
    let entry = modal::field(ui, box_rect, "Replace with", &mut find.replacement);
    if find.field == Field::Replace && !entry.has_focus() {
        entry.request_focus();
    }
    if entry.gained_focus() {
        find.field = Field::Replace;
    }
}
