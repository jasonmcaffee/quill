//! The thin rail down the far left of the window.
//!
//! `task-1658` asks for what the reference editor has: a narrow strip holding one button for each pane, so a pane can
//! be put away and brought back without going to a menu, and the buttons stay where they are whether the
//! pane is showing or not. Unluminous's is narrower than the reference editor's — 36 points against about 40 — because it
//! holds three buttons rather than a dozen and does not need the room.
//!
//! Two groups. The panes that live at the side are at the top, in the order they are in the window: the
//! explorer, then git. The terminal lives along the bottom of the window, so its button is at the bottom
//! of the rail, which is where `task-1658`'s reference capture puts it.
//!
//! `task-1683` added a second button to the bottom group, `Run tile`, **above** `Terminal tile`, and
//! `task-1687` a third, `Debug tile`, above that. The bottom of the window shows **one** of the three
//! and never two stacked, so pressing any of them shows its own tile and puts the others away —
//! which the window settles, not this file.
//!
//! Since `task-1697` a panel is not always where its button says it is: the terminal can be dragged
//! to the right and the explorer to the bottom. **The rail does not follow it**, and that is a
//! decision rather than an oversight — the reference editor moves a tool window's stripe button to the side the
//! window is on, and Unluminous has one rail on one edge, so its two groups say what a panel *is* (a
//! list, or a tile with a grid in it) rather than where it happens to be. A rail that reshuffled
//! itself on every drag would be a second thing moving while somebody was moving the first.
//!
//! What the rail did gain is a **right click**: it opens the panel's own menu, which is the only way
//! to move a panel that has been put away, since a panel that is not showing has no header to grab.
//!
//! A button that is on is the pill every list in Unluminous draws for its chosen row — `SELECTED_ROW`, the row
//! inset and rounded — rather than a filled `ACCENT` square. Three bright blue squares in a rail that is
//! nearly always in that state would be the loudest thing in the window, and the pane being open is a
//! state rather than a press.
//!
//! The rail is also the only way back once a pane has been put away. There used to be a small
//! disclosure button floating over the top left of the editing area for exactly that, and it is gone:
//! the rail does the job, always in the same place whether the explorer is showing or not, which is the
//! point of having one.
//!
//! Nothing here changes anything. Each button reports the `Action` it stands for, so the rail, the menus
//! and the keyboard all go down the one path in `UnluminousApp::run_action`.

use egui::{CornerRadius, Pos2, Rect, Sense, Vec2};

use crate::app::actions::{Action, GitAction};
use crate::theme::{color, icon, size};

/// How big one button in the rail is, and how far apart two of them are.
const BUTTON: f32 = 24.0;
const STEP: f32 = 30.0;
/// Space between the top or bottom edge of the rail and the first button.
const MARGIN: f32 = 8.0;

/// What the rail needs to know to draw itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailState {
    pub explorer_visible: bool,
    /// Whether the editing area — the pane holding the tabs — is showing. `task-28` asks for its button under
    /// the folder one, so it is the second in the top group.
    pub editor_visible: bool,
    /// True when the commit panel is open.
    pub git_open: bool,
    /// False outside a repository, which dims the git button rather than removing it — the same rule
    /// the Git menu already follows.
    pub in_repository: bool,
    pub terminal_visible: bool,
    /// True when the run tile is the one showing at the bottom of the window.
    pub run_visible: bool,
    /// True when the debug tile is the one showing along the bottom.
    pub debug_visible: bool,
}

/// A button the rail draws for a pane a plugin contributed.
///
/// The rail's own two groups say what a panel **is** — a list at the top, a character grid at the
/// bottom — and a contributed button joins the group its manifest named. It is drawn after Unluminous's own,
/// so installing a plugin never moves a button somebody's hand already knows.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginButton {
    /// What the tooltip says, from `pane.label`.
    pub label: String,
    /// Which drawn icon, from `pane.icon`.
    pub icon: String,
    /// True when the pane is showing.
    pub on: bool,
    /// Which of the rail's two groups it is in.
    pub bottom: bool,
    /// The pane's `<plugin id>/<pane id>`, or a tab's own key, which is what the action carries.
    pub key: String,
    /// Which dock slot it is, so a right click opens the right panel's menu.
    pub slot: usize,
    /// What pressing it opens.
    pub opens: Opens,
}

