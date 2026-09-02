//! `Find in Files`: search the whole project for some text, and read the file a result is in.
//!
//! `task-1659` asks for IntelliJ's, including the part of it that makes it worth opening rather than
//! grepping: the **preview under the results**, showing the whole of the file the chosen result is
//! in with the matching line picked out. A list of file names and line numbers tells you where a
//! word is; the preview tells you whether it is the one you meant, without opening anything.
//!
//! Three pieces, and none of them is in this file. `services::text_search` decides what matches and
//! reads the project on a thread. `components::modal` is the frame, the dragging and the resizing.
//! `components::splitter` is the divider between the results and the preview, because every pane in
//! Quill is resized by dragging its edge and a pane inside a modal is still a pane.
//!
//! What this file does is lay those out, and hold what has been typed. Like every other component it
//! changes nothing: opening a result is an [`FindOutcome`] the window acts on, which is what puts the
//! selection on the match in the real document.

use egui::{Pos2, Rect, Vec2};

use crate::components::{controls, modal, splitter};
use crate::services::text_search::{self, Hit, Query, Searcher};
use crate::theme::{color, size};

/// How large the modal is before anything has been dragged.
const WIDTH: f32 = 980.0;
const HEIGHT: f32 = 680.0;
/// How much of the modal the results take before the divider is dragged, and the least and most
/// they can be dragged to.
pub const SPLIT: f32 = 0.45;
pub const SPLIT_MIN: f32 = 0.15;
pub const SPLIT_MAX: f32 = 0.85;
/// The largest file the preview reads. A file larger than this is one nobody is reading in a
/// preview pane, and reading it happens where the window draws.
const PREVIEW_LIMIT: u64 = 4 * 1024 * 1024;

/// The file under the results, once it has been read.
#[derive(Debug, Default)]
struct Preview {
    path: std::path::PathBuf,
    lines: Vec<String>,
    /// Why the file could not be read, when it could not be.
    problem: Option<String>,
}

/// What has been typed into `Find in Files`, what it found, and which result is chosen.
pub struct FindInFiles {
    pub query: String,
    pub match_case: bool,
    pub chosen: usize,
    hits: Vec<Hit>,
    files: usize,
    searching: bool,
    capped: bool,
    /// The question the running search is answering, so a new one is asked only when it changes.
    asked: Option<Query>,
    searcher: Searcher,
    preview: Option<Preview>,
    /// Set when the choice moved, so the list and the preview scroll to it.
    follow: bool,
    /// The result the preview was last scrolled to, so it is scrolled again when the choice really
    /// changes and not on every frame.
    ///
    /// It is not enough to set [`Self::follow`] when the query changes. The thread answers in
    /// batches, so on the frame the query changes there is usually nothing chosen yet, and the
    /// scroll was being spent on an empty preview: the first result then arrived and the preview
    /// opened at the top of a two thousand line file with the match hundreds of lines below. Found
    /// by looking at the real window, which is what the fourth layer of tests is for.
    followed: Option<(std::path::PathBuf, usize)>,
}

