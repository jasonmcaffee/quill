//! The formatting controls and the background opacity control.
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

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_core::{Align, Color, Command, Document, StyleChange};

use crate::theme::{color, icon, size};

/// The colours the toolbar offers, with the name shown when hovering over each one.
pub const COLORS: &[(&str, Color)] = &[
    ("White", Color::WHITE),
    ("Red", Color::RED),
    ("Green", Color::GREEN),
    ("Blue", Color::BLUE),
    ("Amber", Color::YELLOW),
];

/// The sizes the size control offers.
pub const SIZES: &[f32] = &[9.0, 11.0, 13.0, 16.0, 20.0, 24.0, 32.0, 48.0, 64.0];

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
    families: &[String],
    opacity: &mut f32,
    bold_family: &egui::FontFamily,
    view_mode: crate::app::ViewMode,
) -> ToolbarOutcome {
    let mut outcome = ToolbarOutcome::default();
    let background = crate::theme::faded(color::TOOLBAR, *opacity);
    ui.painter_at(area).rect_filled(area, CornerRadius::ZERO, background);

    let style = document.active_style();
    let paragraph = document.active_paragraph_style();
    let middle = area.center().y;
    let mut pen = area.left() + 16.0;

    // The font family, as a button that opens a list.
    let family_width = 175.0;
    if let Some(command) = dropdown(
        ui,
        Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::new(family_width, 28.0)),
        &style.family,
        "Font family",
        |ui| {
            let mut chosen = None;
            for family in families {
                if ui.selectable_label(*family == style.family, family).clicked() {
                    chosen = Some(family.clone());
                }
            }
            chosen.map(|family| Command::ApplyStyle(StyleChange::family(family)))
        },
    ) {
        outcome.commands.push(command);
    }
    pen += family_width + 10.0;

    // The font size.
    let size_width = 52.0;
    if let Some(command) = dropdown(
        ui,
        Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::new(size_width, 28.0)),
        &format!("{:.0}", style.size),
        "Font size",
        |ui| {
            let mut chosen = None;
            for option in SIZES {
                if ui
                    .selectable_label((style.size - option).abs() < 0.01, format!("{option:.0}"))
                    .clicked()
                {
                    chosen = Some(*option);
                }
            }
            chosen.map(|size| Command::ApplyStyle(StyleChange::size(size)))
        },
    ) {
        outcome.commands.push(command);
    }
    pen += size_width + 12.0;

    separator(ui, pen, middle);
    pen += 13.0;

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

    separator(ui, pen, middle);
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

    separator(ui, pen, middle);
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

    separator(ui, pen, middle);
    pen += 15.0;

    // The line spacing.
    let spacing_width = 128.0;
    if let Some(command) = dropdown_with_icon(
        ui,
        Rect::from_min_size(Pos2::new(pen, middle - 14.0), Vec2::new(spacing_width, 28.0)),
        &spacing_label(paragraph.line_spacing),
        "Line spacing",
        icon::line_spacing,
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

    // Undo, redo and the opacity control sit against the right edge.
    let opacity_width = 74.0;
    let opacity_rect = Rect::from_min_size(
        Pos2::new(area.right() - 16.0 - opacity_width, middle - 14.0),
        Vec2::new(opacity_width, 28.0),
    );
    opacity_control(ui, opacity_rect, opacity);

    let group_width = 62.0;
    let group = Rect::from_min_size(
        Pos2::new(opacity_rect.left() - 12.0 - group_width, middle - 14.0),
        Vec2::new(group_width, 28.0),
    );

    // The three view modes, immediately to the left of undo.
    let modes = crate::app::ViewMode::ALL;
    let modes_width = modes.len() as f32 * 32.0 - 4.0;
    let mut mode_pen = group.left() - 14.0 - modes_width;
    for mode in modes {
        let button = Rect::from_min_size(Pos2::new(mode_pen, middle - 14.0), Vec2::splat(28.0));
        if view_mode_button(ui, button, mode, view_mode == mode) {
            outcome.view_mode = Some(mode);
        }
        mode_pen += 32.0;
    }
    separator(ui, group.left() - 7.0, middle);
    ui.painter_at(area).rect_filled(group, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    for (index, (forward, name, command, enabled)) in [
        (false, "Undo", Command::Undo, document.can_undo()),
        (true, "Redo", Command::Redo, document.can_redo()),
    ]
    .into_iter()
    .enumerate()
    {
        let half = Rect::from_min_size(
            Pos2::new(group.left() + index as f32 * group_width / 2.0, group.top()),
            Vec2::new(group_width / 2.0, group.height()),
        );
        let response = ui
            .interact(half, ui.id().with(("history", name)), Sense::click())
            .on_hover_text(name);
        let tint = if enabled { color::TEXT_CONTROL } else { color::TEXT_FAINT.gamma_multiply(0.5) };
        icon::undo_redo(&ui.painter_at(area), half.center(), forward, tint);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name)
        });
        if response.clicked() && enabled {
            outcome.commands.push(command);
        }
    }

    outcome
}