/// Whether a plugin's rail button opens a docked pane or a tab in the editing area.
///
/// **A tab needs a button too**, which it did not have: the rail was built from the panes alone, so
/// Agent-Tasks — which contributes a tab rather than a pane, because a board needs the whole editing
/// area and not a 420 point column — could only be reached from the `Plugins` menu. `task-28` asks for
/// it on the rail under the chat icon, which is where a `top` group button lands.
///
/// A tab has no side and no dock slot, so a right click on one opens **no** menu: there is nothing to
/// move it to, which is the same reason the `Version Control` and `Editing Area` buttons open none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    Pane,
    Tab,
}

/// What the rail reported this frame.
///
/// Two things rather than one since `task-1697`: a **left** click is still the action the button
/// stands for, and a **right** click opens that panel's own menu — the four `Move to` rows and
/// `Reset Panel Layout`. The rail is a second place to reach a panel's menu because it is the one
/// place a panel can be reached when it is put away, and because a header that is being used as a
/// handle is a header somebody is already pointing at.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RailOutcome {
    pub chosen: Option<Action>,
    /// A button was right clicked: where the pointer was, and which panel it is about. The git
    /// button opens no menu, because the commit panel is a modal rather than a docked panel.
    pub menu: Option<(Pos2, crate::app::dock::Panel)>,
}

/// Draw the rail into `area`, and report what was pressed.
pub fn show(ui: &mut egui::Ui, area: Rect, state: RailState, opacity: f32) -> RailOutcome {
    show_with(ui, area, state, opacity, &[])
}

