//! Turning Markdown into styled text.
//!
//! Quill already has everything needed to show formatted text: a styled text model, a layout engine and a
//! painter. So the preview is not a separate renderer. This module reads Markdown source and produces the
//! same three things a document holds, a rope of text with character spans over it and one paragraph
//! setting per line, and the ordinary layout and painting code draws it.
//!
//! Written here rather than taken from a library, which is what the ticket asks for. It covers the parts of
//! Markdown that appear in a `.md` file someone is writing prose in. What it does not do is listed at the
//! bottom of this comment.
//!
//! Handled: headings one to six, bold, italic, bold italic, strikethrough, inline code, fenced code blocks,
//! indented bullet lists, numbered lists, block quotes, horizontal rules, links, hard line breaks,
//! images on a line of their own, and Mermaid diagrams.
//!
//! Not handled: tables, footnotes, reference style links, nested block quotes, and HTML. Each of those
//! either needs layout Quill does not have yet, such as a table, or is rare in prose.
//!
//! ## Images
//!
//! A line whose whole content is an image mark becomes an **empty paragraph** and an entry in
//! [`Preview::images`]. Empty rather than carrying the alt text, because the application draws that
//! line itself: the picture once it has decoded it, and the alt text in the quiet colour when it
//! cannot. Nothing here reads a file or knows what a picture is — this crate has no user interface
//! dependency and cannot decode one — so what it produces is the place the picture goes and the name
//! of the file it is in.
//!
//! An image mark **inside** a line of prose is shown as its alt text in the quiet colour. A picture
//! in the middle of a paragraph needs inline layout the engine does not have, and the alt text is
//! what a reader wants in its place.
//!
//! ## Diagrams
//!
//! A fence whose language is `mermaid` is the same idea again: it becomes an empty paragraph and an
//! entry in [`Preview::diagrams`], and the application lays the diagram out through [`crate::mermaid`]
//! — which can do the arithmetic, but still cannot know how wide the pane is — and paints it into the
//! room it reserved. Nothing about the layout engine changes: it still knows only about glyphs and
//! about a paragraph that has asked to be tall.
//!
//! **A fence nobody has closed yet is still a diagram.** A preview is worked out again on every
//! keystroke, so the half-typed state is the common case rather than the odd one, and showing the
//! diagram so far is far more use than showing nothing until the closing backticks arrive.

use crate::rope::Rope;
use crate::style::{Align, CharStyle, Color, ParagraphStyle, ParagraphStyles, StyleChange, StyleSpans};

/// How much bigger each heading level is than body text. Level one is the first entry.
const HEADING_SCALE: [f32; 6] = [1.9, 1.55, 1.3, 1.15, 1.05, 1.0];

/// Colours the preview uses for the parts that are not ordinary text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewColors {
    /// Ordinary text and headings.
    pub text: Color,
    /// Inline code and code blocks.
    pub code: Color,
    /// A link's text.
    pub link: Color,
    /// A block quote, and the bullet or number in front of a list item.
    pub quiet: Color,
    /// A horizontal rule.
    pub rule: Color,
}

impl Default for PreviewColors {
    fn default() -> Self {
        Self {
            text: Color::WHITE,
            code: Color::rgb(0x7E, 0xD3, 0x9B),
            link: Color::rgb(0x48, 0x9F, 0xF8),
            quiet: Color::rgb(0x8B, 0x93, 0xA3),
            rule: Color::rgb(0x8B, 0x93, 0xA3),
        }
    }
}

/// A picture the preview should draw, and where in the preview it goes.
///
/// The paragraph it names holds no text. Whoever draws the preview decides how large the picture is
/// — which depends on the width of the pane and so cannot be known here — asks that paragraph to be
/// at least that tall through `ParagraphStyle::min_height`, and paints the picture into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    /// Which paragraph of [`Preview::text`] stands in for the picture.
    pub paragraph: usize,
    /// What was between the brackets: a path, usually relative to the document's own folder.
    pub source: String,
    /// The words to show when the picture cannot be drawn.
    pub alt: String,
}

/// A diagram the preview should draw, and where in the preview it goes.
///
/// The same shape as [`PreviewImage`], and for the same reason: how tall a diagram is drawn depends
/// on how wide the pane is and on what the fonts measure, and this module knows neither. So it says
/// which paragraph stands in for the diagram and what was written between the fences, and whoever
/// draws the preview works the rest out — through `crate::mermaid`, which can do the arithmetic but
/// still cannot know the width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDiagram {
    /// Which paragraph of [`Preview::text`] stands in for the diagram.
    pub paragraph: usize,
    /// Everything between the fences, which is a whole Mermaid diagram.
    pub source: String,
}

