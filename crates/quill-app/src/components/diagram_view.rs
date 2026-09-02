//! Drawing a laid-out Mermaid diagram, and the gestures over one.
//!
//! `quill_core::mermaid` works out where everything goes and hands back a `Scene`: rectangles,
//! circles, polygons, lines and pieces of text at absolute positions. **This file knows nothing about
//! diagrams.** It draws those five kinds of item and nothing else, which is why a twenty-first
//! diagram type can be added to `quill-core` without a line changing here.
//!
//! A diagram is not text and does not scroll like text, so the gestures are a picture's: it is drawn
//! fit to the pane when it is larger than the pane and at its own size when it is smaller — never
//! blown up, which is what `fit` means everywhere else in Quill — the wheel scrolls it, a pinch or
//! the zoom modifier scales it, and a drag moves it. The same three `components::picture_view`
//! already has, because a diagram and a picture are the same kind of thing to a reader.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_core::mermaid::scene::{Anchor, Dash, Item, Paint, Point, Scene};

use crate::theme::color;

/// How far in or out one notch of a zoom gesture takes a diagram.
const ZOOM_STEP: f32 = 1.12;
/// The smallest and largest a diagram may be drawn, as a multiple of its natural size.
const SMALLEST: f32 = 0.15;
const LARGEST: f32 = 8.0;
/// How long the marks and gaps of a dashed line are on the screen.
const DASH_ON: f32 = 6.0;

/// Where a diagram has been moved and scaled to, kept between frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// How much larger than its natural size it is drawn. `None` until it has been fitted once.
    pub scale: Option<f32>,
    /// How far it has been dragged from the middle.
    pub offset: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { scale: None, offset: Vec2::ZERO }
    }
}

impl View {
    /// Forget where it was, which is what happens when the diagram itself changes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// What happened in the diagram this frame.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The pointer went down on it, so the editing area should take the keyboard.
    pub take_focus: bool,
}

/// Draw `scene` into `area`, and take the gestures over it.
pub fn show(ui: &mut egui::Ui, area: Rect, scene: &Scene, view: &mut View, name: &str) -> Outcome {
    let response = ui.interact(area, ui.id().with(("diagram", name)), Sense::click_and_drag());
    let mut outcome = Outcome::default();
    if response.clicked() || response.drag_started() {
        outcome.take_focus = true;
    }
    let fitted = fit(scene, area);
    let scale = view.scale.unwrap_or(fitted);
    handle_gestures(ui, &response, area, view, scale, fitted);
    let scale = view.scale.unwrap_or(fitted);

    let drawn = Vec2::new(scene.size.width * scale, scene.size.height * scale);
    let origin = Pos2::new(
        area.center().x - drawn.x / 2.0 + view.offset.x,
        area.center().y - drawn.y / 2.0 + view.offset.y,
    );
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    inner.set_clip_rect(ui.painter().clip_rect().intersect(area));
    paint(&inner, scene, origin, scale);
    // Named, so a screenshot test can find the diagram without knowing where it is.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Diagram: {name}"))
    });
    outcome
}

/// How much a diagram has to be scaled by to fit `area`.
///
/// Never above one: a small diagram is drawn at its own size rather than blown up to fill the pane,
/// which is what `services::picture` already decided for a picture in a tab and is what a reader
/// expects the word to mean.
pub fn fit(scene: &Scene, area: Rect) -> f32 {
    if scene.size.width <= 0.0 || scene.size.height <= 0.0 {
        return 1.0;
    }
    let across = area.width() / scene.size.width;
    let down = area.height() / scene.size.height;
    across.min(down).min(1.0).max(SMALLEST)
}

/// Take the wheel, the drag and the zoom.
fn handle_gestures(
    ui: &egui::Ui,
    response: &egui::Response,
    area: Rect,
    view: &mut View,
    scale: f32,
    fitted: f32,
) {
    if response.dragged() {
        view.offset += response.drag_delta();
    }
    // A double click puts it back to where it started, the way a double click on a splitter puts a
    // pane back to its usual size.
    if response.double_clicked() {
        view.reset();
        return;
    }
    let pointer_inside =
        ui.input(|input| input.pointer.hover_pos()).is_some_and(|at| area.contains(at));
    if !response.hovered() && !pointer_inside {
        return;
    }
    let (zoom, wheel) = ui.input(|input| (input.zoom_delta(), input.smooth_scroll_delta));
    if (zoom - 1.0).abs() > f32::EPSILON {
        view.scale = Some((scale * zoom).clamp(fitted.min(SMALLEST), LARGEST));
        return;
    }
    let modifier = ui.input(|input| input.modifiers.command);
    if modifier && wheel.y.abs() > f32::EPSILON {
        let step = if wheel.y > 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
        view.scale = Some((scale * step).clamp(fitted.min(SMALLEST), LARGEST));
        return;
    }
    if wheel != Vec2::ZERO {
        view.offset += wheel;
    }
}