/// The same, with the buttons the plugins that are switched on contributed.
pub fn show_with(
    ui: &mut egui::Ui,
    area: Rect,
    state: RailState,
    opacity: f32,
    plugins: &[PluginButton],
) -> RailOutcome {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::explorer_footer(), opacity));
    painter.line_segment(
        [Pos2::new(area.right(), area.top()), Pos2::new(area.right(), area.bottom())],
        egui::Stroke::new(1.0, color::divider()),
    );

    let mut outcome = RailOutcome::default();
    // Six points in from the left rather than centred in the rail, because the outer six belong to
    // `components::resize_edges` and a button that reached into them would lose those clicks to the
    // window's own resize grip. The six left at the right hand side make it look centred anyway.
    let centre_x = area.left() + crate::components::resize_edges::EDGE + BUTTON / 2.0;

    // The panes at the side, from the top.
    let top: [(
        &str,
        fn(&egui::Painter, Pos2, egui::Color32),
        bool,
        bool,
        Action,
        Option<crate::app::dock::Panel>,
    ); 3] = [
        (
            "Project",
            icon::folder,
            state.explorer_visible,
            true,
            Action::ToggleExplorer,
            Some(crate::app::dock::Panel::Explorer),
        ),
        (
            // **Under the folder icon**, which is where `task-28` asks for it. Named `Editing Area` rather
            // than `Editor` or `Files`: `Editor` is the Settings window's own page for the same reason
            // `Project` above is the explorer, and no two controls in one window may share a name.
            //
            // It is not a `Panel`, so a right click on it opens no menu: the editing area is what is left when
            // the panels have taken their room, and there is no edge to move it to.
            "Editing Area",
            icon::editing_area,
            state.editor_visible,
            true,
            Action::ToggleEditor,
            None,
        ),
        (
            // Not `Git`: the menu bar already has a `Git`, and no two controls in one window may share
            // a name — a test cannot tell them apart and neither can anybody reading them out. This
            // button opens the commit panel, which is the reference editor's Version Control tool window.
            "Version Control",
            icon::branch,
            state.git_open,
            state.in_repository,
            Action::Git(GitAction::Commit),
            // The commit panel is a modal rather than a docked panel, so it has no side to be moved
            // to and its button opens no menu.
            None,
        ),
    ];
    for (index, (name, draw, on, enabled, action, panel)) in top.into_iter().enumerate() {
        let centre = Pos2::new(centre_x, area.top() + MARGIN + BUTTON / 2.0 + index as f32 * STEP);
        let pressed = rail_button(ui, centre, name, draw, on, enabled);
        if pressed.clicked {
            outcome.chosen = Some(action);
        }
        if let (Some(at), Some(panel)) = (pressed.menu, panel) {
            outcome.menu = Some((at, panel));
        }
    }

    // The three tiles that live along the bottom of the window, so their buttons are at the bottom
    // of the rail, counted **up** from it so the last one is always in the corner however many there
    // are — which is what made adding the third one free. `Terminal tile` rather than `Terminal`,
    // because `Edit -> Settings` has a page called `Terminal` and the `View` menu has an entry
    // called `Terminal`, and no two controls in one window may share a name; `Run tile` and
    // `Debug tile` are named to match, and for the same reason — the `Run` menu is a control too.
    // **The terminal stays in the corner.** The list is read bottom upwards, so the first entry is
    // the one in the corner of the window — which `task-1658`'s capture put the terminal in and
    // which a dozen accepted screenshots are of. The design's §9 says the debug button goes "below
    // the run tile's", and that would have taken the corner; the corner is the older promise, so the
    // new button goes above the run one instead.
    let bottom: [(
        &str,
        fn(&egui::Painter, Pos2, egui::Color32),
        bool,
        Action,
        crate::app::dock::Panel,
    ); 3] = [
        (
            "Terminal tile",
            icon::terminal,
            state.terminal_visible,
            Action::ToggleTerminal,
            crate::app::dock::Panel::Terminal,
        ),
        (
            "Run tile",
            icon::run,
            state.run_visible,
            Action::ToggleRunTile,
            crate::app::dock::Panel::Run,
        ),
        (
            "Debug tile",
            icon::bug,
            state.debug_visible,
            Action::ToggleDebugTile,
            crate::app::dock::Panel::Debug,
        ),
    ];
    for (index, (name, draw, on, action, panel)) in bottom.into_iter().enumerate() {
        let centre =
            Pos2::new(centre_x, area.bottom() - MARGIN - BUTTON / 2.0 - index as f32 * STEP);
        let pressed = rail_button(ui, centre, name, draw, on, true);
        if pressed.clicked {
            outcome.chosen = Some(action);
        }
        if let Some(at) = pressed.menu {
            outcome.menu = Some((at, panel));
        }
    }

    // The plugins' buttons, after Unluminous's own in whichever group each manifest named. A button that
    // would not fit is not drawn: the rail is as tall as the window and a button half off the end reads
    // as a fault rather than as a full rail.
    let mut top_next = 3;
    let mut bottom_next = 3;
    for button in plugins {
        let centre = match button.bottom {
            true => {
                let at = Pos2::new(
                    centre_x,
                    area.bottom() - MARGIN - BUTTON / 2.0 - bottom_next as f32 * STEP,
                );
                bottom_next += 1;
                at
            }
            false => {
                let at = Pos2::new(centre_x, area.top() + MARGIN + BUTTON / 2.0 + top_next as f32 * STEP);
                top_next += 1;
                at
            }
        };
        if centre.y - BUTTON / 2.0 < area.top() || centre.y + BUTTON / 2.0 > area.bottom() {
            continue;
        }
        let pressed = rail_button(ui, centre, &button.label, pane_icon(&button.icon), button.on, true);
        if pressed.clicked {
            outcome.chosen = Some(match button.opens {
                Opens::Pane => Action::PluginPane { pane: button.key.clone() },
                Opens::Tab => Action::PluginTab { tab: button.key.clone() },
            });
        }
        // A tab has no side to be moved to, so no menu. See [`Opens`].
        if let (Some(at), Opens::Pane) = (pressed.menu, button.opens) {
            outcome.menu = Some((at, crate::app::dock::Panel::Plugin(button.slot as u8)));
        }
    }

    outcome
}

/// The drawing behind a `pane.icon` name.
///
/// The names are `plugins::PANE_ICONS`, and a manifest naming one Unluminous cannot draw was refused when it
/// was read, so the fallback here is never reached by a manifest that loaded.
pub fn pane_icon(name: &str) -> fn(&egui::Painter, Pos2, egui::Color32) {
    match name {
        "board" => icon::board,
        "folder" => icon::folder,
        "terminal" => icon::terminal,
        "run" => icon::run,
        "bug" => icon::bug,
        "clock" => icon::clock,
        "branch" => icon::branch,
        "tick" => icon::tick,
        "plus" => icon::plus,
        "image" => icon::image,
        "chat" => icon::chat,
        "database" => icon::database,
        "table" => icon::table,
        _ => icon::board,
    }
}

