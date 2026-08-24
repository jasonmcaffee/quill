//! The boundary between the editor and the fonts.
//!
//! `quill-core` never asks how a glyph is drawn. It asks for the advance width of a grapheme cluster
//! and for the vertical metrics of a style, and it produces positioned glyphs. The application backs
//! this with real font files through `ab_glyph`. Tests back it with a fixed width stub, so every
//! layout test is arithmetic a reader can check by hand and gives the same answer on macOS and on
//! Windows.

use crate::style::CharStyle;

/// How tall a line of one style is, in the same units as the advance widths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    /// Distance from the baseline up to the top of the tallest glyph.
    pub ascent: f32,
    /// Distance from the baseline down to the bottom of the lowest glyph, as a positive number.
    pub descent: f32,
    /// Extra space the font asks for between one line and the next.
    pub line_gap: f32,
}

impl LineMetrics {
    /// The height of one line at single spacing.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

/// Whatever can measure text for layout.
pub trait FontMetrics {
    /// The width one grapheme cluster takes up in `style`.
    ///
    /// A cluster rather than a character because a cluster is what a reader calls one character even
    /// when it is several code points, such as a letter followed by a combining accent.
    fn advance(&self, cluster: &str, style: &CharStyle) -> f32;

    /// The vertical metrics of `style`.
    fn line_metrics(&self, style: &CharStyle) -> LineMetrics;

    /// Where an underline sits relative to the baseline, as a positive distance below it.
    fn underline_offset(&self, style: &CharStyle) -> f32 {
        style.size * 0.12
    }

    /// How thick an underline or a strikethrough rule is.
    fn rule_thickness(&self, style: &CharStyle) -> f32 {
        (style.size * 0.06).max(1.0)
    }

    /// Where a strikethrough sits relative to the baseline, as a positive distance above it.
    fn strikethrough_offset(&self, style: &CharStyle) -> f32 {
        self.line_metrics(style).ascent * 0.32
    }
}

/// A stub for tests: every cluster is the same width and every line is the same height, whatever the
/// style. Layout tests using this can state their expected positions as exact numbers.
#[derive(Debug, Clone, Copy)]
pub struct FixedMetrics {
    pub cluster_width: f32,
    pub ascent: f32,
    pub descent: f32,
}

impl Default for FixedMetrics {
    fn default() -> Self {
        Self { cluster_width: 10.0, ascent: 16.0, descent: 4.0 }
    }
}

impl FontMetrics for FixedMetrics {
    fn advance(&self, cluster: &str, _style: &CharStyle) -> f32 {
        // A tab is four columns wide; everything else is one column, whatever it is.
        if cluster == "\t" {
            self.cluster_width * 4.0
        } else {
            self.cluster_width
        }
    }

    fn line_metrics(&self, _style: &CharStyle) -> LineMetrics {
        LineMetrics { ascent: self.ascent, descent: self.descent, line_gap: 0.0 }
    }
}

/// A stub that does scale with the font size, for the tests that need size to matter. A cluster is
/// half the point size wide and a line is one and a quarter times the point size tall.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScaledMetrics;

impl FontMetrics for ScaledMetrics {
    fn advance(&self, cluster: &str, style: &CharStyle) -> f32 {
        let width = style.size * 0.5;
        if cluster == "\t" {
            width * 4.0
        } else {
            width
        }
    }

    fn line_metrics(&self, style: &CharStyle) -> LineMetrics {
        LineMetrics { ascent: style.size, descent: style.size * 0.25, line_gap: 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_metrics_ignore_the_style() {
        let metrics = FixedMetrics::default();
        let small = CharStyle { size: 8.0, ..CharStyle::default() };
        let large = CharStyle { size: 48.0, ..CharStyle::default() };
        assert_eq!(metrics.advance("a", &small), metrics.advance("a", &large));
        assert_eq!(metrics.line_metrics(&small).height(), 20.0);
    }

    #[test]
    fn scaled_metrics_follow_the_font_size() {
        let metrics = ScaledMetrics;
        let style = CharStyle { size: 20.0, ..CharStyle::default() };
        assert_eq!(metrics.advance("a", &style), 10.0);
        assert_eq!(metrics.line_metrics(&style).height(), 25.0);
    }

    #[test]
    fn a_tab_is_four_columns() {
        let metrics = FixedMetrics::default();
        let style = CharStyle::default();
        assert_eq!(metrics.advance("\t", &style), 40.0);
    }
}
