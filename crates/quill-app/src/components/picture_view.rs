//! The editing area when the tab holds a picture rather than text.
//!
//! It draws the picture centred in the area at whatever scale it is being shown at, and takes the three
//! things a person does to a picture: drag it about, zoom it with a pinch or with the wheel and the zoom
//! modifier, and scroll it with the wheel alone.
//!
//! The keyboard's zoom is not read here. Command and plus is a `View` menu entry, as it is for the
//! editor's font size, because on macOS AppKit hands a menu item's key equivalent to the menu before the
//! window ever sees it — so a key press read in a component would work on Windows and be dead on macOS.
//! `QuillApp::run_action` sends it here instead.
//!
//! Like every component it changes nothing but what it was handed: the scale and the offset live on the
//! [`Picture`], which belongs to the tab.

use egui::{Pos2, Rect, Sense, Vec2};

use crate::services::picture::Picture;
use crate::theme::color;

/// What the picture area asks the window to do.
#[derive(Debug, Default)]
pub struct PictureOutcome {
    /// It was clicked, so the keyboard belongs to the editing area rather than to the terminal.
    pub take_focus: bool,
}

/// Draw `picture` into `area`.
///
/// `name` names the texture, so two tabs showing two files do not share one upload.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    picture: &mut Picture,
    name: &str,
) -> PictureOutcome {
    let mut outcome = PictureOutcome::default();
    let response = ui.interact(area, ui.id().with(("picture", name)), Sense::click_and_drag());
    if response.clicked() || response.drag_started() {
        outcome.take_focus = true;
    }

    if let Some(problem) = picture.problem.clone() {
        say(ui, area, &format!("{name} could not be shown \u{2014} {problem}"));
        return outcome;
    }

    // A pinch, or the wheel with the zoom modifier held. `zoom_delta` reports both as one multiplier,
    // and egui holds the scroll back while the modifier is down, so the picture does not slide about
    // while it is being zoomed. Gated on the pointer being over the area rather than on
    // `response.hovered()`: egui reports no pointer at all on a frame whose only input is a wheel
    // event, which is the fault `QuillApp::zoom_the_text` records in full.
    let over = ui.input(|input| input.pointer.hover_pos()).is_some_and(|at| area.contains(at));
    if over {
        let gesture = ui.input(|input| input.zoom_delta());
        picture.zoom_by_gesture(gesture, area.size());
    }

    // Dragging moves the picture, which is what a person expects to be able to do with one that is
    // bigger than the window.
    if response.dragged() {
        picture.offset += response.drag_delta();
    }
    // The wheel on its own scrolls it, as it does the document.
    if over {
        let wheel = ui.input(|input| input.smooth_scroll_delta);
        if wheel != Vec2::ZERO {
            picture.offset += wheel;
        }
    }
    // Double clicking puts it back to filling the area, which is the same gesture that puts a dragged
    // divider back to its usual size.
    if response.double_clicked() {
        picture.fit();
    }

    let scale = picture.scale_in(area.size());
    let drawn = Vec2::new(picture.size[0] as f32 * scale, picture.size[1] as f32 * scale);
    // A picture smaller than the area sits in the middle of it and cannot be dragged out of view; one
    // larger than the area can be dragged as far as its own edges and no further.
    let room = ((drawn - area.size()) / 2.0).max(Vec2::ZERO);
    picture.offset = picture.offset.clamp(-room, room);

    let Some(texture) = picture.texture(ui.ctx(), name) else {
        say(ui, area, &format!("{name} is still being read"));
        return outcome;
    };
    let at = Rect::from_center_size(area.center() + picture.offset, drawn);
    let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
    painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
    painter_ui.painter().image(
        texture.id(),
        at,
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    outcome
}

/// A line in the middle of the area, for a picture that will not decode or has not been uploaded yet.
fn say(ui: &egui::Ui, area: Rect, text: &str) {
    let painter = ui.painter_at(area);
    let galley =
        painter.layout(text.to_owned(), egui::FontId::proportional(12.5), color::TEXT_DIM, area.width() - 64.0);
    painter.galley(
        Pos2::new(area.center().x - galley.size().x / 2.0, area.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );
}
