//! Find and Replace inside the file that is open.
//!
//! `task-1804` §3.1: *"An editor that ships without Ctrl+F is an editor that gets one review."*
//! Unluminate had `Find in Files` on `Ctrl/Cmd+Shift+F` and nothing at all on `Ctrl/Cmd+F`, and no
//! Replace anywhere — `editor rename` renames a *symbol*, which is a different and better thing and
//! does not help with a string, a comment, a URL or a number.
//!
//! **Everything about the search that can be decided without a window is decided here**, which is
//! `services::file_search` and `services::text_search`'s own arrangement: this file holds what is
//! being looked for, where the matches are and which one is current, and `components::find_bar`
//! draws it. So the whole of the behaviour — the wrap round the end, what a changed document does to
//! the current match, what Replace All leaves selected — is a unit test with no graphics card.
//!
//! ## The matches are recomputed rather than shifted
//!
//! An edit moves every offset after it, and this file's answer to that is to work the matches out
//! again from the text, keyed on the document's `text_revision`. Shifting them would be a second
//! implementation of the rule `Document::insert` and `Document::remove_range` already keep for the
//! marks, the folds and the breakpoints — and unlike those, a search match is **derived** from the
//! text rather than state somebody put there, so there is nothing to preserve across an edit that a
//! fresh reading would not give.
//!
//! Measured on a 2 MB file: `unluminate-app --example frame_cost` does the same scan for the
//! tokeniser, and searching for a five character word over 2 MB is a `str::find` per line.
//!
//! ## Replace All is one undo step
//!
//! Through `Command::ReplaceMany`, which is what `editor rename` is applied by and which exists for
//! exactly this reason: undo restores a snapshot, so one snapshot and then every edit is one step.
//! Replacing forty occurrences one at a time would be forty presses of `Ctrl+Z` to get back.

use std::ops::Range;

/// Which of the two fields the keyboard is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Field {
    #[default]
    Find,
    Replace,
}

/// What is being looked for in the open file, and where it was found.
#[derive(Debug, Clone, Default)]
pub struct Find {
    /// What is typed in the Find box.
    pub needle: String,
    /// What is typed in the Replace box.
    pub replacement: String,
    /// False when `readme` finds `README`, which is what a search box does unless told otherwise.
    pub match_case: bool,
    /// True when a match has to be a whole word: the characters either side of it are not letters,
    /// digits or underscores.
    ///
    /// This is the ordinary Find bar's whole-word toggle rather than `Find References`' reading of
    /// the same words — that one asks the grammar what a hit was found *inside* and is a different
    /// and more expensive question. `task-1675` is where that lives.
    pub whole_word: bool,
    /// True when the Replace row is showing. `Ctrl+H` opens it; `Ctrl+F` opens the bar without it.
    pub replacing: bool,
    /// Which field the keyboard is in.
    pub field: Field,
    /// Where every match is, in bytes into the whole document.
    matches: Vec<Range<usize>>,
    /// Which of them is current, as an index into `matches`. Meaningless when there are none.
    current: usize,
    /// The question the matches answer: the document revision, and the three things that decide a
    /// match. When any of them moves the matches are worked out again.
    answered: Option<Question>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Question {
    revision: u64,
    needle: String,
    match_case: bool,
    whole_word: bool,
}

impl Find {
    /// A fresh bar, opened over `selection` — whatever was selected when the key was pressed.
    ///
    /// **The selection seeds the box**, which is what every editor does and what makes
    /// select-a-word-then-`Ctrl+F` one gesture rather than two. A selection spanning more than one
    /// line is not used, because a Find bar is one line high and a needle with a line break in it
    /// would be a search nothing could show.
    pub fn opened_with(selection: Option<&str>, replacing: bool) -> Self {
        let needle = selection
            .map(str::to_owned)
            .filter(|text| !text.is_empty() && !text.contains('\n'))
            .unwrap_or_default();
        Self { needle, replacing, ..Self::default() }
    }

