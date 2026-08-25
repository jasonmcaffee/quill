//! The menu bar drawn inside the window, which is what Windows uses.
//!
//! On macOS the menus belong in the bar along the top of the screen instead, so this is not drawn there;
//! `services::native_menu` builds that one. Both are built from the same [`crate::app::actions::menus`],
//! so `File` holds the same entries with the same shortcuts either way.
//!
//! `Quill` is first and sits at the very left of the bar, so it reads `Quill  File  Edit  View`, which is
//! what `tasks/improvements.md` asks for. When the menus are in the window the three window buttons move
//! to the right hand end of the bar, which is where Windows puts them, because the left is taken.

use egui::{Pos2, Rect, Stroke, Vec2};

use crate::app::actions::{Action, Entry, Menu};
use crate::components::controls;
use crate::theme::color;

/// How much room the bar needs, so the title bar can leave it clear.
pub fn width(menus: &[Menu]) -> f32 {
    menus.iter().map(|menu| button_width(&menu.name) + GAP).sum::<f32>() + LEFT
}

const LEFT: f32 = 14.0;
const GAP: f32 = 4.0;

fn button_width(name: &str) -> f32 {
    // Roughly seven points a letter at 12.5 point, plus room either side. Measuring the text would need a
    // painter, and the bar is drawn before one is to hand.
    (name.chars().count() as f32 * 7.0 + 18.0).max(40.0)
}

/// Draw the bar into `area` and return what was chosen.
pub fn show(ui: &mut egui::Ui, area: Rect, menus: &[Menu]) -> Option<Action> {
    let mut chosen = None;
    let mut pen = area.left() + LEFT;
    for menu in menus {
        let width = button_width(&menu.name);
        let button = Rect::from_min_size(
            Pos2::new(pen, area.center().y - 11.0),
            Vec2::new(width, 22.0),
        );
        // The application's own menu is drawn a little brighter, as macOS draws it.
        let response = controls::bar_button(ui, button, &menu.name, menu.name == "Quill");
        if let Some(action) = popup(ui, &response, &menu.entries) {
            chosen = Some(action);
        }
        pen += width + GAP;
    }
    chosen
}

/// The list that drops down from one of the words in the bar.
fn popup(ui: &mut egui::Ui, response: &egui::Response, entries: &[Entry]) -> Option<Action> {
    egui::Popup::from_toggle_button_response(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .frame(
            egui::Frame::popup(ui.style())
                .fill(color::MENU)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER))
                .inner_margin(6),
        )
        // Wide enough that the longest entry and its shortcut do not meet in the middle. `Open Folder in New
        // Window` at `Cmd+Option+O` is the longest pair, and at 260 points they overlapped, which a
        // screenshot showed.
        .width(340.0)
        .show(|ui| rows(ui, entries, 0.0))
        .and_then(|inner| inner.inner)
}

/// The rows of one menu.
///
/// A menu inside a menu is drawn as a heading with its entries indented under it rather than as a second
/// list that opens sideways. Recent Projects is the only one, it holds a short list, and a heading with
/// rows under it needs no hovering to reach. The macOS menu bar does have a real submenu there, because
/// that is what the platform draws.
fn rows(ui: &mut egui::Ui, entries: &[Entry], indent: f32) -> Option<Action> {
    let mut chosen = None;
    for entry in entries {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                let keys = shortcut.map(|shortcut| shortcut.label()).unwrap_or_default();
                if controls::menu_row(ui, name, &keys, *enabled, *checked, indent) {
                    chosen = Some(action.clone());
                }
            }
            Entry::Submenu { name, entries } => {
                controls::menu_heading(ui, name, indent);
                if let Some(action) = rows(ui, entries, indent + 14.0) {
                    chosen = Some(action);
                }
            }
        }
    }
    chosen
}
