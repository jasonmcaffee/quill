//! The files that are open, which pane each is in, and which of them is showing.
//!
//! Everything that belongs to one file rather than to the window lives in an [`OpenFile`]: the
//! document, how far it is scrolled, which of the three ways of looking at it is showing, what git
//! has to say about it, and — since `task-1664` — what has been laid out for it and which pane it
//! is in. The window keeps one of these for each tab.
//!
//! There is always at least one. A window that has just started holds one tab with an untitled
//! document in it, and closing the last tab leaves another untitled one rather than a window with no
//! document and a special case everywhere for it.
//!
//! ## The transient tab
//!
//! At most one tab in each pane is transient, and it is the one a single click in the explorer
//! reuses. Clicking a second file replaces its contents instead of adding a tab, so reading through
//! a folder does not leave thirty tabs behind. Double clicking a file, or typing into the transient
//! tab, makes it permanent — editing a file you were only glancing at plainly means you meant to
//! open it. This is what IntelliJ does, and it is what `task-1649` describes when it says a double
//! click opens a file in a new tab.
//!
//! ## A tab holding a picture
//!
//! `task-1658` asks to be able to look at an image, so a tab holds either text or a picture. It still
//! holds a `Document` either way — one made by `Document::at_path`, which carries the path and nothing
//! else — so the tab is named after the file, the explorer marks the row as open and the tab strip needs
//! no second kind of tab. What tells them apart is [`OpenFile::picture`], and the window asks that one
//! question in the two places it matters: what to draw in the editing area, and what to refuse to save.

use std::path::{Path, PathBuf};

use quill_core::{Anchor, Document, Layout, Preview, Selection};

use crate::app::{PlacedDiagram, PlacedPicture, ViewMode};
use crate::components::gutter::{BlameRow, Change};
use crate::services::picture::Picture;

/// What has been worked out about one file and kept between frames: the laid out text, and the
/// Markdown preview with its pictures and diagrams.
///
/// These were ten fields on `QuillApp` until `task-1664`, because there was one editing area and so
/// one file being drawn. With panes there are several, at several widths, and a single set of them
/// is not slow so much as **wrong** in the way a cache is wrong: the first pane lays its file out,
/// the second lays its own over the top, and the next frame does it again, so a large file is laid
/// out from scratch twice a frame for ever.
///
/// Keyed by the thing they describe, they are correct without anybody thinking about it: each pane's
/// width is stable from frame to frame, so nothing is laid out that has not changed. It also all but
/// removes `stale`, which existed because the revision counts changes to *one* document and two
/// documents could be at the same number — a shared cache confusing two files. A cache on the file
/// cannot confuse two files, so what is left of the flag is the one case where a tab's document is
/// **replaced** in place and the cache belongs to the document that has gone.
#[derive(Default)]
pub struct Cached {
    /// The text as it was last laid out.
    pub layout: Layout,
    pub laid_out_revision: u64,
    pub laid_out_width: f32,
    /// Set when the layout has to be worked out again whatever the revision says.
    pub stale: bool,
    /// The Markdown preview, worked out from the source and kept until the source changes.
    pub preview: Option<Preview>,
    pub preview_layout: Layout,
    pub preview_revision: u64,
    pub preview_width: f32,
    /// Where each of the preview's pictures is drawn, worked out with the preview and drawn from
    /// every frame.
    pub preview_pictures: Vec<PlacedPicture>,
    /// The diagrams in the preview, in the order they appear.
    pub preview_diagrams: Vec<PlacedDiagram>,
    /// What this file's live text defines and where its words are, worked out from it and kept
    /// until the text changes.
    ///
    /// The ownership rule `task-1675` follows: **a file that is open is owned by its `Document`**,
    /// so the project's index deliberately holds nothing for it and this is the answer instead.
    /// Keyed on `text_revision`, the same key `colour_the_file` is keyed on, so a caret move
    /// recomputes nothing.
    pub symbols: Option<crate::app::symbols::TabSymbols>,
    /// What in this file could be collapsed, read from its live text and kept until that text
    /// changes. Keyed on `text_revision`, the same key the two above are keyed on.
    ///
    /// Which of them *are* collapsed is not here: that is state rather than a reading, so it lives
    /// in the `Document` where the two functions that move bytes can move it.
    pub fold_regions: Option<crate::app::folding::TabRegions>,
    /// This file's comments and strings, and the `text_revision` they were read at.
    ///
    /// A by-product of `colour_the_file`, which already runs `syntax::scan` over the same text at
    /// the same revision. Reading the blocks that could be collapsed needs exactly that and nothing
    /// else from a tokeniser, and a second pass over a 273 kilobyte file is 2.5 ms of every
    /// keystroke — `task-1666`'s rule, applied to the pass `task-1686` would otherwise have added.
    pub fold_tokens: Option<(u64, quill_core::folding::Tokens)>,
    /// The `fold_revision` the layout was built at, beside the text revision it was built at.
    ///
    /// A second key rather than folding into the first, because collapsing a block changes the
    /// layout and nothing else: keyed on `text_revision` a fold would re-colour the file and rebuild
    /// the Markdown preview. See `tasks/task-1686-folding-tdd.md` section 5.1.
    pub laid_out_folds: u64,
}

impl Cached {
    /// Nothing has been worked out yet, so everything has to be.
    fn fresh() -> Self {
        Self { stale: true, ..Self::default() }
    }
}

/// A place in a view that is to stay where it is while the text is laid out again.
///
/// Zooming changes how tall every line is, so a scroll position — a number of points down the
/// document — means something different afterwards, and the reader is left looking at a different
/// part of the file. What is remembered instead is the text that was under a point on the screen
/// and how far down the view that point was; putting the two back together once the new layout
/// exists gives the scroll position that leaves that text exactly where it was.
///
/// Taken before the font size changes, because it has to describe the layout the reader can still
/// see, and used up on the first frame the file is laid out again — which for a file in a pane that
/// is not on the screen may be a good while later, and is still the right answer, because a file
/// that has not been laid out again has not moved.
#[derive(Debug, Clone, Copy)]
pub struct ViewAnchor {
    /// The text that is to stay put.
    pub at: Anchor,
    /// How far below the top of the view it was, in points.
    pub above: f32,
}

