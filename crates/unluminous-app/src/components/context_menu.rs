//! A menu drawn where the pointer is, which is what a right click opens.
//!
//! It is the same list of [`Entry`] the menu bar is built from, drawn by the same
//! [`crate::components::controls::menu_rows`], so the tick, the dimming, the shortcut column and the
//! row height cannot drift between the bar's menus and this one.
//!
//! Whether it is open is the window's state rather than egui's memory. That is what lets a test open
//! it — a screenshot test cannot press the right mouse button, and a menu that can only be reached
//! by pressing the right mouse button cannot be looked at.

use egui::{Pos2, Stroke};

use crate::app::actions::{Action, Entry};
use crate::components::controls;
use crate::theme::color;

/// How wide a context menu is. Narrower than the bar's menus, which have to fit
/// `Open Folder in New Window` next to `Cmd+Option+O`; the longest pair here is
/// `Compare with Branch or Tag...` with no shortcut at all.
const WIDTH: f32 = 300.0;

/// What happened in the menu this frame.
#[derive(Debug, Default, PartialEq)]
pub struct ContextMenuOutcome {
    /// A row was chosen.
    pub chosen: Option<Action>,
    /// The menu should be put away: a row was chosen, the pointer was clicked outside it, or Escape
    /// was pressed.
    pub close: bool,
}

/// Draw a menu whose top left corner is at `at`.
///
/// `name` separates one menu from another in egui's own bookkeeping, so the explorer's menu and the
/// gutter's do not share a rectangle when both have been open.
pub fn show(ui: &mut egui::Ui, name: &str, at: Pos2, entries: &[Entry]) -> ContextMenuOutcome {
    let mut outcome = ContextMenuOutcome::default();
    let popup = egui::Popup::new(
        egui::Id::new(("unluminous-context-menu", name)),
        ui.ctx().clone(),
        at,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .frame(
        egui::Frame::popup(ui.style())
            .fill(color::menu())
            .stroke(Stroke::new(1.0, color::control_border()))
            .inner_margin(6),
    )
    .width(WIDTH);

    if let Some(response) = popup.show(|ui| controls::menu_rows(ui, entries, 0.0)) {
        outcome.chosen = response.inner;
        outcome.close = response.response.should_close();
    }
    if outcome.chosen.is_some() {
        outcome.close = true;
    }
    // Escape puts a menu away wherever it is, which is the one thing every menu on every platform
    // agrees about.
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        outcome.close = true;
    }
    outcome
}
