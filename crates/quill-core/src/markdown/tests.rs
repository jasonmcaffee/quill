//! What the preview is held to.
//!
//! Two things are asserted here and they are different in kind. The **battery** is one case a row of
//! the table in `tasks/task-1685-markdown-tdd.md` §1: a piece of source, and what the preview should
//! read once the marks have been applied. The **properties** are the four invariants every preview
//! must satisfy whatever it holds, checked for every case in the battery as well as on their own —
//! the same shape `mermaid::check::properties` already has, and for the same reason: a case added
//! later inherits them without anybody remembering to.
//!
//! Nothing here asserts on a number a font decides. The preview is measured in characters and in
//! which style covers which word, so every one of these runs on any machine with no fonts at all.

use super::*;

fn options() -> Options<'static> {
    Options::new(CharStyle::default(), PreviewColors::default(), Some("Courier".to_owned()))
}

fn preview(source: &str) -> Preview {
    let preview = render(source, &options());
    properties(&preview, source);
    preview
}

fn text(source: &str) -> String {
    preview(source).text.to_string()
}

/// The style covering the first occurrence of `needle`.
fn style_of(preview: &Preview, needle: &str) -> CharStyle {
    let text = preview.text.to_string();
    let at = text.find(needle).unwrap_or_else(|| panic!("{needle:?} is not in {text:?}"));
    preview.chars.style_at(at + 1).clone()
}

fn style(source: &str, needle: &str) -> CharStyle {
    let preview = preview(source);
    style_of(&preview, needle)
}

