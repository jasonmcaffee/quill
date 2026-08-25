//! The palette, the measurements and the drawn icons.
//!
//! Every colour here was read out of `design/intial-design-screenshot.png` rather than chosen by eye. The example at
//! `examples/sample_design.rs` reports, for each region of that image, the colour covering most of it and
//! the most saturated colour in it, which is how the accents were found. Run it with
//! `cargo run --example sample_design` to check any of these against the design again.

use egui::{Color32, CornerRadius, Stroke, Vec2};

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

    /// The oldest commit in a file, in the blame column beside the line numbers.
    ///
    /// This pair is the one part of the palette not read out of `design/intial-design-screenshot.png`,
    /// because the design has no gutter in it. They were measured out of the capture the ask came with,
    /// `tasks/quill-ide-tdd.md` section 2, in the same way: the two colours covering the annotation
    /// column of that image.
    pub const BLAME_OLD: Color32 = Color32::from_rgb(0x3C, 0x7D, 0x64);
    /// The newest commit in a file. Everything between is interpolated by rank.
    pub const BLAME_NEW: Color32 = Color32::from_rgb(0xB4, 0x58, 0x8C);

    /// A file, or a line, that git does not have yet. Measured from the commit panel in the same
    /// capture, where it is the colour of the `added` count.
    pub const GIT_ADDED: Color32 = Color32::from_rgb(0x7F, 0xCA, 0x98);
    /// A file, or a line, that differs from the version git has. The `modified` count in that capture.
    pub const GIT_MODIFIED: Color32 = Color32::from_rgb(0x4D, 0x9D, 0xC3);
    /// A file git is not tracking at all.
    pub const GIT_UNTRACKED: Color32 = Color32::from_rgb(0x9A, 0x8C, 0x5A);

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
///
/// Markdown gets the blue square because Quill treats it differently, having a preview for it. Every
/// other kind of text gets the grey one, whether Quill knows the extension or not. What the status bar
/// calls the file is decided by `services::file_kind::kind_name`, not here.
pub fn file_marker(path: &std::path::Path) -> Color32 {
    if crate::services::file_kind::is_markdown(Some(path)) {
        color::FILE_MARKDOWN
    } else {
        color::FILE_TEXT
    }
}

pub mod icon;
