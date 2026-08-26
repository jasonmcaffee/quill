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

/// An `F`, for the button that opens the text options.
///
/// Drawn from three strokes rather than set as a letter, which is what every other icon here does
/// and for a reason worth writing down, because `task-1657` offered to have an image generated for
/// it instead. Every icon in Quill is tinted where it is used — `TEXT_DIM` sitting there,
/// `TEXT_STRONG` while the flyout is open — and is drawn at whatever size the window is running at.
/// A picture can be neither tinted nor drawn at another scale without resampling it, so one image
/// among fifteen drawings would be the one that looked wrong. A letter is no better: the toolbar's
/// `B` is real text and needs the bold face bound before the first frame to look like anything.
///
/// The arms are the length the design's stroke weight wants: the upper one full width, the middle
/// one shorter, which is what tells an `F` from an `E` at ten points.
pub fn font(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let left = centre.x - 3.2;
    let top = centre.y - 5.0;
    let bottom = centre.y + 5.0;
    // The stem.
    painter.line_segment([Pos2::new(left, top), Pos2::new(left, bottom)], stroke);
    // The two arms.
    painter.line_segment([Pos2::new(left, top), Pos2::new(left + 6.4, top)], stroke);
    painter.line_segment(
        [Pos2::new(left, centre.y - 0.4), Pos2::new(left + 4.4, centre.y - 0.4)],
        stroke,
    );
}

/// A folder with a tab on it, for the button that shows and hides the file explorer.
///
/// Drawn from four strokes rather than filled, so it reads at the same weight as the branch and the
/// terminal beside it in the activity bar. The tab across the top left is what tells a folder from a
/// plain rectangle at ten points across.
pub fn folder(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let left = centre.x - 5.5;
    let right = centre.x + 5.5;
    let top = centre.y - 4.0;
    let bottom = centre.y + 4.5;
    // The tab, then the back edge it rises from.
    painter.line_segment([Pos2::new(left, top), Pos2::new(left + 3.4, top)], stroke);
    painter.line_segment([Pos2::new(left + 3.4, top), Pos2::new(left + 4.6, top + 1.6)], stroke);
    painter.line_segment([Pos2::new(left + 4.6, top + 1.6), Pos2::new(right, top + 1.6)], stroke);
    // The body.
    painter.line_segment([Pos2::new(left, top), Pos2::new(left, bottom)], stroke);
    painter.line_segment([Pos2::new(left, bottom), Pos2::new(right, bottom)], stroke);
    painter.line_segment([Pos2::new(right, top + 1.6), Pos2::new(right, bottom)], stroke);
}

/// A prompt: a chevron and an underscore, for the button that shows and hides the terminal.
pub fn terminal(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let left = centre.x - 5.0;
    // The chevron, pointing the way a shell prompt does.
    painter.line_segment(
        [Pos2::new(left, centre.y - 3.6), Pos2::new(left + 3.6, centre.y - 0.2)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(left + 3.6, centre.y - 0.2), Pos2::new(left, centre.y + 3.2)],
        stroke,
    );
    // The line waiting to be typed on.
    painter.line_segment(
        [Pos2::new(left + 5.6, centre.y + 3.6), Pos2::new(left + 10.0, centre.y + 3.6)],
        stroke,
    );
}

/// A picture: a frame with a hill and a sun in it, for a tab holding an image.
pub fn image(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let frame = Rect::from_center_size(centre, egui::Vec2::new(12.0, 10.0));
    painter.rect_stroke(frame, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
    painter.circle_filled(Pos2::new(frame.left() + 3.4, frame.top() + 3.0), 1.3, color);
    // The hill, which is what makes it read as a picture rather than as an empty box.
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(frame.left() + 1.6, frame.bottom() - 1.4),
            Pos2::new(frame.left() + 5.4, frame.bottom() - 5.0),
            Pos2::new(frame.right() - 1.6, frame.bottom() - 1.4),
        ],
        color,
        Stroke::NONE,
    ));
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

