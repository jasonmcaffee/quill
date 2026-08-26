//! The window's half of auto-complete: what is offered, when the popup is open, and what the five
//! keys mean while it is.
//!
//! `quill_core::completion` says what a stem is, what matches it and in what order;
//! `components::completion` draws the list; this is what sits between them. Nothing here draws and
//! nothing here decides what a match is.
//!
//! ## Everything it offers is already in memory
//!
//! There is no new index, no new thread and no watcher. The four sources are ones `task-1676`
//! already keeps fresh: this tab's definitions and its distinct words, cached on the tab and keyed
//! on `Document::text_revision()`; the other open tabs' definitions, the same; the project's
//! `services::symbol_index`, built on its own worker and generation-cancelled; and the file's
//! `Grammar`, which has been in memory since the plugin was read. Completion **reads** what is
//! there, which is what makes it a small feature where most editors' completion is an enormous one.
//!
//! The ownership rule of `task-1675` §3.3 carries over unchanged: *a file that is open is owned by
//! its `Document`, and every other file is owned by the index*. So the open files' paths are
//! dropped from what the index offers, or a name being edited in a tab would be offered twice —
//! once as it is now and once as the disk last saw it.
//!
//! ## And nothing here runs once a frame
//!
//! [`CompletionState`] carries the `text_revision` and the caret its rows were worked out at, and
//! [`QuillApp::keep_the_completion_fresh`] compares two integers before it does anything at all. A
//! caret blink, a repaint, a frame of idling: two comparisons and no allocation, which is
//! `task-1666`'s rule kept the way `symbols::Hover` already keeps it.
//!
//! ## The five keys, and why they are consumed
//!
//! `Up`, `Down`, `Tab`, `Enter` and `Escape` are removed from the frame's input with
//! `consume_key` **before** the panes are drawn, so they never reach
//! `editor_view::handle_input`. That is the one-frame ordering `Go to File` and `Find in Files`
//! already rely on. Everything else flows through untouched: letters keep typing and refiltering the
//! list, and the popup takes exactly five keys and only while it is open.

use std::ops::Range;
use std::path::{Path, PathBuf};

use quill_core::completion::{self, Candidate, Row, Source};
use quill_core::{Command, Grammar, Role};

use crate::app::{Focus, QuillApp};
use crate::components::completion as view;
use crate::services::file_kind;

/// How many rows are drawn before the list scrolls.
pub const VISIBLE_ROWS: usize = 8;

/// How much of a word has to be typed before the popup arrives unasked.
///
/// One character opens on nearly every letter of a file and matches most of it, which is noise; the
/// same argument in Helix's own thread talked its users down from seven to two or three. Two is not
/// a setting: `Ctrl+Space` covers the rest, and it works from one character.
pub const AUTOMATIC_STEM: usize = 2;

/// What is being offered, where, and which row is chosen.
///
/// One of these on the window at most, not one per tab: only one popup can exist and it belongs to
/// the pane with the keyboard, which is the same reasoning as the one `hover` and the one
/// `references` modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionState {
    /// The stem's byte range in the document: what `Enter` replaces.
    pub stem: Range<usize>,
    /// What is offered, best first.
    pub rows: Vec<Row>,
    /// Which row is chosen. The first to begin with, so `Tab` alone takes the best match.
    pub chosen: usize,
    /// The first row drawn, which is what the list scrolls by.
    pub scroll: usize,
    /// The `text_revision` the rows were worked out at.
    pub revision: u64,
    /// Where the caret was then, so a caret that moved with no edit behind it closes the popup.
    pub caret: usize,
    /// True when it was asked for by hand, which is what lets it live in a comment or a string.
    pub manual: bool,
    /// The tab it belongs to. A popup can only exist on a file a plugin claims, so there is always
    /// a path, and comparing it against the tab that is showing is what closes the popup when the
    /// tab changes or the keyboard moves to another pane — derived from the state rather than fired
    /// from each of the places a tab can change.
    pub path: PathBuf,
}

impl CompletionState {
    /// The rows that are drawn: at most [`VISIBLE_ROWS`] of them, starting at the scroll.
    pub fn shown(&self) -> Range<usize> {
        let visible = VISIBLE_ROWS.min(self.rows.len());
        self.scroll..(self.scroll + visible).min(self.rows.len())
    }

    /// The row that would be accepted.
    pub fn chosen_row(&self) -> Option<&Row> {
        self.rows.get(self.chosen)
    }

    /// Bring the chosen row back inside the eight that are drawn, dragging the list with it.
    fn settle_the_scroll(&mut self) {
        let visible = VISIBLE_ROWS.min(self.rows.len());
        if visible == 0 {
            self.scroll = 0;
            return;
        }
        if self.chosen < self.scroll {
            self.scroll = self.chosen;
        }
        if self.chosen >= self.scroll + visible {
            self.scroll = self.chosen + 1 - visible;
        }
        self.scroll = self.scroll.min(self.rows.len() - visible);
    }
}

/// The five keys the popup takes, once they have been read out of a frame's input.
///
/// A structure rather than five arguments, so a caller cannot pass `Tab` where `Enter` goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompletionKeys {
    pub down: bool,
    pub up: bool,
    /// Accept, replacing the whole identifier the caret is inside.
    pub tab: bool,
    /// Accept, replacing the stem only.
    pub enter: bool,
    pub escape: bool,
}

impl CompletionKeys {
    /// One key on its own, which is what a test presses.
    pub fn down() -> Self {
        Self { down: true, ..Self::default() }
    }

    pub fn up() -> Self {
        Self { up: true, ..Self::default() }
    }

    pub fn tab() -> Self {
        Self { tab: true, ..Self::default() }
    }

    pub fn enter() -> Self {
        Self { enter: true, ..Self::default() }
    }

    pub fn escape() -> Self {
        Self { escape: true, ..Self::default() }
    }
}

