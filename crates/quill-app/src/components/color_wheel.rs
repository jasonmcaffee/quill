//! The colour wheel: a hue ring, a saturation and value square inside it, and an opacity bar.
//!
//! `task-1663` asks for "an icon to open a color wheel to select a color and opacity", and this is
//! what that icon opens. It is drawn rather than borrowed. egui has a colour picker of its own and
//! it is a saturation square with two strips beside it, which is not a wheel; it also brings egui's
//! own sliders and layout into a window that paints everything at measured positions.
//!
//! It is a component in the ordinary sense — it takes a `Ui` and a rectangle, draws itself, and
//! returns what the person did in it. The colour being chosen is held by the caller, so nothing here
//! remembers anything between frames and a test can draw it at any colour it likes.
//!
//! ## Where it is drawn, and why it is not a popup
//!
//! It is drawn **inside** the right click menu's own popup rather than in a second one over it. egui
//! keeps at most one popup open at a time, so opening a second shuts the first — the same rule that
//! turned the three line spacings in the text options panel from a dropdown into three buttons.
//! Pressing the wheel icon makes the menu taller instead.
//!
//! ## The colour, in two spellings
//!
//! Hue, saturation and value are what a wheel is for, and `quill_core::Rgba` is what a mark is
//! stored and sent as. [`hsv_to_rgb`] and [`rgb_to_hsv`] are the two ends of that, and the caller
//! only ever sees `Rgba`.

use egui::{Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use quill_core::Rgba;

use crate::components::controls;
use crate::theme::{color, size};

/// How wide the whole control is. It has to sit inside the right click menu, which is 300 points
/// wide with a 6 point margin either side.
pub const WIDTH: f32 = 268.0;
/// How tall it is: the ring, the opacity bar, the reading and the button.
pub const HEIGHT: f32 = 258.0;

/// How many steps the hue ring is built from. Sixty is six degrees a step, which at the radius the
/// ring is drawn at leaves no visible facets.
const STEPS: usize = 60;
/// How far in the ring's inner edge is, as a fraction of its outer radius.
const INNER: f32 = 0.74;
/// How finely the saturation and value square is subdivided. egui interpolates a triangle's colours
/// linearly, and a square drawn as two triangles has a visible seam across its diagonal; a grid does
/// not.
const GRID: usize = 12;
/// The opacity bar.
const BAR_HEIGHT: f32 = 18.0;
/// One square of the checkerboard behind the opacity bar.
const CHECK: f32 = 6.0;

/// What happened in the wheel this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// The colour being chosen, when it moved.
    pub chosen: Option<Rgba>,
    /// `Apply highlight` was pressed, with the colour to mark in.
    pub applied: Option<Rgba>,
}

