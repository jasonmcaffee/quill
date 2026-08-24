//! The palette, the measurements and the drawn icons.
//!
//! Every colour here was read out of `design/intial-design-screenshot.png` rather than chosen by eye. The example at
//! `examples/sample_design.rs` reports, for each region of that image, the colour covering most of it and
//! the most saturated colour in it, which is how the accents were found. Run it with
//! `cargo run --example sample_design` to check any of these against the design again.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};

/// Colours taken from the design.
pub mod color {
    use egui::Color32;

    /// Behind the text. The window's alpha is applied to this by the opacity setting.
    pub const EDITOR: Color32 = Color32::from_rgb(0x1A, 0x1F, 0x26);
    /// The bar along the top holding the window buttons and the file name.
    pub const TITLE_BAR: Color32 = Color32::from_rgb(0x2A, 0x31, 0x3D);
    /// The bar holding the formatting controls.
    pub const TOOLBAR: Color32 = Color32::from_rgb(0x1E, 0x22, 0x2A);
    /// Behind the file explorer.
    pub const EXPLORER: Color32 = Color32::from_rgb(0x1F, 0x23, 0x2A);
    /// The strip at the bottom of the explorer counting the files.
    pub const EXPLORER_FOOTER: Color32 = Color32::from_rgb(0x1C, 0x20, 0x26);
    /// The bar along the very bottom of the window.
    pub const STATUS_BAR: Color32 = Color32::from_rgb(0x10, 0x15, 0x19);
    /// Inside a dropdown or a button that is not active.
    pub const CONTROL: Color32 = Color32::from_rgb(0x35, 0x3B, 0x46);
    /// Inside the box that filters the file list.
    pub const FIELD: Color32 = Color32::from_rgb(0x1D, 0x21, 0x2A);
    /// Round the edge of a control.
    pub const CONTROL_BORDER: Color32 = Color32::from_rgb(0x38, 0x3F, 0x4B);
    /// Between the panels.
    pub const DIVIDER: Color32 = Color32::from_rgb(0x2A, 0x30, 0x3B);
    /// Behind a menu. Darker than a control so that a control drawn on top of it stands out.
    pub const MENU: Color32 = Color32::from_rgb(0x26, 0x2C, 0x36);

    /// Anything switched on: an active button, the caret, the row of the open file.
    pub const ACCENT: Color32 = Color32::from_rgb(0x48, 0x9F, 0xF8);
    /// Behind the name of the file that is open.
    pub const SELECTED_ROW: Color32 = Color32::from_rgb(0x30, 0x43, 0x61);
    /// There are changes that have not been saved.
    pub const UNSAVED: Color32 = Color32::from_rgb(0xFE, 0xBC, 0x2E);
    /// Behind selected text.
    pub const TEXT_SELECTION: Color32 = Color32::from_rgb(0x30, 0x43, 0x61);

    /// A heading in the editor, and the file name in the title bar.
    pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    /// Ordinary text in the editor.
    pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEB, 0xF1);
    /// A label on a control, and a name in the file list.
    pub const TEXT_CONTROL: Color32 = Color32::from_rgb(0xC8, 0xCE, 0xDB);
    /// A heading in the explorer, the counts in its footer, and the status bar.
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8B, 0x93, 0xA3);
    /// The words inside the filter box before anything is typed.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x78, 0x80, 0x8F);

    /// The square in front of a Markdown file.
    pub const FILE_MARKDOWN: Color32 = Color32::from_rgb(0x41, 0x8C, 0xD9);
    /// The square in front of a plain text file.
    pub const FILE_TEXT: Color32 = Color32::from_rgb(0x7E, 0x87, 0x95);

    /// The three window buttons.
    pub const CLOSE: Color32 = Color32::from_rgb(0xFF, 0x5F, 0x57);
    pub const MINIMISE: Color32 = Color32::from_rgb(0xFE, 0xBC, 0x2E);
    pub const MAXIMISE: Color32 = Color32::from_rgb(0x28, 0xC8, 0x40);
}

/// Measurements taken from the design.
pub mod size {
    /// Height of the bar holding the window buttons and the file name.
    pub const TITLE_BAR: f32 = 50.0;
    /// Height of the bar holding the formatting controls.
    pub const TOOLBAR: f32 = 44.0;
    /// Height of the bar along the bottom of the window.
    pub const STATUS_BAR: f32 = 32.0;
    /// Width of the file explorer.
    pub const EXPLORER: f32 = 248.0;
    /// Height of the strip counting the files.
    pub const EXPLORER_FOOTER: f32 = 28.0;
    /// One row in the file list.
    pub const ROW: f32 = 28.0;
    /// How far one level of nesting indents.
    pub const INDENT: f32 = 18.0;
    /// Space between the text and the left edge of the editing area.
    pub const EDITOR_PADDING_X: f32 = 43.0;
    /// Space between the text and the top of the editing area.
    pub const EDITOR_PADDING_Y: f32 = 36.0;
    /// The window's rounded corner.
    pub const WINDOW_CORNER: u8 = 12;
    /// A control's rounded corner.
    pub const CONTROL_CORNER: u8 = 6;
}

