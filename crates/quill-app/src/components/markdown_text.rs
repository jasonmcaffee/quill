//! A string of markdown, drawn as markdown.
//!
//! `task-28`: "The description, comments, etc should have icons to view as raw, or as markdown, if there is the
//! markdown plugin installed (there is)."
//!
//! One correction to the premise, because the whole design rests on it: **there is no markdown plugin.**
//! `crates/quill-app/plugins/` holds agent-tasks, css, html, javascript, mermaid, rust and typescript, and
//! markdown is built into `quill-core` — `quill_core::markdown::render` is what the editor's own preview is made
//! of. So a rendered view is always available and there is nothing to check for. What the plugins supply to a
//! preview is syntax colouring inside a fenced code block, and that is a decoration on the rendered view rather
//! than a condition of it.
//!
//! ## Nothing here renders markdown
//!
//! `quill_core::markdown::render` turns the source into the same three things a document holds — a rope, its
//! character styles and its paragraph styles — so `quill_core::layout` lays it out and
//! `components::editor_view::paint_text` paints it. This module is the join between them, and it exists because
//! `QuillApp::refresh_preview` is the only other place that made that join and it needs a tab: it reads the
//! tab's scroll, its zoom anchor, its folds and its caret history, all of which live on `OpenFile`. A ticket's
//! description has none of those and does not want them.
//!
//! ## What a rendered view here does not have, and why
//!
//! **Pictures and Mermaid diagrams.** `QuillApp::refresh_preview` resolves those in two further passes that
//! decode an image and lay a diagram out, and both need the window: the first uploads a texture to the graphics
//! card and the second measures a font. A description with an image in it shows that paragraph's alt text.
//! `plugin.limitations` says so.

use egui::{Pos2, Rect};
use quill_core::{layout, Layout, Rope};

use crate::services::text_renderer::TextRenderer;

/// A rendered piece of markdown: the text it came to, and where every line of it goes.
///
/// Held by the caller between frames, because rendering and laying out is the expensive half and neither the
/// source nor the width changes on most frames. [`Rendered::stale`] is what asks whether it has to be done again.
pub struct Rendered {
    /// The text the markdown came to, which is what a selection would be measured against.
    pub text: Rope,
    pub layout: Layout,
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
    /// Half a point of tolerance on the width, which is what `QuillApp::refresh_preview` uses: a pane whose
    /// width wobbles by a fraction of a point while a divider settles must not re-lay a description every frame.
    pub fn stale(&self, source: &str, width: f32) -> bool {
        self.source != source || (self.width - width).abs() >= 0.5
    }

    /// How tall it is, which is what a caller scrolling it needs to know.
    pub fn height(&self) -> f32 {
        self.layout.lines.last().map(|line| line.y + line.height).unwrap_or(0.0)
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
    highlighter: Option<&dyn quill_core::CodeHighlighter>,
) -> Rendered {
    let of = |color: egui::Color32| quill_core::Color::rgb(color.r(), color.g(), color.b());
    let base = quill_core::CharStyle {
        family: family.to_owned(),
        size,
        color: of(colors.text),
        ..quill_core::CharStyle::default()
    };
    let mono = renderer.monospaced_family();
    // How many characters of the code font fit across the width, which is the one measurement a table takes.
    // Everything else about a table is integer arithmetic over characters inside `quill_core`.
    let code = quill_core::CharStyle {
        family: mono.clone().unwrap_or_else(|| base.family.clone()),
        size: base.size * 0.95,
        ..quill_core::CharStyle::default()
    };
    let advance = quill_core::FontMetrics::advance(renderer, "M", &code).max(1.0);
    let preview = quill_core::markdown::render(
        source,
        &quill_core::PreviewOptions {
            base: base.clone(),
            colors: quill_core::PreviewColors {
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
    let laid = layout(&preview.text, &preview.chars, &preview.paragraphs, renderer, width);
    Rendered { text: preview.text, layout: laid, source: source.to_owned(), width }
}

/// Paint `rendered` into `area`, scrolled down by `scroll`, and answer how tall it is.
///
/// Only the lines inside `area` are painted, which `editor_view::paint_text` decides from the clip rectangle —
/// so a description of a thousand lines costs a screenful, the property `tasks/task-1666-performance-tdd.md`
/// records for the editing area itself.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    rendered: &Rendered,
    renderer: &TextRenderer,
    scroll: f32,
) -> f32 {
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

/// The rendered markdown a ticket is showing, kept between frames and keyed by what it is.
///
/// Rendering and laying out is the expensive half — a parse, a style span per run and a line per wrapped line —
/// and neither the source nor the width changes on most frames. So it is done when one of them does and not
/// otherwise, which is exactly what `QuillApp::refresh_preview` does for a file. `tasks/agent-tasks-ui-tdd.md` §6
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
        out.debug_struct("Cache").field("entries", &self.made.len()).finish()
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
        highlighter: Option<&dyn quill_core::CodeHighlighter>,
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
    /// Rendering is `quill_core::markdown`'s, which has its own tests for what it produces. What this asserts is
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
        assert!(made.layout.lines.len() >= 5, "a heading, prose, two items and a fence: {:?}", made);
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
        assert!(!made.stale("Some prose.", 400.2), "a fraction of a point is not a change");
        assert!(made.stale("Some prose.", 500.0), "a different width has to be laid out again");
        assert!(made.stale("Other prose.", 400.0), "a different source has to be rendered again");
    }
}
