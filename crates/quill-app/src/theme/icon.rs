//! Drawn icons. The design uses shapes rather than letters for the alignment buttons, for undo and redo
//! and for the small controls in the explorer, and the characters for those are not in egui's default
//! fonts, so they are drawn here.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke};

/// A small triangle pointing down, on the right of a dropdown.
pub fn chevron_down(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let w = 3.5;
    let h = 2.2;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(centre.x - w, centre.y - h),
            Pos2::new(centre.x + w, centre.y - h),
            Pos2::new(centre.x, centre.y + h),
        ],
        color,
        Stroke::NONE,
    ));
}

/// A triangle pointing down when a folder is open and right when it is closed.
pub fn disclosure(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![
            Pos2::new(centre.x - 4.0, centre.y - 2.0),
            Pos2::new(centre.x + 4.0, centre.y - 2.0),
            Pos2::new(centre.x, centre.y + 3.0),
        ]
    } else {
        vec![
            Pos2::new(centre.x - 2.0, centre.y - 4.0),
            Pos2::new(centre.x - 2.0, centre.y + 4.0),
            Pos2::new(centre.x + 3.0, centre.y),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Four stacked lines showing how a paragraph is placed. The short lines sit where the ragged edge
/// would be, which is what makes the four buttons tell each other apart.
pub fn alignment(painter: &egui::Painter, area: Rect, align: quill_core::Align, color: Color32) {
    let full = area.width();
    let short = full * 0.62;
    let spacing = area.height() / 3.0;
    let stroke = Stroke::new(1.6, color);
    for row in 0..4 {
        let y = area.top() + spacing * row as f32;
        // Rows 1 and 3 are the short ones, so the shape reads as a paragraph of text.
        let width = if row % 2 == 1 { short } else { full };
        let x = match align {
            quill_core::Align::Left | quill_core::Align::Justify => area.left(),
            quill_core::Align::Center => area.left() + (full - width) / 2.0,
            quill_core::Align::Right => area.right() - width,
        };
        // Justified text is flush on both sides, so every line is full width except the last.
        let width = if align == quill_core::Align::Justify && row < 3 { full } else { width };
        let x = if align == quill_core::Align::Justify { area.left() } else { x };
        painter.line_segment([Pos2::new(x, y), Pos2::new(x + width, y)], stroke);
    }
}

/// An arc with an arrow head, pointing back for undo and forward for redo.
pub fn undo_redo(painter: &egui::Painter, centre: Pos2, forward: bool, color: Color32) {
    let radius = 5.0;
    let stroke = Stroke::new(1.6, color);
    // Three quarters of a circle, drawn as a run of short lines.
    let mut points = Vec::new();
    let steps = 14;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        // Start at the top and sweep round, leaving a gap where the arrow head goes.
        let angle = std::f32::consts::PI * (0.15 + 1.55 * t);
        let x = angle.cos() * radius;
        let y = angle.sin() * radius;
        points.push(Pos2::new(centre.x + if forward { -x } else { x }, centre.y - y));
    }
    painter.add(egui::Shape::line(points.clone(), stroke));
    // The arrow head sits at the start of the sweep.
    let tip = points[0];
    let direction = if forward { -1.0 } else { 1.0 };
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            Pos2::new(tip.x + 3.4 * direction, tip.y - 1.0),
            Pos2::new(tip.x + 0.6 * direction, tip.y + 3.4),
        ],
        color,
        Stroke::NONE,
    ));
}

/// A circle filled on one side, which is how the design marks the background opacity control.
pub fn half_filled_circle(painter: &egui::Painter, centre: Pos2, radius: f32, color: Color32) {
    painter.circle_stroke(centre, radius, Stroke::new(1.4, color));
    let mut points = vec![Pos2::new(centre.x, centre.y - radius)];
    let steps = 12;
    for step in 0..=steps {
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / steps as f32;
        points.push(Pos2::new(centre.x + angle.cos() * radius, centre.y + angle.sin() * radius));
    }
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A circle with a handle, in front of the box that filters the file list.
pub fn magnifier(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.circle_stroke(Pos2::new(centre.x - 0.8, centre.y - 0.8), 3.4, stroke);
    painter.line_segment(
        [Pos2::new(centre.x + 1.6, centre.y + 1.6), Pos2::new(centre.x + 4.0, centre.y + 4.0)],
        stroke,
    );
}

/// Two crossed diagonal lines, for a button that closes something.
pub fn cross(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let reach = 4.0;
    painter.line_segment(
        [
            Pos2::new(centre.x - reach, centre.y - reach),
            Pos2::new(centre.x + reach, centre.y + reach),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(centre.x - reach, centre.y + reach),
            Pos2::new(centre.x + reach, centre.y - reach),
        ],
        stroke,
    );
}

/// A tick, for a box that is ticked. Drawn rather than lettered, like every other icon here.
pub fn tick(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.8, color);
    painter.line_segment(
        [Pos2::new(centre.x - 4.0, centre.y), Pos2::new(centre.x - 1.2, centre.y + 3.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x - 1.2, centre.y + 3.0), Pos2::new(centre.x + 4.0, centre.y - 3.2)],
        stroke,
    );
}

