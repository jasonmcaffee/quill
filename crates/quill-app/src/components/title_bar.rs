//! The bar along the top of the window.
//!
//! Quill draws its own, because the window has no operating system title bar: the rounded corners and the
//! translucent background need the decorations turned off.
//!
//! It holds four things, and where each of them goes depends on the platform, because the menus and the
//! window buttons swap sides.
//!
//! On macOS the menus belong in the bar along the top of the screen, so this bar holds no menus and the
//! three window buttons sit at the left, where macOS puts them. Everywhere else the menus are drawn here,
//! starting with `Quill` at the very left, and the window buttons move to the right hand end, where
//! Windows puts them. Either way `Quill` is the first thing in the top bar, which is what
//! `tasks/improvements.md` asks for.
//!
//! **The project's name comes after the menus**, which is what `task-1658` asks for — after `Git`, the
//! last of them, and after the window buttons on macOS where there are no menus here to follow.
//!
//! **The open file's name is not here at all.** It used to be centred, with its folder after it and an
//! amber dot for unsaved changes, and all three are now somewhere better: the file's name is on its tab,
//! the dot is on the tab as well, and the folder was the project, which is what the bar now says once
//! rather than repeating on every file.
//!
//! **The text tools sit at the right hand end**, in front of the window buttons, and the **run
//! widget** sits in front of *them*, between the project's name and the tools — which is what
//! `task-1683` asks for. The bar draws neither — `components::text_tools` and
//! `components::run_widget` do — but it decides where both go, through [`tools_rect`] and
//! [`run_rect`], so that nothing it draws itself and nothing that is dragged to move the window
//! ever runs underneath one. **Neither ever changes the bar's height**, which is the whole reason
//! the tools were moved here in the first place.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::actions::{Action, Menu};
use crate::components::menu_bar;
use crate::theme::{color, size};

/// From the middle of one window button to the middle of the next.
const BUTTON_STEP: f32 = 21.0;
/// How much room the three window buttons want either side of themselves.
const BUTTON_MARGIN: f32 = 16.0;

/// Where this window's menus are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPlacement {
    /// Drawn in the title bar, which is what Windows does.
    InWindow,
    /// In the bar along the top of the screen, which is what macOS does.
    Native,
}

impl MenuPlacement {
    /// What this platform does. macOS has a menu bar of its own; nothing else does.
    pub fn for_this_platform() -> Self {
        if cfg!(target_os = "macos") {
            MenuPlacement::Native
        } else {
            MenuPlacement::InWindow
        }
    }
}

/// What the user asked the window to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TitleBarOutcome {
    pub close: bool,
    pub minimise: bool,
    pub toggle_maximise: bool,
    /// Something chosen from a menu in the bar.
    pub action: Option<Action>,
}

/// Where the middle of the first window button is.
///
/// At the left with the menus in the screen's own bar, at the right when the menus are in this one.
fn first_button(area: Rect, placement: MenuPlacement) -> Pos2 {
    if placement == MenuPlacement::Native {
        Pos2::new(area.left() + 22.0, area.center().y)
    } else {
        Pos2::new(area.right() - 64.0, area.center().y)
    }
}

/// The rectangle the text tools are drawn into, `width` points wide.
///
/// They finish in front of the window buttons where those are at the right, and against the right hand
/// edge where they are not. A width of nothing gives an empty rectangle there, which is what a file with
/// no tools gets.
pub fn tools_rect(area: Rect, placement: MenuPlacement, width: f32) -> Rect {
    let right = match placement {
        MenuPlacement::InWindow => first_button(area, placement).x - BUTTON_MARGIN - 10.0,
        MenuPlacement::Native => area.right() - BUTTON_MARGIN,
    };
    Rect::from_min_max(
        Pos2::new(right - width, area.top()),
        Pos2::new(right, area.bottom()),
    )
}

