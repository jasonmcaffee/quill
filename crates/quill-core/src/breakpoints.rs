//! Where a program is to stop: a set of lines in one file, kept as byte offsets so that they move
//! with the text.
//!
//! `task-1687` asks for IntelliJ's red dot in the gutter — the thing stepping and inspection are
//! both built on. `tasks/task-1687-debugging-tdd.md` §6 records what was weighed; this module is the
//! half of it that has no window and no debugger in it.
//!
//! **Offsets, not line numbers.** A stored line number is wrong the moment a line is typed at the
//! top of the file, and every offset in Quill is already a byte offset into the real text — which
//! means [`crate::document::Document::insert`] and `remove_range`, the only two places in Quill that
//! know a range of bytes moved, can shift these in the same two lines that already shift the marked
//! passages and the folds. That is what makes a breakpoint stay on its line while the file is edited
//! above it, with no watcher and no bookkeeping anywhere else.
//!
//! Which **line** an offset is on is derived when the adapter asks, and the adapter's answers are
//! converted back the same way, so the two conversions live at the one seam rather than being
//! scattered.
//!
//! **Toggling one is not an edit.** The revision moves so the window repaints; `modified` does not
//! move and no undo step is pushed — the rule the marked passages, the folds and the editor's font
//! already follow. It does ride the undo `Snapshot`, because undo restores a *state*.
//!
//! An offset is the **start of its line**, which is what makes "is there a breakpoint on this line"
//! a binary search rather than a range test, and what makes clicking the gutter twice on one line
//! take the breakpoint away rather than adding a second.

use std::ops::Range;

/// One breakpoint: where it is, whether it is switched on, and the two strings the adapter does the
/// work for.
///
/// A **condition** is an expression the debugger compiles in the debuggee's own language and a
/// **log message** is a string it formats and prints instead of stopping — IntelliJ's "evaluate and
/// log", which the rest of the world calls a logpoint. Both are carried straight through to the
/// adapter in `SourceBreakpoint`, so Quill's whole cost for two of IntelliJ's features is these two
/// fields and the modal that edits them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breakpoint {
    /// The byte offset of the **start of the line** this breakpoint is on.
    pub offset: usize,
    /// False for one that has been switched off without being taken away, which is IntelliJ's
    /// `Disable Breakpoint`: it is drawn hollow and never sent to the adapter.
    pub enabled: bool,
    /// Stop only when this expression is true. Offered only when the adapter said it can.
    pub condition: Option<String>,
    /// Print this instead of stopping. Offered only when the adapter said it can.
    pub log_message: Option<String>,
}

impl Breakpoint {
    /// An ordinary breakpoint, switched on, with no condition — which is what clicking the gutter
    /// makes.
    pub fn at(offset: usize) -> Self {
        Self { offset, enabled: true, condition: None, log_message: None }
    }

    /// True when this one carries a condition or a log message, which is what puts a badge on the
    /// dot rather than leaving it plain.
    pub fn is_conditional(&self) -> bool {
        has_text(&self.condition) || has_text(&self.log_message)
    }
}

/// Every breakpoint in one file.
///
/// Sorted by offset and never holding two at one offset, which is what makes [`Breakpoints::at`] a
/// binary search and what makes toggling unambiguous. [`Breakpoints::check`] is what the tests
/// assert that invariant with, exactly as `Highlights::check` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breakpoints {
    at: Vec<Breakpoint>,
}