/// The four things that must be true of every preview, whatever the source was.
///
/// One of these failing is not a wrong-looking document, it is a crash or a blank pane: the layout
/// engine indexes the paragraph list by line number, and the scroll crossing indexes `source_lines`
/// the same way.
fn properties(preview: &Preview, source: &str) {
    let lines = preview.text.len_lines();
    assert_eq!(
        preview.chars.total_len(),
        preview.text.len_bytes(),
        "the spans must cover the text exactly, for {source:?}"
    );
    assert_eq!(preview.paragraphs.len(), lines, "one paragraph style a line, for {source:?}");
    assert_eq!(preview.source_lines.len(), lines, "one source line a line, for {source:?}");
    assert!(
        preview.source_lines.windows(2).all(|pair| pair[0] <= pair[1]),
        "the source lines must never go backwards, for {source:?}: {:?}",
        preview.source_lines
    );
    for image in &preview.images {
        assert!(image.paragraph < lines, "a picture outside the text, for {source:?}");
    }
    for diagram in &preview.diagrams {
        assert!(diagram.paragraph < lines, "a diagram outside the text, for {source:?}");
    }
    for panel in &preview.panels {
        assert!(panel.paragraphs.end <= lines, "a panel outside the text, for {source:?}");
        assert!(panel.paragraphs.start < panel.paragraphs.end, "an empty panel, for {source:?}");
    }
    for span in &preview.code_spans {
        assert!(span.end <= preview.text.len_bytes(), "a chip outside the text, for {source:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// The battery. One row of the audit table apiece.
// ---------------------------------------------------------------------------------------------

/// Source, and what the preview should read. The marks are gone; the words are not.
const BATTERY: &[(&str, &str)] = &[
    ("plain prose", "plain prose"),
    ("# Title", "Title"),
    ("###### Six", "Six"),
    ("## Closed ##", "Closed"),
    ("Setext\n======", "Setext"),
    ("Setext\n------", "Setext"),
    ("**bold** and *italic*", "bold and italic"),
    ("~~struck~~", "struck"),
    ("***both***", "both"),
    ("2 * 3 * 4", "2 * 3 * 4"),
    ("a snake_case_name", "a snake_case_name"),
    ("\\*escaped\\*", "*escaped*"),
    ("A &amp; B", "A & B"),
    ("`inline code`", "inline code"),
    ("`` a ` b ``", "a ` b"),
    ("[words](https://example.com)", "words"),
    ("<https://example.com>", "https://example.com"),
    ("go to https://example.com now", "go to https://example.com now"),
    ("a <span>tag</span> here", "a tag here"),
    ("wrapped\nover two lines", "wrapped over two lines"),
    ("- [ ] to do", "\u{2610}  to do"),
    ("- [x] done", "\u{2611}  done"),
    ("- a bullet", "\u{2022}  a bullet"),
    ("3. third", "3.  third"),
    ("> quoted", "\u{2502}  quoted"),
    ("> > twice", "\u{2502}  \u{2502}  twice"),
];

#[test]
fn the_battery_reads_as_it_should() {
    for (source, expected) in BATTERY {
        assert_eq!(&text(source), expected, "for source {source:?}");
    }
}

#[test]
fn the_properties_hold_for_every_case_in_the_battery() {
    // `preview` checks them; this is the test that says so out loud.
    for (source, _) in BATTERY {
        preview(source);
    }
}

// ---------------------------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------------------------

#[test]
fn a_heading_loses_its_hashes_and_becomes_big_and_bold() {
    let heading = style("# Title\n\nBody.", "Title");
    assert!(heading.bold);
    assert!(heading.size > CharStyle::default().size, "a heading is larger than body text");
    let body = style("# Title\n\nBody.", "Body.");
    assert!(!body.bold);
    assert_eq!(body.size, CharStyle::default().size);
}

#[test]
fn the_six_heading_levels_get_smaller() {
    let sizes: Vec<f32> = (1..=6)
        .map(|level| {
            let source = format!("{} Heading{level}", "#".repeat(level));
            style(&source, "Heading").size
        })
        .collect();
    assert!(sizes.windows(2).all(|pair| pair[0] > pair[1]), "{sizes:?}");
}

#[test]
fn a_wrapped_paragraph_is_one_line_of_preview() {
    let preview = preview("one line\nand the next\nand a third");
    assert_eq!(preview.text.len_lines(), 1);
}

#[test]
fn two_trailing_spaces_break_the_line() {
    let preview = preview("one line  \nand the next");
    assert_eq!(preview.text.to_string(), "one line\nand the next");
}

#[test]
fn a_rule_is_drawn_as_a_line_of_its_own() {
    let preview = preview("above\n\n---\n\nbelow");
    let text = preview.text.to_string();
    assert!(text.contains('\u{2500}'), "got {text:?}");
    assert_eq!(style_of(&preview, "\u{2500}").color, PreviewColors::default().rule);
}

#[test]
fn a_quote_gets_a_bar_and_is_said_quietly() {
    let preview = preview("> quoted words");
    let quoted = style_of(&preview, "quoted words");
    assert!(quoted.italic);
    assert_eq!(quoted.color, PreviewColors::default().quiet);
    assert_eq!(style_of(&preview, "\u{2502}").color, PreviewColors::default().quiet);
}

#[test]
fn a_quote_can_hold_a_list_and_a_heading() {
    let text = text("> ## Inside\n>\n> - a bullet\n> - another");
    assert!(text.contains("\u{2502}  Inside"), "got {text:?}");
    assert!(text.contains("\u{2502}  \u{2022}  a bullet"), "got {text:?}");
}

#[test]
fn a_quote_continues_lazily() {
    assert_eq!(text("> one line\nand the next"), "\u{2502}  one line and the next");
}

#[test]
fn a_tight_list_has_no_air_between_its_items() {
    let preview = preview("- a\n- b\n- c");
    assert_eq!(preview.text.len_lines(), 3, "got {:?}", preview.text.to_string());
}

#[test]
fn a_loose_list_has_air_between_its_items() {
    let preview = preview("- a\n\n- b");
    assert_eq!(preview.text.len_lines(), 3, "got {:?}", preview.text.to_string());
}

#[test]
fn a_nested_list_is_indented_under_its_item() {
    let text = text("- outer\n  - inner");
    assert_eq!(text, "\u{2022}  outer\n   \u{2022}  inner");
}

#[test]
fn a_list_item_can_hold_a_paragraph_of_its_own() {
    let text = text("- first\n\n  more of the first\n\n- second");
    assert!(text.contains("   more of the first"), "the second paragraph is indented: {text:?}");
}

#[test]
fn a_numbered_list_counts_from_where_it_started() {
    assert_eq!(text("3. third\n4. fourth"), "3.  third\n4.  fourth");
}

#[test]
fn a_fence_keeps_its_spacing_and_is_set_in_the_code_font() {
    let preview = preview("```\nfn main() {\n    println!();\n}\n```");
    let text = preview.text.to_string();
    assert!(text.contains("    println!();"), "the indent survives: {text:?}");
    assert_eq!(style_of(&preview, "fn main").family, "Courier");
    assert_eq!(style_of(&preview, "fn main").color, PreviewColors::default().code);
}

#[test]
fn a_code_block_asks_for_a_panel_behind_it() {
    let preview = preview("above\n\n```\ncode\nmore\n```\n\nbelow");
    let panel = preview.panels.iter().find(|panel| panel.kind == PanelKind::Code).expect("a panel");
    assert_eq!(panel.paragraphs.len(), 2, "one paragraph a line of code");
    let lines: Vec<String> =
        preview.text.to_string().lines().map(str::to_owned).collect();
    assert_eq!(lines[panel.paragraphs.start], "code");
}

#[test]
fn four_spaces_of_indent_is_a_code_block_too() {
    let preview = preview("text\n\n    indented\n\nmore text");
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::Code));
    assert!(preview.text.to_string().contains("indented"));
}

#[test]
fn inline_code_asks_for_a_chip_behind_exactly_its_own_bytes() {
    let preview = preview("say `hello` now");
    assert_eq!(preview.code_spans.len(), 1);
    let range = preview.code_spans[0].clone();
    assert_eq!(&preview.text.to_string()[range], "hello");
}

#[test]
fn front_matter_is_shown_quietly_rather_than_as_a_rule() {
    let preview = preview("---\ntitle: A page\n---\n\nThe body.");
    let text = preview.text.to_string();
    assert!(text.starts_with("title: A page"), "got {text:?}");
    assert!(!text.contains('\u{2500}'), "it is not a horizontal rule: {text:?}");
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::FrontMatter));
    assert_eq!(style_of(&preview, "title").color, PreviewColors::default().quiet);
}

