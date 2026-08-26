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
//!
//! ## Every modal is moved and resized the same way
//!
//! `task-1659` asks for modals that can be dragged and resized, and both live in [`show`] rather
//! than in any one dialog, so a dialog written later gets them without asking. The header is the
//! handle: dragging it moves the modal, and double clicking it puts the modal back in the middle at
//! the size its dialog asked for. Eight invisible grips round the edge resize it — the same four
//! edges and four corners `components::resize_edges` gives the window itself.
//!
//! Where it has been dragged to and how much bigger it has been made are kept in egui's own memory
//! under the modal's id rather than in `QuillApp`. A modal's geometry belongs to the modal: the
//! window has no decision to make about it, nothing is written to disk, and a dialog closed and
//! opened again is where it was left, which is what every other application does.
//!
//! Two rules about the order things are added in, and both are why they work at all. The **drag
//! strip is added before the contents**, so the close cross the header draws sits over it and a
//! click on the cross closes the modal rather than starting a drag. The **grips are added after the
//! contents**, for the reason `components::resize_edges` records about the window's own grips: egui
//! gives a pointer to the last widget that wants it, and a list or a field reaching the modal's edge
//! would otherwise take a drag meant for the edge.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::components::controls;
use crate::theme::{color, icon, size};

pub const HEADER: f32 = 46.0;
pub const FOOTER: f32 = 52.0;

/// The smallest a modal can be resized down to, whatever size its dialog asked for.
///
/// Small enough to be useful on a laptop screen and large enough that the header, a row of the body
/// and the footer's buttons all still fit, which is what stops a modal being dragged into a shape it
/// cannot be read in.
pub const MIN_WIDTH: f32 = 320.0;
pub const MIN_HEIGHT: f32 = 220.0;

/// How far in from a modal's edge it can be grabbed, and how far along each edge a corner reaches.
///
/// The same numbers `components::resize_edges` uses for the window, because they answer the same
/// question: a one point target cannot be hit with a mouse, and a corner has to win where it
/// overlaps an edge.
const EDGE: f32 = 6.0;
const CORNER: f32 = 16.0;

/// Where a modal has been dragged to, and how much bigger it has been made than its dialog asked.
///
/// Held as a difference rather than as a rectangle so that a modal follows the window: making the
/// Quill window larger moves a modal that was dragged to one side along with the middle it was
/// dragged from, and the size a dialog asks for is still the size it gets the first time it opens.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Placement {
    /// How far from the middle of the window the modal has been dragged.
    pub offset: Vec2,
    /// How much wider and taller than its dialog asked the modal has been made.
    pub grown: Vec2,
}

impl Placement {
    /// True while the modal is where a modal starts: in the middle, at the size it asked for.
    pub fn is_untouched(&self) -> bool {
        *self == Self::default()
    }
}

fn placement_id(id: &str) -> egui::Id {
    egui::Id::new(id).with("placement")
}

/// Where the modal with this id has been dragged to and how much it has been resized.
pub fn placement(ctx: &egui::Context, id: &str) -> Placement {
    ctx.data(|data| data.get_temp::<Placement>(placement_id(id))).unwrap_or_default()
}

/// Put a modal back in the middle at the size its dialog asks for, which is what double clicking its
/// header does.
pub fn reset_placement(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|data| data.remove::<Placement>(placement_id(id)));
}

fn remember_placement(ctx: &egui::Context, id: &str, placement: Placement) {
    ctx.data_mut(|data| data.insert_temp(placement_id(id), placement));
}

/// Put the modal with this id where a caller says, which is what `quill-cli modal move` and
/// `modal size` do. Dragging the header and the grips go through the same value.
pub fn set_placement(ctx: &egui::Context, id: &str, placement: Placement) {
    remember_placement(ctx, id, placement);
}

fn drawn_id(id: &str) -> egui::Id {
    egui::Id::new(id).with("drawn")
}

