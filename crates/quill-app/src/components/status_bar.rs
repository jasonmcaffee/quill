//! The bar along the bottom of the window.
//!
//! Shows which file is open, whether it has unsaved changes, what kind of file it is, where the caret is,
//! and the font family and size at the caret.

use egui::{CornerRadius, Pos2, Rect};

use crate::theme::{color, size};

/// Where the caret is, counted the way a person counts: the first line is line 1 and the first column is
/// column 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

/// Work out the line and column of a byte offset.
///
/// The column counts grapheme clusters rather than bytes, because a column number is meant to say how
/// many characters along the line the caret is, and a letter with a combining accent is one character to
/// a reader however many bytes it takes.
pub fn position_of(text: &quill_core::Rope, offset: usize) -> Position {
    let line = text.byte_to_line(offset);
    let start = text.line_to_byte(line);
    let before = text.byte_slice(start..offset);
    let column = unicode_segmentation::UnicodeSegmentation::graphemes(before.as_str(), true).count();
    Position { line: line + 1, column: column + 1 }
}

/// What the status bar has to say.
#[derive(Debug, Clone)]
pub struct Status<'a> {
    pub name: &'a str,
    pub unsaved: bool,
    /// What kind of file it is, as a person would say it: `Markdown` or `Plain text`.
    pub kind: &'a str,
    pub position: Position,
    pub family: &'a str,
    pub font_size: f32,
    /// Something to say in place of the font: why a file could not be opened, or what version this is.
    /// It stays until something replaces it, because a message that goes away on its own is a message that
    /// is missed.
    pub message: Option<&'a str>,
    /// The branch, how far it is from its upstream, and anything half-finished. Absent when the
    /// folder that is open is not in a repository.
    pub git: Option<&'a str>,
}

/// Draw the status bar into `area`.
pub fn show(ui: &egui::Ui, area: Rect, status: &Status<'_>, opacity: f32) {
    let Status { name, unsaved, kind, position, family, font_size, message, git } = *status;
    let painter = ui.painter_at(area);
    // The bottom two corners are rounded to match the window.
    painter.rect_filled(
        area,
        CornerRadius { nw: 0, ne: 0, sw: size::WINDOW_CORNER, se: size::WINDOW_CORNER },
        crate::theme::faded(color::STATUS_BAR, opacity),
    );

    let font = egui::FontId::proportional(11.0);
    let mut pen = area.left() + 16.0;
    let middle = area.center().y;

    let label = |painter: &egui::Painter, pen: &mut f32, text: String, colour| {
        let galley = painter.layout_no_wrap(text, font.clone(), colour);
        painter.galley(Pos2::new(*pen, middle - galley.size().y / 2.0), galley.clone(), colour);
        *pen += galley.size().x;
    };

    label(&painter, &mut pen, name.to_owned(), color::TEXT_CONTROL);
    pen += 12.0;
    if unsaved {
        painter.circle_filled(Pos2::new(pen + 3.0, middle), 3.0, color::UNSAVED);
        pen += 12.0;
        label(&painter, &mut pen, "Unsaved".to_owned(), color::TEXT_DIM);
        pen += 12.0;
    }
    label(&painter, &mut pen, "\u{2502}".to_owned(), color::DIVIDER);
    pen += 10.0;
    label(&painter, &mut pen, kind.to_owned(), color::TEXT_DIM);
    pen += 12.0;
    label(&painter, &mut pen, "\u{2502}".to_owned(), color::DIVIDER);
    pen += 10.0;
    label(
        &painter,
        &mut pen,
        format!("Ln {}, Col {}", position.line, position.column),
        color::TEXT_DIM,
    );

    // A message, when there is one, sits after the caret position and before the right hand end.
    if let Some(message) = message {
        pen += 12.0;
        label(&painter, &mut pen, "\u{2502}".to_owned(), color::DIVIDER);
        pen += 10.0;
        label(&painter, &mut pen, message.to_owned(), color::TEXT_CONTROL);
    }

    // The family and size sit against the right edge, and the branch sits before them, which is
    // where every editor with a status bar puts it.
    let right_text = format!("{family} \u{00B7} {font_size:.0} pt");
    let galley = painter.layout_no_wrap(right_text, font.clone(), color::TEXT_DIM);
    let mut right = area.right() - 16.0 - galley.size().x;
    painter.galley(Pos2::new(right, middle - galley.size().y / 2.0), galley, color::TEXT_DIM);
    if let Some(git) = git {
        let galley = painter.layout_no_wrap(git.to_owned(), font, color::TEXT_CONTROL);
        right -= 18.0 + galley.size().x;
        crate::theme::icon::branch(&painter, Pos2::new(right - 12.0, middle), color::TEXT_DIM);
        painter.galley(Pos2::new(right, middle - galley.size().y / 2.0), galley, color::TEXT_CONTROL);
    }
}