impl FindInFiles {
    /// Open the modal and start the thread it searches on.
    ///
    /// `wake` asks the window to draw again, which is what makes results appear while nothing is
    /// being typed. The thread lives as long as this struct: shutting the modal drops it, the
    /// request channel closes, and the thread ends.
    pub fn open(wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            query: String::new(),
            match_case: false,
            chosen: 0,
            hits: Vec::new(),
            files: 0,
            searching: false,
            capped: false,
            asked: None,
            searcher: Searcher::start(wake),
            preview: None,
            follow: false,
            followed: None,
        }
    }

    /// Ask the thread a new question if what is being searched for has changed, and take in whatever
    /// it has answered.
    ///
    /// Called by the window each frame before the modal is drawn, because the project's file list
    /// belongs to the window.
    pub fn pump(&mut self, files: &[std::path::PathBuf]) {
        let wanted = Query {
            needle: self.query.trim().to_owned(),
            match_case: self.match_case,
            words: false,
        };
        if self.asked.as_ref() != Some(&wanted) {
            self.searcher.send(files.to_vec(), wanted.clone());
            self.asked = Some(wanted);
            self.hits.clear();
            self.files = 0;
            self.chosen = 0;
            self.capped = false;
            self.searching = true;
            self.preview = None;
        }
        for reply in self.searcher.poll() {
            self.hits.extend(reply.hits);
            self.files = reply.files;
            self.capped |= reply.capped;
            if reply.done {
                self.searching = false;
            }
        }
        self.read_the_preview();
        // Whenever the result being looked at changes — because the arrow keys moved, because a row
        // was clicked, or because the first answer has just arrived — both panes scroll to it.
        let target = self.chosen_hit().map(|hit| (hit.path.clone(), hit.line));
        if target != self.followed {
            self.followed = target;
            self.follow = true;
        }
    }

    /// Read the file the chosen result is in, if it is not the one already read.
    fn read_the_preview(&mut self) {
        let Some(hit) = self.hits.get(self.chosen) else {
            self.preview = None;
            return;
        };
        if self.preview.as_ref().is_some_and(|preview| preview.path == hit.path) {
            return;
        }
        let path = hit.path.clone();
        let too_large = std::fs::metadata(&path).map(|meta| meta.len() > PREVIEW_LIMIT).unwrap_or(false);
        let preview = if too_large {
            Preview {
                path,
                lines: Vec::new(),
                problem: Some("This file is too large to preview.".to_owned()),
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(text) => Preview {
                    path,
                    lines: text.split('\n').map(|line| line.trim_end_matches('\r').to_owned()).collect(),
                    problem: None,
                },
                Err(problem) => Preview { path, lines: Vec::new(), problem: Some(problem.to_string()) },
            }
        };
        self.preview = Some(preview);
    }

    /// Everything found so far, for a test and for the window.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// The result that is chosen, if there is one.
    pub fn chosen_hit(&self) -> Option<&Hit> {
        self.hits.get(self.chosen)
    }

    /// True while the thread is still reading the project.
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    /// The result the preview has been scrolled to, which is what a test asks to check that the
    /// preview really followed the choice rather than sitting at the top of the file.
    pub fn scrolled_to(&self) -> Option<(&std::path::Path, usize)> {
        self.followed.as_ref().map(|(path, line)| (path.as_path(), *line))
    }

    fn move_choice(&mut self, by: i32) {
        if self.hits.is_empty() {
            return;
        }
        let last = self.hits.len() as i32 - 1;
        self.chosen = (self.chosen as i32 + by).clamp(0, last) as usize;
    }

    /// What the footer says: how much was found, or that nothing was.
    fn summary(&self) -> String {
        if self.query.trim().is_empty() {
            return "Type to search this project".to_owned();
        }
        let matches = match self.hits.len() {
            0 => "No matches".to_owned(),
            1 => "1 match".to_owned(),
            many => format!("{many} matches"),
        };
        let mut text = format!("{matches} in {} files", self.files);
        if self.searching {
            text.push_str(" \u{00B7} searching");
        }
        if self.capped {
            text = format!("The first {} matches \u{00B7} there are more", text_search::LIMIT);
        }
        text
    }
}

/// What `Find in Files` asked for this frame.
#[derive(Debug, Default, PartialEq)]
pub struct FindOutcome {
    /// Open this file with this range of it selected, and shut the modal.
    pub open: Option<(std::path::PathBuf, std::ops::Range<usize>)>,
    /// Shut the modal.
    pub close: bool,
    /// How far the divider between the results and the preview was dragged, in points.
    pub drag: f32,
    /// The divider was double clicked, so the split goes back to its usual place.
    pub reset_split: bool,
    /// How tall the two panes are together, so the caller can turn a drag in points into the
    /// fraction it holds. The modal can be resized, so this is not a constant.
    pub panes_height: f32,
}

