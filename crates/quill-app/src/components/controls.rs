//! The small controls more than one part of the window needs: a dropdown, a menu row, an icon button
//! and a divider.
//!
//! They live here rather than in the toolbar because the toolbar is no longer the only thing that needs
//! them: the Settings window has dropdowns, and the menu bar has menu rows. One copy means the dropdown
//! in Settings and the dropdown in the toolbar cannot drift apart.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::actions::{Action, Entry};
use crate::theme::{color, icon, size};

/// The rectangle the `TextEdit` inside one of Quill's fields is given.
///
/// Every field in Quill draws its own frame — `FIELD` with a one point stroke, the corner radius the
/// style guide gives — and puts an `egui::TextEdit` inside it with `Frame::NONE`, because egui's own
/// frame is not the one the design shows. egui then lays that box out at the **top** of the
/// rectangle it is given, and with no frame there is no margin to push it down, so a rectangle the
/// height of the whole field left the words sitting against its top edge: `Filter files` was about
/// three points high in a 24 point box, on a different line from the magnifier beside it.
///
/// So a field hands its box one text row, centred in the field. One function rather than a number
/// repeated in five components, because a fifth field added later would otherwise be the fifth
/// chance to get it wrong.
///
/// `left` is how far in from the field's left edge the text starts — 26 points where there is a
/// magnifier in front of it, 8 where there is not.
pub fn field_text_rect(ui: &egui::Ui, field: Rect, left: f32) -> Rect {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let width = (field.width() - left - 8.0).max(1.0);
    Rect::from_min_size(
        Pos2::new(field.left() + left, (field.center().y - row / 2.0).round()),
        Vec2::new(width, row),
    )
}

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

/// A small icon button that opens a panel under itself and stays open until it is clicked away from.
///
/// A sibling of [`dropdown`] rather than a setting on it, because the two are different things. A
/// dropdown is a value picker: it shows the value it holds, and choosing one closes it. A flyout is
/// a panel of controls that is used several times in a row — bold, then a colour, then an alignment
/// — so it closes only when the pointer goes elsewhere, and `contents` draws whatever it likes into
/// the rectangle it is given rather than returning one choice.
///
/// `contents` is handed the `Ui` inside the panel and returns whatever the caller wants out of it.
/// The return is `None` while the panel is shut.
///
/// One thing a flyout must not hold is another popup. egui keeps at most one popup open at a time,
/// so a dropdown inside this panel would shut the panel the moment it opened.
pub fn flyout<T>(
    ui: &mut egui::Ui,
    area: Rect,
    name: &str,
    draw: fn(&egui::Painter, Pos2, Color32),
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> T,
) -> Option<T> {
    let response = ui
        .interact(area, ui.id().with(("flyout", name)), Sense::click())
        .on_hover_text(name);
    // What the panel will be by the time it is drawn: the click this frame is what toggles it, and
    // the button has to be tinted for the state it is going into rather than the one it is leaving.
    let open =
        egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) != response.clicked();
    let painter = ui.painter();
    if open {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    }
    let tint = if open { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    draw(painter, area.center(), tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), open, name)
    });
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            egui::Frame::popup(ui.style())
                .fill(color::MENU)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER)),
        )
        .width(width)
        .show(contents)
        .map(|inner| inner.inner)
}

/// A button carrying a word rather than a picture, filled when what it stands for is switched on.
///
/// The three line spacings are the only ones. They were a dropdown when they sat in the toolbar and
/// cannot stay one inside a flyout, because egui keeps one popup open at a time and opening the list
/// would shut the panel it was in. Three buttons in a row are better in a panel anyway: which
/// spacing is on can be seen without opening anything, which is how the alignments beside them
/// already work.
pub fn choice_button(ui: &mut egui::Ui, area: Rect, label: &str, active: bool) -> bool {
    let response = ui.interact(area, ui.id().with(("choice", label)), Sense::click());
    let painter = ui.painter();
    if active {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), color::CONTROL);
    } else {
        painter.rect(
            area,
            CornerRadius::same(size::CONTROL_CORNER),
            Color32::TRANSPARENT,
            Stroke::new(1.0, color::CONTROL_BORDER),
            egui::StrokeKind::Inside,
        );
    }
    let tint = if active { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    let galley =
        painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(12.5), tint);
    painter.galley(area.center() - galley.size() / 2.0, galley, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
    });
    response.clicked()
}

/// A heading beside a row of controls in a flyout, naming what the row is for.
pub fn row_label(painter: &egui::Painter, at: Pos2, name: &str) {
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(11.5), color::TEXT_DIM);
    painter.galley(Pos2::new(at.x, at.y - galley.size().y / 2.0), galley, color::TEXT_DIM);
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
        // Drawn, not the character at U+2713. No font in the stack Quill hands egui has a shape for
        // it, so it came out as the empty box a missing glyph renders as — visible against
        // `Raw Markdown` on the View menu in any capture of it, and the exact fault the style guide
        // already records for the shift symbol. `icon::tick` is the same tick every tick box in
        // Quill draws.
        icon::tick(painter, Pos2::new(left + 6.0, rect.center().y), color::ACCENT);
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