/// Where the popup hangs, worked out by the pane that has the keyboard while it draws itself.
///
/// Frame local. The pane loop borrows the focus, so the pane being drawn is the only thing that
/// knows where its caret ended up on the screen; the window draws the popup **after** the loop,
/// from what that pane recorded, so the list is never underneath a divider or a later pane and
/// never drawn twice in a split view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompletionAnchor {
    /// The caret's own box on the screen, which the list hangs under.
    pub caret: egui::Rect,
    /// The editing area it is in, which the list is flipped and clamped inside.
    pub pane: egui::Rect,
}

impl QuillApp {
    /// Whether auto-complete applies to the file that is showing.
    pub fn completion_applies_here(&self) -> bool {
        file_kind::completion_applies(self.files.active().path(), &self.plugins.grammars())
    }

    /// Whether a modal owns the keyboard, in which case there is no popup and no trigger.
    ///
    /// A modal already stands the editing area aside, so this is belt and braces — but the belt is
    /// what stops a popup that was open when `Find in Files` opened over it from staying there.
    fn a_modal_is_open(&self) -> bool {
        self.settings_window.open
            || self.prompt.is_some()
            || self.go_to_file.is_some()
            || self.find_in_files.is_some()
            || self.references.is_some()
            || self.about.is_some()
            || self.confirmation.is_some()
    }

    /// Everything that could be offered for a stem, gathered once and never per frame.
    ///
    /// The four sources of §4.1, in the order the tie-break reads them. Each name is put through
    /// `completion::could_match` before a candidate is built for it, because turning four thousand
    /// index names into owned strings to throw nearly all of them away again is the difference
    /// between a keystroke that allocates and one that does not.
    pub fn completion_candidates(&mut self, stem: &str) -> Vec<Candidate> {
        let mut pool: Vec<Candidate> = Vec::new();
        if stem.is_empty() {
            return pool;
        }
        let here = self.files.active_index();
        let open: Vec<PathBuf> =
            self.files.iter().filter_map(|file| file.path().map(Path::to_path_buf)).collect();

        // The open tabs, read from their live text: this one's definitions and its words first,
        // then every other tab's definitions.
        for index in 0..self.files.len() {
            let path = self.files.at(index).path().map(Path::to_path_buf);
            let detail = path.as_deref().map(file_name).unwrap_or_default();
            let source = if index == here { Source::ThisFile } else { Source::OpenTab };
            let symbols = self.tab_symbols(index);
            for (name, definition) in &symbols.named {
                if completion::could_match(stem, name) {
                    pool.push(Candidate::described(
                        name.clone(),
                        source,
                        Some(definition.kind),
                        detail.clone(),
                    ));
                }
            }
            // Only this file's words. Harvesting every open file's words is §12's rejection: the
            // index's definitions are the cross-file offer, and they carry a kind and a file where
            // a raw word carries nothing.
            if index == here {
                for word in &symbols.words {
                    if completion::could_match(stem, word) {
                        pool.push(Candidate::new(word.clone(), Source::Word));
                    }
                }
            }
        }

        // The project's definitions, with the open files' paths dropped: the ownership rule.
        if let Some(indexer) = self.symbols.as_ref() {
            let index = indexer.index();
            for name in index.sorted_names() {
                if !completion::could_match(stem, name) {
                    continue;
                }
                let Some(entry) =
                    index.definitions_of(name).iter().find(|entry| !open.contains(&entry.path))
                else {
                    continue;
                };
                pool.push(Candidate::described(
                    name.clone(),
                    Source::Index,
                    Some(entry.kind),
                    file_name(&entry.path),
                ));
            }
        }

        // The language's own words. The manifest already holds them; completion is the second
        // reader of the same data.
        if let Some(grammar) = self.grammar_for(self.files.active().path()) {
            let lists: [(&Vec<String>, &str); 3] = [
                (&grammar.keywords, "keyword"),
                (&grammar.builtins, "builtin"),
                (&grammar.types, "type"),
            ];
            for (list, detail) in lists {
                for word in list {
                    if completion::could_match(stem, word) {
                        pool.push(Candidate::described(
                            word.clone(),
                            Source::Language,
                            None,
                            detail,
                        ));
                    }
                }
            }
        }
        pool
    }

    /// The stem at a point in the tab that is showing, and the rows it offers.
    ///
    /// One function, so `quill-cli editor complete` prints exactly the list the popup would show
    /// and the two can never come to disagree about what is on offer.
    pub fn completion_at(&mut self, offset: usize) -> (Range<usize>, String, Vec<Row>) {
        let text = self.document().text().to_string();
        let stem = completion::stem_at(&text, offset, &self.completion_grammar());
        if stem.is_empty() {
            return (stem, String::new(), Vec::new());
        }
        let word = text[stem.clone()].to_owned();
        let rows = self.completion_rows(&word);
        (stem, word, rows)
    }

    /// The rows a stem offers here, best first. What the popup shows and what the command line
    /// prints, so the two can never disagree.
    pub fn completion_rows(&mut self, stem: &str) -> Vec<Row> {
        let pool = self.completion_candidates(stem);
        completion::rank(stem, pool)
    }

    /// The grammar reading the file that is showing, or an empty one.
    fn completion_grammar(&self) -> Grammar {
        self.grammar_for(self.files.active().path()).cloned().unwrap_or_default()
    }

    /// The stem under the caret in the tab that is showing, and the text it is a range of.
    fn stem_here(&self) -> (String, Range<usize>) {
        let text = self.document().text().to_string();
        let head = self.document().selection().head;
        let stem = completion::stem_at(&text, head, &self.completion_grammar());
        (text, stem)
    }

    /// Work the popup for a frame: close it when it has stopped being an answer, and refilter it
    /// when the word being typed has changed.
    ///
    /// Called from the pane with the keyboard, after its input has been handled, so the stem
    /// includes the letter that was just typed. `typed` is whether a character reached the document
    /// this frame — the automatic trigger fires on that and on nothing else, which is why a paste,
    /// an undo or a command line edit does not make a list appear over somebody's work.
    pub fn keep_the_completion_fresh(&mut self, typed: bool) {
        if self.completion.is_some() {
            self.refresh_the_completion();
            return;
        }
        if typed {
            self.offer_a_completion();
        }
    }

