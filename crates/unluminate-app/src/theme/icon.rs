//! Drawn icons. The design uses shapes rather than letters for the alignment buttons, for undo and redo
//! and for the small controls in the explorer, and the characters for those are not in egui's default
//! fonts, so they are drawn here.
//!
//! ## Ten of them have two drawings, and the theme says which
//!
//! `task-1776` asks for the marks on the rail and the explorer's folder arrow to be improved and to be
//! themeable. Themeable in **colour** they already are, and more so since the palette gained
//! `color::icon`, `color::folder` and the three beside them: every icon here is tinted where it is used.
//! What a theme now also chooses is the **shape**, from [`super::IconSet`]:
//!
//! - `unluminate` — the marks Unluminate shipped with: outlines at a 1.3 to 1.6 point stroke, and a solid triangle
//!   for a disclosure.
//! - `material` — heavier and rounder, filled where the Unluminate one is a stroke, and a **chevron** for a
//!   disclosure, which is what the reference editor, VS Code and every Material icon set draw. A stroke meeting at a
//!   point reads as something to press; a filled triangle reads as a bullet.
//!
//! The two drawings of one mark sit next to each other rather than in two modules, because what is worth
//! reading is *how a folder differs between the sets*, not what one set holds. A mark with one drawing —
//! the alignment buttons, undo and redo, the debugger's steps, the symbol kinds — is unchanged and asks
//! for no second one: the ticket is about the rail and the explorer, and fifty icons redrawn twice would
//! be fifty chances to make one of them worse.
//!
//! **Nothing here is a picture, and that is the style guide's rule rather than a preference.** Each of
//! these is drawn in three colours depending on its state and at any zoom, and a bitmap is one colour at
//! one size. `design/icons.md` records how the `material` set's design sheet was generated with Krea 2
//! and what was measured off it.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke};

use super::IconSet;

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
    disclosure_at(painter, centre, open, color, 1.0);
}

/// The same, `scale` times as large.
///
/// **Every icon in this file is drawn from numbers, and a zoom has to reach them.** `task-1771` makes each
/// pane zoomable, and an explorer whose lettering grew while its arrows and its magnifier stayed at eight
/// points would read as a bug rather than as a zoom. Only the three the explorer draws take a scale, and
/// the plain form of each is the scaled one at one — so nothing else in the window changes and there is one
/// shape rather than two that can drift apart.
pub fn disclosure_at(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32, scale: f32) {
    match super::icons() {
        IconSet::Classic => classic_disclosure(painter, centre, open, color, scale),
        IconSet::Material => material_disclosure(painter, centre, open, color, scale),
    }
}