/// Markdown turned into text that Quill can lay out.
#[derive(Debug, Clone)]
pub struct Preview {
    pub text: Rope,
    pub chars: StyleSpans,
    pub paragraphs: ParagraphStyles,
    /// Which line of the source each line of the preview came from, one entry a line.
    ///
    /// This is what lets the source and its preview be scrolled together. A scroll position is a
    /// number of points down a page, and the two pages are nothing like the same height — a heading
    /// is one line of source and three times the height on the page — so the only honest way across
    /// is the text itself. It never goes backwards, because the source is read from the top down,
    /// which is what makes finding a line a binary search.
    ///
    /// It is not one to one in either direction. The backticks of a fence produce no preview line at
    /// all, and a whole Mermaid fence produces a single one — named after the line the fence
    /// **opened** on rather than the one it closed on, because that is the line a reader scrolling
    /// to the diagram is looking for.
    pub source_lines: Vec<usize>,
    /// The pictures, in the order they appear.
    pub images: Vec<PreviewImage>,
    /// The diagrams, in the order they appear.
    pub diagrams: Vec<PreviewDiagram>,
}

impl Preview {
    /// An empty preview, for a document with nothing in it.
    pub fn empty(base: &CharStyle) -> Self {
        Self {
            text: Rope::new(),
            chars: StyleSpans::new(0, base.clone()),
            paragraphs: ParagraphStyles::new(1),
            source_lines: vec![0],
            images: Vec::new(),
            diagrams: Vec::new(),
        }
    }
}

/// Builds up the preview text and the spans over it as the source is walked.
struct Builder {
    out: String,
    /// One entry per span, as a length and the style covering it.
    runs: Vec<(usize, CharStyle)>,
    /// One entry per line of `out`.
    paragraphs: Vec<ParagraphStyle>,
    /// Which line of the source the line being built came from. [`render`] sets it as it walks, so
    /// that [`Builder::end_line`] can record it without all nine branches of the walk passing it in.
    source_line: usize,
    /// One entry per line of `out`, taken from `source_line` as each line is ended.
    source_lines: Vec<usize>,
    base: CharStyle,
    colors: PreviewColors,
    /// The family to set code in, if this system has a monospaced one.
    mono: Option<String>,
    /// The pictures found so far, each naming the paragraph it takes the place of.
    images: Vec<PreviewImage>,
    /// The diagrams found so far, the same way.
    diagrams: Vec<PreviewDiagram>,
}

impl Builder {
    fn new(base: CharStyle, colors: PreviewColors, mono: Option<String>) -> Self {
        Self {
            out: String::new(),
            runs: Vec::new(),
            paragraphs: Vec::new(),
            source_line: 0,
            source_lines: Vec::new(),
            base,
            colors,
            mono,
            images: Vec::new(),
            diagrams: Vec::new(),
        }
    }

    /// Add text in `style`. Neighbouring runs with the same style are folded together, so a line of plain
    /// prose is one span rather than one span per word.
    fn push(&mut self, text: &str, style: CharStyle) {
        if text.is_empty() {
            return;
        }
        self.out.push_str(text);
        match self.runs.last_mut() {
            Some((length, last)) if *last == style => *length += text.len(),
            _ => self.runs.push((text.len(), style)),
        }
    }

    /// End the current line, recording how that whole line is placed.
    fn end_line(&mut self, paragraph: ParagraphStyle) {
        self.paragraphs.push(paragraph);
        self.source_lines.push(self.source_line);
        self.out.push('\n');
        // The line break itself carries the style of whatever came before it.
        let style = self.runs.last().map(|(_, s)| s.clone()).unwrap_or_else(|| self.base.clone());
        match self.runs.last_mut() {
            Some((length, last)) if *last == style => *length += 1,
            _ => self.runs.push((1, style)),
        }
    }

    /// Record a picture, taking the paragraph that is about to be ended. The line itself is left
    /// empty, because the picture is drawn over it rather than beside anything.
    fn image(&mut self, source: &str, alt: &str) {
        self.images.push(PreviewImage {
            paragraph: self.paragraphs.len(),
            source: source.to_owned(),
            alt: alt.to_owned(),
        });
    }

    /// Record a diagram, taking the paragraph that is about to be ended. Like a picture, the line
    /// itself is left empty and the application paints into the room it reserved.
    fn diagram(&mut self, source: &str) {
        self.diagrams.push(PreviewDiagram {
            paragraph: self.paragraphs.len(),
            // Trailing blank lines are dropped, because a fence nobody closed collects the empty
            // line at the end of the file and a diagram should not differ depending on whether its
            // author had finished typing the closing backticks.
            source: source.trim_end().to_owned(),
        });
    }

    fn finish(mut self) -> Preview {
        // The trailing line break is dropped, because a document's last line does not end with one.
        //
        // The paragraph list is left alone. Every `end_line` added one line break and one entry, so N
        // entries go with N line breaks, and removing the last break leaves N lines, which is what N
        // entries describes. Removing an entry here as well would leave the two out of step, which is what
        // the test at the bottom of this file caught.
        if self.out.ends_with('\n') {
            self.out.pop();
            if let Some((length, _)) = self.runs.last_mut() {
                *length -= 1;
            }
            self.runs.retain(|(length, _)| *length > 0);
        }
        if self.paragraphs.is_empty() {
            self.paragraphs.push(ParagraphStyle::default());
            self.source_lines.push(0);
        }
        let mut chars = StyleSpans::new(0, self.base.clone());
        let mut at = 0;
        for (length, style) in &self.runs {
            chars.insert(at, *length);
            chars.set(at..at + length, &full_change(style));
            at += length;
        }
        Preview {
            text: Rope::from_str(&self.out),
            chars,
            paragraphs: ParagraphStyles::from_styles(self.paragraphs),
            source_lines: self.source_lines,
            images: self.images,
            diagrams: self.diagrams,
        }
    }