/// The rectangle the modal with this id last filled, in window points.
///
/// Remembered by [`show`] on every frame, because where a modal really is depends on the size of
/// the window as well as on how far it has been dragged, and a caller asking to move it to a place
/// needs the answer rather than the arithmetic behind it. `None` before the modal has been drawn
/// once.
pub fn drawn(ctx: &egui::Context, id: &str) -> Option<Rect> {
    ctx.data(|data| data.get_temp::<Rect>(drawn_id(id)))
}

fn remember_drawn(ctx: &egui::Context, id: &str, rect: Rect) {
    ctx.data_mut(|data| data.insert_temp(drawn_id(id), rect));
}

/// What a modal's own controls are called, worked out from its id.
///
/// Every control in Quill has a plain name, and the drag strip and the eight grips are controls. The
/// id is what [`show`] is given, so `quill-find-in-files` names them `Move find in files` and
/// `Resize find in files: bottom right`, which is what a test asks for and what assistive technology
/// reads out.
fn plain_name(id: &str) -> String {
    id.trim_start_matches("quill-").replace('-', " ")
}

/// Open a modal of a given size and draw `contents` into it.
///
/// The size is the one the dialog asks for plus whatever it has been resized by, capped to the
/// window, so a modal in a small Quill window is smaller rather than running off the edges. Where it
/// sits is the middle of the window plus whatever it has been dragged by, clamped so that a modal
/// cannot be dragged out of the window and lost.
pub fn show<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    height: f32,
    contents: impl FnOnce(&mut egui::Ui, Rect) -> R,
) -> (R, bool) {
    let mut placement = placement(ctx, id);
    let (position, size) = place(ctx, width, height, &mut placement);
    remember_drawn(ctx, id, Rect::from_min_size(position, size));

    let response = egui::Modal::new(egui::Id::new(id))
        .area(
            // `Modal::default_area` anchors the area to the middle of the window, and egui applies
            // an anchor *after* a fixed position rather than instead of it, so a modal that can be
            // dragged has to build its own area. This is that area with the anchor left off.
            egui::Area::new(egui::Id::new(id))
                .kind(egui::UiKind::Modal)
                .sense(Sense::hover())
                .order(egui::Order::Foreground)
                .interactable(true)
                .fixed_pos(position),
        )
        .backdrop_color(Color32::from_black_alpha(120))
        .frame(
            egui::Frame::NONE
                .fill(color::EXPLORER)
                .stroke(Stroke::new(1.0, color::CONTROL_BORDER))
                .corner_radius(CornerRadius::same(10)),
        )
        .show(ctx, |ui| {
            let (area, _) = ui.allocate_exact_size(size, Sense::hover());
            // Before the contents, so the close cross the header draws sits over the strip.
            let moved = drag_strip(ui, area, id, &mut placement);
            let inner = contents(ui, area);
            // After the contents, for the reason `components::resize_edges` gives.
            let resized = grips(ui, area, id, &mut placement);
            (inner, moved || resized)
        });

    let should_close = response.should_close();
    let (inner, changed) = response.inner;
    if changed {
        remember_placement(ctx, id, placement);
    }
    // Escape closes any modal, which is the one thing every dialog on every platform agrees about.
    let escaped = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    let close = should_close || escaped;
    (inner, close)
}

/// Where a modal of this size sits, and how big it really comes out.
///
/// Split from [`show`] so that the placement it settles on can be read back by a caller, and so the
/// clamping is one function rather than four numbers repeated inside the closure.
pub fn place(ctx: &egui::Context, width: f32, height: f32, placement: &mut Placement) -> (Pos2, Vec2) {
    let available = ctx.content_rect();
    let asked = Vec2::new(width, height);
    let (size, offset) = fit(available.size(), asked, placement.grown, placement.offset);
    placement.grown = size - asked;
    placement.offset = offset;
    (available.center() - size / 2.0 + offset, size)
}

