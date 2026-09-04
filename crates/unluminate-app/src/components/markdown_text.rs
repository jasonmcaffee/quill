//! A string of markdown, drawn as markdown.
//!
//! `task-28`: "The description, comments, etc should have icons to view as raw, or as markdown, if there is the
//! markdown plugin installed (there is)."
//!
//! One correction to the premise, because the whole design rests on it: **there is no markdown plugin.**
//! `crates/unluminate-app/plugins/` holds agent-tasks, css, html, javascript, mermaid, rust and typescript, and
//! markdown is built into `unluminate-core` — `unluminate_core::markdown::render` is what the editor's own preview is made
//! of. So a rendered view is always available and there is nothing to check for. What the plugins supply to a
//! preview is syntax colouring inside a fenced code block, and that is a decoration on the rendered view rather
//! than a condition of it.
//!
//! ## Nothing here renders markdown
//!
//! `unluminate_core::markdown::render` turns the source into the same three things a document holds — a rope, its
//! character styles and its paragraph styles — so `unluminate_core::layout` lays it out and
//! `components::editor_view::paint_text` paints it. This module is the join between them, and it exists because
//! `UnluminateApp::refresh_preview` is the only other place that made that join and it needs a tab: it reads the
//! tab's scroll, its zoom anchor, its folds and its caret history, all of which live on `OpenFile`. A ticket's
//! description has none of those and does not want them.
//!
//! ## What a rendered view here does not have, and why
//!
//! **Pictures and Mermaid diagrams.** `UnluminateApp::refresh_preview` resolves those in two further passes that
//! decode an image and lay a diagram out, and both need the window: the first uploads a texture to the graphics
//! card and the second measures a font. A description with an image in it shows that paragraph's alt text.
//! `plugin.limitations` says so.

use egui::{Pos2, Rect};
use unluminate_core::{layout, Layout, Rope};

use crate::services::text_renderer::TextRenderer;

/// Where a code background goes: the paragraphs a fenced block covers, and the bytes an inline span
/// covers.
///
/// **`unluminate_core::markdown` already answers both** — `Preview::panels` and `Preview::code_spans` —
/// and the editor's own preview paints a panel behind the first and a chip behind the second. This
/// module was throwing them away, so a fence in a plugin's markdown was a line of coloured text
/// floating on the surface behind it and inline code was a word in a different colour. It is the same
/// two questions asked of the same reader; only the painting was missing.
#[derive(Debug, Default, Clone)]
pub struct CodeBackgrounds {
    /// One `start..end` of paragraph numbers a fenced block covers.
    pub panels: Vec<std::ops::Range<usize>>,
    /// One `start..end` of bytes an inline span covers.
    pub spans: Vec<std::ops::Range<usize>>,
}

/// A rendered piece of markdown: the text it came to, and where every line of it goes.
///
/// Held by the caller between frames, because rendering and laying out is the expensive half and neither the
/// source nor the width changes on most frames. [`Rendered::stale`] is what asks whether it has to be done again.
pub struct Rendered {
    /// The text the markdown came to, which is what a selection would be measured against.
    pub text: Rope,
    pub layout: Layout,
    /// Where the code backgrounds go, so a fence reads as a block rather than as coloured text.
    pub code: CodeBackgrounds,
    /// The source this was made from, so the caller can tell when it has changed.
    source: String,
    /// The width it was laid out at, for the same reason.
    width: f32,
}

impl std::fmt::Debug for Rendered {
    /// Written by hand because a `Layout` is a screenful of glyph positions and printing it says nothing.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Rendered")
            .field("lines", &self.layout.lines.len())
            .field("width", &self.width)
            .finish()
    }
}

impl Rendered {
    /// True when `source` or `width` is not what this was made from, so it has to be made again.
    ///
    /// Half a point of tolerance on the width, which is what `UnluminateApp::refresh_preview` uses: a pane whose
    /// width wobbles by a fraction of a point while a divider settles must not re-lay a description every frame.
    pub fn stale(&self, source: &str, width: f32) -> bool {
        self.source != source || (self.width - width).abs() >= 0.5
    }