    fn code_style(&self, size: f32) -> CharStyle {
        CharStyle {
            family: self.mono.clone().unwrap_or_else(|| self.base.family.clone()),
            size,
            color: self.colors.code,
            ..CharStyle::default()
        }
    }
}

/// Every field set, so that inserted text is given exactly this style rather than inheriting.
fn full_change(style: &CharStyle) -> StyleChange {
    StyleChange {
        family: Some(style.family.clone()),
        size: Some(style.size),
        bold: Some(style.bold),
        italic: Some(style.italic),
        underline: Some(style.underline),
        strikethrough: Some(style.strikethrough),
        color: Some(style.color),
    }
}

/// What kind of line this is.
enum Line<'a> {
    Heading { level: usize, text: &'a str },
    /// A line holding nothing but a picture.
    Image { source: &'a str, alt: &'a str },
    Bullet { indent: usize, text: &'a str },
    Numbered { indent: usize, number: &'a str, text: &'a str },
    Quote { text: &'a str },
    Rule,
    /// A fence, and whatever was written after the backticks: the language, usually.
    Fence(&'a str),
    Blank,
    Paragraph(&'a str),
}

/// Work out what a line is before deciding how to draw it.
fn classify(line: &str) -> Line<'_> {
    let trimmed = line.trim_end();
    let indent = trimmed.len() - trimmed.trim_start().len();
    let body = trimmed.trim_start();

    if body.is_empty() {
        return Line::Blank;
    }
    if let Some(rest) = body.strip_prefix("```").or_else(|| body.strip_prefix("~~~")) {
        return Line::Fence(rest.trim());
    }
    // A picture on a line of its own. Only on its own: a picture with words beside it would need the
    // words laid out round it, and the alt text is what stands in for one inside a paragraph.
    if let Some((source, alt)) = whole_line_image(body) {
        return Line::Image { source, alt };
    }
    // A rule is three or more of the same mark and nothing else.
    if body.len() >= 3 {
        for mark in ['-', '*', '_'] {
            if body.chars().all(|c| c == mark) {
                return Line::Rule;
            }
        }
    }
    if let Some(rest) = body.strip_prefix('#') {
        let mut level = 1;
        let mut rest = rest;
        while let Some(more) = rest.strip_prefix('#') {
            level += 1;
            rest = more;
        }
        // A heading mark has to be followed by a space, so `#hashtag` is not a heading.
        if level <= 6 && (rest.starts_with(' ') || rest.is_empty()) {
            return Line::Heading { level, text: rest.trim_start() };
        }
    }
    if let Some(rest) = body.strip_prefix("> ").or_else(|| body.strip_prefix('>')) {
        return Line::Quote { text: rest.trim_start() };
    }
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = body.strip_prefix(mark) {
            return Line::Bullet { indent, text: rest };
        }
    }
    // A numbered item: digits, then a dot or a bracket, then a space.
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let after = &body[digits.len()..];
        for mark in [". ", ") "] {
            if let Some(rest) = after.strip_prefix(mark) {
                return Line::Numbered { indent, number: &body[..digits.len()], text: rest };
            }
        }
    }
    Line::Paragraph(body)
}

/// Read an image mark when it is the whole of a line, and nothing else.
///
/// Returns the source and the alt text. An empty source is not a picture: it names no file, and a
/// paragraph reserved for nothing would be a gap in the page.
fn whole_line_image(body: &str) -> Option<(&str, &str)> {
    let rest = body.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = &rest[..close];
    let after = &rest[close + 2..];
    let end = after.rfind(')')?;
    // Everything after the closing bracket has to be nothing, or this is a line with words on it.
    if !after[end + 1..].trim().is_empty() {
        return None;
    }
    let source = after[..end].trim();
    if source.is_empty() {
        return None;
    }
    Some((source, alt))
}

/// Read `source` as Markdown and produce text Quill can lay out.
///
/// `base` gives the family, the size and the colour of ordinary text; everything else is worked out from
/// it, so the preview follows the font the document is set in. `mono` is a monospaced family to set code
/// in, if this system has one.
pub fn render(
    source: &str,
    base: &CharStyle,
    colors: PreviewColors,
    mono: Option<String>,
) -> Preview {
    let mut builder = Builder::new(base.clone(), colors, mono);
    let body_size = base.size;
    let mut in_code_block = false;
    // The lines of a mermaid fence, collected while one is open, and `None` while one is not.
    let mut diagram: Option<Vec<&str>> = None;
    // Which line the open Mermaid fence started on, so that the single paragraph the whole diagram
    // becomes is named after the fence rather than after the backticks that closed it.
    let mut diagram_from = 0;

    for (number, raw) in source.split('\n').enumerate() {
        let line = classify(raw);
        // Every line ended from here on came from this line of the source. Set once rather than
        // passed into all nine branches below. See `Preview::source_lines`.
        builder.source_line = number;

        // A mermaid fence is gathered whole rather than shown a line at a time, and becomes one
        // empty paragraph for the application to draw the diagram into.
        if let Some(collected) = &mut diagram {
            if matches!(line, Line::Fence(_)) {
                builder.diagram(&collected.join("\n"));
                builder.source_line = diagram_from;
                builder.end_line(ParagraphStyle::default());
                diagram = None;
                continue;
            }
            collected.push(raw);
            continue;
        }

        if in_code_block {
            if matches!(line, Line::Fence(_)) {
                in_code_block = false;
                continue;
            }
            // Inside a fence, nothing is interpreted: the text is shown as it was written.
            let style = builder.code_style(body_size * 0.95);
            builder.push(raw, style);
            builder.end_line(ParagraphStyle { align: Align::Left, line_spacing: 1.0, ..ParagraphStyle::default() });
            continue;
        }

        match line {
            Line::Fence(language) if is_mermaid(language) => {
                diagram = Some(Vec::new());
                diagram_from = number;
            }
            Line::Fence(_) => {
                in_code_block = true;
            }
            Line::Image { source, alt } => {
                // The paragraph is left empty and the application paints the picture into it. How
                // tall it has to be depends on how wide the pane is, which is not known here.
                builder.image(source, alt);
                builder.end_line(ParagraphStyle::default());
            }
            Line::Blank => {
                builder.push(" ", builder.base.clone());
                builder.end_line(ParagraphStyle::default());
            }
            Line::Rule => {
                // A rule is drawn as a run of box drawing characters, because the layout engine places
                // glyphs and has no notion of a line that is not text.
                let style = CharStyle {
                    color: builder.colors.rule,
                    size: body_size,
                    ..builder.base.clone()
                };
                builder.push(&"\u{2500}".repeat(48), style);
                builder.end_line(ParagraphStyle::default());
            }
            Line::Heading { level, text } => {
                let size = body_size * HEADING_SCALE[level - 1];
                let style = CharStyle {
                    size,
                    bold: true,
                    color: builder.colors.text,
                    ..builder.base.clone()
                };
                inline(&mut builder, text, &style);
                builder.end_line(ParagraphStyle { align: Align::Left, line_spacing: 1.15, ..ParagraphStyle::default() });
            }
            Line::Bullet { indent, text } => {
                let level = indent / 2;
                let marker = CharStyle {
                    color: builder.colors.quiet,
                    size: body_size,
                    ..builder.base.clone()
                };
                // Indenting is done with spaces, because the layout engine has no left margin per
                // paragraph. Two spaces of source indent is one level.
                builder.push(&"    ".repeat(level), marker.clone());
                builder.push("\u{2022}  ", marker);
                let style = CharStyle { size: body_size, ..builder.base.clone() };
                inline(&mut builder, text, &style);
                builder.end_line(ParagraphStyle::default());
            }
            Line::Numbered { indent, number, text } => {
                let level = indent / 2;
                let marker = CharStyle {
                    color: builder.colors.quiet,
                    size: body_size,
                    ..builder.base.clone()
                };
                builder.push(&"    ".repeat(level), marker.clone());
                builder.push(&format!("{number}.  "), marker);
                let style = CharStyle { size: body_size, ..builder.base.clone() };
                inline(&mut builder, text, &style);
                builder.end_line(ParagraphStyle::default());
            }
            Line::Quote { text } => {
                let bar = CharStyle {
                    color: builder.colors.quiet,
                    size: body_size,
                    ..builder.base.clone()
                };
                builder.push("\u{2502}  ", bar);
                let style = CharStyle {
                    italic: true,
                    color: builder.colors.quiet,
                    size: body_size,
                    ..builder.base.clone()
                };
                inline(&mut builder, text, &style);
                builder.end_line(ParagraphStyle::default());
            }
            Line::Paragraph(text) => {
                let style = CharStyle { size: body_size, ..builder.base.clone() };
                inline(&mut builder, text, &style);
                builder.end_line(ParagraphStyle::default());
            }
        }
    }

    // A fence nobody closed still becomes a diagram: a preview sees exactly this on every keystroke
    // while one is being typed, and showing the diagram so far is far more use than showing nothing
    // until the closing backticks arrive.
    if let Some(collected) = diagram {
        builder.diagram(&collected.join("\n"));
        builder.source_line = diagram_from;
        builder.end_line(ParagraphStyle::default());
    }
    builder.finish()
}

/// True for the language name on a fence that holds a Mermaid diagram.
///
/// Only the word itself, so ```mermaid-live` or another language whose name merely begins the same
/// way is still shown as code.
fn is_mermaid(language: &str) -> bool {
    language.split_whitespace().next().is_some_and(|word| word.eq_ignore_ascii_case("mermaid"))
}

/// Read the marks that appear inside a line: bold, italic, strikethrough, code and links.
///
/// Written as one pass over the characters rather than with nested passes, because the marks can be next to
/// each other and a nested pass would have to re-scan text it had already produced.
fn inline(builder: &mut Builder, text: &str, style: &CharStyle) {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut plain_from = 0;
    let mut bold = false;
    let mut italic = false;
    let mut struck = false;

    // Flush the plain text collected so far, in whatever the marks currently say.
    macro_rules! flush {
        ($to:expr) => {
            if $to > plain_from {
                let current = CharStyle {
                    bold: bold || style.bold,
                    italic: italic || style.italic,
                    strikethrough: struck || style.strikethrough,
                    ..style.clone()
                };
                builder.push(&text[plain_from..$to], current);
            }
        };
    }

    while at < bytes.len() {
        let rest = &text[at..];

        // Inline code comes first: nothing inside a backtick pair is interpreted.
        if let Some(inner) = rest.strip_prefix('`') {
            if let Some(end) = inner.find('`') {
                flush!(at);
                let code = builder.code_style(style.size * 0.95);
                builder.push(&inner[..end], code);
                at += 1 + end + 1;
                plain_from = at;
                continue;
            }
        }

        // A picture inside a line of prose: its alt text, in the quiet colour, because a picture in
        // the middle of a paragraph needs inline layout the engine does not have. Before the link
        // below it, so that the mark is not left behind as a stray character in front of the label.
        if rest.starts_with("![") {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    flush!(at);
                    let label = &rest[2..close];
                    let quiet = CharStyle { color: builder.colors.quiet, ..style.clone() };
                    builder.push(label, quiet);
                    at += close + 2 + end + 1;
                    plain_from = at;
                    continue;
                }
            }
        }

        // A link: the text in brackets is shown, the address is not.
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    flush!(at);
                    let label = &rest[1..close];
                    let link = CharStyle {
                        color: builder.colors.link,
                        underline: true,
                        ..style.clone()
                    };
                    // The label can itself hold marks, so it goes through this same pass.
                    inline(builder, label, &link);
                    at += close + 2 + end + 1;
                    plain_from = at;
                    continue;
                }
            }
        }

        if rest.starts_with("~~") {
            flush!(at);
            struck = !struck;
            at += 2;
            plain_from = at;
            continue;
        }
        // Two marks mean bold, one means italic. Bold is tested first so `**` is not read as two italics.
        if rest.starts_with("**") || rest.starts_with("__") {
            flush!(at);
            bold = !bold;
            at += 2;
            plain_from = at;
            continue;
        }
        if rest.starts_with('*') || rest.starts_with('_') {
            // An underscore inside a word is part of the word, as in `snake_case`, so it only opens
            // emphasis at the start of a word.
            let mark = rest.as_bytes()[0];
            let previous = text[..at].chars().last();
            let inside_word = mark == b'_'
                && previous.is_some_and(|c| c.is_alphanumeric())
                && rest[1..].chars().next().is_some_and(|c| c.is_alphanumeric());
            if !inside_word {
                flush!(at);
                italic = !italic;
                at += 1;
                plain_from = at;
                continue;
            }
        }

        // Nothing special here, so move on by one character.
        at += rest.chars().next().map(char::len_utf8).unwrap_or(1);
    }
    flush!(bytes.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(source: &str) -> Preview {
        render(source, &CharStyle::default(), PreviewColors::default(), Some("Courier".to_owned()))
    }

    /// The style covering the first occurrence of `needle`.
    fn style_of(preview: &Preview, needle: &str) -> CharStyle {
        let text = preview.text.to_string();
        let at = text.find(needle).unwrap_or_else(|| panic!("{needle:?} is not in {text:?}"));
        preview.chars.style_at(at + 1).clone()
    }

    #[test]
    fn plain_prose_comes_through_unchanged() {
        let preview = preview("Just a sentence.");
        assert_eq!(preview.text.to_string(), "Just a sentence.");
        assert_eq!(preview.chars.total_len(), preview.text.len_bytes());
        assert_eq!(preview.paragraphs.len(), preview.text.len_lines());
    }

    #[test]
    fn a_heading_loses_its_hashes_and_becomes_big_and_bold() {
        let preview = preview("# Title\n\nBody.");
        let text = preview.text.to_string();
        assert!(text.starts_with("Title"), "the hash should not be shown, got {text:?}");
        let heading = style_of(&preview, "Title");
        assert!(heading.bold);
        assert!(heading.size > CharStyle::default().size, "a heading is larger than body text");
        let body = style_of(&preview, "Body.");
        assert!(!body.bold);
        assert_eq!(body.size, CharStyle::default().size);
    }

    #[test]
    fn the_six_heading_levels_get_smaller_in_order() {
        let source = (1..=6).map(|n| format!("{} Level {n}\n", "#".repeat(n))).collect::<String>();
        let preview = preview(&source);
        let sizes: Vec<f32> = (1..=6).map(|n| style_of(&preview, &format!("Level {n}")).size).collect();
        for pair in sizes.windows(2) {
            assert!(pair[0] >= pair[1], "heading sizes should not grow: {sizes:?}");
        }
        assert!(sizes[0] > sizes[5], "level one is larger than level six");
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        let preview = preview("#hashtag is not a heading");
        assert_eq!(preview.text.to_string(), "#hashtag is not a heading");
        assert!(!style_of(&preview, "hashtag").bold);
    }

    #[test]
    fn seven_hashes_are_not_a_heading() {
        let preview = preview("####### too many");
        assert!(preview.text.to_string().starts_with("#######"));
    }

    #[test]
    fn bold_and_italic_marks_are_removed_and_applied() {
        let preview = preview("plain **bold** and *italic* and ~~struck~~ done");
        let text = preview.text.to_string();
        assert_eq!(text, "plain bold and italic and struck done");
        assert!(style_of(&preview, "bold").bold);
        assert!(!style_of(&preview, "bold").italic);
        assert!(style_of(&preview, "italic").italic);
        assert!(!style_of(&preview, "italic").bold);
        assert!(style_of(&preview, "struck").strikethrough);
        assert!(!style_of(&preview, "plain").bold);
        assert!(!style_of(&preview, "done").bold, "the marks close again");
    }

    #[test]
    fn bold_and_italic_together() {
        let preview = preview("***both*** here");
        assert_eq!(preview.text.to_string(), "both here");
        let style = style_of(&preview, "both");
        assert!(style.bold && style.italic, "three marks mean bold and italic");
    }

    #[test]
    fn underscores_inside_a_word_stay_put() {
        let preview = preview("a snake_case_name and _real emphasis_");
        let text = preview.text.to_string();
        assert!(text.contains("snake_case_name"), "got {text:?}");
        assert!(style_of(&preview, "real emphasis").italic);
    }

    #[test]
    fn inline_code_is_set_in_the_monospaced_family_and_not_interpreted() {
        let preview = preview("run `cargo **test** now` please");
        let text = preview.text.to_string();
        assert_eq!(text, "run cargo **test** now please", "nothing inside backticks is interpreted");
        let code = style_of(&preview, "cargo");
        assert_eq!(code.family, "Courier");
        assert!(!code.bold);
        assert_eq!(style_of(&preview, "please").family, CharStyle::default().family);
    }

    #[test]
    fn an_unclosed_backtick_is_shown_as_itself() {
        let preview = preview("a ` lone backtick");
        assert_eq!(preview.text.to_string(), "a ` lone backtick");
    }

    #[test]
    fn a_line_holding_only_a_picture_becomes_an_empty_paragraph_and_a_picture() {
        let preview = preview("before\n![a diagram](diagram.png)\nafter");
        assert_eq!(preview.images.len(), 1);
        let image = &preview.images[0];
        assert_eq!(image.source, "diagram.png");
        assert_eq!(image.alt, "a diagram");
        assert_eq!(image.paragraph, 1, "the line between the two words");
        let lines: Vec<String> = preview.text.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines[1], "", "the line is empty: the application paints the picture into it");
    }

    #[test]
    fn a_picture_inside_a_line_of_prose_is_shown_as_its_alt_text() {
        let preview = preview("see ![the chart](chart.png) here");
        assert!(preview.images.is_empty(), "a picture in a paragraph is not laid out as a picture");
        assert_eq!(preview.text.to_string(), "see the chart here", "and the mark itself is gone");
        assert_eq!(style_of(&preview, "the chart").color, PreviewColors::default().quiet);
    }

    #[test]
    fn a_picture_with_no_source_is_not_a_picture() {
        let preview = preview("![alt]()");
        assert!(preview.images.is_empty(), "there is no file to draw");
    }

    #[test]
    fn a_picture_with_words_after_it_stays_a_paragraph() {
        let preview = preview("![alt](picture.png) and some words");
        assert!(preview.images.is_empty());
        assert_eq!(preview.text.to_string(), "alt and some words");
    }

    #[test]
    fn a_link_that_is_not_a_picture_is_still_a_link() {
        let preview = preview("[the readme](readme.md)");
        assert!(preview.images.is_empty());
        assert_eq!(style_of(&preview, "the readme").color, PreviewColors::default().link);
    }

    #[test]
    fn several_pictures_each_name_their_own_paragraph() {
        let preview = preview("![one](a.png)\n\n![two](b.png)");
        let paragraphs: Vec<usize> = preview.images.iter().map(|image| image.paragraph).collect();
        assert_eq!(paragraphs, vec![0, 2]);
    }

    #[test]
    fn a_fenced_code_block_keeps_its_lines_and_its_indentation() {
        let preview = preview("before\n```\nfn main() {\n    let x = **1**;\n}\n```\nafter");
        let text = preview.text.to_string();
        assert!(text.contains("fn main() {"), "got {text:?}");
        assert!(text.contains("    let x = **1**;"), "indentation and marks are kept, got {text:?}");
        assert!(!text.contains("```"), "the fences themselves are not shown");
        assert_eq!(style_of(&preview, "fn main").family, "Courier");
        assert_eq!(style_of(&preview, "after").family, CharStyle::default().family);
    }

    #[test]
    fn a_link_shows_its_text_and_hides_its_address() {
        let preview = preview("see [the docs](https://example.com/page) for more");
        let text = preview.text.to_string();
        assert_eq!(text, "see the docs for more");
        let link = style_of(&preview, "the docs");
        assert!(link.underline);
        assert_eq!(link.color, PreviewColors::default().link);
        assert!(!style_of(&preview, "for more").underline);
    }

    #[test]
    fn a_link_whose_text_is_bold_keeps_both() {
        let preview = preview("[**strong link**](https://example.com)");
        assert_eq!(preview.text.to_string(), "strong link");
        let style = style_of(&preview, "strong link");
        assert!(style.bold);
        assert!(style.underline);
    }

    #[test]
    fn a_bullet_list_gets_a_bullet_and_keeps_its_text() {
        let preview = preview("- first\n- second\n* third\n+ fourth");
        let text = preview.text.to_string();
        assert_eq!(text.lines().count(), 4);
        for line in text.lines() {
            assert!(line.starts_with('\u{2022}'), "every item starts with a bullet, got {line:?}");
        }
        assert!(text.contains("first") && text.contains("fourth"));
        assert!(!text.contains("- "), "the source marks are not shown");
    }

    #[test]
    fn a_nested_bullet_is_indented_further() {
        let preview = preview("- top\n  - nested\n    - deeper");
        let lines: Vec<String> = preview.text.to_string().lines().map(str::to_owned).collect();
        let indent = |line: &str| line.len() - line.trim_start().len();
        assert_eq!(indent(&lines[0]), 0);
        assert!(indent(&lines[1]) > indent(&lines[0]), "got {lines:?}");
        assert!(indent(&lines[2]) > indent(&lines[1]));
    }

    #[test]
    fn a_numbered_list_keeps_its_numbers() {
        let preview = preview("1. first\n2. second\n10) tenth");
        let text = preview.text.to_string();
        assert!(text.contains("1."), "got {text:?}");
        assert!(text.contains("2."));
        assert!(text.contains("10."), "a bracket is accepted and shown as a dot");
        assert!(text.contains("tenth"));
    }

    #[test]
    fn a_block_quote_is_marked_and_set_apart() {
        let preview = preview("> quoted words\nplain words");
        let text = preview.text.to_string();
        assert!(text.starts_with('\u{2502}'), "a quote gets a bar, got {text:?}");
        let quote = style_of(&preview, "quoted words");
        assert!(quote.italic);
        assert_eq!(quote.color, PreviewColors::default().quiet);
        assert!(!style_of(&preview, "plain words").italic);
    }

    #[test]
    fn marks_inside_a_quote_still_work() {
        let preview = preview("> a **strong** quote");
        assert!(style_of(&preview, "strong").bold);
    }

    #[test]
    fn a_horizontal_rule_becomes_a_line() {
        for source in ["---", "***", "___", "-----"] {
            let preview = preview(source);
            let text = preview.text.to_string();
            assert!(text.contains('\u{2500}'), "{source:?} should draw a rule, got {text:?}");
            assert_eq!(style_of(&preview, "\u{2500}").color, PreviewColors::default().rule);
        }
    }

    #[test]
    fn a_blank_line_stays_a_blank_line() {
        let preview = preview("one\n\ntwo");
        assert_eq!(preview.text.len_lines(), 3, "the gap between the paragraphs is kept");
    }

    #[test]
    fn the_three_structures_always_agree_with_each_other() {
        // The spans must cover exactly the text and there must be one paragraph entry per line. If those
        // drift apart, laying the preview out is wrong or panics.
        let sources = [
            "",
            "\n",
            "# just a heading",
            "- a\n- b\n\n1. c\n\n> d\n\n---\n\n`code`",
            "**unclosed bold and *unclosed italic",
            "```\nunclosed fence\n",
            "[broken](link and [another](https://x.com)",
            "~~~\nalternative fence\n~~~",
            "text with \u{00E9} and \u{1F600} and \u{65E5}\u{672C}\u{8A9E}",
            "# \u{00E9}\u{0301} accented heading",
        ];
        for source in sources {
            let preview = preview(source);
            assert_eq!(
                preview.chars.total_len(),
                preview.text.len_bytes(),
                "spans do not cover the text for {source:?}"
            );
            assert_eq!(
                preview.paragraphs.len(),
                preview.text.len_lines(),
                "paragraph count does not match the line count for {source:?}"
            );
            // The source line map is the fourth structure and has to keep in step with the other
            // three, or scrolling the source moves the preview to a paragraph that is not there.
            assert_eq!(
                preview.source_lines.len(),
                preview.text.len_lines(),
                "the source line map does not match the line count for {source:?}"
            );
            assert!(
                preview.source_lines.windows(2).all(|pair| pair[0] <= pair[1]),
                "the source line map goes backwards for {source:?}: {:?}",
                preview.source_lines
            );
            let last = source.split('\n').count().saturating_sub(1);
            assert!(
                preview.source_lines.iter().all(|line| *line <= last),
                "the source line map names a line past the end of {source:?}: {:?}",
                preview.source_lines
            );
            // Laying it out must not panic on any of these.
            let _ = crate::layout::layout(
                &preview.text,
                &preview.chars,
                &preview.paragraphs,
                &crate::metrics::FixedMetrics::default(),
                200.0,
            );
        }
    }

    #[test]
    fn the_preview_follows_the_font_the_document_is_set_in() {
        let base = CharStyle { family: "Georgia".to_owned(), size: 20.0, ..CharStyle::default() };
        let preview = render(
            "# Heading\n\nbody text",
            &base,
            PreviewColors::default(),
            Some("Menlo".to_owned()),
        );
        assert_eq!(style_of(&preview, "body text").family, "Georgia");
        assert_eq!(style_of(&preview, "body text").size, 20.0);
        assert!(style_of(&preview, "Heading").size > 20.0);
    }

    #[test]
    fn a_system_with_no_monospaced_family_still_shows_code() {
        let preview = render("`code`", &CharStyle::default(), PreviewColors::default(), None);
        assert_eq!(preview.text.to_string(), "code");
        assert_eq!(style_of(&preview, "code").family, CharStyle::default().family);
    }

    #[test]
    fn windows_line_breaks_do_not_leave_stray_characters() {
        // A document opened from disk has already had its line breaks normalised, but a preview should not
        // fall over if a carriage return reaches it.
        let preview = preview("one\r\ntwo");
        let text = preview.text.to_string();
        assert!(!text.contains('\r') || text.lines().count() == 2, "got {text:?}");
    }
}