/// A colour wheel: a ring of six wedges in the six named hues, with a hole in the middle.
///
/// The one icon in Quill drawn in colours of its own rather than in the tint it is given. An icon
/// whose whole meaning is "any colour you like" cannot be one colour, and `color` is used for the
/// line round it so it still sits in the palette. The six are the corners of the hue ring the wheel
/// itself draws, so the button and what it opens are recognisably the same thing.
pub fn color_wheel(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let outer = 8.0;
    let inner = 3.4;
    let wedges = [
        Color32::from_rgb(0xFF, 0x00, 0x00),
        Color32::from_rgb(0xFF, 0xFF, 0x00),
        Color32::from_rgb(0x00, 0xFF, 0x00),
        Color32::from_rgb(0x00, 0xFF, 0xFF),
        Color32::from_rgb(0x00, 0x00, 0xFF),
        Color32::from_rgb(0xFF, 0x00, 0xFF),
    ];
    for (index, wedge) in wedges.iter().enumerate() {
        let from = index as f32 / wedges.len() as f32 * std::f32::consts::TAU;
        let to = (index + 1) as f32 / wedges.len() as f32 * std::f32::consts::TAU;
        // Four points rather than an arc: at this size the flat edge is a fraction of a pixel out
        // and a polygon is one shape where an arc is a mesh.
        let at = |angle: f32, radius: f32| {
            Pos2::new(centre.x + angle.cos() * radius, centre.y + angle.sin() * radius)
        };
        painter.add(egui::Shape::convex_polygon(
            vec![at(from, inner), at(from, outer), at(to, outer), at(to, inner)],
            *wedge,
            Stroke::NONE,
        ));
    }
    painter.circle_stroke(centre, outer, Stroke::new(1.0, color));
    painter.circle_filled(centre, inner, Color32::from_rgb(0x26, 0x2C, 0x36));
}

/// The five kinds of thing a definition can name, drawn rather than lettered.
///
/// A completion row says what it is offering with one of these, in front of the name. Drawn for the
/// reason every icon here is drawn: the characters editors usually use for these — a Greek phi for a
/// function, a bracket pair for a type — are not in the fonts Quill hands egui, and a missing glyph
/// renders as an empty box. Each one is the shape the thing is written as in code, which is what
/// makes five small marks tell each other apart at eleven points:
///
/// - a **function** is a pair of brackets, because that is what a call looks like;
/// - a **type** is a hollow square, the shape of a thing with an inside;
/// - a **constant** is a filled square, the same shape with nothing that can change in it;
/// - a **variable** is a small filled circle, the plainest mark there is;
/// - a **module** is three stacked lines, a folder's worth of things seen edge on.
pub fn symbol_kind(painter: &egui::Painter, centre: Pos2, kind: quill_core::SymbolKind, color: Color32) {
    use quill_core::SymbolKind;
    let stroke = Stroke::new(1.3, color);
    match kind {
        SymbolKind::Function => {
            // A pair of brackets: the tips point inwards and each bow reaches outwards, which is
            // `()`. Drawn the other way round it is `)(`, which is a different thing altogether and
            // is what the first version of this drew.
            for side in [-1.0_f32, 1.0] {
                let tip = centre.x + side * 1.4;
                let bow = centre.x + side * 3.4;
                painter.line_segment(
                    [Pos2::new(tip, centre.y - 4.2), Pos2::new(bow, centre.y - 1.8)],
                    stroke,
                );
                painter.line_segment(
                    [Pos2::new(bow, centre.y - 1.8), Pos2::new(bow, centre.y + 1.8)],
                    stroke,
                );
                painter.line_segment(
                    [Pos2::new(bow, centre.y + 1.8), Pos2::new(tip, centre.y + 4.2)],
                    stroke,
                );
            }
        }
        SymbolKind::Type => {
            painter.rect_stroke(
                Rect::from_center_size(centre, egui::Vec2::splat(7.6)),
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        SymbolKind::Constant => {
            painter.rect_filled(
                Rect::from_center_size(centre, egui::Vec2::splat(6.4)),
                CornerRadius::same(1),
                color,
            );
        }
        SymbolKind::Variable => {
            painter.circle_filled(centre, 3.0, color);
        }
        SymbolKind::Module => {
            for row in -1..=1 {
                let y = centre.y + row as f32 * 3.2;
                painter.line_segment(
                    [Pos2::new(centre.x - 4.0, y), Pos2::new(centre.x + 4.0, y)],
                    stroke,
                );
            }
        }
    }
}

/// A filled triangle pointing right: run.
///
/// `task-1683`'s widget and the run tile both use it, and so does each row of the flyout at a
/// smaller size, which is what `scale` is for. Filled rather than outlined because it is the one
/// control on the title bar that starts something, and green wherever it means "start this",
/// which is IntelliJ's own colour for the same button.
pub fn run(painter: &egui::Painter, centre: Pos2, color: Color32) {
    run_scaled(painter, centre, color, 1.0);
}

/// The same triangle at a fraction of its usual size.
pub fn run_scaled(painter: &egui::Painter, centre: Pos2, color: Color32, scale: f32) {
    let width = 4.6 * scale;
    let height = 5.2 * scale;
    // Nudged right by a fraction of the width, because a triangle looks off-centre when its
    // bounding box is centred: the eye reads the middle of the mass, not of the box.
    let x = centre.x - width / 3.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x, centre.y - height),
            Pos2::new(x, centre.y + height),
            Pos2::new(x + width * 1.7, centre.y),
        ],
        color,
        Stroke::NONE,
    ));
}