/// The arithmetic behind [`place`], with no context in it so a test can check it.
///
/// `asked` is the size the dialog wants, `grown` how much it has been resized by and `offset` how
/// far it has been dragged. Returns the size it really gets and the offset it really sits at: never
/// larger than the room there is, never smaller than [`MIN_WIDTH`] by [`MIN_HEIGHT`], and never
/// dragged so far that any of it leaves the window.
pub fn fit(window: Vec2, asked: Vec2, grown: Vec2, offset: Vec2) -> (Vec2, Vec2) {
    let widest = (window.x - 40.0).max(120.0);
    let tallest = (window.y - 40.0).max(120.0);
    // A dialog that asks for less than the floor still gets what it asked for; the floor is about how
    // far a modal can be dragged *down* to. The text prompt is 190 points tall on purpose, and the
    // smallest useful size for a dialog with a list in it is not a reason to make it taller.
    let floor = Vec2::new(MIN_WIDTH.min(asked.x).min(widest), MIN_HEIGHT.min(asked.y).min(tallest));
    let size = Vec2::new(
        (asked.x + grown.x).clamp(floor.x, widest),
        (asked.y + grown.y).clamp(floor.y, tallest),
    );
    let room = Vec2::new(
        ((window.x - size.x) / 2.0 - 8.0).max(0.0),
        ((window.y - size.y) / 2.0 - 8.0).max(0.0),
    );
    let offset = Vec2::new(offset.x.clamp(-room.x, room.x), offset.y.clamp(-room.y, room.y));
    (size, offset)
}

/// The header, as something to drag the modal by. Returns true when it moved.
///
/// A double click puts the modal back where it started at the size it started, which is what double
/// clicking a divider does to a pane in `components::splitter`.
fn drag_strip(ui: &mut egui::Ui, area: Rect, id: &str, placement: &mut Placement) -> bool {
    let strip = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER));
    let response = ui.interact(strip, ui.id().with(("modal-move", id)), Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let name = format!("Move {}", plain_name(id));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), name.clone())
    });
    if response.double_clicked() {
        *placement = Placement::default();
        return true;
    }
    if response.dragged() && response.drag_delta() != Vec2::ZERO {
        placement.offset += response.drag_delta();
        return true;
    }
    false
}

/// The eight grips round a modal's edge. Returns true when one of them was dragged.
///
/// Dragging an edge keeps the opposite edge where it is, which is what resizing means everywhere
/// else. The modal is positioned from its middle, so growing it by `d` moves that middle by `d / 2`
/// towards the edge being dragged; that is the whole of the arithmetic here.
fn grips(ui: &mut egui::Ui, area: Rect, id: &str, placement: &mut Placement) -> bool {
    let mut moved = false;
    for (name, side, rect, cursor) in edges_and_corners(area) {
        let response = ui.interact(rect, ui.id().with(("modal-resize", id, name)), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        let label = format!("Resize {}: {name}", plain_name(id));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), label.clone())
        });
        if !response.dragged() {
            continue;
        }
        let drag = response.drag_delta();
        let grown = Vec2::new(side.0 as f32 * drag.x, side.1 as f32 * drag.y);
        if grown == Vec2::ZERO {
            continue;
        }
        placement.grown += grown;
        placement.offset += Vec2::new(side.0 as f32 * grown.x, side.1 as f32 * grown.y) / 2.0;
        moved = true;
    }
    moved
}

