//! The workspace tab: a strip of pages, and whichever one is showing.
//!
//! **The plugin contributes one tab**, so that tab holds a strip of its own — which is the shape the
//! Services tool window has in The reference screenshots’ `db_ui_query_console_result_tab.png`, and the honest answer to
//! a manifest that offers one `tab.id`. A console and a row editor are the two kinds of page, and both
//! are drawn on the same card.

use egui::{Pos2, Rect, Sense, Vec2};

use crate::components::database::{card, console, grid, text, Act, PAD, RADIUS};
use crate::services::database::{DatabaseExplorer, Sheet};
use crate::services::plugin_ui::Look;
use crate::theme::color;

/// The strip's own height, which holds a page name and its cross.
const STRIP: f32 = 30.0;

/// Draw the tab.
pub fn show(explorer: &mut DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    if explorer.pages.is_empty() {
        return nothing_open(explorer, ui, look, area);
    }
    let strip = Rect::from_min_size(
        Pos2::new(area.left() + PAD * scale, area.top() + PAD * scale),
        Vec2::new(area.width() - PAD * 2.0 * scale, STRIP * scale),
    );
    acts.extend(pages(explorer, ui, look, strip));

    let body = Rect::from_min_max(
        Pos2::new(area.left() + PAD * scale, strip.bottom() + 4.0 * scale),
        Pos2::new(area.right() - PAD * scale, area.bottom() - PAD * scale),
    );
    card(ui, look, body, look.palette.board_card, RADIUS * scale);
    let inside = body.shrink(PAD * scale);
    let Some(page) = explorer.pages.get(explorer.current) else { return acts };
    let id = page.id;
    match &page.sheet {
        Sheet::Console(_) => acts.extend(console::show(explorer, ui, look, inside, id)),
        Sheet::Grid(_) => acts.extend(grid::show(explorer, ui, look, inside, id)),
    }
    acts
}

/// The strip of page names.
fn pages(explorer: &DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>, strip: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let mut at = strip.left();
    for (index, page) in explorer.pages.iter().enumerate() {
        let title = page.title();
        let painter = ui.painter();
        let measured = painter
            .layout_no_wrap(title.clone(), egui::FontId::proportional(look.font_size * 0.85), color::text())
            .size()
            .x;
        let width = (measured + 34.0 * scale).min(strip.width() * 0.4);
        if at + width > strip.right() {
            break;
        }
        let rect = Rect::from_min_size(Pos2::new(at, strip.top()), Vec2::new(width, strip.height()));
        at += width + 2.0 * scale;
        let showing = index == explorer.current;
        let response = ui.interact(rect, ui.id().with(("database-page", page.id)), Sense::click());
        if showing || response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(6),
                match showing {
                    true => look.palette.board_card,
                    false => look.palette.control,
                },
            );
        }
        if showing {
            // The accent line under the page that is showing, which is what the editing area's own
            // tab strip draws.
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left() + 6.0 * scale, rect.bottom() - 2.0 * scale),
                    Pos2::new(rect.right() - 6.0 * scale, rect.bottom()),
                ),
                egui::CornerRadius::same(1),
                look.palette.accent,
            );
        }
        text(
            ui.painter(),
            Pos2::new(rect.left() + 8.0 * scale, rect.center().y),
            &title,
            match showing {
                true => color::text_strong(),
                false => color::text(),
            },
            look.font_size * 0.85,
            width - 30.0 * scale,
        );
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, showing, &title));
        if response.clicked() {
            acts.push(Act::ShowPage(page.id));
        }
        let cross = Rect::from_center_size(
            Pos2::new(rect.right() - 12.0 * scale, rect.center().y),
            Vec2::splat(16.0 * scale),
        );
        if crate::components::controls::icon_button(ui, cross, &format!("Close {title}"), crate::theme::icon::cross) {
            acts.push(Act::ClosePage(page.id));
        }
    }
    acts
}

/// What the tab shows before anything has been opened.
///
/// Not an empty rectangle: the two things somebody can do from here are drawn as buttons, which is
/// what makes an empty state a place to start rather than a place to leave.
fn nothing_open(explorer: &DatabaseExplorer, ui: &mut egui::Ui, look: &Look<'_>, area: Rect) -> Vec<Act> {
    let scale = look.scale();
    let mut acts = Vec::new();
    let middle = area.center();
    let source = explorer.configuration.chosen.clone();
    let said = match source.is_empty() {
        true => "No data sources yet.".to_owned(),
        false => format!("Nothing open. `{source}` is the data source this is pointed at."),
    };
    text(
        ui.painter(),
        Pos2::new(middle.x - 160.0 * scale, middle.y - 24.0 * scale),
        &said,
        color::text_dim(),
        look.font_size * 0.95,
        320.0 * scale,
    );
    let button = Rect::from_center_size(
        Pos2::new(middle.x, middle.y + 8.0 * scale),
        Vec2::new(180.0 * scale, 26.0 * scale),
    );
    match source.is_empty() {
        true => {
            if crate::components::modal::button(ui, button, "New data source", true, true) {
                acts.push(Act::NewSource);
            }
        }
        false => {
            if crate::components::modal::button(ui, button, "Open a query console", true, true) {
                acts.push(Act::OpenConsole(source));
            }
        }
    }
    acts
}