/// The rectangle the run widget is drawn into, `run_width` points wide.
///
/// In front of the text tools, so the bar reads project, run, tools, window buttons — the run
/// controls beside the file's, and both clear of the buttons. With no tools it takes the room they
/// would have had, which is what a source file's title bar looks like.
pub fn run_rect(area: Rect, placement: MenuPlacement, tools_width: f32, run_width: f32) -> Rect {
    let tools = tools_rect(area, placement, tools_width);
    let right = if tools_width > 0.0 { tools.left() - 12.0 } else { tools.right() };
    Rect::from_min_max(
        Pos2::new(right - run_width, area.top()),
        Pos2::new(right, area.bottom()),
    )
}

/// Draw the title bar into `area`.
///
/// `project` is the name of the folder that is open; `tools_width` and `run_width` are how much
/// room to leave clear at the right for the text tools and the run widget, which the window draws
/// over the top afterwards.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    project: Option<&str>,
    opacity: f32,
    placement: MenuPlacement,
    menus: &[Menu],
    tools_width: f32,
    run_width: f32,
) -> TitleBarOutcome {
    let mut outcome = TitleBarOutcome::default();
    let painter = ui.painter_at(area);
    // The top two corners are rounded to match the window; the bottom two are square because the tabs
    // sit directly underneath.
    painter.rect_filled(
        area,
        CornerRadius { nw: size::WINDOW_CORNER, ne: size::WINDOW_CORNER, sw: 0, se: 0 },
        crate::theme::faded(color::TITLE_BAR, opacity),
    );

    let buttons_at_left = placement == MenuPlacement::Native;
    let first_centre = first_button(area, placement);
    let buttons: [(Color32, &str); 3] = if buttons_at_left {
        // The order macOS puts them in.
        [(color::CLOSE, "Close"), (color::MINIMISE, "Minimise"), (color::MAXIMISE, "Maximise")]
    } else {
        // The order Windows puts them in.
        [(color::MINIMISE, "Minimise"), (color::MAXIMISE, "Maximise"), (color::CLOSE, "Close")]
    };
    for (index, (fill, label)) in buttons.into_iter().enumerate() {
        let centre = Pos2::new(first_centre.x + index as f32 * BUTTON_STEP, first_centre.y);
        let hit = Rect::from_center_size(centre, Vec2::splat(16.0));
        // `Sense::CLICK` and not `Sense::click()`: the same clicks, but the button cannot take
        // keyboard focus. A focused button is pressed by `Space` and by `Enter`, and these three
        // close, minimise and resize the window — so a focus that reached one of them turned a space
        // typed in a file into the window closing. `app::hold_the_keyboard` is what stops the focus
        // wandering in the first place; this is here so that even if it ever does, the keys it arrives
        // with cannot do something a person cannot undo.
        let response = ui
            .interact(hit, ui.id().with(("window-button", label)), Sense::CLICK)
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
            match label {
                "Close" => outcome.close = true,
                "Minimise" => outcome.minimise = true,
                _ => outcome.toggle_maximise = true,
            }
        }
    }

    // The menus, when they are drawn here, and then the project's name after them.
    let mut name_from = first_centre.x + 2.0 * BUTTON_STEP + BUTTON_MARGIN;
    let mut drag_to = area.right();
    match placement {
        MenuPlacement::InWindow => {
            let bar = Rect::from_min_max(
                area.min,
                Pos2::new(area.left() + menu_bar::width(menus), area.bottom()),
            );
            outcome.action = menu_bar::show(ui, bar, menus);
            name_from = bar.right() + 10.0;
            drag_to = first_centre.x - BUTTON_MARGIN;
        }
        MenuPlacement::Native => {}
    }

    // The project's name, cut short with an ellipsis rather than run underneath the run widget or
    // the tools. Whichever of the two is furthest left is where it has to stop.
    let tools = tools_rect(area, placement, tools_width);
    let run = run_rect(area, placement, tools_width, run_width);
    let claimed = if run_width > 0.0 {
        Some(run.left())
    } else if tools_width > 0.0 {
        Some(tools.left())
    } else {
        None
    };
    let name_to = match claimed {
        Some(left) => left - 12.0,
        None => drag_to,
    };
    if let Some(project) = project {
        let galley = painter.layout(
            project.to_owned(),
            egui::FontId::proportional(13.0),
            color::TEXT_STRONG,
            (name_to - name_from).max(1.0),
        );
        if name_to > name_from {
            painter.galley(
                Pos2::new(name_from, area.center().y - galley.size().y / 2.0),
                galley.clone(),
                color::TEXT_STRONG,
            );
            name_from += galley.size().x;
        }
    }

    // Dragging anywhere else on the bar moves the window, which is what a title bar is for. It stops
    // short of whichever control reaches furthest left, so pressing the `F` or the play button
    // never begins a drag of the window behind it.
    let drag_to = drag_to.min(claimed.map(|left| left - 4.0).unwrap_or(drag_to));
    let drag_from = name_from + 8.0;
    if drag_to > drag_from {
        let drag_area =
            Rect::from_min_max(Pos2::new(drag_from, area.top()), Pos2::new(drag_to, area.bottom()));
        // Clicks and drags, and no keyboard focus, for the reason given above the window buttons: a
        // double click here resizes the window, and `Enter` on a focused widget counts as a click.
        let drag = ui.interact(
            drag_area,
            ui.id().with("title-drag"),
            Sense::CLICK | Sense::DRAG,
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            outcome.toggle_maximise = true;
        }
    }

    outcome
}

