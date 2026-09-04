//! Collapsing and expanding blocks: the window's half of `task-1686`.
//!
//! `unluminate_core::folding` reads a file for what **could** be collapsed and holds which of them are;
//! this is where the window asks it, keeps the answer, and turns a press of an arrow or a menu entry
//! into a change. `tasks/task-1686-folding-tdd.md` is the design.
//!
//! Two caches and one rule between them. The **regions** are derived from the text and are kept on
//! the tab keyed on `Document::text_revision`, exactly as `app::symbols::TabSymbols` is, so a frame
//! in which nothing was typed costs one integer comparison. Which of them are **collapsed** is in
//! the document, and every command here rebuilds the whole set from the regions as they are now —
//! which is what keeps an offset that no longer names any head from being kept for ever.

use std::path::Path;

use unluminate_core::folding::{self, Folds, Hidden, Region};

use crate::app::UnluminateApp;
use crate::services::file_kind;

/// What the tab has been read for, and when.
pub struct TabRegions {
    /// The `text_revision` this was read at.
    pub revision: u64,
    pub regions: Vec<Region>,
    /// Which head is collapsed, in the order the regions are, and the `fold_revision` that was
    /// worked out at.
    ///
    /// Kept rather than worked out each time because the gutter and the badges both want it on
    /// every frame, and `task-1666`'s rule is that nothing running once a frame may allocate. Two
    /// integer comparisons say it is still right.
    pub marks: Vec<(usize, bool)>,
    pub marks_at: u64,
}

impl TabRegions {
    /// How many blocks are collapsed, which is what dims `Expand All`.
    pub fn folded(&self) -> usize {
        self.marks.iter().filter(|(_, shut)| *shut).count()
    }
}

impl UnluminateApp {
    /// Everything in the tab at `index` that could be collapsed, read from its live text and kept
    /// until that text changes.
    pub(crate) fn fold_regions(&mut self, index: usize) -> &[Region] {
        let revision = self.files.at(index).document.text_revision();
        let fresh = self
            .files
            .at(index)
            .cached
            .fold_regions
            .as_ref()
            .is_some_and(|read| read.revision == revision);
        if !fresh {
            let too_large = self.files.at(index).document.text().len_bytes() > UnluminateApp::COLOUR_LIMIT;
            // A file too large to colour is too large to read for its blocks, and for the same
            // reason: both are one linear pass over the text on every change. It keeps its line
            // numbers and loses its arrows, which is what `colour_the_file` already does about
            // colours, and it says so in the same place.
            let regions = if !too_large && file_kind::folding_applies(self.files.at(index).path()) {
                let grammars = self.plugins.grammars();
                let path = self.files.at(index).path().map(Path::to_path_buf);
                let reading = file_kind::folding_reading(path.as_deref(), &grammars);
                let text = self.files.at(index).document.text().to_string();
                // The comments and strings `colour_the_file` already read out of this same text, if
                // it has. A file with no plugin, or one too large to colour, is read here instead.
                match self.files.at(index).cached.fold_tokens.as_ref() {
                    Some((at, tokens)) if *at == revision => {
                        folding::regions_from(&text, reading, tokens)
                    }
                    _ => folding::regions(&text, reading),
                }
            } else {
                Vec::new()
            };
            self.files.at_mut(index).cached.fold_regions =
                Some(TabRegions { revision, regions, marks: Vec::new(), marks_at: 0 });
        }
        &self.files.at(index).cached.fold_regions.as_ref().expect("just read").regions
    }

    /// Which paragraphs of the tab at `index` are folded away, which is what layout is handed.
    ///
    /// A union of the bodies of the collapsed regions, because regions nest: collapsing a class and
    /// a method inside it hides one stretch of the file rather than two overlapping ones.
    pub(crate) fn hidden_paragraphs(&mut self, index: usize) -> Hidden {
        if self.files.at(index).document.folds().is_empty() {
            return Hidden::none();
        }
        let collapsed = self.collapsed_regions(index);
        Hidden::of(collapsed.into_iter().map(|region| region.body))
    }

