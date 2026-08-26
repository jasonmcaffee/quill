//! Pipe tables, and the box they are drawn in.
//!
//! A table needs columns of equal width, and Quill's layout engine places one glyph after another
//! with no notion of a column. `tasks/task-1685-markdown-tdd.md` §5 weighs the three ways of giving
//! it one; this is the third. **The table is set in the monospaced font and its rules are drawn with
//! box-drawing characters**, so the columns line up by construction rather than by measurement, and
//! the whole table is ordinary text in the ordinary layout — which is what makes it select, copy,
//! scroll and hit-test with no new code anywhere, and what puts a table a person can paste on the
//! clipboard.
//!
//! It is what the field does: `glamour` behind `glow`, `rich`, `mdcat` and `bat` all draw exactly
//! this. And it is what Quill already does one step down — the horizontal rule has been forty-eight
//! `─` since the preview was written, for the same reason. The layout engine places glyphs, so a
//! line that is not text is a line made of text.
//!
//! The arithmetic is therefore integers: a column is so many characters wide, and every test in this
//! file runs with no fonts at all. The one measurement the whole feature takes is how many
//! characters of the code font fit across the pane, and that is taken once, by the caller.

use unicode_segmentation::UnicodeSegmentation;

use super::blocks::Line;
use super::inline::{self, Kind, References, Span};

/// How a column's cells sit in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Column {
    Left,
    Centre,
    Right,
}

/// A table as it was written: the head, the body, and one alignment a column.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    pub alignments: Vec<Column>,
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The smallest a column may be squeezed to before the table is allowed to overflow instead.
const NARROWEST: usize = 5;

/// True when a table starts at `at`: a row of cells with a delimiter row under it.
pub(crate) fn starts_here(lines: &[Line], at: usize) -> bool {
    read(lines, at).is_some()
}

/// Read a table starting at `at`, giving back the table and the line after it.
pub(crate) fn read(lines: &[Line], at: usize) -> Option<(Table, usize)> {
    let header = lines.get(at)?;
    if super::blocks::indent_of(&header.text) >= 4 || !header.text.contains('|') {
        return None;
    }
    let head = cells(&header.text);
    let delimiter = lines.get(at + 1)?;
    let alignments = delimiter_row(&delimiter.text)?;
    if alignments.len() != head.len() {
        return None;
    }

    let mut rows = Vec::new();
    let mut next = at + 2;
    while next < lines.len() {
        let text = &lines[next].text;
        if super::blocks::is_blank(text) || !text.contains('|') {
            break;
        }
        let mut row = cells(text);
        // GFM: a short row is padded and a long one is cut, so a table never comes apart because
        // somebody miscounted the pipes.
        row.resize(head.len(), String::new());
        rows.push(row);
        next += 1;
    }
    Some((Table { alignments, head, rows }, next))
}

/// Split a row into its cells, on pipes that are not escaped.
fn cells(text: &str) -> Vec<String> {
    let body = text.trim();
    let body = body.strip_prefix('|').unwrap_or(body);
    let body = strip_last_pipe(body);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in body.chars() {
        if escaped {
            // A backslash in front of anything but a pipe is left alone: the inline parser reads it.
            if character != '|' {
                cell.push('\\');
            }
            cell.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '|' => out.push(std::mem::take(&mut cell)),
            _ => cell.push(character),
        }
    }
    if escaped {
        cell.push('\\');
    }
    out.push(cell);
    out.into_iter().map(|cell| cell.trim().to_owned()).collect()
}

/// Take a trailing pipe off, unless it was escaped.
fn strip_last_pipe(body: &str) -> &str {
    if !body.ends_with('|') {
        return body;
    }
    let before = &body[..body.len() - 1];
    let slashes = before.len() - before.trim_end_matches('\\').len();
    if slashes % 2 == 1 {
        return body;
    }
    before
}