    /// Work the matches out again if anything they depend on has moved.
    ///
    /// Called once a frame with the document as it is now. Cheap when nothing changed: four
    /// comparisons and no allocation, which is [`crate::services::vello_canvas`]'s rule about a
    /// frame where nothing happened, applied to a search.
    ///
    /// **Answers whether the *search* changed** -- the words in the box or the two toggles -- and
    /// not whether the *text* did. The window selects the current match when it did, which is what
    /// makes typing into the box walk the file as you type. The distinction matters: an edit to the
    /// document also recomputes the matches, and re-selecting then would pull the caret away from
    /// somebody who is typing in the file with the bar left open.
    pub fn refresh(&mut self, text: &str, revision: u64) -> bool {
        let asking = Question {
            revision,
            needle: self.needle.clone(),
            match_case: self.match_case,
            whole_word: self.whole_word,
        };
        if self.answered.as_ref() == Some(&asking) {
            return false;
        }
        let search_changed = match self.answered.as_ref() {
            Some(answered) => {
                answered.needle != asking.needle
                    || answered.match_case != asking.match_case
                    || answered.whole_word != asking.whole_word
            }
            None => true,
        };
        // Where the current match started, so that after an edit the bar stays on the match nearest
        // to where it was rather than jumping back to the first one in the file. Without this,
        // Replace-then-Replace walks the file from the top again after every replacement.
        let was_at = self.matches.get(self.current).map(|range| range.start);
        self.matches = crate::services::text_search::ranges_in(
            text,
            &self.needle,
            self.match_case,
            self.whole_word,
        );
        self.current = match was_at {
            Some(at) => self
                .matches
                .iter()
                .position(|range| range.start >= at)
                .unwrap_or(0),
            None => 0,
        };
        self.answered = Some(asking);
        search_changed
    }

    /// How many matches there are.
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// Which match is current, counting from one, or nothing when there are none.
    pub fn index(&self) -> Option<usize> {
        (!self.matches.is_empty()).then(|| self.current + 1)
    }

    /// Where the current match is.
    pub fn current(&self) -> Option<Range<usize>> {
        self.matches.get(self.current).cloned()
    }

    /// Every match, in the order they appear in the file.
    pub fn all(&self) -> &[Range<usize>] {
        &self.matches
    }

    /// Every match **except the one that is current**, which is what the editing area paints a band
    /// behind.
    ///
    /// The current one is left out because it is the document's *selection* -- the bar selects it,
    /// so `Ctrl+F` then `Ctrl+C` copies it and Escape leaves the caret on it -- and painting the
    /// band over it as well would say the seventeen matches and the one you are on are the same
    /// thing. Two colours for two meanings, which is what `color::find_match` says about itself.
    pub fn others(&self) -> Vec<Range<usize>> {
        self.matches
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != self.current)
            .map(|(_, range)| range.clone())
            .collect()
    }

    /// What the bar says: `3 of 17`, `No results`, or nothing at all before anything is typed.
    ///
    /// Three states rather than two, because an empty box has not failed to find anything — it is
    /// the same distinction `text_search::Query::is_empty` draws for the project search.
    pub fn tally(&self) -> Option<String> {
        if self.needle.is_empty() {
            return None;
        }
        Some(match self.index() {
            Some(index) => format!("{index} of {}", self.matches.len()),
            None => "No results".to_owned(),
        })
    }

    /// Move to the next match, wrapping round the end of the file.
    ///
    /// Wrapping rather than stopping, because a Find bar that stops at the last match makes a person
    /// scroll back to the top to carry on, and every editor wraps. There is no state to say it
    /// wrapped: the tally already says `17 of 17` and then `1 of 17`.
    pub fn next(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.current()
    }

    /// Move to the previous match, wrapping round the start of the file.
    pub fn previous(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = match self.current {
            0 => self.matches.len() - 1,
            at => at - 1,
        };
        self.current()
    }