/// Set up egui so that the ordinary controls come out looking like the design, rather than restyling each
/// one where it is used.
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.panel_fill = color::TOOLBAR;
    visuals.window_fill = color::MENU;
    visuals.extreme_bg_color = color::FIELD;
    visuals.faint_bg_color = color::EXPLORER_FOOTER;
    visuals.window_corner_radius = CornerRadius::same(size::CONTROL_CORNER);
    visuals.window_stroke = Stroke::new(1.0, color::CONTROL_BORDER);
    visuals.selection.bg_fill = color::ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, color::TEXT_STRONG);

    let corner = CornerRadius::same(size::CONTROL_CORNER);
    // Not interactive: labels and separators.
    visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::DIVIDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, color::TEXT_CONTROL);
    visuals.widgets.noninteractive.corner_radius = corner;
    // Sitting there, not being pointed at.
    visuals.widgets.inactive.bg_fill = color::CONTROL;
    visuals.widgets.inactive.weak_bg_fill = color::CONTROL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, color::CONTROL_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, color::TEXT_CONTROL);
    visuals.widgets.inactive.corner_radius = corner;
    // Being pointed at.
    visuals.widgets.hovered.bg_fill = color::CONTROL.gamma_multiply(1.25);
    visuals.widgets.hovered.weak_bg_fill = color::CONTROL.gamma_multiply(1.25);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color::ACCENT.gamma_multiply(0.6));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, color::TEXT_STRONG);
    visuals.widgets.hovered.corner_radius = corner;
    // Being pressed.
    visuals.widgets.active.bg_fill = color::ACCENT;
    visuals.widgets.active.weak_bg_fill = color::ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, color::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, color::TEXT_STRONG);
    visuals.widgets.active.corner_radius = corner;
    // A dropdown that is open.
    visuals.widgets.open.bg_fill = color::CONTROL;
    visuals.widgets.open.weak_bg_fill = color::CONTROL;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, color::ACCENT);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, color::TEXT_STRONG);
    visuals.widgets.open.corner_radius = corner;

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::same(6);
    });
}

/// The family name egui uses for the interface's bold text.
pub const BOLD_FAMILY: &str = "quill-bold";

/// Set the interface in a real font, so that the toolbar's bold B is actually bold.
///
/// egui's built in fonts have no bold face, so its `strong` styling only brightens the colour. The design
/// shows a genuinely bold B, so the family Quill is using is handed to egui as well, with its bold face
/// under a name the toolbar can ask for. egui's own fonts stay in the list behind ours, because they carry
/// symbols such as the triangles in front of a folder that a text face does not have.
pub fn install_fonts(ctx: &egui::Context, family: &str, regular: Option<Vec<u8>>, bold: Option<Vec<u8>>) {
    let mut fonts = egui::FontDefinitions::default();
    let mut bold_stack = Vec::new();
    if let Some(bytes) = bold {
        fonts.font_data.insert("quill-ui-bold".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        bold_stack.push("quill-ui-bold".to_owned());
    }
    if let Some(bytes) = regular {
        fonts.font_data.insert("quill-ui".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "quill-ui".to_owned());
        }
        bold_stack.push("quill-ui".to_owned());
    }
    // Whatever egui already had stays behind ours as a fallback for symbols.
    if let Some(defaults) = fonts.families.get(&egui::FontFamily::Proportional) {
        for name in defaults.clone() {
            if !bold_stack.contains(&name) {
                bold_stack.push(name);
            }
        }
    }
    if !bold_stack.is_empty() {
        fonts.families.insert(egui::FontFamily::Name(BOLD_FAMILY.into()), bold_stack);
    }
    let _ = family;
    ctx.set_fonts(fonts);
}

/// Apply the opacity setting to a background colour.
///
/// Only backgrounds go through this. Text, icons and the caret are always drawn at full alpha, which is
/// what lets the desktop show through the window without making the writing hard to read.
pub fn faded(base: Color32, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The colour of the square in front of a file, by what kind of file it is.
pub fn file_marker(path: &std::path::Path) -> Color32 {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("md") => color::FILE_MARKDOWN,
        _ => color::FILE_TEXT,
    }
}