/// Draw the wheel into `area`, showing `current`.
pub fn show(ui: &mut egui::Ui, area: Rect, current: Rgba) -> Outcome {
    let mut outcome = Outcome::default();
    let (mut hue, mut saturation, mut value) = rgb_to_hsv(current);
    let mut alpha = current.a;

    let ring = Rect::from_min_size(
        Pos2::new(area.center().x - RING / 2.0, area.top()),
        Vec2::splat(RING),
    );
    let radius = RING / 2.0;
    paint_ring(ui, ring.center(), radius);
    let square = shade_square(ring.center(), radius);
    paint_shade(ui, square, hue);
    paint_markers(ui, ring.center(), radius, square, hue, saturation, value);

    // The ring is added first and the square second, so that where they overlap the square takes the
    // drag: egui gives the pointer to the last widget that wanted it. The window's own resize grips
    // are added last for the same reason.
    let ring_response = ui.interact(ring, ui.id().with("highlight-hue"), Sense::click_and_drag());
    let shade_response =
        ui.interact(square, ui.id().with("highlight-shade"), Sense::click_and_drag());
    ring_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Highlight hue")
    });
    shade_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Highlight shade")
    });

    let mut moved = false;
    if let Some(at) = pressed_at(&shade_response) {
        saturation = ((at.x - square.left()) / square.width()).clamp(0.0, 1.0);
        value = 1.0 - ((at.y - square.top()) / square.height()).clamp(0.0, 1.0);
        moved = true;
    } else if let Some(at) = pressed_at(&ring_response) {
        let away = at - ring.center();
        if away.length() > radius * INNER * 0.55 {
            hue = angle_to_hue(away);
            moved = true;
        }
    }

    // The opacity bar.
    let bar = Rect::from_min_size(
        Pos2::new(area.left(), ring.bottom() + 14.0),
        Vec2::new(area.width(), BAR_HEIGHT),
    );
    let solid = hsv_to_rgb(hue, saturation, value, 0xFF);
    paint_opacity(ui, bar, solid, alpha);
    let bar_response = ui.interact(bar, ui.id().with("highlight-opacity"), Sense::click_and_drag());
    bar_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Highlight opacity")
    });
    if let Some(at) = pressed_at(&bar_response) {
        alpha = (((at.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
        moved = true;
    }

    let chosen = hsv_to_rgb(hue, saturation, value, alpha);
    if moved {
        outcome.chosen = Some(chosen);
    }

    // What it is, written out, so a colour can be copied down or typed into the command line.
    let reading = Rect::from_min_size(
        Pos2::new(area.left(), bar.bottom() + 10.0),
        Vec2::new(area.width(), 20.0),
    );
    let painter = ui.painter();
    let label = painter.layout_no_wrap(
        format!("{}  ·  {} % opacity", chosen.to_hex(), (alpha as f32 / 255.0 * 100.0).round()),
        egui::FontId::monospace(11.5),
        color::TEXT_DIM,
    );
    painter.galley(
        Pos2::new(reading.left(), reading.center().y - label.size().y / 2.0),
        label,
        color::TEXT_DIM,
    );

    let button = Rect::from_min_size(
        Pos2::new(area.left(), reading.bottom() + 8.0),
        Vec2::new(area.width(), 28.0),
    );
    if controls::choice_button(ui, button, "Apply highlight", false) {
        outcome.applied = Some(chosen);
    }
    outcome
}

/// How wide the ring is drawn.
const RING: f32 = 168.0;

/// Where the pointer is while this control is being pressed or dragged.
fn pressed_at(response: &egui::Response) -> Option<Pos2> {
    if response.is_pointer_button_down_on() || response.clicked() || response.dragged() {
        response.interact_pointer_pos()
    } else {
        None
    }
}

/// The square inscribed in the ring's inner circle, which is where saturation and value are chosen.
fn shade_square(centre: Pos2, radius: f32) -> Rect {
    let side = radius * INNER * std::f32::consts::SQRT_2 * 0.98;
    Rect::from_center_size(centre, Vec2::splat(side))
}

/// The hue ring: one mesh, a pair of vertices every few degrees.
fn paint_ring(ui: &egui::Ui, centre: Pos2, radius: f32) {
    let mut mesh = Mesh::default();
    for step in 0..=STEPS {
        let turn = step as f32 / STEPS as f32;
        let angle = turn * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        let colour = theme_colour(hsv_to_rgb(turn, 1.0, 1.0, 0xFF));
        for at in [radius, radius * INNER] {
            mesh.colored_vertex(
                Pos2::new(centre.x + cos * at, centre.y + sin * at),
                colour,
            );
        }
        if step > 0 {
            let base = (step as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 2, base + 3);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
}

/// The saturation and value square, subdivided so the corners interpolate without a seam.
fn paint_shade(ui: &egui::Ui, square: Rect, hue: f32) {
    let mut mesh = Mesh::default();
    for row in 0..=GRID {
        for column in 0..=GRID {
            let saturation = column as f32 / GRID as f32;
            let value = 1.0 - row as f32 / GRID as f32;
            mesh.colored_vertex(
                Pos2::new(
                    square.left() + square.width() * saturation,
                    square.top() + square.height() * (row as f32 / GRID as f32),
                ),
                theme_colour(hsv_to_rgb(hue, saturation, value, 0xFF)),
            );
        }
    }
    let stride = GRID as u32 + 1;
    for row in 0..GRID as u32 {
        for column in 0..GRID as u32 {
            let at = row * stride + column;
            mesh.add_triangle(at, at + 1, at + stride);
            mesh.add_triangle(at + 1, at + stride, at + stride + 1);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
    ui.painter().rect_stroke(
        square,
        CornerRadius::ZERO,
        Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Outside,
    );
}

/// The two rings that say where in the wheel the colour is.
fn paint_markers(
    ui: &egui::Ui,
    centre: Pos2,
    radius: f32,
    square: Rect,
    hue: f32,
    saturation: f32,
    value: f32,
) {
    let painter = ui.painter();
    let angle = hue * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let at = radius * (1.0 + INNER) / 2.0;
    painter.circle_stroke(
        Pos2::new(centre.x + cos * at, centre.y + sin * at),
        6.0,
        Stroke::new(2.0, color::TEXT_STRONG),
    );
    painter.circle_stroke(
        Pos2::new(
            square.left() + square.width() * saturation,
            square.top() + square.height() * (1.0 - value),
        ),
        5.0,
        Stroke::new(2.0, color::TEXT_STRONG),
    );
}

/// The opacity bar: a checkerboard, the colour over it from nothing to full, and where it is set.
///
/// A checkerboard rather than a number, because what a person is choosing is how much of the writing
/// shows through, and that is a thing to look at rather than to read.
fn paint_opacity(ui: &egui::Ui, bar: Rect, solid: Rgba, alpha: u8) {
    let painter = ui.painter();
    painter.rect_filled(bar, CornerRadius::same(size::CONTROL_CORNER), color::FIELD);
    let mut column = 0;
    let mut x = bar.left();
    while x < bar.right() {
        let mut row = 0;
        let mut y = bar.top();
        while y < bar.bottom() {
            if (row + column) % 2 == 0 {
                let square = Rect::from_min_max(
                    Pos2::new(x, y),
                    Pos2::new((x + CHECK).min(bar.right()), (y + CHECK).min(bar.bottom())),
                );
                painter.rect_filled(square, CornerRadius::ZERO, color::CONTROL);
            }
            row += 1;
            y += CHECK;
        }
        column += 1;
        x += CHECK;
    }
    let mut mesh = Mesh::default();
    for step in 0..=STEPS {
        let across = step as f32 / STEPS as f32;
        let colour = theme_colour(Rgba::new(
            solid.r,
            solid.g,
            solid.b,
            (across * 255.0).round() as u8,
        ));
        let x = bar.left() + bar.width() * across;
        mesh.colored_vertex(Pos2::new(x, bar.top()), colour);
        mesh.colored_vertex(Pos2::new(x, bar.bottom()), colour);
        if step > 0 {
            let base = (step as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 2, base + 3);
        }
    }
    painter.add(Shape::mesh(mesh));
    let at = bar.left() + bar.width() * (alpha as f32 / 255.0);
    painter.line_segment(
        [Pos2::new(at, bar.top() - 2.0), Pos2::new(at, bar.bottom() + 2.0)],
        Stroke::new(2.0, color::TEXT_STRONG),
    );
}

fn theme_colour(color: Rgba) -> Color32 {
    crate::theme::color32(color)
}

/// Which way round the ring a point is, as a hue from 0 to 1.
fn angle_to_hue(away: Vec2) -> f32 {
    let turn = away.y.atan2(away.x) / std::f32::consts::TAU;
    if turn < 0.0 {
        turn + 1.0
    } else {
        turn
    }
}

/// Hue, saturation and value — each from 0 to 1 — as a colour.
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32, alpha: u8) -> Rgba {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let sector = hue.floor() as i32;
    let along = hue - sector as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * along);
    let t = value * (1.0 - saturation * (1.0 - along));
    let (r, g, b) = match sector.rem_euclid(6) {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba::new(byte(r), byte(g), byte(b), alpha)
}

/// The other way round, so the wheel opens where the colour it was given is.
pub fn rgb_to_hsv(color: Rgba) -> (f32, f32, f32) {
    let r = color.r as f32 / 255.0;
    let g = color.g as f32 / 255.0;
    let b = color.b as f32 / 255.0;
    let most = r.max(g).max(b);
    let least = r.min(g).min(b);
    let spread = most - least;
    let hue = if spread <= f32::EPSILON {
        0.0
    } else if most == r {
        ((g - b) / spread).rem_euclid(6.0) / 6.0
    } else if most == g {
        ((b - r) / spread + 2.0) / 6.0
    } else {
        ((r - g) / spread + 4.0) / 6.0
    };
    let saturation = if most <= f32::EPSILON { 0.0 } else { spread / most };
    (hue, saturation, most)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ends of the conversion have to agree, or the wheel opens somewhere other than on the
    /// colour it was given and every drag jumps.
    #[test]
    fn a_colour_goes_round_the_conversion_and_comes_back_as_itself() {
        for colour in [
            Rgba::new(0xFE, 0xBC, 0x2E, 0x59),
            Rgba::new(0x7F, 0xCA, 0x98, 0xFF),
            Rgba::new(0x48, 0x9F, 0xF8, 0x00),
            Rgba::new(0xB4, 0x58, 0x8C, 0x80),
            Rgba::new(0x00, 0x00, 0x00, 0xFF),
            Rgba::new(0xFF, 0xFF, 0xFF, 0xFF),
            Rgba::new(0x40, 0x40, 0x40, 0x12),
        ] {
            let (hue, saturation, value) = rgb_to_hsv(colour);
            let back = hsv_to_rgb(hue, saturation, value, colour.a);
            assert_eq!(back, colour, "{colour:?} did not survive the trip through the wheel");
        }
    }

    #[test]
    fn the_hue_ring_runs_red_yellow_green_cyan_blue_magenta_and_back_to_red() {
        let named = |turn: f32| hsv_to_rgb(turn, 1.0, 1.0, 0xFF);
        assert_eq!(named(0.0), Rgba::new(255, 0, 0, 255));
        assert_eq!(named(1.0 / 6.0), Rgba::new(255, 255, 0, 255));
        assert_eq!(named(2.0 / 6.0), Rgba::new(0, 255, 0, 255));
        assert_eq!(named(3.0 / 6.0), Rgba::new(0, 255, 255, 255));
        assert_eq!(named(4.0 / 6.0), Rgba::new(0, 0, 255, 255));
        assert_eq!(named(5.0 / 6.0), Rgba::new(255, 0, 255, 255));
        assert_eq!(named(1.0), Rgba::new(255, 0, 0, 255), "the ring joins up");
    }

    #[test]
    fn every_direction_round_the_ring_is_a_hue_between_nothing_and_one() {
        for degrees in (0..360).step_by(15) {
            let angle = (degrees as f32).to_radians();
            let hue = angle_to_hue(Vec2::new(angle.cos(), angle.sin()));
            assert!((0.0..1.0).contains(&hue), "{degrees} degrees gave {hue}");
        }
    }

    #[test]
    fn the_shade_square_fits_inside_the_ring() {
        let radius = RING / 2.0;
        let square = shade_square(Pos2::ZERO, radius);
        let corner = Vec2::new(square.width() / 2.0, square.height() / 2.0).length();
        assert!(corner < radius * INNER, "a corner of the square is outside the ring's inner edge");
    }
}
