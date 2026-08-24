//! The bar along the top of the window.
//!
//! Quill draws its own, because the design asks for the file name centred with its folder after it and an
//! amber dot when there are unsaved changes, and because the window has no operating system title bar:
//! the rounded corners and the transparency need the decorations turned off.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::theme::{color, size};

/// What the user asked the window to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TitleBarOutcome {
    pub close: bool,
    pub minimise: bool,
    pub toggle_maximise: bool,
    /// Something chosen from the File menu.
    pub file_action: Option<FileAction>,
}

/// An entry in the File menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// Choose a folder and show it in the explorer.
    OpenFolder,
    /// Choose a file and open it in the editor.
    OpenFile,
    Save,
    SaveAs,
}

/// Draw the title bar into `area`.
///
/// `name` is the file name, `folder` is the folder it sits in, and `unsaved` puts an amber dot after them.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    folder: Option<&str>,
    unsaved: bool,
    opacity: f32,
) -> TitleBarOutcome {
    // The modifier is spelled out rather than drawn as a symbol. The Apple command symbol at U+2318 is in
    // egui's fonts but the shift symbol at U+21E7 is not, and it came out as an empty box. Mixing one
    // symbol with one word reads worse than spelling both, and words work on either platform.
    let modifier = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    let mut outcome = TitleBarOutcome::default();
    let painter = ui.painter_at(area);
    // The top two corners are rounded to match the window; the bottom two are square because the toolbar
    // sits directly underneath.
    painter.rect_filled(
        area,
        CornerRadius {
            nw: size::WINDOW_CORNER,
            ne: size::WINDOW_CORNER,
            sw: 0,
            se: 0,
        },
        crate::theme::faded(color::TITLE_BAR, opacity),
    );

    // The three window buttons, in the order macOS puts them.
    let buttons = [
        (color::CLOSE, "Close"),
        (color::MINIMISE, "Minimise"),
        (color::MAXIMISE, "Maximise"),
    ];
    let mut clicked = None;
    for (index, (fill, label)) in buttons.into_iter().enumerate() {
        let centre = Pos2::new(area.left() + 22.0 + index as f32 * 21.0, area.center().y);
        let hit = Rect::from_center_size(centre, Vec2::splat(16.0));
        let response = ui
            .interact(hit, ui.id().with(("window-button", index)), Sense::click())
            .on_hover_text(label);
        painter.circle_filled(centre, 6.5, fill);
        if response.hovered() {
            painter.circle_stroke(centre, 6.5, Stroke::new(1.0, Color32::from_black_alpha(90)));
        }
        // The accessible name is the plain word, so a test can ask for the Close button by name.
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        if response.clicked() {
            clicked = Some(index);
        }
    }
    match clicked {
        Some(0) => outcome.close = true,
        Some(1) => outcome.minimise = true,
        Some(2) => outcome.toggle_maximise = true,
        _ => {}
    }

    // The File menu, after the window buttons. Quill draws its own title bar, so there is no operating
    // system menu bar to put this in.
    let menu_rect = Rect::from_min_size(Pos2::new(area.left() + 78.0, area.center().y - 11.0), Vec2::new(46.0, 22.0));
    let menu_response = ui
        .interact(menu_rect, ui.id().with("file-menu"), Sense::click())
        .on_hover_text("File");
    if menu_response.hovered() {
        painter.rect_filled(menu_rect, CornerRadius::same(4), color::CONTROL);
    }
    let label = painter.layout_no_wrap(
        "File".to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_CONTROL,
    );
    painter.galley(
        Pos2::new(
            menu_rect.center().x - label.size().x / 2.0,
            menu_rect.center().y - label.size().y / 2.0,
        ),
        label,
        color::TEXT_CONTROL,
    );
    menu_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "File"));

    let chosen = egui::Popup::from_toggle_button_response(&menu_response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .frame(
            egui::Frame::popup(ui.style())
                .fill(color::MENU)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER))
                .inner_margin(6),
        )
        .width(240.0)
        .show(|ui| {
            let mut chosen = None;
            for (action, name, shortcut) in [
                (FileAction::OpenFolder, "Open Folder", format!("{modifier}+Shift+O")),
                (FileAction::OpenFile, "Open File", format!("{modifier}+O")),
                (FileAction::Save, "Save", format!("{modifier}+S")),
                (FileAction::SaveAs, "Save As", format!("{modifier}+Shift+S")),
            ] {
                if menu_entry(ui, name, &shortcut) {
                    chosen = Some(action);
                }
                if action == FileAction::OpenFile {
                    ui.separator();
                }
            }
            chosen
        })
        .and_then(|inner| inner.inner);
    outcome.file_action = chosen;

    // The file name, centred, with the folder after it in a dimmer colour and then the unsaved dot.
    let font = egui::FontId::proportional(13.0);
    let name_galley = painter.layout_no_wrap(name.to_owned(), font.clone(), color::TEXT_STRONG);
    let folder_text = folder.map(|folder| format!("  \u{2014} {folder}")).unwrap_or_default();
    let folder_galley = painter.layout_no_wrap(folder_text, font.clone(), color::TEXT_DIM);
    let dot_width = if unsaved { 16.0 } else { 0.0 };
    let total = name_galley.size().x + folder_galley.size().x + dot_width;
    let mut pen = area.center().x - total / 2.0;
    let baseline_y = area.center().y - name_galley.size().y / 2.0;
    painter.galley(Pos2::new(pen, baseline_y), name_galley.clone(), color::TEXT_STRONG);
    pen += name_galley.size().x;
    painter.galley(Pos2::new(pen, baseline_y), folder_galley.clone(), color::TEXT_DIM);
    pen += folder_galley.size().x;
    if unsaved {
        painter.circle_filled(Pos2::new(pen + 8.0, area.center().y), 4.0, color::UNSAVED);
    }

    // Dragging anywhere else on the bar moves the window, which is what a title bar is for.
    let drag_area = Rect::from_min_max(
        Pos2::new(area.left() + 132.0, area.top()),
        Pos2::new(area.right(), area.bottom()),
    );
    let drag = ui.interact(drag_area, ui.id().with("title-drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        outcome.toggle_maximise = true;
    }

    outcome
}

/// One row of the File menu: its name on the left and its keyboard shortcut on the right.
fn menu_entry(ui: &mut egui::Ui, name: &str, shortcut: &str) -> bool {
    let height = 24.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), color::SELECTED_ROW);
    }
    let painter = ui.painter();
    let label = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_CONTROL,
    );
    painter.galley(
        Pos2::new(rect.left() + 8.0, rect.center().y - label.size().y / 2.0),
        label,
        color::TEXT_CONTROL,
    );
    let keys = painter.layout_no_wrap(
        shortcut.to_owned(),
        egui::FontId::proportional(11.5),
        color::TEXT_FAINT,
    );
    painter.galley(
        Pos2::new(rect.right() - 8.0 - keys.size().x, rect.center().y - keys.size().y / 2.0),
        keys,
        color::TEXT_FAINT,
    );
    // The accessible name is the plain wording, so a test can ask for `Open Folder` by name.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response.clicked()
}

/// The line under the toolbar and above the status bar, and the line between the explorer and the editor.
pub fn divider(painter: &egui::Painter, from: Pos2, to: Pos2) {
    painter.line_segment([from, to], Stroke::new(1.0, color::DIVIDER));
}