    /// Open the popup unasked, if every one of §5.1's conditions holds.
    fn offer_a_completion(&mut self) {
        if !self.settings.suggestions.is_automatic()
            || !self.completion_applies_here()
            || self.a_modal_is_open()
            || self.focus != Focus::Editor
        {
            return;
        }
        let (text, stem) = self.stem_here();
        if text[stem.clone()].chars().count() < AUTOMATIC_STEM {
            return;
        }
        // A doc comment's prose does not want a list flickering over it. Asked at the stem's own
        // first byte rather than at the caret, because a caret sitting exactly at the end of a
        // comment is past the span and would read as code.
        let index = self.files.active_index();
        if self.tab_symbols(index).read.role_at(stem.start) != Role::Code {
            return;
        }
        self.open_the_completion(stem, &text, false);
    }

    /// `Complete Word`, `Ctrl+Space`, and `quill-cli editor complete`.
    ///
    /// Works from one character and works inside a comment or a string, where the automatic popup
    /// never opens: somebody who asks in a doc comment deserves the file's words. With no
    /// identifier character to the left of the caret at all it does what every honest miss in Quill
    /// does and says so in the status bar.
    pub fn complete_word(&mut self) {
        if !self.completion_applies_here() {
            self.message =
                Some("No plugin claims this file, so Quill has no words to offer.".to_owned());
            return;
        }
        let (text, stem) = self.stem_here();
        if stem.is_empty() {
            self.completion = None;
            self.message = Some("There is nothing to complete here.".to_owned());
            return;
        }
        let word = text[stem.clone()].to_owned();
        if !self.open_the_completion(stem, &text, true) {
            self.message = Some(format!("Nothing completes '{word}'."));
        }
    }

    /// Work out the rows and open the popup on them. False when there was nothing to offer, in
    /// which case nothing opens: a list that lingers empty is a list that says nothing.
    fn open_the_completion(&mut self, stem: Range<usize>, text: &str, manual: bool) -> bool {
        let Some(path) = self.files.active().path().map(Path::to_path_buf) else {
            return false;
        };
        let word = text[stem.clone()].to_owned();
        let rows = self.completion_rows(&word);
        if rows.is_empty() {
            self.completion = None;
            return false;
        }
        // Whatever the status bar was saying described the state before this list existed, and a
        // popup opening over a stale sentence reads as an answer to the wrong question.
        self.message = None;
        self.completion = Some(CompletionState {
            stem,
            rows,
            chosen: 0,
            scroll: 0,
            revision: self.document().text_revision(),
            caret: self.document().selection().head,
            manual,
            path,
        });
        true
    }

    /// Close it. Nothing but dropping the state: no animation, no memory, nothing written anywhere.
    pub fn close_the_completion(&mut self) {
        self.completion = None;
    }

    /// True while the popup is open, which is what the key routing and the drawing both ask.
    pub fn completion_is_open(&self) -> bool {
        self.completion.is_some()
    }

    /// What is being offered, for a test and for the command line.
    pub fn completion(&self) -> Option<&CompletionState> {
        self.completion.as_ref()
    }

    /// Where the popup was drawn on the last frame, for a test that has to say it flipped.
    pub fn completion_anchor(&self) -> Option<CompletionAnchor> {
        self.completion_anchor
    }

    /// Recompute the rows, or close the popup, by §5.5's one sentence: *it is open only while it is
    /// an answer to the word being typed at the caret.*
    fn refresh_the_completion(&mut self) {
        let Some(state) = self.completion.as_ref() else {
            return;
        };
        let showing = self.files.active().path().map(Path::to_path_buf);
        if showing.as_deref() != Some(state.path.as_path())
            || !self.completion_applies_here()
            || self.a_modal_is_open()
            || self.focus != Focus::Editor
        {
            self.close_the_completion();
            return;
        }
        let revision = self.document().text_revision();
        let head = self.document().selection().head;
        // Nothing moved. Two integer comparisons and no work at all, which is what a caret blink,
        // a repaint and a frame of idling cost.
        if revision == state.revision && head == state.caret {
            return;
        }
        // The caret moved with no edit behind it: a click, `Left`, `Home`, a jump. The popup is an
        // answer to the word being typed, and that is no longer the word being typed.
        if revision == state.revision {
            self.close_the_completion();
            return;
        }
        let started_at = state.stem.start;
        let (text, stem) = self.stem_here();
        if stem.is_empty() || stem.start != started_at {
            self.close_the_completion();
            return;
        }
        let word = text[stem.clone()].to_owned();
        let rows = self.completion_rows(&word);
        if rows.is_empty() {
            // Typing narrowed it to nothing. It does not linger empty; the next character typed
            // asks again.
            self.close_the_completion();
            return;
        }
        let Some(state) = self.completion.as_mut() else {
            return;
        };
        state.stem = stem;
        state.chosen = state.chosen.min(rows.len() - 1);
        state.rows = rows;
        state.revision = revision;
        state.caret = head;
        state.settle_the_scroll();
    }

    /// The popup's five keys, taken out of the frame's input before any pane reads it.
    ///
    /// Only a **bare** `Tab` is taken, so `Ctrl+Tab` is still `Next Tab` and the three meanings of
    /// the key — move tab, indent, complete — stay on their three distinct chords. The same is true
    /// of every other key here: `Modifiers::NONE` is compared, so nothing with the command key held
    /// is ever swallowed.
    ///
    /// Reading the keys and acting on them are two functions, because they fail in different ways:
    /// what a test of the *meanings* wants is [`Self::the_completion_keys`] with five booleans, and
    /// what a test of the **consumption** wants is a real context with a real key event in it — the
    /// property that a key the popup took never reaches `editor_view::handle_input`.
    pub(crate) fn route_the_completion_keys(&mut self, ui: &egui::Ui) {
        if self.completion.is_none()
            || self.focus != Focus::Editor
            || crate::app::text_box_has_the_keyboard(ui.ctx())
        {
            return;
        }
        let keys = ui.input_mut(take_the_five_keys);
        self.the_completion_keys(keys);
    }