/// What the status bar calls this kind of file.
pub fn file_kind(path: Option<&std::path::Path>) -> &'static str {
    match path.and_then(|p| p.extension()).and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref()
    {
        Some("md") => "Markdown",
        Some("txt") => "Plain text",
        _ => "Plain text",
    }
}

/// Drawn icons. The design uses shapes rather than letters for the alignment buttons, for undo and redo
/// and for the small controls in the explorer, and the characters for those are not in egui's default
/// fonts, so they are drawn here.
pub mod icon {
    use super::*;

    /// A small triangle pointing down, on the right of a dropdown.
    pub fn chevron_down(painter: &egui::Painter, centre: Pos2, color: Color32) {
        let w = 3.5;
        let h = 2.2;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(centre.x - w, centre.y - h),
                Pos2::new(centre.x + w, centre.y - h),
                Pos2::new(centre.x, centre.y + h),
            ],
            color,
            Stroke::NONE,
        ));
    }

    /// A triangle pointing down when a folder is open and right when it is closed.
    pub fn disclosure(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32) {
        let points = if open {
            vec![
                Pos2::new(centre.x - 4.0, centre.y - 2.0),
                Pos2::new(centre.x + 4.0, centre.y - 2.0),
                Pos2::new(centre.x, centre.y + 3.0),
            ]
        } else {
            vec![
                Pos2::new(centre.x - 2.0, centre.y - 4.0),
                Pos2::new(centre.x - 2.0, centre.y + 4.0),
                Pos2::new(centre.x + 3.0, centre.y),
            ]
        };
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }

    /// Four stacked lines showing how a paragraph is placed. The short lines sit where the ragged edge
    /// would be, which is what makes the four buttons tell each other apart.
    pub fn alignment(painter: &egui::Painter, area: Rect, align: quill_core::Align, color: Color32) {
        let full = area.width();
        let short = full * 0.62;
        let spacing = area.height() / 3.0;
        let stroke = Stroke::new(1.6, color);
        for row in 0..4 {
            let y = area.top() + spacing * row as f32;
            // Rows 1 and 3 are the short ones, so the shape reads as a paragraph of text.
            let width = if row % 2 == 1 { short } else { full };
            let x = match align {
                quill_core::Align::Left | quill_core::Align::Justify => area.left(),
                quill_core::Align::Center => area.left() + (full - width) / 2.0,
                quill_core::Align::Right => area.right() - width,
            };
            // Justified text is flush on both sides, so every line is full width except the last.
            let width = if align == quill_core::Align::Justify && row < 3 { full } else { width };
            let x = if align == quill_core::Align::Justify { area.left() } else { x };
            painter.line_segment([Pos2::new(x, y), Pos2::new(x + width, y)], stroke);
        }
    }

    /// An arc with an arrow head, pointing back for undo and forward for redo.
    pub fn undo_redo(painter: &egui::Painter, centre: Pos2, forward: bool, color: Color32) {
        let radius = 5.0;
        let stroke = Stroke::new(1.6, color);
        // Three quarters of a circle, drawn as a run of short lines.
        let mut points = Vec::new();
        let steps = 14;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            // Start at the top and sweep round, leaving a gap where the arrow head goes.
            let angle = std::f32::consts::PI * (0.15 + 1.55 * t);
            let x = angle.cos() * radius;
            let y = angle.sin() * radius;
            points.push(Pos2::new(centre.x + if forward { -x } else { x }, centre.y - y));
        }
        painter.add(egui::Shape::line(points.clone(), stroke));
        // The arrow head sits at the start of the sweep.
        let tip = points[0];
        let direction = if forward { -1.0 } else { 1.0 };
        painter.add(egui::Shape::convex_polygon(
            vec![
                tip,
                Pos2::new(tip.x + 3.4 * direction, tip.y - 1.0),
                Pos2::new(tip.x + 0.6 * direction, tip.y + 3.4),
            ],
            color,
            Stroke::NONE,
        ));
    }

    /// A circle filled on one side, which is how the design marks the background opacity control.
    pub fn half_filled_circle(painter: &egui::Painter, centre: Pos2, radius: f32, color: Color32) {
        painter.circle_stroke(centre, radius, Stroke::new(1.4, color));
        let mut points = vec![Pos2::new(centre.x, centre.y - radius)];
        let steps = 12;
        for step in 0..=steps {
            let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / steps as f32;
            points.push(Pos2::new(centre.x + angle.cos() * radius, centre.y + angle.sin() * radius));
        }
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }

    /// A circle with a handle, in front of the box that filters the file list.
    pub fn magnifier(painter: &egui::Painter, centre: Pos2, color: Color32) {
        let stroke = Stroke::new(1.3, color);
        painter.circle_stroke(Pos2::new(centre.x - 0.8, centre.y - 0.8), 3.4, stroke);
        painter.line_segment(
            [Pos2::new(centre.x + 1.6, centre.y + 1.6), Pos2::new(centre.x + 4.0, centre.y + 4.0)],
            stroke,
        );
    }

    /// Two crossed lines.
    pub fn plus(painter: &egui::Painter, centre: Pos2, color: Color32) {
        let stroke = Stroke::new(1.5, color);
        painter.line_segment(
            [Pos2::new(centre.x - 4.0, centre.y), Pos2::new(centre.x + 4.0, centre.y)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(centre.x, centre.y - 4.0), Pos2::new(centre.x, centre.y + 4.0)],
            stroke,
        );
    }

    /// An arrow pointing into a corner, for the button that hides the explorer.
    pub fn collapse(painter: &egui::Painter, centre: Pos2, color: Color32) {
        let stroke = Stroke::new(1.4, color);
        let a = Pos2::new(centre.x + 3.5, centre.y - 3.5);
        let b = Pos2::new(centre.x - 3.5, centre.y + 3.5);
        painter.line_segment([a, b], stroke);
        painter.line_segment([b, Pos2::new(b.x + 4.5, b.y)], stroke);
        painter.line_segment([b, Pos2::new(b.x, b.y - 4.5)], stroke);
    }

    /// The three view modes, drawn as small pictures of what each one shows.
    ///
    /// Raw Markdown is a page of even lines. Side by side is a page split down the middle. Preview is a
    /// page with a heading bar above its lines. Drawn rather than lettered, to match the alignment buttons.
    pub fn view_mode(painter: &egui::Painter, area: Rect, mode: crate::app::ViewMode, color: Color32) {
        use crate::app::ViewMode;
        let stroke = Stroke::new(1.2, color);
        // The page.
        painter.rect_stroke(area, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
        let inner = area.shrink(3.5);
        let line = |from: Pos2, to: Pos2| painter.line_segment([from, to], Stroke::new(1.1, color));
        match mode {
            ViewMode::Raw => {
                // Three even lines, the same on every row, because raw Markdown is just text.
                for row in 0..3 {
                    let y = inner.top() + inner.height() * row as f32 / 2.0;
                    line(Pos2::new(inner.left(), y), Pos2::new(inner.right(), y));
                }
            }
            ViewMode::SideBySide => {
                // A line down the middle, with rows either side of it.
                let middle = area.center().x;
                painter.line_segment(
                    [Pos2::new(middle, area.top() + 1.0), Pos2::new(middle, area.bottom() - 1.0)],
                    stroke,
                );
                for row in 0..3 {
                    let y = inner.top() + inner.height() * row as f32 / 2.0;
                    line(Pos2::new(inner.left(), y), Pos2::new(middle - 2.0, y));
                    line(Pos2::new(middle + 2.0, y), Pos2::new(inner.right(), y));
                }
            }
            ViewMode::Preview => {
                // A thick heading bar, then two thinner lines, which is what a rendered page looks like.
                let bar = Rect::from_min_size(
                    inner.left_top(),
                    egui::Vec2::new(inner.width() * 0.62, 2.6),
                );
                painter.rect_filled(bar, CornerRadius::same(1), color);
                for row in 1..3 {
                    let y = inner.top() + inner.height() * row as f32 / 2.0 + 1.0;
                    let width = if row == 2 { inner.width() * 0.75 } else { inner.width() };
                    line(Pos2::new(inner.left(), y), Pos2::new(inner.left() + width, y));
                }
            }
        }
    }

    /// An arrow with a head at each end, in front of the line spacing control.
    pub fn line_spacing(painter: &egui::Painter, centre: Pos2, color: Color32) {
        let stroke = Stroke::new(1.3, color);
        let top = Pos2::new(centre.x, centre.y - 5.0);
        let bottom = Pos2::new(centre.x, centre.y + 5.0);
        painter.line_segment([top, bottom], stroke);
        for (point, direction) in [(top, 1.0), (bottom, -1.0)] {
            painter.line_segment(
                [point, Pos2::new(point.x - 2.2, point.y + 2.6 * direction)],
                stroke,
            );
            painter.line_segment(
                [point, Pos2::new(point.x + 2.2, point.y + 2.6 * direction)],
                stroke,
            );
        }
    }
}