/// Read the row of dashes and colons that says where the columns are and how they line up.
fn delimiter_row(text: &str) -> Option<Vec<Column>> {
    if super::blocks::indent_of(text) >= 4 || !text.contains('-') {
        return None;
    }
    let parts = cells(text);
    let mut out = Vec::with_capacity(parts.len());
    for part in &parts {
        let body = part.trim();
        let left = body.starts_with(':');
        let right = body.ends_with(':') && body.len() > 1;
        let dashes = body.trim_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
            return None;
        }
        out.push(match (left, right) {
            (true, true) => Column::Centre,
            (false, true) => Column::Right,
            _ => Column::Left,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// A table drawn out: one entry a preview line, each a list of spans to push.
#[derive(Debug, Clone)]
pub(crate) struct Drawn {
    pub lines: Vec<Vec<Span>>,
}

/// Draw `table` into at most `available` characters of the monospaced font.
///
/// Columns are shrunk widest first when the table is too wide, down to [`NARROWEST`], and a cell too
/// long for its column is wrapped at word boundaries. A row is then as many preview lines as its
/// tallest cell, each of them one contiguous run of text — which is the property that keeps hit
/// testing and selection working without the layout engine learning anything about tables.
pub(crate) fn draw(table: &Table, references: &References, available: usize) -> Drawn {
    let head: Vec<Vec<Span>> =
        table.head.iter().map(|cell| bold(inline::parse(cell, references))).collect();
    let rows: Vec<Vec<Vec<Span>>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|cell| inline::parse(cell, references)).collect())
        .collect();

    let widths = fit(&head, &rows, available);
    let mut lines = Vec::new();
    lines.push(rule(&widths, '\u{250C}', '\u{252C}', '\u{2510}'));
    lines.extend(row_lines(&head, &widths, &table.alignments));
    lines.push(rule(&widths, '\u{251C}', '\u{253C}', '\u{2524}'));
    for row in &rows {
        lines.extend(row_lines(row, &widths, &table.alignments));
    }
    lines.push(rule(&widths, '\u{2514}', '\u{2534}', '\u{2518}'));
    Drawn { lines }
}

/// The head is bold, which is what makes the grid recede and the data come forward.
fn bold(spans: Vec<Span>) -> Vec<Span> {
    spans.into_iter().map(|span| Span { bold: true, ..span }).collect()
}

/// Work out how wide each column is drawn.
fn fit(head: &[Vec<Span>], rows: &[Vec<Vec<Span>>], available: usize) -> Vec<usize> {
    let count = head.len();
    let mut widths: Vec<usize> = head.iter().map(|cell| width_of(cell).max(1)).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate().take(count) {
            widths[index] = widths[index].max(width_of(cell));
        }
    }
    // Every column costs its width plus a space either side, and there is one more rule than there
    // are columns.
    let furniture = count * 3 + 1;
    let mut total: usize = widths.iter().sum::<usize>() + furniture;
    while total > available {
        let widest = widths.iter().copied().max().unwrap_or(0);
        if widest <= NARROWEST {
            break;
        }
        let Some(index) = widths.iter().position(|width| *width == widest) else { break };
        widths[index] -= 1;
        total -= 1;
    }
    widths
}

/// How many characters wide a cell's text is. A hard break inside a cell is a space, because a cell
/// is one run of text however it was written.
fn width_of(cell: &[Span]) -> usize {
    cell.iter()
        .map(|span| match span.kind {
            Kind::Break => 1,
            _ => span.text.graphemes(true).count(),
        })
        .sum()
}

/// One horizontal rule of the box, with the corner pieces it is drawn with.
fn rule(widths: &[usize], left: char, middle: char, right: char) -> Vec<Span> {
    let mut text = String::new();
    text.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            text.push(middle);
        }
        text.push_str(&"\u{2500}".repeat(width + 2));
    }
    text.push(right);
    vec![Span { text, bold: false, italic: false, strike: false, kind: Kind::Quiet }]
}

