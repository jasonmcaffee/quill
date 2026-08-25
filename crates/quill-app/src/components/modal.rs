//! The furniture every modal in Quill is built from.
//!
//! `design/style-guide.md` says what a modal is: an `egui::Modal` over a backdrop of
//! `from_black_alpha(120)`, filled `EXPLORER`, a one point `CONTROL_BORDER` stroke, corner radius
//! 10, a 46 point header filled `TITLE_BAR` with the title at the left and a close cross at the
//! right, and a 52 point footer with its buttons at the right.
//!
//! The Settings window is where that shape came from. It is written down here because the commit
//! panel, the seven git dialogs, the text prompt and the confirmation are all the same shape, and
//! ten copies of it would be ten modals that almost agree.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::components::controls;
use crate::theme::{color, icon, size};

pub const HEADER: f32 = 46.0;
pub const FOOTER: f32 = 52.0;

/// Open a modal of a given size and draw `contents` into it.
///
/// The size is capped to the window, so a modal in a small Quill window is smaller rather than
/// running off the edges.
pub fn show<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    height: f32,
    contents: impl FnOnce(&mut egui::Ui, Rect) -> R,
) -> (R, bool) {
    let response = egui::Modal::new(egui::Id::new(id))
        .backdrop_color(Color32::from_black_alpha(120))
        .frame(
            egui::Frame::NONE
                .fill(color::EXPLORER)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER))
                .corner_radius(CornerRadius::same(10)),
        )
        .show(ctx, |ui| {
            let available = ctx.content_rect().size();
            let width = width.min(available.x - 40.0);
            let height = height.min(available.y - 40.0);
            let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            contents(ui, area)
        });
    // Escape closes any modal, which is the one thing every dialog on every platform agrees about.
    let escaped = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    let close = response.should_close() || escaped;
    (response.inner, close)
}

/// The bar across the top: the title at the left, a close cross at the right. Returns true when the
/// cross was pressed.
pub fn header(ui: &mut egui::Ui, area: Rect, title: &str) -> bool {
    let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER));
    let painter = ui.painter_at(area);
    painter.rect_filled(bar, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, color::TITLE_BAR);
    let galley =
        painter.layout_no_wrap(title.to_owned(), egui::FontId::proportional(13.0), color::TEXT_STRONG);
    painter.galley(
        Pos2::new(area.left() + 20.0, bar.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_STRONG,
    );
    painter.line_segment(
        [Pos2::new(bar.left(), bar.bottom()), Pos2::new(bar.right(), bar.bottom())],
        Stroke::new(1.0, color::DIVIDER),
    );
    let close = Rect::from_center_size(Pos2::new(area.right() - 24.0, bar.center().y), Vec2::splat(22.0));
    controls::icon_button(ui, close, "Close", icon::cross)
}

/// The rectangle between the header and the footer, inset by the usual margin.
pub fn body(area: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(area.left() + 20.0, area.top() + HEADER + 14.0),
        Pos2::new(area.right() - 20.0, area.bottom() - FOOTER - 8.0),
    )
}

/// The bar across the bottom, with its buttons at the right.
///
/// `buttons` is given in the order they read, left to right; the last one is the one that does the
/// thing and is filled in the accent colour. Returns which one was pressed.
pub fn footer(ui: &mut egui::Ui, area: Rect, buttons: &[(&str, bool)]) -> Option<usize> {
    let bar = Rect::from_min_max(Pos2::new(area.left(), area.bottom() - FOOTER), area.max);
    ui.painter_at(area).line_segment(
        [Pos2::new(bar.left(), bar.top()), Pos2::new(bar.right(), bar.top())],
        Stroke::new(1.0, color::DIVIDER),
    );
    let mut pressed = None;
    let mut right = bar.right() - 20.0;
    for (index, (name, enabled)) in buttons.iter().enumerate().rev() {
        let width = (name.chars().count() as f32 * 7.5 + 36.0).max(90.0);
        let rect = Rect::from_min_size(
            Pos2::new(right - width, bar.center().y - 14.0),
            Vec2::new(width, 28.0),
        );
        if button(ui, rect, name, *enabled, index + 1 == buttons.len()) {
            pressed = Some(index);
        }
        right = rect.left() - 8.0;
    }
    pressed
}

/// A button with a word in it. The one that does the thing is filled in the accent colour, so the
/// answers to a question do not all look the same.
pub fn button(ui: &mut egui::Ui, area: Rect, name: &str, enabled: bool, primary: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(area, ui.id().with(("modal-button", name)), sense);
    let fill = match (enabled, primary, response.hovered()) {
        (false, _, _) => color::CONTROL.gamma_multiply(0.6),
        (true, true, _) => color::ACCENT,
        (true, false, true) => color::CONTROL.gamma_multiply(1.25),
        (true, false, false) => color::CONTROL,
    };
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        fill,
        Stroke::new(1.0, if primary && enabled { color::ACCENT } else { color::CONTROL_BORDER }),
        egui::StrokeKind::Inside,
    );
    let tint = if enabled { color::TEXT_STRONG } else { color::TEXT_FAINT };
    let galley = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), tint);
    painter.galley(area.center() - galley.size() / 2.0, galley, tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    response.clicked()
}

