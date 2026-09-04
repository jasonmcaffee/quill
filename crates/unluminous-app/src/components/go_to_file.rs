//! `Go to File`: type part of a name, and open the file it finds.
//!
//! `task-1659` asks for the reference editor's, and this is the part of the reference editor's that is worth having: one
//! box, a list that narrows as you type, and a file opened from it. What is deliberately left out is
//! the rest of `Search Everywhere` — the tabs across the top for classes, symbols, actions and text.
//! Unluminous has no symbol index and its actions are on two short menus, so those tabs would be three
//! empty rooms, and `Find in Files` is a modal of its own with a preview in it rather than a tab
//! here.
//!
//! The matching is not here. `services::file_search` decides which files match and in what order,
//! so it can be tested without a window, and this file draws the list it produces.
//!
//! Opening is a **double click**, which is what the ticket asks for, or Enter on the row the arrow
//! keys are on. A single click chooses a row without opening it, so the list can be read through
//! with the mouse the way it can with the keyboard.

use egui::{Pos2, Rect, Vec2};

use crate::components::{controls, modal};
use crate::services::file_search::Found;
use crate::theme::color;

/// How large the modal is before anything has been dragged.
const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 480.0;
/// The most results that are listed. The same cap the explorer's filter uses, and for the same
/// reason: past a few hundred rows the list has stopped being an answer.
const LIMIT: usize = 300;

/// What has been typed into `Go to File` and which row is chosen.
///
/// The window holds one of these while the modal is open. The matches are worked out when the query
/// changes rather than on every frame, because a project can hold thousands of files and a frame is
/// sixteen milliseconds.
#[derive(Debug, Default)]
pub struct GoToFile {
    pub query: String,
    /// Which row is chosen, as a position in [`Self::results`].
    pub chosen: usize,
    results: Vec<Found>,
    /// The query the results were worked out for, so they are worked out again only when it changes.
    searched: Option<String>,
    /// Set when the keyboard moved the choice, so the list scrolls to keep it in view.
    follow: bool,
}

impl GoToFile {
    /// Work the matches out again if the query has changed since the last time.
    ///
    /// Called by the window before the modal is drawn, because the file list belongs to the window
    /// and a component is handed what it draws rather than reaching for it.
    pub fn refresh(&mut self, root: &std::path::Path, files: &[std::path::PathBuf]) {
        if self.searched.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.results = crate::services::file_search::find(root, files, &self.query, LIMIT);
        self.searched = Some(self.query.clone());
        self.chosen = 0;
        self.follow = true;
    }

    /// The files being offered, for a test that wants to know what the box found.
    pub fn results(&self) -> &[Found] {
        &self.results
    }

    /// The file the chosen row holds, if there is one.
    pub fn chosen_path(&self) -> Option<std::path::PathBuf> {
        self.results.get(self.chosen).map(|found| found.path.clone())
    }

    fn move_choice(&mut self, by: i32) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len() as i32 - 1;
        self.chosen = (self.chosen as i32 + by).clamp(0, last) as usize;
        self.follow = true;
    }
}

/// What `Go to File` asked for this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GoToFileOutcome {
    /// Open this file and shut the modal.
    pub open: Option<std::path::PathBuf>,
    /// Shut the modal without opening anything.
    pub close: bool,
}