/// The lines one row of cells is drawn on: as many as its tallest cell needs.
fn row_lines(row: &[Vec<Span>], widths: &[usize], alignments: &[Column]) -> Vec<Vec<Span>> {
    let wrapped: Vec<Vec<Vec<Span>>> = widths
        .iter()
        .enumerate()
        .map(|(index, width)| wrap(row.get(index).map(Vec::as_slice).unwrap_or(&[]), *width))
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let bar = |text: &str| Span {
        text: text.to_owned(),
        bold: false,
        italic: false,
        strike: false,
        kind: Kind::Quiet,
    };

    let mut lines = Vec::with_capacity(height);
    for depth in 0..height {
        let mut line = vec![bar("\u{2502} ")];
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                line.push(bar(" \u{2502} "));
            }
            let empty = Vec::new();
            let piece = wrapped[index].get(depth).unwrap_or(&empty);
            let used = width_of(piece);
            let alignment = alignments.get(index).copied().unwrap_or(Column::Left);
            let room = width.saturating_sub(used);
            let (before, after) = match alignment {
                Column::Left => (0, room),
                Column::Right => (room, 0),
                Column::Centre => (room / 2, room - room / 2),
            };
            if before > 0 {
                line.push(bar(&" ".repeat(before)));
            }
            line.extend(piece.iter().cloned());
            if after > 0 {
                line.push(bar(&" ".repeat(after)));
            }
        }
        line.push(bar(" \u{2502}"));
        lines.push(line);
    }
    lines
}

/// Break a cell's spans into lines of at most `width` characters, at word boundaries where there is
/// one and in the middle of a word where there is not.
fn wrap(cell: &[Span], width: usize) -> Vec<Vec<Span>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    // A cell is one run of text: a hard break inside it becomes a space, because there is nowhere
    // for a second line of a cell to go except the wrapping this function already does.
    let flat: Vec<Span> = cell
        .iter()
        .map(|span| match span.kind {
            Kind::Break => Span { text: " ".to_owned(), kind: Kind::Text, ..span.clone() },
            _ => span.clone(),
        })
        .collect();

    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut line: Vec<Span> = Vec::new();
    let mut used = 0;
    for span in &flat {
        for word in split_keeping_spaces(&span.text) {
            let length = word.graphemes(true).count();
            if used + length > width && used > 0 {
                trim_end(&mut line);
                lines.push(std::mem::take(&mut line));
                used = 0;
                if word.trim().is_empty() {
                    continue;
                }
            }
            // A single word longer than the column is cut, because leaving it whole would push the
            // rules of the box out of line and a broken word is easier to read than a broken table.
            let mut word = word;
            while word.graphemes(true).count() > width {
                let cut: String = word.graphemes(true).take(width - used).collect();
                push_word(&mut line, span, &cut);
                trim_end(&mut line);
                lines.push(std::mem::take(&mut line));
                used = 0;
                word = &word[cut.len()..];
            }
            let length = word.graphemes(true).count();
            if length == 0 {
                continue;
            }
            push_word(&mut line, span, word);
            used += length;
        }
    }
    trim_end(&mut line);
    lines.push(line);
    lines
}

/// Add a word to the line being built, folding it into the last span when they look the same.
fn push_word(line: &mut Vec<Span>, span: &Span, word: &str) {
    match line.last_mut() {
        Some(last)
            if last.bold == span.bold
                && last.italic == span.italic
                && last.strike == span.strike
                && last.kind == span.kind =>
        {
            last.text.push_str(word)
        }
        _ => line.push(Span { text: word.to_owned(), ..span.clone() }),
    }
}

/// Take the spaces off the end of a line, so a wrap does not leave a cell one character too wide.
fn trim_end(line: &mut Vec<Span>) {
    while let Some(last) = line.last_mut() {
        while last.text.ends_with(' ') {
            last.text.pop();
        }
        if last.text.is_empty() {
            line.pop();
        } else {
            break;
        }
    }
}

