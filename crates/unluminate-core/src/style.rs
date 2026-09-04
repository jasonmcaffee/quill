//! Character formatting and paragraph formatting.
//!
//! Character formatting is a list of spans covering the whole document with no gaps and no overlaps.
//! A span stores a length rather than an absolute range, which means inserting text grows one span
//! instead of shifting every span after the insertion point.
//!
//! The idea of holding formatting as ranges over the text comes from cosmic-text
//! (<https://github.com/pop-os/cosmic-text>, commit daae9c75), whose `src/attrs.rs` keeps an
//! `AttrsList` as a range map from byte ranges to attributes. Ours is a plain sorted vector, because a
//! document of the size Unluminate targets never holds enough spans for the difference to matter and a
//! vector is much easier to test.

use std::ops::Range;

/// A colour with no alpha. Text in Unluminate is always fully opaque, so there is no alpha to store; see
/// the transparency section of the technical design document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const WHITE: Self = Self::rgb(0xF2, 0xF2, 0xF2);
    pub const RED: Self = Self::rgb(0xE0, 0x4A, 0x4A);
    pub const GREEN: Self = Self::rgb(0x4A, 0xC0, 0x6A);
    pub const BLUE: Self = Self::rgb(0x5A, 0x9A, 0xE8);
    pub const YELLOW: Self = Self::rgb(0xE8, 0xC0, 0x4A);
}

/// How a run of characters looks.
#[derive(Debug, Clone, PartialEq)]
pub struct CharStyle {
    /// A font family name as the operating system knows it, for example `Helvetica`.
    pub family: String,
    /// Size in points.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: Color,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            family: "Helvetica".to_owned(),
            size: 16.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            color: Color::WHITE,
        }
    }
}

/// A change to apply to character formatting. Every field left as `None` keeps whatever the text
/// already had, which is what makes it possible to set the colour of a selection without flattening
/// the bold and italic already in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleChange {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub color: Option<Color>,
}

impl StyleChange {
    pub fn family(name: impl Into<String>) -> Self {
        Self { family: Some(name.into()), ..Self::default() }
    }

    pub fn size(size: f32) -> Self {
        Self { size: Some(size), ..Self::default() }
    }

    pub fn bold(on: bool) -> Self {
        Self { bold: Some(on), ..Self::default() }
    }

    pub fn italic(on: bool) -> Self {
        Self { italic: Some(on), ..Self::default() }
    }

    pub fn underline(on: bool) -> Self {
        Self { underline: Some(on), ..Self::default() }
    }

    pub fn strikethrough(on: bool) -> Self {
        Self { strikethrough: Some(on), ..Self::default() }
    }