/// What one button in the rail reported.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Pressed {
    clicked: bool,
    /// Where the pointer was when it was right clicked, which opens the panel's own menu.
    menu: Option<Pos2>,
}

/// One button in the rail: a pill when what it opens is open, and a drawn icon.
fn rail_button(
    ui: &mut egui::Ui,
    centre: Pos2,
    name: &str,
    draw: fn(&egui::Painter, Pos2, egui::Color32),
    on: bool,
    enabled: bool,
) -> Pressed {
    let hit = Rect::from_center_size(centre, Vec2::splat(BUTTON));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui
        .interact(hit, ui.id().with(("activity", name)), sense)
        .on_hover_text(name);
    let painter = ui.painter();
    if on {
        painter.rect_filled(hit, CornerRadius::same(size::CONTROL_CORNER), color::selected_row());
    } else if response.hovered() && enabled {
        painter.rect_filled(hit, CornerRadius::same(size::CONTROL_CORNER), color::control());
    }
    // The rail's three states, through the palette's icon roles rather than through the text ladder they
    // used to borrow. Each role defaults to exactly the colour that was passed before `task-1776`, so
    // nothing moved when they arrived — what they buy is that a theme can tint the rail without also
    // moving every heading and every placeholder in the window.
    let tint = if !enabled {
        color::icon_disabled().gamma_multiply(0.6)
    } else if on {
        color::icon_active()
    } else {
        color::icon()
    };
    draw(painter, centre, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, name)
    });
    Pressed {
        clicked: response.clicked(),
        menu: match response.secondary_clicked() {
            true => response.interact_pointer_pos().or_else(|| response.hover_pos()),
            false => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rail has to be tall enough to hold its two top buttons and the terminal button at the
    /// bottom without them meeting. Below this the window is smaller than its own minimum, which
    /// `main.rs` sets to 400 points.
    #[test]
    fn the_rail_is_narrow_enough_to_be_a_rail_and_wide_enough_to_hold_a_button() {
        assert!(size::ACTIVITY_BAR < 40.0, "narrower than the reference editor's, which is what was asked for");
        assert!(size::ACTIVITY_BAR >= BUTTON + 4.0, "a button has to fit inside it");
        // The button starts where the window's own left resize grip stops, so the two never overlap.
        let left = crate::components::resize_edges::EDGE;
        assert!(left + BUTTON <= size::ACTIVITY_BAR, "the button has to fit clear of the resize grip");
    }
}

#[cfg(test)]
mod icon_tests {
    use super::pane_icon;
    use crate::services::plugins::PANE_ICONS;
    use crate::theme::icon;

    #[test]
    fn every_named_icon_is_actually_drawn_rather_than_falling_back_to_the_board() {
        // The registry and the drawing are two lists, and a name added to one and not the other is a
        // rail button that quietly draws the wrong picture — which is what happened the first time
        // `database` and `table` were added, and it is invisible in every test that does not open the
        // image. This is that check as an assertion.
        // A function item cast to an integer is the whole of the question here -- two names draw
        // the same picture when they are the same function -- so the lint is answered rather than
        // worked around. `fn_addr_comparisons` is about *comparing* addresses across crates, which
        // this is not: both are `fn(&Painter, Pos2, Color32)` from this module.
        #[allow(clippy::fn_to_numeric_cast_any)]
        let address = |drawing: fn(&egui::Painter, egui::Pos2, egui::Color32)| drawing as usize;
        let fallback = address(pane_icon("nothing-like-this"));
        assert_eq!(fallback, address(icon::board), "the fallback is still the board");
        for name in PANE_ICONS {
            let drawn = address(pane_icon(name));
            if *name == "board" {
                continue;
            }
            assert_ne!(drawn, fallback, "`{name}` is in PANE_ICONS with no drawing behind it");
        }
    }
}