impl Breakpoints {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a list in any order, which is what reading a file written by hand produces.
    ///
    /// A second breakpoint at an offset that already has one **replaces** it, because two dots on
    /// one line have no meaning and `Edit Breakpoint...` would have no single answer.
    pub fn from_list(list: impl IntoIterator<Item = Breakpoint>) -> Self {
        let mut out = Self::new();
        for breakpoint in list {
            out.set(breakpoint);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// Every one, in order. What the gutter walks and what is written down.
    pub fn all(&self) -> &[Breakpoint] {
        &self.at
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Breakpoint> {
        self.at.iter()
    }

    /// The one at exactly this offset, by binary search.
    pub fn at(&self, offset: usize) -> Option<&Breakpoint> {
        self.index_of(offset).map(|index| &self.at[index])
    }

    pub fn at_mut(&mut self, offset: usize) -> Option<&mut Breakpoint> {
        self.index_of(offset).map(|index| &mut self.at[index])
    }

    /// Put one in, replacing whatever was at its offset.
    pub fn set(&mut self, breakpoint: Breakpoint) {
        match self.at.binary_search_by_key(&breakpoint.offset, |known| known.offset) {
            Ok(index) => self.at[index] = breakpoint,
            Err(index) => self.at.insert(index, breakpoint),
        }
    }

    /// Take the one at this offset away. True when there was one.
    ///
    /// Named apart from [`Breakpoints::remove`], which is the one the document calls when bytes were
    /// deleted: the two take a number that means different things, and one name for both would be a
    /// trap for whoever writes the next caller.
    pub fn remove_at(&mut self, offset: usize) -> bool {
        match self.index_of(offset) {
            Some(index) => {
                self.at.remove(index);
                true
            }
            None => false,
        }
    }

    /// Put one at this offset, or take away the one that is there.
    ///
    /// True when there is now one there. This is what a click in the gutter is, and it is here
    /// rather than in the window so that the command line's `debug breakpoint add` and the click
    /// cannot come to different answers about what toggling means.
    pub fn toggle(&mut self, offset: usize) -> bool {
        if self.remove_at(offset) {
            return false;
        }
        self.set(Breakpoint::at(offset));
        true
    }

    /// Switch one on or off without taking it away. True when there was one to change.
    pub fn set_enabled(&mut self, offset: usize, enabled: bool) -> bool {
        match self.at_mut(offset) {
            Some(breakpoint) => {
                breakpoint.enabled = enabled;
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.at.is_empty();
        self.at.clear();
        had
    }

    /// Everything inside `range`, as a slice of the sorted list.
    ///
    /// What the gutter asks for with the range that is on screen: a binary search to the first and
    /// then a walk, so a file with a hundred breakpoints costs a frame the handful that can be seen.
    pub fn in_range(&self, range: Range<usize>) -> &[Breakpoint] {
        if range.start >= range.end || self.at.is_empty() {
            return &[];
        }
        let first = self.at.partition_point(|known| known.offset < range.start);
        let last = self.at.partition_point(|known| known.offset < range.end);
        &self.at[first.min(last)..last]
    }

    /// `len` bytes were typed in at `at`.
    ///
    /// An offset **exactly at** the insertion point does not move, for the reason `Folds::insert`
    /// gives: the line still starts there, so typing at the start of a line with a breakpoint on it
    /// adds to that line rather than pushing the breakpoint down the file.
    pub fn insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for breakpoint in &mut self.at {
            if breakpoint.offset > at {
                breakpoint.offset += len;
            }
        }
    }

    /// `range` was deleted from the text.
    ///
    /// A breakpoint inside what went is **kept, at the start of the deletion**, rather than dropped:
    /// selecting three lines and typing over them is one edit, and a person who had marked the
    /// middle of them meant to mark that place in the program rather than those exact bytes.
    /// Whatever ends up at that offset is where the dot is, which is what every editor does.
    /// Two breakpoints that land on the same offset become one, which is the invariant.
    pub fn remove(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            return;
        }
        let len = range.end - range.start;
        for breakpoint in &mut self.at {
            if breakpoint.offset >= range.end {
                breakpoint.offset -= len;
            } else if breakpoint.offset > range.start {
                breakpoint.offset = range.start;
            }
        }
        self.dedupe();
    }

    /// Bring every offset inside a text of `len` bytes.
    ///
    /// What a file that changed on the disk underneath Quill needs, and what reading
    /// `.quill/breakpoints.conf` needs: the offsets were written against bytes that may no longer be
    /// there. `Highlights::clamp`'s rule — a misplaced dot rather than a panic, and the adapter's
    /// `verified` answer then says so honestly.
    pub fn clamp(&mut self, len: usize) {
        for breakpoint in &mut self.at {
            breakpoint.offset = breakpoint.offset.min(len);
        }
        self.dedupe();
    }

    /// True when the invariant holds: sorted, and never two at one offset.
    pub fn check(&self) -> bool {
        self.at.windows(2).all(|pair| pair[0].offset < pair[1].offset)
    }

    fn index_of(&self, offset: usize) -> Option<usize> {
        self.at.binary_search_by_key(&offset, |known| known.offset).ok()
    }

    /// Keep the first of any run that shares an offset. Reached only after a deletion or a clamp
    /// brought two together, and it keeps the earlier one because that is the one whose line
    /// survived.
    fn dedupe(&mut self) {
        self.at.dedup_by_key(|breakpoint| breakpoint.offset);
    }
}

/// True when an optional string has something in it worth sending.
///
/// A person who opened the modal, thought better of it and left the field empty has not asked for a
/// condition of `""` — which some debuggers read as false and would then never stop.
fn has_text(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditional(offset: usize, condition: &str) -> Breakpoint {
        Breakpoint { offset, enabled: true, condition: Some(condition.to_owned()), log_message: None }
    }

    fn three() -> Breakpoints {
        Breakpoints::from_list([Breakpoint::at(10), Breakpoint::at(30), Breakpoint::at(20)])
    }

    #[test]
    fn a_set_built_from_any_order_comes_out_sorted() {
        let breakpoints = three();
        assert!(breakpoints.check());
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn a_second_breakpoint_at_one_offset_replaces_the_first() {
        let mut breakpoints = Breakpoints::new();
        breakpoints.set(Breakpoint::at(10));
        breakpoints.set(conditional(10, "x > 1"));
        assert_eq!(breakpoints.len(), 1, "two dots on one line have no meaning");
        assert_eq!(breakpoints.at(10).and_then(|one| one.condition.clone()), Some("x > 1".to_owned()));
        assert!(breakpoints.check());
    }

    #[test]
    fn toggling_puts_one_there_and_takes_it_away_again() {
        let mut breakpoints = Breakpoints::new();
        assert!(breakpoints.toggle(4), "the first click puts one there");
        assert!(breakpoints.at(4).is_some());
        assert!(!breakpoints.toggle(4), "the second takes it away");
        assert!(breakpoints.at(4).is_none());
        assert!(breakpoints.is_empty());
    }

    #[test]
    fn one_that_is_switched_off_is_still_there() {
        let mut breakpoints = three();
        assert!(breakpoints.set_enabled(20, false));
        assert!(!breakpoints.at(20).expect("still there").enabled);
        assert_eq!(breakpoints.len(), 3, "disabling is not removing");
        assert!(!breakpoints.set_enabled(99, false), "and there is nothing at 99 to change");
    }

    #[test]
    fn a_condition_or_a_log_message_makes_it_conditional_and_a_blank_one_does_not() {
        assert!(conditional(1, "x > 1").is_conditional());
        assert!(!conditional(1, "   ").is_conditional(), "blank is the same as absent");
        assert!(!Breakpoint::at(1).is_conditional());
        let logging = Breakpoint {
            offset: 1,
            enabled: true,
            condition: None,
            log_message: Some("here".to_owned()),
        };
        assert!(logging.is_conditional());
    }

    /// The gutter asks for what is on screen, which is a binary search and a walk.
    #[test]
    fn only_what_is_in_range_is_answered() {
        let breakpoints = three();
        assert_eq!(breakpoints.in_range(0..15).len(), 1);
        assert_eq!(breakpoints.in_range(10..31).len(), 3);
        assert_eq!(breakpoints.in_range(11..30).len(), 1, "the end is exclusive");
        assert!(breakpoints.in_range(40..50).is_empty());
        assert!(breakpoints.in_range(10..10).is_empty(), "an empty range holds nothing");
    }

    /// Text typed above a breakpoint moves it down, and text typed at the start of its own line does
    /// not — the same rule `Folds::insert` keeps, and for the same reason.
    #[test]
    fn text_typed_above_moves_a_breakpoint_and_text_typed_at_its_line_start_does_not() {
        let mut breakpoints = three();
        breakpoints.insert(5, 4);
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![14, 24, 34]
        );
        breakpoints.insert(14, 3);
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![14, 27, 37],
            "the line still starts where it started"
        );
        assert!(breakpoints.check());
    }

    #[test]
    fn text_taken_out_above_moves_a_breakpoint_back() {
        let mut breakpoints = three();
        breakpoints.remove(0..5);
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![5, 15, 25]
        );
        assert!(breakpoints.check());
    }

    /// A breakpoint inside a deletion is kept at the start of it. Selecting three lines and typing
    /// over them is one edit, and the dot lands on whatever ends up there — which is what every
    /// editor does and is what a person means.
    #[test]
    fn a_breakpoint_inside_a_deletion_lands_at_the_start_of_it() {
        let mut breakpoints = three();
        breakpoints.remove(15..25);
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![10, 15, 20]
        );
        assert!(breakpoints.check());
    }

