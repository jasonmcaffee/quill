//! Keeping the Markdown source and its preview at the same place in the file.
//!
//! `task-1673` asks that scrolling either half of the side by side view scroll the other. A scroll
//! position is a number of points down a page, and the two pages are nothing like the same height:
//! a heading is one line of source and three times a line's height on the page, a fence's backticks
//! are two lines of source and nothing at all on the page, and a picture is one line of source and
//! four hundred points of page. So the two numbers cannot be scaled into one another, and a
//! proportion of the total height — the obvious answer, and the one a browser preview usually gives
//! — drifts further from the truth the further down a document with any structure in it you go.
//!
//! What both halves *do* agree about is the text. [`unluminous_core::Preview::source_lines`] says which
//! line of the source each line of the preview came from, so a place in one page can be turned into
//! a place in the other by way of a line number: which paragraph is at this height and how far down
//! it the point sits, which line of the source that is, and where that line ended up on the other
//! page at the same fraction. The fraction is what makes it smooth rather than stepped — without it
//! the other half would jump a paragraph at a time.
//!
//! Both functions are pure and take laid out pages, so all of this is tested with no window.

use crate::layout::Layout;

/// Where the preview should be scrolled to so that it shows what the source at `y` shows.
///
/// `source_lines` is [`crate::Preview::source_lines`], one entry per preview paragraph.
pub fn preview_y_for_source_y(
    source: &Layout,
    preview: &Layout,
    source_lines: &[usize],
    y: f32,
) -> f32 {
    let (line, fraction) = source.paragraph_at_y(y.max(0.0));
    let paragraph = preview_paragraph_for_line(source_lines, line);
    at_fraction(preview, paragraph, fraction)
}

/// The other way: where the source should be scrolled to so that it shows what the preview at `y`
/// shows.
pub fn source_y_for_preview_y(
    source: &Layout,
    preview: &Layout,
    source_lines: &[usize],
    y: f32,
) -> f32 {
    let (paragraph, fraction) = preview.paragraph_at_y(y.max(0.0));
    let line = source_lines.get(paragraph).copied().unwrap_or(0);
    at_fraction(source, line, fraction)
}

/// Which preview paragraph stands for a line of the source: the **first** one that came from that
/// line, or the last one from above it when nothing came from it at all.
///
/// A binary search, because `source_lines` never goes backwards — the source is read from the top
/// down. Two things are decided here and both matter.
///
/// *The first, not the last.* Several preview lines may name one source line: a heading and the
/// blank that separates it from what follows both belong to the heading's line, and a table row too
/// wide for the pane is several lines all naming the row. Scrolling to a source line means scrolling
/// to where that line's own text begins, which is the first of them.
///
/// *Or the last from above.* A line that produces no preview paragraph of its own — a fence's
/// backticks, the interior of a Mermaid block — belongs with the paragraph before it, which is where
/// a reader scrolling through them is looking.
fn preview_paragraph_for_line(source_lines: &[usize], line: usize) -> usize {
    let at = source_lines.partition_point(|from| *from < line);
    if source_lines.get(at) == Some(&line) {
        return at;
    }
    at.saturating_sub(1)
}

