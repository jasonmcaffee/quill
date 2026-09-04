//! Packet diagrams: `packet` and `packet-beta`.
//!
//! A grid thirty two bits wide with a labelled field over each range of bits, and the bit numbers
//! along the top. It is the picture at the front of every network protocol specification.
//!
//! Two ways of writing a field, and they mix freely:
//!
//! - `0-15: "Source Port"` names the bits outright;
//! - `+16: "Destination Port"` says how many bits, starting wherever the last field ended.
//!
//! **A field that spans a row boundary is drawn as two rectangles**, one on each row, which is what
//! the picture in a specification does and is the only way a 32-bit grid can show a field that
//! starts at bit 24 and runs for 16.

use super::parts;
use super::scene::{Anchor, Item, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How many bits are drawn on one row.
const PER_ROW: usize = 32;
/// How wide one bit is.
const BIT: f32 = 17.0;
/// How tall one row of the grid is.
const ROW: f32 = 40.0;
/// How much room the bit numbers take above each row.
const NUMBERS: f32 = 16.0;
/// The most bits a diagram may describe, so a typing mistake cannot ask for a mile of grid.
const LIMIT: usize = 32 * 64;

/// One field.
#[derive(Debug, Clone, PartialEq)]
struct Field {
    label: String,
    /// The first bit, counting from zero.
    from: usize,
    /// The last bit, inclusive.
    to: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    fields: Vec<Field>,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    let mut next = 0_usize;
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            diagram.title = Some(source::label(rest));
            continue;
        }
        let Some((range, label)) = line.text.split_once(':') else {
            return Err(Problem::at(
                line,
                "a field looks like `0-15: \"Source Port\"` — a range of bits, a colon, and a name.",
            ));
        };
        let range = range.trim();
        let (from, to) = if let Some(count) = range.strip_prefix('+') {
            let Ok(count) = count.trim().parse::<usize>() else {
                return Err(Problem::at(line, format!("`+{}` is not a number of bits", count.trim())));
            };
            if count == 0 {
                return Err(Problem::at(line, "a field of no bits has nothing to draw"));
            }
            (next, next + count - 1)
        } else if let Some((first, last)) = range.split_once('-') {
            let (Ok(first), Ok(last)) = (first.trim().parse::<usize>(), last.trim().parse::<usize>())
            else {
                return Err(Problem::at(line, format!("`{range}` is not a range of bits")));
            };
            if last < first {
                return Err(Problem::at(line, "a field's last bit comes before its first"));
            }
            (first, last)
        } else {
            let Ok(only) = range.parse::<usize>() else {
                return Err(Problem::at(line, format!("`{range}` is not a bit")));
            };
            (only, only)
        };
        if to >= LIMIT {
            return Err(Problem::at(
                line,
                format!("this field reaches bit {to}, and Unluminate draws {LIMIT} bits at most."),
            ));
        }
        next = to + 1;
        diagram.fields.push(Field { label: source::label(label), from, to });
    }
    Ok(diagram)
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&diagram.title);
    }
    let width = parts::MARGIN * 2.0 + BIT * PER_ROW as f32;
    let top = parts::title(&mut scene, &titled, options, width);
    if diagram.fields.is_empty() {
        parts::finish(&mut scene);
        return scene;
    }
    let rows = diagram.fields.iter().map(|field| field.to).max().unwrap_or(0) / PER_ROW + 1;
    let grid_top = top + parts::MARGIN;

    draw_numbers(&mut scene, rows, grid_top, options);
    draw_fields(&mut scene, diagram, grid_top, options);
    scene.claim(Rect::new(
        0.0,
        0.0,
        width,
        grid_top + (ROW + NUMBERS) * rows as f32,
    ));
    parts::finish(&mut scene);
    scene
}

/// The bit numbers above each row: every eighth one, and the last.
fn draw_numbers(scene: &mut Scene, rows: usize, grid_top: f32, options: &Options) {
    let style = parts::text_style(options, 0.7, false, options.theme.dim);
    let measure = options.style(0.7, false);
    for row in 0..rows {
        let y = grid_top + (ROW + NUMBERS) * row as f32;
        for bit in (0..PER_ROW).step_by(8).chain(std::iter::once(PER_ROW - 1)) {
            let words = (row * PER_ROW + bit).to_string();
            let width = text::width_of(&words, &measure, options.metrics);
            parts::one_line(
                scene,
                &words,
                Point::new(parts::MARGIN + BIT * bit as f32 + BIT / 2.0, y),
                &style,
                Anchor::Middle,
                width,
            );
        }
    }
}

