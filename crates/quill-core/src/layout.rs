//! Turning a document into positioned glyphs.
//!
//! Layout takes the text, the character formatting, the paragraph formatting and an available width,
//! and produces lines. Each line holds runs, and each run holds positioned clusters that share one
//! style, so one run is one font at one size in one colour. Painting walks that list. Hit testing
//! walks it backwards to turn a mouse position into a position in the text.
//!
//! The shape of this interface follows parley (<https://github.com/linebender/parley>, commit
//! 1aba7cac): styles as ranges over the text, layout producing lines that hold runs that hold
//! positioned glyphs.
//!
//! Version one places one cluster after another from left to right. That is correct for Latin, Greek
//! and Cyrillic and wrong for Arabic and Hindi, which need a shaping step. The `FontMetrics` boundary
//! is where that step goes.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::metrics::FontMetrics;
use crate::rope::Rope;
use crate::style::{Align, CharStyle, ParagraphStyles, StyleSpans};

/// A rectangle in the editor's own coordinates, with the origin at the top left of the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// One grapheme cluster, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedCluster {
    /// The text of the cluster, which is what the painter asks the font for.
    pub text: String,
    /// Where the cluster came from in the document, so a click can be turned back into an offset.
    pub bytes: Range<usize>,
    /// Left edge, relative to the left edge of the text area.
    pub x: f32,
    pub advance: f32,
}

impl PlacedCluster {
    pub fn right(&self) -> f32 {
        self.x + self.advance
    }
}

/// A stretch of one line that shares a single style.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedRun {
    pub style: CharStyle,
    pub clusters: Vec<PlacedCluster>,
}

impl PlacedRun {
    pub fn left(&self) -> f32 {
        self.clusters.first().map(|c| c.x).unwrap_or(0.0)
    }

    pub fn right(&self) -> f32 {
        self.clusters.last().map(PlacedCluster::right).unwrap_or(0.0)
    }
}

/// One line on screen. With word wrap one paragraph is several lines.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLine {
    /// Top edge of the line.
    pub y: f32,
    /// Full height of the line, including the paragraph's line spacing.
    pub height: f32,
    /// Where the baseline sits, measured from the top of the line.
    pub baseline: f32,
    /// The tallest ascent of any style on the line, and the deepest descent.
    ///
    /// Together they are the box the glyphs actually occupy, which is not the same as `height`:
    /// that carries the font's line gap, the reading leading the application adds on top of it for
    /// prose, and the paragraph's line spacing, none of which any glyph reaches into. The caret is
    /// drawn to this box rather than to the line, so it stands exactly as tall as the letters
    /// beside it instead of towering over them by the air that was added between the lines.
    pub ascent: f32,
    pub descent: f32,
    /// The bytes of the document this line covers, not including a trailing line break.
    pub bytes: Range<usize>,
    /// Which paragraph this line belongs to.
    pub paragraph: usize,
    /// True when this is the last line of its paragraph, which justified alignment needs to know.
    pub last_in_paragraph: bool,
    pub runs: Vec<PlacedRun>,
    /// The style to use for a caret sitting on an otherwise empty line.
    pub empty_style: CharStyle,
}

impl PlacedLine {
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    fn clusters(&self) -> impl Iterator<Item = &PlacedCluster> {
        self.runs.iter().flat_map(|run| run.clusters.iter())
    }

    /// Left edge of the first cluster, which alignment moves around.
    pub fn left(&self) -> f32 {
        self.runs.first().map(PlacedRun::left).unwrap_or(0.0)
    }

    /// Right edge of the last cluster.
    pub fn right(&self) -> f32 {
        self.runs.last().map(PlacedRun::right).unwrap_or(self.left())
    }
}

/// A whole document, laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub lines: Vec<PlacedLine>,
    /// The width layout was asked to fit into.
    pub width: f32,
    /// The total height of every line.
    pub height: f32,
}

/// Where a caret should be drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub line: usize,
}

