//! What a person sees and does while a panel is being moved to another edge of the window.
//!
//! `task-1697` asks for two things a person can see: a panel's header has to be draggable, and while
//! one is in the air there have to be *"blue highlighted regions to indicate where I can drag to"*.
//! Both are here. Where a panel would land, and where it does land, are `app::dock`'s to decide —
//! this component **reports and decides nothing**, which is the rule every component in Quill follows.
//!
//! ## The handle is the header, and it is added first on purpose
//!
//! A tile's header already holds tabs that are dragged along their own strip, and buttons. So the
//! handle is added to the `Ui` **before** them, over the whole header, and they are added after it:
//! egui gives a pointer to the last widget that asked for the point, which is the rule
//! `components::splitter` and `components::resize_edges` are both written around, used here the other
//! way up. What the handle is left with is exactly the part of the header nothing else wanted — the
//! heading word and the empty space beside it, which is IntelliJ's own handle for the same gesture.
//!
//! ## The blue rectangle is the layout, not a picture of it
//!
//! The four bands are drawn faintly, because the ask is that a person can see there are four places
//! rather than only the one they happen to be over. The strong rectangle is not a guess about where
//! the panel will go: it is `app::dock::regions` run over the layout **as it would be after the
//! drop**, so the preview and the drop are one function applied to one value and cannot disagree.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::dock::{Panel, Side, Zone};
use crate::theme::color;

/// How much of the accent each part of the overlay is painted at.
const BAND_FILL: f32 = 0.14;
const BAND_EDGE: f32 = 0.55;
const LANDING_FILL: f32 = 0.28;

/// What a panel's header reported this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Grab {
    /// Where the pointer is while the panel is in the air.
    pub carrying: Option<Pos2>,
    /// True on the frame it was let go.
    pub dropped: bool,
    /// The header was right clicked, which opens the panel's own menu.
    pub menu: Option<Pos2>,
}

/// Make `header` the handle a panel is carried by.
///
/// Call it **before** anything else in the header — see the note at the top of the file.
pub fn handle(ui: &mut egui::Ui, header: Rect, panel: Panel) -> Grab {
    let mut grab = Grab::default();
    if header.width() <= 0.0 || header.height() <= 0.0 {
        return grab;
    }
    // `Sense::CLICK | Sense::DRAG` and not `Sense::click_and_drag()`: the same presses, but the
    // handle cannot take egui's keyboard focus. It covers the whole of a panel's header, so a bare
    // `Tab` press would land on it very easily — and `app::hold_the_keyboard` records what a widget
    // holding the focus is worth, which is that `Space` then presses it.
    let response =
        ui.interact(header, ui.id().with(("dock-handle", panel.name())), Sense::CLICK | Sense::DRAG);
    if response.dragged() || response.drag_stopped() {
        if let Some(pointer) = response.interact_pointer_pos() {
            grab.carrying = Some(pointer);
            grab.dropped = response.drag_stopped();
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if response.secondary_clicked() {
        grab.menu = response.interact_pointer_pos().or_else(|| response.hover_pos());
    }
    // Named, because every control in Quill has a plain name and a test finds one by it.
    let name = format!("Move {}", panel.label());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), name.clone())
    });
    grab
}

/// Draw the four places a panel can be dropped, and the one it would land in.
///
/// `landing` is the rectangle the panel would occupy — worked out by the window from
/// `app::dock::regions`, so it is the real one. `Rect::ZERO` while the pointer is over none of the
/// bands, which is a drag that can still be thought better of.
pub fn zones(ui: &egui::Ui, bands: &[Zone; 4], chosen: Option<Side>, landing: Rect, carrying: Panel) {
    let painter = ui.painter();
    for zone in bands {
        if Some(zone.side) == chosen {
            continue;
        }
        painter.rect_filled(zone.band, CornerRadius::ZERO, fade(BAND_FILL));
        painter.rect_stroke(
            zone.band.shrink(1.0),
            CornerRadius::same(2),
            Stroke::new(1.0, fade(BAND_EDGE)),
            egui::StrokeKind::Inside,
        );
    }
    if chosen.is_none() || landing.width() <= 1.0 || landing.height() <= 1.0 {
        return;
    }
    painter.rect_filled(landing, CornerRadius::ZERO, fade(LANDING_FILL));
    painter.rect_stroke(
        landing.shrink(1.0),
        CornerRadius::same(3),
        Stroke::new(2.0, color::ACCENT),
        egui::StrokeKind::Inside,
    );
    // The panel's own name in the middle of where it is going, so a preview over an empty editing
    // area still says what is about to happen there.
    let label = painter.layout_no_wrap(
        carrying.label().to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_STRONG,
    );
    let size = label.size();
    if size.x + 20.0 < landing.width() && size.y + 12.0 < landing.height() {
        let plate = Rect::from_center_size(landing.center(), size + Vec2::new(20.0, 12.0));
        painter.rect_filled(plate, CornerRadius::same(4), color::ACCENT);
        painter.galley(plate.center() - size / 2.0, label, color::TEXT_STRONG);
    }
}

/// The accent at a fraction of its opacity.
///
/// Written out rather than using `gamma_multiply`, because the overlay is painted over a window that
/// is already translucent and the alpha wanted here is a flat one a reader can check against the
/// numbers above.
fn fade(amount: f32) -> Color32 {
    let alpha = (255.0 * amount).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(
        color::ACCENT.r(),
        color::ACCENT.g(),
        color::ACCENT.b(),
        alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_is_the_accent_and_nothing_else() {
        // The palette is closed. A drop zone is the blue the ask names, which is the one blue the
        // design has, at three strengths rather than three colours.
        for amount in [BAND_FILL, BAND_EDGE, LANDING_FILL] {
            // Asked unmultiplied, because `Color32` keeps its alpha premultiplied and the question
            // here is which colour was chosen rather than what it comes out as over black.
            let [red, green, blue, alpha] = fade(amount).to_srgba_unmultiplied();
            // Within a few units of the accent: going through egui's premultiplied storage and back
            // again rounds, and the question is which colour was chosen rather than the last bit of it.
            for (painted, wanted) in
                [(red, color::ACCENT.r()), (green, color::ACCENT.g()), (blue, color::ACCENT.b())]
            {
                assert!(painted.abs_diff(wanted) <= 4, "{painted} is not the accent's {wanted}");
            }
            assert!(alpha > 0 && alpha < 255);
        }
        assert!(fade(BAND_FILL).a() < fade(LANDING_FILL).a(), "the one being aimed at is the stronger");
    }
}
