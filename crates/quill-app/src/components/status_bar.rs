//! The bar along the bottom of the window.
//!
//! Shows which file is open, whether it has unsaved changes, what kind of file it is, where the caret is,
//! and the font family and size at the caret.
//!
//! A tab holding a picture has neither a caret nor a font, so it has neither of those: the line and
//! column are absent, and the right hand end says how big the picture is and how far it is zoomed
//! instead. One field each rather than a second kind of status bar, because it is the same bar saying
//! what it can about whatever is open.

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
    /// Where the caret is. Absent for a tab holding a picture, which has no caret.
    pub position: Option<Position>,
    /// What is said at the right hand end: the font at the caret, or a picture's size and scale.
    pub detail: &'a str,
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
    let Status { name, unsaved, kind, position, detail, message, git } = *status;
    let painter = ui.painter_at(area);
    // The bottom two corners are rounded to match the window.
    painter.rect_filled(
        area,
        CornerRadius { nw: 0, ne: 0, sw: size::WINDOW_CORNER, se: size::WINDOW_CORNER },
        crate::theme::faded(color::status_bar(), opacity),
    );

    let font = egui::FontId::proportional(11.0);
    let mut pen = area.left() + 16.0;
    let middle = area.center().y;

    let label = |painter: &egui::Painter, pen: &mut f32, text: String, colour| {
        let galley = painter.layout_no_wrap(text, font.clone(), colour);
        painter.galley(Pos2::new(*pen, middle - galley.size().y / 2.0), galley.clone(), colour);
        *pen += galley.size().x;
    };

    label(&painter, &mut pen, name.to_owned(), color::text_control());
    pen += 12.0;
    if unsaved {
        painter.circle_filled(Pos2::new(pen + 3.0, middle), 3.0, color::unsaved());
        pen += 12.0;
        label(&painter, &mut pen, "Unsaved".to_owned(), color::text_dim());
        pen += 12.0;
    }
    label(&painter, &mut pen, "\u{2502}".to_owned(), color::divider());
    pen += 10.0;
    label(&painter, &mut pen, kind.to_owned(), color::text_dim());
    // A picture has no caret, so it has no line and column either.
    if let Some(position) = position {
        pen += 12.0;
        label(&painter, &mut pen, "\u{2502}".to_owned(), color::divider());
        pen += 10.0;
        label(
            &painter,
            &mut pen,
            format!("Ln {}, Col {}", position.line, position.column),
            color::text_dim(),
        );
    }

    // The font, or the picture's size, sits against the right edge, and the branch sits before it,
    // which is where every editor with a status bar puts it. Worked out before the message is drawn,
    // because where they end is what says how much room the message has.
    let galley = painter.layout_no_wrap(detail.to_owned(), font.clone(), color::text_dim());
    let mut right = area.right() - 16.0 - galley.size().x;
    painter.galley(Pos2::new(right, middle - galley.size().y / 2.0), galley, color::text_dim());
    if let Some(git) = git {
        let galley = painter.layout_no_wrap(git.to_owned(), font.clone(), color::text_control());
        right -= 18.0 + galley.size().x;
        crate::theme::icon::branch(&painter, Pos2::new(right - 12.0, middle), color::text_dim());
        painter.galley(Pos2::new(right, middle - galley.size().y / 2.0), galley, color::text_control());
    }

    // A message, when there is one, sits after the caret position and **before whatever the right
    // hand end took**, cut short with an ellipsis rather than drawn over the branch and the font. A
    // long one used to be drawn straight through them: `task-1692`'s refusals name a program, where
    // it comes from and the command that installs it, so a sentence longer than the bar stopped
    // being the rare case it had been.
    if let Some(message) = message {
        pen += 12.0;
        label(&painter, &mut pen, "\u{2502}".to_owned(), color::divider());
        pen += 10.0;
        // Clear of the branch icon as well, which is drawn twelve points to the left of `right`.
        let room = right - 26.0 - pen;
        if room > 24.0 {
            let galley = elided(&painter, message, &font, room);
            painter.galley(Pos2::new(pen, middle - galley.size().y / 2.0), galley, color::text_control());
        }
    }
}

/// `message` laid out in one line, cut short with an ellipsis when it will not fit in `room`.
///
/// Measured rather than counted, because the font is proportional and a count of characters would be
/// wrong by a word either way. The first guess is the proportion that fits and the loop takes one
/// character at a time from there, which is a handful of layouts for a long sentence and none at all
/// for the usual message, which fits whole.
fn elided(
    painter: &egui::Painter,
    message: &str,
    font: &egui::FontId,
    room: f32,
) -> std::sync::Arc<egui::Galley> {
    let whole = painter.layout_no_wrap(message.to_owned(), font.clone(), color::text_control());
    if whole.size().x <= room {
        return whole;
    }
    let characters: Vec<char> = message.chars().collect();
    let mut keep = ((characters.len() as f32) * (room / whole.size().x)).floor() as usize;
    loop {
        let cut: String = characters.iter().take(keep).collect::<String>() + "\u{2026}";
        let galley = painter.layout_no_wrap(cut, font.clone(), color::text_control());
        if galley.size().x <= room || keep == 0 {
            return galley;
        }
        keep -= 1;
    }
}