/// Draw the modal. The window owns whether there is one at all.
pub fn show(ctx: &egui::Context, state: &mut GoToFile) -> GoToFileOutcome {
    let mut outcome = GoToFileOutcome::default();
    let (_, closed) = modal::show(ctx, "unluminous-go-to-file", WIDTH, HEIGHT, |ui, area| {
        if modal::header(ui, area, "Go to File") {
            outcome.close = true;
        }
        // The arrow keys and Enter are taken out of the frame's events before the field is drawn.
        // egui leaves the events a text box consumed in the list for everyone else to read, and the
        // list moving is not the same as a caret moving, so they are claimed here first.
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
        let field = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
        let entry = controls::search_field(ui, field, "Go to file", "Type a file name", &mut state.query);
        // The box has the keyboard from the moment the modal opens, because a search box that has to
        // be clicked before it can be typed into is a search box that gets typed past.
        if !entry.has_focus() {
            entry.request_focus();
        }

        let list = Rect::from_min_max(Pos2::new(body.left(), field.bottom() + 10.0), body.max);
        if let Some(path) = rows(ui, list, state) {
            outcome.open = Some(path);
        }
        if enter {
            outcome.open = state.chosen_path();
        }

        let count = state.results.len();
        let summary = match count {
            0 if state.query.trim().is_empty() => "No files in this project".to_owned(),
            0 => "No file matches".to_owned(),
            1 => "1 file".to_owned(),
            many => format!("{many} files"),
        };
        modal::label(
            &ui.painter_at(area),
            Rect::from_min_size(
                Pos2::new(area.left() + 20.0, area.bottom() - modal::FOOTER),
                Vec2::new(200.0, modal::FOOTER),
            ),
            area.left() + 20.0,
            &summary,
            color::text_faint(),
            11.0,
        );
        if modal::footer(ui, area, &[("OPEN", state.chosen_path().is_some())]) == Some(0) {
            outcome.open = state.chosen_path();
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

/// The list of files. Returns the one that was double clicked.
fn rows(ui: &mut egui::Ui, area: Rect, state: &mut GoToFile) -> Option<std::path::PathBuf> {
    let mut opened = None;
    let mut chose = None;
    let mut follow_to = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    egui::ScrollArea::vertical().id_salt("go-to-file-rows").show(&mut child, |ui| {
        if state.results.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  No file matches").size(11.5).color(color::text_faint()));
            return;
        }
        for (index, found) in state.results.iter().enumerate() {
            let chosen = index == state.chosen;
            let name = found.name.clone();
            let folder = found.folder.clone();
            let hits = found.hits.clone();
            let marker = crate::theme::file_marker(&found.path);
            // The row's name carries where the file is as well as what it is called, because the
            // explorer has a row called `readme.md` too and two controls with one name cannot be
            // told apart — by a person hearing them read out, or by a test asking for one.
            let label = if folder.is_empty() {
                format!("Go to {name}")
            } else {
                format!("Go to {folder}/{name}")
            };
            // A file Unluminous cannot open is listed and drawn dimmed, which is exactly what the
            // explorer does with one. A list that leaves them out would answer "no file matches" for
            // a file that is plainly there; opening one puts the reason in the status bar.
            let openable = crate::services::file_kind::is_openable(&found.path);
            let response = modal::row(ui, index, &label, chosen, |painter, row| {
                let square = Rect::from_center_size(
                    Pos2::new(row.left() + 20.0, row.center().y),
                    Vec2::splat(8.0),
                );
                painter.rect_filled(
                    square,
                    egui::CornerRadius::same(2),
                    if openable { marker } else { marker.gamma_multiply(0.5) },
                );
                let tint = match (openable, chosen) {
                    (false, _) => color::text_faint(),
                    (true, true) => color::text_strong(),
                    (true, false) => color::text_control(),
                };
                let galley = controls::marked_text(painter, &name, &hits, tint, egui::FontId::proportional(12.5));
                let width = galley.size().x;
                painter.galley(
                    Pos2::new(row.left() + 34.0, row.center().y - galley.size().y / 2.0),
                    galley,
                    tint,
                );
                if !folder.is_empty() {
                    modal::label(
                        painter,
                        row,
                        row.left() + 34.0 + width + 14.0,
                        &folder,
                        color::text_faint(),
                        11.0,
                    );
                }
            });
            if response.clicked() {
                chose = Some(index);
            }
            if response.double_clicked() {
                opened = Some(found.path.clone());
            }
            if chosen && state.follow {
                follow_to = Some(response.rect);
            }
        }
    });
    if let Some(index) = chose {
        state.chosen = index;
    }
    // Keeping the chosen row in view is only wanted when the keyboard moved the choice. Doing it
    // every frame would fight the wheel: the list would spring back the moment it was scrolled.
    if let Some(rect) = follow_to {
        child.scroll_to_rect(rect, None);
        state.follow = false;
    }
    opened
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn state_with(files: &[&str]) -> GoToFile {
        let mut state = GoToFile::default();
        let paths: Vec<PathBuf> = files.iter().map(|name| Path::new("/project").join(name)).collect();
        state.refresh(Path::new("/project"), &paths);
        state
    }

    #[test]
    fn the_list_is_worked_out_again_only_when_the_query_changes() {
        let mut state = state_with(&["readme.md", "notes.txt"]);
        assert_eq!(state.results().len(), 2);
        state.query = "read".to_owned();
        state.refresh(Path::new("/project"), &[PathBuf::from("/project/readme.md")]);
        assert_eq!(state.results().len(), 1);
        // Asked again with the same query, the file list is not looked at: an empty one changes
        // nothing, which is what says the work was skipped.
        state.refresh(Path::new("/project"), &[]);
        assert_eq!(state.results().len(), 1);
    }

    #[test]
    fn the_arrow_keys_walk_the_list_and_stop_at_its_ends() {
        let mut state = state_with(&["a.md", "b.md", "c.md"]);
        assert_eq!(state.chosen, 0);
        state.move_choice(-1);
        assert_eq!(state.chosen, 0, "there is nothing above the first row");
        state.move_choice(1);
        state.move_choice(1);
        state.move_choice(1);
        assert_eq!(state.chosen, 2, "and nothing below the last");
    }

    #[test]
    fn a_new_query_starts_at_the_top_of_the_list() {
        let mut state = state_with(&["a.md", "b.md"]);
        state.move_choice(1);
        assert_eq!(state.chosen, 1);
        state.query = "b".to_owned();
        state.refresh(Path::new("/project"), &[PathBuf::from("/project/b.md")]);
        assert_eq!(state.chosen, 0);
    }

    #[test]
    fn the_chosen_row_is_the_file_that_would_be_opened() {
        let mut state = state_with(&["a.md", "b.md"]);
        state.move_choice(1);
        assert_eq!(state.chosen_path(), Some(PathBuf::from("/project/b.md")));
    }

    #[test]
    fn nothing_is_chosen_when_nothing_matched() {
        let mut state = GoToFile { query: "zzz".to_owned(), ..GoToFile::default() };
        state.refresh(Path::new("/project"), &[PathBuf::from("/project/a.md")]);
        assert_eq!(state.chosen_path(), None);
    }
}