    /// How tall it is, which is what a caller scrolling it needs to know.
    pub fn height(&self) -> f32 {
        self.layout
            .lines
            .last()
            .map(|line| line.y + line.height)
            .unwrap_or(0.0)
    }
}

/// What the rendered text is coloured with, which the caller takes from its own palette.
///
/// A struct rather than five arguments because the list had reached the length at which a caller starts passing
/// them in the wrong order, which is the reason `explorer::View` is one too.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    /// Ordinary text.
    pub text: egui::Color32,
    /// Headings.
    pub strong: egui::Color32,
    /// Inline code and code blocks.
    pub code: egui::Color32,
    /// A link's text.
    pub link: egui::Color32,
    /// A block quote, a list's bullets, and a table's rules.
    pub quiet: egui::Color32,
    /// A horizontal rule.
    pub rule: egui::Color32,
}

/// Render `source` as markdown and lay it out to `width`.
///
/// `highlighter` colours the inside of a fenced code block. `None` leaves code in the code colour, which is what
/// a caller with no access to the plugins passes.
pub fn render(
    source: &str,
    renderer: &TextRenderer,
    family: &str,
    size: f32,
    colors: Colors,
    width: f32,
    highlighter: Option<&dyn unluminate_core::CodeHighlighter>,
) -> Rendered {
    let of = |color: egui::Color32| unluminate_core::Color::rgb(color.r(), color.g(), color.b());
    let base = unluminate_core::CharStyle {
        family: family.to_owned(),
        size,
        color: of(colors.text),
        ..unluminate_core::CharStyle::default()
    };
    let mono = renderer.monospaced_family();
    // How many characters of the code font fit across the width, which is the one measurement a table takes.
    // Everything else about a table is integer arithmetic over characters inside `unluminate_core`.
    let code = unluminate_core::CharStyle {
        family: mono.clone().unwrap_or_else(|| base.family.clone()),
        size: base.size * 0.95,
        ..unluminate_core::CharStyle::default()
    };
    let advance = unluminate_core::FontMetrics::advance(renderer, "M", &code).max(1.0);
    let preview = unluminate_core::markdown::render(
        source,
        &unluminate_core::PreviewOptions {
            base: base.clone(),
            colors: unluminate_core::PreviewColors {
                text: of(colors.strong),
                code: of(colors.code),
                link: of(colors.link),
                quiet: of(colors.quiet),
                rule: of(colors.rule),
            },
            mono,
            columns: (width / advance).floor().max(16.0) as usize,
            highlighter,
        },
    );
    let laid = layout(
        &preview.text,
        &preview.chars,
        &preview.paragraphs,
        renderer,
        width,
    );
    let code = CodeBackgrounds {
        panels: preview
            .panels
            .iter()
            .map(|panel| panel.paragraphs.clone())
            .collect(),
        spans: preview.code_spans.clone(),
    };
    Rendered {
        text: preview.text,
        layout: laid,
        code,
        source: source.to_owned(),
        width,
    }
}

/// Paint `rendered` into `area`, scrolled down by `scroll`, and answer how tall it is.
///
/// Only the lines inside `area` are painted, which `editor_view::paint_text` decides from the clip rectangle —
/// so a description of a thousand lines costs a screenful, the property `tasks/task-1666-performance-tdd.md`
/// records for the editing area itself.
pub fn show(ui: &mut egui::Ui, area: Rect, rendered: &Rendered, renderer: &TextRenderer, scroll: f32) -> f32 {
    show_with(ui, area, rendered, renderer, scroll, None)
}

