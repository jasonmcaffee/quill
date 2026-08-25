//! Radar charts: `radar` and `radar-beta`.
//!
//! A polygon graticule with one spoke per axis and a closed curve per series, with a legend.
//!
//! Values may be written as a plain list in the order the axes were declared, or as
//! `{ axis3: 30, axis1: 20 }` — named, and so in any order. Both end up as one value per axis, with
//! anything unnamed sitting at the middle rather than being left out, because a curve with a hole in
//! it is not a shape.

use std::collections::HashMap;

use super::parts;
use super::scene::{Anchor, Dash, Item, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How far the outermost ring is from the middle.
const RADIUS: f32 = 150.0;
/// How much room the axis names take outside the outermost ring.
const NAMES: f32 = 70.0;

/// One curve.
#[derive(Debug, Clone, PartialEq)]
struct Curve {
    name: String,
    /// One value an axis, in the order the axes were declared.
    values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    title: Option<String>,
    /// The identifier of each axis, and the words shown for it.
    axes: Vec<(String, String)>,
    curves: Vec<Curve>,
    lowest: Option<f32>,
    highest: Option<f32>,
    /// How many rings are drawn.
    rings: usize,
    /// True when the graticule is circles rather than polygons.
    round: bool,
    legend: bool,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let chart = read(source)?;
    Ok(draw(&chart, source, options))
}

fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart { rings: 5, legend: true, ..Chart::default() };
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            chart.title = Some(source::label(rest));
            continue;
        }
        if let Some(rest) = line.after_word("axis") {
            for piece in source::split_outside_quotes(rest, ',') {
                if let Some(axis) = read_axis(&piece) {
                    chart.axes.push(axis);
                }
            }
            continue;
        }
        if let Some(rest) = line.after_word("curve") {
            chart.curves.push(read_curve(&chart, rest, line)?);
            continue;
        }
        for (word, into) in [("max", &mut chart.highest), ("min", &mut chart.lowest)] {
            if let Some(rest) = line.after_word(word) {
                *into = rest.trim().parse::<f32>().ok();
            }
        }
        if let Some(rest) = line.after_word("ticks") {
            chart.rings = rest.trim().parse::<usize>().unwrap_or(5).clamp(1, 12);
            continue;
        }
        if let Some(rest) = line.after_word("graticule") {
            chart.round = rest.trim().eq_ignore_ascii_case("circle");
            continue;
        }
        if let Some(rest) = line.after_word("showLegend") {
            chart.legend = !rest.trim().eq_ignore_ascii_case("false");
            continue;
        }
    }
    Ok(chart)
}

/// `id["The words"]`, or just `id`.
fn read_axis(piece: &str) -> Option<(String, String)> {
    let piece = piece.trim();
    if piece.is_empty() {
        return None;
    }
    match (piece.find('['), piece.rfind(']')) {
        (Some(open), Some(close)) if close > open => Some((
            piece[..open].trim().to_owned(),
            source::label(&piece[open + 1..close]),
        )),
        _ => Some((piece.to_owned(), source::label(piece))),
    }
}

/// `curve id["The name"]{1, 2, 3}` or `curve id{ axis3: 30, axis1: 20 }`.
fn read_curve(chart: &Chart, rest: &str, line: &super::source::Line) -> Result<Curve, Problem> {
    let rest = rest.trim();
    let Some(open) = rest.find('{') else {
        return Err(Problem::at(
            line,
            "a curve looks like `curve Team A{4, 3, 5}` — its values go in curly brackets.",
        ));
    };
    let Some(close) = rest.rfind('}') else {
        return Err(Problem::at(line, "this curve's brackets were never closed"));
    };
    let name = read_axis(&rest[..open]).map(|(_, shown)| shown).unwrap_or_default();
    let pieces = source::split_outside_quotes(&rest[open + 1..close], ',');
    // Named values may come in any order, so they are collected first and then laid against the
    // axes. An axis nothing named sits at the middle: a curve with a hole in it is not a shape.
    let mut named: HashMap<String, f32> = HashMap::new();
    let mut listed: Vec<f32> = Vec::new();
    for piece in &pieces {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        if let Some((key, value)) = piece.split_once(':') {
            let Ok(value) = value.trim().parse::<f32>() else {
                return Err(Problem::at(line, format!("`{}` is not a number", value.trim())));
            };
            named.insert(key.trim().to_owned(), value);
            continue;
        }
        let Ok(value) = piece.parse::<f32>() else {
            return Err(Problem::at(line, format!("`{piece}` is not a number")));
        };
        listed.push(value);
    }
    let values = if named.is_empty() {
        listed
    } else {
        chart
            .axes
            .iter()
            .map(|(id, _)| named.get(id).copied().unwrap_or(0.0))
            .collect()
    };
    Ok(Curve { name, values })
}

fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&chart.title);
    }
    if chart.axes.len() < 3 {
        // Fewer than three spokes is not a radar chart; there is nothing to enclose.
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let size = (RADIUS + NAMES) * 2.0;
    let width = parts::MARGIN * 2.0 + size;
    let top = parts::title(&mut scene, &titled, options, width);
    let centre = Point::new(parts::MARGIN + size / 2.0, top + parts::MARGIN + size / 2.0);
    let (low, high) = range(chart);

    draw_graticule(&mut scene, chart, centre, options);
    draw_axis_names(&mut scene, chart, centre, options);
    draw_curves(&mut scene, chart, centre, low, high, options);
    if chart.legend {
        let entries: Vec<(String, crate::style::Color)> = chart
            .curves
            .iter()
            .enumerate()
            .filter(|(_, curve)| !curve.name.trim().is_empty())
            .map(|(index, curve)| (curve.name.clone(), options.theme.series(index)))
            .collect();
        if !entries.is_empty() {
            parts::legend(&mut scene, &entries, Point::new(parts::MARGIN, top + parts::MARGIN), options);
        }
    }
    scene.claim(Rect::new(0.0, 0.0, width, centre.y + size / 2.0));
    parts::finish(&mut scene);
    scene
}

/// The values the outermost ring and the middle stand for.
fn range(chart: &Chart) -> (f32, f32) {
    let low = chart.lowest.unwrap_or(0.0);
    let high = chart.highest.unwrap_or_else(|| {
        chart
            .curves
            .iter()
            .flat_map(|curve| curve.values.iter().copied())
            .fold(f32::NEG_INFINITY, f32::max)
            .max(low + 1.0)
    });
    if high <= low {
        (low, low + 1.0)
    } else {
        (low, high)
    }
}

/// Where one axis's outermost point is.
fn spoke(centre: Point, index: usize, count: usize, distance: f32) -> Point {
    // Straight up for the first axis, then clockwise, which is how every radar chart is read.
    let angle = index as f32 / count as f32 * std::f32::consts::TAU;
    Point::new(centre.x + distance * angle.sin(), centre.y - distance * angle.cos())
}

/// The rings and the spokes behind the curves.
fn draw_graticule(scene: &mut Scene, chart: &Chart, centre: Point, options: &Options) {
    let stroke = Stroke::new(options.theme.grid, 1.0);
    let count = chart.axes.len();
    for ring in 1..=chart.rings {
        let distance = RADIUS * ring as f32 / chart.rings as f32;
        let points: Vec<Point> = if chart.round {
            parts::arc(centre, distance, 0.0, std::f32::consts::TAU, parts::arc_steps(std::f32::consts::TAU))
        } else {
            (0..count).map(|index| spoke(centre, index, count, distance)).collect()
        };
        scene.add(Item::Polygon { points, fill: None, stroke: Some(stroke) });
    }
    for index in 0..count {
        scene.add(Item::Line {
            points: vec![centre, spoke(centre, index, count, RADIUS)],
            stroke,
            dash: Dash::Solid,
        });
    }
}

/// Each axis's name, outside the outermost ring.
fn draw_axis_names(scene: &mut Scene, chart: &Chart, centre: Point, options: &Options) {
    let style = parts::text_style(options, 0.85, false, options.theme.text);
    let measure = options.style(0.85, false);
    let count = chart.axes.len();
    for (index, (_, name)) in chart.axes.iter().enumerate() {
        let at = spoke(centre, index, count, RADIUS + 14.0);
        let width = text::width_of(name, &measure, options.metrics);
        // Anchored by which side of the middle it is on, so a name never runs back over the chart.
        let anchor = if at.x > centre.x + 4.0 {
            Anchor::Start
        } else if at.x < centre.x - 4.0 {
            Anchor::End
        } else {
            Anchor::Middle
        };
        parts::one_line(
            scene,
            name,
            Point::new(at.x, at.y - measure.size * 0.6),
            &style,
            anchor,
            width,
        );
    }
}

