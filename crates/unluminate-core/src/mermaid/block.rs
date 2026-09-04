//! Block diagrams: `block` and `block-beta`.
//!
//! A grid the author controls: `columns N` says how wide it is, blocks fill it left to right, `space`
//! leaves a hole, and `id["label"]:2` spans two columns. Nested `block:id ... end` puts a grid inside
//! one cell.
//!
//! **The author decides where things go**, which is the whole difference between this and a
//! flowchart. So nothing here is laid out by an algorithm: a block goes in the next free cell, and if
//! that is a poor arrangement then the answer is to write a different one. Arrows between blocks are
//! drawn where the two blocks ended up.

use std::collections::HashMap;

use super::parts::{self, Ending, Outline};
use super::scene::{Dash, Item, Point, Rect, Scene, Size, Stroke};
use super::shapes::Shape;
use super::source::{self, Line, Source};
use super::text;
use super::{Options, Problem};

/// How wide one column is.
const CELL: f32 = 130.0;
/// How tall one row is.
const ROW: f32 = 56.0;
/// The gap between two cells.
const GAP: f32 = 10.0;
/// The room a block holding a grid leaves at its top for its own name.
const HEADER: f32 = 26.0;
/// How many columns a grid has when nothing says.
const DEFAULT_COLUMNS: usize = 3;

