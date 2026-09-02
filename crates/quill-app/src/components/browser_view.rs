//! The small Quill-owned toolbar above a native browser child view.

use egui::{Align2, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::services::browser::{BrowserCommand, BrowserPlacement, BrowserTab};
use crate::theme::{color, size};

const TOOLBAR_HEIGHT: f32 = 38.0;
const BUTTON_SIZE: f32 = 26.0;

/// What the browser toolbar asked the window to do this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub command: Option<BrowserCommand>,
    pub took_focus: bool,
}

/// The content and command of one compact navigation control.
struct Button<'a> {
    name: &'a str,
    glyph: &'a str,
    enabled: bool,
    command: BrowserCommand,
}

/// Draw navigation controls and return the rectangle reserved for the native child view.
pub fn show(ui: &mut egui::Ui, area: Rect, tab: &BrowserTab, focused: bool, showing: bool) -> (Outcome, BrowserPlacement) {
    let toolbar = Rect::from_min_size(area.min, Vec2::new(area.width(), TOOLBAR_HEIGHT));
    let browser = Rect::from_min_max(Pos2::new(area.left(), toolbar.bottom()), area.max);
    ui.painter().rect_filled(toolbar, CornerRadius::ZERO, color::toolbar());
    ui.painter().line_segment([Pos2::new(toolbar.left(), toolbar.bottom()), toolbar.right_bottom()], Stroke::new(1.0, color::divider()));
    let mut outcome = Outcome::default();
    let mut left = toolbar.left() + 6.0;
    draw_button(ui, &mut left, toolbar, Button { name: "Back", glyph: "‹", enabled: tab.can_go_back(), command: BrowserCommand::Back }, &mut outcome);
    draw_button(ui, &mut left, toolbar, Button { name: "Forward", glyph: "›", enabled: tab.can_go_forward(), command: BrowserCommand::Forward }, &mut outcome);
    draw_button(ui, &mut left, toolbar, Button { name: "Reload", glyph: "↻", enabled: true, command: BrowserCommand::Reload }, &mut outcome);
    let address = Rect::from_min_max(Pos2::new(left + 6.0, toolbar.top() + 6.0), Pos2::new(toolbar.right() - 8.0, toolbar.bottom() - 6.0));
    ui.painter().rect(address, CornerRadius::same(size::CONTROL_CORNER), color::field(), Stroke::new(1.0, color::control_border()), egui::StrokeKind::Inside);
    let label = if tab.loading { format!("Loading · {}", tab.current_url()) } else { tab.current_url().to_owned() };
    ui.painter().text(Pos2::new(address.left() + 9.0, address.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), color::text_control());
    let page = ui.interact(browser, ui.id().with(("browser-page", tab.id)), Sense::click());
    outcome.took_focus |= page.clicked();
    // A window has one native view, so a second rendered tab beside this one in a split pane has
    // nothing to draw here. It says so rather than showing an empty rectangle.
    if !showing {
        ui.painter().rect_filled(browser, CornerRadius::ZERO, color::editor());
        ui.painter().text(browser.center(), Align2::CENTER_CENTER, "This page is showing in the other pane.", FontId::proportional(13.0), color::text_faint());
    }
    (outcome, BrowserPlacement { id: tab.id, area: browser, focused })
}

/// Draw one compact navigation button and record a click when it is available.
fn draw_button(ui: &mut egui::Ui, left: &mut f32, toolbar: Rect, button: Button<'_>, outcome: &mut Outcome) {
    let area = Rect::from_min_size(Pos2::new(*left, toolbar.top() + 6.0), Vec2::splat(BUTTON_SIZE));
    let response = ui.interact(area, ui.id().with(("browser", button.name)), Sense::click());
    let fill = if response.hovered() && button.enabled { color::control() } else { egui::Color32::TRANSPARENT };
    ui.painter().rect_filled(area, CornerRadius::same(size::CONTROL_CORNER), fill);
    let text = if button.enabled { color::text_control() } else { color::text_faint() };
    ui.painter().text(area.center(), Align2::CENTER_CENTER, button.glyph, FontId::proportional(19.0), text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, button.enabled, button.name));
    if button.enabled && response.clicked() { outcome.command = Some(button.command); }
    *left = area.right() + 2.0;
}
