//! The colours a diagram is drawn in.
//!
//! Handed in by whoever is drawing, the way [`crate::markdown::PreviewColors`] is, so this crate
//! names no palette of its own and the window's colours reach a diagram without this module knowing
//! what a window is. The default is Unluminate's own palette, so a test here needs no application.
//!
//! **A document does not choose these.** `style`, `classDef` and `:::` are read and ignored, which
//! §13 of `tasks/unluminate-mermaid-plugin-tdd.md` records: honouring arbitrary colours out of a document
//! would put the document in charge of the window's palette, and a diagram whose author chose black
//! on white would be unreadable in a dark editor. The same decision `services::plugins` already made
//! about a plugin's colour scheme, for the same reason.

use crate::style::Color;

use super::scene::Paint;

/// One colour per thing a diagram is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Words inside a node, and a title.
    pub text: Color,
    /// A label on an edge, an axis tick, anything secondary.
    pub dim: Color,
    /// Inside a node.
    pub node_fill: Paint,
    /// Round a node.
    pub node_stroke: Color,
    /// Inside a subgraph, a composite state, a `box`, a section band.
    pub group_fill: Paint,
    pub group_stroke: Color,
    /// An edge, a lifeline, a relationship.
    pub line: Color,
    /// Anything the diagram itself marks out: a highlighted commit, a critical task.
    pub accent: Color,
    /// A chart's grid and its axes.
    pub grid: Color,
    /// The colours a chart cycles through: pie slices, gantt sections, series, branches.
    pub series: [Color; 8],
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: Color::rgb(0xE8, 0xEB, 0xF1),
            dim: Color::rgb(0x8B, 0x93, 0xA3),
            node_fill: Paint::solid(Color::rgb(0x2A, 0x31, 0x3D)),
            node_stroke: Color::rgb(0x5A, 0x64, 0x76),
            group_fill: Paint::faded(Color::rgb(0x48, 0x9F, 0xF8), 26),
            group_stroke: Color::rgb(0x3E, 0x4A, 0x5C),
            line: Color::rgb(0x9A, 0xA3, 0xB4),
            accent: Color::rgb(0x48, 0x9F, 0xF8),
            grid: Color::rgb(0x38, 0x3F, 0x4B),
            series: [
                Color::rgb(0x48, 0x9F, 0xF8),
                Color::rgb(0x7F, 0xCA, 0x98),
                Color::rgb(0xE8, 0xC0, 0x4A),
                Color::rgb(0xB4, 0x58, 0x8C),
                Color::rgb(0x4D, 0x9D, 0xC3),
                Color::rgb(0xE0, 0x7A, 0x5F),
                Color::rgb(0x9A, 0x8C, 0xE8),
                Color::rgb(0x3C, 0x9D, 0x74),
            ],
        }
    }
}

impl Theme {
    /// The colour at `index` in the series, wrapping round.
    ///
    /// Wrapping rather than running out, because a pie chart with nine slices is an ordinary thing
    /// and the ninth slice still has to be drawn.
    pub fn series(&self, index: usize) -> Color {
        self.series[index % self.series.len()]
    }

    /// The series colour at `index`, faded, for the area under a curve or behind a band.
    pub fn wash(&self, index: usize, alpha: u8) -> Paint {
        Paint::faded(self.series(index), alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_series_wraps_rather_than_running_out() {
        let theme = Theme::default();
        assert_eq!(theme.series(0), theme.series(8));
        assert_eq!(theme.series(3), theme.series(11));
    }

    #[test]
    fn every_series_colour_is_different_from_every_other() {
        // Two slices of a pie the same colour is a chart that cannot be read.
        let theme = Theme::default();
        for (index, colour) in theme.series.iter().enumerate() {
            for (other, second) in theme.series.iter().enumerate() {
                assert!(index == other || colour != second, "{index} and {other} match");
            }
        }
    }
}