/// Each curve, as a filled and stroked closed shape with a dot on each spoke.
fn draw_curves(
    scene: &mut Scene,
    chart: &Chart,
    centre: Point,
    low: f32,
    high: f32,
    options: &Options,
) {
    let count = chart.axes.len();
    let span = (high - low).max(f32::EPSILON);
    for (index, curve) in chart.curves.iter().enumerate() {
        let points: Vec<Point> = (0..count)
            .map(|axis| {
                let value = curve.values.get(axis).copied().unwrap_or(low);
                let share = ((value - low) / span).clamp(0.0, 1.0);
                spoke(centre, axis, count, RADIUS * share)
            })
            .collect();
        scene.add(Item::Polygon {
            points: points.clone(),
            fill: Some(options.theme.wash(index, 60)),
            stroke: Some(Stroke::new(options.theme.series(index), parts::THICK * 0.7)),
        });
        for point in points {
            scene.add(Item::Circle {
                centre: point,
                radius: 3.5,
                fill: Some(super::scene::Paint::solid(options.theme.series(index))),
                stroke: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    fn chart(text: &str) -> Chart {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn axes_and_curves_are_read() {
        let text = "radar-beta\n title Performance\n axis Speed, Accuracy, Quality\n \
                    curve TeamA{4, 3, 5}\n curve TeamB{5, 4, 3}\n max 5\n";
        let chart = chart(text);
        assert_eq!(chart.title.as_deref(), Some("Performance"));
        assert_eq!(chart.axes.len(), 3);
        assert_eq!(chart.axes[0].1, "Speed");
        assert_eq!(chart.curves.len(), 2);
        assert_eq!(chart.curves[0].values, vec![4.0, 3.0, 5.0]);
        assert_eq!(chart.highest, Some(5.0));
    }

    #[test]
    fn an_axis_may_carry_its_own_words() {
        let chart = chart("radar-beta\n axis a[\"How fast\"], b[\"How right\"], c\n curve x{1,2,3}\n");
        assert_eq!(chart.axes[0], ("a".to_owned(), "How fast".to_owned()));
        assert_eq!(chart.axes[2], ("c".to_owned(), "c".to_owned()));
    }

    #[test]
    fn named_values_may_come_in_any_order() {
        let text = "radar-beta\n axis one, two, three\n curve x{ three: 30, one: 10, two: 20 }\n";
        let chart = chart(text);
        assert_eq!(chart.curves[0].values, vec![10.0, 20.0, 30.0], "laid against the axes in order");
    }

    #[test]
    fn an_axis_nothing_named_sits_at_the_middle_rather_than_leaving_a_hole() {
        let chart = chart("radar-beta\n axis one, two, three\n curve x{ one: 10 }\n");
        assert_eq!(chart.curves[0].values, vec![10.0, 0.0, 0.0]);
    }

    #[test]
    fn a_curve_that_is_not_numbers_says_which_line() {
        let problem = check::refused("radar-beta\n axis a, b, c\n curve x{1, two, 3}\n", &options());
        assert_eq!(problem.line, Some(3));
    }

    #[test]
    fn the_first_axis_points_straight_up() {
        let centre = Point::new(100.0, 100.0);
        let first = spoke(centre, 0, 4, 50.0);
        assert!((first.x - 100.0).abs() < 0.01 && (first.y - 50.0).abs() < 0.01);
        let quarter = spoke(centre, 1, 4, 50.0);
        assert!((quarter.x - 150.0).abs() < 0.01, "the second is a quarter turn clockwise");
    }

    #[test]
    fn a_radar_chart_is_drawn_and_keeps_every_property() {
        let text = "radar-beta\n title Performance\n\
            axis Speed, Accuracy, Efficiency, Reliability, Quality\n\
            curve TeamA[\"Team A\"]{4, 3, 5, 4, 3}\n curve TeamB[\"Team B\"]{5, 4, 3, 5, 4}\n\
            max 5\n graticule polygon\n ticks 5\n";
        check::drawn(
            text,
            &options(),
            &["Performance", "Speed", "Quality", "Team A", "Team B"],
        );
    }

    #[test]
    fn fewer_than_three_axes_is_not_a_radar_chart_and_draws_nothing_rather_than_a_line() {
        let scene = super::super::render("radar-beta\n axis a, b\n curve x{1,2}\n", &options())
            .expect("it should draw");
        assert!(scene.items.is_empty());
    }
}