#[test]
fn a_picture_on_its_own_line_leaves_an_empty_paragraph_for_the_window() {
    let preview = preview("before\n\n![the alt](picture.png)\n\nafter");
    assert_eq!(preview.images.len(), 1);
    let image = &preview.images[0];
    assert_eq!(image.source, "picture.png");
    assert_eq!(image.alt, "the alt");
    let lines: Vec<String> = preview.text.to_string().split('\n').map(str::to_owned).collect();
    assert_eq!(lines[image.paragraph], "", "the line itself is empty");
}

#[test]
fn a_mermaid_fence_becomes_a_diagram_named_after_the_line_it_opened_on() {
    let preview = preview("intro\n\n```mermaid\ngraph TD\n  A --> B\n```\n\nafter");
    assert_eq!(preview.diagrams.len(), 1);
    assert_eq!(preview.diagrams[0].source, "graph TD\n  A --> B");
    assert_eq!(preview.source_lines[preview.diagrams[0].paragraph], 2);
}

#[test]
fn a_fence_nobody_has_closed_yet_is_still_a_diagram() {
    let preview = preview("```mermaid\ngraph TD\n  A --> B");
    assert_eq!(preview.diagrams.len(), 1);
    assert_eq!(preview.diagrams[0].source, "graph TD\n  A --> B");
}

#[test]
fn a_footnote_is_numbered_where_it_is_used_and_where_it_is_written() {
    let text = text("A claim[^why].\n\n[^why]: Because of this.");
    assert!(text.contains("A claim[1]."), "got {text:?}");
    assert!(text.contains("[1]  Because of this."), "got {text:?}");
}

#[test]
fn a_reference_link_is_read_and_its_definition_is_not_shown() {
    let text = text("See [the design][d].\n\n[d]: https://example.com/design");
    assert_eq!(text, "See the design.");
}

// ---------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------

#[test]
fn a_table_is_drawn_in_a_box_and_the_pipes_are_gone() {
    let preview = preview("| Crate | Lines |\n| --- | ---: |\n| core | 9132 |");
    let text = preview.text.to_string();
    assert!(!text.contains('|'), "the pipes are the box, not the text: {text:?}");
    assert!(text.contains("\u{250C}"), "a top left corner: {text:?}");
    assert!(text.contains("Crate"), "the words survive: {text:?}");
    assert!(text.contains("9132"));
}

#[test]
fn a_table_is_set_in_the_code_font_so_its_columns_line_up() {
    let preview = preview("| a | b |\n| --- | --- |\n| 1 | 2 |");
    assert_eq!(style_of(&preview, "a").family, "Courier");
    // The rules are quiet and the data is not, so the grid recedes.
    assert_eq!(style_of(&preview, "\u{250C}").color, PreviewColors::default().quiet);
    assert_eq!(style_of(&preview, "1").color, PreviewColors::default().text);
}

#[test]
fn the_head_of_a_table_is_bold() {
    let preview = preview("| head |\n| --- |\n| body |");
    assert!(style_of(&preview, "head").bold);
    assert!(!style_of(&preview, "body").bold);
}

#[test]
fn every_line_of_a_table_is_the_same_width() {
    let preview = preview("| one | two |\n| --- | --- |\n| a | bbbbbbb |\n| ccc | d |");
    let widths: Vec<usize> =
        preview.text.to_string().lines().map(|line| line.chars().count()).collect();
    assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "{widths:?}");
}

#[test]
fn a_table_asks_for_a_panel_and_stays_inside_the_pane() {
    let mut options = options();
    options.columns = 30;
    let source = "| one | two |\n| --- | --- |\n| a much longer cell than fits | and another |";
    let preview = render(source, &options);
    properties(&preview, source);
    for line in preview.text.to_string().lines() {
        assert!(line.chars().count() <= 30, "{line:?}");
    }
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::Table));
}