/// The triangle Unluminate shipped with.
fn classic_disclosure(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32, scale: f32) {
    let at = |x: f32, y: f32| Pos2::new(centre.x + x * scale, centre.y + y * scale);
    let points = if open {
        vec![at(-4.0, -2.0), at(4.0, -2.0), at(0.0, 3.0)]
    } else {
        vec![at(-2.0, -4.0), at(-2.0, 4.0), at(3.0, 0.0)]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A chevron: two strokes meeting at a point, with round caps.
///
/// **The mark `task-1776` names.** It is a little narrower than the triangle it replaces — 3.2 points
/// either side of the point rather than 4 — because a chevron reads at its corner and the triangle read
/// at its mass, and the explorer's rows are 18 points apart at one level of indent.
fn material_disclosure(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32, scale: f32) {
    let at = |x: f32, y: f32| Pos2::new(centre.x + x * scale, centre.y + y * scale);
    let points = if open {
        vec![at(-3.4, -1.6), at(0.0, 2.0), at(3.4, -1.6)]
    } else {
        vec![at(-1.6, -3.4), at(2.0, 0.0), at(-1.6, 3.4)]
    };
    painter.add(egui::Shape::line(points, Stroke::new(1.6 * scale, color)));
}

/// How much room a folder's own mark takes in front of its name, in points, at this scale.
///
/// Zero for the `unluminate` set, which draws no folder mark at all — the explorer has never had one, and a
/// row that gained one under every theme would be a change nobody asked for. The explorer asks this
/// rather than assuming, so the name sits against the arrow under one set and past the folder under the
/// other, and nothing hard-codes which set is on.
pub fn folder_mark_width(scale: f32) -> f32 {
    match super::icons() {
        IconSet::Classic => 0.0,
        IconSet::Material => 16.0 * scale,
    }
}

/// The mark in front of a folder's name in the explorer, when the set draws one.
///
/// This is Atom Material Icons' whole idea, and it is why `color::folder` and `color::folder_open` are
/// two roles: a folder that is open is drawn in the accent, so the path down to what you are reading is
/// visible without reading any of the names.
pub fn folder_mark(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32, scale: f32) {
    if super::icons() == IconSet::Classic {
        return;
    }
    material_folder(painter, centre, open, color, scale * 0.85);
}

/// A filled folder, with a raised tab, and leaning open when it is.
///
/// One drawing behind both the rail's button and the explorer's mark, so the two cannot drift apart.
fn material_folder(painter: &egui::Painter, centre: Pos2, open: bool, color: Color32, scale: f32) {
    let at = |x: f32, y: f32| Pos2::new(centre.x + x * scale, centre.y + y * scale);
    let round = (2.0 * scale).max(1.0) as u8;
    // **Open and shut differ by a gap, not by a second shape.** The design sheet draws the open folder as
    // the closed one with the tab lifted a point and a half clear of the body, and that is the whole
    // difference — which matters here because an icon has one colour, so a lid drawn *over* the body
    // would be invisible and one drawn beside it would make the mark wider on alternate rows.
    let lift = if open { 1.6 } else { 0.0 };
    painter.rect_filled(
        Rect::from_min_max(at(-6.0, -5.0 - lift), at(-0.4, -3.0 - lift)),
        CornerRadius { nw: round, ne: round, sw: 0, se: 0 },
        color,
    );
    // The body's top **left** corner is square when the folder is shut, because the tab sits on it: a
    // rounded one leaves a notch between the two shapes that reads as a bite taken out of the mark. Open,
    // the tab has lifted clear and the corner is rounded like the other three.
    painter.rect_filled(
        Rect::from_min_max(at(-6.0, -3.4), at(6.0, 5.0)),
        CornerRadius { nw: if open { round } else { 0 }, ne: round, sw: round, se: round },
        color,
    );
}

/// Four stacked lines showing how a paragraph is placed. The short lines sit where the ragged edge
/// would be, which is what makes the four buttons tell each other apart.
pub fn alignment(painter: &egui::Painter, area: Rect, align: unluminate_core::Align, color: Color32) {
    let full = area.width();
    let short = full * 0.62;
    let spacing = area.height() / 3.0;
    let stroke = Stroke::new(1.6, color);
    for row in 0..4 {
        let y = area.top() + spacing * row as f32;
        // Rows 1 and 3 are the short ones, so the shape reads as a paragraph of text.
        let width = if row % 2 == 1 { short } else { full };
        let x = match align {
            unluminate_core::Align::Left | unluminate_core::Align::Justify => area.left(),
            unluminate_core::Align::Center => area.left() + (full - width) / 2.0,
            unluminate_core::Align::Right => area.right() - width,
        };
        // Justified text is flush on both sides, so every line is full width except the last.
        let width = if align == unluminate_core::Align::Justify && row < 3 { full } else { width };
        let x = if align == unluminate_core::Align::Justify { area.left() } else { x };
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
    magnifier_at(painter, centre, color, 1.0);
}

/// The same, `scale` times as large. See [`disclosure_at`].
pub fn magnifier_at(painter: &egui::Painter, centre: Pos2, color: Color32, scale: f32) {
    let stroke = Stroke::new(1.3 * scale, color);
    let at = |x: f32, y: f32| Pos2::new(centre.x + x * scale, centre.y + y * scale);
    painter.circle_stroke(at(-0.8, -0.8), 3.4 * scale, stroke);
    painter.line_segment([at(1.6, 1.6), at(4.0, 4.0)], stroke);
}

/// A waste bin: a lid, a body and two lines down it.
///
/// Drawn rather than lettered, which is `design/style-guide.md`'s rule for every mark in this file. It is
/// beside the one control on a ticket that destroys work, where a word on its own reads as one more field.
pub fn bin(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let (w, h) = (4.0, 5.0);
    // The lid, with the little handle over it.
    painter.line_segment(
        [Pos2::new(centre.x - w - 1.0, centre.y - h + 1.0), Pos2::new(centre.x + w + 1.0, centre.y - h + 1.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x - 1.6, centre.y - h - 1.0), Pos2::new(centre.x + 1.6, centre.y - h - 1.0)],
        stroke,
    );
    // The body, as three sides of a box that narrows towards the bottom.
    painter.line_segment(
        [Pos2::new(centre.x - w, centre.y - h + 2.0), Pos2::new(centre.x - w + 0.8, centre.y + h)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x + w, centre.y - h + 2.0), Pos2::new(centre.x + w - 0.8, centre.y + h)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x - w + 0.8, centre.y + h), Pos2::new(centre.x + w - 0.8, centre.y + h)],
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
    match super::icons() {
        IconSet::Classic => classic_branch(painter, centre, color),
        IconSet::Material => material_branch(painter, centre, color),
    }
}

/// The same branch at the set's weight: heavier strokes with round caps and larger discs.
///
/// The design sheet's own attempt at this one was the weakest thing on it — it came back as an X with
/// four dots, which says nothing about git — so this keeps the shape Unluminate already had and only takes the
/// weight and the caps from the sheet. That is what a design reference is for: the parts of it that are
/// better are copied and the parts that are not are not.
fn material_branch(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let at = |x: f32, y: f32| Pos2::new(centre.x + x, centre.y + y);
    let stroke = Stroke::new(1.6, color);
    // Three commits and the line between them, which is what a branch is: a stem with one at each end,
    // and a third off to the side that the stem forks to. The fork is a polyline with a corner in it
    // rather than a diagonal, so at ten points across it reads as a branch and not as a letter.
    painter.line_segment([at(-4.4, -3.2), at(-4.4, 3.2)], stroke);
    painter.add(egui::Shape::line(
        vec![at(-4.4, 1.6), at(0.6, 1.6), at(4.4, -2.2), at(4.4, -3.0)],
        stroke,
    ));
    for dot in [at(-4.4, -5.0), at(-4.4, 5.0), at(4.4, -5.0)] {
        painter.circle_filled(dot, 2.2, color);
    }
}

/// The branch Unluminate shipped with.
fn classic_branch(painter: &egui::Painter, centre: Pos2, color: Color32) {
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
    collapse_at(painter, centre, color, 1.0);
}

/// The same, `scale` times as large. See [`disclosure_at`].
pub fn collapse_at(painter: &egui::Painter, centre: Pos2, color: Color32, scale: f32) {
    let stroke = Stroke::new(1.4 * scale, color);
    let at = |x: f32, y: f32| Pos2::new(centre.x + x * scale, centre.y + y * scale);
    let a = at(3.5, -3.5);
    let b = at(-3.5, 3.5);
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, Pos2::new(b.x + 4.5 * scale, b.y)], stroke);
    painter.line_segment([b, Pos2::new(b.x, b.y - 4.5 * scale)], stroke);
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
/// it instead. Every icon in Unluminate is tinted where it is used — `TEXT_DIM` sitting there,
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
    match super::icons() {
        IconSet::Classic => classic_folder(painter, centre, color),
        IconSet::Material => material_folder(painter, centre, false, color, 1.0),
    }
}

