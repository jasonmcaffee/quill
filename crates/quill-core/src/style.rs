//! Character formatting and paragraph formatting.
//!
//! Character formatting is a list of spans covering the whole document with no gaps and no overlaps.
//! A span stores a length rather than an absolute range, which means inserting text grows one span
//! instead of shifting every span after the insertion point.
//!
//! The idea of holding formatting as ranges over the text comes from cosmic-text
//! (<https://github.com/pop-os/cosmic-text>, commit daae9c75), whose `src/attrs.rs` keeps an
//! `AttrsList` as a range map from byte ranges to attributes. Ours is a plain sorted vector, because a
//! document of the size Quill targets never holds enough spans for the difference to matter and a
//! vector is much easier to test.

use std::ops::Range;

/// A colour with no alpha. Text in Quill is always fully opaque, so there is no alpha to store; see
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
        let runs = self.runs_in(range);
        !runs.is_empty() && runs.iter().all(|(_, style)| predicate(style))
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
        let mut i = 0;
        while i + 1 < self.spans.len() {
            if self.spans[i].style == self.spans[i + 1].style {
                let merged = self.spans.remove(i + 1);
                self.spans[i].len += merged.len;
            } else if self.spans[i].len == 0 && self.spans.len() > 1 {
                self.spans.remove(i);
            } else {
                i += 1;
            }
        }
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
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self { align: Align::Left, line_spacing: 1.0 }
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