/// Draw the modal. `split` is how much of it the results take, which the window owns because it is
/// written to the settings file like every other pane.
pub fn show(ctx: &egui::Context, state: &mut FindInFiles, split: f32) -> FindOutcome {
    let mut outcome = FindOutcome::default();
    let (_, closed) = modal::show(ctx, "quill-find-in-files", WIDTH, HEIGHT, |ui, area| {
        if modal::header(ui, area, "Find in Files") {
            outcome.close = true;
        }
        // Claimed before the field is drawn, for the reason `go_to_file` gives: egui leaves the
        // events a text box consumed in the frame's list, and walking a list of results is not the
        // same as moving a caret.
        let (down, up, enter) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        if down {
            state.move_choice(1);
        }
        if up {
            state.move_choice(-1);
        }

        let body = modal::body(area);
        let tick_width = 120.0;
        let field = Rect::from_min_size(body.min, Vec2::new(body.width() - tick_width - 12.0, 30.0));
        let entry =
            controls::search_field(ui, field, "Find in files", "Text to find", &mut state.query);
        if !entry.has_focus() {
            entry.request_focus();
        }
        let tick = Rect::from_min_size(
            Pos2::new(body.right() - tick_width, field.top()),
            Vec2::new(tick_width, 30.0),
        );
        modal::check(ui, tick, "Match case", &mut state.match_case);

        // The results above, the preview below, and the divider between them.
        let panes = Rect::from_min_max(Pos2::new(body.left(), field.bottom() + 12.0), body.max);
        outcome.panes_height = panes.height();
        let split = split.clamp(SPLIT_MIN, SPLIT_MAX);
        let results_height = (panes.height() * split).floor();
        let results = Rect::from_min_size(panes.min, Vec2::new(panes.width(), results_height));
        let preview = Rect::from_min_max(Pos2::new(panes.left(), results.bottom() + 9.0), panes.max);

        if let Some(opened) = rows(ui, results, state) {
            outcome.open = Some(opened);
        }
        show_preview(ui, preview, state);
        // Added after both panes, for the reason `components::splitter` records: a widget added
        // earlier sits underneath one added later, and the two lists take drags over the whole of
        // their rectangles.
        let line = Rect::from_min_size(
            Pos2::new(panes.left(), results.bottom() + 4.0),
            Vec2::new(panes.width(), 1.0),
        );
        let drag = splitter::show(ui, line, "find results", splitter::Axis::Flat);
        outcome.drag = drag.delta;
        outcome.reset_split = drag.reset;

        if enter {
            outcome.open = state.chosen_hit().map(|hit| (hit.path.clone(), hit.offset.clone()));
        }

        let summary = state.summary();
        modal::label(
            &ui.painter_at(area),
            Rect::from_min_size(
                Pos2::new(area.left() + 20.0, area.bottom() - modal::FOOTER),
                Vec2::new(400.0, modal::FOOTER),
            ),
            area.left() + 20.0,
            &summary,
            color::text_faint(),
            11.0,
        );
        if modal::footer(ui, area, &[("OPEN", state.chosen_hit().is_some())]) == Some(0) {
            outcome.open = state.chosen_hit().map(|hit| (hit.path.clone(), hit.offset.clone()));
        }
    });
    if closed {
        outcome.close = true;
    }
    if outcome.open.is_some() {
        outcome.close = true;
    }
    outcome
}