/// Lay `text` out into `width`, measuring with `metrics`.
pub fn layout(
    text: &Rope,
    chars: &StyleSpans,
    paragraphs: &ParagraphStyles,
    metrics: &dyn FontMetrics,
    width: f32,
) -> Layout {
    let width = width.max(1.0);
    let mut lines = Vec::new();
    let mut y = 0.0_f32;

    for paragraph in 0..text.len_lines() {
        let paragraph_style = paragraphs.get(paragraph);
        let bytes = text.line_range(paragraph);
        let source = text.byte_slice(bytes.clone());
        let runs = chars.runs_in(bytes.clone());

        // Flatten the paragraph into clusters, each carrying the index of the run it came from.
        let mut clusters: Vec<(usize, PlacedCluster)> = Vec::new();
        for (run_index, (run_bytes, style)) in runs.iter().enumerate() {
            let local = (run_bytes.start - bytes.start)..(run_bytes.end - bytes.start);
            let run_text = &source[local.clone()];
            for (offset, cluster) in run_text.grapheme_indices(true) {
                let start = run_bytes.start + offset;
                clusters.push((
                    run_index,
                    PlacedCluster {
                        text: cluster.to_owned(),
                        bytes: start..start + cluster.len(),
                        x: 0.0,
                        advance: metrics.advance(cluster, style),
                    },
                ));
            }
        }

        let empty_style = runs
            .first()
            .map(|(_, style)| (*style).clone())
            .unwrap_or_else(|| chars.style_at(bytes.start).clone());

        if clusters.is_empty() {
            // An empty paragraph is still a line, so the caret has somewhere to sit.
            let line_metrics = metrics.line_metrics(&empty_style);
            let height = line_metrics.height() * paragraph_style.line_spacing;
            lines.push(PlacedLine {
                y,
                height,
                baseline: line_metrics.ascent + (height - line_metrics.height()) / 2.0,
                ascent: line_metrics.ascent,
                descent: line_metrics.descent,
                bytes: bytes.clone(),
                paragraph,
                last_in_paragraph: true,
                runs: Vec::new(),
                empty_style,
            });
            y += height;
            continue;
        }

        // Break the clusters into lines that fit the width, preferring to break after a space.
        let mut breaks: Vec<Range<usize>> = Vec::new();
        let mut line_start = 0;
        let mut pen = 0.0_f32;
        let mut last_space: Option<usize> = None;
        for index in 0..clusters.len() {
            let advance = clusters[index].1.advance;
            let is_space = clusters[index].1.text.chars().all(char::is_whitespace);
            if pen + advance > width && index > line_start {
                // Break after the last space if there was one, otherwise break a word that is on its
                // own wider than the line, because the alternative is text running off the edge.
                let at = match last_space {
                    Some(space) if space + 1 > line_start => space + 1,
                    _ => index,
                };
                breaks.push(line_start..at);
                line_start = at;
                pen = clusters[line_start..index + 1].iter().map(|(_, c)| c.advance).sum();
                last_space = if is_space { Some(index) } else { None };
                continue;
            }
            pen += advance;
            if is_space {
                last_space = Some(index);
            }
        }
        breaks.push(line_start..clusters.len());

        let break_count = breaks.len();
        for (index, span) in breaks.into_iter().enumerate() {
            let last_in_paragraph = index + 1 == break_count;
            let slice = &clusters[span.clone()];

            // Trailing spaces do not count towards the width used for alignment, because a centred
            // line should look centred on its visible text.
            let visible = slice
                .iter()
                .rposition(|(_, c)| !c.text.chars().all(char::is_whitespace))
                .map(|last| last + 1)
                .unwrap_or(0);
            let visible_width: f32 = slice[..visible].iter().map(|(_, c)| c.advance).sum();

            let mut extra_per_gap = 0.0;
            let mut offset = match paragraph_style.align {
                Align::Left => 0.0,
                Align::Center => ((width - visible_width) / 2.0).max(0.0),
                Align::Right => (width - visible_width).max(0.0),
                Align::Justify => {
                    // The last line of a paragraph is left aligned. Stretching a short last line to
                    // the full width looks broken, so no typesetter does it.
                    if !last_in_paragraph {
                        let gaps = slice[..visible]
                            .iter()
                            .filter(|(_, c)| c.text.chars().all(char::is_whitespace))
                            .count();
                        if gaps > 0 {
                            extra_per_gap = ((width - visible_width) / gaps as f32).max(0.0);
                        }
                    }
                    0.0
                }
            };

            // Place the clusters, grouping neighbouring clusters that share a run into one PlacedRun.
            let mut placed_runs: Vec<PlacedRun> = Vec::new();
            let mut pen = offset;
            for (run_index, cluster) in slice {
                let mut cluster = cluster.clone();
                cluster.x = pen;
                if cluster.text.chars().all(char::is_whitespace) {
                    cluster.advance += extra_per_gap;
                }
                pen += cluster.advance;
                let same_run = placed_runs
                    .last()
                    .is_some_and(|last| last.style == runs[*run_index].1.clone());
                if same_run {
                    placed_runs.last_mut().expect("checked").clusters.push(cluster);
                } else {
                    placed_runs.push(PlacedRun {
                        style: runs[*run_index].1.clone(),
                        clusters: vec![cluster],
                    });
                }
            }
            offset = 0.0;
            let _ = offset;

            // The line is as tall as the tallest style in it. The descent is collected alongside the
            // ascent rather than being taken from `natural`, because `natural` also carries the line
            // gap and the leading, and the caret is drawn to the glyphs rather than to the line.
            let mut ascent = 0.0_f32;
            let mut descent = 0.0_f32;
            let mut natural = 0.0_f32;
            for run in &placed_runs {
                let line_metrics = metrics.line_metrics(&run.style);
                ascent = ascent.max(line_metrics.ascent);
                descent = descent.max(line_metrics.descent);
                natural = natural.max(line_metrics.height());
            }
            if placed_runs.is_empty() {
                let line_metrics = metrics.line_metrics(&empty_style);
                ascent = line_metrics.ascent;
                descent = line_metrics.descent;
                natural = line_metrics.height();
            }
            let height = natural * paragraph_style.line_spacing;

            let start = slice.first().map(|(_, c)| c.bytes.start).unwrap_or(bytes.start);
            let end = slice.last().map(|(_, c)| c.bytes.end).unwrap_or(bytes.start);

            lines.push(PlacedLine {
                y,
                height,
                // Extra line spacing is added below the text rather than above it, so single and
                // double spaced paragraphs start at the same place.
                baseline: ascent,
                ascent,
                descent,
                bytes: start..end,
                paragraph,
                last_in_paragraph,
                runs: placed_runs,
                empty_style: empty_style.clone(),
            });
            y += height;
        }
    }

    Layout { lines, width, height: y }
}

