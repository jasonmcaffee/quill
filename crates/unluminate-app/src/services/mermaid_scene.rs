//! The diagrams a preview draws, laid out once and kept.
//!
//! `unluminate_core::mermaid` turns a diagram's source into a `Scene`, and that is arithmetic rather than
//! a file read — but a preview is redrawn sixty times a second and a document with six diagrams in it
//! would be six layouts a frame for no reason at all. So a scene is worked out once and kept, keyed
//! by exactly the three things that can change it: the source, the font it is measured in, and its
//! size.
//!
//! The same idea as `services::preview_images`, one layer up: that one decodes a photograph once, and
//! this one lays a diagram out once.
//!
//! **The plugin decides whether any of this happens at all.** `is_enabled` is asked before a diagram
//! is laid out, so switching the Mermaid plugin off in `Plugins` withdraws every diagram in the same
//! frame — which is what makes it a plugin rather than a feature with a plugin painted on it.

use std::collections::HashMap;

use unluminate_core::mermaid::{self, Options, Problem, Scene, Theme};
use unluminate_core::metrics::{FontMetrics, LineMetrics};
use unluminate_core::CharStyle;

/// Measuring a diagram's text with the fonts `egui` will actually draw it in.
///
/// **A diagram is the one thing in the window that `unluminate-core` measures and `egui` draws.** The
/// editor measures through `TextRenderer` and paints through its own glyph atlas, so those two agree
/// by construction; a diagram's boxes are worked out in `unluminate-core` and its text is handed to
/// `egui::Painter`, and two text engines never agree exactly. `ab_glyph`'s advances and egui's
/// atlas differ by a few per cent, which is invisible in a paragraph and is a label hanging over the
/// edge of the box that was sized for it — which is what a requirement diagram's fields did.
///
/// So the measuring is done through `egui` itself. Nothing else changes: this is a `FontMetrics`
/// like any other, and `unluminate-core` still knows nothing about what is drawing.
pub struct EguiMetrics<'a> {
    context: &'a egui::Context,
    /// The family a bold style asks for, which is whatever `theme::install_fonts` registered.
    bold: egui::FontFamily,
}

impl<'a> EguiMetrics<'a> {
    pub fn new(context: &'a egui::Context, bold: egui::FontFamily) -> Self {
        Self { context, bold }
    }

    /// The font egui will lay this style out in, which is the whole point of this type.
    fn font(&self, style: &CharStyle) -> egui::FontId {
        let family =
            if style.bold { self.bold.clone() } else { egui::FontFamily::Proportional };
        egui::FontId::new(style.size.max(1.0), family)
    }
}

impl FontMetrics for EguiMetrics<'_> {
    fn advance(&self, cluster: &str, style: &CharStyle) -> f32 {
        let font = self.font(style);
        // `fonts_mut` rather than `fonts`: asking for a glyph is what puts it in the atlas, so
        // measuring one that has not been drawn yet needs the mutable handle.
        self.context.fonts_mut(|fonts| {
            cluster.chars().map(|character| fonts.glyph_width(&font, character)).sum()
        })
    }

    fn line_metrics(&self, style: &CharStyle) -> LineMetrics {
        let font = self.font(style);
        let height = self.context.fonts_mut(|fonts| fonts.row_height(&font));
        // egui reports one number for a row rather than the three the editor's layout wants. Split
        // in the usual proportion, because what a diagram uses it for is how far apart two lines of
        // a label go, and that is the total.
        LineMetrics { ascent: height * 0.8, descent: height * 0.2, line_gap: 0.0 }
    }
}

/// How many laid-out diagrams are kept before the least recently wanted are dropped.
///
/// A document with more diagrams than this in it still draws every one of them; it simply lays the
/// ones that have scrolled out of use out again when they come back. Sixty-four is far past any
/// document a person writes and small enough that the memory never matters.
const KEPT: usize = 64;

/// What a scene was worked out from. Two diagrams agreeing on all of it can share one scene.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    source: String,
    family: String,
    /// The font size, in tenths of a point, so it can be part of a key.
    size: u32,
}

/// A diagram that has been laid out, or the reason it could not be.
pub type Laid = Result<Scene, Problem>;

/// Every diagram that has been laid out, and when each was last wanted.
#[derive(Default)]
pub struct MermaidScenes {
    known: HashMap<Key, (Laid, u64)>,
    /// Counts up on every request, so the least recently wanted can be found without a clock.
    clock: u64,
}