/// The eight rectangles a modal is resized by, each with which way dragging it grows the modal.
///
/// The side is `-1`, `0` or `1` on each axis, and it means "dragging this way makes the modal
/// larger": the right edge is `(1, 0)` and the top left corner is `(-1, -1)`. The corners come last
/// so they win where they overlap an edge, exactly as they do for the window itself.
fn edges_and_corners(area: Rect) -> [(&'static str, (i32, i32), Rect, egui::CursorIcon); 8] {
    let width = area.width();
    let height = area.height();
    [
        (
            "top",
            (0, -1),
            Rect::from_min_size(area.left_top(), Vec2::new(width, EDGE)),
            egui::CursorIcon::ResizeNorth,
        ),
        (
            "bottom",
            (0, 1),
            Rect::from_min_size(Pos2::new(area.left(), area.bottom() - EDGE), Vec2::new(width, EDGE)),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            "left",
            (-1, 0),
            Rect::from_min_size(area.left_top(), Vec2::new(EDGE, height)),
            egui::CursorIcon::ResizeWest,
        ),
        (
            "right",
            (1, 0),
            Rect::from_min_size(Pos2::new(area.right() - EDGE, area.top()), Vec2::new(EDGE, height)),
            egui::CursorIcon::ResizeEast,
        ),
        (
            "top left",
            (-1, -1),
            Rect::from_min_size(area.left_top(), Vec2::splat(CORNER)),
            egui::CursorIcon::ResizeNorthWest,
        ),
        (
            "top right",
            (1, -1),
            Rect::from_min_size(area.right_top() - Vec2::new(CORNER, 0.0), Vec2::splat(CORNER)),
            egui::CursorIcon::ResizeNorthEast,
        ),
        (
            "bottom left",
            (-1, 1),
            Rect::from_min_size(area.left_bottom() - Vec2::new(0.0, CORNER), Vec2::splat(CORNER)),
            egui::CursorIcon::ResizeSouthWest,
        ),
        (
            "bottom right",
            (1, 1),
            Rect::from_min_size(area.right_bottom() - Vec2::splat(CORNER), Vec2::splat(CORNER)),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ]
}

/// The bar across the top: the title at the left, a close cross at the right. Returns true when the
/// cross was pressed.
///
/// The bar is also what the modal is dragged by, and a grip is drawn at its right hand end so that
/// it looks like something that can be moved. Neither is added here: [`show`] adds the drag strip
/// before the contents so that this function's close cross sits over it.
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

/// What presses a modal's primary button from the keyboard.
///
/// `task-1682` asks that a modal be answerable without reaching for the pointer: somebody who has
/// just typed a name into a field should not have to move their hand to press `Create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// Enter, which is what every modal in Quill but one uses.
    Enter,
    /// The command key with Enter, for a modal whose **body** owns Enter: the commit message is a
    /// multiline field where Enter is a new line, and a new line is what a person pressing it there
    /// means. IntelliJ's commit dialog makes the same choice for the same reason.
    CommandEnter,
}

impl Confirm {
    /// Whether the key press that presses the primary button happened this frame.
    ///
    /// The press is **taken out of the frame's input**, so a modal that is confirmed from the
    /// keyboard cannot also be read as an ordinary Enter by anything drawn after it. The list
    /// dialogs — `Go to File`, `Find in Files` and the references modal — take Enter for
    /// themselves before their footer is drawn, which is what stops it meaning two things there:
    /// in those, Enter opens the row that is chosen and the button at the bottom right is the same
    /// thing said twice.
    pub fn pressed(&self, ui: &egui::Ui) -> bool {
        // Asked of what is really held rather than through `consume_key`, which matches by
        // `Modifiers::matches_logically`: that only asks whether the modifiers the *pattern* names
        // are held, so a pattern of `NONE` matches `Command+Enter` too and the commit dialog would
        // commit on both. `task-1678`'s completion popup had to make the same comparison.
        //
        // `command_only` rather than an equality test, because the command key is not one flag:
        // on Windows `Ctrl+Enter` arrives with **both** `ctrl` and `command` set, so a modal that
        // compared against `Modifiers::COMMAND` would work in a test and never in the window.
        let wanted = |held: &egui::Modifiers| match self {
            Confirm::Enter => held.is_none(),
            Confirm::CommandEnter => held.command_only(),
        };
        ui.input_mut(|input| {
            let mut pressed = false;
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers: held,
                    ..
                } if wanted(held) => {
                    pressed = true;
                    false
                }
                _ => true,
            });
            pressed
        })
    }
}

