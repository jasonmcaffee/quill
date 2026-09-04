//! Marked passages: a colour behind a range of bytes, kept where the text is.
//!
//! `task-1663` asks for a way to mark blocks of a file — a person reading unfamiliar code marking
//! the four places a bug could be in, or an agent marking every place it is about to change — and
//! for the marks to still be there next time the file is opened.
//!
//! A highlight is not character formatting and does not belong in [`crate::style::StyleSpans`].
//! Formatting covers the whole document with no gaps, which would make a document with two marked
//! words hold five spans, three of them saying "nothing here"; it carries no alpha, deliberately,
//! because text in Unluminate is always fully opaque; and it is inherited by whatever is typed next,
//! which a mark somebody drew over a passage must not be.
//!
//! So this is a **sparse** set instead, and it keeps one invariant that everything else here rests
//! on: the highlights are **sorted by where they start, they never overlap, and none of them is
//! empty**. That is what makes finding the one under the caret a binary search, finding the handful
//! on the screen a binary search and a walk, and `Clear Highlight` have a single answer.
//!
//! Two overlapping colours would give a third colour nobody chose, so adding one cuts away whatever
//! it lands on first. That is what a marker pen does.

use std::ops::Range;

/// A colour with an opacity, which is what a highlight is drawn in.
///
/// [`crate::style::Color`] has no alpha and says why: text is always painted fully opaque. A
/// highlight is a background rather than a letter, and the ask is explicit that its opacity can be
/// chosen, so it needs a fourth number and gets a type of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `#RRGGBBAA`, which is how a highlight is written down and how the command line takes one.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }

    /// Read `#RGB`, `#RRGGBB` or `#RRGGBBAA`, with or without the hash.
    ///
    /// A colour with no alpha given is fully opaque, which is what `#RRGGBB` means everywhere else.
    /// A caller that wants the usual translucent mark says so with the fourth pair.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let pair = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        match digits.len() {
            3 => {
                let one = |at: usize| {
                    u8::from_str_radix(&digits[at..at + 1], 16).ok().map(|value| value * 17)
                };
                Some(Self::new(one(0)?, one(1)?, one(2)?, 0xFF))
            }
            6 => Some(Self::new(pair(0)?, pair(2)?, pair(4)?, 0xFF)),
            8 => Some(Self::new(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
            _ => None,
        }
    }
}

/// One marked passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlight {
    pub range: Range<usize>,
    pub color: Rgba,
}

/// Every marked passage in one document.
///
/// Sorted by where each one starts, never overlapping, never empty. Every method here keeps that
/// invariant, and [`Highlights::check`] is what the tests assert it with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlights {
    marks: Vec<Highlight>,
}