/// The same, with the two colours a code background is painted in.
///
/// `None` paints none, which is what this drew before and what a caller with nothing to say about
/// code still gets. The colours are the caller's rather than this module's for the reason the whole
/// `Colors` value is: a component in Unluminate is handed the palette rather than reaching for one.
pub fn show_with(
    ui: &mut egui::Ui,
    area: Rect,
    rendered: &Rendered,
    renderer: &TextRenderer,
    scroll: f32,
    code: Option<CodeColors>,
) -> f32 {
    if let Some(colours) = code {
        paint_the_code_backgrounds(ui, area, rendered, scroll, colours);
    }
    let mut clipped = ui.new_child(egui::UiBuilder::new().max_rect(area));
    // **Intersected with what the caller was already clipped to, rather than replacing it.**
    // `Ui::set_clip_rect` sets outright, so a block whose rectangle reaches past the pane it is drawn
    // in — a message scrolled half out of a chat, which is the ordinary case — painted its text over
    // whatever was above the scrolling area. Measured on a real window in `task-1767`, where a message
    // scrolled off the top was drawn across the pane's own header.
    clipped.set_clip_rect(area.intersect(ui.clip_rect()));
    crate::components::editor_view::paint_text(
        &clipped,
        renderer,
        &rendered.layout,
        Pos2::new(area.left(), area.top() - scroll),
    );
    rendered.height()
}

/// What a code background is painted in: a fenced block's panel and an inline span's chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeColors {
    pub panel: egui::Color32,
    pub chip: egui::Color32,
    pub radius: u8,
}

/// Paint a panel behind every fenced block and a chip behind every inline span.
///
/// **Behind**, which is why it runs before the text: the painter takes shapes in the order they
/// arrive, so a background added afterwards would cover the words it is a background for.
fn paint_the_code_backgrounds(
    ui: &mut egui::Ui,
    area: Rect,
    rendered: &Rendered,
    scroll: f32,
    colours: CodeColors,
) {
    let painter = ui.painter_at(area.intersect(ui.clip_rect()));
    let top = area.top() - scroll;
    let radius = egui::CornerRadius::same(colours.radius);
    for block in &rendered.code.panels {
        // A block is a run of paragraphs, and a paragraph's lines are contiguous — so the panel is
        // the band from the first line's top to the last line's bottom, drawn the whole width so a
        // fence reads as a block rather than as text that happens to be in another font.
        let lines: Vec<&unluminate_core::PlacedLine> = rendered
            .layout
            .lines
            .iter()
            .filter(|line| block.contains(&line.paragraph))
            .collect();
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            continue;
        };
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(area.left(), top + first.y - 3.0),
                Pos2::new(area.right(), top + last.y + last.height + 3.0),
            ),
            radius,
            colours.panel,
        );
    }
    for span in &rendered.code.spans {
        // An inline span may wrap, so it is painted a line at a time — the part of each line the
        // span covers, which is what `selection_rects_in` already answers for a selection.
        for chip in rendered
            .layout
            .selection_rects_in(0..rendered.layout.lines.len(), span.clone())
        {
            // `unluminate_core`'s rectangle is a position and a size, and it is in the layout's own
            // coordinates; the chip is that moved to where the text really is, and widened a little
            // either side so a word does not touch the edge of its own background.
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(area.left() + chip.x - 2.0, top + chip.y),
                    egui::Vec2::new(chip.width + 4.0, chip.height),
                ),
                radius,
                colours.chip,
            );
        }
    }
}

/// The rendered markdown a ticket is showing, kept between frames and keyed by what it is.
///
/// Rendering and laying out is the expensive half — a parse, a style span per run and a line per wrapped line —
/// and neither the source nor the width changes on most frames. So it is done when one of them does and not
/// otherwise, which is exactly what `UnluminateApp::refresh_preview` does for a file. `tasks/agent-tasks-ui-tdd.md` §6
/// is about the cost of this board doing work once a frame that it did not need to do; this is that lesson
/// applied to the one thing added since.
///
/// The keys are `description` and `comment-<id>`, so a ticket's description and its comments share one cache and
/// one code path. [`Cache::forget`] is called when the ticket that is open changes, because a key that names
/// another ticket's comment is a render nobody will ask for again.
#[derive(Default)]
pub struct Cache {
    made: std::collections::HashMap<String, Rendered>,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Cache")
            .field("entries", &self.made.len())
            .finish()
    }
}