impl Layout {
    /// Which line holds a document offset. An offset on a line break belongs to the earlier line.
    pub fn line_of_offset(&self, offset: usize) -> usize {
        for (index, line) in self.lines.iter().enumerate() {
            if offset <= line.bytes.end {
                return index;
            }
        }
        self.lines.len().saturating_sub(1)
    }

    /// Which line a vertical position falls on, clamped to the first and last lines so that dragging
    /// above or below the text selects to the start or the end.
    pub fn line_at_y(&self, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        if y < 0.0 {
            return 0;
        }
        for (index, line) in self.lines.iter().enumerate() {
            if y < line.bottom() {
                return index;
            }
        }
        self.lines.len() - 1
    }

    /// The document offset closest to a point, which is what a mouse click needs.
    ///
    /// Clicking in the right half of a character puts the caret after it, which is what every editor
    /// does and what a writer expects.
    pub fn offset_at(&self, x: f32, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let line = &self.lines[self.line_at_y(y)];
        let mut best = line.bytes.start;
        let mut best_distance = f32::INFINITY;
        let mut consider = |offset: usize, at: f32| {
            let distance = (at - x).abs();
            if distance < best_distance {
                best_distance = distance;
                best = offset;
            }
        };
        consider(line.bytes.start, line.left());
        for cluster in line.clusters() {
            consider(cluster.bytes.start, cluster.x);
            consider(cluster.bytes.end, cluster.right());
        }
        best
    }