/// The line under a bar and above the status bar, and the line between the explorer and the editor.
pub fn divider(painter: &egui::Painter, from: Pos2, to: Pos2) {
    painter.line_segment([from, to], Stroke::new(1.0, color::DIVIDER));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(1180.0, size::TITLE_BAR))
    }

    #[test]
    fn the_tools_stop_before_the_window_buttons_when_the_buttons_are_at_the_right() {
        let area = bar();
        let tools = tools_rect(area, MenuPlacement::InWindow, 124.0);
        let nearest_button = first_button(area, MenuPlacement::InWindow).x - 8.0;
        assert!(
            tools.right() < nearest_button,
            "the tools finish at {} and the first button starts at {nearest_button}",
            tools.right()
        );
        assert_eq!(tools.width(), 124.0);
    }

    #[test]
    fn the_tools_go_against_the_right_edge_when_the_buttons_are_at_the_left() {
        let area = bar();
        let tools = tools_rect(area, MenuPlacement::Native, 124.0);
        assert_eq!(tools.right(), area.right() - BUTTON_MARGIN);
        assert!(tools.left() > first_button(area, MenuPlacement::Native).x, "clear of the buttons");
    }

    #[test]
    fn a_file_with_no_tools_leaves_an_empty_rectangle_rather_than_a_gap() {
        let tools = tools_rect(bar(), MenuPlacement::InWindow, 0.0);
        assert_eq!(tools.width(), 0.0);
    }

    #[test]
    fn the_run_widget_sits_in_front_of_the_text_tools() {
        // The bar reads project, run, tools, window buttons.
        let area = bar();
        let tools = tools_rect(area, MenuPlacement::InWindow, 124.0);
        let run = run_rect(area, MenuPlacement::InWindow, 124.0, 120.0);
        assert!(run.right() < tools.left(), "run finishes at {} and tools start at {}", run.right(), tools.left());
        assert_eq!(run.width(), 120.0);
    }

    #[test]
    fn with_no_text_tools_the_run_widget_takes_the_room_they_would_have_had() {
        let area = bar();
        let tools = tools_rect(area, MenuPlacement::InWindow, 0.0);
        let run = run_rect(area, MenuPlacement::InWindow, 0.0, 120.0);
        assert_eq!(run.right(), tools.right(), "a source file's title bar");
        assert!(run.left() < first_button(area, MenuPlacement::InWindow).x);
    }
}
