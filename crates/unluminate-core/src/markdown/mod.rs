//! Turning Markdown into styled text.
//!
//! Unluminate already has everything needed to show formatted text: a styled text model, a layout engine
//! and a painter. So the preview is not a second renderer. This module reads Markdown source and
//! produces the same three things a document holds — a rope of text, character spans over it and one
//! paragraph setting per line — and the ordinary layout, the ordinary painter, the ordinary
//! scrollbar and the ordinary hit testing draw it. That is what makes selecting text in the preview
//! a small feature rather than a second code path.
//!
//! It is written here rather than taken from a crate for the reason
//! `tasks/task-1685-markdown-tdd.md` §2 gives: a Markdown crate produces events shaped for HTML, and
//! Unluminate has no HTML, no box model and no inline layout. Everything a crate gave back would have to
//! be walked and re-expressed as those four things, which is the whole of the work; what it would
//! save is the tokenising, which is the part that has tests.
//!
//! Three modules do the reading. [`blocks`] says what the lines are — a tree, so a list item can
//! hold a paragraph and a quote can hold a list. [`inline`] says what the marks inside a line mean,
//! through CommonMark's delimiter stack rather than a set of toggles. [`table`] reads a pipe table
//! and draws it in a box. This module turns what they produce into text, spans and paragraph styles.
//!
//! ## Images and diagrams
//!
//! A line whose whole content is an image mark becomes an **empty paragraph** and an entry in
//! [`Preview::images`]. Empty rather than carrying the alt text, because the application draws that
//! line itself: the picture once it has decoded it, and the alt text in the quiet colour when it
//! cannot. Nothing here reads a file or knows what a picture is — this crate has no user interface
//! dependency and cannot decode one.
//!
//! A ```` ```mermaid ```` fence is the same idea again: an empty paragraph and an entry in
//! [`Preview::diagrams`], laid out by [`crate::mermaid`] and painted by the window. **A fence nobody
//! has closed yet is still a diagram**, because a preview is worked out again on every keystroke and
//! the half-typed state is the common case rather than the odd one.
//!
//! ## Code
//!
//! A fence's contents are coloured by whoever is drawing, through [`CodeHighlighter`]: this crate
//! holds no plugin registry and must not learn about one, so it asks a question and the window
//! answers it with the same grammar and the same theme it colours a source file with. A language
//! nothing claims falls back to one colour for the whole block, which is what the preview did before
//! and is why the change can never make anything worse.
//!
//! Where the code blocks are is reported as [`Preview::panels`], so the window can paint a panel
//! behind them, and where the inline code is as [`Preview::code_spans`], so it can paint a chip
//! behind each. Neither is a drawing decision made here — this crate says which paragraphs and which
//! bytes, and the window decides what a code background looks like.

mod blocks;
mod entity;
mod inline;
mod table;

use std::ops::Range;

use crate::rope::Rope;
use crate::style::{
    Align, CharStyle, Color, ParagraphStyle, ParagraphStyles, StyleChange, StyleSpans,
};

use blocks::{Block, Item, Kind, Line, List};
use inline::{Kind as SpanKind, References, Span};
use table::Table;

/// How much bigger each heading level is than body text. Level one is the first entry.
const HEADING_SCALE: [f32; 6] = [1.9, 1.55, 1.3, 1.15, 1.05, 1.0];

/// How much smaller code is set than the prose round it.
const CODE_SCALE: f32 = 0.95;

/// How many characters of the code font a table is fitted to when nobody has measured the pane.
const DEFAULT_COLUMNS: usize = 80;

/// How wide a horizontal rule is drawn, in box-drawing characters.
const RULE_WIDTH: usize = 48;

/// Colours the preview uses for the parts that are not ordinary text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewColors {
    /// Ordinary text and headings.
    pub text: Color,
    /// Inline code and code blocks.
    pub code: Color,
    /// A link's text.
    pub link: Color,
    /// A block quote, the bullet or number in front of a list item, and a table's rules.
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