/// The outlined folder Unluminate shipped with.
fn classic_folder(painter: &egui::Painter, centre: Pos2, color: Color32) {
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

/// The editing area: a panel with a tab along the top of it, for the button that shows and hides it.
///
/// `task-28` asks for a toggle under the folder icon for the pane holding the tabs. What that pane looks like
/// is a rectangle with one tab on its top edge, so that is what is drawn — the same outline the folder above it
/// is drawn with, at the same weight, so the two read as a pair.
pub fn editing_area(painter: &egui::Painter, centre: Pos2, color: Color32) {
    match super::icons() {
        IconSet::Classic => classic_editing_area(painter, centre, color),
        IconSet::Material => material_editing_area(painter, centre, color),
    }
}

/// Two filled slabs side by side, which is what the editing area **is**.
///
/// The design sheet drew it this way and it is a better mark than the one it replaces: Unluminate's editing
/// area is a row of panes, so two panes with a gap between them says what the button does, where a panel
/// with a tab on it says "a document" and could as easily have meant the explorer.
///
/// Nothing here is knocked out of a fill. An icon has one colour and is drawn over four different
/// grounds — the rail, the rail's own chosen pill, a menu row and a flyout — so a shape painted in "the
/// background" would be right in one place and wrong in the other three.
fn material_editing_area(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let round = CornerRadius::same(2);
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(centre.x - 6.0, centre.y - 4.5), Pos2::new(centre.x - 0.9, centre.y + 4.5)),
        round,
        color,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(centre.x + 0.9, centre.y - 4.5), Pos2::new(centre.x + 6.0, centre.y + 4.5)),
        round,
        color,
    );
}

