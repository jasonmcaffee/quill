//! Kanban boards: `kanban`.
//!
//! A column for each list and a card for each item, with the assignee, the ticket and the priority
//! shown on the card. Indentation says which column a card is in, exactly as it does in a mindmap.
//!
//! **Every column is the same width and the columns are as tall as the longest one**, which is what
//! a board looks like and what makes two columns comparable at a glance. A column with one card in
//! it is not drawn shorter than the one beside it with six.

use super::parts;
use super::scene::{Anchor, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How wide one column is.
const COLUMN: f32 = 190.0;
/// The gap between two columns.
const COLUMN_GAP: f32 = 14.0;
/// How much room a column's own name takes at the top of it.
const HEADER: f32 = 34.0;
/// The gap between two cards.
const CARD_GAP: f32 = 8.0;

/// One card.
#[derive(Debug, Clone, PartialEq, Default)]
struct Card {
    label: String,
    assigned: String,
    ticket: String,
    priority: String,
}

/// One column of the board.
#[derive(Debug, Clone, PartialEq)]
struct Column {
    label: String,
    cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Board {
    columns: Vec<Column>,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let board = read(source)?;
    Ok(draw(&board, source, options))
}

fn read(source: &Source) -> Result<Board, Problem> {
    let mut board = Board::default();
    // The indent a column sits at, which is whatever the first line used.
    let mut column_indent: Option<usize> = None;
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            board.title = Some(source::label(rest));
            continue;
        }
        let (head, metadata) = split_metadata(&line.text);
        // A column and a card are both written as a flowchart node would be, so they are read
        // through the same function rather than through a second copy of the bracket table.
        let (id, shown, _) = super::flowchart::read_block_shape(head);
        let label = shown.unwrap_or(id);
        if label.is_empty() {
            continue;
        }
        let indent = line.indent;
        match column_indent {
            // The first line sets what a column's indent is; everything further in is a card.
            None => {
                column_indent = Some(indent);
                board.columns.push(Column { label, cards: Vec::new() });
            }
            Some(known) if indent <= known => {
                board.columns.push(Column { label, cards: Vec::new() });
            }
            Some(_) => {
                let Some(column) = board.columns.last_mut() else {
                    return Err(Problem::at(line, "this card has no column above it to belong to"));
                };
                column.cards.push(read_card(label, metadata));
            }
        }
    }
    Ok(board)
}

/// Split a line into what is before its `@{ ... }` block and what is inside it.
fn split_metadata(text: &str) -> (&str, &str) {
    match (text.find("@{"), text.rfind('}')) {
        (Some(open), Some(close)) if close > open => (&text[..open], &text[open + 2..close]),
        _ => (text, ""),
    }
}

/// Read the three keys Mermaid gives a card.
fn read_card(label: String, metadata: &str) -> Card {
    let mut card = Card { label, ..Card::default() };
    for piece in source::split_outside_quotes(metadata, ',') {
        let Some((key, value)) = piece.split_once(':') else {
            continue;
        };
        let value = source::label(value);
        match key.trim().to_ascii_lowercase().as_str() {
            "assigned" => card.assigned = value,
            "ticket" => card.ticket = value,
            "priority" => card.priority = value,
            _ => {}
        }
    }
    card
}

/// One card, measured.
struct Measured {
    label: Label,
    detail: Option<Label>,
    height: f32,
}

fn draw(board: &Board, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&board.title);
    }
    if board.columns.is_empty() {
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let cards: Vec<Vec<Measured>> = board
        .columns
        .iter()
        .map(|column| column.cards.iter().map(|card| measure(card, options)).collect())
        .collect();

    let width = parts::MARGIN * 2.0
        + COLUMN * board.columns.len() as f32
        + COLUMN_GAP * (board.columns.len() - 1) as f32;
    let top = parts::title(&mut scene, &titled, options, width);
    // The tallest column decides how tall every column is drawn, so the board reads as a board.
    let tallest = cards
        .iter()
        .map(|column| {
            column.iter().map(|card| card.height + CARD_GAP).sum::<f32>() + HEADER + CARD_GAP
        })
        .fold(HEADER + CARD_GAP * 2.0, f32::max);

    for (index, column) in board.columns.iter().enumerate() {
        let left = parts::MARGIN + (COLUMN + COLUMN_GAP) * index as f32;
        let frame = Rect::new(left, top + parts::MARGIN, COLUMN, tallest);
        scene.add(Item::Rect {
            rect: frame,
            radius: parts::CORNER,
            fill: Some(options.theme.group_fill),
            stroke: Some(Stroke::new(options.theme.group_stroke, parts::LINE)),
        });
        let title_width = text::width_of(&column.label, &options.style(1.0, true), options.metrics);
        parts::one_line(
            &mut scene,
            &column.label,
            Point::new(frame.centre().x, frame.top() + 8.0),
            &parts::text_style(options, 1.0, true, options.theme.text),
            Anchor::Middle,
            title_width,
        );
        let mut y = frame.top() + HEADER + CARD_GAP;
        for (at, card) in column.cards.iter().enumerate() {
            let rect = Rect::new(left + CARD_GAP, y, COLUMN - CARD_GAP * 2.0, cards[index][at].height);
            draw_card(&mut scene, card, &cards[index][at], rect, index, options);
            y = rect.bottom() + CARD_GAP;
        }
    }
    scene.claim(Rect::new(0.0, 0.0, width, top + parts::MARGIN + tallest));
    parts::finish(&mut scene);
    scene
}