impl Highlights {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a list that may be in any order and may overlap, which is what reading a file
    /// written by hand, or a bulk request from the command line, produces.
    pub fn from_list(marks: impl IntoIterator<Item = Highlight>) -> Self {
        let mut out = Self::new();
        for mark in marks {
            out.add(mark.range, mark.color);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.marks.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Highlight> {
        self.marks.iter()
    }

    /// Mark `range` in `color`, cutting away whatever was already there.
    ///
    /// Replacing rather than layering, because two translucent colours over one another make a third
    /// colour nobody chose and because the one under the pointer has to have a single answer.
    pub fn add(&mut self, range: Range<usize>, color: Rgba) {
        if range.start >= range.end {
            return;
        }
        self.cut(range.clone());
        let at = self.marks.partition_point(|mark| mark.range.start < range.start);
        self.marks.insert(at, Highlight { range, color });
        self.merge_neighbours(at);
    }

    /// Take away everything `range` touches. True when anything was taken away.
    ///
    /// A highlight that only partly overlaps is trimmed rather than removed, so clearing the middle
    /// of a long mark leaves the two ends marked.
    pub fn clear(&mut self, range: Range<usize>) -> bool {
        if range.start >= range.end {
            return false;
        }
        let was = self.marks.clone();
        self.cut(range);
        was != self.marks
    }

    /// Take away the one covering `offset`. True when there was one.
    pub fn clear_at(&mut self, offset: usize) -> bool {
        let Some(index) = self.index_at(offset) else {
            return false;
        };
        self.marks.remove(index);
        true
    }

    /// Take them all away. True when there were any.
    pub fn clear_all(&mut self) -> bool {
        let had = !self.marks.is_empty();
        self.marks.clear();
        had
    }

    /// The highlight covering `offset`, by binary search.
    ///
    /// The end is exclusive, so the caret sitting just past the last character of a mark is not in
    /// it — which is what makes clicking between two marks unambiguous.
    pub fn at(&self, offset: usize) -> Option<&Highlight> {
        self.index_at(offset).map(|index| &self.marks[index])
    }

    /// Everything `range` touches, as a slice of the sorted list.
    ///
    /// This is what painting asks for, with the range that is on the screen: a binary search to the
    /// first candidate and then a walk, so a file with a thousand marks costs a frame the dozen that
    /// can be seen.
    pub fn overlapping(&self, range: Range<usize>) -> &[Highlight] {
        if range.start >= range.end || self.marks.is_empty() {
            return &[];
        }
        // The first mark that could reach into the range is the one before the first that starts at
        // or after it, so the search starts one earlier and the walk drops it if it stops short.
        let mut first = self.marks.partition_point(|mark| mark.range.start < range.start);
        if first > 0 && self.marks[first - 1].range.end > range.start {
            first -= 1;
        }
        let last = self.marks.partition_point(|mark| mark.range.start < range.end);
        &self.marks[first.min(last)..last]
    }

    /// `len` bytes were typed in at `at`.
    ///
    /// A mark grows only when the text landed **strictly inside** it. Text typed at either end is
    /// left outside, because a highlight is something somebody drew over a passage rather than a
    /// property the next letter inherits — which is the opposite of what character formatting does
    /// at the edge of a bold word, and is the difference on purpose.
    pub fn insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for mark in &mut self.marks {
            if mark.range.start >= at {
                mark.range.start += len;
                mark.range.end += len;
            } else if mark.range.end > at {
                mark.range.end += len;
            }
        }
    }

    /// `range` was deleted from the text.
    ///
    /// A mark the deletion swallowed whole goes, rather than being left as a mark of no width that
    /// cannot be seen or clicked.
    pub fn remove(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            return;
        }
        let len = range.end - range.start;
        let shift = |offset: usize| {
            if offset <= range.start {
                offset
            } else if offset >= range.end {
                offset - len
            } else {
                range.start
            }
        };
        for mark in &mut self.marks {
            mark.range = shift(mark.range.start)..shift(mark.range.end);
        }
        self.marks.retain(|mark| mark.range.start < mark.range.end);
    }

    /// Bring every mark inside a document of `len` bytes.
    ///
    /// What a file that changed on the disk underneath Unluminate needs: the offsets were written against
    /// bytes that are no longer there, and a range reaching past the end of the rope is a panic in
    /// the layout engine rather than a wrong colour.
    pub fn clamp(&mut self, len: usize) {
        for mark in &mut self.marks {
            mark.range.start = mark.range.start.min(len);
            mark.range.end = mark.range.end.min(len);
        }
        self.marks.retain(|mark| mark.range.start < mark.range.end);
    }

    /// True when the invariant holds: sorted, never overlapping, never empty. The tests assert it
    /// after every operation, which is how a change here is stopped from quietly breaking the
    /// binary searches that rest on it.
    pub fn check(&self) -> bool {
        self.marks.windows(2).all(|pair| pair[0].range.end <= pair[1].range.start)
            && self.marks.iter().all(|mark| mark.range.start < mark.range.end)
    }

    fn index_at(&self, offset: usize) -> Option<usize> {
        let after = self.marks.partition_point(|mark| mark.range.start <= offset);
        let index = after.checked_sub(1)?;
        (self.marks[index].range.end > offset).then_some(index)
    }

    /// Take `range` out of everything it touches, leaving the parts either side of it.
    fn cut(&mut self, range: Range<usize>) {
        let mut out: Vec<Highlight> = Vec::with_capacity(self.marks.len() + 1);
        for mark in self.marks.drain(..) {
            if mark.range.end <= range.start || mark.range.start >= range.end {
                out.push(mark);
                continue;
            }
            if mark.range.start < range.start {
                out.push(Highlight { range: mark.range.start..range.start, color: mark.color });
            }
            if mark.range.end > range.end {
                out.push(Highlight { range: range.end..mark.range.end, color: mark.color });
            }
        }
        self.marks = out;
    }