/// The outlined panel Unluminate shipped with.
fn classic_editing_area(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let left = centre.x - 5.5;
    let right = centre.x + 5.5;
    let top = centre.y - 4.5;
    let bottom = centre.y + 4.5;
    // The tab, filled so it reads as the one that is showing rather than as a notch in the outline.
    let tab = Rect::from_min_max(Pos2::new(left, top), Pos2::new(left + 4.4, top + 2.4));
    painter.rect_filled(tab, CornerRadius::same(1), color);
    // The strip the tab sits on, and the body under it.
    painter.line_segment([Pos2::new(left, top + 2.4), Pos2::new(right, top + 2.4)], stroke);
    painter.line_segment([Pos2::new(left + 4.4, top), Pos2::new(right, top)], stroke);
    painter.line_segment([Pos2::new(left, top), Pos2::new(left, bottom)], stroke);
    painter.line_segment([Pos2::new(right, top), Pos2::new(right, bottom)], stroke);
    painter.line_segment([Pos2::new(left, bottom), Pos2::new(right, bottom)], stroke);
}

/// A prompt: a chevron and an underscore, for the button that shows and hides the terminal.
pub fn terminal(painter: &egui::Painter, centre: Pos2, color: Color32) {
    match super::icons() {
        IconSet::Classic => classic_terminal(painter, centre, color),
        IconSet::Material => material_terminal(painter, centre, color),
    }
}

/// A window with a filled title bar and a chevron inside it, which is the design sheet's shape.
///
/// The body is stroked at 1.6 and the bar and the chevron are filled, so the mark carries the set's
/// weight without any part of it being painted in the ground — see [`material_editing_area`].
fn material_terminal(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let body = Rect::from_center_size(centre, egui::Vec2::new(12.0, 10.0));
    painter.rect_stroke(body, CornerRadius::same(2), Stroke::new(1.6, color), egui::StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + 2.6)),
        CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 },
        color,
    );
    // The prompt, pointing the way a shell's does.
    painter.add(egui::Shape::line(
        vec![
            Pos2::new(body.left() + 3.0, body.top() + 5.0),
            Pos2::new(body.left() + 5.6, body.top() + 7.0),
            Pos2::new(body.left() + 3.0, body.top() + 9.0),
        ],
        Stroke::new(1.6, color),
    ));
}

