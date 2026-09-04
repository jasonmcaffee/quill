//! The snapshot the painter reads.
//!
//! Plain data with no locks in it and nothing borrowed from the emulator. A frame takes one of these while
//! holding the terminal's lock, and then draws from it with the lock released, because drawing touches the
//! font atlas and the graphics device and holding the lock across all of that would stall the thread
//! reading the shell's output.
//!
//! Two things a terminal carries as flags are resolved into colours here rather than left for the painter.
//! Inverse video means the two colours are swapped, and dim means the foreground is the darker variant. A
//! painter that had to know those rules would be a second place they could be got wrong.

use crate::palette::Rgb;

/// One cell of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    /// The character in the cell. A space when there is nothing in it.
    pub character: char,
    /// Combining marks that are drawn over the character, such as an accent.
    pub marks: Vec<char>,
    pub foreground: Rgb,
    pub background: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// The character takes two columns, which is what a Chinese, Japanese or Korean character does, and
    /// what many emoji do.
    pub wide: bool,
    /// The second column of a wide character. Nothing is drawn in it; its neighbour covers it.
    pub spacer: bool,
    /// The program asked for the character not to be shown, which is what a password prompt does.
    pub hidden: bool,
}

impl ScreenCell {
    /// An empty cell in the terminal's own colours.
    pub fn blank(foreground: Rgb, background: Rgb) -> Self {
        Self {
            character: ' ',
            marks: Vec::new(),
            foreground,
            background,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            wide: false,
            spacer: false,
            hidden: false,
        }
    }

    /// True when there is nothing to draw in this cell but its background.
    pub fn is_blank(&self) -> bool {
        self.spacer || self.hidden || self.character == ' ' || self.character == '\0'
    }
}

/// The shape the program asked the cursor to be drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// A filled rectangle over the cell.
    #[default]
    Block,
    /// A line down the left of the cell.
    Beam,
    /// A line under the cell.
    Underline,
}

/// Where the cursor is and what it looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub column: usize,
    pub shape: CursorShape,
}

/// What is on the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub rows: usize,
    pub columns: usize,
    /// `rows * columns` cells, the first row first.
    pub cells: Vec<ScreenCell>,
    /// Absent when the program has hidden the cursor, or when the view is scrolled up into the history.
    pub cursor: Option<Cursor>,
    /// The cells the mouse has selected, as offsets into `cells`.
    pub selection: Option<std::ops::Range<usize>>,
    /// How many lines above the bottom the view is. Zero means the newest output is showing.
    pub scrollback: usize,
    /// How many lines of history there are to scroll back through.
    pub history: usize,
    /// The title the program set, which is what a tab is named after.
    pub title: String,
    /// The colour behind the whole grid, which is what the tile is filled with before the cells are drawn.
    pub background: Rgb,
}

impl Screen {
    pub fn empty(rows: usize, columns: usize, foreground: Rgb, background: Rgb) -> Self {
        Self {
            rows,
            columns,
            cells: vec![ScreenCell::blank(foreground, background); rows * columns],
            cursor: None,
            selection: None,
            scrollback: 0,
            history: 0,
            title: String::new(),
            background,
        }
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&ScreenCell> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        self.cells.get(row * self.columns + column)
    }

    /// One row as text, with the trailing spaces removed. Used by the tests and by copying a line.
    pub fn row_text(&self, row: usize) -> String {
        let mut text = String::new();
        for column in 0..self.columns {
            if let Some(cell) = self.cell(row, column) {
                if cell.spacer {
                    continue;
                }
                text.push(cell.character);
                for mark in &cell.marks {
                    text.push(*mark);
                }
            }
        }
        text.trim_end().to_owned()
    }

    /// The whole screen as text, one line a row, with the blank lines at the bottom left off.
    ///
    /// This is what the tests that run a real shell assert against, because when a shell prints its prompt
    /// is not something a test can know, but what it printed is.
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = (0..self.rows).map(|row| self.row_text(row)).collect();
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// True when `needle` is anywhere on the screen. A test waiting for output asks this.
    pub fn contains(&self, needle: &str) -> bool {
        (0..self.rows).any(|row| self.row_text(row).contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Screen {
        let mut screen = Screen::empty(3, 6, Rgb::new(1, 1, 1), Rgb::new(0, 0, 0));
        for (index, character) in "hello".chars().enumerate() {
            screen.cells[index].character = character;
        }
        for (index, character) in "world".chars().enumerate() {
            screen.cells[6 + index].character = character;
        }
        screen
    }

    #[test]
    fn a_row_reads_back_as_text_without_its_trailing_spaces() {
        let screen = screen();
        assert_eq!(screen.row_text(0), "hello");
        assert_eq!(screen.row_text(1), "world");
        assert_eq!(screen.row_text(2), "");
    }

    #[test]
    fn the_screen_reads_back_without_the_blank_lines_at_the_bottom() {
        assert_eq!(screen().text(), "hello\nworld");
    }

    #[test]
    fn a_search_finds_text_on_any_row() {
        let screen = screen();
        assert!(screen.contains("hello"));
        assert!(screen.contains("orl"));
        assert!(!screen.contains("helloworld"), "a row does not run into the next one");
    }

    #[test]
    fn a_cell_outside_the_grid_is_nothing_rather_than_a_panic() {
        let screen = screen();
        assert!(screen.cell(2, 5).is_some());
        assert!(screen.cell(3, 0).is_none());
        assert!(screen.cell(0, 6).is_none());
    }

    #[test]
    fn a_blank_cell_has_nothing_to_draw() {
        let blank = ScreenCell::blank(Rgb::default(), Rgb::default());
        assert!(blank.is_blank());
        let letter = ScreenCell { character: 'a', ..blank.clone() };
        assert!(!letter.is_blank());
        let spacer = ScreenCell { character: 'x', spacer: true, ..blank };
        assert!(spacer.is_blank(), "the second column of a wide character draws nothing");
    }
}