/// Something that can colour the inside of a fenced code block.
///
/// The seam exists so that a fence of Rust in a document is coloured exactly as a `.rs` file is,
/// without this crate learning what a plugin is. The window implements it over the grammars it has
/// already loaded; a test implements it in three lines.
pub trait CodeHighlighter {
    /// Colours for `code`, as byte ranges into it. An empty answer means the language is not one
    /// this machine knows, and the block is drawn in the single code colour.
    fn colour(&self, language: &str, code: &str) -> Vec<(Range<usize>, Color)>;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDiagram {
    /// Which paragraph of [`Preview::text`] stands in for the diagram.
    pub paragraph: usize,
    /// Everything between the fences, which is a whole Mermaid diagram.
    pub source: String,
}

/// What kind of block a panel is drawn behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    /// A fenced or indented code block.
    Code,
    /// The block of settings at the top of a file written for a static site.
    FrontMatter,
    /// A table, so the grid sits on a ground of its own.
    Table,
}

/// A run of paragraphs the window should paint a background behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewPanel {
    /// The paragraphs it covers, which are contiguous.
    pub paragraphs: Range<usize>,
    pub kind: PanelKind,
}

/// Markdown turned into text that Unluminate can lay out.
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
    /// all, a whole Mermaid fence produces a single one, and a table row too wide for the pane
    /// produces several — each named after the line the block **opened** on, because that is the
    /// line a reader scrolling to it is looking for.
    pub source_lines: Vec<usize>,
    /// The pictures, in the order they appear.
    pub images: Vec<PreviewImage>,
    /// The diagrams, in the order they appear.
    pub diagrams: Vec<PreviewDiagram>,
    /// The runs of paragraphs that want a background behind them.
    pub panels: Vec<PreviewPanel>,
    /// Where the inline code is, so a chip can be drawn behind each piece.
    pub code_spans: Vec<Range<usize>>,
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
            panels: Vec::new(),
            code_spans: Vec::new(),
        }
    }
}

/// Everything the preview needs to know that is not the source.
pub struct Options<'a> {
    /// The family, the size and the colour of ordinary text. Everything else is worked out from it,
    /// so the preview follows the font the document is set in.
    pub base: CharStyle,
    pub colors: PreviewColors,
    /// A monospaced family to set code and tables in, if this system has one.
    pub mono: Option<String>,
    /// How many characters of the monospaced font fit across the pane, which is what a table is
    /// fitted to. It is the one measurement the whole feature takes, and the caller takes it,
    /// because this crate has no fonts.
    pub columns: usize,
    /// Who colours the inside of a fenced code block, if anybody.
    pub highlighter: Option<&'a dyn CodeHighlighter>,
}

impl<'a> Options<'a> {
    /// The options for a preview nobody has measured a pane for, which is what most tests want.
    pub fn new(base: CharStyle, colors: PreviewColors, mono: Option<String>) -> Self {
        Self { base, colors, mono, columns: DEFAULT_COLUMNS, highlighter: None }
    }
}

/// Read `source` as Markdown and produce text Unluminate can lay out.
pub fn render(source: &str, options: &Options<'_>) -> Preview {
    let (parsed, references) = blocks::parse(source);
    let mut writer = Writer::new(options, references);
    writer.sequence(&parsed, &Prefix::default(), true);
    writer.finish()
}

/// What goes in front of every line of a block: a quote's bars and a list's indents, in the order
/// they were opened.
#[derive(Debug, Clone, Default)]
struct Prefix {
    pieces: Vec<(String, bool)>,
}

impl Prefix {
    /// The same prefix with `text` added. `quiet` says whether it is drawn in the quiet colour,
    /// which a quote's bar and a list's bullet are and a list's indent is not.
    fn with(&self, text: &str, quiet: bool) -> Self {
        let mut pieces = self.pieces.clone();
        pieces.push((text.to_owned(), quiet));
        Self { pieces }
    }