    /// What the five keys mean, once they have been read.
    pub fn the_completion_keys(&mut self, keys: CompletionKeys) {
        if self.completion.is_none() {
            return;
        }
        if keys.escape {
            self.close_the_completion();
            return;
        }
        if keys.down {
            self.move_the_completion(1);
        }
        if keys.up {
            self.move_the_completion(-1);
        }
        // `Tab` replaces the whole identifier and `Enter` replaces the stem, which is IntelliJ's own
        // distinction and is right in both directions: `Enter` when finishing a fresh word, `Tab`
        // when retyping the front of an existing one.
        if keys.tab {
            self.accept_the_completion(true);
        } else if keys.enter {
            self.accept_the_completion(false);
        }
    }

    /// Move the pill, clamped at the ends. No wrap: a list that jumps from the last row back to the
    /// first is a list you cannot hold `Down` on.
    pub fn move_the_completion(&mut self, delta: i32) {
        let Some(state) = self.completion.as_mut() else {
            return;
        };
        if state.rows.is_empty() {
            return;
        }
        let last = state.rows.len() - 1;
        let wanted = state.chosen as i64 + delta as i64;
        state.chosen = wanted.clamp(0, last as i64) as usize;
        state.settle_the_scroll();
    }

    /// Choose a row by name, which is what a click and `editor complete --choose` both do.
    pub fn choose_the_completion(&mut self, name: &str) -> bool {
        let Some(state) = self.completion.as_mut() else {
            return false;
        };
        let Some(at) = state.rows.iter().position(|row| row.name == name) else {
            return false;
        };
        state.chosen = at;
        state.settle_the_scroll();
        true
    }

    /// Take the chosen row.
    ///
    /// One `Command::ReplaceMany`, which is one undo step by construction because undo restores a
    /// snapshot — so one press of undo puts back the stem as it was typed. The caret lands at the
    /// end of the inserted name, the marked passages and the selection shift exactly as they do for
    /// every other edit, and the file is marked as changed, because this **is** an edit.
    ///
    /// `whole_word` is `Tab`: the range is the identifier the caret is inside rather than the stem,
    /// so `dra│wing` completed to `draw_frame` does not leave `wing` dangling behind the caret.
    pub fn accept_the_completion(&mut self, whole_word: bool) -> bool {
        let Some(state) = self.completion.take() else {
            return false;
        };
        let Some(row) = state.chosen_row().cloned() else {
            return false;
        };
        let text = self.document().text().to_string();
        let head = self.document().selection().head;
        let range = match whole_word {
            true => completion::word_at(&text, head, &self.completion_grammar()),
            false => state.stem.clone(),
        };
        // A range that came out empty is a caret with nothing to its left, which cannot happen while
        // a popup is open; falling back to the stem rather than inserting at a guess keeps it true.
        let range = if range.is_empty() { state.stem.clone() } else { range };
        let applied =
            self.document_mut().apply(Command::ReplaceMany(vec![(range, row.name.clone())]));
        if applied {
            // Completing into a file you were only glancing at plainly means you meant to open it,
            // which is what typing into one already does.
            let active = self.files.active_index();
            self.files.make_permanent(active);
        }
        applied
    }
}

impl QuillApp {
    /// Write down where the popup hangs, from the caret's own box in the pane that has the keyboard.
    ///
    /// The same arithmetic the caret itself is painted with, and taken at the position the frame
    /// settled on rather than the one it opened with — a list anchored to where the caret was before
    /// the wheel had its say would be a frame behind the writing.
    ///
    /// A wheel scroll that leaves the caret on the screen keeps the popup, and one that takes its
    /// line off the screen closes it: an offer hanging off a word nobody can see is not an offer.
    pub(crate) fn remember_where_the_completion_hangs(
        &mut self,
        origin: egui::Pos2,
        area: egui::Rect,
    ) {
        self.completion_anchor = None;
        if self.completion.is_none() {
            return;
        }
        let caret = self.layout().caret_at(self.document().selection().head);
        let box_of_it = egui::Rect::from_min_size(
            egui::Pos2::new(origin.x + caret.x, origin.y + caret.y),
            egui::Vec2::new(2.0, caret.height),
        );
        if box_of_it.bottom() < area.top() || box_of_it.top() > area.bottom() {
            self.close_the_completion();
            return;
        }
        self.completion_anchor =
            Some(CompletionAnchor { caret: box_of_it, pane: area });
    }

    /// Draw the popup, and take the row a click landed on.
    ///
    /// A click accepts the same way `Enter` does — the stem only — and never reaches the editing
    /// area behind it, because the list's own `Area` is in front and takes the hit.
    pub(crate) fn show_the_completion(&mut self, ui: &mut egui::Ui) {
        let (Some(anchor), Some(state)) = (self.completion_anchor, self.completion.as_ref()) else {
            return;
        };
        let outcome = view::show(ui, state, anchor.caret, anchor.pane);
        if let Some(name) = outcome.accepted {
            if self.choose_the_completion(&name) {
                self.accept_the_completion(false);
            }
        }
    }
}

/// Take the popup's five keys out of a frame's input, leaving everything else in it.
///
/// Written out rather than five calls to `InputState::consume_key`, and the reason is worth keeping.
/// `consume_key` matches through `Modifiers::matches_logically`, which asks only that the modifiers
/// the *pattern* names are held — so a pattern of `NONE` matches a press with **shift** held as
/// well, and `Shift+Enter`, which is an ordinary new line in the editing area, was being swallowed
/// as an accept. What the popup wants is the bare key and nothing else, so the modifiers are
/// compared for real.
///
/// `Ctrl+Tab` was never at risk, because the command key is the one modifier `matches_logically`
/// does compare both ways — but a rule that holds for one of the four by accident is not a rule.
fn take_the_five_keys(input: &mut egui::InputState) -> CompletionKeys {
    let mut keys = CompletionKeys::default();
    input.events.retain(|event| {
        let egui::Event::Key { key, pressed: true, modifiers, .. } = event else {
            return true;
        };
        if !modifiers.is_none() {
            return true;
        }
        match key {
            egui::Key::ArrowDown => keys.down = true,
            egui::Key::ArrowUp => keys.up = true,
            egui::Key::Tab => keys.tab = true,
            egui::Key::Enter => keys.enter = true,
            egui::Key::Escape => keys.escape = true,
            _ => return true,
        }
        false
    });
    keys
}