    /// The regions of the tab at `index` that are collapsed, in order.
    fn collapsed_regions(&mut self, index: usize) -> Vec<Region> {
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let document = &self.files.at(index).document;
        regions
            .into_iter()
            .filter(|region| document.folds().holds(&document.text().line_range(region.head)))
            .collect()
    }

    /// The paragraphs that head a region, and whether each is collapsed — what the gutter draws its
    /// arrows from and what the editing area draws its badges from.
    ///
    /// Kept beside the regions and rebuilt only when the text or the folds have moved, because both
    /// callers ask on every frame of every pane.
    pub(crate) fn fold_marks(&mut self, index: usize) -> &[(usize, bool)] {
        let folded = self.files.at(index).document.fold_revision();
        self.fold_regions(index);
        let read = self.files.at(index).cached.fold_regions.as_ref().expect("just read");
        if read.marks_at != folded || read.marks.len() != read.regions.len() {
            let regions: Vec<Region> = read.regions.clone();
            let document = &self.files.at(index).document;
            let marks: Vec<(usize, bool)> = regions
                .iter()
                .map(|region| {
                    let line = document.text().line_range(region.head);
                    (region.head, document.folds().holds(&line))
                })
                .collect();
            let read = self.files.at_mut(index).cached.fold_regions.as_mut().expect("just read");
            read.marks = marks;
            read.marks_at = folded;
        }
        &self.files.at(index).cached.fold_regions.as_ref().expect("just read").marks
    }

    /// The heads that are collapsed, sorted.
    pub(crate) fn collapsed_heads(&mut self, index: usize) -> Vec<usize> {
        self.fold_marks(index).iter().filter(|(_, shut)| *shut).map(|(at, _)| *at).collect()
    }

    /// How many blocks the file that is showing has, and how many of them are collapsed — what the
    /// fold menu entries are dimmed by.
    ///
    /// Read from what the last frame worked out rather than working it out again, because the menus
    /// are built from `&self`. A tab that has never been drawn answers zero, which is right: it has
    /// nothing on the screen to fold.
    pub(crate) fn fold_counts(&self) -> (usize, usize) {
        match &self.files.active().cached.fold_regions {
            Some(read) => (read.regions.len(), read.folded()),
            None => (0, 0),
        }
    }

    /// Write a set of collapsed heads back to the document, as byte offsets.
    ///
    /// The one place the fold state changes, so every command below is a list of head lines and
    /// nothing more. Rebuilding from lines rather than editing the offsets in place is what prunes
    /// an offset that no longer names a head.
    fn set_collapsed(&mut self, index: usize, heads: &[usize]) -> bool {
        let mut folds = Folds::new();
        {
            let text = self.files.at(index).document.text();
            for head in heads {
                if *head < text.len_lines() {
                    folds.add(text.line_to_byte(*head));
                }
            }
        }
        self.files.at_mut(index).document.set_folds(folds)
    }

    /// Collapse or expand the region headed by `line`, which is what pressing its arrow means.
    pub(crate) fn toggle_fold_at_line(&mut self, line: usize) -> bool {
        let index = self.files.active_index();
        let mut marks: Vec<(usize, bool)> = self.fold_marks(index).to_vec();
        let Some(entry) = marks.iter_mut().find(|(head, _)| *head == line) else {
            self.message = Some(format!("There is nothing to fold at line {}.", line + 1));
            return false;
        };
        entry.1 = !entry.1;
        let heads: Vec<usize> = marks.iter().filter(|(_, shut)| *shut).map(|(at, _)| *at).collect();
        let changed = self.set_collapsed(index, &heads);
        self.keep_the_caret_visible(index);
        changed
    }

