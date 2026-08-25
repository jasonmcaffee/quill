//! The small controls more than one part of the window needs: a dropdown, a menu row, an icon button
//! and a divider.
//!
//! They live here rather than in the toolbar because the toolbar is no longer the only thing that needs
//! them: the Settings window has dropdowns, and the menu bar has menu rows. One copy means the dropdown
//! in Settings and the dropdown in the toolbar cannot drift apart.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::theme::{color, icon, size};

/// A button showing the current value, which opens a list when clicked.
///
/// `contents` draws the list and returns what was chosen, so the caller decides what a choice is: the
/// toolbar returns a `quill_core::Command` and the Settings window returns a font size.
pub fn dropdown<T>(
    ui: &mut egui::Ui,
    area: Rect,
    value: &str,
    name: &str,
    draw: Option<fn(&egui::Painter, Pos2, Color32)>,
    contents: impl FnOnce(&mut egui::Ui) -> Option<T>,
) -> Option<T> {
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

/// One row of a menu: a tick when it is switched on, its name, and its keyboard shortcut on the right.
///
/// A row that cannot be used just now is drawn dimmed and takes no clicks, which is how a menu says that
/// there is nothing to undo. The accessible name is the plain wording, with no tick and no padding in it,
/// so a test can ask for `Open Folder` by name however the row happens to be decorated.
pub fn menu_row(
    ui: &mut egui::Ui,
    name: &str,
    shortcut: &str,
    enabled: bool,
    checked: bool,
    indent: f32,
) -> bool {
    let height = 24.0;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), sense);
    if response.hovered() && enabled {
        ui.painter().rect_filled(rect, CornerRadius::same(4), color::SELECTED_ROW);
    }
    let painter = ui.painter();
    let tint = if enabled { color::TEXT_CONTROL } else { color::TEXT_FAINT.gamma_multiply(0.6) };
    let left = rect.left() + 8.0 + indent;
    if checked {
        let tick = painter.layout_no_wrap(
            "\u{2713}".to_owned(),
            egui::FontId::proportional(12.5),
            color::ACCENT,
        );
        painter.galley(
            Pos2::new(left, rect.center().y - tick.size().y / 2.0),
            tick,
            color::ACCENT,
        );
    }
    let label = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), tint);
    painter.galley(
        Pos2::new(left + 18.0, rect.center().y - label.size().y / 2.0),
        label,
        tint,
    );
    if !shortcut.is_empty() {
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
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name)
    });
    response.clicked()
}

/// A heading inside a menu, which is what a menu inside a menu is drawn as inside the window.
pub fn menu_heading(ui: &mut egui::Ui, name: &str, indent: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let painter = ui.painter();
    let label =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(11.0), color::TEXT_DIM);
    painter.galley(
        Pos2::new(rect.left() + 8.0 + indent, rect.center().y - label.size().y / 2.0),
        label,
        color::TEXT_DIM,
    );
}

/// A small square button holding a drawn icon.
pub fn icon_button(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    draw: fn(&egui::Painter, Pos2, Color32),
) -> bool {
    let response = ui
        .interact(area, ui.id().with(("icon-button", name)), Sense::click())
        .on_hover_text(name);
    if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(4), color::CONTROL);
    }
    draw(ui.painter(), area.center(), color::TEXT_DIM);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response.clicked()
}

/// A word in a bar that opens a menu when clicked, which is what `Quill`, `File`, `Edit` and `View` are.
pub fn bar_button(ui: &mut egui::Ui, area: Rect, name: &str, strong: bool) -> egui::Response {
    let response = ui.interact(area, ui.id().with(("bar-button", name)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(4), color::CONTROL);
    }
    let tint = if strong { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    let painter = ui.painter();
    let label = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), tint);
    painter.galley(
        Pos2::new(area.center().x - label.size().x / 2.0, area.center().y - label.size().y / 2.0),
        label,
        tint,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response
}

/// A short upright line between two groups of controls.
pub fn separator(ui: &egui::Ui, x: f32, middle: f32) {
    ui.painter().line_segment(
        [Pos2::new(x, middle - 10.0), Pos2::new(x, middle + 10.0)],
        Stroke::new(1.0, color::DIVIDER),
    );
}
