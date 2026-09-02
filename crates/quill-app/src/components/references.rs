//! `Find References`, the candidate list when a name has several definitions, and `Rename Symbol`.
//!
//! One modal for three questions, because they are three faces of one: *where is this name used*.
//! What the ticket asks for is "a modal that has the file path, then under that scrolled to the
//! first reference in that file, with the reference highlight", which is `Find in Files`' exact
//! anatomy — results above, a preview of the chosen file below, scrolled to the match, the match
//! picked out. So it is built from the same parts: `components::modal` for the frame, the dragging
//! and the resizing, `components::splitter` for the divider, and the same streamed results from
//! `services::text_search`, in its whole-word mode.
//!
//! Four things are different from `Find in Files`, and each is the answer to something a reference
//! list has to do that a text search does not.
//!
//! **The results are grouped by file.** A header row with the path and a count, then that file's
//! references beneath it. Every editor surveyed groups this list; a flat list makes twenty
//! references in one file read as twenty places.
//!
//! **Comments and strings come second.** Within each file the code references are listed first and
//! the textual ones after them, in the quiet colour and suffixed `· comment` or `· string`. Shown,
//! because a rename that must update a doc comment needs to find it; second-class, because they are
//! textual matches and the modal does not pretend otherwise.
//!
//! **There is no query field.** The question was asked by the click. The header says
//! `References to 'name'` and the footer says what the search said, including the cap when it was
//! hit.
//!
//! **In rename, the list *is* the change set.** A tick box on every row, and what is applied is
//! exactly the ticked rows. IntelliJ reaches its preview through a dialog and VS Code hides it
//! behind `Shift+Enter`; here the preview is the interface, because on a syntactic tier the
//! person's confirmation is the correctness mechanism, and a preview that can be skipped is a
//! preview that will be.

use std::ops::Range;
use std::path::{Path, PathBuf};

use egui::{Pos2, Rect, Vec2};

use quill_core::symbols::Role;

use crate::components::{controls, modal, splitter};
use crate::services::plugins::Grammars;
use crate::services::text_search::{self, Hit, Query, Searcher};
use crate::theme::{color, size};

/// How large the modal is before anything has been dragged.
const WIDTH: f32 = 980.0;
const HEIGHT: f32 = 680.0;
/// How much of the modal the results take before the divider is dragged, and the least and most
/// they can be dragged to.
pub const SPLIT: f32 = 0.5;
pub const SPLIT_MIN: f32 = 0.15;
pub const SPLIT_MAX: f32 = 0.85;
/// The largest file the preview reads, as in `Find in Files`: reading happens where the window
/// draws, and a file larger than this is not one anybody is reading in a preview pane.
const PREVIEW_LIMIT: u64 = 4 * 1024 * 1024;

/// Which of the three questions this modal is answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Every place a name is used.
    References,
    /// Which of several definitions was meant. The same furniture, because a picker for "which
    /// `new` did you mean" and a reference list are the same thing.
    Definitions,
    /// The change set of a rename, with a tick box on every row.
    Rename,
}

impl Purpose {
    fn title(self, name: &str) -> String {
        match self {
            Purpose::References => format!("References to '{name}'"),
            Purpose::Definitions => format!("Definitions of '{name}'"),
            Purpose::Rename => format!("Rename '{name}'"),
        }
    }
}

/// One row in the list: a file's heading, or one reference under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A file, and how many references are in it.
    File { path: PathBuf, count: usize },
    /// One reference, as an index into the hits.
    Reference(usize),
}

/// The file under the results, once it has been read.
#[derive(Debug, Default)]
struct Preview {
    path: PathBuf,
    lines: Vec<String>,
    /// Why the file could not be read, when it could not be.
    problem: Option<String>,
}

/// What the modal is showing and what has been chosen in it.
pub struct References {
    pub purpose: Purpose,
    /// The name the question is about.
    pub name: String,
    /// What it is being renamed to, which starts as the name itself with the whole of it selected
    /// so that typing replaces it.
    pub new_name: String,
    /// Which row is chosen, as an index into [`Self::rows`].
    pub chosen: usize,
    hits: Vec<Hit>,
    /// One per hit, in the same order: whether that row will be changed by the rename.
    ticked: Vec<bool>,
    rows: Vec<Row>,
    files: usize,
    searching: bool,
    capped: bool,
    asked: Option<Query>,
    searcher: Option<Searcher>,
    preview: Option<Preview>,
    /// Set when the choice moved, so the list and the preview scroll to it.
    follow: bool,
    /// What the preview was last scrolled to, so it is scrolled again when the choice really
    /// changes and not on every frame. The semantics `find_in_files` earned the hard way carry over
    /// verbatim: the scroll must not be spent before the first result exists.
    followed: Option<(PathBuf, usize)>,
    /// Why the rename cannot be applied, when it cannot.
    pub refusal: Option<String>,
    /// A collision the person should see but which does not stop them.
    pub warning: Option<String>,
    /// Set on the frame the field should take the keyboard, which is the frame it opens on.
    focus_the_field: bool,
}