    /// And two that land on one offset become one, which is the invariant everything else rests on.
    #[test]
    fn two_breakpoints_a_deletion_brought_together_become_one() {
        let mut breakpoints = Breakpoints::from_list([
            Breakpoint::at(10),
            conditional(14, "x"),
            Breakpoint::at(18),
        ]);
        breakpoints.remove(12..20);
        assert!(breakpoints.check());
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![10, 12]
        );
    }

    #[test]
    fn an_empty_edit_moves_nothing() {
        let mut breakpoints = three();
        let before = breakpoints.clone();
        breakpoints.insert(5, 0);
        breakpoints.remove(5..5);
        assert_eq!(breakpoints, before);
    }

    /// A file rewritten outside Quill gives a misplaced dot rather than a panic in the layout
    /// engine, which is `Highlights::clamp`'s rule.
    #[test]
    fn offsets_past_the_end_are_brought_inside_it() {
        let mut breakpoints = three();
        breakpoints.clamp(12);
        assert!(breakpoints.check());
        assert_eq!(
            breakpoints.all().iter().map(|one| one.offset).collect::<Vec<_>>(),
            vec![10, 12],
            "the two past the end land on it and become one"
        );
    }

    #[test]
    fn clearing_says_whether_there_was_anything_to_clear() {
        let mut breakpoints = three();
        assert!(breakpoints.clear());
        assert!(!breakpoints.clear());
        assert!(breakpoints.is_empty());
    }
}
