//! The caret and the selection, and how they move over text.
//!
//! Movement left and right is by grapheme cluster, not by byte and not by character. A grapheme
//! cluster is what a reader calls one character even when it is several Unicode code points, such as
//! an accented letter written as a letter followed by a combining accent, or a flag emoji. Moving by
//! byte would land inside a character. Moving by code point would separate the accent from its letter.

use unicode_segmentation::UnicodeSegmentation;

/// Where the caret is, and what is selected.
///
/// `anchor` is where the selection started and `head` is where the caret is now. When they are equal
/// there is a caret and nothing is selected. Holding shift and pressing an arrow key moves `head` and
/// leaves `anchor` where it was, which is how selection by keyboard works everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Default for Selection {
    fn default() -> Self {
        Self::caret(0)
    }
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self { anchor: at, head: at }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Move the caret, dropping any selection.
    pub fn set_caret(&mut self, at: usize) {
        self.anchor = at;
        self.head = at;
    }

    /// Move the caret and keep the anchor, growing or shrinking the selection.
    pub fn extend_to(&mut self, at: usize) {
        self.head = at;
    }

    /// Move to `at`, extending the selection when `extend` is true and collapsing it when it is not.
    pub fn move_to(&mut self, at: usize, extend: bool) {
        if extend {
            self.extend_to(at);
        } else {
            self.set_caret(at);
        }
    }
}

/// The byte offset of the grapheme cluster boundary before `offset`.
///
/// `text` is a window of the document and `base` is the document offset that window starts at.
pub fn prev_grapheme(text: &str, base: usize, offset: usize) -> usize {
    if offset <= base {
        return base;
    }
    let local = offset - base;
    let mut previous = 0;
    for (index, _) in text.grapheme_indices(true) {
        if index >= local {
            break;
        }
        previous = index;
    }
    base + previous
}

/// The byte offset of the grapheme cluster boundary after `offset`.
pub fn next_grapheme(text: &str, base: usize, offset: usize) -> usize {
    let local = offset.saturating_sub(base);
    if local >= text.len() {
        return base + text.len();
    }
    for (index, cluster) in text.grapheme_indices(true) {
        if index >= local {
            return base + index + cluster.len();
        }
    }
    base + text.len()
}

/// True when a word bound from `unicode-segmentation` is a word rather than spacing or punctuation.
fn is_word(bound: &str) -> bool {
    bound.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The start of the word before `offset`. Skips any spacing first, then moves over the word, which is
/// what pressing alt with the left arrow does in every editor.
pub fn prev_word(text: &str, base: usize, offset: usize) -> usize {
    let local = offset.saturating_sub(base);
    let bounds: Vec<(usize, &str)> = text.split_word_bound_indices().collect();
    let mut candidate = 0;
    for (index, bound) in &bounds {
        if *index >= local {
            break;
        }
        if is_word(bound) {
            candidate = *index;
        }
    }
    // If the caret is already at the start of that word, go to the word before it.
    if base + candidate == offset {
        let mut previous = 0;
        for (index, bound) in &bounds {
            if *index >= candidate {
                break;
            }
            if is_word(bound) {
                previous = *index;
            }
        }
        return base + previous;
    }
    base + candidate
}

/// The end of the word after `offset`.
pub fn next_word(text: &str, base: usize, offset: usize) -> usize {
    let local = offset.saturating_sub(base);
    for (index, bound) in text.split_word_bound_indices() {
        let end = index + bound.len();
        if end > local && is_word(bound) {
            return base + end;
        }
    }
    base + text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_selection_is_a_caret() {
        let selection = Selection::caret(5);
        assert!(selection.is_empty());
        assert_eq!(selection.range(), 5..5);
    }

    #[test]
    fn a_selection_reports_its_range_whichever_way_it_was_dragged() {
        let forwards = Selection::new(3, 9);
        let backwards = Selection::new(9, 3);
        assert_eq!(forwards.range(), 3..9);
        assert_eq!(backwards.range(), 3..9);
        assert!(!forwards.is_empty());
    }

    #[test]
    fn extending_moves_the_head_and_leaves_the_anchor() {
        let mut selection = Selection::caret(4);
        selection.move_to(8, true);
        assert_eq!(selection.anchor, 4);
        assert_eq!(selection.head, 8);
        selection.move_to(2, false);
        assert!(selection.is_empty());
        assert_eq!(selection.anchor, 2);
    }

    #[test]
    fn moving_right_over_plain_text_moves_one_byte_at_a_time() {
        let text = "abc";
        assert_eq!(next_grapheme(text, 0, 0), 1);
        assert_eq!(next_grapheme(text, 0, 1), 2);
        assert_eq!(next_grapheme(text, 0, 3), 3, "stops at the end");
    }

    #[test]
    fn moving_over_an_accented_letter_skips_the_whole_letter() {
        // "é" written as the letter e followed by a combining acute accent: three bytes, one cluster.
        let text = "e\u{0301}x";
        assert_eq!(text.len(), 4);
        assert_eq!(next_grapheme(text, 0, 0), 3, "the letter and its accent move together");
        assert_eq!(prev_grapheme(text, 0, 3), 0);
    }

    #[test]
    fn moving_over_an_emoji_skips_the_whole_emoji() {
        // A family emoji: several code points joined by zero width joiners, one cluster.
        let text = "a👨‍👩‍👧‍👦b";
        let after_a = 1;
        let after_emoji = next_grapheme(text, 0, after_a);
        assert!(after_emoji > after_a + 4, "the whole emoji moves as one, not code point by code point");
        assert_eq!(&text[after_emoji..], "b");
        assert_eq!(prev_grapheme(text, 0, after_emoji), after_a);
    }

    #[test]
    fn movement_respects_the_window_base_offset() {
        // The window is the second line of a document whose first line is six bytes long.
        let text = "second";
        assert_eq!(next_grapheme(text, 6, 6), 7);
        assert_eq!(prev_grapheme(text, 6, 6), 6, "cannot move before the start of the window");
        assert_eq!(next_grapheme(text, 6, 12), 12, "cannot move past the end of the window");
    }

    #[test]
    fn word_movement_skips_spacing_and_lands_on_word_edges() {
        let text = "the quick brown fox";
        assert_eq!(next_word(text, 0, 0), 3, "end of 'the'");
        assert_eq!(next_word(text, 0, 3), 9, "end of 'quick'");
        assert_eq!(next_word(text, 0, 4), 9);
        assert_eq!(prev_word(text, 0, 19), 16, "start of 'fox'");
        assert_eq!(prev_word(text, 0, 16), 10, "start of 'brown'");
    }

    #[test]
    fn word_movement_stops_at_the_ends() {
        let text = "one two";
        assert_eq!(next_word(text, 0, 7), 7);
        assert_eq!(prev_word(text, 0, 0), 0);
    }
}