/// A path's last part, which is what a row says about where a definition came from.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{Action, Entry};
    use crate::settings::Suggestions;
    use quill_core::Command;

    /// A little project to type into: two Rust files that are opened, one that is not, a stylesheet
    /// and a note.
    ///
    /// `layout.rs` defines five things starting `draw` or near enough to rank against each other,
    /// `distant.rs` is never opened so its `draw_everything` can only come from the index, and
    /// `notes.md` is what a file no plugin claims looks like.
    fn a_project(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(
            folder.join("layout.rs"),
            "pub struct Layout;\n\nimpl Layout {\n    pub fn new() -> Self {\n        Layout\n    }\n\n    pub fn draw(&self) {}\n\n    pub fn draw_frame(&self) {}\n\n    pub fn redraw(&self) {}\n\n    // draw the whole page\n    pub fn paint_text(&self) {}\n}\n",
        )
        .expect("write layout.rs");
        std::fs::write(
            folder.join("caret.rs"),
            "pub struct Caret;\n\nimpl Caret {\n    pub fn new() -> Self {\n        Caret\n    }\n\n    pub fn paint(&self, layout: &Layout) {\n        layout.draw();\n    }\n}\n",
        )
        .expect("write caret.rs");
        std::fs::write(folder.join("distant.rs"), "pub fn draw_everything() {}\n")
            .expect("write distant.rs");
        std::fs::write(folder.join("site.css"), ".card {\n  --brand-hue: 280;\n}\n")
            .expect("write site.css");
        std::fs::write(folder.join("notes.md"), "# draw\nA note about drawing.\n")
            .expect("write notes.md");
        folder
    }

    /// A window on that project, its index built, with `layout.rs` open and the caret at the end.
    fn a_window(name: &str) -> (PathBuf, QuillApp) {
        let folder = a_project(name);
        let mut app = QuillApp::new(&folder);
        build_the_index(&mut app);
        app.open_path_permanently(&folder.join("layout.rs"));
        let end = app.document().text().len_bytes();
        app.document_mut().apply(Command::PlaceCaret { offset: end, extend: false });
        (folder, app)
    }

    /// Read the project and wait for the thread, which is what a frame of the real window does over
    /// however many frames it takes.
    fn build_the_index(app: &mut QuillApp) {
        app.the_project_changed_on_disk();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            app.keep_the_symbol_index_fresh();
            if app.symbols_indexer().is_some_and(|indexer| !indexer.is_building()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the index should have been built");
    }

    /// Type, one character at a time, exactly as the window does: the letter lands in the document
    /// and then the popup is worked for that frame.
    fn typing(app: &mut QuillApp, text: &str) {
        for character in text.chars() {
            app.document_mut().apply(Command::Insert(character.to_string()));
            app.keep_the_completion_fresh(true);
        }
    }

    /// A frame in which nothing was typed, which is what everything that is not typing looks like.
    fn a_quiet_frame(app: &mut QuillApp) {
        app.keep_the_completion_fresh(false);
    }

    /// Put the file back as it was and type the stem again, so a test that presses several things in
    /// turn presses each of them against the same open popup rather than against whatever the last
    /// one left behind.
    fn an_open_popup(app: &mut QuillApp, original: &str) {
        app.close_the_completion();
        let whole = 0..app.document().text().len_bytes();
        app.document_mut().apply(Command::ReplaceMany(vec![(whole, original.to_owned())]));
        let end = app.document().text().len_bytes();
        app.document_mut().apply(Command::PlaceCaret { offset: end, extend: false });
        typing(app, "dra");
    }

    /// The names on offer, in order.
    fn offered(app: &QuillApp) -> Vec<String> {
        app.completion()
            .map(|state| state.rows.iter().map(|row| row.name.clone()).collect())
            .unwrap_or_default()
    }

    fn text_of(app: &QuillApp) -> String {
        app.document().text().to_string()
    }

    #[test]
    fn typing_the_second_letter_of_a_word_opens_the_list_with_the_best_row_chosen() {
        // Scenario 12. One letter is not enough — that is scenario 11 — and the second one is.
        let (folder, mut app) = a_window("quill-completion-opens");
        typing(&mut app, "d");
        assert!(app.completion().is_none(), "one character is noise, not an offer");
        typing(&mut app, "r");
        let state = app.completion().expect("the popup opened on the second letter");
        assert_eq!(state.chosen, 0, "the first row is pre-chosen, so Tab alone takes the best match");
        assert_eq!(state.rows[0].name, "draw", "which is the shortest thing starting with `dr`");
        assert!(offered(&app).contains(&"draw_frame".to_owned()), "{:?}", offered(&app));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_one_character_stem_asks_the_pool_for_nothing_at_all() {
        // Scenario 11 at the layer that decides it: the automatic path never even gathers.
        let (folder, mut app) = a_window("quill-completion-one-letter");
        typing(&mut app, "d");
        assert!(app.completion().is_none());
        // And an empty stem answers nothing however it is asked.
        assert!(app.completion_candidates("").is_empty());
        assert!(app.completion_rows("").is_empty());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn nothing_ever_opens_in_a_file_no_plugin_claims() {
        // Scenario 13. Absent rather than dimmed, which is Quill's rule for a control that can never
        // apply: the menu entry is not there either.
        let (folder, mut app) = a_window("quill-completion-prose");
        app.open_path_permanently(&folder.join("notes.md"));
        let end = app.document().text().len_bytes();
        app.document_mut().apply(Command::PlaceCaret { offset: end, extend: false });
        typing(&mut app, "draw");
        assert!(app.completion().is_none(), "prose has no words worth offering");
        assert!(!app.completion_applies_here());
        app.complete_word();
        assert!(app.completion().is_none(), "and asking by hand says so rather than opening");
        assert!(app.message.is_some());
        let entries = crate::app::actions::symbol_entries(&app.menu_state());
        assert!(
            !entries.iter().any(|entry| matches!(
                entry,
                Entry::Item { action: Action::CompleteWord, .. }
            )),
            "the menu entry is absent for a note"
        );
        // A stylesheet is the opposite: no definers, but its own words and keywords are real offers.
        app.open_path_permanently(&folder.join("site.css"));
        assert!(app.completion_applies_here(), "CSS completes");
        let entries = crate::app::actions::symbol_entries(&app.menu_state());
        assert!(entries.iter().any(|entry| matches!(
            entry,
            Entry::Item { action: Action::CompleteWord, .. }
        )));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_css_file_completes_its_own_custom_properties() {
        // The other half of scenario 9, in the window: CSS names no definers, so source 1 — this
        // file's words — is the only thing it has, and it is enough.
        let (folder, mut app) = a_window("quill-completion-css");
        app.open_path_permanently(&folder.join("site.css"));
        let end = app.document().text().len_bytes();
        app.document_mut().apply(Command::PlaceCaret { offset: end, extend: false });
        typing(&mut app, "--br");
        let rows = offered(&app);
        assert!(rows.contains(&"--brand-hue".to_owned()), "{rows:?}");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_automatic_popup_stays_out_of_a_comment_and_asking_by_hand_opens_one_there() {
        // Scenario 14. A doc comment's prose does not want a list flickering over it; somebody who
        // asks in one deserves the file's words.
        let (folder, mut app) = a_window("quill-completion-comment");
        let inside = text_of(&app).find("the whole page").expect("the comment") + "the ".len();
        app.document_mut().apply(Command::PlaceCaret { offset: inside, extend: false });
        typing(&mut app, "dr");
        assert!(app.completion().is_none(), "nothing arrives unasked inside a comment");
        app.complete_word();
        let rows = offered(&app);
        assert!(rows.contains(&"draw".to_owned()), "but asking gets the file's words: {rows:?}");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn typing_on_until_nothing_matches_closes_it_and_the_next_letter_asks_again() {
        // Scenario 15. It does not linger empty, and it does not reopen on its own.
        let (folder, mut app) = a_window("quill-completion-narrows");
        typing(&mut app, "dra");
        assert!(app.completion().is_some());
        typing(&mut app, "z");
        assert!(app.completion().is_none(), "`draz` matches nothing here");
        a_quiet_frame(&mut app);
        assert!(app.completion().is_none(), "and an idle frame does not bring it back");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn backspace_inside_the_word_keeps_it_open_and_refilters_it() {
        // Scenario 16.
        let (folder, mut app) = a_window("quill-completion-backspace");
        typing(&mut app, "draw_");
        let narrow = offered(&app);
        assert!(narrow.contains(&"draw_frame".to_owned()), "{narrow:?}");
        assert!(!narrow.contains(&"redraw".to_owned()), "the underscore ruled it out: {narrow:?}");
        app.document_mut().apply(Command::DeleteBackward);
        app.keep_the_completion_fresh(false);
        let wider = offered(&app);
        assert!(app.completion().is_some(), "still open");
        assert!(wider.contains(&"redraw".to_owned()), "and refiltered: {wider:?}");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn typing_a_word_boundary_closes_it() {
        // Scenario 17. The stem is gone, so there is no word being typed to be an answer to.
        for boundary in ["(", " ", "."] {
            let (folder, mut app) = a_window("quill-completion-boundary");
            typing(&mut app, "dra");
            assert!(app.completion().is_some());
            typing(&mut app, boundary);
            assert!(app.completion().is_none(), "{boundary} ends the word");
            std::fs::remove_dir_all(&folder).ok();
        }
    }

    #[test]
    fn the_caret_moving_by_anything_but_typing_closes_it() {
        // Scenario 18: a click, an arrow, Home, a jump — every one of them is a caret that moved
        // with no edit behind it, which is one rule rather than six.
        let (folder, mut app) = a_window("quill-completion-caret-moved");
        let original = text_of(&app);
        for movement in [
            Command::MoveLeft { extend: false },
            Command::MoveLineStart { extend: false },
            Command::MoveDocumentStart { extend: false },
            Command::PlaceCaret { offset: 0, extend: false },
        ] {
            an_open_popup(&mut app, &original);
            assert!(app.completion().is_some(), "{movement:?} needs a popup to close");
            app.document_mut().apply(movement.clone());
            a_quiet_frame(&mut app);
            assert!(app.completion().is_none(), "{movement:?} closed it");
        }
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_modal_opening_and_the_tab_changing_both_close_it() {
        // The rest of scenario 18, and scenario 32's second half.
        let (folder, mut app) = a_window("quill-completion-modal");
        typing(&mut app, "dra");
        assert!(app.completion().is_some());
        app.settings_window.open = true;
        a_quiet_frame(&mut app);
        assert!(app.completion().is_none(), "a modal owns the keyboard");
        app.settings_window.open = false;

        typing(&mut app, "w");
        typing(&mut app, "_");
        assert!(app.completion().is_some(), "{:?}", offered(&app));
        app.open_path_permanently(&folder.join("caret.rs"));
        a_quiet_frame(&mut app);
        assert!(app.completion().is_none(), "the popup goes with the tab it belonged to");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn escape_closes_it_and_leaves_the_document_alone() {
        // Scenario 19. Consumed, so it cannot also clear a selection.
        let (folder, mut app) = a_window("quill-completion-escape");
        typing(&mut app, "dra");
        let before = text_of(&app);
        app.document_mut().apply(Command::MoveLeft { extend: true });
        app.the_completion_keys(CompletionKeys::escape());
        assert!(app.completion().is_none());
        assert_eq!(text_of(&app), before, "Escape is not an edit");
        assert!(!app.document().selection().is_empty(), "and the selection survives it");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn manual_suggestions_stop_the_unasked_popup_and_keep_the_asked_one() {
        // Scenario 20. `manual` is already the off switch, which is why there is no third value.
        let (folder, mut app) = a_window("quill-completion-manual");
        app.settings.suggestions = Suggestions::Manual;
        typing(&mut app, "draw");
        assert!(app.completion().is_none(), "nothing arrives unasked");
        app.complete_word();
        assert!(app.completion().is_some(), "and Ctrl+Space still works: {:?}", app.message);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn asking_with_no_word_to_the_left_of_the_caret_says_so_and_opens_nothing() {
        // Scenario 21, and it works from one character, which the automatic path does not.
        let (folder, mut app) = a_window("quill-completion-nothing-there");
        typing(&mut app, " ");
        app.complete_word();
        assert!(app.completion().is_none());
        assert_eq!(app.message.as_deref(), Some("There is nothing to complete here."));
        app.message = None;
        typing(&mut app, "d");
        assert!(app.completion().is_none(), "one letter is still not an unasked offer");
        app.complete_word();
        assert!(app.completion().is_some(), "but asking from one letter works: {:?}", app.message);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn only_the_pane_with_the_keyboard_has_a_popup() {
        // Scenario 23. One `Option` on the window is what makes "at most one" true by construction;
        // what has to be shown is that it belongs to the pane being typed into.
        let (folder, mut app) = a_window("quill-completion-split");
        app.open_path_permanently(&folder.join("caret.rs"));
        let context = egui::Context::default();
        app.run_action(Action::SplitRight, &context);
        assert_eq!(app.files.pane_count(), 2, "the editing area is split");
        let end = app.document().text().len_bytes();
        app.document_mut().apply(Command::PlaceCaret { offset: end, extend: false });
        typing(&mut app, "pai");
        assert!(app.completion().is_some(), "the pane with the keyboard has it");
        app.run_action(Action::PreviousPane, &context);
        a_quiet_frame(&mut app);
        assert!(app.completion().is_none(), "and the keyboard moving away closes it");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn down_twice_then_enter_replaces_the_stem_and_one_undo_puts_it_back() {
        // Scenario 24, and scenario 31 with it: the third row is the one from the file that is not
        // open, so accepting it proves nothing is opened and nothing is read from the disk.
        let (folder, mut app) = a_window("quill-completion-accept");
        let before = text_of(&app);
        typing(&mut app, "dra");
        assert_eq!(
            offered(&app),
            vec!["draw", "draw_frame", "draw_everything", "redraw"],
            "the order the rubric gives"
        );
        app.the_completion_keys(CompletionKeys::down());
        app.the_completion_keys(CompletionKeys::down());
        assert_eq!(app.completion().expect("open").chosen, 2);
        app.the_completion_keys(CompletionKeys::enter());
        assert!(app.completion().is_none(), "accepting closes it");
        assert!(text_of(&app).ends_with("draw_everything"), "{:?}", text_of(&app));
        assert_eq!(
            app.document().selection().head,
            app.document().text().len_bytes(),
            "the caret lands after the inserted name"
        );
        assert_eq!(
            app.files.active().path(),
            Some(folder.join("layout.rs").as_path()),
            "nothing was opened to insert a name from a closed file"
        );
        app.document_mut().apply(Command::Undo);
        assert!(text_of(&app).ends_with("dra"), "one step puts the stem back: {:?}", text_of(&app));
        assert!(text_of(&app).starts_with(&before[..before.len() - 1]));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_pill_is_clamped_at_the_ends_and_the_list_scrolls_with_it() {
        // Scenario 25. No wrap: a list that jumps from the last row back to the first is one you
        // cannot hold `Down` on.
        let (folder, mut app) = a_window("quill-completion-steering");
        typing(&mut app, "dr");
        let rows = app.completion().expect("open").rows.len();
        app.the_completion_keys(CompletionKeys::up());
        assert_eq!(app.completion().expect("open").chosen, 0, "clamped at the top");
        for _ in 0..rows + 5 {
            app.the_completion_keys(CompletionKeys::down());
        }
        let state = app.completion().expect("open");
        assert_eq!(state.chosen, rows - 1, "clamped at the bottom");
        assert!(state.shown().contains(&state.chosen), "and the list scrolled to it");
        assert!(state.shown().len() <= VISIBLE_ROWS, "eight rows at most");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_list_scrolls_when_the_pill_walks_off_the_eight_that_are_drawn() {
        // The other half of scenario 25, which needs more rows than the list draws.
        let (folder, mut app) = a_window("quill-completion-scrolls");
        typing(&mut app, "ra");
        let rows = app.completion().expect("open").rows.len();
        assert!(rows > VISIBLE_ROWS, "the fixture has to offer more than eight: {rows}");
        assert_eq!(app.completion().expect("open").scroll, 0, "it starts at the top");
        for _ in 0..VISIBLE_ROWS {
            app.the_completion_keys(CompletionKeys::down());
        }
        let state = app.completion().expect("open");
        assert!(state.scroll > 0, "walking past the eighth row drags the list");
        assert!(state.shown().contains(&state.chosen));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn tab_replaces_the_whole_word_and_enter_replaces_only_the_stem() {
        // Scenarios 26 and 27, the pair IntelliJ keeps apart and this design keeps apart with it.
        for (whole_word, expected) in [(true, "draw_frame;"), (false, "draw_framewing;")] {
            let (folder, mut app) = a_window("quill-completion-mid-word");
            typing(&mut app, "drawing;");
            let at = text_of(&app).find("drawing").expect("the word") + "dra".len();
            app.document_mut().apply(Command::PlaceCaret { offset: at, extend: false });
            app.complete_word();
            assert!(app.choose_the_completion("draw_frame"), "{:?}", offered(&app));
            app.the_completion_keys(if whole_word {
                CompletionKeys::tab()
            } else {
                CompletionKeys::enter()
            });
            assert!(text_of(&app).ends_with(expected), "{:?}", text_of(&app));
            std::fs::remove_dir_all(&folder).ok();
        }
    }

    #[test]
    fn typing_while_it_is_open_lands_in_the_document_and_refilters_the_list() {
        // Scenario 29, in that order: the letters are not consumed, and the list narrows to them.
        let (folder, mut app) = a_window("quill-completion-typing-through");
        typing(&mut app, "dr");
        let wide = offered(&app);
        typing(&mut app, "aw_f");
        assert!(text_of(&app).ends_with("draw_f"), "every letter reached the file");
        let narrow = offered(&app);
        assert!(narrow.len() < wide.len(), "{wide:?} narrowed to {narrow:?}");
        assert_eq!(narrow, vec!["draw_frame".to_owned()]);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_click_on_a_row_accepts_it_exactly_as_enter_does() {
        // Scenario 30 at the layer that decides what a click means; the click itself is the
        // screenshot test's.
        let (folder, mut app) = a_window("quill-completion-click");
        typing(&mut app, "dra");
        assert!(app.choose_the_completion("redraw"));
        assert!(app.accept_the_completion(false));
        assert!(text_of(&app).ends_with("redraw"), "{:?}", text_of(&app));
        assert!(app.completion().is_none());
        // And a name that is not on offer changes nothing at all.
        typing(&mut app, "_x");
        assert!(!app.choose_the_completion("nothing_offers_this"));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_popup_takes_exactly_five_keys_and_only_while_it_is_open() {
        // The **non-interference** invariant. With it shut, nothing is consumed and every key means
        // what it meant before this ticket; with it open, exactly the five of §5.3 are taken out of
        // the frame and everything else — the letters above all — flows through.
        let (folder, mut app) = a_window("quill-completion-non-interference");
        let original = text_of(&app);
        let five = [
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::Tab,
            egui::Key::Enter,
            egui::Key::Escape,
        ];
        for key in five {
            assert!(app.completion().is_none());
            assert!(!pressing(&mut app, key, egui::Modifiers::NONE), "{key:?} with it shut");
        }
        for key in five {
            an_open_popup(&mut app, &original);
            assert!(pressing(&mut app, key, egui::Modifiers::NONE), "{key:?} with it open");
        }
        // Everything else is left alone, including a `Tab` with the control key held, which is
        // `Next Tab` and must stay `Next Tab`.
        for (key, modifiers) in [
            (egui::Key::Tab, egui::Modifiers::COMMAND),
            (egui::Key::ArrowLeft, egui::Modifiers::NONE),
            (egui::Key::ArrowRight, egui::Modifiers::NONE),
            (egui::Key::Home, egui::Modifiers::NONE),
            (egui::Key::Backspace, egui::Modifiers::NONE),
            (egui::Key::Enter, egui::Modifiers::SHIFT),
        ] {
            an_open_popup(&mut app, &original);
            assert!(!pressing(&mut app, key, modifiers), "{key:?} {modifiers:?} is not the popup's");
        }
        std::fs::remove_dir_all(&folder).ok();
    }

    /// Press one key at a real `egui::Context` and say whether the popup took it out of the frame.
    ///
    /// The consumption is the property, so it is measured the only way it can be: the event is put
    /// into a frame, the routing runs, and what is left in the frame afterwards is looked at.
    fn pressing(app: &mut QuillApp, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        let context = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        let mut left = 0;
        let output = context.run_ui(input, |ui| {
            app.route_the_completion_keys(ui);
            left = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter(|event| matches!(event, egui::Event::Key { key: pressed, .. } if *pressed == key))
                    .count()
            });
        });
        output.drop_without_applying_deltas();
        left == 0
    }

    #[test]
    fn the_index_answers_for_closed_files_and_never_for_open_ones() {
        // The ownership rule of `task-1675` §3.3, at the completion end: a name being edited in a
        // tab must never be offered twice, once live and once as the disk last saw it.
        let (folder, mut app) = a_window("quill-completion-ownership");
        let rows = app.completion_rows("dra");
        let everything: Vec<&Row> =
            rows.iter().filter(|row| row.name == "draw_everything").collect();
        assert_eq!(everything.len(), 1, "one row for it: {rows:?}");
        assert_eq!(everything[0].source, Source::Index, "and it comes from the index");
        assert_eq!(everything[0].detail, "distant.rs");
        let here: Vec<&Row> = rows.iter().filter(|row| row.name == "draw_frame").collect();
        assert_eq!(here.len(), 1, "and the open file's own definition is not doubled: {here:?}");
        assert_eq!(here[0].source, Source::ThisFile);

        // Open the file the index knew about, and the index's copy of it stops being offered.
        app.open_path_permanently(&folder.join("distant.rs"));
        let index = app.files.active_index();
        let _ = index;
        app.open_path_permanently(&folder.join("layout.rs"));
        let rows = app.completion_rows("dra");
        let everything: Vec<&Row> =
            rows.iter().filter(|row| row.name == "draw_everything").collect();
        assert_eq!(everything.len(), 1, "still one row: {rows:?}");
        assert_eq!(everything[0].source, Source::OpenTab, "now from the tab, read live");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_language_s_own_words_are_offered_and_labelled_as_such() {
        // Source 3: the manifest already holds them, and completion is the second reader of it.
        let (folder, mut app) = a_window("quill-completion-keywords");
        let rows = app.completion_rows("str");
        let keyword = rows.iter().find(|row| row.name == "struct").expect("`struct`: {rows:?}");
        assert_eq!(keyword.source, Source::Language);
        assert_eq!(keyword.detail, "keyword");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_frame_in_which_nothing_moved_recomputes_nothing() {
        // `task-1666`'s rule: a caret blink, a repaint and a frame of idling are two integer
        // comparisons. What is checked is the answer that rule produces — the rows are the same
        // objects, untouched — because the comparisons themselves cannot be seen from outside.
        let (folder, mut app) = a_window("quill-completion-idle");
        typing(&mut app, "dra");
        let before = app.completion().cloned().expect("open");
        for _ in 0..10 {
            a_quiet_frame(&mut app);
        }
        assert_eq!(app.completion(), Some(&before), "ten idle frames changed nothing");
        std::fs::remove_dir_all(&folder).ok();
    }
}