/// The prompt Unluminate shipped with.
fn classic_terminal(painter: &egui::Painter, centre: Pos2, color: Color32) {
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
/// The one icon in Unluminate drawn in colours of its own rather than in the tint it is given. An icon
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
/// function, a bracket pair for a type — are not in the fonts Unluminate hands egui, and a missing glyph
/// renders as an empty box. Each one is the shape the thing is written as in code, which is what
/// makes five small marks tell each other apart at eleven points:
///
/// - a **function** is a pair of brackets, because that is what a call looks like;
/// - a **type** is a hollow square, the shape of a thing with an inside;
/// - a **constant** is a filled square, the same shape with nothing that can change in it;
/// - a **variable** is a small filled circle, the plainest mark there is;
/// - a **module** is three stacked lines, a folder's worth of things seen edge on.
pub fn symbol_kind(painter: &egui::Painter, centre: Pos2, kind: unluminate_core::SymbolKind, color: Color32) {
    use unluminate_core::SymbolKind;
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
/// which is the reference editor's own colour for the same button.
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
/// not in the fonts Unluminate hands egui, and one that is missing renders as an empty box.
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
/// The reference editor puts a question mark on a conditional breakpoint, and a question mark is what this was
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
    match super::icons() {
        IconSet::Classic => classic_bug(painter, centre, color),
        IconSet::Material => material_bug(painter, centre, color),
    }
}

/// The same insect with a filled body, which is how the design sheet drew it.
///
/// Filled bodies and stroked limbs is the set's own rule — a folder, a title bar and a bug's shell are
/// mass, and a leg, an antenna and a prompt are lines.
fn material_bug(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let body = Rect::from_center_size(centre, egui::Vec2::new(8.4, 10.0));
    painter.rect_filled(body, CornerRadius::same(4), color);
    for step in 0..3 {
        let y = body.top() + 2.5 + step as f32 * 2.75;
        painter.line_segment([Pos2::new(body.left(), y), Pos2::new(body.left() - 3.0, y - 1.0)], stroke);
        painter.line_segment([Pos2::new(body.right(), y), Pos2::new(body.right() + 3.0, y - 1.0)], stroke);
    }
    painter.line_segment(
        [Pos2::new(centre.x - 1.5, body.top() + 0.6), Pos2::new(centre.x - 3.6, body.top() - 3.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x + 1.5, body.top() + 0.6), Pos2::new(centre.x + 3.6, body.top() - 3.0)],
        stroke,
    );
}

/// The outlined insect Unluminate shipped with.
fn classic_bug(painter: &egui::Painter, centre: Pos2, color: Color32) {
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

/// A stack of rows of decreasing width: a list of things waiting, which is what a backlog is.
pub fn stack(painter: &egui::Painter, centre: Pos2, color: Color32) {
    for (row, width) in [(-4.0_f32, 6.0_f32), (0.0, 5.0), (4.0, 3.5)] {
        painter.line_segment(
            [Pos2::new(centre.x - width, centre.y + row), Pos2::new(centre.x + width, centre.y + row)],
            Stroke::new(1.6, color),
        );
    }
}

/// A diamond, which is what the reference board marks its epics with.
///
/// An outline rather than a filled shape, so it reads at the same weight as the tick and the stack beside
/// it — a solid lozenge at this size is a blob.
pub fn diamond(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let reach = 5.5;
    let points = [
        Pos2::new(centre.x, centre.y - reach),
        Pos2::new(centre.x + reach, centre.y),
        Pos2::new(centre.x, centre.y + reach),
        Pos2::new(centre.x - reach, centre.y),
    ];
    for pair in 0..4 {
        painter.line_segment([points[pair], points[(pair + 1) % 4]], Stroke::new(1.6, color));
    }
}

/// A speech mark: a small rounded box with a tail, which is what a comment count is drawn beside.
pub fn comment(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let box_rect = Rect::from_center_size(Pos2::new(centre.x, centre.y - 1.0), egui::Vec2::new(11.0, 8.0));
    painter.rect_stroke(
        box_rect,
        CornerRadius::same(2),
        Stroke::new(1.3, color),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [Pos2::new(centre.x - 2.0, box_rect.max.y), Pos2::new(centre.x - 3.5, box_rect.max.y + 3.0)],
        Stroke::new(1.3, color),
    );
}

/// Two overlapping sheets, which is what a copy button is drawn with.
///
/// Drawn rather than lettered, and drawn rather than borrowed: `icon::comment` was standing in for it
/// and a speech mark does not mean copy — beside a message it means the message.
pub fn copy(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let sheet = egui::Vec2::new(7.5, 9.0);
    // The one behind, offset up and left, drawn first so the front one covers its corner.
    painter.rect_stroke(
        Rect::from_center_size(Pos2::new(centre.x - 1.5, centre.y - 1.5), sheet),
        CornerRadius::same(2),
        Stroke::new(1.2, color),
        egui::StrokeKind::Inside,
    );
    let front = Rect::from_center_size(Pos2::new(centre.x + 1.5, centre.y + 1.5), sheet);
    painter.rect_filled(front, CornerRadius::same(2), color.gamma_multiply(0.0));
    painter.rect_stroke(front, CornerRadius::same(2), Stroke::new(1.4, color), egui::StrokeKind::Inside);
}

/// A speech bubble with two lines in it, which is what `pane.icon = chat` draws.
///
/// `comment` is the same idea at eleven points across, drawn beside a comment count on a card; this
/// one is the rail's size and has the lines in it, because at that size an empty bubble reads as a
/// rounded rectangle. Drawn rather than lettered, which is `design/style-guide.md`'s rule for every
/// icon here: a drawn icon takes the tint it is given and follows the rail's own three states.
pub fn chat(painter: &egui::Painter, centre: Pos2, color: Color32) {
    match super::icons() {
        IconSet::Classic => classic_chat(painter, centre, color),
        IconSet::Material => material_chat(painter, centre, color),
    }
}

/// A filled bubble with a tail, and no words in it.
///
/// The lines of words go with the outline: inside a filled bubble they would have to be painted in the
/// ground, which is the one thing this set does not do. What is left is the silhouette, which is what the
/// design sheet drew and what reads at ten points.
fn material_chat(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let bubble = Rect::from_center_size(Pos2::new(centre.x, centre.y - 1.0), egui::Vec2::new(13.0, 10.0));
    painter.rect_filled(bubble, CornerRadius::same(3), color);
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(centre.x - 4.6, bubble.max.y - 1.0),
            Pos2::new(centre.x - 1.2, bubble.max.y - 1.0),
            Pos2::new(centre.x - 4.0, bubble.max.y + 3.4),
        ],
        color,
        Stroke::NONE,
    ));
}