/// A circle with two hands, for the button that offers the recent commit messages.
pub fn clock(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.circle_stroke(centre, 5.0, stroke);
    painter.line_segment([centre, Pos2::new(centre.x, centre.y - 3.2)], stroke);
    painter.line_segment([centre, Pos2::new(centre.x + 2.6, centre.y + 0.6)], stroke);
}

/// A branch: a line with a second one leaving it, for anything about git.
pub fn branch(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.line_segment(
        [Pos2::new(centre.x - 3.0, centre.y - 5.0), Pos2::new(centre.x - 3.0, centre.y + 5.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x - 3.0, centre.y), Pos2::new(centre.x + 3.5, centre.y - 3.5)],
        stroke,
    );
    painter.circle_filled(Pos2::new(centre.x - 3.0, centre.y + 5.0), 1.8, color);
    painter.circle_filled(Pos2::new(centre.x + 3.5, centre.y - 3.5), 1.8, color);
}

/// Two crossed lines.
pub fn plus(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    painter.line_segment(
        [Pos2::new(centre.x - 4.0, centre.y), Pos2::new(centre.x + 4.0, centre.y)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x, centre.y - 4.0), Pos2::new(centre.x, centre.y + 4.0)],
        stroke,
    );
}

/// An arrow pointing into a corner, for the button that hides the explorer.
pub fn collapse(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let a = Pos2::new(centre.x + 3.5, centre.y - 3.5);
    let b = Pos2::new(centre.x - 3.5, centre.y + 3.5);
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, Pos2::new(b.x + 4.5, b.y)], stroke);
    painter.line_segment([b, Pos2::new(b.x, b.y - 4.5)], stroke);
}

/// The three view modes, drawn as small pictures of what each one shows.
///
/// Raw Markdown is a page of even lines. Side by side is a page split down the middle. Preview is a
/// page with a heading bar above its lines. Drawn rather than lettered, to match the alignment buttons.
pub fn view_mode(painter: &egui::Painter, area: Rect, mode: crate::app::ViewMode, color: Color32) {
    use crate::app::ViewMode;
    let stroke = Stroke::new(1.2, color);
    // The page.
    painter.rect_stroke(area, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
    let inner = area.shrink(3.5);
    let line = |from: Pos2, to: Pos2| painter.line_segment([from, to], Stroke::new(1.1, color));
    match mode {
        ViewMode::Raw => {
            // Three even lines, the same on every row, because raw Markdown is just text.
            for row in 0..3 {
                let y = inner.top() + inner.height() * row as f32 / 2.0;
                line(Pos2::new(inner.left(), y), Pos2::new(inner.right(), y));
            }
        }
        ViewMode::SideBySide => {
            // A line down the middle, with rows either side of it.
            let middle = area.center().x;
            painter.line_segment(
                [Pos2::new(middle, area.top() + 1.0), Pos2::new(middle, area.bottom() - 1.0)],
                stroke,
            );
            for row in 0..3 {
                let y = inner.top() + inner.height() * row as f32 / 2.0;
                line(Pos2::new(inner.left(), y), Pos2::new(middle - 2.0, y));
                line(Pos2::new(middle + 2.0, y), Pos2::new(inner.right(), y));
            }
        }
        ViewMode::Preview => {
            // A thick heading bar, then two thinner lines, which is what a rendered page looks like.
            let bar = Rect::from_min_size(
                inner.left_top(),
                egui::Vec2::new(inner.width() * 0.62, 2.6),
            );
            painter.rect_filled(bar, CornerRadius::same(1), color);
            for row in 1..3 {
                let y = inner.top() + inner.height() * row as f32 / 2.0 + 1.0;
                let width = if row == 2 { inner.width() * 0.75 } else { inner.width() };
                line(Pos2::new(inner.left(), y), Pos2::new(inner.left() + width, y));
            }
        }
    }
}

/// An arrow with a head at each end, in front of the line spacing control.
pub fn line_spacing(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let top = Pos2::new(centre.x, centre.y - 5.0);
    let bottom = Pos2::new(centre.x, centre.y + 5.0);
    painter.line_segment([top, bottom], stroke);
    for (point, direction) in [(top, 1.0), (bottom, -1.0)] {
        painter.line_segment(
            [point, Pos2::new(point.x - 2.2, point.y + 2.6 * direction)],
            stroke,
        );
        painter.line_segment(
            [point, Pos2::new(point.x + 2.2, point.y + 2.6 * direction)],
            stroke,
        );
    }
}