impl References {
    /// Open the modal on a name, with the thread it searches on.
    ///
    /// The thread lives as long as this struct: shutting the modal drops it, the request channel
    /// closes, and the thread ends — the same arrangement `Find in Files` has.
    pub fn open(
        purpose: Purpose,
        name: &str,
        wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            purpose,
            name: name.to_owned(),
            new_name: name.to_owned(),
            chosen: 0,
            hits: Vec::new(),
            ticked: Vec::new(),
            rows: Vec::new(),
            files: 0,
            searching: true,
            capped: false,
            asked: None,
            searcher: Some(Searcher::start(wake)),
            preview: None,
            follow: false,
            followed: None,
            refusal: None,
            warning: None,
            focus_the_field: purpose == Purpose::Rename,
        }
    }

    /// A modal showing a list that was worked out rather than searched for: the candidates when a
    /// name has several definitions.
    ///
    /// No thread, because there is nothing to search: the answer was in the index already.
    pub fn candidates(name: &str, hits: Vec<Hit>) -> Self {
        let mut modal = Self {
            purpose: Purpose::Definitions,
            name: name.to_owned(),
            new_name: name.to_owned(),
            chosen: 0,
            hits,
            ticked: Vec::new(),
            rows: Vec::new(),
            files: 0,
            searching: false,
            capped: false,
            asked: None,
            searcher: None,
            preview: None,
            follow: false,
            followed: None,
            refusal: None,
            warning: None,
            focus_the_field: false,
        };
        modal.files = modal.file_count();
        modal.rebuild_rows();
        modal
    }

    /// Ask the thread for the references if it has not been asked, and take in what it has answered.
    ///
    /// Called by the window each frame before the modal is drawn, because the project's file list,
    /// its grammars and the text of the open tabs all belong to the window.
    pub fn pump(
        &mut self,
        files: &[PathBuf],
        grammars: std::sync::Arc<Grammars>,
        open: std::sync::Arc<Vec<(PathBuf, String)>>,
    ) {
        let Some(searcher) = self.searcher.as_mut() else {
            self.read_the_preview();
            self.settle_the_follow();
            return;
        };
        let wanted = Query { needle: self.name.clone(), match_case: true, words: true };
        if self.asked.as_ref() != Some(&wanted) {
            searcher.ask(files.to_vec(), wanted.clone(), grammars, open);
            self.asked = Some(wanted);
            self.hits.clear();
            self.ticked.clear();
            self.rows.clear();
            self.files = 0;
            self.chosen = 0;
            self.capped = false;
            self.searching = true;
            self.preview = None;
        }
        let mut arrived = false;
        for reply in searcher.poll() {
            self.hits.extend(reply.hits);
            self.capped |= reply.capped;
            arrived = true;
            if reply.done {
                self.searching = false;
            }
        }
        if arrived {
            self.files = self.file_count();
            self.rebuild_rows();
        }
        self.read_the_preview();
        self.settle_the_follow();
    }

    /// Whenever the row being looked at changes — because the arrow keys moved, because a row was
    /// clicked, or because the first answer has just arrived — both panes scroll to it.
    fn settle_the_follow(&mut self) {
        let target = self.chosen_hit().map(|hit| (hit.path.clone(), hit.line));
        if target != self.followed {
            self.followed = target;
            self.follow = true;
        }
    }

    /// How many files hold a reference.
    fn file_count(&self) -> usize {
        let mut seen: Vec<&Path> = Vec::new();
        for hit in &self.hits {
            if !seen.contains(&hit.path.as_path()) {
                seen.push(hit.path.as_path());
            }
        }
        seen.len()
    }

    /// Build the rows from the hits: a header for each file, then that file's references under it,
    /// code first and the textual ones after.
    ///
    /// Rebuilt whenever results arrive rather than kept up to date in pieces, because the results
    /// stream and a list assembled incrementally would put the second file's header in the middle
    /// of the first file's rows the moment two files answered out of order.
    fn rebuild_rows(&mut self) {
        let chosen_hit = self.chosen_hit_index();
        let mut order: Vec<PathBuf> = Vec::new();
        for hit in &self.hits {
            if !order.contains(&hit.path) {
                order.push(hit.path.clone());
            }
        }
        let mut rows: Vec<Row> = Vec::new();
        for path in order {
            let mut code: Vec<usize> = Vec::new();
            let mut quiet: Vec<usize> = Vec::new();
            for (index, hit) in self.hits.iter().enumerate() {
                if hit.path != path {
                    continue;
                }
                match hit.role {
                    Role::Code => code.push(index),
                    _ => quiet.push(index),
                }
            }
            rows.push(Row::File { path, count: code.len() + quiet.len() });
            rows.extend(code.into_iter().chain(quiet).map(Row::Reference));
        }
        self.rows = rows;
        self.ticked.resize(self.hits.len(), false);
        // Keep the reference that was chosen chosen, so results arriving underneath a person who
        // has already moved the selection do not move it back to the top.
        if let Some(hit) = chosen_hit {
            if let Some(at) = self.rows.iter().position(|row| *row == Row::Reference(hit)) {
                self.chosen = at;
                return;
            }
        }
        if self.chosen >= self.rows.len() {
            self.chosen = 0;
        }
    }

    /// Tick the rows a rename should change by default, given what the window worked out about each
    /// one. `ticks` is one per hit, in the same order.
    pub fn set_ticks(&mut self, ticks: Vec<bool>) {
        self.ticked = ticks;
        self.ticked.resize(self.hits.len(), false);
    }

    /// Everything found so far, for the window and for a test.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Which rows are ticked, one per hit.
    pub fn ticks(&self) -> &[bool] {
        &self.ticked
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The reference the chosen row is about: the row itself, or a file header's first reference,
    /// which is the ticket's own sentence implemented literally.
    pub fn chosen_hit(&self) -> Option<&Hit> {
        self.chosen_hit_index().and_then(|index| self.hits.get(index))
    }

    fn chosen_hit_index(&self) -> Option<usize> {
        match self.rows.get(self.chosen)? {
            Row::Reference(index) => Some(*index),
            Row::File { path, .. } => self.rows.iter().find_map(|row| match row {
                Row::Reference(index) if self.hits[*index].path == *path => Some(*index),
                _ => None,
            }),
        }
    }

    /// True while the thread is still reading the project.
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    /// True when the search stopped at the cap rather than at the end of the project.
    pub fn is_capped(&self) -> bool {
        self.capped
    }

    /// What the preview has been scrolled to, which is what a test asks to check that the preview
    /// really followed the choice rather than sitting at the top of the file.
    pub fn scrolled_to(&self) -> Option<(&Path, usize)> {
        self.followed.as_ref().map(|(path, line)| (path.as_path(), *line))
    }

    /// The change a rename would apply: the ticked rows, by file.
    pub fn change(&self) -> Vec<(PathBuf, Vec<Range<usize>>)> {
        let mut by_file: Vec<(PathBuf, Vec<Range<usize>>)> = Vec::new();
        for (index, hit) in self.hits.iter().enumerate() {
            if !self.ticked.get(index).copied().unwrap_or(false) {
                continue;
            }
            match by_file.iter_mut().find(|(path, _)| *path == hit.path) {
                Some((_, ranges)) => ranges.push(hit.offset.clone()),
                None => by_file.push((hit.path.clone(), vec![hit.offset.clone()])),
            }
        }
        by_file
    }

    /// How many rows a rename would change.
    pub fn ticked_count(&self) -> usize {
        self.ticked.iter().filter(|ticked| **ticked).count()
    }

    /// Whether the rename can be applied: a name this language could hold, and at least one row.
    fn can_rename(&self) -> bool {
        self.refusal.is_none() && self.ticked_count() > 0 && self.new_name != self.name
    }

    fn move_choice(&mut self, by: i32) {
        if self.rows.is_empty() {
            return;
        }
        // The arrow keys walk references and step over the file headings, which are a label rather
        // than somewhere to be.
        let last = self.rows.len() as i32 - 1;
        let mut at = self.chosen as i32;
        for _ in 0..self.rows.len() {
            at = (at + by).clamp(0, last);
            if matches!(self.rows[at as usize], Row::Reference(_)) {
                self.chosen = at as usize;
                return;
            }
            if at == 0 || at == last {
                break;
            }
        }
        self.chosen = at.clamp(0, last) as usize;
    }

    /// Read the file the chosen row is in, if it is not the one already read.
    fn read_the_preview(&mut self) {
        let Some(hit) = self.chosen_hit() else {
            self.preview = None;
            return;
        };
        if self.preview.as_ref().is_some_and(|preview| preview.path == hit.path) {
            return;
        }
        let path = hit.path.clone();
        let too_large =
            std::fs::metadata(&path).map(|meta| meta.len() > PREVIEW_LIMIT).unwrap_or(false);
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
                    lines: text
                        .split('\n')
                        .map(|line| line.trim_end_matches('\r').to_owned())
                        .collect(),
                    problem: None,
                },
                Err(problem) => {
                    Preview { path, lines: Vec::new(), problem: Some(problem.to_string()) }
                }
            }
        };
        self.preview = Some(preview);
    }

    /// What the footer says: how much was found, or what is wrong with the new name.
    fn summary(&self) -> String {
        if let Some(refusal) = &self.refusal {
            return refusal.clone();
        }
        if let Some(warning) = &self.warning {
            return warning.clone();
        }
        if self.capped {
            return format!(
                "The first {} \u{00B7} there are more",
                text_search::LIMIT
            );
        }
        let found = match self.hits.len() {
            0 if self.searching => return "Searching...".to_owned(),
            0 => return format!("Nothing in this project uses '{}'", self.name),
            1 => "1 reference".to_owned(),
            many => format!("{many} references"),
        };
        let mut text = format!("{found} in {} files", self.files);
        if self.purpose == Purpose::Rename {
            text = format!("{} of {found} will change in {} files", self.ticked_count(), self.files);
        }
        if self.searching {
            text.push_str(" \u{00B7} searching");
        }
        text
    }
}