/// The outlined bubble Unluminate shipped with.
fn classic_chat(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let box_rect = Rect::from_center_size(Pos2::new(centre.x, centre.y - 1.0), egui::Vec2::new(13.0, 10.0));
    painter.rect_stroke(
        box_rect,
        CornerRadius::same(3),
        Stroke::new(1.4, color),
        egui::StrokeKind::Inside,
    );
    // The tail, down and to the left, which is what tells a bubble from a box at this size.
    painter.line_segment(
        [Pos2::new(centre.x - 2.5, box_rect.max.y), Pos2::new(centre.x - 4.5, box_rect.max.y + 3.5)],
        Stroke::new(1.4, color),
    );
    // Two lines of words in it. Inset by three so neither touches the stroke.
    for (index, share) in [1.0_f32, 0.62].into_iter().enumerate() {
        let y = box_rect.top() + 3.5 + index as f32 * 3.0;
        let left = box_rect.left() + 3.0;
        let width = (box_rect.width() - 6.0) * share;
        painter.line_segment([Pos2::new(left, y), Pos2::new(left + width, y)], Stroke::new(1.2, color));
    }
}

/// The four columns of a task board, which is what `pane.icon = board` draws.
///
/// Drawn rather than lettered, which is the rule `design/style-guide.md` sets for every icon here: a
/// drawn icon takes the tint it is given, so it follows the rail's own three states and the window's
/// colours rather than carrying a colour of its own.
pub fn board(painter: &egui::Painter, centre: Pos2, color: Color32) {
    match super::icons() {
        IconSet::Classic => classic_board(painter, centre, color),
        IconSet::Material => material_board(painter, centre, color),
    }
}

/// Three columns under a header bar, which is what a board looks like on the design sheet.
///
/// The bar is what tells it from a bar chart, and it is the one thing the Unluminate drawing is missing: four
/// columns of different heights with nothing over them is a chart, and a chart is not what the button
/// opens.
fn material_board(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let half = 5.5;
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(centre.x - half, centre.y - half), Pos2::new(centre.x + half, centre.y - 3.2)),
        CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 },
        color,
    );
    let column = 2.8;
    for (index, share) in [1.0_f32, 0.6, 0.82].into_iter().enumerate() {
        let x = centre.x - half + index as f32 * (column + 1.2);
        let height = (half + 3.2) * share;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, centre.y - 2.2), egui::Vec2::new(column, height)),
            CornerRadius::same(1),
            color,
        );
    }
}

/// The four columns Unluminate shipped with.
fn classic_board(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let half = 5.5;
    let column = 2.4;
    // Four columns of different heights, which is what a board with different numbers of cards in each
    // lane looks like at this size.
    let heights = [1.0, 0.62, 0.85, 0.45];
    for (index, share) in heights.into_iter().enumerate() {
        let x = centre.x - half + index as f32 * (column + 1.0);
        let height = half * 2.0 * share;
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x, centre.y - half),
                egui::Vec2::new(column, height),
            ),
            CornerRadius::same(1),
            color,
        );
    }
}