impl MermaidScenes {
    pub fn new() -> Self {
        Self::default()
    }

    /// The scene for `source`, laid out if it has not been already.
    ///
    /// `base` is the family and the size the diagram's text is set in, which is the editor's own
    /// font: a diagram follows the setting in `Edit -> Settings -> Appearance -> Font` exactly as
    /// the Markdown preview does.
    pub fn scene(
        &mut self,
        source: &str,
        base: &CharStyle,
        metrics: &dyn FontMetrics,
        theme: &Theme,
    ) -> Laid {
        let key = Key {
            source: source.to_owned(),
            family: base.family.clone(),
            size: (base.size * 10.0).round().max(0.0) as u32,
        };
        self.clock += 1;
        if let Some((laid, last)) = self.known.get_mut(&key) {
            *last = self.clock;
            return laid.clone();
        }
        let options = Options { metrics, base: base.clone(), theme: theme.clone() };
        let laid = mermaid::render(source, &options);
        self.forget_the_oldest();
        self.known.insert(key, (laid.clone(), self.clock));
        laid
    }

    /// Drop the least recently wanted entries once there are too many.
    fn forget_the_oldest(&mut self) {
        while self.known.len() >= KEPT {
            let Some(oldest) = self
                .known
                .iter()
                .min_by_key(|(_, (_, last))| *last)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.known.remove(&oldest);
        }
    }

    /// Throw everything away, which is what a change of theme asks for.
    pub fn forget(&mut self) {
        self.known.clear();
    }

    /// How many are being held, for a test.
    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

/// Unluminate's palette, as a diagram's theme.
///
/// Read out of `theme::color` rather than chosen here, so a diagram is drawn in the same colours as
/// everything else in the window and there is one palette rather than two.
pub fn theme() -> Theme {
    use crate::theme::color;
    let of = |colour: egui::Color32| unluminate_core::Color::rgb(colour.r(), colour.g(), colour.b());
    Theme {
        text: of(color::text()),
        dim: of(color::text_dim()),
        node_fill: unluminate_core::mermaid::Paint::solid(of(color::title_bar())),
        node_stroke: of(color::control_border()),
        group_fill: unluminate_core::mermaid::Paint::faded(of(color::accent()), 26),
        group_stroke: of(color::divider()),
        line: of(color::text_control()),
        accent: of(color::accent()),
        grid: of(color::divider()),
        series: [
            of(color::accent()),
            of(color::git_added()),
            of(color::unsaved()),
            of(color::blame_new()),
            of(color::git_modified()),
            unluminate_core::Color::rgb(0xE0, 0x7A, 0x5F),
            unluminate_core::Color::rgb(0x9A, 0x8C, 0xE8),
            of(color::blame_old()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unluminate_core::metrics::FixedMetrics;

    /// A renderer is a real thing with fonts behind it, so these tests measure through the stub the
    /// layout tests use and reach `mermaid::render` directly rather than through `scene`.
    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    #[test]
    fn the_theme_is_unluminates_own_palette_and_every_series_colour_differs() {
        let theme = theme();
        assert_eq!(theme.accent, unluminate_core::Color::rgb(0x48, 0x9F, 0xF8));
        for first in 0..theme.series.len() {
            for second in first + 1..theme.series.len() {
                assert_ne!(
                    theme.series[first], theme.series[second],
                    "series {first} and {second} are the same colour"
                );
            }
        }
    }

    #[test]
    fn a_diagram_drawn_in_unluminates_palette_still_keeps_every_property() {
        // The palette is handed in from outside, so it is worth proving that the real one produces a
        // scene as sound as the default one the core's own tests use.
        let mut options = options();
        options.theme = theme();
        let scene = unluminate_core::mermaid::render("flowchart LR\n A[One] --> B[Two]\n", &options)
            .expect("it should draw");
        assert!(scene.size.width > 0.0 && scene.size.height > 0.0);
        assert!(scene.texts().contains(&"One"));
    }

    #[test]
    fn two_diagrams_with_the_same_source_and_font_share_one_key() {
        let first = Key {
            source: "pie\n\"a\" : 1\n".to_owned(),
            family: "Helvetica".to_owned(),
            size: 140,
        };
        let second = first.clone();
        assert_eq!(first, second);
        // A different size is a different key, because the layout depends on what the text measures.
        let larger = Key { size: 180, ..first.clone() };
        assert_ne!(first, larger);
    }
}