    /// Bring the caret out of a block that has just been collapsed, onto that block's head line.
    ///
    /// The other half of "a caret is never inside a hidden paragraph". Collapsing something the
    /// caret is inside must not expand it again — that would make `Collapse All` do nothing
    /// whenever somebody was in the middle of a function — so the caret moves instead, which is what
    /// The reference editor does. The head line is where the block still is on the screen.
    fn keep_the_caret_visible(&mut self, index: usize) {
        let hidden = self.hidden_paragraphs(index);
        if hidden.is_empty() {
            return;
        }
        let document = &self.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        if !hidden.contains(caret) {
            return;
        }
        // The first visible line at or above it, which is the head of the outermost block holding
        // the caret.
        let mut line = caret;
        while line > 0 && hidden.contains(line) {
            line -= 1;
        }
        let offset = self.files.at(index).document.text().line_to_byte(line);
        self.files.at_mut(index).document.apply(unluminate_core::Command::PlaceCaret {
            offset,
            extend: false,
        });
    }

    /// Collapse or expand the innermost region the caret is in, which is what the keyboard and the
    /// menu entry mean.
    ///
    /// The innermost, because that is the block a person looking at that line is thinking about. On
    /// a line that heads a region it is that region; inside one it is the one holding it.
    pub(crate) fn toggle_fold_at_caret(&mut self) -> bool {
        let index = self.files.active_index();
        let document = &self.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let Some(region) = folding::region_at(&regions, caret) else {
            self.message = Some("There is nothing to fold here.".to_owned());
            return false;
        };
        let head = region.head;
        self.toggle_fold_at_line(head)
    }

    /// Collapse the region headed by `line` and every region inside it.
    ///
    /// `task-1707`: "open that function" is the ask, and a function's children are the `for` and the
    /// `if` inside it. The block outside is left as it was — only the subtree the caller named moves.
    /// Collapsing twice is a no-op the second time, because the set is already what it asks for.
    pub(crate) fn collapse_recursively_at_line(&mut self, line: usize) -> bool {
        let index = self.files.active_index();
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let Some(tree) = folding::region_tree(&regions, line) else {
            self.message =
                Some(format!("Nothing at line {} heads a block. Run `fold list`.", line + 1));
            return false;
        };
        let heads = self.collapse_or_expand_recursively(index, tree, true);
        self.keep_the_caret_visible(index);
        heads
    }

    /// Expand the region headed by `line` and every region inside it.
    ///
    /// The children's own collapsed state is destroyed — a child that was folded on its own opens
    /// too — which is what the reference editor's and VS Code's recursive expand both do, and what "open that
    /// function so I can read it" wants. `tasks/task-1707-recursive-folding-tdd.md` section 3.
    pub(crate) fn expand_recursively_at_line(&mut self, line: usize) -> bool {
        let index = self.files.active_index();
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let Some(tree) = folding::region_tree(&regions, line) else {
            self.message =
                Some(format!("Nothing at line {} heads a block. Run `fold list`.", line + 1));
            return false;
        };
        self.collapse_or_expand_recursively(index, tree, false)
    }

    /// The shared half of the two recursive commands: work out the new collapsed set from the
    /// subtree and write it back.
    ///
    /// Everything outside the subtree keeps whatever state it had. Inside it, every head is
    /// collapsed when `collapse` is set and every head is left open when it is not, which is what
    /// makes a recursive expand a hard open of the whole subtree rather than the set's natural
    /// "remove the parent and keep the children".
    fn collapse_or_expand_recursively(&mut self, index: usize, tree: Vec<&Region>, collapse: bool) -> bool {
        let tree_heads: Vec<usize> = tree.iter().map(|region| region.head).collect();
        let mut heads = self.collapsed_heads(index);
        heads.retain(|head| !tree_heads.contains(head));
        if collapse {
            heads.extend(tree_heads);
        }
        let changed = self.set_collapsed(index, &heads);
        changed
    }

    /// Collapse the innermost region the caret is in, and every region inside it.
    pub(crate) fn collapse_recursively_at_caret(&mut self) -> bool {
        let index = self.files.active_index();
        let document = &self.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let Some(region) = folding::region_at(&regions, caret) else {
            self.message = Some("There is nothing to fold here.".to_owned());
            return false;
        };
        self.collapse_recursively_at_line(region.head)
    }

    /// Expand the innermost region the caret is in, and every region inside it.
    pub(crate) fn expand_recursively_at_caret(&mut self) -> bool {
        let index = self.files.active_index();
        let document = &self.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let Some(region) = folding::region_at(&regions, caret) else {
            self.message = Some("There is nothing to fold here.".to_owned());
            return false;
        };
        self.expand_recursively_at_line(region.head)
    }