#[cfg(test)]
mod diagrams {
    use super::*;

    fn preview(source: &str) -> Preview {
        render(source, &CharStyle::default(), PreviewColors::default(), None)
    }

    #[test]
    fn a_mermaid_fence_becomes_a_diagram_rather_than_code() {
        let source = "# Title\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nAfter.\n";
        let preview = preview(source);
        assert_eq!(preview.diagrams.len(), 1);
        assert_eq!(preview.diagrams[0].source, "flowchart TD\n  A --> B");
        // None of the diagram's own text is in the preview: the application draws it.
        assert!(!preview.text.to_string().contains("flowchart"));
        assert!(preview.text.to_string().contains("Title"));
        assert!(preview.text.to_string().contains("After."));
    }

    #[test]
    fn the_paragraph_a_diagram_stands_in_for_is_empty_and_is_where_it_says_it_is() {
        let source = "One\n\n```mermaid\npie\n\"a\" : 1\n```\n\nTwo\n";
        let preview = preview(source);
        let lines: Vec<String> =
            preview.text.to_string().split('\n').map(str::to_owned).collect();
        let at = preview.diagrams[0].paragraph;
        assert!(lines[at].trim().is_empty(), "the line it stands in for holds no text");
        assert!(lines[..at].iter().any(|line| line.contains("One")));
        assert!(lines[at + 1..].iter().any(|line| line.contains("Two")));
    }