/// What the modal asked for this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Open this file with this range of it selected, and shut the modal.
    pub open: Option<(PathBuf, Range<usize>)>,
    /// Shut the modal.
    pub close: bool,
    /// Apply the rename.
    pub rename: bool,
    /// How far the divider between the results and the preview was dragged, in points.
    pub drag: f32,
    /// The divider was double clicked, so the split goes back to its usual place.
    pub reset_split: bool,
    /// How tall the two panes are together, so the caller can turn a drag in points into the
    /// fraction it holds.
    pub panes_height: f32,
}

/// Draw the modal. `split` is how much of it the results take, which the window owns because it is
/// written to the settings file like every other pane.
pub fn show(ctx: &egui::Context, state: &mut References, split: f32) -> Outcome {
    let mut outcome = Outcome::default();
    let (_, closed) = modal::show(ctx, "quill-references", WIDTH, HEIGHT, |ui, area| {
        if modal::header(ui, area, &state.purpose.title(&state.name)) {
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
        // The rename's field sits above the list, pre-filled with the name it is about. The other
        // two questions were asked by the click and have nothing to type.
        let panes = if state.purpose == Purpose::Rename {
            let field = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
            let entry = modal::field(ui, field, "New name", &mut state.new_name);
            if state.focus_the_field {
                entry.request_focus();
                // The whole name selected, so typing replaces it rather than appending to it.
                select_the_whole_field(ui, entry.id);
                state.focus_the_field = false;
            }
            Rect::from_min_max(Pos2::new(body.left(), field.bottom() + 12.0), body.max)
        } else {
            body
        };

        outcome.panes_height = panes.height();
        let split = split.clamp(SPLIT_MIN, SPLIT_MAX);
        let results_height = (panes.height() * split).floor();
        let results = Rect::from_min_size(panes.min, Vec2::new(panes.width(), results_height));
        let preview =
            Rect::from_min_max(Pos2::new(panes.left(), results.bottom() + 9.0), panes.max);

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
        let drag = splitter::show(ui, line, "reference results", splitter::Axis::Flat);
        outcome.drag = drag.delta;
        outcome.reset_split = drag.reset;

        if enter {
            match state.purpose {
                Purpose::Rename if state.can_rename() => outcome.rename = true,
                Purpose::Rename => {}
                _ => {
                    outcome.open =
                        state.chosen_hit().map(|hit| (hit.path.clone(), hit.offset.clone()));
                }
            }
        }

        let summary = state.summary();
        let tint = match (&state.refusal, &state.warning) {
            (Some(_), _) => color::close(),
            (None, Some(_)) => color::git_modified(),
            _ => color::text_faint(),
        };
        modal::label(
            &ui.painter_at(area),
            Rect::from_min_size(
                Pos2::new(area.left() + 20.0, area.bottom() - modal::FOOTER),
                Vec2::new(area.width() - 260.0, modal::FOOTER),
            ),
            area.left() + 20.0,
            &summary,
            tint,
            11.0,
        );
        let buttons: Vec<(&str, bool)> = match state.purpose {
            Purpose::Rename => vec![("RENAME", state.can_rename())],
            _ => vec![("OPEN", state.chosen_hit().is_some())],
        };
        if modal::footer(ui, area, &buttons) == Some(0) {
            match state.purpose {
                Purpose::Rename => outcome.rename = true,
                _ => {
                    outcome.open =
                        state.chosen_hit().map(|hit| (hit.path.clone(), hit.offset.clone()));
                }
            }
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

/// Put the caret round the whole of a field's text, so the first thing typed replaces it.
fn select_the_whole_field(ui: &egui::Ui, id: egui::Id) {
    let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) else {
        return;
    };
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(usize::MAX),
    );
    state.cursor.set_char_range(Some(range));
    state.store(ui.ctx(), id);
}

/// The results: a heading for each file and one row for each reference in it. Returns the one that
/// was double clicked.
fn rows(ui: &mut egui::Ui, area: Rect, state: &mut References) -> Option<(PathBuf, Range<usize>)> {
    frame(ui, area);
    let mut opened = None;
    let mut chose = None;
    let mut toggled = None;
    let mut toggled_file = None;
    let mut follow_to = None;
    let inner = area.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    let renaming = state.purpose == Purpose::Rename;
    egui::ScrollArea::vertical().id_salt("reference-rows").show(&mut child, |ui| {
        if state.rows.is_empty() {
            ui.add_space(8.0);
            let words = if state.searching {
                "  Searching...".to_owned()
            } else {
                format!("  Nothing in this project uses '{}'", state.name)
            };
            ui.label(egui::RichText::new(words).size(11.5).color(color::text_faint()));
            return;
        }
        for (index, row) in state.rows.iter().enumerate() {
            let chosen = index == state.chosen;
            let response = match row {
                Row::File { path, count } => {
                    let shown = short_path(path);
                    // A heading names itself by the file, which is unique in this list.
                    let label = format!("References in {shown}");
                    // Three states rather than two, because a file with one of its three rows
                    // ticked is neither ticked nor empty, and a box that said "empty" there would
                    // be saying something untrue about the change set below it.
                    let (ticked_here, rows_here) = state
                        .hits
                        .iter()
                        .enumerate()
                        .filter(|(_, hit)| hit.path == *path)
                        .fold((0, 0), |(ticked, rows), (at, _)| {
                            let on = usize::from(state.ticked.get(at).copied().unwrap_or(false));
                            (ticked + on, rows + 1)
                        });
                    let all_ticked = renaming && ticked_here == rows_here;
                    let some_ticked = renaming && ticked_here > 0 && ticked_here < rows_here;
                    let response = modal::row(ui, index, &label, chosen, |painter, rect| {
                        let mut x = rect.left() + 16.0;
                        if renaming {
                            match some_ticked {
                                true => part_tick(painter, rect, x),
                                false => tick(painter, rect, x, all_ticked),
                            }
                            x += 24.0;
                        }
                        let x = modal::label(
                            painter,
                            rect,
                            x,
                            &shown,
                            color::text_strong(),
                            11.5,
                        );
                        modal::label(
                            painter,
                            rect,
                            x + 10.0,
                            &format!("\u{00B7} {count}"),
                            color::text_faint(),
                            11.0,
                        );
                    });
                    if renaming && response.clicked() && on_the_tick(&response) {
                        toggled_file = Some((path.clone(), !all_ticked));
                    }
                    response
                }
                Row::Reference(hit_index) => {
                    let hit = &state.hits[*hit_index];
                    let name = hit
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let where_it_is = format!("{name}:{}", hit.line);
                    let label = format!("Reference {where_it_is}");
                    let (text, marks) = picked_out(&hit.text, &hit.range);
                    let quiet = hit.role != Role::Code;
                    let ticked = state.ticked.get(*hit_index).copied().unwrap_or(false);
                    let response = modal::row(ui, index, &label, chosen, |painter, rect| {
                        let mut x = rect.left() + 28.0;
                        if renaming {
                            tick(painter, rect, x, ticked);
                            x += 24.0;
                        }
                        let tint = match (chosen, quiet) {
                            (_, true) => color::text_faint(),
                            (true, false) => color::text_strong(),
                            (false, false) => color::text_dim(),
                        };
                        let x = modal::label(painter, rect, x, &format!("{}", hit.line), tint, 11.0);
                        let galley = controls::marked_text(
                            painter,
                            &text,
                            &marks,
                            if quiet { color::text_faint() } else { color::text_control() },
                            egui::FontId::monospace(11.5),
                        );
                        let width = galley.size().x;
                        painter.galley(
                            Pos2::new(x + 14.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            if quiet { color::text_faint() } else { color::text_control() },
                        );
                        // The suffix that says a match is textual rather than code, which is the
                        // whole of what makes it second-class rather than hidden.
                        if quiet {
                            modal::label(
                                painter,
                                rect,
                                x + 14.0 + width + 12.0,
                                &format!("\u{00B7} {}", hit.role.suffix()),
                                color::text_faint(),
                                10.5,
                            );
                        }
                    });
                    if renaming && response.clicked() && on_the_tick(&response) {
                        toggled = Some(*hit_index);
                    }
                    if response.double_clicked() {
                        opened = Some((hit.path.clone(), hit.offset.clone()));
                    }
                    response
                }
            };
            if response.clicked() {
                chose = Some(index);
            }
            if chosen && state.follow {
                follow_to = Some(response.rect);
            }
        }
    });
    if let Some(index) = chose {
        state.chosen = index;
    }
    if let Some(index) = toggled {
        if let Some(ticked) = state.ticked.get_mut(index) {
            *ticked = !*ticked;
        }
    }
    if let Some((path, on)) = toggled_file {
        for (index, hit) in state.hits.iter().enumerate() {
            if hit.path == path {
                if let Some(ticked) = state.ticked.get_mut(index) {
                    *ticked = on;
                }
            }
        }
    }
    if let Some(rect) = follow_to {
        child.scroll_to_rect(rect, None);
    }
    opened
}

/// Whether a click landed on the tick box at the left of a row rather than on the row itself.
///
/// A row is chosen by clicking it and its box is ticked by clicking the box, which is what every
/// list with tick boxes in it does. Without this a person choosing a row to look at would tick it.
fn on_the_tick(response: &egui::Response) -> bool {
    response
        .interact_pointer_pos()
        .is_some_and(|at| at.x < response.rect.left() + 52.0)
}

/// One tick box, drawn the way `modal::check` draws one, at a position rather than in a layout.
fn tick(painter: &egui::Painter, row: Rect, x: f32, on: bool) {
    let box_size = 14.0;
    let rect = Rect::from_min_size(
        Pos2::new(x, row.center().y - box_size / 2.0),
        Vec2::splat(box_size),
    );
    painter.rect(
        rect,
        egui::CornerRadius::same(3),
        if on { color::accent() } else { egui::Color32::TRANSPARENT },
        egui::Stroke::new(1.0, if on { color::accent() } else { color::control_border() }),
        egui::StrokeKind::Inside,
    );
    if on {
        let middle = rect.center();
        painter.line_segment(
            [
                Pos2::new(middle.x - 3.5, middle.y),
                Pos2::new(middle.x - 1.0, middle.y + 2.5),
            ],
            egui::Stroke::new(1.6, color::editor()),
        );
        painter.line_segment(
            [
                Pos2::new(middle.x - 1.0, middle.y + 2.5),
                Pos2::new(middle.x + 3.5, middle.y - 2.5),
            ],
            egui::Stroke::new(1.6, color::editor()),
        );
    }
}

/// A tick box saying that some of the rows under it are ticked and some are not: a dash rather than
/// a tick, which is what every list with a mixed group in it draws.
fn part_tick(painter: &egui::Painter, row: Rect, x: f32) {
    let box_size = 14.0;
    let rect = Rect::from_min_size(
        Pos2::new(x, row.center().y - box_size / 2.0),
        Vec2::splat(box_size),
    );
    painter.rect(
        rect,
        egui::CornerRadius::same(3),
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(1.0, color::accent()),
        egui::StrokeKind::Inside,
    );
    let middle = rect.center();
    painter.line_segment(
        [Pos2::new(middle.x - 3.5, middle.y), Pos2::new(middle.x + 3.5, middle.y)],
        egui::Stroke::new(1.6, color::accent()),
    );
}

/// The chosen file, with its path at the top and the chosen reference picked out.
///
/// The path is drawn in the preview itself rather than only in the list, so the answer to "which
/// file am I looking at" never depends on where the list happens to be scrolled — which is the
/// ticket's own sentence: *a modal that has the file path, then under that scrolled to the first
/// reference in that file*.
fn show_preview(ui: &mut egui::Ui, area: Rect, state: &mut References) {
    frame(ui, area);
    let inner = area.shrink(2.0);
    let Some(preview) = &state.preview else {
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        ui.set_clip_rect(inner);
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("  Choose a reference to see the file it is in")
                .size(11.5)
                .color(color::text_faint()),
        );
        return;
    };
    // The path along the top of the pane, on a line of its own.
    let heading = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 22.0));
    let painter = ui.painter_at(heading);
    painter.rect_filled(heading, egui::CornerRadius::same(3), color::control());
    modal::label(
        &painter,
        heading,
        heading.left() + 8.0,
        &short_path(&preview.path),
        color::text_strong(),
        11.0,
    );
    let below = Rect::from_min_max(Pos2::new(inner.left(), heading.bottom() + 2.0), inner.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(below));
    child.set_clip_rect(below);
    if let Some(problem) = &preview.problem {
        let mut ui = child;
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("  {problem}")).size(11.5).color(color::close()));
        return;
    }
    let hit = state.chosen_hit();
    let matched_line = hit.map(|hit| hit.line).unwrap_or(0);
    let range = hit.map(|hit| hit.range.clone()).unwrap_or(0..0);
    let row_height = child.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    // What one row really costs: `ScrollArea::show_rows` adds `item_spacing.y` itself, and an offset
    // worked out from the row height alone lands about a quarter of the way short. `find_in_files`
    // found that by looking at the real window, and it is the same arithmetic here.
    let pitch = row_height + child.spacing().item_spacing.y;
    let mut scroll = egui::ScrollArea::both().id_salt("reference-preview");
    if state.follow {
        scroll = scroll.vertical_scroll_offset(scroll_to_line(matched_line, pitch, below.height()));
        state.follow = false;
    }
    let lines = preview.lines.clone();
    scroll.show_rows(&mut child, row_height, lines.len(), |ui, shown| {
        for index in shown {
            let number = index + 1;
            let line = &lines[index];
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width().max(1.0), row_height),
                egui::Sense::hover(),
            );
            let painter = ui.painter();
            let on_the_match = number == matched_line;
            if on_the_match {
                painter.rect_filled(rect, egui::CornerRadius::same(3), color::selected_row());
            }
            let tint = if on_the_match { color::text_strong() } else { color::text_dim() };
            modal::label(
                painter,
                rect,
                rect.left() + 6.0,
                &format!("{number:>5}"),
                color::text_faint(),
                10.5,
            );
            let marks = if on_the_match { char_marks(line, &range) } else { Vec::new() };
            let galley =
                controls::marked_text(painter, line, &marks, tint, egui::FontId::monospace(11.5));
            painter.galley(
                Pos2::new(rect.left() + 52.0, rect.center().y - galley.size().y / 2.0),
                galley,
                tint,
            );
        }
    });
}