    /// Collapse every region in the file that is showing.
    pub(crate) fn collapse_all_folds(&mut self) -> bool {
        let index = self.files.active_index();
        let heads: Vec<usize> = self.fold_regions(index).iter().map(|region| region.head).collect();
        if heads.is_empty() {
            self.message = Some("There is nothing in this file to collapse.".to_owned());
            return false;
        }
        let changed = self.set_collapsed(index, &heads);
        self.keep_the_caret_visible(index);
        changed
    }

    /// Show all again.
    pub(crate) fn expand_all_folds(&mut self) -> bool {
        let index = self.files.active_index();
        self.files.at_mut(index).document.expand_all_folds()
    }

    /// Collapse everything that does not hold a marked passage — the ticket's `Collapse All But
    /// Highlighted`.
    ///
    /// VS Code's `foldAllExcept` over the marks of `task-1663`, and the parents of a kept region are
    /// kept too: a marked line inside a method inside a class is only visible if the class and the
    /// method are both open, which `folding::collapse_all_but` arranges by asking whether a region
    /// **covers** the line rather than heads it.
    ///
    /// **With nothing marked it falls back to the selection**, because somebody who has selected a
    /// function and asked for this plainly means that function; with neither it says so rather than
    /// collapsing the whole file, which is what a person would read as the command having gone
    /// wrong.
    pub(crate) fn collapse_all_but_marked(&mut self) -> bool {
        self.collapse_all_but_kept(false)
    }

    /// The same, with a caller that has said which of the two to keep.
    ///
    /// `unluminate-cli fold others --selection` is the only thing that says so: an agent that has just
    /// selected a range and wants only that on the screen should not have to clear somebody's marks
    /// first.
    pub(crate) fn collapse_all_but_kept(&mut self, prefer_selection: bool) -> bool {
        let index = self.files.active_index();
        let keep = self.lines_to_keep_open(index, prefer_selection);
        if keep.is_empty() {
            self.message = Some(
                "Highlight a passage or select some text first: this collapses everything else."
                    .to_owned(),
            );
            return false;
        }
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let heads = folding::collapse_all_but(&regions, &keep);
        let changed = self.set_collapsed(index, &heads);
        self.keep_the_caret_visible(index);
        changed
    }