/// A heading inside a page, with a rule running to the right edge, as IntelliJ draws one.
pub fn section(ui: &mut egui::Ui, area: Rect, top: f32, name: &str) -> f32 {
    let painter = ui.painter_at(area.expand(20.0));
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), color::TEXT_STRONG);
    let y = top + 8.0;
    painter.galley(Pos2::new(area.left(), y - galley.size().y / 2.0), galley.clone(), color::TEXT_STRONG);
    painter.line_segment(
        [Pos2::new(area.left() + galley.size().x + 12.0, y), Pos2::new(area.right(), y)],
        Stroke::new(1.0, color::DIVIDER),
    );
    y + 18.0
}

/// A line of explanation, in the faintest colour. Returns the y below it.
pub fn note(ui: &mut egui::Ui, area: Rect, top: f32, text: &str) -> f32 {
    let painter = ui.painter_at(area.expand(20.0));
    let galley =
        painter.layout(text.to_owned(), egui::FontId::proportional(11.5), color::TEXT_FAINT, area.width());
    let height = galley.size().y;
    painter.galley(Pos2::new(area.left(), top), galley, color::TEXT_FAINT);
    top + height + 8.0
}

/// A tick box with its label to the right of it. Returns true when it was changed.
pub fn check(ui: &mut egui::Ui, row: Rect, name: &str, value: &mut bool) -> bool {
    let box_rect = Rect::from_min_size(Pos2::new(row.left(), row.center().y - 8.0), Vec2::splat(16.0));
    let response = ui.interact(row, ui.id().with(("modal-check", name)), Sense::click());
    let painter = ui.painter();
    painter.rect(
        box_rect,
        CornerRadius::same(3),
        if *value { color::ACCENT } else { color::FIELD },
        Stroke::new(1.0, if *value { color::ACCENT } else { color::CONTROL_BORDER }),
        egui::StrokeKind::Inside,
    );
    if *value {
        icon::tick(painter, box_rect.center(), color::TEXT_STRONG);
    }
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), color::TEXT_CONTROL);
    painter.galley(
        Pos2::new(box_rect.right() + 10.0, row.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, name)
    });
    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// A field to type into, drawn the way every other one in Quill is.
pub fn field(ui: &mut egui::Ui, area: Rect, name: &str, value: &mut String) -> egui::Response {
    ui.painter().rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::FIELD,
        Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
    let text_rect = crate::components::controls::field_text_rect(ui, area, 8.0);
    let mut edit = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = edit.add(
        egui::TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .text_color(color::TEXT_CONTROL),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
    response
}

/// One row of a list: the pill when it is chosen or hovered, and whatever `draw` puts in it.
///
/// The pill is the one `design/style-guide.md` describes, so a row in the branches list is drawn
/// exactly like the open file in the explorer.
pub fn row(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    chosen: bool,
    draw: impl FnOnce(&egui::Painter, Rect),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size::ROW), Sense::hover());
    let response = ui.interact(rect, ui.id().with(("modal-row", id)), Sense::click());
    let pill = rect.shrink2(Vec2::new(8.0, 1.0));
    if chosen {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::SELECTED_ROW);
    } else if response.hovered() {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::CONTROL);
    }
    draw(ui.painter(), rect);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, name)
    });
    response
}

/// Text at a position, at the ordinary size, vertically centred in `row`.
pub fn label(painter: &egui::Painter, row: Rect, x: f32, text: &str, tint: Color32, size: f32) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(size), tint);
    painter.galley(Pos2::new(x, row.center().y - galley.size().y / 2.0), galley.clone(), tint);
    x + galley.size().x
}

/// A monospaced block of text that scrolls, which is what a diff and a commit are shown in.
pub fn monospaced(ui: &mut egui::Ui, area: Rect, id: &str, text: &str) {
    ui.painter().rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::EDITOR,
        Stroke::new(1.0, color::DIVIDER),
        egui::StrokeKind::Inside,
    );
    let inner = area.shrink(8.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    egui::ScrollArea::both().id_salt(id).show(&mut child, |ui| {
        for line in text.lines() {
            // A diff reads by colour before it reads by sign, which is the whole point of one.
            let tint = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => color::GIT_ADDED,
                Some('-') if !line.starts_with("---") => color::CLOSE,
                Some('@') => color::ACCENT,
                _ => color::TEXT_CONTROL,
            };
            ui.label(egui::RichText::new(line).monospace().size(11.5).color(tint));
        }
        if text.trim().is_empty() {
            ui.label(egui::RichText::new("Nothing to show.").size(11.5).color(color::TEXT_FAINT));
        }
    });
}