/// One open file, and everything about it that is not about the window.
pub struct OpenFile {
    pub document: Document,
    pub view_mode: ViewMode,
    /// How far the source is scrolled.
    pub scroll: f32,
    /// How far the Markdown preview is scrolled, which is separate from the source.
    pub preview_scroll: f32,
    /// What the source is to be scrolled back to once it has been laid out at a new font size, and
    /// the same for the preview. See [`ViewAnchor`].
    pub zoom_anchor: Option<ViewAnchor>,
    pub preview_anchor: Option<ViewAnchor>,
    /// What is selected in this tab's Markdown preview, as a range into the preview's own text.
    ///
    /// On the tab beside the scroll position it lives with, rather than on the window, because a
    /// preview belongs to a file and two panes can be showing two of them. It is emptied when the
    /// preview is worked out again, since a byte range into text that has been rebuilt means
    /// nothing — see `QuillApp::refresh_preview`.
    pub preview_selection: Selection,
    /// One row a paragraph, once this file has been annotated with git blame.
    pub blame: Option<Vec<BlameRow>>,
    /// Which paragraphs differ from the version git has.
    pub line_changes: Vec<(usize, Change)>,
    /// True while this is the tab a single click reuses.
    pub transient: bool,
    /// Set once git has been asked what it thinks of this file, so it is not asked every frame.
    pub git_asked: bool,
    /// The picture, when this tab holds one rather than text.
    pub picture: Option<Picture>,
    /// The revision this file's marked passages were last pushed into `services::file_marks` at.
    ///
    /// A document that has not changed since it was last pushed cannot have gained a mark, so this
    /// makes keeping the store up to date one integer comparison a tab a frame rather than a
    /// comparison of two lists.
    pub marked_revision: Option<u64>,
    /// The revision this file was last coloured at, so a plugin's syntax colouring is not run twice
    /// for one revision. One per file rather than one for the window, so the file in the second pane
    /// is coloured too.
    pub coloured_revision: Option<u64>,
    /// Where the diagram has been moved and scaled to, for a tab holding a Mermaid file.
    ///
    /// Beside `preview_scroll` rather than instead of it, because they are two different ways of
    /// moving about: the Markdown preview scrolls like text, and a diagram is panned and zoomed like
    /// a picture.
    pub diagram: crate::components::diagram_view::View,
    /// Which pane this tab is in, counting from the left. See [`OpenFiles`].
    pub pane: usize,
    /// When this tab was last shown, from `OpenFiles`' own counter. The tab showing in a pane is the
    /// one in it with the highest stamp.
    pub shown_at: u64,
    /// What has been laid out for this file, kept between frames.
    pub cached: Cached,
}

impl OpenFile {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            view_mode: ViewMode::Raw,
            scroll: 0.0,
            preview_scroll: 0.0,
            zoom_anchor: None,
            preview_anchor: None,
            preview_selection: Selection::caret(0),
            blame: None,
            line_changes: Vec::new(),
            transient: false,
            git_asked: false,
            picture: None,
            marked_revision: None,
            coloured_revision: None,
            diagram: crate::components::diagram_view::View::default(),
            pane: 0,
            shown_at: 0,
            cached: Cached::fresh(),
        }
    }

    /// A tab holding a picture rather than text.
    pub fn picture(path: &Path) -> Self {
        Self { picture: Some(Picture::open(path)), ..Self::new(Document::at_path(path)) }
    }

    /// True when this tab holds a picture, which is what decides how the editing area is drawn and
    /// what `Save` refuses to do.
    pub fn is_picture(&self) -> bool {
        self.picture.is_some()
    }

    /// What the tab is called: the file's name, or `untitled` when it has never been saved.
    pub fn name(&self) -> String {
        self.document
            .path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_owned())
    }

    pub fn path(&self) -> Option<&Path> {
        self.document.path()
    }

    /// Git's idea of this file is about the file on disk, so it is thrown away when the file
    /// changes underneath it or a different file takes the tab.
    pub fn forget_git(&mut self) {
        self.blame = None;
        self.line_changes.clear();
        self.git_asked = false;
    }

    /// A different document has taken this tab, so everything worked out from the last one goes.
    ///
    /// The revision starts again at one for the document that has arrived, so comparing revisions
    /// alone would leave the new file wearing the old one's layout and the old one's colours.
    pub fn forget_what_was_worked_out(&mut self) {
        self.cached = Cached::fresh();
        self.coloured_revision = None;
    }

    /// A different document has taken this tab, so where the last one was being read means nothing.
    ///
    /// Separate from [`Self::forget_what_was_worked_out`] because that is also how showing a tab
    /// throws away what was laid out for it, and a tab being shown is a tab whose document is the
    /// one it always had: an anchor thrown away there is a tab that jumps the next time the font
    /// changes while it is not the one on the screen, which is the fault `task-1672` is about.
    pub fn forget_where_it_was_being_read(&mut self) {
        self.zoom_anchor = None;
        self.preview_anchor = None;
    }

    /// Remember where each view is, so a change of font size can put it back.
    ///
    /// The top of the view, which is what a person means by "do not move the file about" when the
    /// size is changed from the Settings window or from a tab they are not looking at. A zoom over
    /// the text asks for a point of its own — the pointer, or the caret — and sets that first,
    /// which is why an anchor already taken is left alone: the one nearest to what the reader is
    /// actually doing wins, and both describe the layout as it is now.
    pub fn anchor_the_views(&mut self) {
        if self.zoom_anchor.is_none() {
            let at = self.cached.layout.anchor_at_y(self.scroll);
            self.zoom_anchor = Some(ViewAnchor { at, above: 0.0 });
        }
        if self.preview_anchor.is_none() {
            let at = self.cached.preview_layout.anchor_at_y(self.preview_scroll);
            self.preview_anchor = Some(ViewAnchor { at, above: 0.0 });
        }
    }
}

/// The open files, which pane each is in, and which pane has the keyboard.
///
/// ## Panes
///
/// `task-1664` asks for IntelliJ's split view: the editing area cut into panes side by side, each
/// with its own tabs, so several files are on the screen at once. A pane is **a set of tabs and
/// which of them is showing**, and everything else a person would call the state of an editor —
/// the scroll position, the view mode, the blame, the laid out text — is already on the tab.
///
/// Which pane a tab is in is **written on the tab**, as [`OpenFile::pane`], rather than held as a
/// list of indices in a pane. Every index into `files` shifts when a tab is opened or closed, so a
/// pane holding indices would have to be fixed up by all seven of the operations below, and a
/// fix-up is the sort of thing that is right for a month. A number on the tab survives every
/// shuffle of the vector without a line of maintenance.
///
/// Which tab is *showing* in a pane is answered the same way. [`OpenFile::shown_at`] is stamped
/// from `clock` each time a tab is shown, and the tab showing in a pane is the one in it with the
/// highest stamp. That is a walk of a handful of integers, and it gives the right answer for free
/// in the case that would otherwise need thinking about: close the tab that is showing and the one
/// that comes forward is the one you were looking at before it, which is what IntelliJ does.
///
/// Two invariants are kept by [`Self::tidy`] after every change, and asserted in the tests:
///
/// - **Panes are numbered `0..panes` with no gaps**, so a pane can be found by counting from the
///   left and drawn in that order.
/// - **No pane is empty.** A pane that loses its last tab is removed, except the last remaining
///   pane, which is left with a fresh untitled tab — the rule [`Self::close`] already keeps for the
///   window as a whole, so nothing that draws a pane needs a special case for an empty one.
pub struct OpenFiles {
    files: Vec<OpenFile>,
    /// How many panes the editing area is divided into. Never less than one.
    panes: usize,
    /// Which pane has the keyboard. Always less than `panes`.
    focus: usize,
    /// Each pane's share of the editing area's width, in the same order. Sums to one.
    ///
    /// A fraction rather than a measurement so that opening the project on a screen of another size
    /// gives the same proportions rather than the same points.
    widths: Vec<f32>,
    /// Stamps [`OpenFile::shown_at`]. Counts up and is never reset.
    clock: u64,
}