/// The results: one row for each match, saying which file and line it is on and showing the line
/// with the match picked out. Returns the one that was double clicked.
fn rows(
    ui: &mut egui::Ui,
    area: Rect,
    state: &mut FindInFiles,
) -> Option<(std::path::PathBuf, std::ops::Range<usize>)> {
    frame(ui, area);
    let mut opened = None;
    let mut chose = None;
    let mut follow_to = None;
    let inner = area.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    egui::ScrollArea::vertical().id_salt("find-rows").show(&mut child, |ui| {
        if state.hits.is_empty() {
            ui.add_space(8.0);
            let words = if state.query.trim().is_empty() {
                "  Type something to find it in this project"
            } else if state.searching {
                "  Searching..."
            } else {
                "  Nothing in this project matches"
            };
            ui.label(egui::RichText::new(words).size(11.5).color(color::text_faint()));
            return;
        }
        for (index, hit) in state.hits.iter().enumerate() {
            let chosen = index == state.chosen;
            let name = hit
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let where_it_is = format!("{name}:{}", hit.line);
            // A result's name is where the match is, which is unique inside this list and reads as
            // what it is when it is spoken.
            let label = format!("Result {where_it_is}");
            let (text, marks) = picked_out(&hit.text, &hit.range);
            let response = modal::row(ui, index, &label, chosen, |painter, row| {
                let tint = if chosen { color::text_strong() } else { color::text_dim() };
                let x = modal::label(painter, row, row.left() + 16.0, &where_it_is, tint, 11.5);
                let galley = controls::marked_text(
                    painter,
                    &text,
                    &marks,
                    color::text_control(),
                    egui::FontId::monospace(11.5),
                );
                painter.galley(
                    Pos2::new(x + 16.0, row.center().y - galley.size().y / 2.0),
                    galley,
                    color::text_control(),
                );
            });
            if response.clicked() {
                chose = Some(index);
            }
            if response.double_clicked() {
                opened = Some((hit.path.clone(), hit.offset.clone()));
            }
            if chosen && state.follow {
                follow_to = Some(response.rect);
            }
        }
    });
    if let Some(index) = chose {
        state.chosen = index;
    }
    if let Some(rect) = follow_to {
        child.scroll_to_rect(rect, None);
    }
    opened
}

/// The whole of the file the chosen result is in, with the matching line picked out.
///
/// Only the rows that are on screen are laid out, which is what `show_rows` is for: a preview of a
/// ten thousand line file that laid every line out would cost more than the search that found it.
fn show_preview(ui: &mut egui::Ui, area: Rect, state: &mut FindInFiles) {
    frame(ui, area);
    let inner = area.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    let Some(preview) = &state.preview else {
        let mut ui = child;
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("  Choose a result to see the file it is in")
                .size(11.5)
                .color(color::text_faint()),
        );
        return;
    };
    if let Some(problem) = &preview.problem {
        let mut ui = child;
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("  {problem}")).size(11.5).color(color::close()));
        return;
    }
    let hit = state.hits.get(state.chosen);
    let matched_line = hit.map(|hit| hit.line).unwrap_or(0);
    let row_height = child.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    // What one row really costs. `ScrollArea::show_rows` is given the height of a row *without* the
    // spacing between rows and adds `item_spacing.y` itself, so a scroll offset worked out from the
    // row height alone lands about a quarter of the way short — which is how a match on line 1770 of
    // a real file opened at line 1307. Found by looking at the real window.
    let pitch = row_height + child.spacing().item_spacing.y;
    let mut scroll = egui::ScrollArea::both().id_salt("find-preview");
    if state.follow {
        scroll = scroll.vertical_scroll_offset(scroll_to_line(matched_line, pitch, inner.height()));
        state.follow = false;
    }
    let lines = preview.lines.clone();
    let range = hit.map(|hit| hit.range.clone()).unwrap_or(0..0);
    scroll.show_rows(&mut child, row_height, lines.len(), |ui, shown| {
        for index in shown {
            let number = index + 1;
            let line = &lines[index];
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width().max(1.0), row_height), egui::Sense::hover());
            let painter = ui.painter();
            let on_the_match = number == matched_line;
            if on_the_match {
                painter.rect_filled(rect, egui::CornerRadius::same(3), color::selected_row());
            }
            let tint = if on_the_match { color::text_strong() } else { color::text_dim() };
            modal::label(painter, rect, rect.left() + 6.0, &format!("{number:>5}"), color::text_faint(), 10.5);
            let marks = if on_the_match { char_marks(line, &range) } else { Vec::new() };
            let galley = controls::marked_text(
                painter,
                line,
                &marks,
                tint,
                egui::FontId::monospace(11.5),
            );
            painter.galley(
                Pos2::new(rect.left() + 52.0, rect.center().y - galley.size().y / 2.0),
                galley,
                tint,
            );
        }
    });
}