#[test]
fn a_table_inside_a_quote_keeps_its_bar_and_still_fits() {
    let mut options = options();
    options.columns = 40;
    let source = "> | one | two |\n> | --- | --- |\n> | a | b |";
    let preview = render(source, &options);
    properties(&preview, source);
    for line in preview.text.to_string().lines() {
        assert!(line.starts_with('\u{2502}'), "every line keeps the quote's bar: {line:?}");
        assert!(line.chars().count() <= 40, "{line:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Colouring a fence
// ---------------------------------------------------------------------------------------------

/// A highlighter that paints one word red, which is enough to prove the seam works.
struct OneWord;

impl CodeHighlighter for OneWord {
    fn colour(&self, language: &str, code: &str) -> Vec<(Range<usize>, Color)> {
        if language != "rust" {
            return Vec::new();
        }
        code.match_indices("fn").map(|(at, word)| (at..at + word.len(), Color::RED)).collect()
    }
}

#[test]
fn a_fence_is_coloured_by_whoever_is_drawing() {
    let highlighter = OneWord;
    let mut options = options();
    options.highlighter = Some(&highlighter);
    let source = "```rust\nfn main() {}\n```";
    let preview = render(source, &options);
    properties(&preview, source);
    assert_eq!(style_of(&preview, "fn").color, Color::RED);
    assert_eq!(style_of(&preview, "main").color, PreviewColors::default().code);
}

#[test]
fn a_language_nothing_claims_is_drawn_in_the_one_code_colour() {
    let highlighter = OneWord;
    let mut options = options();
    options.highlighter = Some(&highlighter);
    let source = "```klingon\nfn main() {}\n```";
    let preview = render(source, &options);
    properties(&preview, source);
    assert_eq!(style_of(&preview, "fn").color, PreviewColors::default().code);
}

#[test]
fn the_colours_of_a_fence_are_cut_at_its_line_ends() {
    struct Whole;
    impl CodeHighlighter for Whole {
        fn colour(&self, _: &str, code: &str) -> Vec<(Range<usize>, Color)> {
            vec![(0..code.len(), Color::GREEN)]
        }
    }
    let highlighter = Whole;
    let mut options = options();
    options.highlighter = Some(&highlighter);
    let source = "```x\none\ntwo\nthree\n```";
    let preview = render(source, &options);
    properties(&preview, source);
    assert_eq!(style_of(&preview, "two").color, Color::GREEN);
    assert_eq!(style_of(&preview, "three").color, Color::GREEN);
}

// ---------------------------------------------------------------------------------------------
// Properties, on their own
// ---------------------------------------------------------------------------------------------

#[test]
fn an_empty_document_is_one_empty_paragraph() {
    let preview = preview("");
    assert_eq!(preview.text.to_string(), "");
    assert_eq!(preview.paragraphs.len(), 1);
}

#[test]
fn a_document_of_one_of_everything_keeps_the_properties() {
    let source = "\
---
title: Everything
---

# Heading

A paragraph with **bold**, *italic*, ~~struck~~, `code`, a [link](https://example.com) and
a wrapped second line.

## Lists

- a bullet
  - nested
- [x] a ticked box

1. first
2. second

> A quote
> > inside a quote

| Column | Another |
| ------ | ------: |
| a      |       1 |

```rust
fn main() {}
```

![a picture](picture.png)

```mermaid
graph TD
  A --> B
```

A claim[^one].

[^one]: The reason.

---

The end.";
    let preview = preview(source);
    let text = preview.text.to_string();
    for word in ["Heading", "bold", "italic", "struck", "code", "link", "Column", "The end."] {
        assert!(text.contains(word), "{word:?} is missing from {text}");
    }
    assert_eq!(preview.images.len(), 1);
    assert_eq!(preview.diagrams.len(), 1);
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::Code));
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::Table));
    assert!(preview.panels.iter().any(|panel| panel.kind == PanelKind::FrontMatter));
}

/// A source line that is deep inside a nested structure still produces a preview line that names it,
/// which is what the side-by-side scrolling reads.
#[test]
fn the_source_lines_reach_the_end_of_the_document() {
    let source = "# One\n\nprose\n\n- a\n- b\n\n```\ncode\n```\n\nlast";
    let preview = preview(source);
    let last = *preview.source_lines.last().expect("a line");
    assert_eq!(last, source.split('\n').count() - 1);
}

/// Nothing in a document can make the preview panic, however odd it is.
#[test]
fn odd_documents_do_not_bring_anything_down() {
    let odd = [
        "```",
        "```\n",
        "> ",
        ">",
        "- ",
        "|",
        "| |",
        "|---|",
        "| a |\n| --- |",
        "[",
        "![](",
        "[]()",
        "***",
        "*",
        "~~~~~~",
        "#######",
        "    ",
        "\n\n\n",
        "---\n",
        "a\n---\n",
        "\u{1F600} **\u{1F600}** \u{1F600}",
        "| \u{1F600} | b |\n| --- | --- |\n| c | d |",
    ];
    for source in odd {
        preview(source);
    }
}