/// Where a paragraph's `fraction` falls on a page. Nothing to say gives the top of the page, which is
/// the only honest answer and is where the view is anyway.
fn at_fraction(page: &Layout, paragraph: usize, fraction: f32) -> f32 {
    match page.paragraph_band(paragraph) {
        Some((top, height)) => top + height * fraction,
        None => page.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown;
    use crate::{layout, CharStyle, Color, FixedMetrics, PreviewColors};

    /// The preview of a source, laid out through the fixed width stub so the numbers are arithmetic
    /// a reader can check and are the same on every machine.
    fn pages(source: &str) -> (Layout, Layout, Vec<usize>) {
        let base = CharStyle { size: 10.0, ..CharStyle::default() };
        let colors = PreviewColors {
            text: Color::rgb(0xFF, 0xFF, 0xFF),
            code: Color::rgb(0x7E, 0xD3, 0x9B),
            link: Color::rgb(0x48, 0x9F, 0xF8),
            quiet: Color::rgb(0x8B, 0x93, 0xA3),
            rule: Color::rgb(0x2A, 0x30, 0x3B),
        };
        let preview = markdown::render(source, &markdown::Options::new(base, colors, None));
        let metrics = FixedMetrics::default();
        let document = crate::Document::from_text(source);
        let source_page = layout(
            document.text(),
            document.chars(),
            document.paragraphs(),
            &metrics,
            600.0,
        );
        let preview_page =
            layout(&preview.text, &preview.chars, &preview.paragraphs, &metrics, 600.0);
        (source_page, preview_page, preview.source_lines)
    }

    const DOCUMENT: &str = "# Title\n\nfirst\n\n## Second\n\nsecond body\n\n```\ncode one\ncode two\n```\n\nlast line\n";

    /// **The top of one page is the top of the other.**
    #[test]
    fn the_top_of_the_source_is_the_top_of_the_preview() {
        let (source, preview, map) = pages(DOCUMENT);
        assert_eq!(preview_y_for_source_y(&source, &preview, &map, 0.0), 0.0);
        assert_eq!(source_y_for_preview_y(&source, &preview, &map, 0.0), 0.0);
    }

    /// **A heading scrolled to the top of the source is at the top of the preview.**
    ///
    /// This is the whole of the ask. `## Second` is line four of the source; scrolling the source so
    /// that line is at the top must put the same heading at the top of the preview, whatever the two
    /// pages weigh above it — and they do not weigh the same, because the preview sets a heading
    /// larger and throws the hash marks away.
    #[test]
    fn a_heading_at_the_top_of_the_source_is_at_the_top_of_the_preview() {
        let (source, preview, map) = pages(DOCUMENT);
        let (top, _) = source.paragraph_band(4).expect("the second heading");
        let there = preview_y_for_source_y(&source, &preview, &map, top);
        let paragraph = map.iter().position(|from| *from == 4).expect("it is in the preview");
        let (want, _) = preview.paragraph_band(paragraph).expect("laid out");
        assert!((there - want).abs() < 0.01, "expected {want}, got {there}");
        // And the two pages really are different heights above it, or this would prove nothing.
        assert!((top - want).abs() > 1.0, "the two pages agree by accident: {top} and {want}");
    }

    /// **Both directions agree.** Going across and back lands on the same paragraph.
    #[test]
    fn mapping_across_and_back_keeps_the_paragraph() {
        let (source, preview, map) = pages(DOCUMENT);
        for paragraph in 0..source.paragraph_count() {
            let Some((top, _)) = source.paragraph_band(paragraph) else { continue };
            let there = preview_y_for_source_y(&source, &preview, &map, top);
            let back = source_y_for_preview_y(&source, &preview, &map, there);
            let (landed, _) = source.paragraph_at_y(back);
            assert!(
                landed <= paragraph,
                "paragraph {paragraph} came back as {landed}, which is further down the file"
            );
        }
    }

    /// **A fence's backticks belong to the paragraph before them.** They produce no preview line, so
    /// the source at them maps to the last thing that was drawn.
    #[test]
    fn a_line_with_no_preview_paragraph_maps_to_the_one_before_it() {
        let (_, _, map) = pages(DOCUMENT);
        // Line 8 is the opening backticks and line 9 is the first line of code.
        assert_eq!(map.iter().filter(|from| **from == 8).count(), 0, "backticks draw nothing");
        assert_eq!(preview_paragraph_for_line(&map, 8), preview_paragraph_for_line(&map, 7));
    }

    /// **The map never goes backwards**, which is what makes the search a binary one.
    #[test]
    fn the_map_never_goes_backwards() {
        let (_, _, map) = pages(DOCUMENT);
        assert!(map.windows(2).all(|pair| pair[0] <= pair[1]), "{map:?}");
    }

    /// **A whole Mermaid fence is named after the line it opened on**, so scrolling to the fence
    /// scrolls to the diagram.
    #[test]
    fn a_mermaid_fence_is_named_after_the_line_it_opened_on() {
        let source = "words\n\n```mermaid\ngraph TD\n  a --> b\n```\n\nafter\n";
        let (_, _, map) = pages(source);
        assert!(map.contains(&2), "the fence opens on line 2: {map:?}");
        assert!(!map.contains(&5), "and not on the line that closed it: {map:?}");
    }

    /// A document with nothing in it has an answer rather than a panic, and the answer stays inside
    /// the page — a point below the only paragraph is the bottom of it, and the caller clamps that
    /// against what there is to scroll as it does with every other scroll position.
    #[test]
    fn an_empty_document_maps_inside_the_page() {
        let (source, preview, map) = pages("");
        assert_eq!(preview_y_for_source_y(&source, &preview, &map, 0.0), 0.0);
        let back = source_y_for_preview_y(&source, &preview, &map, 40.0);
        assert!((0.0..=source.height).contains(&back), "{back} is outside a {} page", source.height);
    }
}
