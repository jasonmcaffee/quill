//! The text tools in the title bar: the `F` button, the panel of text options behind it, and the three
//! view mode buttons beside it.
//!
//! Every control sends a `quill_core::Command`. None of them changes the document directly, so the tools
//! and the keyboard go down the same path and cannot disagree about what bold means.
//!
//! Each control is given an accessible name, because the screenshot tests find controls by name rather
//! than by position, so moving a button does not break a test.
//!
//! The design draws the buttons rather than labelling them: the B is bold, the I is italic, the U is
//! underlined and the S is struck through, so each one looks like what it does, and the four alignment
//! buttons are small pictures of a paragraph. The colours are circles with a ring round the chosen one.
//!
//! ## Where they are, and why they moved
//!
//! They used to sit in a strip of their own, forty four points tall, between the title bar and the tabs.
//! `task-1658` moved them into the right hand end of the title bar, next to the window buttons, and the
//! strip went with them. The reason is what the strip did to everything below it: it was drawn for a
//! `.md` file and not for a `.rs` one, so switching tabs moved the tabs, the explorer and the whole
//! editing area up and down by forty four points. A control that appears is fine; a window that jumps is
//! not. In the title bar the room is already there whether the tools are in it or not.
//!
//! ## What is shown, and when
//!
//! Two questions are asked of the open file, and neither is answered anywhere but in
//! `services::file_kind`:
//!
//! - [`applies`] — are there any tools at all. Prose only: Markdown, a text file, a document that has
//!   not been saved yet. A source file and a picture get none.
//! - [`file_kind::preview_applies`] — are the three view mode buttons drawn. Markdown, and a document
//!   that has not been saved anywhere yet. A `.txt` file has nothing to preview.
//!
//! ## Where the formatting went
//!
//! Behind one 28 point button carrying a drawn `F`, named `Text options`, which opens a flyout holding
//! the four character formats, the five colours, the four alignments and the three line spacings. Nine
//! controls that are set rarely no longer take the width of the window permanently, and each keeps the
//! name it had, so a test asks for `Bold` exactly as it did before — after opening the panel.
//!
//! Three things that were here before that are still gone. The font family and the font size are in
//! `Edit -> Settings -> Appearance -> Font` and on the keyboard at command or control with plus and
//! minus, the background opacity is in `Edit -> Settings -> Appearance -> Background`, and undo and
//! redo are on the keyboard alone.

use std::path::Path;

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_core::{Align, Color, Command, Document, StyleChange};

use crate::components::controls;
use crate::services::file_kind;
use crate::theme::{color, icon, size};

/// The colours the text options panel offers, with the name shown when hovering over each one.
pub const COLORS: &[(&str, Color)] = &[
    ("White", Color::WHITE),
    ("Red", Color::RED),
    ("Green", Color::GREEN),
    ("Blue", Color::BLUE),
    ("Amber", Color::YELLOW),
];

/// The line spacings the spacing control offers, with the name shown for each.
pub const SPACINGS: &[(&str, f32)] = &[("Single", 1.0), ("One and a half", 1.5), ("Double", 2.0)];

/// How big one tool button is, and from the left edge of one to the left edge of the next.
pub const BUTTON: f32 = 28.0;
const STEP: f32 = 32.0;
/// Between the `F` button and the three view modes, so they read as two groups rather than four
/// buttons in a row.
const GROUP_GAP: f32 = 10.0;

/// How wide the panel behind the `F` button is. Wide enough for `One and a half` to sit beside the
/// other two spacings without the row wrapping, which is the longest thing in it.
const PANEL: f32 = 316.0;
/// The column the labels down the left of the panel take.
const PANEL_LABEL: f32 = 78.0;
/// From the middle of one row of the panel to the middle of the next.
const PANEL_ROW: f32 = 34.0;

/// What the tools produced this frame.
#[derive(Debug, Default)]
pub struct ToolsOutcome {
    pub commands: Vec<Command>,
    /// Set when one of the three view mode buttons was pressed.
    pub view_mode: Option<crate::app::ViewMode>,
}

/// Whether any of the tools are drawn at all for this file.
///
/// **Either** question, not just the first. The `F` button and everything behind it is about how
/// prose is shown, so it is drawn for prose and not for code; but a `.mmd` file is not prose and
/// still has a preview, so it still wants the three view mode buttons. `task-1660` is what made
/// the two come apart: before it, a file that had a preview always had formatting too.
///
/// The title bar asks this before it lays itself out, because the answer decides how much of its
/// right hand end is spoken for.
pub fn applies(path: Option<&Path>) -> bool {
    file_kind::formatting_applies(path) || file_kind::preview_applies(path)
}

/// How much room the tools need for this file, which is nothing at all for most files.
///
/// The title bar leaves this much clear at its right hand end, in front of the window buttons, so
/// nothing it draws ever runs underneath a tool.
pub fn width(path: Option<&Path>) -> f32 {
    if !applies(path) {
        return 0.0;
    }
    let modes = crate::app::ViewMode::ALL.len() as f32;
    let modes_width = modes * STEP - (STEP - BUTTON);
    match (file_kind::formatting_applies(path), file_kind::preview_applies(path)) {
        (true, true) => BUTTON + GROUP_GAP + modes_width,
        (true, false) => BUTTON,
        // A diagram: the view modes on their own, with no `F` in front of them.
        (false, true) => modes_width,
        (false, false) => 0.0,
    }
}