/// A database: the cylinder every tool in the world draws for one.
///
/// Drawn rather than lettered, which is the rule `design/style-guide.md` sets for every icon here: a
/// drawn icon takes the tint it is given, so it follows the rail's three states and the window's
/// colours rather than carrying a colour of its own. Three bands, because two read as a coin and four
/// as a stack of plates at sixteen points.
pub fn database(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let half_width = 5.5;
    let top = centre.y - 6.0;
    let bottom = centre.y + 4.5;
    // The top ellipse, drawn as a squashed circle: `egui` has no ellipse, so it is a short polyline
    // round one, which at this size is indistinguishable and takes the tint.
    for band in 0..3 {
        let y = top + band as f32 * 4.0;
        let points: Vec<Pos2> = (0..=16)
            .map(|step| {
                let angle = std::f32::consts::PI * step as f32 / 16.0;
                Pos2::new(centre.x - half_width * angle.cos(), y + 1.9 * angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.3, color)));
    }
    // The closing curve of the top, so the first band reads as an ellipse rather than a smile.
    let over: Vec<Pos2> = (0..=16)
        .map(|step| {
            let angle = std::f32::consts::PI * step as f32 / 16.0;
            Pos2::new(centre.x + half_width * angle.cos(), top - 1.9 * angle.sin())
        })
        .collect();
    painter.add(egui::Shape::line(over, Stroke::new(1.3, color)));
    // The two sides.
    for side in [-1.0_f32, 1.0] {
        painter.line_segment(
            [Pos2::new(centre.x + side * half_width, top), Pos2::new(centre.x + side * half_width, bottom - 2.0)],
            Stroke::new(1.3, color),
        );
    }
}

/// A table: a grid with a heavier first row, which is what a header is.
pub fn table(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let rect = Rect::from_center_size(centre, egui::Vec2::new(13.0, 11.0));
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.2, color), egui::StrokeKind::Inside);
    // The header rule, heavier than the rest, which is what tells a table from a window.
    let header = rect.top() + 3.5;
    painter.line_segment(
        [Pos2::new(rect.left(), header), Pos2::new(rect.right(), header)],
        Stroke::new(1.4, color),
    );
    painter.line_segment(
        [Pos2::new(rect.left(), header + 3.5), Pos2::new(rect.right(), header + 3.5)],
        Stroke::new(1.0, color),
    );
    let column = rect.left() + rect.width() * 0.45;
    painter.line_segment(
        [Pos2::new(column, rect.top()), Pos2::new(column, rect.bottom())],
        Stroke::new(1.0, color),
    );
}

/// An undo arrow, in the shape every icon button takes.
///
/// `undo_redo` already draws one and takes a direction; this is that with the direction bound, so it
/// can be passed to `controls::icon_button`, which takes a plain `fn(&Painter, Pos2, Color32)`.
pub fn undo(painter: &egui::Painter, centre: Pos2, color: Color32) {
    undo_redo(painter, centre, false, color);
}

/// A key, which is what marks the column a row is addressed by.
///
/// A tick was tried first and read as "this one is selected", which in a tree whose chosen row is
/// already a pill is exactly the wrong thing to say. A key says what the mark means without a
/// tooltip, which is what an icon is for.
pub fn key(painter: &egui::Painter, centre: Pos2, color: Color32) {
    let bow = Pos2::new(centre.x - 3.5, centre.y - 1.5);
    painter.circle_stroke(bow, 2.6, Stroke::new(1.3, color));
    // The shaft, running up and to the right out of the bow.
    let end = Pos2::new(centre.x + 4.5, centre.y - 4.0);
    painter.line_segment([Pos2::new(bow.x + 1.8, bow.y - 1.4), end], Stroke::new(1.3, color));
    // Two teeth on the underside of it.
    for along in [0.45_f32, 0.75] {
        let at = Pos2::new(
            bow.x + 1.8 + (end.x - bow.x - 1.8) * along,
            bow.y - 1.4 + (end.y - bow.y + 1.4) * along,
        );
        painter.line_segment([at, Pos2::new(at.x + 1.6, at.y + 2.4)], Stroke::new(1.3, color));
    }
}