    /// Where to draw the caret for a document offset.
    pub fn caret_at(&self, offset: usize) -> Caret {
        let index = self.line_of_offset(offset);
        let Some(line) = self.lines.get(index) else {
            return Caret { x: 0.0, y: 0.0, height: 0.0, line: 0 };
        };
        let mut x = line.left();
        for cluster in line.clusters() {
            if offset >= cluster.bytes.end {
                x = cluster.right();
            } else if offset > cluster.bytes.start {
                x = cluster.x;
            }
        }
        if offset <= line.bytes.start {
            x = line.left();
        }
        // The glyph box rather than the line box. A line carries the font's line gap, the reading
        // leading and the paragraph's line spacing on top of the letters, and a caret drawn to the
        // whole of that stands taller than the text it is sitting in — which at double spacing is
        // twice as tall as the writing.
        Caret {
            x,
            y: line.y + line.baseline - line.ascent,
            height: line.ascent + line.descent,
            line: index,
        }
    }

    /// The rectangles to paint behind a selected range, one per line it covers.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        if range.is_empty() {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for line in &self.lines {
            let from = range.start.max(line.bytes.start);
            let to = range.end.min(line.bytes.end);
            if from > to {
                continue;
            }
            // A line break inside the selection is shown as a small tail past the last character, so
            // that selecting several whole lines looks continuous.
            let covers_break = range.end > line.bytes.end && !line.last_in_paragraph_end();
            if from == to && !covers_break {
                continue;
            }
            let mut left = line.right();
            let mut right = line.left();
            for cluster in line.clusters() {
                if cluster.bytes.start >= from && cluster.bytes.end <= to {
                    left = left.min(cluster.x);
                    right = right.max(cluster.right());
                }
            }
            if right < left {
                left = line.right();
                right = left;
            }
            if covers_break {
                right = right.max(left) + line.height * 0.25;
            }
            if right > left {
                rects.push(Rect { x: left, y: line.y, width: right - left, height: line.height });
            }
        }
        rects
    }

    /// The offset on `line` nearest the horizontal position `x`. Vertical movement uses this with a
    /// remembered position, so that moving down through a short line and on to a long one returns to
    /// the original column.
    pub fn offset_on_line_at_x(&self, line: usize, x: f32) -> usize {
        let Some(placed) = self.lines.get(line) else {
            return 0;
        };
        self.offset_at(x, placed.y + placed.height / 2.0)
    }

    /// The rules to draw for underline and strikethrough, as rectangles with the colour to use.
    pub fn decorations(&self, metrics: &dyn FontMetrics) -> Vec<(Rect, crate::style::Color)> {
        let mut out = Vec::new();
        for line in &self.lines {
            for run in &line.runs {
                if !run.style.underline && !run.style.strikethrough {
                    continue;
                }
                let left = run.left();
                let width = run.right() - left;
                if width <= 0.0 {
                    continue;
                }
                let thickness = metrics.rule_thickness(&run.style);
                let baseline = line.y + line.baseline;
                if run.style.underline {
                    out.push((
                        Rect {
                            x: left,
                            y: baseline + metrics.underline_offset(&run.style),
                            width,
                            height: thickness,
                        },
                        run.style.color,
                    ));
                }
                if run.style.strikethrough {
                    out.push((
                        Rect {
                            x: left,
                            y: baseline - metrics.strikethrough_offset(&run.style),
                            width,
                            height: thickness,
                        },
                        run.style.color,
                    ));
                }
            }
        }
        out
    }
}

impl PlacedLine {
    /// True when the selection reaching past this line does not cross a paragraph break, meaning the
    /// line ended because of word wrap rather than because of a line break in the text.
    fn last_in_paragraph_end(&self) -> bool {
        !self.last_in_paragraph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{FixedMetrics, ScaledMetrics};
    use crate::style::{Color, StyleChange};

    /// Every expected number in these tests comes from FixedMetrics: each cluster is 10 wide, the
    /// ascent is 16 and the descent is 4, so a line is 20 tall.
    fn fixture(text: &str) -> (Rope, StyleSpans, ParagraphStyles) {
        let rope = Rope::from_str(text);
        let spans = StyleSpans::new(rope.len_bytes(), CharStyle::default());
        let paragraphs = ParagraphStyles::new(rope.len_lines());
        (rope, spans, paragraphs)
    }

    fn line_texts(layout: &Layout) -> Vec<String> {
        layout
            .lines
            .iter()
            .map(|line| line.runs.iter().flat_map(|r| r.clusters.iter()).map(|c| c.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn one_short_line_is_placed_at_the_origin() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.lines.len(), 1);
        let line = &result.lines[0];
        assert_eq!(line.y, 0.0);
        assert_eq!(line.height, 20.0);
        assert_eq!(line.baseline, 16.0);
        assert_eq!(line.left(), 0.0);
        assert_eq!(line.right(), 30.0, "three clusters at 10 each");
        assert_eq!(result.height, 20.0);
    }

    #[test]
    fn an_empty_document_still_has_a_line_for_the_caret() {
        let (rope, spans, paragraphs) = fixture("");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.lines[0].height, 20.0);
        assert!(result.lines[0].runs.is_empty());
        assert_eq!(result.lines[0].bytes, 0..0);
    }

    #[test]
    fn each_line_break_starts_a_new_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo\nthree");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(line_texts(&result), vec!["one", "two", "three"]);
        assert_eq!(result.lines[0].y, 0.0);
        assert_eq!(result.lines[1].y, 20.0);
        assert_eq!(result.lines[2].y, 40.0);
        assert_eq!(result.lines[1].paragraph, 1);
    }