    /// Join the mark at `index` to the ones either side of it when they touch and match, so
    /// highlighting two halves of a word in one colour leaves one mark rather than two.
    fn merge_neighbours(&mut self, index: usize) {
        if index + 1 < self.marks.len()
            && self.marks[index].range.end == self.marks[index + 1].range.start
            && self.marks[index].color == self.marks[index + 1].color
        {
            let next = self.marks.remove(index + 1);
            self.marks[index].range.end = next.range.end;
        }
        if index > 0
            && self.marks[index - 1].range.end == self.marks[index].range.start
            && self.marks[index - 1].color == self.marks[index].color
        {
            let here = self.marks.remove(index);
            self.marks[index - 1].range.end = here.range.end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YELLOW: Rgba = Rgba::new(0xE8, 0xC0, 0x4A, 0x59);
    const BLUE: Rgba = Rgba::new(0x48, 0x9F, 0xF8, 0x59);

    fn marks() -> Highlights {
        let mut marks = Highlights::new();
        marks.add(10..20, YELLOW);
        marks.add(30..40, BLUE);
        marks
    }

    #[test]
    fn a_new_set_is_empty_and_answers_nothing() {
        let marks = Highlights::new();
        assert!(marks.is_empty());
        assert_eq!(marks.at(0), None);
        assert!(marks.overlapping(0..100).is_empty());
    }

    #[test]
    fn they_are_kept_in_order_however_they_are_added() {
        let mut marks = Highlights::new();
        marks.add(30..40, BLUE);
        marks.add(10..20, YELLOW);
        marks.add(50..60, YELLOW);
        assert!(marks.check());
        let starts: Vec<usize> = marks.iter().map(|mark| mark.range.start).collect();
        assert_eq!(starts, vec![10, 30, 50]);
    }

    #[test]
    fn an_empty_range_marks_nothing() {
        let mut marks = Highlights::new();
        marks.add(10..10, YELLOW);
        assert!(marks.is_empty(), "a caret with no selection has nothing to mark");
    }

    #[test]
    fn a_new_mark_cuts_away_whatever_it_lands_on() {
        let mut marks = marks();
        marks.add(15..35, YELLOW);
        assert!(marks.check());
        // The blue mark is trimmed back to what the new one does not cover, and the yellow left of
        // it joins the new one because they touch and match.
        assert_eq!(
            marks.iter().map(|mark| (mark.range.clone(), mark.color)).collect::<Vec<_>>(),
            vec![(10..35, YELLOW), (35..40, BLUE)],
        );
    }

    #[test]
    fn marking_the_two_halves_of_a_passage_in_one_colour_leaves_one_mark() {
        let mut marks = Highlights::new();
        marks.add(10..20, YELLOW);
        marks.add(20..30, YELLOW);
        assert_eq!(marks.len(), 1, "they touch and they match, so they are one");
        assert_eq!(marks.iter().next().unwrap().range, 10..30);
    }

    #[test]
    fn two_colours_that_touch_stay_two_marks() {
        let mut marks = Highlights::new();
        marks.add(10..20, YELLOW);
        marks.add(20..30, BLUE);
        assert_eq!(marks.len(), 2);
    }

    #[test]
    fn the_one_under_an_offset_is_found_and_the_end_is_not_in_it() {
        let marks = marks();
        assert_eq!(marks.at(9), None);
        assert_eq!(marks.at(10).map(|mark| mark.color), Some(YELLOW));
        assert_eq!(marks.at(19).map(|mark| mark.color), Some(YELLOW));
        assert_eq!(marks.at(20), None, "the end is exclusive");
        assert_eq!(marks.at(35).map(|mark| mark.color), Some(BLUE));
    }

    #[test]
    fn only_what_the_range_touches_is_returned() {
        let marks = marks();
        assert_eq!(marks.overlapping(0..5).len(), 0);
        assert_eq!(marks.overlapping(0..15).len(), 1, "a mark reaching into the range counts");
        assert_eq!(marks.overlapping(15..35).len(), 2);
        assert_eq!(marks.overlapping(20..30).len(), 0, "the gap between them holds nothing");
        assert_eq!(marks.overlapping(45..90).len(), 0);
    }

    #[test]
    fn clearing_the_middle_of_a_mark_leaves_the_two_ends() {
        let mut marks = Highlights::new();
        marks.add(10..40, YELLOW);
        assert!(marks.clear(20..30));
        assert!(marks.check());
        assert_eq!(
            marks.iter().map(|mark| mark.range.clone()).collect::<Vec<_>>(),
            vec![10..20, 30..40]
        );
    }

    #[test]
    fn clearing_where_there_is_nothing_says_so() {
        let mut marks = marks();
        assert!(!marks.clear(21..29));
        assert_eq!(marks.len(), 2);
    }

    #[test]
    fn clearing_at_an_offset_takes_the_whole_mark() {
        let mut marks = marks();
        assert!(marks.clear_at(15));
        assert_eq!(marks.len(), 1);
        assert!(!marks.clear_at(15), "there is nothing there now");
    }

    #[test]
    fn typing_inside_a_mark_grows_it_and_typing_at_either_edge_does_not() {
        let mut marks = Highlights::new();
        marks.add(10..20, YELLOW);

        let mut inside = marks.clone();
        inside.insert(15, 3);
        assert_eq!(inside.iter().next().unwrap().range, 10..23, "typed into it, so it grew");

        let mut in_front = marks.clone();
        in_front.insert(10, 3);
        assert_eq!(in_front.iter().next().unwrap().range, 13..23, "pushed along, not grown");

        let mut behind = marks.clone();
        behind.insert(20, 3);
        assert_eq!(behind.iter().next().unwrap().range, 10..20, "the mark ends where it did");
    }

    #[test]
    fn typing_above_a_mark_carries_it_down_by_exactly_what_was_typed() {
        let mut marks = marks();
        marks.insert(0, 7);
        assert_eq!(
            marks.iter().map(|mark| mark.range.clone()).collect::<Vec<_>>(),
            vec![17..27, 37..47]
        );
    }

    #[test]
    fn deleting_the_text_under_a_mark_takes_the_mark_with_it() {
        let mut marks = marks();
        marks.remove(8..22);
        assert!(marks.check());
        assert_eq!(marks.len(), 1, "the first mark's text is gone, so the mark is");
        assert_eq!(marks.iter().next().unwrap().range, 16..26);
    }

    #[test]
    fn deleting_part_of_a_mark_shrinks_it() {
        let mut marks = Highlights::new();
        marks.add(10..40, YELLOW);
        marks.remove(20..30);
        assert_eq!(marks.iter().next().unwrap().range, 10..30);
    }

    #[test]
    fn a_file_that_shrank_underneath_us_leaves_no_range_past_the_end() {
        let mut marks = marks();
        marks.clamp(25);
        assert!(marks.check());
        assert_eq!(
            marks.iter().map(|mark| mark.range.clone()).collect::<Vec<_>>(),
            vec![10..20],
            "the second mark is entirely past the end, so it goes"
        );
    }

    #[test]
    fn a_colour_reads_back_as_it_was_written() {
        let colour = Rgba::new(0xE8, 0xC0, 0x4A, 0x59);
        assert_eq!(colour.to_hex(), "#E8C04A59");
        assert_eq!(Rgba::parse("#E8C04A59"), Some(colour));
        assert_eq!(Rgba::parse("e8c04a59"), Some(colour));
        assert_eq!(Rgba::parse("#FF0000"), Some(Rgba::new(0xFF, 0, 0, 0xFF)));
        assert_eq!(Rgba::parse("#F00"), Some(Rgba::new(0xFF, 0, 0, 0xFF)));
        assert_eq!(Rgba::parse("#12345"), None);
        assert_eq!(Rgba::parse("blue"), None);
    }

    #[test]
    fn a_list_in_any_order_with_overlaps_in_it_comes_out_sorted_and_apart() {
        let marks = Highlights::from_list([
            Highlight { range: 30..40, color: BLUE },
            Highlight { range: 10..20, color: YELLOW },
            Highlight { range: 15..35, color: YELLOW },
        ]);
        assert!(marks.check());
        assert_eq!(
            marks.iter().map(|mark| mark.range.clone()).collect::<Vec<_>>(),
            vec![10..35, 35..40]
        );
    }
}