/// Every field, split across rows where it has to be.
fn draw_fields(scene: &mut Scene, diagram: &Diagram, grid_top: f32, options: &Options) {
    let style = options.style(0.78, false);
    for (index, field) in diagram.fields.iter().enumerate() {
        // A field that crosses a row boundary becomes one rectangle a row, which is what the picture
        // in a specification does.
        let mut at = field.from;
        while at <= field.to {
            let row = at / PER_ROW;
            let row_end = (row + 1) * PER_ROW - 1;
            let piece_end = field.to.min(row_end);
            let left = parts::MARGIN + BIT * (at % PER_ROW) as f32;
            let rect = Rect::new(
                left,
                grid_top + (ROW + NUMBERS) * row as f32 + NUMBERS,
                BIT * (piece_end - at + 1) as f32,
                ROW,
            );
            scene.add(Item::Rect {
                rect,
                radius: 2.0,
                fill: Some(options.theme.wash(index, 80)),
                stroke: Some(Stroke::new(options.theme.series(index), parts::LINE)),
            });
            let label = text::measure(&field.label, &style, options.metrics, rect.width - 6.0);
            parts::centred_label(
                scene,
                &label,
                rect,
                &parts::text_style(options, 0.78, false, options.theme.text),
            );
            at = piece_end + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    fn diagram(text: &str) -> Diagram {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn a_range_and_a_single_bit_are_both_read() {
        let diagram = diagram("packet-beta\n 0-15: \"Source Port\"\n 16: \"A flag\"\n");
        assert_eq!(diagram.fields[0], Field { label: "Source Port".to_owned(), from: 0, to: 15 });
        assert_eq!(diagram.fields[1], Field { label: "A flag".to_owned(), from: 16, to: 16 });
    }

    #[test]
    fn the_plus_form_carries_on_from_the_field_before_it() {
        let diagram = diagram("packet-beta\n +1: \"a\"\n +8: \"b\"\n 9-15: \"c\"\n +4: \"d\"\n");
        let ranges: Vec<(usize, usize)> =
            diagram.fields.iter().map(|field| (field.from, field.to)).collect();
        assert_eq!(ranges, vec![(0, 0), (1, 8), (9, 15), (16, 19)], "and mixes with the other form");
    }

    #[test]
    fn a_range_written_backwards_says_so() {
        let problem = check::refused("packet-beta\n 15-0: \"Backwards\"\n", &options());
        assert_eq!(problem.line, Some(2));
        assert!(problem.reason.contains("before its first"));
    }

    #[test]
    fn a_line_with_no_colon_says_what_a_field_looks_like() {
        let problem = check::refused("packet-beta\n 0-15 Source Port\n", &options());
        assert!(problem.reason.contains("Source Port"));
    }

    #[test]
    fn a_field_far_past_the_end_is_refused_rather_than_drawn_as_a_mile_of_grid() {
        let problem = check::refused("packet-beta\n 0-999999: \"Enormous\"\n", &options());
        assert!(problem.reason.contains("at most"));
    }

    #[test]
    fn a_field_that_crosses_a_row_becomes_one_rectangle_a_row() {
        // Bits 24 to 39 straddle the boundary at 32.
        let scene = check::drawn("packet-beta\n 0-23: \"Head\"\n 24-39: \"Straddles\"\n", &options(), &["Head"]);
        let rects = scene.rects();
        assert_eq!(rects.len(), 3, "one for Head and two for the field that straddles");
        assert!(rects[1].bottom() <= rects[2].top() + 0.01, "the second piece is on the row below");
    }

    #[test]
    fn a_packet_diagram_is_drawn_and_keeps_every_property() {
        let text = "packet-beta\n title TCP\n\
            0-15: \"Source Port\"\n 16-31: \"Destination Port\"\n\
            32-63: \"Sequence Number\"\n 64-95: \"Acknowledgment Number\"\n\
            96-99: \"Data Offset\"\n 100-105: \"Reserved\"\n 106: \"URG\"\n 107: \"ACK\"\n";
        check::drawn(
            text,
            &options(),
            &["TCP", "Source Port", "Sequence Number", "URG", "ACK"],
        );
    }

    #[test]
    fn the_bit_numbers_are_drawn_along_the_top_of_each_row() {
        let scene = check::drawn("packet-beta\n 0-63: \"Two rows\"\n", &options(), &["Two rows"]);
        let texts = scene.texts();
        assert!(texts.contains(&"0"), "the first row starts at zero: {texts:?}");
        assert!(texts.contains(&"32"), "the second row starts at thirty two");
        assert!(texts.contains(&"63"), "and it ends at sixty three");
    }
}