    /// The lines that have to stay visible: every line a mark touches, or every line the selection
    /// touches when there are no marks.
    fn lines_to_keep_open(&self, index: usize, prefer_selection: bool) -> Vec<usize> {
        let document = &self.files.at(index).document;
        let text = document.text();
        let mut ranges: Vec<std::ops::Range<usize>> = if prefer_selection {
            Vec::new()
        } else {
            document.highlights().iter().map(|mark| mark.range.clone()).collect()
        };
        if ranges.is_empty() {
            let selection = document.selection().range();
            if !selection.is_empty() {
                ranges.push(selection);
            }
        }
        let mut lines: Vec<usize> = Vec::new();
        for range in ranges {
            let first = text.byte_to_line(range.start);
            let last = text.byte_to_line(range.end.min(text.len_bytes()));
            for line in first..=last {
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
        }
        lines.sort_unstable();
        lines
    }

    /// Expand whatever is hiding the caret.
    ///
    /// **A caret is never inside a hidden paragraph.** Called wherever the caret is put somewhere by
    /// something other than a click — a jump to a definition, a search hit, `unluminate-cli editor caret
    /// --line` — and derived from where the caret now is rather than fired from each of those
    /// places, because the next one added would be the one that forgot.
    pub fn reveal_the_caret_from_a_fold(&mut self) -> bool {
        let index = self.files.active_index();
        if self.files.at(index).document.folds().is_empty() {
            return false;
        }
        let document = &self.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        let hiding: Vec<usize> = folding::regions_holding(&regions, caret)
            .into_iter()
            .map(|region| region.head)
            .collect();
        if hiding.is_empty() {
            return false;
        }
        let mut heads = self.collapsed_heads(index);
        heads.retain(|head| !hiding.contains(head));
        self.set_collapsed(index, &heads)
    }

    /// Every region of the file that is showing, as the command line reports it.
    pub(crate) fn fold_report(&mut self) -> Vec<serde_json::Value> {
        let index = self.files.active_index();
        let marks: Vec<(usize, bool)> = self.fold_marks(index).to_vec();
        let regions: Vec<Region> = self.fold_regions(index).to_vec();
        regions
            .iter()
            .zip(marks.iter())
            .map(|(region, (_, collapsed))| {
                serde_json::json!({
                    "line": region.head + 1,
                    "lastLine": region.last() + 1,
                    "hiddenLines": region.hidden_lines(),
                    "kind": region.kind.name(),
                    "collapsed": collapsed,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::UnluminateApp;

    /// The study's own shape: a function holding a `for` holding an `if`, and a second function
    /// that is not inside the first. The heads are 0, 2, 3 and 9.
    fn a_nested_project(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(
            folder.join("area.rs"),
            "fn total_area() {\n    let s = 0;\n    for side in sides {\n        if side > 0 {\n            s += side;\n        }\n    }\n    s\n}\nfn other() {\n    x();\n}\n",
        )
        .expect("write area.rs");
        folder
    }

    fn a_window(name: &str) -> (std::path::PathBuf, UnluminateApp) {
        let folder = a_nested_project(name);
        let mut app = UnluminateApp::new(&folder);
        app.open_path_permanently(&folder.join("area.rs")).expect("the file opens");
        (folder, app)
    }

    /// Which heads are collapsed, in the order the regions are.
    fn collapsed(app: &mut UnluminateApp) -> Vec<usize> {
        app.collapsed_heads(app.files.active_index())
    }

    #[test]
    fn collapsing_recursively_closes_the_whole_subtree_and_only_it() {
        let (folder, mut app) = a_window("unluminate-fold-recursive-collapse");
        let index = app.files.active_index();
        assert!(app.collapse_recursively_at_line(0), "the function heads line 0");
        // The function, the for and the if are all collapsed; the second function is not.
        assert_eq!(collapsed(&mut app), vec![0, 2, 3]);
        // The hidden set is the body of the function, which swallows the for and the if with it.
        assert_eq!(app.hidden_paragraphs(index).ranges(), &[1..9]);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn expanding_recursively_opens_the_children_that_were_folded_on_their_own() {
        let (folder, mut app) = a_window("unluminate-fold-recursive-expand");
        let index = app.files.active_index();
        // Collapse the function and its children, then open the function recursively: the children
        // open too, which is the decision of the TDD — a recursive expand is a hard open of the
        // whole subtree, not the set's natural "remove the parent and keep the children".
        app.collapse_recursively_at_line(0);
        assert!(app.expand_recursively_at_line(0));
        assert!(collapsed(&mut app).is_empty(), "the whole subtree is open");
        assert!(app.hidden_paragraphs(index).is_empty());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn collapsing_recursively_twice_is_a_no_op_the_second_time() {
        let (folder, mut app) = a_window("unluminate-fold-recursive-noop");
        assert!(app.collapse_recursively_at_line(0), "the first time it changes");
        assert!(!app.collapse_recursively_at_line(0), "the second time it does not");
        assert_eq!(collapsed(&mut app), vec![0, 2, 3]);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_caret_inside_a_recursively_collapsed_block_is_brought_onto_its_head() {
        let (folder, mut app) = a_window("unluminate-fold-recursive-caret");
        let index = app.files.active_index();
        // Put the caret on a line inside the function that is about to be hidden.
        let offset = app.files.at(index).document.text().line_to_byte(4);
        app.files.at_mut(index).document.apply(unluminate_core::Command::PlaceCaret {
            offset,
            extend: false,
        });
        assert!(app.collapse_recursively_at_line(0));
        let document = &app.files.at(index).document;
        let caret = document.text().byte_to_line(document.selection().head);
        assert_eq!(caret, 0, "the caret is on the head line, not inside the fold");
        std::fs::remove_dir_all(&folder).ok();
    }
}