    #[test]
    fn an_ordinary_code_fence_is_still_shown_as_code() {
        let source = "```rust\nfn main() {}\n```\n";
        let preview = preview(source);
        assert!(preview.diagrams.is_empty());
        assert!(preview.text.to_string().contains("fn main() {}"));
    }

    #[test]
    fn a_fence_whose_language_merely_begins_the_same_way_is_code() {
        let source = "```mermaidish\nnot a diagram\n```\n";
        let preview = preview(source);
        assert!(preview.diagrams.is_empty(), "only the word itself makes a diagram");
        assert!(preview.text.to_string().contains("not a diagram"));
    }

    #[test]
    fn the_language_may_be_written_in_any_case_and_carry_more_after_it() {
        for fence in ["```Mermaid", "```MERMAID", "```mermaid  ", "```mermaid theme=dark"] {
            let source = format!("{fence}\npie\n\"a\" : 1\n```\n");
            assert_eq!(preview(&source).diagrams.len(), 1, "{fence} should open a diagram");
        }
    }

    #[test]
    fn several_diagrams_in_one_document_are_all_found_in_order() {
        let source = "```mermaid\npie\n\"a\" : 1\n```\n\nwords\n\n```mermaid\nflowchart LR\nA-->B\n```\n";
        let preview = preview(source);
        assert_eq!(preview.diagrams.len(), 2);
        assert!(preview.diagrams[0].source.starts_with("pie"));
        assert!(preview.diagrams[1].source.starts_with("flowchart"));
        assert!(preview.diagrams[0].paragraph < preview.diagrams[1].paragraph);
    }