/// How far the preview is scrolled so that `line` sits a third of the way down a pane `height`
/// tall, spacing included. The same function `Find in Files` uses, for the same reason.
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

/// A path as a heading shows it: the last two parts of it, which is enough to tell two files of the
/// same name apart without a row a person has to read sideways.
fn short_path(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect();
    let kept = parts.len().min(2);
    parts[parts.len() - kept..].join(std::path::MAIN_SEPARATOR_STR)
}

/// A matching line trimmed of the indent in front of it, and where the match is in what is left.
fn picked_out(text: &str, range: &Range<usize>) -> (String, Vec<usize>) {
    let indent = text.len() - text.trim_start().len();
    let indent = indent.min(range.start);
    let trimmed = text[indent..].to_owned();
    let moved = (range.start - indent)..(range.end - indent);
    let marks = char_marks(&trimmed, &moved);
    (trimmed, marks)
}

/// A byte range turned into the character positions inside it, which is what a marked galley wants.
fn char_marks(text: &str, range: &Range<usize>) -> Vec<usize> {
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

    fn hit(path: &str, line: usize, text: &str, role: Role) -> Hit {
        Hit {
            path: PathBuf::from(path),
            line,
            text: text.to_owned(),
            range: 0..4,
            offset: (line * 100)..(line * 100 + 4),
            role,
        }
    }

    fn modal_with(hits: Vec<Hit>) -> References {
        let mut modal = References::candidates("draw", hits);
        modal.purpose = Purpose::References;
        modal
    }

    #[test]
    fn the_results_are_grouped_by_file_with_a_count_on_each_heading() {
        // Scenario 20. A flat list makes twenty references in one file read as twenty places.
        let modal = modal_with(vec![
            hit("a/one.rs", 3, "draw();", Role::Code),
            hit("a/one.rs", 9, "draw();", Role::Code),
            hit("b/two.rs", 1, "draw();", Role::Code),
        ]);
        let rows = modal.rows();
        assert_eq!(rows.len(), 5, "two headings and three references: {rows:?}");
        assert_eq!(rows[0], Row::File { path: PathBuf::from("a/one.rs"), count: 2 });
        assert!(matches!(rows[1], Row::Reference(0)));
        assert!(matches!(rows[2], Row::Reference(1)));
        assert_eq!(rows[3], Row::File { path: PathBuf::from("b/two.rs"), count: 1 });
    }

    #[test]
    fn comment_and_string_references_come_after_the_code_ones_in_their_own_file() {
        // Scenario 24: shown, because a rename that must update a doc comment needs to find them;
        // second-class, because they are textual matches.
        let modal = modal_with(vec![
            hit("one.rs", 1, "// draw it", Role::Comment),
            hit("one.rs", 4, "draw();", Role::Code),
            hit("one.rs", 7, "\"draw\"", Role::String),
        ]);
        let order: Vec<usize> = modal
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Reference(index) => Some(*index),
                Row::File { .. } => None,
            })
            .collect();
        assert_eq!(order, vec![1, 0, 2], "the code one first, then the two textual ones");
    }

    #[test]
    fn choosing_a_file_heading_previews_that_files_first_reference() {
        // The ticket's own sentence, implemented literally.
        let mut modal = modal_with(vec![
            hit("a/one.rs", 3, "draw();", Role::Code),
            hit("a/one.rs", 9, "draw();", Role::Code),
            hit("b/two.rs", 12, "draw();", Role::Code),
        ]);
        modal.chosen = 3; // the heading of the second file
        assert_eq!(modal.chosen_hit().expect("a reference").line, 12);
        modal.chosen = 0; // the heading of the first
        assert_eq!(modal.chosen_hit().expect("a reference").line, 3);
        modal.chosen = 2; // a reference row of its own
        assert_eq!(modal.chosen_hit().expect("a reference").line, 9);
    }

    #[test]
    fn the_arrow_keys_walk_references_and_step_over_the_headings() {
        let mut modal = modal_with(vec![
            hit("a/one.rs", 3, "draw();", Role::Code),
            hit("b/two.rs", 12, "draw();", Role::Code),
        ]);
        modal.chosen = 0;
        modal.move_choice(1);
        assert!(matches!(modal.rows()[modal.chosen], Row::Reference(0)));
        modal.move_choice(1);
        assert!(
            matches!(modal.rows()[modal.chosen], Row::Reference(1)),
            "the second file's heading is stepped over: {:?}",
            modal.rows()[modal.chosen]
        );
        modal.move_choice(-1);
        assert!(matches!(modal.rows()[modal.chosen], Row::Reference(0)));
    }

    #[test]
    fn a_rename_changes_exactly_the_ticked_rows() {
        // Scenario 37, at the modal's own end: what is applied is the tick boxes and nothing else.
        let mut modal = modal_with(vec![
            hit("one.rs", 1, "draw();", Role::Code),
            hit("one.rs", 4, "draw();", Role::Code),
            hit("two.rs", 2, "draw();", Role::Code),
        ]);
        modal.purpose = Purpose::Rename;
        modal.set_ticks(vec![true, false, true]);
        let change = modal.change();
        assert_eq!(change.len(), 2, "two files hold a ticked row");
        assert_eq!(change[0].1, vec![100..104], "only the first row of one.rs");
        assert_eq!(change[1].1, vec![200..204]);
        assert_eq!(modal.ticked_count(), 2);
    }

    #[test]
    fn a_rename_with_nothing_ticked_or_a_name_that_is_refused_cannot_be_applied() {
        // Scenarios 38 and 47.
        let mut modal = modal_with(vec![hit("one.rs", 1, "draw();", Role::Code)]);
        modal.purpose = Purpose::Rename;
        modal.new_name = "paint".to_owned();
        assert!(!modal.can_rename(), "nothing is ticked yet");
        modal.set_ticks(vec![true]);
        assert!(modal.can_rename());
        modal.refusal = Some("'match' is a Rust keyword.".to_owned());
        assert!(!modal.can_rename());
        modal.refusal = None;
        modal.new_name = "draw".to_owned();
        assert!(!modal.can_rename(), "renaming a name to itself changes nothing");
    }

    #[test]
    fn the_footer_says_what_the_search_said_including_the_cap() {
        let mut modal = modal_with(vec![
            hit("a/one.rs", 3, "draw();", Role::Code),
            hit("b/two.rs", 1, "draw();", Role::Code),
        ]);
        assert_eq!(modal.summary(), "2 references in 2 files");
        modal.capped = true;
        assert!(modal.summary().contains("there are more"), "{}", modal.summary());
        modal.capped = false;
        modal.refusal = Some("'match' is a Rust keyword.".to_owned());
        assert_eq!(modal.summary(), "'match' is a Rust keyword.");
    }

    #[test]
    fn a_heading_shows_enough_of_the_path_to_tell_two_files_of_one_name_apart() {
        assert_eq!(
            short_path(Path::new("src/services/file_marks.rs")),
            format!("services{}file_marks.rs", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(short_path(Path::new("main.rs")), "main.rs");
    }

    #[test]
    fn the_preview_scrolls_so_the_reference_sits_a_third_of_the_way_down() {
        assert_eq!(scroll_to_line(1770, 23.0, 300.0), 1769.0 * 23.0 - 100.0);
        assert_eq!(scroll_to_line(2, 23.0, 300.0), 0.0, "there is nothing above it to show");
    }

    #[test]
    fn the_indent_in_front_of_a_reference_is_trimmed_and_the_match_moves_with_it() {
        let (text, marks) = picked_out("        draw();", &(8..12));
        assert_eq!(text, "draw();");
        assert_eq!(marks, vec![0, 1, 2, 3]);
    }
}