impl Cache {
    /// The rendered `source` for `key`, rendering it again only if the source or the width has changed.
    #[allow(clippy::too_many_arguments)]
    pub fn rendered(
        &mut self,
        key: &str,
        source: &str,
        renderer: &TextRenderer,
        family: &str,
        size: f32,
        colors: Colors,
        width: f32,
        highlighter: Option<&dyn unluminate_core::CodeHighlighter>,
    ) -> &Rendered {
        let stale = self.made.get(key).is_none_or(|made| made.stale(source, width));
        if stale {
            let made = render(source, renderer, family, size, colors, width, highlighter);
            self.made.insert(key.to_owned(), made);
        }
        self.made.get(key).expect("just rendered")
    }

    /// Throw away everything, which is what changing which ticket is open means.
    pub fn forget(&mut self) {
        self.made.clear();
    }
}

#[cfg(test)]
mod tests_task_28 {
    use super::*;

    /// `task-28`: a description can be read as markdown rather than as its source.
    ///
    /// Rendering is `unluminate_core::markdown`'s, which has its own tests for what it produces. What this asserts is
    /// the join: a heading, a list, a fence and a table all come through as laid out lines, and a heading is
    /// **larger** than body text, which is the visible difference between the two views.
    #[test]
    fn markdown_becomes_laid_out_lines_and_a_heading_is_larger_than_the_prose() {
        let renderer = TextRenderer::new();
        let colors = Colors {
            text: egui::Color32::WHITE,
            strong: egui::Color32::WHITE,
            code: egui::Color32::GREEN,
            link: egui::Color32::BLUE,
            quiet: egui::Color32::GRAY,
            rule: egui::Color32::DARK_GRAY,
        };
        let source = "# A heading\n\nSome prose.\n\n- one\n- two\n\n```rust\nlet a = 1;\n```\n";
        let made = render(source, &renderer, "sans-serif", 14.0, colors, 400.0, None);
        assert!(
            made.layout.lines.len() >= 5,
            "a heading, prose, two items and a fence: {:?}",
            made
        );
        assert!(made.height() > 0.0);
        // The heading is the first line and is set larger than the prose under it, which is the whole point of
        // looking at it rendered.
        let heading = made.layout.lines.first().expect("the heading");
        let prose = made.layout.lines.get(1).expect("the prose");
        assert!(
            heading.height > prose.height,
            "a heading should stand taller than prose: {} against {}",
            heading.height,
            prose.height
        );
    }

    /// It is rendered again when the source or the width changes, and not otherwise. This is what keeps a ticket
    /// with a long description from re-laying it sixty times a second.
    #[test]
    fn the_same_source_at_the_same_width_is_rendered_once() {
        let renderer = TextRenderer::new();
        let colors = Colors {
            text: egui::Color32::WHITE,
            strong: egui::Color32::WHITE,
            code: egui::Color32::GREEN,
            link: egui::Color32::BLUE,
            quiet: egui::Color32::GRAY,
            rule: egui::Color32::DARK_GRAY,
        };
        let made = render("Some prose.", &renderer, "sans-serif", 14.0, colors, 400.0, None);
        assert!(!made.stale("Some prose.", 400.0), "nothing changed");
        assert!(
            !made.stale("Some prose.", 400.2),
            "a fraction of a point is not a change"
        );
        assert!(
            made.stale("Some prose.", 500.0),
            "a different width has to be laid out again"
        );
        assert!(
            made.stale("Other prose.", 400.0),
            "a different source has to be rendered again"
        );
    }
}