    pub fn color(color: Color) -> Self {
        Self { color: Some(color), ..Self::default() }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn apply_to(&self, style: &mut CharStyle) {
        if let Some(family) = &self.family {
            style.family = family.clone();
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
}

#[derive(Debug, Clone, PartialEq)]
struct Span {
    len: usize,
    style: CharStyle,
}

/// The character formatting of one document.
///
/// The spans always cover exactly the document's byte length. An empty document holds one span of
/// length zero, so that formatting chosen before anything is typed is remembered.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleSpans {
    spans: Vec<Span>,
}

impl Default for StyleSpans {
    fn default() -> Self {
        Self::new(0, CharStyle::default())
    }
}

impl StyleSpans {
    pub fn new(len: usize, style: CharStyle) -> Self {
        Self { spans: vec![Span { len, style }] }
    }

    pub fn total_len(&self) -> usize {
        self.spans.iter().map(|s| s.len).sum()
    }

    /// The formatting at a byte offset. An offset on the boundary between two spans reports the
    /// earlier one, so that typing at the end of a bold word stays bold.
    pub fn style_at(&self, offset: usize) -> &CharStyle {
        let mut acc = 0;
        for span in &self.spans {
            if offset <= acc + span.len && span.len > 0 {
                return &span.style;
            }
            acc += span.len;
        }
        &self.spans.last().expect("there is always one span").style
    }

    /// Grow the formatting to cover `len` bytes inserted at `offset`.
    ///
    /// The inserted text takes the formatting of the text it was typed into, which is what a writer
    /// expects when they type inside a bold word.
    pub fn insert(&mut self, offset: usize, len: usize) {
        if len == 0 {
            return;
        }
        let mut acc = 0;
        for span in self.spans.iter_mut() {
            if offset <= acc + span.len {
                span.len += len;
                return;
            }
            acc += span.len;
        }
        self.spans.last_mut().expect("there is always one span").len += len;
    }

    /// Shrink the formatting to match `range` being deleted from the text.
    pub fn remove(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut acc = 0;
        for span in self.spans.iter_mut() {
            let start = acc;
            let end = acc + span.len;
            acc = end;
            let overlap = range.end.min(end).saturating_sub(range.start.max(start));
            span.len -= overlap;
        }
        self.spans.retain(|s| s.len > 0);
        if self.spans.is_empty() {
            self.spans.push(Span { len: 0, style: CharStyle::default() });
        }
        self.merge_neighbours();
    }

    /// Apply `change` to the bytes in `range`.
    pub fn set(&mut self, range: Range<usize>, change: &StyleChange) {
        if range.is_empty() || change.is_empty() {
            return;
        }
        self.split_at(range.start);
        self.split_at(range.end);
        let mut acc = 0;
        for span in self.spans.iter_mut() {
            let start = acc;
            let end = acc + span.len;
            acc = end;
            if start >= range.start && end <= range.end && span.len > 0 {
                change.apply_to(&mut span.style);
            }
        }
        self.merge_neighbours();
    }

    /// The formatting to use for text typed at a caret, or for a selection about to be replaced.
    pub fn style_for_insertion(&self, offset: usize) -> CharStyle {
        self.style_at(offset).clone()
    }

    /// Every span, as an absolute byte range paired with its formatting.
    ///
    /// A span stores a length rather than a position, which is what makes an insertion grow one span
    /// instead of shifting every span after it. The cost is that a caller wanting positions has to
    /// add the lengths up, and doing that inside a loop is how layout came to be O(paragraphs x
    /// spans) — see `tasks/task-1666-performance-tdd.md` section 4. This is the one pass that turns
    /// the list into positions, so a caller can walk it once and then binary search.
    pub fn spans(&self) -> impl Iterator<Item = (Range<usize>, &CharStyle)> + '_ {
        let mut acc = 0;
        self.spans.iter().map(move |span| {
            let start = acc;
            acc += span.len;
            (start..acc, &span.style)
        })
    }

    /// Apply many changes in one pass over the span list.
    ///
    /// `set` is a pass over every span, which is right for one change: a person selecting a word and
    /// pressing bold. Applying a tokeniser's output one call at a time made colouring a file
    /// O(tokens x spans), which was measured at 561 ms for a 169 kilobyte source file. This walks the
    /// spans and the changes together instead, and costs one pass whatever the number of changes.
    ///
    /// The ranges must be in increasing order and must not overlap, which is what a tokeniser
    /// produces. One that is not is **skipped** rather than applied in the wrong place: a colour is
    /// never worth showing the wrong text.
    pub fn set_many(&mut self, changes: &[(Range<usize>, StyleChange)]) {
        let total = self.total_len();
        let mut ordered: Vec<(Range<usize>, &StyleChange)> = Vec::with_capacity(changes.len());
        let mut previous_end = 0;
        for (range, change) in changes {
            let end = range.end.min(total);
            if range.start < previous_end || range.start >= end || change.is_empty() {
                continue;
            }
            previous_end = end;
            ordered.push((range.start..end, change));
        }
        if ordered.is_empty() {
            return;
        }

        let mut out: Vec<Span> = Vec::with_capacity(self.spans.len() + ordered.len() * 2);
        let mut next = 0usize;
        let mut acc = 0usize;
        for span in &self.spans {
            let start = acc;
            let end = acc + span.len;
            acc = end;
            if span.len == 0 {
                // The zero length span an empty document keeps its formatting in.
                out.push(span.clone());
                continue;
            }
            let mut at = start;
            while next < ordered.len() && ordered[next].0.end <= at {
                next += 1;
            }
            while at < end {
                let Some((range, change)) = ordered.get(next) else { break };
                if range.start >= end {
                    break;
                }
                if range.start > at {
                    out.push(Span { len: range.start - at, style: span.style.clone() });
                    at = range.start;
                }
                let stop = range.end.min(end);
                let mut styled = span.style.clone();
                change.apply_to(&mut styled);
                out.push(Span { len: stop - at, style: styled });
                at = stop;
                // A change reaching past this span carries on into the next one, so it is not
                // finished with yet.
                if range.end <= end {
                    next += 1;
                }
            }
            if at < end {
                out.push(Span { len: end - at, style: span.style.clone() });
            }
        }
        self.spans = out;
        self.merge_neighbours();
    }

    /// Walk the runs overlapping `range`, each as a byte range in document coordinates paired with its
    /// formatting. A run is a stretch of text with one style, which is what layout needs: one run is
    /// one font at one size in one colour.
    pub fn runs_in(&self, range: Range<usize>) -> Vec<(Range<usize>, &CharStyle)> {
        let mut out = Vec::new();
        let mut acc = 0;
        for span in &self.spans {
            let start = acc;
            let end = acc + span.len;
            acc = end;
            let from = range.start.max(start);
            let to = range.end.min(end);
            if from < to {
                out.push((from..to, &span.style));
            }
            if acc >= range.end {
                break;
            }
        }
        out
    }

    /// True when every byte in `range` already has `predicate` true of its formatting. Used to decide
    /// whether the bold button should turn bold on or off for a mixed selection.
    pub fn all_in(&self, range: Range<usize>, predicate: impl Fn(&CharStyle) -> bool) -> bool {
        // Walked rather than collected. The toolbar asks this four times a frame — bold, italic,
        // underline and strikethrough — so on a document held in twenty thousand spans, collecting
        // was four lists of them built and thrown away for every frame drawn.
        let mut any = false;
        for (span, style) in self.spans() {
            if span.start >= range.end {
                break;
            }
            if span.end <= range.start || span.is_empty() {
                continue;
            }
            any = true;
            if !predicate(style) {
                return false;
            }
        }
        any
    }

    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// Cut the span that straddles `offset` in two, so that a later loop can treat spans as wholly
    /// inside or wholly outside a range.
    fn split_at(&mut self, offset: usize) {
        let mut acc = 0;
        for i in 0..self.spans.len() {
            let start = acc;
            let end = acc + self.spans[i].len;
            acc = end;
            if offset > start && offset < end {
                let right = Span { len: end - offset, style: self.spans[i].style.clone() };
                self.spans[i].len = offset - start;
                self.spans.insert(i + 1, right);
                return;
            }
        }
    }

    /// Fold neighbouring spans that now hold the same formatting into one.
    ///
    /// Without this the list grows by one span per keystroke and never shrinks.
    fn merge_neighbours(&mut self) {
        // One pass with a write cursor rather than `Vec::remove` in a loop. Removing from the middle
        // shifts everything after it, so folding a twenty thousand span list was itself quadratic,
        // which is half of what made colouring a source file slow.
        let mut write = 0usize;
        for read in 0..self.spans.len() {
            if write > 0 && self.spans[write - 1].style == self.spans[read].style {
                let len = self.spans[read].len;
                self.spans[write - 1].len += len;
                continue;
            }
            if self.spans[read].len == 0 {
                continue;
            }
            if write != read {
                self.spans.swap(write, read);
            }
            write += 1;
        }
        if write == 0 {
            // Every span was empty, which is an empty document. The first one is kept rather than a
            // fresh one made, so that formatting chosen before anything was typed is remembered.
            self.spans.truncate(1);
            return;
        }
        self.spans.truncate(write);
    }
}

/// How the lines of a paragraph are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub const ALL: [Align; 4] = [Align::Left, Align::Center, Align::Right, Align::Justify];