/// Paint every item of the scene, with `origin` at the scene's own top left corner.
pub fn paint(ui: &egui::Ui, scene: &Scene, origin: Pos2, scale: f32) {
    let painter = ui.painter();
    let at = |point: &Point| Pos2::new(origin.x + point.x * scale, origin.y + point.y * scale);
    for item in &scene.items {
        match item {
            Item::Rect { rect, radius, fill, stroke } => {
                let drawn = Rect::from_min_size(
                    at(&Point::new(rect.x, rect.y)),
                    Vec2::new(rect.width * scale, rect.height * scale),
                );
                painter.rect(
                    drawn,
                    CornerRadius::same((radius * scale).round().clamp(0.0, 255.0) as u8),
                    paint_of(fill),
                    stroke_of(stroke, scale),
                    egui::StrokeKind::Inside,
                );
            }
            Item::Circle { centre, radius, fill, stroke } => {
                painter.circle(at(centre), radius * scale, paint_of(fill), stroke_of(stroke, scale));
            }
            Item::Polygon { points, fill, stroke } => {
                let drawn: Vec<Pos2> = points.iter().map(at).collect();
                painter.add(egui::Shape::convex_polygon(
                    drawn,
                    paint_of(fill),
                    stroke_of(stroke, scale),
                ));
            }
            Item::Line { points, stroke, dash } => {
                let drawn: Vec<Pos2> = points.iter().map(at).collect();
                paint_line(painter, &drawn, stroke, *dash, scale);
            }
            Item::Text { at: origin_of_text, text, style, anchor } => {
                let font = egui::FontId::new(
                    (style.size * scale).max(1.0),
                    if style.bold {
                        egui::FontFamily::Name(crate::theme::BOLD_FAMILY.into())
                    } else {
                        egui::FontFamily::Proportional
                    },
                );
                let colour = colour_of(style.color);
                let galley = painter.layout_no_wrap(text.clone(), font, colour);
                let left = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => galley.size().x / 2.0,
                    Anchor::End => galley.size().x,
                };
                let position = at(origin_of_text);
                painter.galley(Pos2::new(position.x - left, position.y), galley, colour);
            }
        }
    }
}

/// Draw one polyline, breaking it into marks when it is dashed.
///
/// egui has no dashed line of its own that takes a polyline, so a dashed line is drawn as a run of
/// short solid ones. Worked out here rather than in `quill-core` because how long a mark should be
/// depends on how large the diagram is being drawn, which only this side knows.
fn paint_line(
    painter: &egui::Painter,
    points: &[Pos2],
    stroke: &quill_core::mermaid::scene::Stroke,
    dash: Dash,
    scale: f32,
) {
    let drawn = Stroke::new((stroke.width * scale).max(0.6), colour_of(stroke.color));
    let Dash::Dashed(on, off) = dash else {
        painter.add(egui::Shape::line(points.to_vec(), drawn));
        return;
    };
    let (on, off) = ((on * scale).max(2.0), (off * scale).max(1.5));
    let _ = (DASH_ON, on);
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length <= f32::EPSILON {
            continue;
        }
        let step = on + off;
        let mut walked = 0.0;
        while walked < length {
            let from = walked / length;
            let to = ((walked + on) / length).min(1.0);
            painter.add(egui::Shape::line_segment(
                [pair[0].lerp(pair[1], from), pair[0].lerp(pair[1], to)],
                drawn,
            ));
            walked += step;
        }
    }
}

fn paint_of(fill: &Option<Paint>) -> Color32 {
    match fill {
        Some(paint) => Color32::from_rgba_unmultiplied(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            paint.alpha,
        ),
        None => Color32::TRANSPARENT,
    }
}

fn stroke_of(stroke: &Option<quill_core::mermaid::scene::Stroke>, scale: f32) -> Stroke {
    match stroke {
        Some(stroke) => Stroke::new((stroke.width * scale).max(0.6), colour_of(stroke.color)),
        None => Stroke::NONE,
    }
}

/// A diagram's colour as egui knows it. Always fully opaque, like every other glyph in Quill.
fn colour_of(colour: quill_core::Color) -> Color32 {
    Color32::from_rgb(colour.r, colour.g, colour.b)
}

/// Draw the panel that stands in for a diagram that could not be drawn.
///
/// Two different things are said differently, which is the whole reason `Problem` carries the
/// distinction: a mistake in the source is the author's and names the line it is on, and a diagram
/// type Quill does not draw yet is Quill's limitation and says so. Either way the **source is shown
/// underneath**, so nothing a person wrote disappears behind an error.
pub fn show_problem(
    ui: &egui::Ui,
    area: Rect,
    problem: &quill_core::mermaid::Problem,
    source: &str,
) {
    let painter = ui.painter();
    let panel = Rect::from_min_size(
        area.min + Vec2::new(12.0, 12.0),
        Vec2::new((area.width() - 24.0).max(0.0), (area.height() - 24.0).max(0.0)),
    );
    painter.rect(
        panel,
        CornerRadius::same(6),
        color::explorer(),
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
    let heading = if problem.unsupported { color::text_dim() } else { color::unsaved() };
    let mut y = panel.top() + 12.0;
    y += write(painter, panel, y, &problem.message(), heading, 14.0);
    if !problem.text.trim().is_empty() {
        y += write(painter, panel, y, &problem.text, color::text_dim(), 13.0);
    }
    y += 8.0;
    for line in source.lines().take(200) {
        if y > panel.bottom() - 14.0 {
            break;
        }
        y += write(painter, panel, y, line, color::text_dim(), 12.5);
    }
}

/// One line of the panel, returning how far down it took.
fn write(
    painter: &egui::Painter,
    panel: Rect,
    y: f32,
    words: &str,
    colour: Color32,
    size: f32,
) -> f32 {
    let galley = painter.layout(
        words.to_owned(),
        egui::FontId::proportional(size),
        colour,
        (panel.width() - 24.0).max(20.0),
    );
    let height = galley.size().y;
    painter.galley(Pos2::new(panel.left() + 12.0, y), galley, colour);
    height + 2.0
}