impl OpenFiles {
    /// One tab, holding `document`, in one pane.
    pub fn new(document: Document) -> Self {
        Self { files: vec![OpenFile::new(document)], panes: 1, focus: 0, widths: vec![1.0], clock: 0 }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Which tab is showing: the most recently shown tab in the pane that has the keyboard.
    ///
    /// This is the meaning of "the open file" everywhere else in the window, which is why it is
    /// derived rather than stored. Nothing outside this file had to learn about panes to go on
    /// asking it.
    pub fn active_index(&self) -> usize {
        self.showing_in(self.focus).unwrap_or(0)
    }

    pub fn active(&self) -> &OpenFile {
        &self.files[self.active_index()]
    }

    pub fn active_mut(&mut self) -> &mut OpenFile {
        let index = self.active_index();
        &mut self.files[index]
    }

    pub fn iter(&self) -> impl Iterator<Item = &OpenFile> {
        self.files.iter()
    }

    /// Every open file, to be changed. The editor's font is one setting for the whole window, so
    /// there has to be a way to reach the tabs that are not showing.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut OpenFile> {
        self.files.iter_mut()
    }

    pub fn get(&self, index: usize) -> Option<&OpenFile> {
        self.files.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut OpenFile> {
        self.files.get_mut(index)
    }

    /// Which tab holds `path`, if any.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|file| file.path() == Some(path))
    }

    /// The tab at `index`. Panics past the end, as `active` does: an index here always came from
    /// [`Self::index_of`] or from a walk of the tabs.
    pub fn at(&self, index: usize) -> &OpenFile {
        &self.files[index]
    }

    pub fn at_mut(&mut self, index: usize) -> &mut OpenFile {
        &mut self.files[index]
    }

    // ------------------------------------------------------------------------------- the panes

    /// How many panes the editing area is divided into.
    pub fn pane_count(&self) -> usize {
        self.panes
    }

    /// Which pane has the keyboard.
    pub fn focused_pane(&self) -> usize {
        self.focus
    }

    /// Put the keyboard in a pane. A number past the end is refused rather than clamped, so a
    /// command line that names a pane that is not there is told so.
    pub fn focus_pane(&mut self, pane: usize) -> bool {
        if pane >= self.panes {
            return false;
        }
        self.focus = pane;
        true
    }

    /// The keyboard to the next pane, wrapping round at the right hand end.
    pub fn next_pane(&mut self) {
        self.focus = (self.focus + 1) % self.panes;
    }

    pub fn previous_pane(&mut self) {
        self.focus = (self.focus + self.panes - 1) % self.panes;
    }

    /// Which pane a tab is in.
    pub fn pane_of(&self, index: usize) -> usize {
        self.files.get(index).map(|file| file.pane).unwrap_or(0)
    }