    /// How many characters wide it is, which a table has to know to fit into what is left.
    fn width(&self) -> usize {
        self.pieces.iter().map(|(text, _)| text.chars().count()).sum()
    }
}

/// Builds up the preview text and everything alongside it as the blocks are walked.
struct Writer<'a> {
    out: String,
    /// One entry per span, as a length and the style covering it.
    runs: Vec<(usize, CharStyle)>,
    /// One entry per line of `out`.
    paragraphs: Vec<ParagraphStyle>,
    /// Which line of the source the line being built came from. Set as the blocks are walked, so
    /// that [`Writer::end_line`] can record it without every branch passing it in.
    source_line: usize,
    source_lines: Vec<usize>,
    images: Vec<PreviewImage>,
    diagrams: Vec<PreviewDiagram>,
    panels: Vec<PreviewPanel>,
    code_spans: Vec<Range<usize>>,
    /// The style ordinary text is in right now, which a quote bends for its whole subtree.
    base: CharStyle,
    /// How many quotes deep the walk is, which decides whether a heading keeps the quiet colour.
    quoted: usize,
    options: &'a Options<'a>,
    references: References,
}

impl<'a> Writer<'a> {
    fn new(options: &'a Options<'a>, references: References) -> Self {
        Self {
            out: String::new(),
            runs: Vec::new(),
            paragraphs: Vec::new(),
            source_line: 0,
            source_lines: Vec::new(),
            images: Vec::new(),
            diagrams: Vec::new(),
            panels: Vec::new(),
            code_spans: Vec::new(),
            base: options.base.clone(),
            quoted: 0,
            options,
            references,
        }
    }

    fn colors(&self) -> PreviewColors {
        self.options.colors
    }

    /// The monospaced family, or the ordinary one on a system with none.
    fn mono(&self) -> String {
        self.options.mono.clone().unwrap_or_else(|| self.options.base.family.clone())
    }

    /// The style code is set in: the monospaced family and the code colour.
    fn code_style(&self, size: f32) -> CharStyle {
        CharStyle { family: self.mono(), size, color: self.colors().code, ..CharStyle::default() }
    }

