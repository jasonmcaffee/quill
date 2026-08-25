//! Measuring and wrapping a label.
//!
//! A diagram is mostly boxes that have to be big enough for the words in them, so measuring is the
//! first thing every renderer here does. It goes through [`FontMetrics`], the same seam the editor's
//! layout uses, which is what lets every test in this module state its expected numbers as
//! arithmetic and get the same answer on macOS and on Windows.
//!
//! **Wrapping is at a fixed width, not at the width of the pane.** Two hundred points, which is
//! Mermaid's own `wrappingWidth` default, so a label wraps where Mermaid wraps it. Doing it by the
//! pane instead would mean a diagram that reflowed as the splitter moved and a screenshot test that
//! depended on the window's size.

use crate::metrics::FontMetrics;
use crate::style::CharStyle;

use super::scene::Size;

/// How wide a label is allowed to get before it wraps. Mermaid's own default.
pub const WRAP: f32 = 200.0;

/// How wide an edge label is allowed to get before it wraps.
///
/// Narrower than an ordinary label, and the reason is the rank gap: the space between two ranks has
/// to hold the label, so an unwrapped `the source changed` pushes every rank in a left to right diagram
/// apart by its full width and the picture sprawls. Wrapped, it takes two short lines and the gap
/// stays close to what it is for a diagram with no labels at all.
pub const EDGE_WRAP: f32 = 120.0;

/// A label, split into the lines it will be drawn as and measured.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Label {
    pub lines: Vec<String>,
    /// The widest line.
    pub width: f32,
    /// Every line's height added up.
    pub height: f32,
    /// One line's height, which is what the lines are spaced by.
    pub line_height: f32,
}

impl Label {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

/// Measure `text` in `style`, wrapping it at `wrap` points.
///
/// A line break already in the text — which is what `<br>` became — is always kept. Wrapping only
/// ever adds breaks; it never joins two lines that the author separated.
pub fn measure(text: &str, style: &CharStyle, metrics: &dyn FontMetrics, wrap: f32) -> Label {
    let line_height = metrics.line_metrics(style).height();
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        lines.extend(wrap_one(paragraph, style, metrics, wrap));
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    let width = lines.iter().map(|line| width_of(line, style, metrics)).fold(0.0_f32, f32::max);
    Label { height: line_height * lines.len() as f32, width, line_height, lines }
}

/// Measure `text` without wrapping it, which is what a short label on an edge wants.
pub fn measure_unwrapped(text: &str, style: &CharStyle, metrics: &dyn FontMetrics) -> Label {
    measure(text, style, metrics, f32::INFINITY)
}

/// How wide one line of text is, by adding up its grapheme clusters.
pub fn width_of(text: &str, style: &CharStyle, metrics: &dyn FontMetrics) -> f32 {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).map(|cluster| metrics.advance(cluster, style)).sum()
}

/// Break one paragraph into lines no wider than `wrap`.
///
/// Broken at spaces. A single word wider than `wrap` is left alone rather than cut: a box a little
/// too wide is better than a word split down the middle, which is what a reader would call a bug.
fn wrap_one(text: &str, style: &CharStyle, metrics: &dyn FontMetrics, wrap: f32) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }
    if !wrap.is_finite() || width_of(text, style, metrics) <= wrap {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate =
            if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if !current.is_empty() && width_of(&candidate, style, metrics) > wrap {
            lines.push(std::mem::take(&mut current));
            current = word.to_owned();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FixedMetrics;

    fn style() -> CharStyle {
        CharStyle { size: 14.0, ..CharStyle::default() }
    }

    /// Ten points a cluster and twenty points a line, so every number below is arithmetic.
    fn metrics() -> FixedMetrics {
        FixedMetrics::default()
    }

    #[test]
    fn a_short_label_is_one_line_as_wide_as_its_characters() {
        let label = measure("Start", &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec!["Start"]);
        assert_eq!(label.width, 50.0, "five characters at ten points each");
        assert_eq!(label.height, 20.0, "one line");
    }

    #[test]
    fn a_long_label_wraps_at_the_wrapping_width() {
        // Twenty five characters is 250 points, which is past the 200 point wrap.
        let label = measure("aaaa bbbb cccc dddd eeee f", &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec!["aaaa bbbb cccc dddd", "eeee f"]);
        assert_eq!(label.width, 190.0, "the widest line, not the wrapping width");
        assert_eq!(label.height, 40.0, "two lines");
    }

    #[test]
    fn a_break_the_author_wrote_is_always_kept() {
        let label = measure("One\nTwo", &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec!["One", "Two"]);
        assert_eq!(label.height, 40.0);
    }

    #[test]
    fn wrapping_never_joins_two_lines_the_author_separated() {
        // Both halves fit on one line, and they still must not be run together.
        let label = measure("a\nb", &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec!["a", "b"]);
    }

    #[test]
    fn a_single_word_wider_than_the_wrap_is_left_whole() {
        let long = "a".repeat(40);
        let label = measure(&long, &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec![long], "a word is never cut in half");
        assert_eq!(label.width, 400.0);
    }

    #[test]
    fn an_empty_label_is_one_empty_line_and_not_nothing() {
        // A node with no words in it is still a node, and it still has a height.
        let label = measure("", &style(), &metrics(), WRAP);
        assert_eq!(label.lines, vec![""]);
        assert_eq!(label.height, 20.0);
        assert_eq!(label.width, 0.0);
        assert!(label.is_empty());
    }

    #[test]
    fn measuring_without_wrapping_leaves_a_long_label_on_one_line() {
        let label = measure_unwrapped("aaaa bbbb cccc dddd eeee f", &style(), &metrics());
        assert_eq!(label.lines.len(), 1);
        assert_eq!(label.width, 260.0);
    }
}