    /// Put the current match at the first one at or after `offset`, which is the caret.
    ///
    /// What opening the bar does, so the first `Enter` finds the next match **below where you were
    /// reading** rather than the first one in the file.
    pub fn start_from(&mut self, offset: usize) {
        self.current =
            self.matches.iter().position(|range| range.start >= offset).unwrap_or(0);
    }

    /// The one edit that replaces the current match, or nothing when there is no match to replace.
    ///
    /// A value rather than a change, so the decision is testable with no document: the window
    /// applies it. The bar does **not** advance here — the replacement makes the text different, so
    /// `refresh` works the matches out again and lands on the one after it by itself.
    pub fn replacement_for_current(&self) -> Option<(Range<usize>, String)> {
        self.current().map(|range| (range, self.replacement.clone()))
    }

    /// Every edit Replace All would make, back to front.
    ///
    /// Back to front so that no range can shift one still to be made, which is the order
    /// `Command::ReplaceMany` documents as its own requirement and the order `symbols::replacements`
    /// puts a rename in.
    pub fn replacements_for_all(&self) -> Vec<(Range<usize>, String)> {
        self.matches
            .iter()
            .rev()
            .map(|range| (range.clone(), self.replacement.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(needle: &str, text: &str) -> Find {
        let mut find = Find { needle: needle.to_owned(), ..Find::default() };
        find.refresh(text, 1);
        find
    }

    #[test]
    fn every_match_is_found_and_the_tally_counts_them() {
        let find = found("one", "one\ntwo\none more\nnone at all\n");
        assert_eq!(find.count(), 3, "including the one inside 'none'");
        assert_eq!(find.tally().as_deref(), Some("1 of 3"));
    }

    #[test]
    fn a_search_ignores_case_until_it_is_told_not_to() {
        let mut find = found("readme", "README and readme\n");
        assert_eq!(find.count(), 2);
        find.match_case = true;
        find.refresh("README and readme\n", 1);
        assert_eq!(find.count(), 1, "only the one that is spelt that way");
    }

    #[test]
    fn whole_word_leaves_out_the_matches_inside_longer_words() {
        let mut find = found("one", "one\nnone\nbone\none_more\n");
        assert_eq!(find.count(), 4);
        find.whole_word = true;
        find.refresh("one\nnone\nbone\none_more\n", 1);
        assert_eq!(find.count(), 1, "an underscore is a word character, so one_more does not count");
    }

    #[test]
    fn next_and_previous_wrap_round_the_ends_of_the_file() {
        let mut find = found("a", "a b a b a\n");
        assert_eq!(find.index(), Some(1));
        find.next();
        find.next();
        assert_eq!(find.index(), Some(3), "the last one");
        find.next();
        assert_eq!(find.index(), Some(1), "and round to the first");
        find.previous();
        assert_eq!(find.index(), Some(3), "and back round the other way");
    }

    #[test]
    fn nothing_typed_is_not_the_same_as_nothing_found() {
        let empty = found("", "some text\n");
        assert_eq!(empty.tally(), None, "an empty box has not failed to find anything");
        let missing = found("zebra", "some text\n");
        assert_eq!(missing.tally().as_deref(), Some("No results"));
        assert_eq!(missing.index(), None);
        assert_eq!(missing.current(), None);
    }

    #[test]
    fn opening_the_bar_over_a_selection_puts_it_in_the_box() {
        assert_eq!(Find::opened_with(Some("needle"), false).needle, "needle");
        assert_eq!(Find::opened_with(Some(""), false).needle, "");
        assert_eq!(
            Find::opened_with(Some("two\nlines"), false).needle,
            "",
            "a Find bar is one line high, so a needle with a break in it is not taken"
        );
        assert!(Find::opened_with(None, true).replacing, "Ctrl+H opens the Replace row");
    }

    #[test]
    fn the_first_match_is_the_one_after_the_caret() {
        let mut find = found("x", "x\nx\nx\n");
        find.start_from(2);
        assert_eq!(find.index(), Some(2), "the one on the second line");
        find.start_from(99);
        assert_eq!(find.index(), Some(1), "past the last one, round to the first");
    }

    #[test]
    fn replace_all_is_offered_back_to_front_so_no_range_shifts_another() {
        let mut find = found("cat", "cat dog cat\n");
        find.replacement = "bird".to_owned();
        let edits = find.replacements_for_all();
        assert_eq!(edits.len(), 2);
        assert!(edits[0].0.start > edits[1].0.start, "back to front: {edits:?}");
        assert!(edits.iter().all(|(_, with)| with == "bird"));
    }

    /// After an edit the bar stays where it was rather than starting again at the top, which is what
    /// makes Replace, Replace, Replace walk the file once.
    #[test]
    fn after_an_edit_the_current_match_is_the_nearest_one_at_or_after_where_it_was() {
        let text = "cat cat cat\n";
        let mut find = found("cat", text);
        find.next();
        assert_eq!(find.index(), Some(2));
        let at = find.current().expect("a match").start;
        // The text changed under it: the first `cat` became `bird`, so there are two left and the
        // one that was current has moved back by one byte.
        let after = "bird cat cat\n";
        find.refresh(after, 2);
        assert_eq!(find.count(), 2);
        assert!(
            find.current().expect("a match").start >= at.saturating_sub(2),
            "it stayed near where it was rather than jumping to the top"
        );
    }

    #[test]
    fn a_match_that_has_gone_leaves_the_bar_on_the_first_of_what_is_left() {
        let mut find = found("cat", "cat cat\n");
        find.next();
        find.refresh("cat\n", 2);
        assert_eq!(find.count(), 1);
        assert_eq!(find.index(), Some(1));
    }

    #[test]
    fn replacing_the_current_match_is_one_edit_and_the_bar_does_not_move_itself() {
        let mut find = found("cat", "cat dog\n");
        find.replacement = "bird".to_owned();
        let (range, with) = find.replacement_for_current().expect("there is a match");
        assert_eq!(range, 0..3);
        assert_eq!(with, "bird");
        assert_eq!(find.index(), Some(1), "the bar is moved by the refresh, not by this");
    }

    #[test]
    fn only_a_change_to_the_search_asks_the_window_to_move_the_selection() {
        let mut find = Find { needle: "a".to_owned(), ..Find::default() };
        assert!(find.refresh("a b a\n", 1), "the first reading is a change");
        assert!(!find.refresh("a b a\n", 1), "nothing moved");
        assert!(
            !find.refresh("a b a c\n", 2),
            "the document changed and the search did not, so the caret is left alone"
        );
        find.needle = "b".to_owned();
        assert!(find.refresh("a b a c\n", 2), "the words in the box changed");
        find.match_case = true;
        assert!(find.refresh("a b a c\n", 2), "and so does a toggle");
    }

    #[test]
    fn the_band_is_painted_behind_every_match_but_the_one_that_is_current() {
        let mut find = found("a", "a b a b a\n");
        assert_eq!(find.count(), 3);
        assert_eq!(find.others().len(), 2, "the current one is the selection instead");
        assert!(!find.others().contains(&find.current().expect("a match")));
        find.next();
        assert!(!find.others().contains(&find.current().expect("a match")), "and it follows");
    }

    #[test]
    fn nothing_is_recomputed_when_nothing_moved() {
        let mut find = found("a", "a a a\n");
        let before = find.all().to_vec();
        find.refresh("a a a\n", 1);
        assert_eq!(find.all(), before.as_slice());
        // A different revision is a different question even with the same words in the box.
        find.refresh("a a\n", 2);
        assert_eq!(find.count(), 2);
    }
}