/// One block.
#[derive(Debug, Clone, PartialEq)]
struct Block {
    id: String,
    label: String,
    shape: Shape,
    /// How many columns it takes.
    span: usize,
    /// The block it is nested inside, if any.
    parent: Option<usize>,
    /// True for `space`, which takes a cell and draws nothing.
    blank: bool,
    /// Its own grid, when it holds other blocks.
    columns: usize,
    children: Vec<usize>,
    /// Filled in when the grid is laid out.
    rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
struct Arrow {
    from: usize,
    to: usize,
    label: String,
    dashed: bool,
    both: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    blocks: Vec<Block>,
    by_id: HashMap<String, usize>,
    arrows: Vec<Arrow>,
    roots: Vec<usize>,
    columns: usize,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram { columns: DEFAULT_COLUMNS, ..Diagram::default() };
    let mut open: Vec<usize> = Vec::new();
    for line in source.statements() {
        if line.starts_with_word("end") {
            open.pop();
            continue;
        }
        if let Some(rest) = line.after_word("columns") {
            let wanted = rest.trim().parse::<usize>().unwrap_or(DEFAULT_COLUMNS).clamp(1, 32);
            match open.last() {
                Some(&parent) => diagram.blocks[parent].columns = wanted,
                None => diagram.columns = wanted,
            }
            continue;
        }
        if ["style", "classDef", "class", "click"].iter().any(|word| line.starts_with_word(word)) {
            continue;
        }
        // `block:group` opens a nested grid whose blocks follow until `end`. The colon is written
        // against the word with no space, so this cannot go through `after_word`, which asks for a
        // whole word and would read `block:group` as a block called `block:group`.
        if let Some(rest) = nested_block(line) {
            let index = block_of(&mut diagram, rest, open.last().copied());
            diagram.blocks[index].columns = DEFAULT_COLUMNS;
            open.push(index);
            continue;
        }
        read_statement(&mut diagram, line, open.last().copied())?;
    }
    Ok(diagram)
}

/// The name of the grid a `block:group` line opens, if the line is one.
///
/// `block` on its own also opens one, unnamed, which Mermaid allows for a group nobody needs to
/// refer to.
fn nested_block(line: &Line) -> Option<&str> {
    let text = line.text.trim();
    if text.len() >= 6 && text[..6].eq_ignore_ascii_case("block:") {
        return Some(text[6..].trim().trim_end_matches(char::is_whitespace));
    }
    if text.eq_ignore_ascii_case("block") {
        return Some("");
    }
    None
}

/// Read one line of a grid: a run of blocks, possibly with arrows between them.
fn read_statement(
    diagram: &mut Diagram,
    line: &Line,
    parent: Option<usize>,
) -> Result<(), Problem> {
    let pieces = split_arrows(&line.text);
    let mut previous: Option<usize> = None;
    for (index, piece) in pieces.parts.iter().enumerate() {
        // Several blocks may sit on one line with only spaces between them, which is how a row of a
        // grid is usually written.
        let mut here = None;
        for word in split_blocks(piece) {
            let word = word.trim();
            if word.is_empty() {
                continue;
            }
            here = Some(read_block(diagram, word, parent)?);
        }
        let Some(here) = here else {
            continue;
        };
        if index > 0 {
            if let Some(from) = previous {
                let arrow = &pieces.arrows[index - 1];
                diagram.arrows.push(Arrow {
                    from,
                    to: here,
                    label: arrow.label.clone(),
                    dashed: arrow.dashed,
                    both: arrow.both,
                });
            }
        }
        previous = Some(here);
    }
    Ok(())
}

/// Split a run of blocks written on one line, keeping bracketed labels together.
fn split_blocks(piece: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut quoted = false;
    for character in piece.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            '[' | '(' | '{' | '<' if !quoted => {
                depth += 1;
                current.push(character);
            }
            ']' | ')' | '}' | '>' if !quoted && depth > 0 => {
                depth -= 1;
                current.push(character);
            }
            c if c.is_whitespace() && depth == 0 && !quoted => {
                if !current.trim().is_empty() {
                    words.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        words.push(current);
    }
    words
}

/// Read one block: `id`, `id["words"]`, `id["words"]:2`, `space`, `space:3`.
fn read_block(
    diagram: &mut Diagram,
    word: &str,
    parent: Option<usize>,
) -> Result<usize, Problem> {
    // A `:N` suffix outside any brackets says how many columns it takes.
    let (body, span) = split_span(word);
    if body.eq_ignore_ascii_case("space") {
        let index = diagram.blocks.len();
        diagram.blocks.push(Block {
            id: format!("space {index}"),
            label: String::new(),
            shape: Shape::Rect,
            span: span.unwrap_or(1),
            parent,
            blank: true,
            columns: DEFAULT_COLUMNS,
            children: Vec::new(),
            rect: Rect::default(),
        });
        attach(diagram, index, parent);
        return Ok(index);
    }
    let (id, label, shape) = super::flowchart::read_block_shape(body);
    let index = block_of(diagram, &id, parent);
    if let Some(label) = label {
        diagram.blocks[index].label = label;
        diagram.blocks[index].shape = shape;
    }
    // Only when one was written. A block named again in an arrow — `editor --> painter` — carries no
    // span, and taking that as "one column" silently threw away the `:3` it was declared with.
    if let Some(span) = span {
        diagram.blocks[index].span = span;
    }
    Ok(index)
}

/// Take a `:N` span off the end of a block, but not one inside its brackets.
///
/// `None` when the block carried no span at all, which is not the same as one column: see
/// [`read_block`].
fn split_span(word: &str) -> (&str, Option<usize>) {
    let mut depth = 0;
    let mut quoted = false;
    for (at, character) in word.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ':' if depth == 0 && !quoted => {
                let span = word[at + 1..].trim().parse::<usize>().unwrap_or(1).clamp(1, 32);
                return (&word[..at], Some(span));
            }
            _ => {}
        }
    }
    (word, None)
}

fn block_of(diagram: &mut Diagram, id: &str, parent: Option<usize>) -> usize {
    let id = id.trim().to_owned();
    if let Some(&known) = diagram.by_id.get(&id) {
        return known;
    }
    let index = diagram.blocks.len();
    diagram.blocks.push(Block {
        id: id.clone(),
        label: id.clone(),
        shape: Shape::Rect,
        span: 1,
        parent,
        blank: false,
        columns: DEFAULT_COLUMNS,
        children: Vec::new(),
        rect: Rect::default(),
    });
    diagram.by_id.insert(id, index);
    attach(diagram, index, parent);
    index
}

fn attach(diagram: &mut Diagram, index: usize, parent: Option<usize>) {
    match parent {
        Some(parent) => diagram.blocks[parent].children.push(index),
        None => diagram.roots.push(index),
    }
}

/// A statement split into the blocks and the arrows between them.
struct Pieces {
    parts: Vec<String>,
    arrows: Vec<ArrowForm>,
}

struct ArrowForm {
    label: String,
    dashed: bool,
    both: bool,
}