    #[test]
    fn an_empty_paragraph_between_two_others_takes_a_full_line() {
        let (rope, spans, paragraphs) = fixture("one\n\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.lines.len(), 3);
        assert_eq!(result.lines[1].height, 20.0);
        assert!(result.lines[1].runs.is_empty());
        assert_eq!(result.lines[2].y, 40.0);
    }

    #[test]
    fn a_long_paragraph_wraps_at_a_space() {
        // Width 65 fits six clusters. "the quick" would need nine, so it breaks after "the ".
        let (rope, spans, paragraphs) = fixture("the quick brown fox");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 65.0);
        let texts = line_texts(&result);
        assert!(texts.len() > 1, "it should wrap, got {texts:?}");
        for line in &result.lines {
            let trimmed = line
                .runs
                .iter()
                .flat_map(|r| r.clusters.iter())
                .filter(|c| !c.text.chars().all(char::is_whitespace))
                .map(|c| c.advance)
                .sum::<f32>();
            assert!(trimmed <= 65.0, "line wider than the width: {trimmed}");
        }
        // Every line still belongs to the one paragraph, and only the last is marked as its end.
        assert!(result.lines.iter().all(|l| l.paragraph == 0));
        assert_eq!(result.lines.iter().filter(|l| l.last_in_paragraph).count(), 1);
        // The text is not lost or reordered by wrapping.
        let rejoined: String = texts.join("");
        assert_eq!(rejoined, "the quick brown fox");
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken_rather_than_running_off_the_edge() {
        let (rope, spans, paragraphs) = fixture("abcdefghij");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 35.0);
        assert!(result.lines.len() >= 3, "a 100 wide word in a 35 wide line needs three lines");
        let rejoined: String = line_texts(&result).join("");
        assert_eq!(rejoined, "abcdefghij", "no characters lost");
    }

    #[test]
    fn alignment_moves_the_line_within_the_width() {
        let (rope, spans, mut paragraphs) = fixture("abc");
        let metrics = FixedMetrics::default();

        paragraphs.set(0..1, |p| p.align = Align::Left);
        let left = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(left.lines[0].left(), 0.0);

        paragraphs.set(0..1, |p| p.align = Align::Center);
        let center = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(center.lines[0].left(), 35.0, "(100 - 30) / 2");

        paragraphs.set(0..1, |p| p.align = Align::Right);
        let right = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(right.lines[0].left(), 70.0, "100 - 30");
        assert_eq!(right.lines[0].right(), 100.0);
    }

