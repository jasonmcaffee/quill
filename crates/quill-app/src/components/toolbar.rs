//! The formatting controls along the top of the window.
//!
//! Every control sends a `quill_core::Command`. None of them changes the document directly, so the
//! toolbar and the keyboard go down the same path and cannot disagree about what bold means.
//!
//! Each control is given an accessible name, because the screenshot tests find controls by name rather
//! than by position, so moving a button does not break a test.
//!
//! The design draws the buttons rather than labelling them: the B is bold, the I is italic, the U is
//! underlined and the S is struck through, so each one looks like what it does, and the four alignment
//! buttons are small pictures of a paragraph. The colours are circles with a ring round the chosen one.
//!
//! Three things that used to be here are not any more. The font family and the font size are in
//! `Edit -> Settings -> Appearance -> Font`, the background opacity is in
//! `Edit -> Settings -> Appearance -> Background`, and undo and redo are on the keyboard alone, at
//! command or control with Z and with shift and Z. A toolbar button for undo says nothing a reader does
//! not already know and takes room from the formatting.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_core::{Align, Color, Command, Document, StyleChange};

use crate::components::controls;
use crate::theme::{color, icon, size};

/// The colours the toolbar offers, with the name shown when hovering over each one.
pub const COLORS: &[(&str, Color)] = &[
    ("White", Color::WHITE),
    ("Red", Color::RED),
    ("Green", Color::GREEN),
    ("Blue", Color::BLUE),
    ("Amber", Color::YELLOW),
];

/// The line spacings the spacing control offers, with the name shown for each.
pub const SPACINGS: &[(&str, f32)] = &[("Single", 1.0), ("One and a half", 1.5), ("Double", 2.0)];

/// What the toolbar produced this frame.
#[derive(Debug, Default)]
pub struct ToolbarOutcome {
    pub commands: Vec<Command>,
    /// Set when one of the three view mode buttons was pressed.
    pub view_mode: Option<crate::app::ViewMode>,
}

/// Draw the toolbar into `area`.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    document: &Document,
    opacity: f32,
    bold_family: &egui::FontFamily,
    view_mode: crate::app::ViewMode,
) -> ToolbarOutcome {
    let mut outcome = ToolbarOutcome::default();
    let background = crate::theme::faded(color::TOOLBAR, opacity);
    ui.painter_at(area).rect_filled(area, CornerRadius::ZERO, background);

    let style = document.active_style();
    let paragraph = document.active_paragraph_style();
    let middle = area.center().y;
    let mut pen = area.left() + 16.0;

    // Bold, italic, underline and strikethrough. Each glyph carries the formatting it applies.
    let formats: [(&str, &str, bool, Command); 4] = [
        ("B", "Bold", style.bold, Command::ToggleBold),
        ("I", "Italic", style.italic, Command::ToggleItalic),
        ("U", "Underline", style.underline, Command::ToggleUnderline),
        ("S", "Strikethrough", style.strikethrough, Command::ToggleStrikethrough),
    ];
    for (letter, name, active, command) in formats {
        let button = Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::splat(28.0));
        if format_button(ui, button, letter, name, active, bold_family) {
            outcome.commands.push(command);
        }
        pen += 32.0;
    }
    pen += 2.0;

    controls::separator(ui, pen, middle);
    pen += 15.0;

    // The colours, as circles with a ring round the chosen one.
    for (name, swatch) in COLORS {
        let centre = Pos2::new(pen + 8.0, middle);
        let hit = Rect::from_center_size(centre, Vec2::splat(24.0));
        let response = ui
            .interact(hit, ui.id().with(("colour", name)), Sense::click())
            .on_hover_text(format!("Colour: {name}"));
        let chosen = style.color == *swatch;
        let painter = ui.painter_at(area);
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
            outcome.commands.push(Command::ApplyStyle(StyleChange::color(*swatch)));
        }
        pen += 24.0;
    }
    pen += 4.0;

    controls::separator(ui, pen, middle);
    pen += 15.0;

    // The four alignments, drawn as small pictures of a paragraph.
    for align in Align::ALL {
        let button = Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::splat(28.0));
        if alignment_button(ui, button, align, paragraph.align == align) {
            outcome.commands.push(Command::SetAlign(align));
        }
        pen += 32.0;
    }
    pen += 2.0;

    controls::separator(ui, pen, middle);
    pen += 15.0;

    // The line spacing.
    let spacing_width = 128.0;
    if let Some(command) = controls::dropdown(
        ui,
        Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::new(spacing_width, 28.0)),
        &spacing_label(paragraph.line_spacing),
        "Line spacing",
        Some(icon::line_spacing),
        |ui| {
            let mut chosen = None;
            for (name, spacing) in SPACINGS {
                let selected = (paragraph.line_spacing - spacing).abs() < 0.01;
                if ui.selectable_label(selected, *name).clicked() {
                    chosen = Some(*spacing);
                }
            }
            chosen.map(Command::SetLineSpacing)
        },
    ) {
        outcome.commands.push(command);
    }

    // The three view modes sit against the right edge.
    let modes = crate::app::ViewMode::ALL;
    let modes_width = modes.len() as f32 * 32.0 - 4.0;
    let mut mode_pen = area.right() - 16.0 - modes_width;
    for mode in modes {
        let button = Rect::from_min_size(Pos2::new(mode_pen, middle - 14.0), Vec2::splat(28.0));
        if view_mode_button(ui, button, mode, view_mode == mode) {
            outcome.view_mode = Some(mode);
        }
        mode_pen += 32.0;
    }

    outcome
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
) -> bool {
    let name = mode.label();
    let response = ui
        .interact(area, ui.id().with(("view-mode", name)), Sense::click())
        .on_hover_text(mode.description());
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

fn spacing_label(spacing: f32) -> String {
    for (name, value) in SPACINGS {
        if (spacing - value).abs() < 0.01 {
            return (*name).to_owned();
        }
    }
    format!("{spacing:.2}")
}