fn separator(ui: &egui::Ui, x: f32, middle: f32) {
    ui.painter().line_segment(
        [Pos2::new(x, middle - 10.0), Pos2::new(x, middle + 10.0)],
        Stroke::new(1.0, color::DIVIDER),
    );
}

/// A button showing the current value, which opens a list when clicked.
fn dropdown(
    ui: &mut egui::Ui,
    area: Rect,
    value: &str,
    name: &str,
    contents: impl FnOnce(&mut egui::Ui) -> Option<Command>,
) -> Option<Command> {
    dropdown_inner(ui, area, value, name, None, contents)
}

/// The same, with a small drawn icon in front of the value.
fn dropdown_with_icon(
    ui: &mut egui::Ui,
    area: Rect,
    value: &str,
    name: &str,
    draw: fn(&egui::Painter, Pos2, Color32),
    contents: impl FnOnce(&mut egui::Ui) -> Option<Command>,
) -> Option<Command> {
    dropdown_inner(ui, area, value, name, Some(draw), contents)
}

fn dropdown_inner(
    ui: &mut egui::Ui,
    area: Rect,
    value: &str,
    name: &str,
    draw: Option<fn(&egui::Painter, Pos2, Color32)>,
    contents: impl FnOnce(&mut egui::Ui) -> Option<Command>,
) -> Option<Command> {
    let id = ui.id().with(("dropdown", name));
    let response = ui.interact(area, id, Sense::click()).on_hover_text(name);
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::CONTROL,
        Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
    let mut text_left = area.left() + 10.0;
    if let Some(draw) = draw {
        draw(painter, Pos2::new(text_left + 4.0, area.center().y), color::TEXT_DIM);
        text_left += 16.0;
    }
    let galley = painter.layout_no_wrap(
        value.to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_CONTROL,
    );
    painter.galley(
        Pos2::new(text_left, area.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    icon::chevron_down(painter, Pos2::new(area.right() - 11.0, area.center().y), color::TEXT_DIM);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), name)
    });

    // `Popup::from_toggle_button_response` opens and closes on clicks of this button and holds the state
    // itself. The memory functions that would do it by hand are private in egui 0.36.
    let chosen = egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            egui::Frame::popup(ui.style())
                .fill(color::CONTROL)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER)),
        )
        .width(area.width().max(120.0))
        .show(contents)
        .and_then(|inner| inner.inner);
    if chosen.is_some() {
        egui::Popup::close_id(ui.ctx(), id);
    }
    chosen
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

/// The background opacity control: a button showing the percentage, which opens a menu holding the slider.
///
/// The design puts it here as a compact control rather than an inline slider, and its own welcome text
/// says to open the menu to fade the desktop through the window.
fn opacity_control(ui: &mut egui::Ui, area: Rect, opacity: &mut f32) {
    let id = ui.id().with("opacity");
    let response = ui
        .interact(area, id, Sense::click())
        .on_hover_text("Background opacity. The desktop shows through. Text stays fully opaque.");
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::CONTROL,
        Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
    icon::half_filled_circle(
        painter,
        Pos2::new(area.left() + 15.0, area.center().y),
        6.0,
        color::TEXT_CONTROL,
    );
    let text = format!("{:.0}%", *opacity * 100.0);
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.5), color::TEXT_CONTROL);
    painter.galley(
        Pos2::new(area.left() + 27.0, area.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Background opacity")
    });

    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            egui::Frame::popup(ui.style())
                // Darker than a control, so the slider's rail, which is drawn in the control colour, is
                // visible against it rather than blending in.
                .fill(color::MENU)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER))
                .inner_margin(10.0),
        )
        .width(250.0)
        .show(|ui| {
            ui.label(egui::RichText::new("Background opacity").size(12.0).color(color::TEXT_DIM));
            ui.add_space(2.0);
            // The floor is above zero so the window cannot be lost entirely by dragging the slider to
            // the end.
            ui.spacing_mut().slider_width = 180.0;
            let percent = format!("{:.0}%", *opacity * 100.0);
            ui.add(egui::Slider::new(opacity, 0.05..=1.0).show_value(false).text(percent));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("The desktop shows through. Text stays solid.")
                    .size(11.0)
                    .color(color::TEXT_FAINT),
            );
        });
}

fn spacing_label(spacing: f32) -> String {
    for (name, value) in SPACINGS {
        if (spacing - value).abs() < 0.01 {
            return (*name).to_owned();
        }
    }
    format!("{spacing:.2}")
}