    /// Add text in `style`. Neighbouring runs with the same style are folded together, so a line of
    /// plain prose is one span rather than one span a word.
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
        let style =
            self.runs.last().map(|(_, style)| style.clone()).unwrap_or_else(|| self.base.clone());
        match self.runs.last_mut() {
            Some((length, last)) if *last == style => *length += 1,
            _ => self.runs.push((1, style)),
        }
    }

    /// How many lines have been written, which is the paragraph a block is about to start on.
    fn line_count(&self) -> usize {
        self.paragraphs.len()
    }

    /// Put the prefix at the start of a line: a quote's bars, a list's bullet, a list's indents.
    fn start_line(&mut self, prefix: &Prefix) {
        let quiet = CharStyle { color: self.colors().quiet, italic: false, ..self.base.clone() };
        let plain = self.base.clone();
        for (text, is_quiet) in prefix.pieces.clone() {
            let style = if is_quiet { quiet.clone() } else { plain.clone() };
            self.push(&text, style);
        }
    }

    /// A line with nothing on it, which is what separates one block from the next.
    fn blank(&mut self, prefix: &Prefix) {
        self.start_line(prefix);
        if prefix.pieces.is_empty() {
            let base = self.base.clone();
            self.push(" ", base);
        }
        self.end_line(ParagraphStyle::default());
    }

    /// Write a run of blocks, with a blank line between them when they are separated.
    ///
    /// A blank carries the source line of the block it follows, which is what makes `source_lines`
    /// never go backwards. Which of two preview lines naming one source line is the one to scroll to
    /// is `scroll_sync`'s question, and it takes the first — so a block always wins over the blank
    /// in front of it.
    fn sequence(&mut self, blocks: &[Block], prefix: &Prefix, separated: bool) {
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 && separated {
                self.blank(prefix);
            }
            self.block(block, prefix);
        }
    }

    fn block(&mut self, block: &Block, prefix: &Prefix) {
        self.source_line = block.line;
        match &block.kind {
            Kind::Heading { level, content } => self.heading(*level, content, prefix),
            Kind::Paragraph { content } => {
                let style = self.base.clone();
                let spans = inline::parse(content, &self.references);
                self.spans(&spans, &style, prefix, ParagraphStyle::default());
            }
            Kind::Quote(inner) => self.quote(inner, &prefix.with("\u{2502}  ", true)),
            Kind::List(list) => self.list(list, prefix),
            Kind::Code { language, lines } => self.code(language, lines, prefix),
            Kind::Diagram { source } => {
                self.diagrams
                    .push(PreviewDiagram { paragraph: self.line_count(), source: source.clone() });
                self.end_line(ParagraphStyle::default());
            }
            Kind::Table(parsed) => self.table(parsed, prefix),
            Kind::Rule => self.rule(prefix),
            Kind::FrontMatter(lines) => self.front_matter(lines, prefix),
            Kind::Image { source, alt } => {
                self.images.push(PreviewImage {
                    paragraph: self.line_count(),
                    source: source.clone(),
                    alt: alt.clone(),
                });
                self.end_line(ParagraphStyle::default());
            }
            Kind::Footnote { number, blocks } => self.footnote(*number, blocks, prefix),
        }
    }

    /// Write one block's worth of spans, which may be more than one line if it holds a hard break.
    fn spans(
        &mut self,
        spans: &[Span],
        base: &CharStyle,
        prefix: &Prefix,
        paragraph: ParagraphStyle,
    ) {
        self.start_line(prefix);
        for span in spans {
            if span.kind == SpanKind::Break {
                self.end_line(paragraph);
                self.start_line(prefix);
                continue;
            }
            let style = self.style_for(span, base);
            let from = self.out.len();
            self.push(&span.text, style);
            if span.kind == SpanKind::Code && self.out.len() > from {
                self.code_spans.push(from..self.out.len());
            }
        }
        self.end_line(paragraph);
    }

    /// What one span looks like, worked out from the block's own base style.
    fn style_for(&self, span: &Span, base: &CharStyle) -> CharStyle {
        let colors = self.colors();
        let bold = span.bold || base.bold;
        let italic = span.italic || base.italic;
        let struck = span.strike || base.strikethrough;
        match span.kind {
            SpanKind::Code => {
                // Code inside something already set in the code font — a table cell — keeps that
                // size, or the columns would stop lining up.
                let size =
                    if base.family == self.mono() { base.size } else { base.size * CODE_SCALE };
                CharStyle {
                    bold,
                    italic,
                    strikethrough: struck,
                    ..self.code_style(size)
                }
            }
            SpanKind::Link => CharStyle {
                color: colors.link,
                underline: true,
                bold,
                italic,
                strikethrough: struck,
                ..base.clone()
            },
            SpanKind::Quiet => {
                CharStyle { color: colors.quiet, bold, italic, strikethrough: struck, ..base.clone() }
            }
            _ => CharStyle { bold, italic, strikethrough: struck, ..base.clone() },
        }
    }

    fn heading(&mut self, level: usize, content: &str, prefix: &Prefix) {
        let size = self.base.size * HEADING_SCALE[(level - 1).min(5)];
        // A heading inside a quote stays the quote's colour, so the quote reads as one thing.
        let color = if self.quoted > 0 { self.base.color } else { self.colors().text };
        let style = CharStyle { size, bold: true, color, ..self.base.clone() };
        let spans = inline::parse(content, &self.references);
        self.spans(
            &spans,
            &style,
            prefix,
            ParagraphStyle { align: Align::Left, line_spacing: 1.15, ..ParagraphStyle::default() },
        );
    }

    /// A quote is said in the quiet colour and in italics, behind a bar. It is the one place the
    /// base style is bent for a whole subtree, which is what lets a quote hold a list or a heading
    /// and still read as a quotation.
    fn quote(&mut self, inner: &[Block], prefix: &Prefix) {
        let was = self.base.clone();
        self.base = CharStyle { color: self.colors().quiet, italic: true, ..was.clone() };
        self.quoted += 1;
        self.sequence(inner, prefix, true);
        self.quoted -= 1;
        self.base = was;
    }

    fn list(&mut self, list: &List, prefix: &Prefix) {
        for (index, item) in list.items.iter().enumerate() {
            if index > 0 && !list.tight {
                self.blank(prefix);
            }
            self.item(list, index, item, prefix);
        }
    }

    fn item(&mut self, list: &List, index: usize, item: &Item, prefix: &Prefix) {
        let marker = match (item.task, list.ordered) {
            (Some(true), _) => "\u{2611}  ".to_owned(),
            (Some(false), _) => "\u{2610}  ".to_owned(),
            (None, false) => "\u{2022}  ".to_owned(),
            (None, true) => format!("{}.  ", list.start + index as u64),
        };
        let indent = " ".repeat(marker.chars().count());
        let first = prefix.with(&marker, true);
        let rest = prefix.with(&indent, false);
        self.source_line = item.line;
        let Some((head, tail)) = item.blocks.split_first() else {
            self.start_line(&first);
            self.end_line(ParagraphStyle::default());
            return;
        };
        self.block(head, &first);
        for block in tail {
            // A list nested under an item follows straight on, because the item's own words are what
            // it belongs to. Anything else is a second block and gets air.
            if !matches!(block.kind, Kind::List(_)) {
                self.blank(&rest);
            }
            self.block(block, &rest);
        }
    }

    /// A code block: one preview line a source line, coloured by whoever is drawing, on a panel.
    fn code(&mut self, language: &str, lines: &[Line], prefix: &Prefix) {
        let start = self.line_count();
        let source = lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n");
        let colours = self
            .options
            .highlighter
            .map(|highlighter| highlighter.colour(language, &source))
            .unwrap_or_default();
        let plain = self.code_style(self.base.size * CODE_SCALE);
        let paragraph =
            ParagraphStyle { align: Align::Left, line_spacing: 1.0, ..ParagraphStyle::default() };
        let mut offset = 0;
        for line in lines {
            self.source_line = line.number;
            self.start_line(prefix);
            for (piece, colour) in split_by_colour(&line.text, offset, &colours) {
                let style = match colour {
                    Some(color) => CharStyle { color, ..plain.clone() },
                    None => plain.clone(),
                };
                self.push(piece, style);
            }
            self.end_line(paragraph);
            offset += line.text.len() + 1;
        }
        if lines.is_empty() {
            self.start_line(prefix);
            self.push(" ", plain);
            self.end_line(paragraph);
        }
        self.panels
            .push(PreviewPanel { paragraphs: start..self.line_count(), kind: PanelKind::Code });
    }

    fn table(&mut self, parsed: &Table, prefix: &Prefix) {
        let start = self.line_count();
        // A table inside a quote or a list has less room, because its bar and its indent are drawn
        // in front of every one of its lines.
        let available = self.options.columns.saturating_sub(prefix.width()).max(16);
        let drawn = table::draw(parsed, &self.references, available);
        let base = CharStyle {
            family: self.mono(),
            size: self.base.size * CODE_SCALE,
            color: self.base.color,
            ..CharStyle::default()
        };
        let paragraph =
            ParagraphStyle { align: Align::Left, line_spacing: 1.0, ..ParagraphStyle::default() };
        for line in &drawn.lines {
            self.spans(line, &base, prefix, paragraph);
        }
        self.panels
            .push(PreviewPanel { paragraphs: start..self.line_count(), kind: PanelKind::Table });
    }

    /// A rule is drawn as a run of box-drawing characters, because the layout engine places glyphs
    /// and has no notion of a line that is not text.
    fn rule(&mut self, prefix: &Prefix) {
        self.start_line(prefix);
        let style =
            CharStyle { color: self.colors().rule, italic: false, ..self.options.base.clone() };
        self.push(&"\u{2500}".repeat(RULE_WIDTH), style);
        self.end_line(ParagraphStyle::default());
    }

    /// The block of settings at the top of a file written for a static site. Shown quietly rather
    /// than as a rule followed by a paragraph of YAML, which is what it used to look like.
    fn front_matter(&mut self, lines: &[Line], prefix: &Prefix) {
        let start = self.line_count();
        let style = CharStyle {
            family: self.mono(),
            size: self.base.size * CODE_SCALE,
            color: self.colors().quiet,
            ..CharStyle::default()
        };
        let paragraph =
            ParagraphStyle { align: Align::Left, line_spacing: 1.0, ..ParagraphStyle::default() };
        for line in lines {
            self.source_line = line.number;
            self.start_line(prefix);
            self.push(&line.text, style.clone());
            self.end_line(paragraph);
        }
        if lines.is_empty() {
            self.start_line(prefix);
            self.push(" ", style);
            self.end_line(paragraph);
        }
        self.panels.push(PreviewPanel {
            paragraphs: start..self.line_count(),
            kind: PanelKind::FrontMatter,
        });
    }

    fn footnote(&mut self, number: usize, blocks: &[Block], prefix: &Prefix) {
        let marker = format!("[{number}]  ");
        let indent = " ".repeat(marker.chars().count());
        let first = prefix.with(&marker, true);
        let rest = prefix.with(&indent, false);
        let Some((head, tail)) = blocks.split_first() else {
            self.start_line(&first);
            self.end_line(ParagraphStyle::default());
            return;
        };
        self.block(head, &first);
        for block in tail {
            self.blank(&rest);
            self.block(block, &rest);
        }
    }

    fn finish(mut self) -> Preview {
        // The trailing line break is dropped, because a document's last line does not end with one.
        //
        // The paragraph list is left alone. Every `end_line` added one line break and one entry, so
        // N entries go with N line breaks, and removing the last break leaves N lines, which is what
        // N entries describes. Removing an entry here as well would leave the two out of step.
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
        let mut chars = StyleSpans::new(0, self.options.base.clone());
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
            panels: self.panels,
            code_spans: self.code_spans,
        }
    }
}

/// Cut one line of a code block into the pieces the highlighter gave different colours.
///
/// `offset` is where the line starts in the whole block, since the colours are ranges over the
/// block. A gap between two coloured ranges is text the grammar had nothing to say about, and it
/// comes back with no colour so it keeps the ordinary code colour.
fn split_by_colour<'t>(
    text: &'t str,
    offset: usize,
    colours: &[(Range<usize>, Color)],
) -> Vec<(&'t str, Option<Color>)> {
    if colours.is_empty() || text.is_empty() {
        return vec![(text, None)];
    }
    let line = offset..offset + text.len();
    let mut out = Vec::new();
    let mut at = line.start;
    for (range, color) in colours {
        if range.end <= line.start {
            continue;
        }
        if range.start >= line.end {
            break;
        }
        let from = range.start.max(at);
        let to = range.end.min(line.end);
        if from > at {
            out.push((&text[at - offset..from - offset], None));
        }
        if to > from {
            out.push((&text[from - offset..to - offset], Some(*color)));
        }
        at = to;
    }
    if at < line.end {
        out.push((&text[at - offset..], None));
    }
    out
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

#[cfg(test)]
mod tests;