/// Split text into words, keeping the space that followed each so a wrap can throw it away.
fn split_keeping_spaces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (at, character) in text.char_indices() {
        let space = character == ' ';
        if at > start && space != in_space {
            out.push(&text[start..at]);
            start = at;
        }
        in_space = space;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<Line> {
        source
            .split('\n')
            .enumerate()
            .map(|(number, text)| Line { number, text: text.to_owned() })
            .collect()
    }

    fn drawn(source: &str, available: usize) -> Vec<String> {
        let lines = lines(source);
        let (table, _) = read(&lines, 0).expect("a table");
        draw(&table, &References::default(), available)
            .lines
            .iter()
            .map(|line| line.iter().map(|span| span.text.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn a_row_of_cells_with_dashes_under_it_is_a_table() {
        let source = "| a | b |\n| --- | --- |\n| 1 | 2 |";
        let (table, used) = read(&lines(source), 0).expect("a table");
        assert_eq!(table.head, ["a", "b"]);
        assert_eq!(table.rows, [["1", "2"]]);
        assert_eq!(used, 3);
    }

    #[test]
    fn the_colons_say_how_a_column_lines_up() {
        let source = "| a | b | c |\n| :-- | :-: | --: |\n| 1 | 2 | 3 |";
        let (table, _) = read(&lines(source), 0).expect("a table");
        assert_eq!(table.alignments, [Column::Left, Column::Centre, Column::Right]);
    }

    #[test]
    fn the_outer_pipes_are_optional() {
        let source = "a | b\n--- | ---\n1 | 2";
        let (table, _) = read(&lines(source), 0).expect("a table");
        assert_eq!(table.head, ["a", "b"]);
        assert_eq!(table.rows, [["1", "2"]]);
    }

    #[test]
    fn a_row_with_the_wrong_number_of_cells_is_padded_or_cut() {
        let source = "| a | b |\n| --- | --- |\n| 1 |\n| 1 | 2 | 3 |";
        let (table, _) = read(&lines(source), 0).expect("a table");
        assert_eq!(table.rows[0], ["1", ""]);
        assert_eq!(table.rows[1], ["1", "2"]);
    }

    #[test]
    fn an_escaped_pipe_stays_inside_its_cell() {
        let source = "| a | b |\n| --- | --- |\n| one \\| two | three |";
        let (table, _) = read(&lines(source), 0).expect("a table");
        assert_eq!(table.rows[0], ["one | two", "three"]);
    }

    #[test]
    fn a_head_and_a_delimiter_of_different_lengths_is_not_a_table() {
        assert!(read(&lines("| a | b |\n| --- |\n"), 0).is_none());
        assert!(read(&lines("| a | b |\nnot a delimiter\n"), 0).is_none());
    }

    #[test]
    fn the_table_is_drawn_in_a_box() {
        let out = drawn("| Crate | Lines |\n| --- | ---: |\n| core | 9132 |", 80);
        assert_eq!(out[0], "\u{250C}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{252C}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}");
        assert_eq!(out[1], "\u{2502} Crate \u{2502} Lines \u{2502}");
        assert_eq!(out[3], "\u{2502} core  \u{2502}  9132 \u{2502}");
        assert_eq!(out.last().unwrap().chars().next(), Some('\u{2514}'));
    }

    #[test]
    fn every_drawn_line_is_the_same_width() {
        let out = drawn("| a | bbbb |\n| --- | --- |\n| cc | d |\n| e | ffff |", 80);
        let widths: Vec<usize> = out.iter().map(|line| line.chars().count()).collect();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "{widths:?} from {out:?}");
    }

    #[test]
    fn a_centred_column_is_centred() {
        let out = drawn("| head |\n| :--: |\n| ab |", 40);
        assert_eq!(out[3], "\u{2502}  ab  \u{2502}");
    }

    #[test]
    fn a_table_too_wide_for_the_pane_is_squeezed_and_wrapped() {
        let source =
            "| one | two |\n| --- | --- |\n| a much longer cell than fits | another long one |";
        let out = drawn(source, 30);
        for line in &out {
            assert!(line.chars().count() <= 30, "{line:?} is wider than the pane");
        }
        // The long cell became more than one line, so the row is more than one line tall.
        assert!(out.len() > 5, "{out:?}");
    }

    #[test]
    fn a_word_longer_than_its_column_is_cut_rather_than_breaking_the_box() {
        let out = drawn("| a |\n| --- |\n| supercalifragilistic |", 12);
        for line in &out {
            assert!(line.chars().count() <= 12, "{line:?}");
        }
    }

    #[test]
    fn the_marks_inside_a_cell_are_read() {
        let lines = lines("| a |\n| --- |\n| **bold** |");
        let (table, _) = read(&lines, 0).expect("a table");
        let drawn = draw(&table, &References::default(), 40);
        let row = &drawn.lines[3];
        assert!(row.iter().any(|span| span.text == "bold" && span.bold), "{row:?}");
        let text: String = row.iter().map(|span| span.text.as_str()).collect();
        assert!(!text.contains('*'), "the marks are not shown: {text:?}");
    }
}