    #[test]
    fn justified_text_stretches_every_line_but_the_last() {
        let (rope, spans, mut paragraphs) = fixture("aa bb cc dd ee ff");
        paragraphs.set(0..1, |p| p.align = Align::Justify);
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 90.0);
        assert!(result.lines.len() >= 2, "needs to wrap for justification to show");
        for line in &result.lines[..result.lines.len() - 1] {
            let right = line
                .runs
                .iter()
                .flat_map(|r| r.clusters.iter())
                .filter(|c| !c.text.chars().all(char::is_whitespace))
                .map(PlacedCluster::right)
                .fold(0.0_f32, f32::max);
            assert!((right - 90.0).abs() < 0.01, "a justified line should reach the full width, got {right}");
        }
        let last = result.lines.last().expect("at least one line");
        assert_eq!(last.left(), 0.0, "the last line of a justified paragraph is left aligned");
    }

    #[test]
    fn line_spacing_multiplies_the_line_height() {
        let (rope, spans, mut paragraphs) = fixture("one\ntwo");
        paragraphs.set(0..2, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.lines[0].height, 40.0, "20 at single spacing, doubled");
        assert_eq!(result.lines[1].y, 40.0);
        assert_eq!(result.height, 80.0);
        assert_eq!(result.lines[0].baseline, 16.0, "the first baseline does not move");
    }

    #[test]
    fn a_bigger_font_makes_a_taller_line_and_wider_clusters() {
        let (rope, mut spans, paragraphs) = fixture("abcdef");
        spans.set(3..6, &StyleChange::size(32.0));
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 1000.0);
        assert_eq!(result.lines.len(), 1);
        let line = &result.lines[0];
        assert_eq!(line.runs.len(), 2, "one run at 16 point and one at 32");
        assert_eq!(line.runs[0].clusters[0].advance, 8.0, "16 point is 8 wide");
        assert_eq!(line.runs[1].clusters[0].advance, 16.0, "32 point is 16 wide");
        assert_eq!(line.height, 40.0, "the tallest run sets the line height: 32 * 1.25");
        assert_eq!(line.baseline, 32.0, "the baseline follows the tallest ascent");
    }

    #[test]
    fn one_style_change_produces_one_run_per_style() {
        let (rope, mut spans, paragraphs) = fixture("plain bold plain");
        spans.set(6..10, &StyleChange::bold(true));
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        let line = &result.lines[0];
        assert_eq!(line.runs.len(), 3);
        assert!(!line.runs[0].style.bold);
        assert!(line.runs[1].style.bold);
        assert!(!line.runs[2].style.bold);
        let bold: String = line.runs[1].clusters.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(bold, "bold");
        assert_eq!(line.runs[1].left(), 60.0, "six clusters before it");
    }

    #[test]
    fn clicking_in_the_right_half_of_a_character_puts_the_caret_after_it() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.offset_at(0.0, 10.0), 0);
        assert_eq!(result.offset_at(3.0, 10.0), 0, "left of the middle of 'a'");
        assert_eq!(result.offset_at(7.0, 10.0), 1, "right of the middle of 'a'");
        assert_eq!(result.offset_at(14.0, 10.0), 1);
        assert_eq!(result.offset_at(1000.0, 10.0), 3, "past the end of the line");
    }

    #[test]
    fn clicking_below_the_text_lands_on_the_last_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.offset_at(0.0, 25.0), 4, "the start of the second line");
        assert_eq!(result.offset_at(0.0, 900.0), 4, "clamped to the last line");
        assert_eq!(result.offset_at(0.0, -50.0), 0, "clamped to the first line");
    }

    #[test]
    fn the_caret_sits_where_the_offset_is() {
        let (rope, spans, paragraphs) = fixture("one\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.caret_at(0).x, 0.0);
        assert_eq!(result.caret_at(2).x, 20.0);
        assert_eq!(result.caret_at(3).x, 30.0, "end of the first line");
        let second = result.caret_at(4);
        assert_eq!(second.x, 0.0);
        assert_eq!(second.y, 20.0, "start of the second line");
        assert_eq!(second.line, 1);
    }

    #[test]
    fn a_caret_is_the_height_of_the_text_and_not_of_the_line_box() {
        // `ScaledMetrics` gives a line a quarter more height than the letters occupy, and double
        // spacing doubles that again. The caret must stay on the letters: at double spacing it used
        // to be two and a half times the height of the text it was sitting in.
        let (rope, spans, mut paragraphs) = fixture("one\ntwo");
        paragraphs.set(0..2, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 500.0);
        let line = &result.lines[0];
        assert_eq!(line.height, 40.0, "16 point at 1.25 leading, doubled");
        let caret = result.caret_at(0);
        assert_eq!(caret.height, 20.0, "the ascent and the descent, and nothing else");
        assert_eq!(caret.y, 0.0, "which starts at the top of the letters");
        assert!(caret.height < line.height, "a caret must not be taller than its line");
    }

    #[test]
    fn a_caret_on_a_line_of_two_sizes_takes_the_taller() {
        let (rope, mut spans, paragraphs) = fixture("small BIG");
        spans.set(6..9, &StyleChange::size(32.0));
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 1000.0);
        let caret = result.caret_at(0);
        assert_eq!(caret.height, 40.0, "32 point: an ascent of 32 and a descent of 8");
        assert_eq!(caret.y, 0.0, "measured up from the line's own baseline");
    }

    #[test]
    fn a_caret_on_an_empty_line_sits_where_the_text_would() {
        // An empty paragraph centres its baseline in the line, so extra spacing shows above the
        // caret as well as below it. The caret is still only as tall as a letter would be.
        let (rope, spans, mut paragraphs) = fixture("one\n\ntwo");
        paragraphs.set(0..3, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 500.0);
        let empty = result.caret_at(4);
        assert_eq!(empty.line, 1, "the blank line between the two words");
        assert_eq!(empty.height, 20.0);
        assert_eq!(empty.y, 50.0, "the second line starts at 40 and the extra 20 is shared");
    }

    #[test]
    fn a_selection_within_one_line_is_one_rectangle() {
        let (rope, spans, paragraphs) = fixture("abcdef");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let rects = result.selection_rects(2..4);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 20.0);
        assert_eq!(rects[0].width, 20.0);
        assert_eq!(rects[0].height, 20.0);
    }

    #[test]
    fn a_selection_across_lines_is_one_rectangle_per_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo\nthree");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let rects = result.selection_rects(1..10);
        assert_eq!(rects.len(), 3, "part of the first line, all of the second, part of the third");
        assert_eq!(rects[0].y, 0.0);
        assert_eq!(rects[1].y, 20.0);
        assert_eq!(rects[2].y, 40.0);
    }

    #[test]
    fn an_empty_selection_paints_nothing() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert!(result.selection_rects(2..2).is_empty());
    }

    #[test]
    fn underline_and_strikethrough_become_rules_under_and_through_the_run() {
        let (rope, mut spans, paragraphs) = fixture("plain marked");
        spans.set(6..12, &StyleChange::underline(true));
        spans.set(6..12, &StyleChange::strikethrough(true));
        spans.set(6..12, &StyleChange::color(Color::RED));
        let metrics = FixedMetrics::default();
        let result = layout(&rope, &spans, &paragraphs, &metrics, 1000.0);
        let rules = result.decorations(&metrics);
        assert_eq!(rules.len(), 2, "one underline and one strikethrough");
        for (rect, color) in &rules {
            assert_eq!(rect.x, 60.0, "the rules start where the marked run starts");
            assert_eq!(rect.width, 60.0, "six clusters wide");
            assert_eq!(*color, Color::RED, "the rule takes the colour of the text");
        }
        let baseline = result.lines[0].y + result.lines[0].baseline;
        let underline = rules.iter().map(|(r, _)| r.y).fold(f32::MIN, f32::max);
        let strike = rules.iter().map(|(r, _)| r.y).fold(f32::MAX, f32::min);
        assert!(underline > baseline, "the underline sits below the baseline");
        assert!(strike < baseline, "the strikethrough sits above the baseline");
    }

    #[test]
    fn plain_text_has_no_rules_to_draw() {
        let (rope, spans, paragraphs) = fixture("nothing marked here");
        let metrics = FixedMetrics::default();
        let result = layout(&rope, &spans, &paragraphs, &metrics, 1000.0);
        assert!(result.decorations(&metrics).is_empty());
    }

    #[test]
    fn accented_letters_are_one_cluster_wide_not_one_byte_wide() {
        // "é" as a letter plus a combining accent is three bytes and one cluster.
        let (rope, spans, paragraphs) = fixture("e\u{0301}x");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let clusters: Vec<&PlacedCluster> =
            result.lines[0].runs.iter().flat_map(|r| r.clusters.iter()).collect();
        assert_eq!(clusters.len(), 2, "two clusters, not four bytes");
        assert_eq!(clusters[0].bytes, 0..3);
        assert_eq!(clusters[1].x, 10.0);
    }
}