/// How much room one card needs.
fn measure(card: &Card, options: &Options) -> Measured {
    let label = text::measure(
        &card.label,
        &options.style(0.9, false),
        options.metrics,
        COLUMN - CARD_GAP * 4.0,
    );
    let words = detail_words(card);
    let detail = (!words.is_empty()).then(|| {
        text::measure(&words, &options.style(0.75, false), options.metrics, COLUMN - CARD_GAP * 4.0)
    });
    let height = label.height
        + detail.as_ref().map_or(0.0, |label| label.height + 2.0)
        + parts::PADDING_Y * 2.0;
    Measured { label, detail, height }
}

/// The line under a card's own words: who has it, its ticket, and how urgent it is.
fn detail_words(card: &Card) -> String {
    [card.ticket.as_str(), card.assigned.as_str(), card.priority.as_str()]
        .iter()
        .filter(|piece| !piece.trim().is_empty())
        .copied()
        .collect::<Vec<&str>>()
        .join("  ·  ")
}

/// Draw one card. A high priority one is edged in the accent colour so it is picked out at a glance.
fn draw_card(
    scene: &mut Scene,
    card: &Card,
    measured: &Measured,
    rect: Rect,
    column: usize,
    options: &Options,
) {
    let urgent = card.priority.to_ascii_lowercase().contains("high");
    scene.add(Item::Rect {
        rect,
        radius: parts::CORNER,
        fill: Some(Paint::solid(options.theme.node_fill.color)),
        stroke: Some(Stroke::new(
            if urgent { options.theme.accent } else { options.theme.series(column) },
            if urgent { parts::THICK * 0.7 } else { parts::LINE },
        )),
    });
    parts::label_at(
        scene,
        &measured.label,
        Point::new(rect.left() + CARD_GAP, rect.top() + parts::PADDING_Y),
        &parts::text_style(options, 0.9, false, options.theme.text),
        Anchor::Start,
    );
    let Some(detail) = &measured.detail else {
        return;
    };
    parts::label_at(
        scene,
        detail,
        Point::new(
            rect.left() + CARD_GAP,
            rect.top() + parts::PADDING_Y + measured.label.height + 2.0,
        ),
        &parts::text_style(options, 0.75, false, options.theme.dim),
        Anchor::Start,
    );
}

#[cfg(test)]
mod tests {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    fn board(text: &str) -> Board {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn indentation_tells_a_column_from_a_card() {
        let text = "kanban\n todo[Todo]\n  a[Write it]\n  b[Test it]\n doing[Doing]\n  c[Ship it]\n";
        let board = board(text);
        assert_eq!(board.columns.len(), 2);
        assert_eq!(board.columns[0].label, "Todo");
        assert_eq!(board.columns[0].cards.len(), 2);
        assert_eq!(board.columns[0].cards[0].label, "Write it");
        assert_eq!(board.columns[1].label, "Doing");
        assert_eq!(board.columns[1].cards.len(), 1);
    }

    #[test]
    fn the_metadata_block_is_read_for_its_three_keys() {
        let text = "kanban\n todo[Todo]\n  a[Fix it]@{ assigned: \"Jason\", ticket: \"UNLUMINOUS-12\", priority: \"High\" }\n";
        let board = board(text);
        let card = &board.columns[0].cards[0];
        assert_eq!(card.label, "Fix it");
        assert_eq!(card.assigned, "Jason");
        assert_eq!(card.ticket, "UNLUMINOUS-12");
        assert_eq!(card.priority, "High");
    }

    #[test]
    fn a_card_with_no_metadata_is_still_a_card() {
        let board = board("kanban\n todo[Todo]\n  a[Plain]\n");
        assert_eq!(board.columns[0].cards[0].label, "Plain");
        assert!(board.columns[0].cards[0].assigned.is_empty());
    }

    #[test]
    fn a_column_with_no_brackets_uses_its_own_name() {
        let board = board("kanban\n Todo\n  Write it\n");
        assert_eq!(board.columns[0].label, "Todo");
        assert_eq!(board.columns[0].cards[0].label, "Write it");
    }

    #[test]
    fn a_kanban_board_is_drawn_and_keeps_every_property() {
        let text = "kanban\n\
            todo[Todo]\n  t1[Write the parser]@{ assigned: \"Jason\", ticket: \"UNLUMINOUS-1\", priority: \"High\" }\n  t2[Draw the shapes]\n\
            doing[In progress]\n  d1[Lay it out]@{ assigned: \"Jason\" }\n\
            done[Done]\n  x1[Read the syntax]\n  x2[Write the TDD]\n  x3[Choose the approach]\n";
        check::drawn(
            text,
            &options(),
            &["Todo", "In progress", "Done", "Write the parser", "UNLUMINOUS-1", "Lay it out", "Write the TDD"],
        );
    }

    #[test]
    fn every_column_is_drawn_the_same_height_however_many_cards_it_holds() {
        // A board where one column is shorter than the one beside it does not read as a board.
        let text = "kanban\n a[One]\n  x[card]\n b[Many]\n  y1[card]\n  y2[card]\n  y3[card]\n";
        let scene = check::drawn(text, &options(), &["One", "Many"]);
        let columns: Vec<Rect> = scene
            .rects()
            .into_iter()
            .filter(|rect| (rect.width - COLUMN).abs() < 0.01)
            .collect();
        assert_eq!(columns.len(), 2);
        assert!((columns[0].height - columns[1].height).abs() < 0.01);
        assert!(columns[0].height > 100.0, "and tall enough for the longest column");
    }
}
