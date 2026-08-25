//! The bar along the top of the window.
//!
//! Quill draws its own, because the design asks for the file name centred with its folder after it and an
//! amber dot when there are unsaved changes, and because the window has no operating system title bar:
//! the rounded corners and the transparency need the decorations turned off.
//!
//! Where the menus and the window buttons go depends on the platform, and the two swap sides.
//!
//! On macOS the menus belong in the bar along the top of the screen, so this bar holds no menus and the
//! three window buttons sit at the left, where macOS puts them. Everywhere else the menus are drawn in
//! this bar, starting with `Quill` at the very left, and the window buttons move to the right hand end,
//! where Windows puts them. Either way `Quill` is the first thing in the top bar, which is what
//! `tasks/improvements.md` asks for.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::actions::{Action, Menu};
use crate::components::menu_bar;
use crate::theme::{color, size};

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
    placement: MenuPlacement,
    menus: &[Menu],
) -> TitleBarOutcome {
    let mut outcome = TitleBarOutcome::default();
    let painter = ui.painter_at(area);
    // The top two corners are rounded to match the window; the bottom two are square because the toolbar
    // sits directly underneath.
    painter.rect_filled(
        area,
        CornerRadius { nw: size::WINDOW_CORNER, ne: size::WINDOW_CORNER, sw: 0, se: 0 },
        crate::theme::faded(color::TITLE_BAR, opacity),
    );

    // The three window buttons: at the left with the menus in the screen's own bar, at the right when the
    // menus are in this one.
    let buttons_at_left = placement == MenuPlacement::Native;
    let first_centre = if buttons_at_left {
        Pos2::new(area.left() + 22.0, area.center().y)
    } else {
        Pos2::new(area.right() - 64.0, area.center().y)
    };
    let buttons: [(Color32, &str); 3] = if buttons_at_left {
        // The order macOS puts them in.
        [(color::CLOSE, "Close"), (color::MINIMISE, "Minimise"), (color::MAXIMISE, "Maximise")]
    } else {
        // The order Windows puts them in.
        [(color::MINIMISE, "Minimise"), (color::MAXIMISE, "Maximise"), (color::CLOSE, "Close")]
    };
    for (index, (fill, label)) in buttons.into_iter().enumerate() {
        let centre = Pos2::new(first_centre.x + index as f32 * 21.0, first_centre.y);
        let hit = Rect::from_center_size(centre, Vec2::splat(16.0));
        let response = ui
            .interact(hit, ui.id().with(("window-button", label)), Sense::click())
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

    // The menus, when they are drawn here.
    let mut drag_from = area.left() + 132.0;
    let mut drag_to = area.right();
    match placement {
        MenuPlacement::InWindow => {
            let bar = Rect::from_min_max(
                area.min,
                Pos2::new(area.left() + menu_bar::width(menus), area.bottom()),
            );
            outcome.action = menu_bar::show(ui, bar, menus);
            drag_from = bar.right() + 6.0;
            drag_to = first_centre.x - 16.0;
        }
        MenuPlacement::Native => {}
    }

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
    if drag_to > drag_from {
        let drag_area =
            Rect::from_min_max(Pos2::new(drag_from, area.top()), Pos2::new(drag_to, area.bottom()));
        let drag = ui.interact(drag_area, ui.id().with("title-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            outcome.toggle_maximise = true;
        }
    }

    outcome
}

/// The line under the toolbar and above the status bar, and the line between the explorer and the editor.
pub fn divider(painter: &egui::Painter, from: Pos2, to: Pos2) {
    painter.line_segment([from, to], Stroke::new(1.0, color::DIVIDER));
}