/// How far the preview is scrolled so that `line` sits a third of the way down a pane `height` tall.
///
/// A third of the way down rather than at the top, so there is something above the match as well as
/// below it, which is what a person needs to recognise where they are. `pitch` is what one row
/// really costs, spacing included.
fn scroll_to_line(line: usize, pitch: f32, height: f32) -> f32 {
    ((line.saturating_sub(1)) as f32 * pitch - height / 3.0).max(0.0)
}

/// The frame round one of the two panes, drawn the way every sunken box in Quill is.
fn frame(ui: &egui::Ui, area: Rect) {
    ui.painter().rect(
        area,
        egui::CornerRadius::same(size::CONTROL_CORNER),
        color::editor(),
        egui::Stroke::new(1.0, color::divider()),
        egui::StrokeKind::Inside,
    );
}

/// A matching line trimmed of the indent in front of it, and where the match is in what is left.
///
/// Deeply indented code would otherwise show a row of nothing with the interesting part off the
/// right hand end. The positions move with the text, which is the whole reason this is a function
/// rather than two lines at the call site.
fn picked_out(text: &str, range: &std::ops::Range<usize>) -> (String, Vec<usize>) {
    let indent = text.len() - text.trim_start().len();
    let indent = indent.min(range.start);
    let trimmed = text[indent..].to_owned();
    let moved = (range.start - indent)..(range.end - indent);
    let marks = char_marks(&trimmed, &moved);
    (trimmed, marks)
}

/// A byte range turned into the character positions inside it, which is what a marked galley wants.
fn char_marks(text: &str, range: &std::ops::Range<usize>) -> Vec<usize> {
    if range.is_empty() || range.end > text.len() {
        return Vec::new();
    }
    let before = text[..range.start].chars().count();
    let inside = text[range.clone()].chars().count();
    (before..before + inside).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indent_in_front_of_a_matching_line_is_trimmed_and_the_match_moves_with_it() {
        let (text, marks) = picked_out("        let needle = 1;", &(12..18));
        assert_eq!(text, "let needle = 1;");
        assert_eq!(&text[4..10], "needle");
        assert_eq!(marks, vec![4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn a_match_inside_the_indent_itself_is_not_trimmed_away() {
        let (text, marks) = picked_out("    \tindented", &(0..4));
        assert_eq!(text, "    \tindented", "trimming would have moved the match out of the line");
        assert_eq!(marks, vec![0, 1, 2, 3]);
    }

    #[test]
    fn letters_wider_than_one_byte_are_counted_as_letters_and_not_as_bytes() {
        // The match is the six bytes of `needle`, but it is the fourth character onward, because
        // each of the three accented letters in front of it is two bytes.
        let marks = char_marks("\u{00E9}\u{00E9}\u{00E9}needle", &(6..12));
        assert_eq!(marks, vec![3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn a_range_that_is_not_in_the_text_marks_nothing_rather_than_panicking() {
        assert!(char_marks("short", &(40..50)).is_empty());
    }

    #[test]
    fn the_preview_scrolls_so_the_match_sits_a_third_of_the_way_down() {
        // A row 17 points tall with 6 points between rows costs 23, and the pane is 300 tall. Line
        // 1770 therefore starts 1769 rows down, less the hundred points that are left above it.
        assert_eq!(scroll_to_line(1770, 23.0, 300.0), 1769.0 * 23.0 - 100.0);
    }

    #[test]
    fn a_match_near_the_top_of_a_file_leaves_the_preview_at_the_top() {
        assert_eq!(scroll_to_line(2, 23.0, 300.0), 0.0, "there is nothing above it to show");
        assert_eq!(scroll_to_line(0, 23.0, 300.0), 0.0, "and no line zero to scroll to");
    }

    #[test]
    fn the_spacing_between_rows_is_part_of_what_a_row_costs() {
        // The fault this is about: the offset was worked out from the height of a row alone, and
        // `ScrollArea::show_rows` adds `item_spacing.y` on top of it, so the preview landed about a
        // quarter of the way short of the match.
        let short = scroll_to_line(1770, 17.0, 300.0);
        let right = scroll_to_line(1770, 23.0, 300.0);
        assert!(short < right * 0.8, "{short} against {right}");
    }
}