    pub fn label(&self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Center => "Center",
            Align::Right => "Right",
            Align::Justify => "Justify",
        }
    }
}

/// Formatting that belongs to a whole paragraph rather than to a range of characters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphStyle {
    pub align: Align,
    /// A multiplier on the line height. 1.0 is single spacing.
    pub line_spacing: f32,
    /// The least this paragraph may be, in points. Zero means "as tall as its own letters", which
    /// is what every paragraph of a document is.
    ///
    /// It exists for the Markdown preview, where a line holding a picture has to be as tall as the
    /// picture and the layout engine has no notion of anything but glyphs. The application works
    /// out how tall the picture is drawn — which depends on the width of the pane, and so is known
    /// only where the window draws — and asks for a paragraph at least that tall. Keeping it here
    /// rather than teaching layout about images is what keeps this crate free of any user interface
    /// dependency.
    pub min_height: f32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self { align: Align::Left, line_spacing: 1.0, min_height: 0.0 }
    }
}

/// The paragraph formatting of one document, one entry per paragraph.
///
/// A paragraph is the text between two line breaks, so the entry count always equals the document's
/// line count. Alignment and line spacing cannot be held in the character spans, because they belong
/// to the paragraph as a whole: half a paragraph cannot be centred.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphStyles {
    styles: Vec<ParagraphStyle>,
}