/// The bar across the bottom, with its buttons at the right.
///
/// `buttons` is given in the order they read, left to right; the last one is the one that does the
/// thing and is filled in the accent colour. Returns which one was pressed — by the pointer, or by
/// Enter, which presses that last button when it is enabled.
pub fn footer(ui: &mut egui::Ui, area: Rect, buttons: &[(&str, bool)]) -> Option<usize> {
    footer_confirmed_by(ui, area, buttons, Confirm::Enter)
}

/// The same footer, for a modal whose body owns Enter. See [`Confirm`].
pub fn footer_confirmed_by(
    ui: &mut egui::Ui,
    area: Rect,
    buttons: &[(&str, bool)],
    confirm: Confirm,
) -> Option<usize> {
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
    // The keyboard presses the last button, which is the one that does the thing. A footer whose
    // last button is dimmed — no name typed, nothing chosen — is a modal there is nothing to
    // confirm, so the key press is left alone rather than doing nothing loudly.
    let primary = buttons.len().checked_sub(1);
    if let Some(primary) = primary {
        if buttons[primary].1 && confirm.pressed(ui) {
            pressed = Some(primary);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Vec2 = Vec2::new(1180.0, 740.0);

    #[test]
    fn a_modal_that_has_not_been_touched_gets_the_size_its_dialog_asked_for() {
        let (size, offset) = fit(WINDOW, Vec2::new(560.0, 420.0), Vec2::ZERO, Vec2::ZERO);
        assert_eq!(size, Vec2::new(560.0, 420.0));
        assert_eq!(offset, Vec2::ZERO, "and sits in the middle");
    }

    #[test]
    fn a_modal_is_never_larger_than_the_window_it_is_in() {
        let (size, _) = fit(Vec2::new(600.0, 400.0), Vec2::new(900.0, 560.0), Vec2::ZERO, Vec2::ZERO);
        assert_eq!(size, Vec2::new(560.0, 360.0), "twenty points of margin either side");
    }

    #[test]
    fn a_modal_cannot_be_resized_below_what_can_be_read() {
        let (size, _) = fit(WINDOW, Vec2::new(560.0, 420.0), Vec2::new(-900.0, -900.0), Vec2::ZERO);
        assert_eq!(size, Vec2::new(MIN_WIDTH, MIN_HEIGHT));
    }

    #[test]
    fn a_modal_cannot_be_dragged_out_of_the_window() {
        let asked = Vec2::new(560.0, 420.0);
        let (_, offset) = fit(WINDOW, asked, Vec2::ZERO, Vec2::new(4000.0, -4000.0));
        // Half the room either side, less the eight points that keep it off the window's own edge.
        assert_eq!(offset.x, (WINDOW.x - asked.x) / 2.0 - 8.0);
        assert_eq!(offset.y, -((WINDOW.y - asked.y) / 2.0 - 8.0));
    }

    #[test]
    fn dragging_an_edge_keeps_the_opposite_edge_where_it_was() {
        // The right edge dragged 100 points to the right: 100 wider, and the middle 50 to the right,
        // which leaves the left edge exactly where it was.
        let mut placement = Placement::default();
        let drag = Vec2::new(100.0, 0.0);
        let side = (1, 0);
        let grown = Vec2::new(side.0 as f32 * drag.x, side.1 as f32 * drag.y);
        placement.grown += grown;
        placement.offset += Vec2::new(side.0 as f32 * grown.x, side.1 as f32 * grown.y) / 2.0;
        assert_eq!(placement.grown, Vec2::new(100.0, 0.0));
        assert_eq!(placement.offset, Vec2::new(50.0, 0.0));
    }

    #[test]
    fn a_modals_controls_are_named_after_it() {
        assert_eq!(plain_name("quill-find-in-files"), "find in files");
        assert_eq!(plain_name("quill-settings"), "settings");
    }

    #[test]
    fn the_corners_reach_further_than_the_edges_so_they_win_where_they_overlap() {
        assert!(CORNER > EDGE);
    }
}
