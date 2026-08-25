//! One open document: its text, its formatting, its caret and its undo history.
//!
//! Everything the editor does goes through `Document::apply`, which takes a `Command`. Keeping every
//! change behind one function is what makes undo and the stale layout flag reliable: there is one
//! place where a change is recorded and one place where the revision is bumped.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::cursor::{self, Selection};
use crate::layout::Layout;
use crate::rope::Rope;
use crate::style::{Align, CharStyle, Color, ParagraphStyle, ParagraphStyles, StyleChange, StyleSpans};

/// Something the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Type text, replacing the selection if there is one. Also used for paste.
    Insert(String),
    /// Backspace: delete the selection, or the cluster before the caret.
    DeleteBackward,
    /// Delete forwards: delete the selection, or the cluster after the caret.
    DeleteForward,
    /// Delete the word before the caret.
    DeleteWordBackward,
    MoveLeft { extend: bool },
    MoveRight { extend: bool },
    MoveWordLeft { extend: bool },
    MoveWordRight { extend: bool },
    MoveLineStart { extend: bool },
    MoveLineEnd { extend: bool },
    MoveDocumentStart { extend: bool },
    MoveDocumentEnd { extend: bool },
    SelectAll,
    /// Put the caret at a document offset, from a mouse click.
    PlaceCaret { offset: usize, extend: bool },
    /// Apply character formatting to the selection, or to the next text typed if nothing is selected.
    ApplyStyle(StyleChange),
    /// Turn bold on for the selection, or off if all of it is already bold.
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    ToggleStrikethrough,
    SetAlign(Align),
    SetLineSpacing(f32),
    Undo,
    Redo,
}

/// What kind of change the last command made, so that a run of typing collapses into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    None,
    Typing,
    Deleting,
    Other,
}

/// A whole document state, kept so that undo can restore it.
///
/// Undo restores a saved state rather than replaying an inverse operation. Replaying inverses is more
/// frugal with memory, but it has to reconstruct the formatting spans and the paragraph list that the
/// edit destroyed, and getting that wrong corrupts the document silently. Restoring a state cannot be
/// wrong. The cost is bounded by capping the history at `UNDO_LIMIT` states, and Quill opens plain
/// text files rather than very large ones.
#[derive(Debug, Clone)]
struct Snapshot {
    text: Rope,
    chars: StyleSpans,
    paragraphs: ParagraphStyles,
    selection: Selection,
}

const UNDO_LIMIT: usize = 256;