impl Default for ParagraphStyles {
    fn default() -> Self {
        Self::new(1)
    }
}

impl ParagraphStyles {
    pub fn new(paragraphs: usize) -> Self {
        Self { styles: vec![ParagraphStyle::default(); paragraphs.max(1)] }
    }

    /// Build from a list already worked out, which is how the Markdown preview supplies its own.
    pub fn from_styles(styles: Vec<ParagraphStyle>) -> Self {
        if styles.is_empty() {
            return Self::new(1);
        }
        Self { styles }
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn get(&self, paragraph: usize) -> ParagraphStyle {
        self.styles.get(paragraph).copied().unwrap_or_default()
    }

    pub fn set(&mut self, paragraphs: Range<usize>, style: impl Fn(&mut ParagraphStyle)) {
        for index in paragraphs {
            if let Some(entry) = self.styles.get_mut(index) {
                style(entry);
            }
        }
    }

    /// Text with `line_breaks` line breaks in it was inserted inside paragraph `paragraph`, so that
    /// paragraph became `line_breaks + 1` paragraphs. The new ones inherit its formatting.
    pub fn split(&mut self, paragraph: usize, line_breaks: usize) {
        if line_breaks == 0 {
            return;
        }
        let inherited = self.get(paragraph);
        let at = (paragraph + 1).min(self.styles.len());
        for _ in 0..line_breaks {
            self.styles.insert(at, inherited);
        }
    }

    /// A deletion joined paragraphs `first` through `last` into one, which keeps `first`'s formatting.
    pub fn join(&mut self, first: usize, last: usize) {
        if last <= first {
            return;
        }
        let from = (first + 1).min(self.styles.len());
        let to = (last + 1).min(self.styles.len());
        self.styles.drain(from..to);
        if self.styles.is_empty() {
            self.styles.push(ParagraphStyle::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold_style() -> CharStyle {
        CharStyle { bold: true, ..CharStyle::default() }
    }

    /// `set_many` has to give exactly the answer a loop of `set` gives, because that loop is what it
    /// replaced. Colouring a source file is where the difference showed: one call per token walked
    /// the whole span list per token, which was 561 ms on a 169 kilobyte file.
    #[test]
    fn set_many_gives_the_same_answer_as_setting_one_at_a_time() {
        let cases: Vec<Vec<(Range<usize>, StyleChange)>> = vec![
            vec![(0..4, StyleChange::bold(true))],
            vec![(0..4, StyleChange::bold(true)), (4..8, StyleChange::italic(true))],
            vec![(2..5, StyleChange::color(Color::RED)), (9..14, StyleChange::color(Color::BLUE))],
            vec![(0..20, StyleChange::size(20.0))],
            vec![(19..20, StyleChange::underline(true))],
            (0..10).map(|i| (i * 2..i * 2 + 1, StyleChange::color(Color::GREEN))).collect(),
        ];
        for changes in cases {
            let mut one_at_a_time = StyleSpans::new(20, CharStyle::default());
            one_at_a_time.set(0..7, &StyleChange::bold(true));
            let mut all_at_once = one_at_a_time.clone();
            for (range, change) in &changes {
                one_at_a_time.set(range.clone(), change);
            }
            all_at_once.set_many(&changes);
            assert_eq!(all_at_once, one_at_a_time, "for {changes:?}");
        }
    }

    /// Out of order or overlapping ranges are skipped rather than applied in the wrong place, because
    /// the alternative is showing the right text in the wrong colour and never finding out.
    #[test]
    fn set_many_skips_a_range_that_goes_backwards_or_overlaps_the_one_before_it() {
        let mut spans = StyleSpans::new(20, CharStyle::default());
        spans.set_many(&[
            (4..8, StyleChange::color(Color::RED)),
            (6..10, StyleChange::color(Color::BLUE)),
            (2..3, StyleChange::color(Color::GREEN)),
            (12..14, StyleChange::color(Color::YELLOW)),
        ]);
        assert_eq!(spans.style_at(5).color, Color::RED);
        assert_eq!(spans.style_at(9).color, CharStyle::default().color, "the overlap was skipped");
        assert_eq!(spans.style_at(2).color, CharStyle::default().color, "going backwards was skipped");
        assert_eq!(spans.style_at(13).color, Color::YELLOW, "and the list carries on afterwards");
        assert_eq!(spans.total_len(), 20, "the spans still cover the document exactly");
    }

    #[test]
    fn set_many_over_an_empty_document_leaves_the_formatting_it_was_given() {
        let mut spans = StyleSpans::new(0, bold_style());
        spans.set_many(&[(0..4, StyleChange::italic(true))]);
        assert_eq!(spans.span_count(), 1);
        assert_eq!(spans.total_len(), 0);
        assert!(spans.style_at(0).bold, "the formatting chosen before anything was typed is kept");
    }

    /// `spans` is what layout walks once instead of asking `runs_in` per paragraph, so the two have
    /// to agree.
    #[test]
    fn spans_reports_the_same_runs_runs_in_does_for_the_whole_document() {
        let mut spans = StyleSpans::new(30, CharStyle::default());
        spans.set(4..9, &StyleChange::bold(true));
        spans.set(12..20, &StyleChange::color(Color::RED));
        let walked: Vec<(Range<usize>, &CharStyle)> = spans.spans().collect();
        assert_eq!(walked, spans.runs_in(0..30));
        assert_eq!(walked.last().expect("there is always one span").0.end, 30);
    }

    #[test]
    fn a_new_document_has_one_span_of_default_formatting() {
        let spans = StyleSpans::default();
        assert_eq!(spans.span_count(), 1);
        assert_eq!(spans.total_len(), 0);
        assert_eq!(spans.style_at(0), &CharStyle::default());
    }

    #[test]
    fn setting_a_range_splits_into_three_spans() {
        let mut spans = StyleSpans::new(11, CharStyle::default());
        spans.set(6..11, &StyleChange::bold(true));
        assert_eq!(spans.span_count(), 2, "one plain span and one bold span");
        assert_eq!(spans.total_len(), 11);
        assert!(!spans.style_at(0).bold);
        assert!(spans.style_at(7).bold);

        spans.set(2..4, &StyleChange::italic(true));
        assert_eq!(spans.total_len(), 11);
        assert!(spans.style_at(3).italic);
        assert!(!spans.style_at(0).italic);
        assert!(!spans.style_at(5).italic);
    }

    #[test]
    fn a_change_leaves_the_formatting_it_does_not_name_alone() {
        let mut spans = StyleSpans::new(10, bold_style());
        spans.set(0..10, &StyleChange::color(Color::RED));
        assert!(spans.style_at(3).bold, "setting the colour must not clear bold");
        assert_eq!(spans.style_at(3).color, Color::RED);
    }

    #[test]
    fn text_typed_inside_a_span_inherits_its_formatting() {
        let mut spans = StyleSpans::new(10, CharStyle::default());
        spans.set(5..10, &StyleChange::bold(true));
        spans.insert(7, 3);
        assert_eq!(spans.total_len(), 13);
        assert!(spans.style_at(8).bold, "text typed inside the bold run should be bold");
        assert_eq!(spans.span_count(), 2, "typing must not add a span");
    }

    #[test]
    fn typing_never_grows_the_span_list() {
        let mut spans = StyleSpans::new(0, CharStyle::default());
        for i in 0..500 {
            spans.insert(i, 1);
        }
        assert_eq!(spans.total_len(), 500);
        assert_eq!(spans.span_count(), 1, "500 keystrokes must still be one span");
    }

    #[test]
    fn deleting_clips_spans_and_merges_the_neighbours() {
        let mut spans = StyleSpans::new(30, CharStyle::default());
        spans.set(10..20, &StyleChange::bold(true));
        assert_eq!(spans.span_count(), 3);
        // Delete the whole bold run. The plain spans either side are now neighbours holding identical
        // formatting, so they must fold into one.
        spans.remove(10..20);
        assert_eq!(spans.total_len(), 20);
        assert_eq!(spans.span_count(), 1, "the two plain spans should have merged");
        assert!(!spans.style_at(10).bold);
    }

    #[test]
    fn deleting_across_a_boundary_keeps_both_sides() {
        let mut spans = StyleSpans::new(30, CharStyle::default());
        spans.set(10..20, &StyleChange::bold(true));
        spans.remove(5..15);
        assert_eq!(spans.total_len(), 20);
        assert!(!spans.style_at(4).bold);
        assert!(spans.style_at(7).bold, "the surviving half of the bold run is still bold");
    }

    #[test]
    fn runs_in_a_range_report_document_offsets() {
        let mut spans = StyleSpans::new(30, CharStyle::default());
        spans.set(10..20, &StyleChange::bold(true));
        let runs = spans.runs_in(8..25);
        let ranges: Vec<Range<usize>> = runs.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(ranges, vec![8..10, 10..20, 20..25]);
        assert!(!runs[0].1.bold);
        assert!(runs[1].1.bold);
        assert!(!runs[2].1.bold);
    }

    #[test]
    fn all_in_reports_whether_a_selection_is_uniform() {
        let mut spans = StyleSpans::new(30, CharStyle::default());
        spans.set(10..20, &StyleChange::bold(true));
        assert!(spans.all_in(12..18, |s| s.bold), "wholly inside the bold run");
        assert!(!spans.all_in(5..15, |s| s.bold), "half in the bold run is not all bold");
        assert!(!spans.all_in(0..5, |s| s.bold));
    }

    #[test]
    fn removing_everything_leaves_one_empty_span() {
        let mut spans = StyleSpans::new(20, bold_style());
        spans.remove(0..20);
        assert_eq!(spans.total_len(), 0);
        assert_eq!(spans.span_count(), 1);
    }

    #[test]
    fn paragraph_styles_split_when_a_line_break_is_typed() {
        let mut paragraphs = ParagraphStyles::new(1);
        paragraphs.set(0..1, |p| p.align = Align::Center);
        paragraphs.split(0, 1);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs.get(0).align, Align::Center);
        assert_eq!(paragraphs.get(1).align, Align::Center, "the new paragraph inherits");
    }

    #[test]
    fn paragraph_styles_join_and_keep_the_first() {
        let mut paragraphs = ParagraphStyles::new(4);
        paragraphs.set(0..1, |p| p.align = Align::Right);
        paragraphs.set(1..4, |p| p.align = Align::Center);
        paragraphs.join(0, 2);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs.get(0).align, Align::Right, "the first paragraph's formatting survives");
        assert_eq!(paragraphs.get(1).align, Align::Center);
    }

    #[test]
    fn paragraph_styles_never_go_empty() {
        let mut paragraphs = ParagraphStyles::new(3);
        paragraphs.join(0, 5);
        assert_eq!(paragraphs.len(), 1);
    }
}