/// Draw the tools into `area`, which is the rectangle the title bar left clear for them.
///
/// Nothing is painted behind them: the title bar is already there, and a second fill over it would be a
/// panel inside a panel. `area` is exactly [`width`] wide, so the `F` starts at its left edge and the
/// view modes finish at its right.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    document: &Document,
    bold_family: &egui::FontFamily,
    view_mode: crate::app::ViewMode,
) -> ToolsOutcome {
    let mut outcome = ToolsOutcome::default();
    let middle = area.center().y;
    let path = document.path();

    // The `F` is drawn only for prose. A control that can never apply to this file is absent, which
    // is the rule the window already keeps.
    let mut pen = area.left();
    if file_kind::formatting_applies(path) {
        let button =
            Rect::from_min_size(Pos2::new(pen, middle - BUTTON / 2.0), Vec2::splat(BUTTON));
        if let Some(commands) =
            controls::flyout(ui, button, "Text options", icon::font, PANEL, |panel| {
                text_options(panel, document, bold_family)
            })
        {
            outcome.commands.extend(commands);
        }
        pen = button.right() + GROUP_GAP;
    }

    // The three view modes sit beside the `F`, which is what `task-1658` asks for: one group of text
    // tools rather than one at each end of a bar.
    if file_kind::preview_applies(path) {
        let kind = file_kind::preview_kind(path);
        for mode in crate::app::ViewMode::ALL {
            let button =
                Rect::from_min_size(Pos2::new(pen, middle - BUTTON / 2.0), Vec2::splat(BUTTON));
            if view_mode_button(ui, button, mode, view_mode == mode, kind) {
                outcome.view_mode = Some(mode);
            }
            pen += STEP;
        }
    }

    outcome
}

/// The panel behind the `F` button: four rows, each named down the left.
///
/// A rule separates what applies to the selected text from what applies to the paragraph it is in,
/// because those are two different things and pressing one when you meant the other is the mistake
/// the rule is there to stop.
fn text_options(
    ui: &mut egui::Ui,
    document: &Document,
    bold_family: &egui::FontFamily,
) -> Vec<Command> {
    let mut commands = Vec::new();
    let style = document.active_style();
    let paragraph = document.active_paragraph_style();

    // The panel is painted at absolute positions inside one reserved rectangle, which is how
    // everything else in Quill is drawn. The rule between the two halves adds its own gap.
    let rule_gap = 11.0;
    let (area, _) = ui.allocate_exact_size(
        Vec2::new(PANEL - 12.0, PANEL_ROW * 4.0 + rule_gap),
        Sense::hover(),
    );
    let left = area.left() + PANEL_LABEL;
    let mut middle = area.top() + PANEL_ROW / 2.0;

    // Bold, italic, underline and strikethrough. Each glyph carries the formatting it applies.
    controls::row_label(ui.painter(), Pos2::new(area.left(), middle), "Format");
    let formats: [(&str, &str, bool, Command); 4] = [
        ("B", "Bold", style.bold, Command::ToggleBold),
        ("I", "Italic", style.italic, Command::ToggleItalic),
        ("U", "Underline", style.underline, Command::ToggleUnderline),
        ("S", "Strikethrough", style.strikethrough, Command::ToggleStrikethrough),
    ];
    let mut pen = left;
    for (letter, name, active, command) in formats {
        let button = Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::splat(28.0));
        if format_button(ui, button, letter, name, active, bold_family) {
            commands.push(command);
        }
        pen += 32.0;
    }
    middle += PANEL_ROW;

    // The colours, as circles with a ring round the chosen one.
    controls::row_label(ui.painter(), Pos2::new(area.left(), middle), "Colour");
    let mut pen = left + 4.0;
    for (name, swatch) in COLORS {
        let centre = Pos2::new(pen + 8.0, middle);
        let hit = Rect::from_center_size(centre, Vec2::splat(24.0));
        let response = ui
            .interact(hit, ui.id().with(("colour", name)), Sense::click())
            .on_hover_text(format!("Colour: {name}"));
        let chosen = style.color == *swatch;
        let painter = ui.painter();
        let fill = Color32::from_rgb(swatch.r, swatch.g, swatch.b);
        painter.circle_filled(centre, if chosen { 7.5 } else { 8.0 }, fill);
        if chosen {
            painter.circle_stroke(centre, 10.0, Stroke::new(1.6, color::TEXT_STRONG));
        } else if response.hovered() {
            painter.circle_stroke(centre, 10.0, Stroke::new(1.2, color::TEXT_DIM));
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, *name)
        });
        if response.clicked() {
            commands.push(Command::ApplyStyle(StyleChange::color(*swatch)));
        }
        pen += 26.0;
    }
    middle += PANEL_ROW / 2.0 + rule_gap / 2.0;

    ui.painter().line_segment(
        [
            Pos2::new(area.left(), middle.round()),
            Pos2::new(area.right(), middle.round()),
        ],
        Stroke::new(1.0, color::DIVIDER),
    );
    middle += PANEL_ROW / 2.0 + rule_gap / 2.0;

    // The four alignments, drawn as small pictures of a paragraph.
    controls::row_label(ui.painter(), Pos2::new(area.left(), middle), "Alignment");
    let mut pen = left;
    for align in Align::ALL {
        let button = Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::splat(28.0));
        if alignment_button(ui, button, align, paragraph.align == align) {
            commands.push(Command::SetAlign(align));
        }
        pen += 32.0;
    }
    middle += PANEL_ROW;

    // The line spacing, as three buttons rather than a dropdown: a dropdown inside a flyout would
    // shut the flyout, because egui keeps one popup open at a time.
    controls::row_label(ui.painter(), Pos2::new(area.left(), middle), "Line spacing");
    let mut pen = left;
    for (name, spacing) in SPACINGS {
        let width = spacing_width(ui, name);
        let button = Rect::from_min_size(Pos2::new(pen, middle - 13.0), Vec2::new(width, 26.0));
        let chosen = (paragraph.line_spacing - spacing).abs() < 0.01;
        if controls::choice_button(ui, button, name, chosen) {
            commands.push(Command::SetLineSpacing(*spacing));
        }
        pen += width + 6.0;
    }

    commands
}