/// A filled square: stop.
pub fn stop(painter: &egui::Painter, centre: Pos2, color: Color32) {
    painter.rect_filled(Rect::from_center_size(centre, egui::Vec2::splat(9.0)), CornerRadius::same(1), color);
}

/// An arrow going round in a circle: rerun.
///
/// Three quarters of a circle with a head on the end, drawn as line segments rather than as an arc
/// shape, which is how `color_wheel` already draws a ring: egui has no arc primitive and a
/// polyline of a dozen points is indistinguishable from one at this size.
pub fn rerun(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let radius = 5.2;
    let stroke = Stroke::new(1.4, color);
    let mut points: Vec<Pos2> = Vec::new();
    // From just past the top, clockwise, stopping short of where it started so the gap the head
    // sits in is visible.
    for step in 0..=18 {
        let angle = -std::f32::consts::FRAC_PI_2 + 0.35
            + step as f32 / 18.0 * (std::f32::consts::TAU - 1.1);
        points.push(Pos2::new(centre.x + radius * angle.cos(), centre.y + radius * angle.sin()));
    }
    painter.add(egui::Shape::line(points, stroke));
    // The head, on the end that stopped short, pointing the way the arrow was going.
    let head = Pos2::new(centre.x + radius * 0.35_f32.cos(), centre.y - radius * 0.9);
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(head.x - 3.2, head.y - 1.0),
            Pos2::new(head.x + 1.4, head.y - 3.0),
            Pos2::new(head.x + 1.0, head.y + 2.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Three lines of output with a stroke through them: clear.
///
/// Drawn rather than lettered, for the reason the tick is: the characters that would say this are
/// not in the fonts Quill hands egui, and one that is missing renders as an empty box.
pub fn clear(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    for (row, width) in [(-4.0, 5.0), (0.0, 6.5), (4.0, 3.5)] {
        painter.line_segment(
            [
                Pos2::new(centre.x - 5.5, centre.y + row),
                Pos2::new(centre.x - 5.5 + width, centre.y + row),
            ],
            stroke,
        );
    }
    painter.line_segment(
        [Pos2::new(centre.x + 6.0, centre.y - 5.5), Pos2::new(centre.x - 1.0, centre.y + 5.5)],
        Stroke::new(1.5, color),
    );
}

/// A small filled circle, which is what says a run is going.
pub fn state_dot(painter: &egui::Painter, centre: Pos2, color: Color32) {
    painter.circle_filled(centre, 3.2, color);
}

/// How wide a breakpoint dot is drawn, which is what the gutter reserves for it.
pub const BREAKPOINT_RADIUS: f32 = 4.5;

/// The breakpoint dot in the gutter: filled while it is on, a ring while it is off or while the
/// adapter has not bound it.
///
/// Drawn rather than lettered, which is what the style guide asks for and what every other mark in
/// the gutter already is. A ring rather than a second colour, because what is different about an
/// unverified breakpoint is that it is hollow — the program has not agreed to stop there yet.
pub fn breakpoint(painter: &egui::Painter, centre: Pos2, filled: bool, color: Color32) {
    if filled {
        painter.circle_filled(centre, BREAKPOINT_RADIUS, color);
    } else {
        painter.circle_stroke(centre, BREAKPOINT_RADIUS - 0.75, Stroke::new(1.5, color));
    }
}

/// The mark a breakpoint carrying a condition or a log message wears: a halo round the dot.
///
/// IntelliJ puts a question mark on a conditional breakpoint, and a question mark is what this was
/// first drawn as — two short strokes at the dot's lower right. Blown up four times it read as a
/// **blob stuck to the dot** rather than as a mark, which at nine real pixels is what anybody would
/// see: there is no room inside or beside a nine-pixel circle for a glyph.
///
/// A ring round it has the one property that matters at this size — a silhouette that differs from
/// the plain dot's at a glance — and it cannot be confused with a **hollow** dot, because a hollow
/// one has nothing in the middle.
pub fn breakpoint_badge(painter: &egui::Painter, centre: Pos2, color: Color32) {
    painter.circle_stroke(
        centre,
        BREAKPOINT_RADIUS + 2.5,
        Stroke::new(1.2, color.gamma_multiply(0.7)),
    );
}

/// The bug on the activity bar's debug button: a body, three pairs of legs and two antennae.
///
/// Drawn rather than lettered, in the manner of every other icon here, and recognisable at the
/// eighteen points the rail draws its buttons at.
pub fn bug(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let body = Rect::from_center_size(centre, egui::Vec2::new(8.0, 10.0));
    painter.rect_stroke(body, CornerRadius::same(4), stroke, egui::StrokeKind::Middle);
    // The three pairs of legs, evenly down the body, which is what makes it read as an insect
    // rather than as a rounded rectangle.
    for step in 0..3 {
        let y = body.top() + 2.5 + step as f32 * 2.75;
        painter
            .line_segment([Pos2::new(body.left(), y), Pos2::new(body.left() - 3.0, y - 1.0)], stroke);
        painter.line_segment(
            [Pos2::new(body.right(), y), Pos2::new(body.right() + 3.0, y - 1.0)],
            stroke,
        );
    }
    painter.line_segment(
        [Pos2::new(centre.x - 1.5, body.top()), Pos2::new(centre.x - 3.5, body.top() - 3.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x + 1.5, body.top()), Pos2::new(centre.x + 3.5, body.top() - 3.0)],
        stroke,
    );
}

/// Resume: the play triangle with a bar in front of it, which is what every debugger draws.
pub fn resume(painter: &egui::Painter, centre: Pos2, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(centre.x - 6.0, centre.y - 5.0), egui::Vec2::new(2.0, 10.0)),
        CornerRadius::same(1),
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(centre.x - 1.0, centre.y - 5.5),
            Pos2::new(centre.x + 6.0, centre.y),
            Pos2::new(centre.x - 1.0, centre.y + 5.5),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Which of the three stepping pictures [`step`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepIcon {
    Over,
    Into,
    Out,
}

/// The three stepping icons, which differ only in where the arrow goes.
///
/// One function rather than three, because they are one picture with a parameter: a line the program
/// is on with the statement as a dot on it, and an arrow that goes over it, into it, or out of it.
/// Drawing them separately would be three chances for the three to stop looking like a set.
pub fn step(painter: &egui::Painter, centre: Pos2, kind: StepIcon, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let base = centre.y + 5.0;
    painter.line_segment([Pos2::new(centre.x - 6.0, base), Pos2::new(centre.x + 6.0, base)], stroke);
    match kind {
        StepIcon::Over => {
            // An arc that hops over the dot, as three segments: a real arc would be a dozen points
            // for a shape eleven points wide, and a glyph is not an option here.
            painter.line_segment(
                [Pos2::new(centre.x - 5.0, base - 1.0), Pos2::new(centre.x - 2.5, base - 5.5)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(centre.x - 2.5, base - 5.5), Pos2::new(centre.x + 2.5, base - 5.5)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(centre.x + 2.5, base - 5.5), Pos2::new(centre.x + 5.0, base - 1.0)],
                stroke,
            );
            painter.circle_filled(Pos2::new(centre.x, base - 1.5), 1.8, color);
        }
        StepIcon::Into => {
            painter.line_segment(
                [Pos2::new(centre.x, base - 9.0), Pos2::new(centre.x, base - 3.5)],
                stroke,
            );
            arrow_head(painter, Pos2::new(centre.x, base - 1.5), 1.0, color);
        }
        StepIcon::Out => {
            painter.line_segment(
                [Pos2::new(centre.x, base - 1.5), Pos2::new(centre.x, base - 7.5)],
                stroke,
            );
            arrow_head(painter, Pos2::new(centre.x, base - 9.5), -1.0, color);
        }
    }
}

/// A small filled triangle pointing up or down, which is the head of the two stepping arrows.
fn arrow_head(painter: &egui::Painter, tip: Pos2, direction: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            Pos2::new(tip.x - 3.0, tip.y - 3.5 * direction),
            Pos2::new(tip.x + 3.0, tip.y - 3.5 * direction),
        ],
        color,
        Stroke::NONE,
    ));
}