    /// The tabs in one pane, as indices into the open files, in the order they are drawn.
    pub fn tabs_in(&self, pane: usize) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.pane == pane)
            .map(|(index, _)| index)
            .collect()
    }

    /// Which tab is showing in `pane`: the one in it that was shown most recently.
    pub fn showing_in(&self, pane: usize) -> Option<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.pane == pane)
            .max_by_key(|(_, file)| file.shown_at)
            .map(|(index, _)| index)
    }

    /// Each pane's share of the editing area's width, left to right.
    pub fn pane_widths(&self) -> &[f32] {
        &self.widths
    }

    /// Move the divider between pane `left` and the one after it by `delta` of the whole width.
    ///
    /// `smallest` is the least share a pane may have, which the caller works out from how wide the
    /// editing area is: a divider that could be dragged past its neighbour would be a way of losing
    /// a pane off the side of the window.
    pub fn move_divider(&mut self, left: usize, delta: f32, smallest: f32) {
        if left + 1 >= self.panes || self.widths.len() != self.panes {
            return;
        }
        let total = self.widths[left] + self.widths[left + 1];
        let smallest = smallest.min(total / 2.0);
        let taken = (self.widths[left] + delta).clamp(smallest, total - smallest);
        self.widths[left] = taken;
        self.widths[left + 1] = total - taken;
    }

    /// Set one pane's share directly, which is what the command line does, sharing what is left
    /// between the others in the proportions they already had.
    pub fn set_pane_width(&mut self, pane: usize, fraction: f32) -> bool {
        if pane >= self.panes || self.panes < 2 {
            return false;
        }
        let wanted = fraction.clamp(0.05, 0.95);
        let rest: f32 = self.widths.iter().enumerate().filter(|(at, _)| *at != pane).map(|(_, w)| w).sum();
        for (at, width) in self.widths.iter_mut().enumerate() {
            if at == pane {
                *width = wanted;
            } else if rest > 0.0 {
                *width = *width / rest * (1.0 - wanted);
            } else {
                *width = (1.0 - wanted) / (self.panes - 1) as f32;
            }
        }
        true
    }

    /// Every pane the same width, which is what double clicking a divider asks for.
    pub fn reset_pane_widths(&mut self) {
        self.widths = vec![1.0 / self.panes as f32; self.panes];
    }

    /// Put a pane to the right of the one that has the keyboard, and move the tab that is showing
    /// into it.
    ///
    /// The tab **moves** rather than being copied, which is where Quill and IntelliJ part company:
    /// IntelliJ's `Split Right` shows the same file in both splits, and Quill cannot, because two
    /// tabs on one file would be two documents over one path and saving either would throw the
    /// other away. This is IntelliJ's `Split and Move Right` under the name a person looks for.
    /// `tasks/task-1664-split-view-tdd.md` §3 records what was weighed.
    ///
    /// **When the pane holds only that tab**, taking it away would empty the pane it came from and
    /// leave the window looking exactly as it did. So the tab stays where it is and the new pane
    /// opens empty, with a fresh untitled tab in it. That is what a person means by putting a pane
    /// on the right: the next file they open lands in it, because opening a file always lands in
    /// the pane with the keyboard.
    pub fn split_right(&mut self) {
        let pane = self.focus;
        let alone = self.tabs_in(pane).len() < 2;
        let showing = self.showing_in(pane);
        self.add_pane_after(pane);
        let new = pane + 1;
        if alone {
            let mut fresh = OpenFile::new(Document::new());
            fresh.pane = new;
            let at = showing.map(|index| index + 1).unwrap_or(self.files.len()).min(self.files.len());
            self.files.insert(at, fresh);
            self.focus = new;
            self.stamp(at);
        } else if let Some(index) = showing {
            self.files[index].pane = new;
            self.focus = new;
            self.stamp(index);
        }
        self.tidy();
    }

    /// Move the tab that is showing into the pane beside it. `false` when there is no pane that way.
    pub fn move_tab(&mut self, right: bool) -> bool {
        let pane = self.focus;
        let target = if right { pane + 1 } else { pane.checked_sub(1).unwrap_or(usize::MAX) };
        if target >= self.panes {
            return false;
        }
        let Some(index) = self.showing_in(pane) else {
            return false;
        };
        self.files[index].pane = target;
        self.focus = target;
        self.stamp(index);
        // The pane it left may now be empty, in which case `tidy` removes it and the panes after it
        // are renumbered — including the one the tab has just been put into.
        self.tidy();
        true
    }

    /// Put the tab at `index` into `pane`, `position` tabs along it. This is what dragging a tab
    /// does, and what `quill-cli tab move` asks for.
    ///
    /// `position` counts the tabs of the target pane **as they are on the screen now**, including
    /// the tab being moved when it is already in that pane — because that is what a person dragging
    /// one is looking at. Taking it out first shifts everything after it up by one, so a move within
    /// a pane to a place further along has one subtracted from it here rather than at every call.
    ///
    /// A position past the end means the end, so dropping a tab anywhere to the right of the last
    /// one puts it last. Dropping it into a pane with nothing in it works for the same reason.
    ///
    /// The tab is **shown** where it lands and the keyboard follows it, which is what dragging
    /// something somewhere means; and the pane it left is folded away by [`Self::tidy`] if it was
    /// its last tab, exactly as [`Self::move_tab`] already leaves it.
    pub fn drag_tab(&mut self, index: usize, pane: usize, position: usize) -> bool {
        if index >= self.files.len() || pane >= self.panes {
            return false;
        }
        let from = self.files[index].pane;
        let within = self.tabs_in(from).iter().position(|at| *at == index).unwrap_or(0);
        let mut position = position;
        if from == pane {
            if within < position {
                position -= 1;
            }
            if within == position {
                // Dropped where it already is. Showing it is still right — a person who picked a tab
                // up and put it back plainly means to be looking at it — but nothing moves.
                self.focus = pane;
                self.stamp(index);
                return true;
            }
        }
        let mut file = self.files.remove(index);
        file.pane = pane;
        // Where in the vector: at the tab that is to come after it, or after the last one when it is
        // going on the end. Worked out after the removal, so the indices are the ones being inserted
        // into rather than the ones that were there a moment ago.
        let targets = self.tabs_in(pane);
        let at = match targets.get(position) {
            Some(index) => *index,
            None => targets.last().map(|index| index + 1).unwrap_or(self.files.len()),
        };
        self.files.insert(at, file);
        self.focus = pane;
        self.stamp(at);
        self.tidy();
        true
    }

    /// Fold the pane that has the keyboard into the one beside it: the pane on its left where there
    /// is one, otherwise the pane on its right. IntelliJ's `Unsplit`.
    pub fn unsplit(&mut self) -> bool {
        if self.panes < 2 {
            return false;
        }
        let pane = self.focus;
        let target = if pane > 0 { pane - 1 } else { 1 };
        for file in self.files.iter_mut().filter(|file| file.pane == pane) {
            file.pane = target;
        }
        self.focus = target;
        self.tidy();
        true
    }

    /// Every tab back into one pane. IntelliJ's `Unsplit All`.
    pub fn unsplit_all(&mut self) -> bool {
        if self.panes < 2 {
            return false;
        }
        for file in &mut self.files {
            file.pane = 0;
        }
        self.focus = 0;
        self.tidy();
        true
    }

    /// Put the tabs back into the panes a project was left in.
    ///
    /// `panes` is one number a tab, in tab order, and anything it says that would break an invariant
    /// is corrected rather than refused: a pane number past the end is clamped, a list of the wrong
    /// length leaves the tabs it does not reach in pane zero, and a set of numbers that leaves a
    /// pane empty is collapsed by [`Self::tidy`]. A hand edited state file must not stop a project
    /// opening, which is the rule the whole of `services::project_state` keeps.
    pub fn restore_panes(&mut self, panes: &[usize], widths: &[f32], focus: usize) {
        let most = panes.iter().copied().max().map(|highest| highest + 1).unwrap_or(1);
        self.panes = most.max(1);
        for (file, pane) in self.files.iter_mut().zip(panes) {
            file.pane = (*pane).min(most.saturating_sub(1));
        }
        self.widths = if widths.len() == self.panes {
            widths.to_vec()
        } else {
            vec![1.0 / self.panes as f32; self.panes]
        };
        self.focus = focus.min(self.panes.saturating_sub(1));
        self.tidy();
    }

    /// Which pane each tab is in, in tab order, for the project's state file.
    pub fn panes_of_tabs(&self) -> Vec<usize> {
        self.files.iter().map(|file| file.pane).collect()
    }

    /// Add an empty pane after `pane`, dividing that pane's share of the width in half.
    ///
    /// Half of the pane being split rather than an equal share of everything, because the panes
    /// either side of it have no reason to move when the third of four is split.
    fn add_pane_after(&mut self, pane: usize) {
        for file in &mut self.files {
            if file.pane > pane {
                file.pane += 1;
            }
        }
        let share = self.widths.get(pane).copied().unwrap_or(1.0) / 2.0;
        if pane < self.widths.len() {
            self.widths[pane] = share;
        }
        self.widths.insert((pane + 1).min(self.widths.len()), share);
        self.panes += 1;
    }

    /// Keep the two invariants: panes numbered without gaps, and no pane empty.
    ///
    /// Called after everything that moves a tab between panes or takes one away. A pane that is
    /// removed gives its share of the width to the pane that takes its place in the row, so the
    /// widths still sum to one and the panes either side of it do not jump.
    fn tidy(&mut self) {
        if self.files.is_empty() {
            self.panes = 1;
            self.focus = 0;
            self.widths = vec![1.0];
            return;
        }
        if self.widths.len() != self.panes {
            self.widths = vec![1.0 / self.panes.max(1) as f32; self.panes.max(1)];
        }
        // Old pane number to new, keeping only the panes that still hold a tab.
        let mut renumbered: Vec<Option<usize>> = vec![None; self.panes];
        let mut widths: Vec<f32> = Vec::new();
        let mut carried = 0.0;
        for pane in 0..self.panes {
            let width = self.widths.get(pane).copied().unwrap_or(0.0);
            if self.files.iter().any(|file| file.pane == pane) {
                renumbered[pane] = Some(widths.len());
                widths.push(width + carried);
                carried = 0.0;
            } else {
                carried += width;
            }
        }
        if widths.is_empty() {
            // Every pane number on every tab is out of range, which a hand edited state file could
            // ask for. One pane holding everything is the answer that cannot be wrong.
            for file in &mut self.files {
                file.pane = 0;
            }
            self.panes = 1;
            self.focus = 0;
            self.widths = vec![1.0];
            return;
        }
        if carried > 0.0 {
            let last = widths.len() - 1;
            widths[last] += carried;
        }
        for file in &mut self.files {
            file.pane = renumbered.get(file.pane).copied().flatten().unwrap_or(0);
        }
        self.panes = widths.len();
        // The pane that had the keyboard may have gone, in which case the keyboard goes to the pane
        // that took its place, which is the one now standing where it stood.
        self.focus = renumbered
            .get(self.focus)
            .copied()
            .flatten()
            .unwrap_or_else(|| self.focus.min(self.panes - 1));
        let total: f32 = widths.iter().sum();
        self.widths = if total > 0.0 && total.is_finite() {
            widths.iter().map(|width| width / total).collect()
        } else {
            vec![1.0 / self.panes as f32; self.panes]
        };
    }

    /// This tab is the one showing in its pane from now on.
    fn stamp(&mut self, index: usize) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(file) = self.files.get_mut(index) {
            file.shown_at = clock;
        }
    }

    // ------------------------------------------------------------------------------- the tabs

    /// Show the tab at `index`, if there is one there, putting the keyboard in its pane.
    pub fn show(&mut self, index: usize) {
        if index < self.files.len() {
            self.focus = self.files[index].pane;
            self.stamp(index);
        }
    }

    /// Show the next tab **in the pane that has the keyboard**, wrapping round at the end, which is
    /// what Alt and an arrow key do. A pane's tabs are its own, so walking them never leaves it.
    pub fn next(&mut self) {
        self.step(true);
    }

    pub fn previous(&mut self) {
        self.step(false);
    }

    fn step(&mut self, forwards: bool) {
        let tabs = self.tabs_in(self.focus);
        if tabs.is_empty() {
            return;
        }
        let showing = self.active_index();
        let at = tabs.iter().position(|index| *index == showing).unwrap_or(0);
        let next = if forwards {
            (at + 1) % tabs.len()
        } else {
            (at + tabs.len() - 1) % tabs.len()
        };
        self.show(tabs[next]);
    }

    /// This tab is no longer one a single click will reuse.
    pub fn make_permanent(&mut self, index: usize) {
        if let Some(file) = self.files.get_mut(index) {
            file.transient = false;
        }
    }

    /// Open `document`.
    ///
    /// A file that is already open is shown rather than opened twice, because two tabs on one file
    /// would be two documents over one path and saving either would throw the other away. When
    /// `permanent` is false and the pane with the keyboard has a transient tab, that tab's contents
    /// are replaced; otherwise a tab is added after the one that is showing, which is where a new
    /// tab belongs when it was opened from the one before it.
    ///
    /// Both the tab it reuses and the tab it adds are in the **pane that has the keyboard**, which
    /// is what makes a new pane useful: split, then open, and the file lands in the new pane.
    ///
    /// Returns the index of the tab the document ended up in.
    pub fn open(&mut self, document: Document, permanent: bool) -> usize {
        if let Some(path) = document.path() {
            if let Some(index) = self.index_of(path) {
                self.show(index);
                if permanent {
                    self.files[index].transient = false;
                }
                return index;
            }
        }
        match self.reuse(permanent) {
            Some(index) => {
                let file = &mut self.files[index];
                file.document = document;
                file.view_mode = ViewMode::Raw;
                file.scroll = 0.0;
                file.preview_scroll = 0.0;
                file.transient = !permanent;
                file.picture = None;
                file.forget_git();
                file.forget_what_was_worked_out();
                file.forget_where_it_was_being_read();
                self.show(index);
                index
            }
            None => {
                let mut file = OpenFile::new(document);
                file.transient = !permanent;
                self.insert_beside_the_open_tab(file)
            }
        }
    }

    /// Open a tab that is already built, which is how a picture gets one: it is not a document that
    /// was read, so it cannot come in through [`Self::open`].
    ///
    /// It follows exactly the same rules — a file that is already open is shown rather than opened
    /// twice, a transient tab is reused, a new tab lands beside the one it was opened from — because
    /// they are the rules about tabs rather than about text.
    pub fn open_file(&mut self, file: OpenFile, permanent: bool) -> usize {
        if let Some(path) = file.path().map(Path::to_path_buf) {
            if let Some(index) = self.index_of(&path) {
                self.show(index);
                if permanent {
                    self.files[index].transient = false;
                }
                return index;
            }
        }
        let mut file = file;
        file.transient = !permanent;
        match self.reuse(permanent) {
            Some(index) => {
                file.pane = self.files[index].pane;
                self.files[index] = file;
                self.show(index);
                index
            }
            None => self.insert_beside_the_open_tab(file),
        }
    }

    /// The tab a newly opened file should take over, if there is one: the transient tab in the pane
    /// with the keyboard, or an untitled tab in it that has never been touched.
    ///
    /// The untitled one is reused whether the file was asked for permanently or not, so opening the
    /// first file in a fresh window — or in a pane that has just been split off — does not leave an
    /// empty tab beside it.
    fn reuse(&self, permanent: bool) -> Option<usize> {
        let mine = |file: &OpenFile| file.pane == self.focus;
        let transient = self.files.iter().position(|file| mine(file) && file.transient);
        let empty = self.files.iter().position(|file| {
            mine(file)
                && file.path().is_none()
                && file.document.text().is_empty()
                && !file.document.is_modified()
        });
        if permanent {
            empty
        } else {
            transient.or(empty)
        }
    }

    /// Put a tab into the pane that has the keyboard, after the tab showing in it.
    fn insert_beside_the_open_tab(&mut self, mut file: OpenFile) -> usize {
        file.pane = self.focus;
        let at = self
            .showing_in(self.focus)
            .map(|index| index + 1)
            .unwrap_or(self.files.len())
            .min(self.files.len());
        self.files.insert(at, file);
        self.show(at);
        at
    }

    /// Close the tab at `index`.
    ///
    /// The tab that comes forward in its place is the one that was showing in that pane before it,
    /// which is what IntelliJ does and what falls out of the stamps for nothing.
    ///
    /// A pane emptied by the close is removed and the panes after it move up. Closing the last tab
    /// of the last pane leaves a fresh untitled tab rather than no tabs, so there is never a window
    /// with nothing to type into and never a pane with nothing to draw.
    pub fn close(&mut self, index: usize) {
        if index >= self.files.len() {
            return;
        }
        let pane = self.files[index].pane;
        self.files.remove(index);
        if self.files.is_empty() {
            self.files.push(OpenFile::new(Document::new()));
            self.panes = 1;
            self.focus = 0;
            self.widths = vec![1.0];
            return;
        }
        // The keyboard stays in the pane the tab was closed in while it still has tabs; `tidy` moves
        // it along when the pane has gone.
        self.focus = pane;
        self.tidy();
    }

    /// Forget what git said about every open file, which is what happens after an operation that
    /// could have changed any of them.
    pub fn forget_git(&mut self) {
        for file in &mut self.files {
            file.forget_git();
        }
    }

    /// Every open file's path, for a test and for the window's title.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().filter_map(|file| file.path().map(Path::to_path_buf)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(name: &str) -> Document {
        let mut document = Document::from_text(&format!("in {name}\n"));
        let path = std::env::temp_dir().join("quill-open-files").join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, format!("in {name}\n")).expect("write the file");
        document.save_as(&path).expect("save it");
        document
    }

    fn names(files: &OpenFiles) -> Vec<String> {
        files.iter().map(OpenFile::name).collect()
    }

    #[test]
    fn a_new_window_has_one_untitled_tab() {
        let files = OpenFiles::new(Document::new());
        assert_eq!(files.len(), 1);
        assert_eq!(files.active().name(), "untitled");
    }

    #[test]
    fn a_single_click_reuses_the_transient_tab_and_a_double_click_adds_one() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), false);
        assert_eq!(names(&files), vec!["one.md"], "the empty untitled tab was reused");
        files.open(document("two.md"), false);
        assert_eq!(names(&files), vec!["two.md"], "a second glance replaces the transient tab");
        files.open(document("three.md"), true);
        assert_eq!(names(&files), vec!["two.md", "three.md"], "a double click adds a tab");
        assert_eq!(files.active_index(), 1);
    }

    #[test]
    fn a_file_that_is_already_open_is_shown_rather_than_opened_twice() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        assert_eq!(files.active_index(), 1);
        files.open(document("one.md"), true);
        assert_eq!(files.len(), 2, "two tabs on one file would be two documents over one path");
        assert_eq!(files.active_index(), 0);
    }

    #[test]
    fn typing_in_the_transient_tab_makes_it_permanent() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), false);
        assert!(files.active().transient);
        files.make_permanent(files.active_index());
        files.open(document("two.md"), false);
        assert_eq!(names(&files), vec!["one.md", "two.md"], "the tab that was typed into is kept");
    }

    #[test]
    fn a_new_tab_opens_beside_the_one_it_was_opened_from() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.open(document("three.md"), true);
        files.show(0);
        files.open(document("four.md"), true);
        assert_eq!(names(&files), vec!["one.md", "four.md", "two.md", "three.md"]);
    }

    #[test]
    fn closing_the_last_tab_leaves_an_untitled_one() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.close(0);
        assert_eq!(files.len(), 1);
        assert_eq!(files.active().name(), "untitled");
    }

    #[test]
    fn closing_a_tab_shows_the_one_to_its_left() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.open(document("three.md"), true);
        assert_eq!(files.active_index(), 2);
        files.close(2);
        assert_eq!(names(&files), vec!["one.md", "two.md"]);
        assert_eq!(files.active().name(), "two.md");
        files.close(0);
        assert_eq!(files.active().name(), "two.md", "closing a tab before the open one keeps it open");
    }

    #[test]
    fn the_next_tab_wraps_round() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.show(1);
        files.next();
        assert_eq!(files.active_index(), 0);
        files.previous();
        assert_eq!(files.active_index(), 1);
    }

    #[test]
    fn a_picture_takes_a_tab_by_the_same_rules_as_a_file_of_text() {
        let mut files = OpenFiles::new(Document::new());
        let path = std::env::temp_dir().join("quill-open-files").join("photo.png");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, b"not really a png").expect("write it");

        files.open_file(OpenFile::picture(&path), true);
        assert_eq!(files.len(), 1, "the empty untitled tab was reused");
        assert!(files.active().is_picture());
        assert_eq!(files.active().name(), "photo.png");

        // Opening it again shows the tab it is already in rather than reading it twice.
        files.open(document("one.md"), true);
        files.open_file(OpenFile::picture(&path), true);
        assert_eq!(files.len(), 2);
        assert_eq!(files.active_index(), 0);
    }

    #[test]
    fn a_tab_that_held_a_picture_and_is_reused_for_text_stops_being_a_picture() {
        let mut files = OpenFiles::new(Document::new());
        let path = std::env::temp_dir().join("quill-open-files").join("reused.png");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, b"not really a png").expect("write it");
        files.open_file(OpenFile::picture(&path), false);
        assert!(files.active().is_picture());
        files.open(document("after.md"), false);
        assert!(!files.active().is_picture(), "the transient tab was reused for a file of text");
    }

    // ------------------------------------------------------------------------------- the panes

    /// Both invariants, asserted after every operation the pane tests do: the panes are numbered
    /// `0..pane_count` with no gaps, none of them is empty, the keyboard is in one that exists, and
    /// the widths are one a pane and sum to one.
    #[track_caller]
    fn invariants(files: &OpenFiles) {
        assert!(files.pane_count() >= 1, "there is always at least one pane");
        assert!(files.focused_pane() < files.pane_count(), "the keyboard is in a pane that exists");
        for pane in 0..files.pane_count() {
            assert!(!files.tabs_in(pane).is_empty(), "pane {pane} is empty");
        }
        for file in files.iter() {
            assert!(file.pane < files.pane_count(), "a tab is in pane {} of {}", file.pane, files.pane_count());
        }
        assert_eq!(files.pane_widths().len(), files.pane_count(), "one width a pane");
        let total: f32 = files.pane_widths().iter().sum();
        assert!((total - 1.0).abs() < 0.001, "the widths should sum to one, not {total}");
    }

    /// Two files open in one pane, which is where most of the pane tests start.
    fn two_open() -> OpenFiles {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files
    }

    #[test]
    fn a_new_window_has_one_pane_holding_everything() {
        let files = two_open();
        assert_eq!(files.pane_count(), 1);
        assert_eq!(files.tabs_in(0), vec![0, 1]);
        invariants(&files);
    }

    #[test]
    fn splitting_moves_the_tab_that_is_showing_into_a_new_pane_on_the_right() {
        let mut files = two_open();
        assert_eq!(files.active().name(), "two.md");
        files.split_right();
        assert_eq!(files.pane_count(), 2);
        assert_eq!(files.focused_pane(), 1, "the keyboard follows the tab into the new pane");
        assert_eq!(names_in(&files, 0), vec!["one.md"]);
        assert_eq!(names_in(&files, 1), vec!["two.md"]);
        assert_eq!(files.active().name(), "two.md");
        invariants(&files);
    }

    #[test]
    fn splitting_a_pane_holding_one_tab_opens_an_empty_pane_beside_it() {
        // Taking the only tab out of a pane would empty the pane it came from and leave the window
        // looking exactly as it did, so the tab stays and the new pane starts empty.
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.split_right();
        assert_eq!(files.pane_count(), 2);
        assert_eq!(names_in(&files, 0), vec!["one.md"]);
        assert_eq!(names_in(&files, 1), vec!["untitled"]);
        assert_eq!(files.focused_pane(), 1);
        invariants(&files);
    }

    #[test]
    fn a_file_opened_after_a_split_lands_in_the_pane_with_the_keyboard() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.split_right();
        // The new pane holds a fresh untitled tab, which the file takes over rather than sitting
        // beside.
        files.open(document("two.md"), true);
        assert_eq!(names_in(&files, 0), vec!["one.md"]);
        assert_eq!(names_in(&files, 1), vec!["two.md"]);
        invariants(&files);
    }

    #[test]
    fn splitting_halves_the_pane_that_was_split_and_leaves_the_others_alone() {
        let mut files = OpenFiles::new(Document::new());
        for name in ["one.md", "two.md", "three.md", "four.md"] {
            files.open(document(name), true);
        }
        files.split_right();
        files.show(0);
        files.split_right();
        // Two panes of a quarter each where the first was, and the half the second pane took stays
        // where it was.
        let widths = files.pane_widths().to_vec();
        assert_eq!(widths.len(), 3);
        assert!((widths[0] - 0.25).abs() < 0.001, "{widths:?}");
        assert!((widths[1] - 0.25).abs() < 0.001, "{widths:?}");
        assert!((widths[2] - 0.5).abs() < 0.001, "{widths:?}");
        invariants(&files);
    }

    /// Three files in one pane, which is what a rearrangement is done to.
    fn three_open() -> OpenFiles {
        let mut files = OpenFiles::new(Document::new());
        for name in ["one.md", "two.md", "three.md"] {
            files.open(document(name), true);
        }
        files
    }

    /// **A tab dragged along its own strip lands where the pointer left it.**
    #[test]
    fn a_tab_dragged_to_the_front_of_its_pane_goes_there() {
        let mut files = three_open();
        assert!(files.drag_tab(2, 0, 0));
        assert_eq!(names(&files), vec!["three.md", "one.md", "two.md"]);
        assert_eq!(files.active().name(), "three.md", "a tab dragged somewhere is shown there");
        invariants(&files);
    }

    /// **Dragging one to the right counts the tabs as they are on the screen.** Moving the first tab
    /// to position two means "past the second", which leaves it in the middle — not on the end,
    /// which is where a position not corrected for its own removal would put it.
    #[test]
    fn a_tab_dragged_along_its_own_strip_counts_the_tabs_it_passed() {
        let mut files = three_open();
        assert!(files.drag_tab(0, 0, 2));
        assert_eq!(names(&files), vec!["two.md", "one.md", "three.md"]);
        invariants(&files);
    }

    /// Past the end means the end.
    #[test]
    fn a_tab_dragged_past_the_last_one_goes_last() {
        let mut files = three_open();
        assert!(files.drag_tab(0, 0, 99));
        assert_eq!(names(&files), vec!["two.md", "three.md", "one.md"]);
        invariants(&files);
    }

    /// Dropped where it already was, nothing moves — but it is still shown, because a person who
    /// picked a tab up and put it back plainly means to be looking at it.
    #[test]
    fn a_tab_dropped_where_it_already_was_does_not_move() {
        let mut files = three_open();
        files.show(0);
        assert!(files.drag_tab(1, 0, 1));
        assert_eq!(names(&files), vec!["one.md", "two.md", "three.md"]);
        assert_eq!(files.active().name(), "two.md");
        invariants(&files);
    }

    /// **A tab dragged into another pane lands in it**, is shown there, and the keyboard follows.
    #[test]
    fn a_tab_dragged_into_another_pane_lands_in_it() {
        let mut files = three_open();
        files.split_right();
        assert_eq!(names_in(&files, 0), vec!["one.md", "two.md"]);
        assert_eq!(names_in(&files, 1), vec!["three.md"]);
        // one.md, which is tab zero, into the pane on the right, in front of three.md.
        assert!(files.drag_tab(0, 1, 0));
        assert_eq!(names_in(&files, 0), vec!["two.md"]);
        assert_eq!(names_in(&files, 1), vec!["one.md", "three.md"]);
        assert_eq!(files.focused_pane(), 1);
        assert_eq!(files.active().name(), "one.md");
        invariants(&files);
    }

    /// Dragging the last tab out of a pane takes the pane with it, exactly as `move_tab` does.
    #[test]
    fn dragging_the_last_tab_out_of_a_pane_takes_the_pane_with_it() {
        let mut files = two_open();
        files.split_right();
        assert_eq!(files.pane_count(), 2);
        assert!(files.drag_tab(1, 0, 0));
        assert_eq!(files.pane_count(), 1, "an emptied pane is removed");
        assert_eq!(names_in(&files, 0), vec!["two.md", "one.md"]);
        invariants(&files);
    }

    /// A tab that is not there, or a pane that is not there, is refused rather than clamped — the
    /// same rule `focus_pane` follows, so a command line that names a pane that is not there is told
    /// so instead of quietly doing something else.
    #[test]
    fn dragging_to_somewhere_that_is_not_there_is_refused() {
        let mut files = three_open();
        assert!(!files.drag_tab(9, 0, 0));
        assert!(!files.drag_tab(0, 4, 0));
        assert_eq!(names(&files), vec!["one.md", "two.md", "three.md"]);
        invariants(&files);
    }

    #[test]
    fn moving_the_last_tab_out_of_a_pane_takes_the_pane_with_it() {
        let mut files = two_open();
        files.split_right();
        assert_eq!(files.pane_count(), 2);
        // two.md is alone in the pane on the right, so moving it back leaves that pane empty.
        assert!(files.move_tab(false));
        assert_eq!(files.pane_count(), 1, "an emptied pane is removed");
        assert_eq!(names_in(&files, 0), vec!["one.md", "two.md"]);
        invariants(&files);
    }

    #[test]
    fn there_is_nothing_to_the_right_of_the_last_pane() {
        let mut files = two_open();
        assert!(!files.move_tab(true), "one pane has nothing beside it");
        assert!(!files.move_tab(false));
        invariants(&files);
    }

    #[test]
    fn unsplitting_folds_a_pane_into_the_one_on_its_left() {
        let mut files = two_open();
        files.open(document("three.md"), true);
        files.split_right();
        files.open(document("four.md"), true);
        assert_eq!(names_in(&files, 1), vec!["three.md", "four.md"]);
        assert!(files.unsplit());
        assert_eq!(files.pane_count(), 1);
        assert_eq!(names_in(&files, 0), vec!["one.md", "two.md", "three.md", "four.md"]);
        invariants(&files);
    }

    #[test]
    fn unsplitting_the_leftmost_pane_folds_it_into_the_one_on_its_right() {
        let mut files = two_open();
        files.split_right();
        files.focus_pane(0);
        assert!(files.unsplit());
        assert_eq!(files.pane_count(), 1);
        assert_eq!(names_in(&files, 0), vec!["one.md", "two.md"]);
        invariants(&files);
    }

    #[test]
    fn unsplit_all_puts_every_tab_back_in_one_pane() {
        let mut files = two_open();
        files.split_right();
        files.open(document("three.md"), true);
        files.split_right();
        assert_eq!(files.pane_count(), 3);
        assert!(files.unsplit_all());
        assert_eq!(files.pane_count(), 1);
        assert_eq!(files.len(), 3);
        assert!(!files.unsplit_all(), "there is nothing to unsplit with one pane");
        invariants(&files);
    }

    #[test]
    fn each_pane_walks_its_own_tabs() {
        let mut files = two_open();
        files.open(document("three.md"), true);
        files.split_right();
        files.open(document("four.md"), true);
        // The pane on the right holds three.md and four.md, and stepping through it never reaches
        // the two files in the pane on the left.
        assert_eq!(files.active().name(), "four.md");
        files.next();
        assert_eq!(files.active().name(), "three.md");
        files.next();
        assert_eq!(files.active().name(), "four.md");
        invariants(&files);
    }

    #[test]
    fn closing_the_tab_that_is_showing_brings_back_the_one_before_it() {
        let mut files = two_open();
        files.open(document("three.md"), true);
        files.show(0);
        files.show(2);
        // one.md was shown before three.md, so it is the one that comes forward.
        files.close(2);
        assert_eq!(files.active().name(), "one.md");
        invariants(&files);
    }

    #[test]
    fn closing_the_last_tab_in_a_pane_removes_the_pane_and_moves_the_keyboard() {
        let mut files = two_open();
        files.split_right();
        let showing = files.active_index();
        files.close(showing);
        assert_eq!(files.pane_count(), 1);
        assert_eq!(files.focused_pane(), 0);
        assert_eq!(files.active().name(), "one.md");
        invariants(&files);
    }

    #[test]
    fn the_keyboard_walks_the_panes_and_wraps_round() {
        let mut files = two_open();
        files.split_right();
        assert_eq!(files.focused_pane(), 1);
        files.next_pane();
        assert_eq!(files.focused_pane(), 0);
        files.previous_pane();
        assert_eq!(files.focused_pane(), 1);
        assert!(!files.focus_pane(9), "a pane that is not there is refused rather than clamped");
        invariants(&files);
    }

    #[test]
    fn showing_a_tab_puts_the_keyboard_in_its_pane() {
        let mut files = two_open();
        files.split_right();
        assert_eq!(files.focused_pane(), 1);
        let one = files.index_of(&document("one.md").path().expect("a path").to_path_buf()).expect("open");
        files.show(one);
        assert_eq!(files.focused_pane(), 0, "clicking a tab moves the keyboard to its pane");
        assert_eq!(files.active().name(), "one.md");
        invariants(&files);
    }

    #[test]
    fn a_divider_cannot_be_dragged_past_its_neighbour() {
        let mut files = two_open();
        files.split_right();
        files.move_divider(0, 5.0, 0.2);
        let widths = files.pane_widths().to_vec();
        assert!((widths[0] - 0.8).abs() < 0.001, "{widths:?}");
        assert!((widths[1] - 0.2).abs() < 0.001, "{widths:?}");
        invariants(&files);
    }

    #[test]
    fn a_state_file_that_asks_for_panes_that_would_be_empty_is_corrected() {
        let mut files = two_open();
        files.open(document("three.md"), true);
        // Pane 1 is named by nothing, so it cannot exist; what is asked for is two panes, not three.
        files.restore_panes(&[0, 2, 2], &[], 2);
        assert_eq!(files.pane_count(), 2);
        assert_eq!(names_in(&files, 0), vec!["one.md"]);
        assert_eq!(names_in(&files, 1), vec!["two.md", "three.md"]);
        invariants(&files);
    }

    #[test]
    fn a_pane_number_past_the_end_is_clamped_rather_than_refused() {
        let mut files = two_open();
        files.restore_panes(&[0, 1], &[0.3, 0.7], 7);
        assert_eq!(files.pane_count(), 2);
        assert_eq!(files.focused_pane(), 1);
        let widths = files.pane_widths().to_vec();
        assert!((widths[0] - 0.3).abs() < 0.001, "{widths:?}");
        invariants(&files);
    }

    #[test]
    fn what_the_panes_are_is_what_comes_back() {
        let mut files = two_open();
        files.split_right();
        assert_eq!(files.panes_of_tabs(), vec![0, 1]);
    }

    /// The names of the tabs in one pane, which is what the pane tests assert against.
    fn names_in(files: &OpenFiles, pane: usize) -> Vec<String> {
        files.tabs_in(pane).into_iter().map(|index| files.at(index).name()).collect()
    }
}