/// Find the arrows on a line. Block diagrams use the flowchart's own forms, so this is the simple
/// case of that: `-->`, `---`, `-.->`, `<-->`, and a label in bars after any of them.
fn split_arrows(text: &str) -> Pieces {
    let mut parts = Vec::new();
    let mut arrows = Vec::new();
    let mut at = 0;
    while let Some(found) = next_arrow(text, at) {
        parts.push(text[at..found.0].to_owned());
        arrows.push(found.2);
        at = found.1;
    }
    parts.push(text[at..].to_owned());
    Pieces { parts, arrows }
}

/// The next arrow at or after `from`: where it starts, where it ends, and what it is.
fn next_arrow(text: &str, from: usize) -> Option<(usize, usize, ArrowForm)> {
    let bytes = text.as_bytes();
    let mut at = from;
    let mut depth = 0;
    let mut quoted = false;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => quoted = !quoted,
            b'[' | b'(' | b'{' if !quoted => depth += 1,
            b']' | b')' | b'}' if !quoted => depth -= 1,
            b'-' | b'<' if !quoted && depth == 0 => {
                let both = bytes[at] == b'<';
                let start = at;
                let mut end = at + usize::from(both);
                let mut dashed = false;
                while end < bytes.len() && matches!(bytes[end], b'-' | b'.') {
                    dashed |= bytes[end] == b'.';
                    end += 1;
                }
                if end - at - usize::from(both) < 2 {
                    at += 1;
                    continue;
                }
                if end < bytes.len() && bytes[end] == b'>' {
                    end += 1;
                }
                let (label, after) = read_bar_label(text, end);
                return Some((start, after, ArrowForm { label, dashed, both }));
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// A label written after an arrow in bars.
fn read_bar_label(text: &str, from: usize) -> (String, usize) {
    let rest = &text[from..];
    let trimmed = rest.trim_start();
    let skipped = rest.len() - trimmed.len();
    let Some(inner) = trimmed.strip_prefix('|') else {
        return (String::new(), from);
    };
    let Some(close) = inner.find('|') else {
        return (String::new(), from);
    };
    (source::label(&inner[..close]), from + skipped + 1 + close + 1)
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let mut placed = diagram.blocks.clone();
    let height =
        lay_grid(&mut placed, &diagram.roots, diagram.columns, Point::new(parts::MARGIN, 0.0), CELL);
    let width = parts::MARGIN * 2.0 + CELL * diagram.columns as f32 + GAP * (diagram.columns - 1) as f32;

    let mut scene = Scene::new();
    let top = parts::title(&mut scene, source, options, width);
    for block in placed.iter_mut() {
        block.rect = block.rect.moved(0.0, top + parts::MARGIN);
    }
    draw_blocks(&mut scene, &placed, &diagram.roots, options);
    draw_arrows(&mut scene, diagram, &placed, options);
    scene.claim(Rect::new(0.0, 0.0, width, top + parts::MARGIN + height));
    parts::finish(&mut scene);
    scene
}

/// How tall one block has to be: a row, or enough for the grid inside it.
///
/// Worked out before anything is placed, because a block holding a grid is taller than a row and the
/// row it sits in has to know that before it decides where the next row goes. Doing it the other way
/// round — placing first and growing afterwards — is what let a grown block run over the row beneath
/// it, which the shared "boxes nest or miss" check caught.
fn measure_block(blocks: &[Block], index: usize) -> f32 {
    let children = &blocks[index].children;
    if children.is_empty() {
        return ROW;
    }
    let inner = measure_grid(blocks, children, blocks[index].columns.max(1));
    HEADER + inner + GAP * 2.0
}

/// How many columns a block has to take: what it asked for, or enough for the grid inside it.
///
/// A block holding a grid three columns wide cannot be one column wide itself — its children would
/// each get a third of a cell, and their words would be squeezed into a strip. Mermaid gives a
/// nested block the room its own `columns` asks for, and so does this.
fn columns_for(blocks: &[Block], index: usize) -> usize {
    let declared = blocks[index].span.max(1);
    if blocks[index].children.is_empty() {
        return declared;
    }
    declared.max(blocks[index].columns.max(1))
}

/// How tall a whole grid is, and how tall each of its rows is.
fn measure_grid(blocks: &[Block], nodes: &[usize], columns: usize) -> f32 {
    rows_of(blocks, nodes, columns).iter().map(|row| row.height + GAP).sum::<f32>() - GAP
}

/// One row of a grid: which blocks are in it and how tall it is.
struct Row {
    members: Vec<usize>,
    height: f32,
}

/// Break a list of blocks into rows, filling each row until the next block will not fit.
fn rows_of(blocks: &[Block], nodes: &[usize], columns: usize) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut used = 0;
    for &index in nodes {
        let span = columns_for(blocks, index).min(columns);
        if used + span > columns || rows.is_empty() {
            rows.push(Row { members: Vec::new(), height: 0.0 });
            used = 0;
        }
        let row = rows.last_mut().expect("a row was just pushed");
        row.height = row.height.max(measure_block(blocks, index));
        row.members.push(index);
        used += span;
    }
    rows
}

/// Place a list of blocks into a grid `columns` wide, from `at`. Returns how tall it came to.
///
/// `cell` is how wide one column is. A nested grid gets a smaller one, worked out from the room its
/// parent actually has: a child cell the same width as a top level cell cannot fit inside a one-cell
/// parent, and sticks out of the right of it by exactly the padding.
fn lay_grid(
    blocks: &mut [Block],
    nodes: &[usize],
    columns: usize,
    at: Point,
    cell: f32,
) -> f32 {
    let rows = rows_of(blocks, nodes, columns);
    let mut y = at.y;
    for row in &rows {
        let mut column = 0;
        for &index in &row.members {
            let span = columns_for(blocks, index).min(columns);
            let width = cell * span as f32 + GAP * (span - 1) as f32;
            blocks[index].rect =
                Rect::new(at.x + (cell + GAP) * column as f32, y, width, row.height);
            column += span;
        }
        // The children go in after the row has its final height, so a nested grid is placed inside
        // the rectangle its parent actually ended up with.
        for &index in &row.members {
            let children = blocks[index].children.clone();
            if children.is_empty() {
                continue;
            }
            let rect = blocks[index].rect;
            let inner_columns = blocks[index].columns.max(1);
            let room = rect.width - GAP * 2.0;
            let inner_cell =
                ((room - GAP * (inner_columns - 1) as f32) / inner_columns as f32).max(8.0);
            lay_grid(
                blocks,
                &children,
                inner_columns,
                Point::new(rect.x + GAP, rect.y + HEADER),
                inner_cell,
            );
        }
        y += row.height + GAP;
    }
    (y - at.y - GAP).max(0.0)
}

/// Draw every block, parents before their children so a child is on top.
fn draw_blocks(scene: &mut Scene, blocks: &[Block], nodes: &[usize], options: &Options) {
    for &index in nodes {
        let block = &blocks[index];
        if block.blank {
            continue;
        }
        let theme = &options.theme;
        let has_children = !block.children.is_empty();
        block.shape.draw(
            scene,
            block.rect,
            if has_children { theme.group_fill } else { theme.node_fill },
            Stroke::new(if has_children { theme.group_stroke } else { theme.node_stroke }, parts::LINE),
        );
        let style = options.style(0.95, has_children);
        let label = text::measure(&block.label, &style, options.metrics, block.rect.width - 12.0);
        let at = if has_children {
            // A block with a grid inside it has its name across the top, out of the way.
            Rect::new(block.rect.x, block.rect.y, block.rect.width, parts::PADDING_Y * 2.0 + label.height)
        } else {
            block.rect
        };
        parts::centred_label(
            scene,
            &label,
            at,
            &parts::text_style(options, 0.95, has_children, theme.text),
        );
        draw_blocks(scene, blocks, &block.children, options);
    }
}

/// Draw the arrows between blocks, wherever the two blocks ended up.
fn draw_arrows(scene: &mut Scene, diagram: &Diagram, blocks: &[Block], options: &Options) {
    let theme = &options.theme;
    for arrow in &diagram.arrows {
        let from = blocks[arrow.from].rect;
        let to = blocks[arrow.to].rect;
        if from.width <= 0.0 || to.width <= 0.0 {
            continue;
        }
        let path = vec![
            Outline::Rect(from).border_towards(to.centre()),
            Outline::Rect(to).border_towards(from.centre()),
        ];
        scene.add(Item::Line {
            points: parts::trimmed(&path, 0.0, parts::ending_inset(Ending::Arrow)),
            stroke: Stroke::new(theme.line, parts::LINE),
            dash: if arrow.dashed { parts::DASH } else { Dash::Solid },
        });
        parts::ending(scene, Ending::Arrow, path[1], parts::heading(&path), theme.line, theme.node_fill);
        if arrow.both {
            parts::ending(scene, Ending::Arrow, path[0], parts::tail_heading(&path), theme.line, theme.node_fill);
        }
        if arrow.label.trim().is_empty() {
            continue;
        }
        let style = options.style(0.8, false);
        let label = text::measure_unwrapped(&arrow.label, &style, options.metrics);
        let middle = path[0].towards(path[1], 0.5);
        parts::centred_label(
            scene,
            &label,
            Rect::around(middle, Size::new(label.width + 6.0, label.height)),
            &parts::text_style(options, 0.8, false, theme.dim),
        );
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
    fn columns_and_blocks_are_read() {
        let diagram = diagram("block-beta\n columns 3\n a b c\n");
        assert_eq!(diagram.columns, 3);
        assert_eq!(diagram.roots.len(), 3);
        assert_eq!(diagram.blocks[0].id, "a");
    }

    #[test]
    fn a_block_may_span_several_columns() {
        let diagram = diagram("block-beta\n columns 3\n a[\"Wide\"]:2 b\n");
        assert_eq!(diagram.blocks[0].span, 2);
        assert_eq!(diagram.blocks[0].label, "Wide");
        assert_eq!(diagram.blocks[1].span, 1);
    }

    #[test]
    fn a_colon_inside_a_label_is_not_a_span() {
        let diagram = diagram("block-beta\n a[\"Time: all of it\"]\n");
        assert_eq!(diagram.blocks[0].label, "Time: all of it");
        assert_eq!(diagram.blocks[0].span, 1);
    }

    #[test]
    fn space_takes_a_cell_and_draws_nothing() {
        let diagram = diagram("block-beta\n columns 3\n a space b\n");
        assert_eq!(diagram.roots.len(), 3);
        assert!(diagram.blocks[1].blank);
        let scene = check::drawn("block-beta\n columns 3\n a space b\n", &options(), &["a", "b"]);
        assert_eq!(scene.rects().len(), 2, "the space is a hole, not a box");
    }

    #[test]
    fn a_nested_block_holds_the_blocks_written_inside_it() {
        let text = "block-beta\n columns 2\n a\n block:group\n  x\n  y\n end\n b\n";
        let diagram = diagram(text);
        let group = diagram.by_id["group"];
        assert_eq!(diagram.blocks[group].children.len(), 2);
        assert_eq!(diagram.blocks[diagram.by_id["x"]].parent, Some(group));
        assert_eq!(diagram.blocks[diagram.by_id["b"]].parent, None);
    }

    #[test]
    fn arrows_between_blocks_are_read_with_their_labels() {
        let diagram = diagram("block-beta\n a --> b\n c -.-> d\n e <--> f\n g -->|yes| h\n");
        assert_eq!(diagram.arrows.len(), 4);
        assert!(!diagram.arrows[0].dashed);
        assert!(diagram.arrows[1].dashed);
        assert!(diagram.arrows[2].both);
        assert_eq!(diagram.arrows[3].label, "yes");
    }

    #[test]
    fn a_block_diagram_is_drawn_and_keeps_every_property() {
        let text = "block-beta\n columns 3\n\
            frontend[\"The front end\"]:3\n\
            block:services\n  api[\"API\"]\n  auth[\"Auth\"]\n end\n\
            space\n db[(\"Database\")]\n\
            frontend --> db\n";
        check::drawn(
            text,
            &options(),
            &["The front end", "API", "Auth", "Database"],
        );
    }

    #[test]
    fn a_nested_grid_stays_inside_the_block_that_holds_it() {
        let text = "block-beta\n columns 2\n block:outer\n  x\n  y\n end\n z\n";
        let scene = check::drawn(text, &options(), &["x", "y", "z"]);
        check::boxes_nest_or_miss(&scene.rects());
    }
}