/// An open document.
#[derive(Debug, Clone)]
pub struct Document {
    text: Rope,
    chars: StyleSpans,
    paragraphs: ParagraphStyles,
    selection: Selection,
    /// Formatting chosen while nothing was selected, applied to the next text typed.
    pending: StyleChange,
    /// The horizontal position to aim for when moving up and down. Without this, moving down through a
    /// short line and on to a long one loses the original column.
    desired_x: Option<f32>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: EditKind,
    path: Option<PathBuf>,
    /// True when there are changes that have not been written to disk.
    modified: bool,
    /// Bumped on every change. The view compares it against the revision it last laid out to know
    /// whether the layout it holds is stale.
    revision: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            chars: StyleSpans::new(0, CharStyle::default()),
            paragraphs: ParagraphStyles::new(1),
            selection: Selection::caret(0),
            pending: StyleChange::default(),
            desired_x: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::None,
            path: None,
            modified: false,
            revision: 1,
        }
    }

    /// A document holding `text`, as if it had just been opened from disk.
    pub fn from_text(text: &str) -> Self {
        let rope = Rope::from_str(text);
        Self {
            chars: StyleSpans::new(rope.len_bytes(), CharStyle::default()),
            paragraphs: ParagraphStyles::new(rope.len_lines()),
            text: rope,
            ..Self::new()
        }
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        // Files written on Windows use a carriage return and a line feed for each line break. The
        // buffer stores line feeds only, so that offsets and line counts have one meaning.
        let text = text.replace("\r\n", "\n");
        let mut document = Self::from_text(&text);
        document.path = Some(path.to_owned());
        Ok(document)
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| std::io::Error::other("this document has no file to save to"))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.text.to_string())?;
        self.path = Some(path.to_owned());
        self.modified = false;
        Ok(())
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn chars(&self) -> &StyleSpans {
        &self.chars
    }

    pub fn paragraphs(&self) -> &ParagraphStyles {
        &self.paragraphs
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The text that is selected, which is what copy and cut put on the clipboard.
    pub fn selected_text(&self) -> String {
        self.text.byte_slice(self.selection.range())
    }

    /// The formatting that a toolbar should show as active: the formatting of the selection when it is
    /// uniform, and otherwise the formatting at the caret, with anything chosen but not yet typed
    /// applied on top.
    pub fn active_style(&self) -> CharStyle {
        let mut style = self.chars.style_at(self.selection.head).clone();
        if !self.selection.is_empty() {
            let range = self.selection.range();
            style = self.chars.style_at(range.start).clone();
            style.bold = self.chars.all_in(range.clone(), |s| s.bold);
            style.italic = self.chars.all_in(range.clone(), |s| s.italic);
            style.underline = self.chars.all_in(range.clone(), |s| s.underline);
            style.strikethrough = self.chars.all_in(range, |s| s.strikethrough);
        }
        self.pending.clone().apply_over(&mut style);
        style
    }

    /// Change the formatting of the whole document, and of text typed after it, without treating it as an
    /// edit.
    ///
    /// This is what `Settings -> Appearance -> Font` uses. Setting the family and the size there is a
    /// choice about how the document is shown rather than a change to it, so three things follow. It
    /// pushes nothing onto the undo history, because undoing a settings change from the editor would be
    /// surprising and because the setting is written to the settings file instead. It does not mark the
    /// document as having unsaved changes, because a file Quill saves is plain text and carries no
    /// formatting, so nothing about the file has changed. It does bump the revision, because the text has
    /// to be laid out again at the new size.
    ///
    /// Only the fields the change names are touched, so a word set in bold or in red stays bold or red
    /// when the family changes underneath it.
    pub fn set_base_style(&mut self, change: StyleChange) {
        if change.is_empty() {
            return;
        }
        let end = self.text.len_bytes();
        if end > 0 {
            self.chars.set(0..end, &change);
        }
        // Also the formatting for text typed next, so that an empty document and a document with text in
        // it behave the same way.
        change.apply_over_change(&mut self.pending);
        self.revision += 1;
    }

    /// Colour the document by what its text is: keywords, strings, comments and the rest.
    ///
    /// This is not an edit, and follows the same three rules [`Self::set_base_style`] does and for
    /// the same reasons. It pushes nothing onto the undo history, because undoing a colour scheme
    /// from the editor would be surprising. It does not mark the document as having unsaved
    /// changes, because what Quill saves is plain text and carries no formatting, so nothing about
    /// the file has changed. It does bump the revision, because the text has to be drawn again.
    ///
    /// The spans are given as ranges of bytes and a colour each. Everything not covered goes back to
    /// `base`, so switching from one language to another, or turning colouring off, does not leave
    /// the last language's colours behind.
    pub fn set_syntax(&mut self, base: Color, spans: &[(Range<usize>, Color)]) {
        let end = self.text.len_bytes();
        if end == 0 {
            return;
        }
        self.chars.set(0..end, &StyleChange::color(base));
        for (range, color) in spans {
            // A span from a highlighter that has not caught up with an edit yet would otherwise
            // panic inside the span list, and a colour is never worth a crash.
            let start = range.start.min(end);
            let stop = range.end.min(end);
            if start >= stop || !self.text.is_char_boundary(start) || !self.text.is_char_boundary(stop)
            {
                continue;
            }
            self.chars.set(start..stop, &StyleChange::color(*color));
        }
        self.revision += 1;
    }

    /// The paragraph formatting of the paragraph the caret is in.
    pub fn active_paragraph_style(&self) -> ParagraphStyle {
        self.paragraphs.get(self.text.byte_to_line(self.selection.head))
    }

    /// The paragraphs the selection touches.
    fn selected_paragraphs(&self) -> Range<usize> {
        let range = self.selection.range();
        let first = self.text.byte_to_line(range.start);
        let last = self.text.byte_to_line(range.end);
        first..last + 1
    }

    /// The text of the line the caret is on, and the document offset it starts at. Movement by
    /// grapheme cluster and by word works on this window rather than on the whole document.
    fn line_window(&self, offset: usize) -> (String, usize) {
        let line = self.text.byte_to_line(offset);
        let range = self.text.line_range(line);
        (self.text.byte_slice(range.clone()), range.start)
    }

    /// Run a command. Returns true when the document changed in a way that needs repainting.
    pub fn apply(&mut self, command: Command) -> bool {
        let before = self.revision;
        let selection_before = self.selection;
        match command {
            Command::Insert(text) => self.insert(&text),
            Command::DeleteBackward => self.delete_backward(),
            Command::DeleteForward => self.delete_forward(),
            Command::DeleteWordBackward => self.delete_word_backward(),
            Command::MoveLeft { extend } => self.move_horizontally(-1, extend, false),
            Command::MoveRight { extend } => self.move_horizontally(1, extend, false),
            Command::MoveWordLeft { extend } => self.move_horizontally(-1, extend, true),
            Command::MoveWordRight { extend } => self.move_horizontally(1, extend, true),
            Command::MoveLineStart { extend } => {
                let (_, start) = self.line_window(self.selection.head);
                self.selection.move_to(start, extend);
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::MoveLineEnd { extend } => {
                let (text, start) = self.line_window(self.selection.head);
                self.selection.move_to(start + text.len(), extend);
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::MoveDocumentStart { extend } => {
                self.selection.move_to(0, extend);
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::MoveDocumentEnd { extend } => {
                self.selection.move_to(self.text.len_bytes(), extend);
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::SelectAll => {
                self.selection = Selection::new(0, self.text.len_bytes());
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::PlaceCaret { offset, extend } => {
                let offset = self.clamp_to_boundary(offset);
                self.selection.move_to(offset, extend);
                self.desired_x = None;
                self.caret_moved(selection_before);
            }
            Command::ApplyStyle(change) => self.apply_style(change),
            Command::ToggleBold => {
                let on = !self.active_style().bold;
                self.apply_style(StyleChange::bold(on));
            }
            Command::ToggleItalic => {
                let on = !self.active_style().italic;
                self.apply_style(StyleChange::italic(on));
            }
            Command::ToggleUnderline => {
                let on = !self.active_style().underline;
                self.apply_style(StyleChange::underline(on));
            }
            Command::ToggleStrikethrough => {
                let on = !self.active_style().strikethrough;
                self.apply_style(StyleChange::strikethrough(on));
            }
            Command::SetAlign(align) => {
                let paragraphs = self.selected_paragraphs();
                self.push_undo(EditKind::Other);
                self.paragraphs.set(paragraphs, |p| p.align = align);
                self.mark_changed();
            }
            Command::SetLineSpacing(spacing) => {
                let paragraphs = self.selected_paragraphs();
                let spacing = spacing.clamp(0.5, 4.0);
                self.push_undo(EditKind::Other);
                self.paragraphs.set(paragraphs, |p| p.line_spacing = spacing);
                self.mark_changed();
            }
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
        }
        self.revision != before
    }

    /// Move the caret up or down `delta` lines, using a laid out document, because with word wrap one
    /// paragraph is several lines on screen and the text alone cannot say what is above a position.
    pub fn move_vertically(&mut self, layout: &Layout, delta: i32, extend: bool) -> bool {
        if layout.lines.is_empty() {
            return false;
        }
        let selection_before = self.selection;
        let caret = layout.caret_at(self.selection.head);
        // Remember the column across a run of vertical moves, so that going down through a short line
        // and on to a long one comes back to where it started.
        let target_x = self.desired_x.unwrap_or(caret.x);
        let line = (caret.line as i32 + delta).clamp(0, layout.lines.len() as i32 - 1) as usize;
        if line == caret.line {
            // Already at the top or the bottom: go to the start or the end of the text instead, which
            // is what pressing up on the first line does everywhere.
            let at = if delta < 0 { 0 } else { self.text.len_bytes() };
            self.selection.move_to(at, extend);
            self.caret_moved(selection_before);
            return true;
        }
        let offset = layout.offset_on_line_at_x(line, target_x);
        self.selection.move_to(self.clamp_to_boundary(offset), extend);
        self.desired_x = Some(target_x);
        self.last_edit = EditKind::Other;
        self.revision += 1;
        true
    }

    fn clamp_to_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len_bytes());
        while offset > 0 && !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// Record that the caret may have moved. A key press that moves nothing, such as the right arrow
    /// at the end of the text, must not bump the revision, or the view repaints for nothing.
    fn caret_moved(&mut self, before: Selection) {
        if self.selection != before {
            self.last_edit = EditKind::Other;
            self.revision += 1;
        }
    }

    fn mark_changed(&mut self) {
        self.modified = true;
        self.revision += 1;
        self.redo.clear();
    }

    fn push_undo(&mut self, kind: EditKind) {
        // A run of single character typing is one undo step, so that undo removes a word rather than a
        // letter. A caret move, a delete or a formatting change breaks the run.
        if kind != EditKind::None && kind == self.last_edit && kind == EditKind::Typing {
            return;
        }
        self.undo.push(Snapshot {
            text: self.text.clone(),
            chars: self.chars.clone(),
            paragraphs: self.paragraphs.clone(),
            selection: self.selection,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.last_edit = kind;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            chars: self.chars.clone(),
            paragraphs: self.paragraphs.clone(),
            selection: self.selection,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.chars = snapshot.chars;
        self.paragraphs = snapshot.paragraphs;
        self.selection = snapshot.selection;
    }

    fn undo(&mut self) {
        let Some(previous) = self.undo.pop() else {
            return;
        };
        let current = self.snapshot();
        self.restore(previous);
        self.redo.push(current);
        self.last_edit = EditKind::Other;
        self.modified = true;
        self.revision += 1;
    }

    fn redo(&mut self) {
        let Some(next) = self.redo.pop() else {
            return;
        };
        let current = self.snapshot();
        self.restore(next);
        self.undo.push(current);
        self.last_edit = EditKind::Other;
        self.modified = true;
        self.revision += 1;
    }

    fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let single = text.chars().count() == 1 && text != "\n";
        self.push_undo(if single { EditKind::Typing } else { EditKind::Other });
        let range = self.selection.range();
        // The formatting for the new text is read before the deletion, because deleting the selection
        // can remove the very span the new text should inherit from.
        let mut style = self.chars.style_for_insertion(range.start);
        self.pending.clone().apply_over(&mut style);
        if !range.is_empty() {
            self.remove_range(range.clone());
        }
        let at = range.start;
        let paragraph = self.text.byte_to_line(at);
        let line_breaks = text.bytes().filter(|b| *b == b'\n').count();

        self.text.insert(at, text);
        self.chars.insert(at, text.len());
        self.chars.set(at..at + text.len(), &style_as_change(&style));
        self.paragraphs.split(paragraph, line_breaks);

        self.selection.set_caret(at + text.len());
        self.pending = StyleChange::default();
        self.desired_x = None;
        self.mark_changed();
        self.last_edit = if single { EditKind::Typing } else { EditKind::Other };
    }

    fn remove_range(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let first = self.text.byte_to_line(range.start);
        let last = self.text.byte_to_line(range.end);
        self.text.remove(range.clone());
        self.chars.remove(range.clone());
        self.paragraphs.join(first, last);
        self.selection.set_caret(range.start);
    }

    fn delete_backward(&mut self) {
        self.push_undo(EditKind::Deleting);
        if !self.selection.is_empty() {
            let range = self.selection.range();
            self.remove_range(range);
        } else if self.selection.head > 0 {
            let (window, base) = self.line_window(self.selection.head);
            let from = if self.selection.head == base {
                // At the start of a line, backspace joins this line to the one before it.
                self.selection.head - 1
            } else {
                cursor::prev_grapheme(&window, base, self.selection.head)
            };
            self.remove_range(from..self.selection.head);
        } else {
            return;
        }
        self.desired_x = None;
        self.mark_changed();
        self.last_edit = EditKind::Deleting;
    }

    fn delete_forward(&mut self) {
        self.push_undo(EditKind::Deleting);
        if !self.selection.is_empty() {
            let range = self.selection.range();
            self.remove_range(range);
        } else if self.selection.head < self.text.len_bytes() {
            let (window, base) = self.line_window(self.selection.head);
            let to = if self.selection.head == base + window.len() {
                self.selection.head + 1
            } else {
                cursor::next_grapheme(&window, base, self.selection.head)
            };
            self.remove_range(self.selection.head..to);
        } else {
            return;
        }
        self.desired_x = None;
        self.mark_changed();
        self.last_edit = EditKind::Deleting;
    }

    fn delete_word_backward(&mut self) {
        self.push_undo(EditKind::Other);
        if !self.selection.is_empty() {
            let range = self.selection.range();
            self.remove_range(range);
        } else {
            let (window, base) = self.line_window(self.selection.head);
            let from = cursor::prev_word(&window, base, self.selection.head);
            if from == self.selection.head {
                return;
            }
            self.remove_range(from..self.selection.head);
        }
        self.desired_x = None;
        self.mark_changed();
    }

    fn move_horizontally(&mut self, direction: i32, extend: bool, by_word: bool) {
        let selection_before = self.selection;
        // With a selection and no shift held, an arrow key collapses to the near edge rather than
        // moving past it, which is what a writer expects.
        if !extend && !self.selection.is_empty() {
            let range = self.selection.range();
            let at = if direction < 0 { range.start } else { range.end };
            self.selection.set_caret(at);
            self.desired_x = None;
            self.caret_moved(selection_before);
            return;
        }
        let head = self.selection.head;
        let (window, base) = self.line_window(head);
        let target = match (direction < 0, by_word) {
            (true, false) => {
                if head == base && head > 0 {
                    head - 1 // cross the line break into the line above
                } else {
                    cursor::prev_grapheme(&window, base, head)
                }
            }
            (false, false) => {
                if head == base + window.len() && head < self.text.len_bytes() {
                    head + 1 // cross the line break into the line below
                } else {
                    cursor::next_grapheme(&window, base, head)
                }
            }
            (true, true) => {
                let at = cursor::prev_word(&window, base, head);
                if at == head && head > 0 {
                    head - 1
                } else {
                    at
                }
            }
            (false, true) => {
                let at = cursor::next_word(&window, base, head);
                if at == head && head < self.text.len_bytes() {
                    head + 1
                } else {
                    at
                }
            }
        };
        self.selection.move_to(target.min(self.text.len_bytes()), extend);
        self.desired_x = None;
        self.caret_moved(selection_before);
    }

    fn apply_style(&mut self, change: StyleChange) {
        if change.is_empty() {
            return;
        }
        if self.selection.is_empty() {
            // Nothing is selected, so remember the choice and apply it to the next text typed. This is
            // how pressing bold and then typing works in every word processor.
            change.apply_over_change(&mut self.pending);
            self.revision += 1;
            return;
        }
        self.push_undo(EditKind::Other);
        let range = self.selection.range();
        self.chars.set(range, &change);
        self.mark_changed();
    }
}

/// Turn a full style into a change that sets every field, so that inserted text is given exactly that
/// style rather than inheriting from its neighbours.
fn style_as_change(style: &CharStyle) -> StyleChange {
    StyleChange {
        family: Some(style.family.clone()),
        size: Some(style.size),
        bold: Some(style.bold),
        italic: Some(style.italic),
        underline: Some(style.underline),
        strikethrough: Some(style.strikethrough),
        color: Some(style.color),
    }
}

impl StyleChange {
    /// Apply this change on top of a style.
    fn apply_over(self, style: &mut CharStyle) {
        if let Some(family) = self.family {
            style.family = family;
        }
        if let Some(size) = self.size {
            style.size = size;
        }
        if let Some(bold) = self.bold {
            style.bold = bold;
        }
        if let Some(italic) = self.italic {
            style.italic = italic;
        }
        if let Some(underline) = self.underline {
            style.underline = underline;
        }
        if let Some(strikethrough) = self.strikethrough {
            style.strikethrough = strikethrough;
        }
        if let Some(color) = self.color {
            style.color = color;
        }
    }

    /// Fold this change into another one, so that pressing bold and then italic before typing keeps
    /// both.
    fn apply_over_change(self, other: &mut StyleChange) {
        if self.family.is_some() {
            other.family = self.family;
        }
        if self.size.is_some() {
            other.size = self.size;
        }
        if self.bold.is_some() {
            other.bold = self.bold;
        }
        if self.italic.is_some() {
            other.italic = self.italic;
        }
        if self.underline.is_some() {
            other.underline = self.underline;
        }
        if self.strikethrough.is_some() {
            other.strikethrough = self.strikethrough;
        }
        if self.color.is_some() {
            other.color = self.color;
        }
    }
}

#[cfg(test)]
mod base_style_tests {
    use super::*;
    use crate::style::Color;

    fn document_with_a_bold_red_word() -> Document {
        let mut document = Document::from_text("plain BOLD plain");
        document.apply(Command::PlaceCaret { offset: 6, extend: false });
        document.apply(Command::PlaceCaret { offset: 10, extend: true });
        document.apply(Command::ToggleBold);
        document.apply(Command::ApplyStyle(StyleChange::color(Color::RED)));
        document.apply(Command::PlaceCaret { offset: 0, extend: false });
        document
    }

    #[test]
    fn the_base_style_changes_the_family_and_size_of_the_whole_document() {
        let mut document = Document::from_text("two lines here\nand the second");
        document.set_base_style(StyleChange { size: Some(28.0), ..StyleChange::family("Courier".to_owned()) });
        for offset in [0, 5, 20, document.text().len_bytes() - 1] {
            let style = document.chars().style_at(offset);
            assert_eq!(style.family, "Courier", "offset {offset} should have the new family");
            assert_eq!(style.size, 28.0, "offset {offset} should have the new size");
        }
    }

    #[test]
    fn the_base_style_leaves_formatting_it_does_not_name_alone() {
        let mut document = document_with_a_bold_red_word();
        document.set_base_style(StyleChange::family("Courier".to_owned()));
        let word = document.chars().style_at(7);
        assert!(word.bold, "the word that was made bold should still be bold");
        assert_eq!(word.color, Color::RED, "and still red");
        assert_eq!(word.family, "Courier", "with the new family under it");
        assert!(!document.chars().style_at(0).bold, "the words either side are still not bold");
    }

    #[test]
    fn the_base_style_is_not_an_edit() {
        let mut document = Document::from_text("nothing here has been edited");
        assert!(!document.is_modified());
        let revision = document.revision();
        document.set_base_style(StyleChange::size(30.0));
        assert!(
            !document.is_modified(),
            "a font setting is not a change to the file, which holds no formatting"
        );
        assert!(!document.can_undo(), "and there is nothing to undo");
        assert!(document.revision() > revision, "but the text has to be laid out again");
    }

    #[test]
    fn the_base_style_applies_to_text_typed_afterwards() {
        let mut document = Document::new();
        document.set_base_style(StyleChange::size(30.0));
        document.apply(Command::Insert("typed after the setting".to_owned()));
        assert_eq!(document.chars().style_at(3).size, 30.0);
    }

    #[test]
    fn an_empty_change_does_nothing_at_all() {
        let mut document = Document::from_text("unchanged");
        let revision = document.revision();
        document.set_base_style(StyleChange::default());
        assert_eq!(document.revision(), revision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::metrics::FixedMetrics;
    use crate::style::Color;

    fn typed(text: &str) -> Document {
        let mut document = Document::new();
        document.apply(Command::Insert(text.to_owned()));
        document
    }

    fn lay_out(document: &Document, width: f32) -> Layout {
        layout(
            document.text(),
            document.chars(),
            document.paragraphs(),
            &FixedMetrics::default(),
            width,
        )
    }

    #[test]
    fn a_new_document_is_empty_and_unmodified() {
        let document = Document::new();
        assert_eq!(document.text().to_string(), "");
        assert!(!document.is_modified());
        assert!(!document.can_undo());
        assert_eq!(document.selection(), Selection::caret(0));
    }

    #[test]
    fn typing_puts_the_caret_after_what_was_typed() {
        let document = typed("hello");
        assert_eq!(document.text().to_string(), "hello");
        assert_eq!(document.selection(), Selection::caret(5));
        assert!(document.is_modified());
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut document = typed("hello world");
        document.apply(Command::PlaceCaret { offset: 0, extend: false });
        document.apply(Command::PlaceCaret { offset: 5, extend: true });
        document.apply(Command::Insert("goodbye".to_owned()));
        assert_eq!(document.text().to_string(), "goodbye world");
        assert_eq!(document.selection(), Selection::caret(7));
    }

    #[test]
    fn backspace_removes_one_cluster_not_one_byte() {
        let mut document = typed("cafe\u{0301}");
        assert_eq!(document.text().len_bytes(), 6);
        document.apply(Command::DeleteBackward);
        assert_eq!(document.text().to_string(), "caf", "the e and its accent went together");
    }

    #[test]
    fn backspace_at_the_start_of_a_line_joins_it_to_the_line_before() {
        let mut document = typed("one\ntwo");
        document.apply(Command::PlaceCaret { offset: 4, extend: false });
        document.apply(Command::DeleteBackward);
        assert_eq!(document.text().to_string(), "onetwo");
        assert_eq!(document.text().len_lines(), 1);
    }

    #[test]
    fn delete_forward_at_the_end_of_a_line_pulls_the_next_line_up() {
        let mut document = typed("one\ntwo");
        document.apply(Command::PlaceCaret { offset: 3, extend: false });
        document.apply(Command::DeleteForward);
        assert_eq!(document.text().to_string(), "onetwo");
    }

    #[test]
    fn arrow_keys_move_the_caret_and_shift_extends_the_selection() {
        let mut document = typed("hello");
        document.apply(Command::MoveLeft { extend: false });
        assert_eq!(document.selection(), Selection::caret(4));
        document.apply(Command::MoveLeft { extend: true });
        assert_eq!(document.selection(), Selection::new(4, 3));
        assert_eq!(document.selected_text(), "l");
        document.apply(Command::MoveRight { extend: true });
        assert!(document.selection().is_empty());
    }

    #[test]
    fn an_arrow_key_with_a_selection_collapses_to_the_near_edge() {
        let mut document = typed("hello world");
        document.apply(Command::PlaceCaret { offset: 2, extend: false });
        document.apply(Command::PlaceCaret { offset: 8, extend: true });
        document.apply(Command::MoveLeft { extend: false });
        assert_eq!(document.selection(), Selection::caret(2), "collapses to the start, does not move past it");

        document.apply(Command::PlaceCaret { offset: 2, extend: false });
        document.apply(Command::PlaceCaret { offset: 8, extend: true });
        document.apply(Command::MoveRight { extend: false });
        assert_eq!(document.selection(), Selection::caret(8));
    }

    #[test]
    fn left_and_right_cross_line_breaks() {
        let mut document = typed("ab\ncd");
        document.apply(Command::PlaceCaret { offset: 2, extend: false });
        document.apply(Command::MoveRight { extend: false });
        assert_eq!(document.selection().head, 3, "moved over the line break");
        document.apply(Command::MoveLeft { extend: false });
        assert_eq!(document.selection().head, 2);
    }

    #[test]
    fn line_start_and_line_end_stay_on_the_line() {
        let mut document = typed("one\ntwo three");
        document.apply(Command::PlaceCaret { offset: 6, extend: false });
        document.apply(Command::MoveLineStart { extend: false });
        assert_eq!(document.selection().head, 4);
        document.apply(Command::MoveLineEnd { extend: false });
        assert_eq!(document.selection().head, 13);
    }

    #[test]
    fn word_movement_jumps_over_words() {
        let mut document = typed("the quick brown");
        document.apply(Command::MoveDocumentStart { extend: false });
        document.apply(Command::MoveWordRight { extend: false });
        assert_eq!(document.selection().head, 3);
        document.apply(Command::MoveWordRight { extend: false });
        assert_eq!(document.selection().head, 9);
        document.apply(Command::MoveWordLeft { extend: false });
        assert_eq!(document.selection().head, 4);
    }

    #[test]
    fn select_all_selects_the_whole_document() {
        let mut document = typed("one\ntwo");
        document.apply(Command::SelectAll);
        assert_eq!(document.selected_text(), "one\ntwo");
    }

    #[test]
    fn moving_down_and_back_up_returns_to_the_same_column() {
        //   long line
        //   ab            <- short
        //   another long line
        let mut document = typed("0123456789\nab\n0123456789");
        document.apply(Command::PlaceCaret { offset: 8, extend: false });
        let placed = lay_out(&document, 1000.0);
        assert_eq!(placed.caret_at(8).x, 80.0);

        document.move_vertically(&placed, 1, false);
        assert_eq!(document.selection().head, 13, "the short line ends before column 8");

        document.move_vertically(&placed, 1, false);
        assert_eq!(
            document.selection().head, 22,
            "the remembered column brings the caret back to column 8 on the long line"
        );
    }

    #[test]
    fn moving_up_from_the_first_line_goes_to_the_start_of_the_document() {
        let mut document = typed("one\ntwo");
        document.apply(Command::PlaceCaret { offset: 2, extend: false });
        let placed = lay_out(&document, 1000.0);
        document.move_vertically(&placed, -1, false);
        assert_eq!(document.selection().head, 0);
    }

    #[test]
    fn moving_down_from_the_last_line_goes_to_the_end_of_the_document() {
        let mut document = typed("one\ntwo");
        document.apply(Command::PlaceCaret { offset: 5, extend: false });
        let placed = lay_out(&document, 1000.0);
        document.move_vertically(&placed, 1, false);
        assert_eq!(document.selection().head, 7);
    }

    #[test]
    fn moving_down_through_wrapped_lines_stays_in_one_paragraph() {
        let mut document = typed("aa bb cc dd ee ff gg hh");
        document.apply(Command::MoveDocumentStart { extend: false });
        let placed = lay_out(&document, 60.0);
        assert!(placed.lines.len() > 2, "the paragraph must wrap for this test to mean anything");
        document.move_vertically(&placed, 1, false);
        assert!(document.selection().head > 0, "moved onto the second visual line of one paragraph");
        assert_eq!(document.text().byte_to_line(document.selection().head), 0, "still paragraph 0");
    }

    #[test]
    fn bold_applies_to_the_selection_only() {
        let mut document = typed("plain bold plain");
        document.apply(Command::PlaceCaret { offset: 6, extend: false });
        document.apply(Command::PlaceCaret { offset: 10, extend: true });
        document.apply(Command::ToggleBold);
        assert!(document.chars().style_at(7).bold);
        assert!(!document.chars().style_at(2).bold);
        assert!(!document.chars().style_at(12).bold);
    }

    #[test]
    fn the_bold_button_turns_bold_off_when_the_whole_selection_is_bold() {
        let mut document = typed("word");
        document.apply(Command::SelectAll);
        document.apply(Command::ToggleBold);
        assert!(document.active_style().bold);
        document.apply(Command::ToggleBold);
        assert!(!document.active_style().bold, "pressing it again turns it off");
        assert!(!document.chars().style_at(2).bold);
    }

    #[test]
    fn the_bold_button_turns_bold_on_for_a_partly_bold_selection() {
        let mut document = typed("aaaabbbb");
        document.apply(Command::PlaceCaret { offset: 0, extend: false });
        document.apply(Command::PlaceCaret { offset: 4, extend: true });
        document.apply(Command::ToggleBold);
        document.apply(Command::SelectAll);
        assert!(!document.active_style().bold, "a half bold selection reports as not bold");
        document.apply(Command::ToggleBold);
        assert!(document.chars().style_at(1).bold);
        assert!(document.chars().style_at(6).bold, "the whole selection is now bold");
    }

    #[test]
    fn formatting_chosen_with_nothing_selected_applies_to_the_next_text_typed() {
        let mut document = typed("plain ");
        document.apply(Command::ToggleBold);
        assert!(document.text().to_string() == "plain ", "pressing bold types nothing");
        document.apply(Command::Insert("bold".to_owned()));
        assert!(document.chars().style_at(8).bold);
        assert!(!document.chars().style_at(2).bold);
        document.apply(Command::Insert(" after".to_owned()));
        assert!(document.chars().style_at(13).bold, "the run carries on until it is turned off");
    }

    #[test]
    fn two_formatting_choices_before_typing_both_apply() {
        let mut document = Document::new();
        document.apply(Command::ToggleBold);
        document.apply(Command::ApplyStyle(StyleChange::color(Color::RED)));
        document.apply(Command::Insert("x".to_owned()));
        let style = document.chars().style_at(0);
        assert!(style.bold);
        assert_eq!(style.color, Color::RED);
    }

    #[test]
    fn setting_the_colour_of_a_selection_keeps_its_bold() {
        let mut document = typed("word");
        document.apply(Command::SelectAll);
        document.apply(Command::ToggleBold);
        document.apply(Command::ApplyStyle(StyleChange::color(Color::BLUE)));
        let style = document.chars().style_at(2);
        assert!(style.bold, "the colour change must not clear bold");
        assert_eq!(style.color, Color::BLUE);
    }

    #[test]
    fn text_typed_inside_a_formatted_run_inherits_its_formatting() {
        let mut document = typed("bold");
        document.apply(Command::SelectAll);
        document.apply(Command::ToggleBold);
        document.apply(Command::PlaceCaret { offset: 2, extend: false });
        document.apply(Command::Insert("XX".to_owned()));
        assert_eq!(document.text().to_string(), "boXXld");
        assert!(document.chars().style_at(3).bold);
    }

    #[test]
    fn alignment_applies_to_every_paragraph_the_selection_touches() {
        let mut document = typed("one\ntwo\nthree");
        document.apply(Command::PlaceCaret { offset: 5, extend: false });
        document.apply(Command::SetAlign(Align::Center));
        assert_eq!(document.paragraphs().get(1).align, Align::Center);
        assert_eq!(document.paragraphs().get(0).align, Align::Left, "only the caret's paragraph");

        document.apply(Command::SelectAll);
        document.apply(Command::SetAlign(Align::Right));
        for paragraph in 0..3 {
            assert_eq!(document.paragraphs().get(paragraph).align, Align::Right);
        }
    }

    #[test]
    fn line_spacing_is_kept_within_sensible_bounds() {
        let mut document = typed("text");
        document.apply(Command::SetLineSpacing(2.0));
        assert_eq!(document.active_paragraph_style().line_spacing, 2.0);
        document.apply(Command::SetLineSpacing(99.0));
        assert_eq!(document.active_paragraph_style().line_spacing, 4.0, "clamped");
        document.apply(Command::SetLineSpacing(0.0));
        assert_eq!(document.active_paragraph_style().line_spacing, 0.5, "clamped");
    }

    #[test]
    fn a_new_paragraph_inherits_the_formatting_of_the_one_it_came_from() {
        let mut document = typed("centred");
        document.apply(Command::SetAlign(Align::Center));
        document.apply(Command::MoveDocumentEnd { extend: false });
        document.apply(Command::Insert("\n".to_owned()));
        document.apply(Command::Insert("second".to_owned()));
        assert_eq!(document.text().len_lines(), 2);
        assert_eq!(document.paragraphs().len(), 2);
        assert_eq!(document.paragraphs().get(1).align, Align::Center);
    }

    #[test]
    fn deleting_a_line_break_joins_the_paragraphs_and_keeps_the_first_ones_formatting() {
        let mut document = typed("one\ntwo");
        document.apply(Command::PlaceCaret { offset: 0, extend: false });
        document.apply(Command::SetAlign(Align::Right));
        document.apply(Command::PlaceCaret { offset: 5, extend: false });
        document.apply(Command::SetAlign(Align::Center));
        document.apply(Command::PlaceCaret { offset: 4, extend: false });
        document.apply(Command::DeleteBackward);
        assert_eq!(document.text().to_string(), "onetwo");
        assert_eq!(document.paragraphs().len(), 1);
        assert_eq!(document.paragraphs().get(0).align, Align::Right);
    }

    #[test]
    fn undo_removes_a_whole_run_of_typing_not_one_letter() {
        let mut document = Document::new();
        for letter in "hello".chars() {
            document.apply(Command::Insert(letter.to_string()));
        }
        assert_eq!(document.text().to_string(), "hello");
        document.apply(Command::Undo);
        assert_eq!(document.text().to_string(), "", "one undo removes the whole run");
    }

    #[test]
    fn a_caret_move_breaks_a_run_of_typing_into_two_undo_steps() {
        let mut document = Document::new();
        for letter in "one".chars() {
            document.apply(Command::Insert(letter.to_string()));
        }
        document.apply(Command::MoveDocumentStart { extend: false });
        for letter in "two".chars() {
            document.apply(Command::Insert(letter.to_string()));
        }
        assert_eq!(document.text().to_string(), "twoone");
        document.apply(Command::Undo);
        assert_eq!(document.text().to_string(), "one");
        document.apply(Command::Undo);
        assert_eq!(document.text().to_string(), "");
    }

    #[test]
    fn undo_restores_formatting_as_well_as_text() {
        let mut document = typed("word");
        document.apply(Command::SelectAll);
        document.apply(Command::ToggleBold);
        assert!(document.chars().style_at(2).bold);
        document.apply(Command::Undo);
        assert!(!document.chars().style_at(2).bold, "undo puts the formatting back too");
    }

    #[test]
    fn undo_restores_alignment() {
        let mut document = typed("one\ntwo");
        document.apply(Command::SelectAll);
        document.apply(Command::SetAlign(Align::Center));
        document.apply(Command::Undo);
        assert_eq!(document.paragraphs().get(0).align, Align::Left);
    }

    #[test]
    fn redo_puts_back_what_undo_removed() {
        let mut document = typed("hello");
        document.apply(Command::Undo);
        assert_eq!(document.text().to_string(), "");
        document.apply(Command::Redo);
        assert_eq!(document.text().to_string(), "hello");
    }

    #[test]
    fn a_new_edit_clears_the_redo_history() {
        let mut document = typed("hello");
        document.apply(Command::Undo);
        document.apply(Command::Insert("different".to_owned()));
        assert!(!document.can_redo());
        document.apply(Command::Redo);
        assert_eq!(document.text().to_string(), "different", "redo did nothing");
    }

    #[test]
    fn undo_on_a_fresh_document_does_nothing() {
        let mut document = Document::new();
        document.apply(Command::Undo);
        assert_eq!(document.text().to_string(), "");
    }

    #[test]
    fn the_undo_history_is_capped() {
        let mut document = Document::new();
        for i in 0..(UNDO_LIMIT + 50) {
            // A caret move between each insert makes every one its own undo step.
            document.apply(Command::Insert(format!("{i} ")));
            document.apply(Command::MoveDocumentStart { extend: false });
        }
        assert!(document.undo.len() <= UNDO_LIMIT, "history grew to {}", document.undo.len());
    }

    #[test]
    fn the_revision_changes_on_every_edit_so_the_view_knows_to_lay_out_again() {
        let mut document = Document::new();
        let start = document.revision();
        assert!(document.apply(Command::Insert("a".to_owned())));
        assert!(document.revision() > start);
        let after = document.revision();
        assert!(!document.apply(Command::MoveRight { extend: false }), "already at the end");
        assert_eq!(document.revision(), after, "a move that changes nothing does not bump it");
    }

    #[test]
    fn opening_a_file_written_on_windows_normalises_its_line_breaks() {
        let directory = std::env::temp_dir().join("quill-core-test-crlf");
        std::fs::create_dir_all(&directory).expect("make the test directory");
        let path = directory.join("windows.txt");
        std::fs::write(&path, "one\r\ntwo\r\n").expect("write the test file");
        let document = Document::open(&path).expect("open it");
        assert_eq!(document.text().to_string(), "one\ntwo\n");
        assert_eq!(document.text().len_lines(), 3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn saving_writes_the_text_and_clears_the_modified_mark() {
        let directory = std::env::temp_dir().join("quill-core-test-save");
        std::fs::create_dir_all(&directory).expect("make the test directory");
        let path = directory.join("out.md");
        let mut document = typed("# heading\n\nbody");
        assert!(document.is_modified());
        document.save_as(&path).expect("save it");
        assert!(!document.is_modified());
        assert_eq!(std::fs::read_to_string(&path).expect("read it back"), "# heading\n\nbody");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn every_edit_keeps_the_formatting_and_paragraph_counts_in_step_with_the_text() {
        // The three structures have to agree: the spans must cover exactly the document's bytes and
        // there must be one paragraph entry per line. Anything that breaks that corrupts the document
        // later, so it is checked after every command here.
        let mut document = Document::new();
        let commands = [
            Command::Insert("the quick brown fox\njumps over\n".to_owned()),
            Command::SelectAll,
            Command::ToggleBold,
            Command::PlaceCaret { offset: 4, extend: false },
            Command::PlaceCaret { offset: 9, extend: true },
            Command::ApplyStyle(StyleChange::size(28.0)),
            Command::Insert("SLOW".to_owned()),
            Command::SetAlign(Align::Center),
            Command::Insert("\n\nmore".to_owned()),
            Command::DeleteBackward,
            Command::DeleteWordBackward,
            Command::SelectAll,
            Command::DeleteForward,
            Command::Insert("start again".to_owned()),
            Command::Undo,
            Command::Undo,
            Command::Redo,
        ];
        for (index, command) in commands.into_iter().enumerate() {
            document.apply(command.clone());
            assert_eq!(
                document.chars().total_len(),
                document.text().len_bytes(),
                "the formatting spans no longer cover the text after command {index}: {command:?}"
            );
            assert_eq!(
                document.paragraphs().len(),
                document.text().len_lines(),
                "the paragraph count no longer matches the line count after command {index}: {command:?}"
            );
            assert!(
                document.selection().head <= document.text().len_bytes(),
                "the caret is past the end of the text after command {index}: {command:?}"
            );
            // Laying out must not panic on any of these states.
            let _ = lay_out(&document, 120.0);
        }
    }
}