    #[test]
    fn a_fence_nobody_closed_still_draws_what_it_had() {
        // Which is exactly what a preview sees on every keystroke while one is being typed. Showing
        // the diagram so far is far more use than showing nothing until the closing backticks land.
        let source = "```mermaid\nflowchart TD\n  A --> B\n";
        let preview = preview(source);
        assert_eq!(preview.diagrams.len(), 1);
        assert_eq!(preview.diagrams[0].source, "flowchart TD\n  A --> B");
    }

    #[test]
    fn a_tilde_fence_opens_a_diagram_too() {
        let preview = preview("~~~mermaid\npie\n\"a\" : 1\n~~~\n");
        assert_eq!(preview.diagrams.len(), 1);
    }

    #[test]
    fn the_paragraphs_still_line_up_with_the_lines_after_a_diagram() {
        // The fault this guards against is the one the picture code already had: a diagram that
        // added a line break without adding a paragraph leaves every style after it on the wrong
        // line, which shows up as headings in the wrong place further down the document.
        let source = "# One\n\n```mermaid\npie\n\"a\" : 1\n```\n\n## Two\n\nbody\n";
        let preview = preview(source);
        let lines = preview.text.to_string().split('\n').count();
        assert_eq!(preview.paragraphs.len(), lines, "one paragraph setting a line");
    }
}