/// How wide the button for one line spacing has to be to hold its name.
fn spacing_width(ui: &egui::Ui, name: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_CONTROL,
    );
    (galley.size().x + 16.0).round()
}

/// One of the four character formatting buttons. The letter is drawn with the formatting it applies, so
/// the button looks like what it does.
fn format_button(
    ui: &mut egui::Ui,
    area: Rect,
    letter: &str,
    name: &str,
    active: bool,
    bold_family: &egui::FontFamily,
) -> bool {
    let response = ui
        .interact(area, ui.id().with(("format", name)), Sense::click())
        .on_hover_text(name);
    if active {
        ui.painter().rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::ACCENT);
    } else if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    }
    let tint = if active { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    // The letter is drawn with the formatting it applies, so the button looks like what it does. Bold uses
    // the real bold face installed in `theme::install_fonts`, because egui's built in font has none.
    let font_id = if name == "Bold" {
        egui::FontId::new(13.5, bold_family.clone())
    } else {
        egui::FontId::proportional(13.5)
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        letter,
        0.0,
        egui::TextFormat {
            font_id,
            color: tint,
            italics: name == "Italic",
            underline: if name == "Underline" { Stroke::new(1.0, tint) } else { Stroke::NONE },
            strikethrough: if name == "Strikethrough" { Stroke::new(1.0, tint) } else { Stroke::NONE },
            ..Default::default()
        },
    );
    let painter = ui.painter();
    let galley = painter.layout_job(job);
    painter.galley(area.center() - galley.size() / 2.0, galley, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, name)
    });
    response.clicked()
}

/// One of the three view mode buttons, drawn as a small picture of what that mode shows.
fn view_mode_button(
    ui: &mut egui::Ui,
    area: Rect,
    mode: crate::app::ViewMode,
    active: bool,
    kind: file_kind::PreviewKind,
) -> bool {
    let name = mode.label_for(kind);
    let response = ui
        .interact(area, ui.id().with(("view-mode", name)), Sense::click())
        .on_hover_text(mode.description_for(kind));
    let painter = ui.painter();
    if active {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    }
    let tint = if active { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    icon::view_mode(painter, area.shrink(6.0), mode, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, name)
    });
    response.clicked()
}

/// One of the four alignment buttons, drawn as a small picture of a paragraph placed that way.
fn alignment_button(ui: &mut egui::Ui, area: Rect, align: Align, active: bool) -> bool {
    let name = align.label();
    let response = ui
        .interact(area, ui.id().with(("align", name)), Sense::click())
        .on_hover_text(format!("Align {}", name.to_lowercase()));
    let painter = ui.painter();
    if active {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    }
    let tint = if active { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    icon::alignment(painter, area.shrink2(Vec2::new(7.0, 9.0)), align, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, name)
    });
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_file_with_no_tools_takes_no_room_in_the_title_bar() {
        for code in ["main.rs", "Cargo.toml", "photo.png"] {
            assert_eq!(width(Some(Path::new(code))), 0.0, "{code} has no text tools");
        }
    }

    #[test]
    fn markdown_gets_the_f_and_the_three_view_modes_and_a_text_file_gets_only_the_f() {
        let markdown = width(Some(Path::new("notes.md")));
        let plain = width(Some(Path::new("notes.txt")));
        assert_eq!(plain, BUTTON, "a text file has nothing to preview, so it is the F alone");
        assert_eq!(markdown, BUTTON + GROUP_GAP + 3.0 * STEP - (STEP - BUTTON));
        assert!(markdown > plain);
        // A document that has never been saved is treated as the beginning of a Markdown file, which
        // is what `file_kind::preview_applies` already decides for the View menu.
        assert_eq!(width(None), markdown);
    }
}
