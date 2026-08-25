//! The small controls more than one part of the window needs: a dropdown, a menu row, an icon button
//! and a divider.
//!
//! They live here rather than in the toolbar because the toolbar is no longer the only thing that needs
//! them: the Settings window has dropdowns, and the menu bar has menu rows. One copy means the dropdown
//! in Settings and the dropdown in the toolbar cannot drift apart.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::actions::{Action, Entry};
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

/// The rows of one menu, whether it hangs from the bar or from a right click.
///
/// A menu inside a menu is drawn as a heading with its entries indented under it rather than as a
/// second list that opens sideways. Recent Projects and the explorer's `Git` submenu are the only
/// ones, both hold a short list, and a heading with rows under it needs no hovering to reach. The
/// macOS menu bar does have a real submenu there, because that is what the platform draws.
///
/// This lives here rather than in `menu_bar` because there are three menus in Quill now — the bar
/// inside the window, the explorer's context menu and the gutter's — and one renderer is what stops
/// them growing three row heights.
pub fn menu_rows(ui: &mut egui::Ui, entries: &[Entry], indent: f32) -> Option<Action> {
    // A menu taller than the window scrolls rather than running off the bottom of it. The Git menu
    // has twenty-two entries and does not fit in a small window; before this, its last few could
    // not be reached at all.
    let room = (ui.ctx().content_rect().height() - 120.0).max(180.0);
    // egui puts `item_spacing.y` between every row, so a count of row heights alone comes out short
    // by a third and the menu is decided to fit when it does not.
    let gap = ui.spacing().item_spacing.y;
    let height: f32 = entries
        .iter()
        .map(|entry| match entry {
            Entry::Separator => 8.0 + gap,
            Entry::Item { .. } => 24.0 + gap,
            Entry::Submenu { entries, .. } => 22.0 + gap + entries.len() as f32 * (24.0 + gap),
        })
        .sum();
    if height > room {
        return egui::ScrollArea::vertical()
            .max_height(room)
            // Without this the box comes out about two thirds of what it was allowed, because a
            // scroll area inside a popup measures itself against the popup's own idea of how much
            // room there is rather than against the number it was given.
            .min_scrolled_height(room)
            .id_salt("quill-menu-scroll")
            .show(ui, |ui| rows(ui, entries, indent))
            .inner;
    }
    rows(ui, entries, indent)
}

/// The rows themselves, once it has been decided whether they scroll.
fn rows(ui: &mut egui::Ui, entries: &[Entry], indent: f32) -> Option<Action> {
    let mut chosen = None;
    for entry in entries {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                let keys = shortcut.map(|shortcut| shortcut.label()).unwrap_or_default();
                if menu_row(ui, name, &keys, *enabled, *checked, indent) {
                    chosen = Some(action.clone());
                }
            }
            Entry::Submenu { name, entries } => {
                menu_heading(ui, name, indent);
                if let Some(action) = rows(ui, entries, indent + 14.0) {
                    chosen = Some(action);
                }
            }
        }
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
