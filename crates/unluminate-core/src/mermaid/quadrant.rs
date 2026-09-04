//! Quadrant charts: `quadrantChart`.
//!
//! A square divided into four, with a label in each quadrant, the axis titles written at both ends of
//! both axes, and a labelled point for each row.
//!
//! Mermaid numbers the quadrants **anticlockwise from the top right**: one is top right, two is top
//! left, three is bottom left, four is bottom right. That is not the order anybody guesses, so it is
//! written down here and tested rather than left in the arithmetic.

use super::parts;
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How big the plotting square is.
const PLOT: f32 = 400.0;
/// How much room the axis titles take round it.
const GUTTER: f32 = 34.0;
/// How big a point's circle is when nothing says otherwise.
const DOT: f32 = 7.0;

/// One plotted point.
#[derive(Debug, Clone, PartialEq)]
struct Spot {
    label: String,
    /// Both from zero to one, with one at the right and at the **top**.
    x: f32,
    y: f32,
    radius: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    title: Option<String>,
    /// The words at the left and the right of the horizontal axis.
    x_axis: (String, String),
    /// The words at the bottom and the top of the vertical axis.
    y_axis: (String, String),
    /// Anticlockwise from the top right, which is Mermaid's own numbering.
    quadrants: [String; 4],
    spots: Vec<Spot>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let chart = read(source)?;
    Ok(draw(&chart, source, options))
}

fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart::default();
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            chart.title = Some(source::label(rest));
            continue;
        }
        if let Some(rest) = line.after_word("x-axis") {
            chart.x_axis = split_ends(rest);
            continue;
        }
        if let Some(rest) = line.after_word("y-axis") {
            chart.y_axis = split_ends(rest);
            continue;
        }
        let mut quadrant = None;
        for number in 1..=4 {
            if let Some(rest) = line.after_word(&format!("quadrant-{number}")) {
                quadrant = Some((number - 1, source::label(rest)));
            }
        }
        if let Some((index, words)) = quadrant {
            chart.quadrants[index] = words;
            continue;
        }
        if ["classDef", "class", "style"].iter().any(|word| line.starts_with_word(word)) {
            continue;
        }
        chart.spots.push(read_spot(line)?);
    }
    Ok(chart)
}

/// `Reach --> More reach` becomes the words at each end of an axis.
fn split_ends(rest: &str) -> (String, String) {
    match rest.split_once("-->") {
        Some((left, right)) => (source::label(left), source::label(right)),
        None => (source::label(rest), String::new()),
    }
}

/// Read `Campaign A: [0.3, 0.6]`, with the newer `radius:` and `:::class` forms allowed after it.
fn read_spot(line: &super::source::Line) -> Result<Spot, Problem> {
    let text = line.text.trim();
    let Some(open) = text.find('[') else {
        return Err(Problem::at(
            line,
            "a point looks like `Campaign A: [0.3, 0.6]` — a name, a colon, and two numbers from zero to one.",
        ));
    };
    let Some(close) = text[open..].find(']').map(|at| at + open) else {
        return Err(Problem::at(line, "this point's brackets were never closed"));
    };
    let head = text[..open].trim().trim_end_matches(':').trim();
    // `Point A:::class1: [0.9, 0.0]` — the class is read and ignored.
    let name = match head.find(":::") {
        Some(at) => head[..at].trim(),
        None => head.trim_end_matches(':').trim(),
    };
    let numbers: Vec<&str> = text[open + 1..close].split(',').collect();
    let [x, y] = numbers.as_slice() else {
        return Err(Problem::at(line, "a point needs exactly two numbers in its brackets"));
    };
    let (Ok(x), Ok(y)) = (x.trim().parse::<f32>(), y.trim().parse::<f32>()) else {
        return Err(Problem::at(line, "a point's two numbers have to be numbers"));
    };
    let radius = text[close..]
        .split_once("radius:")
        .and_then(|(_, rest)| rest.trim().split([',', ' ']).next()?.parse::<f32>().ok())
        .unwrap_or(DOT);
    Ok(Spot {
        label: source::label(name),
        x: x.clamp(0.0, 1.0),
        y: y.clamp(0.0, 1.0),
        radius: radius.clamp(2.0, 40.0),
    })
}

fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&chart.title);
    }
    let width = parts::MARGIN * 2.0 + GUTTER + PLOT;
    let top = parts::title(&mut scene, &titled, options, width);
    let plot = Rect::new(parts::MARGIN + GUTTER, top + parts::MARGIN, PLOT, PLOT);

    draw_quadrants(&mut scene, chart, plot, options);
    draw_axes(&mut scene, chart, plot, options);
    draw_spots(&mut scene, chart, plot, options);
    scene.claim(Rect::new(0.0, 0.0, width, plot.bottom() + GUTTER));
    parts::finish(&mut scene);
    scene
}

/// The four washes and their names.
///
/// Mermaid numbers them anticlockwise from the top right, so quadrant one is at `(right, top)`.
fn draw_quadrants(scene: &mut Scene, chart: &Chart, plot: Rect, options: &Options) {
    let half = PLOT / 2.0;
    let corners = [
        Rect::new(plot.centre().x, plot.top(), half, half),
        Rect::new(plot.left(), plot.top(), half, half),
        Rect::new(plot.left(), plot.centre().y, half, half),
        Rect::new(plot.centre().x, plot.centre().y, half, half),
    ];
    for (index, rect) in corners.iter().enumerate() {
        scene.add(Item::Rect {
            rect: *rect,
            radius: 0.0,
            fill: Some(options.theme.wash(index, 34)),
            stroke: None,
        });
        let words = &chart.quadrants[index];
        if words.trim().is_empty() {
            continue;
        }
        let label = text::measure(words, &options.style(0.95, true), options.metrics, half - 20.0);
        parts::label_at(
            scene,
            &label,
            Point::new(rect.centre().x, rect.centre().y - label.height / 2.0),
            &parts::text_style(options, 0.95, true, options.theme.text),
            Anchor::Middle,
        );
    }
    // The two dividing lines, drawn after the washes so they are not covered by them.
    let stroke = Stroke::new(options.theme.grid, parts::LINE);
    scene.add(Item::Line {
        points: vec![
            Point::new(plot.centre().x, plot.top()),
            Point::new(plot.centre().x, plot.bottom()),
        ],
        stroke,
        dash: Dash::Solid,
    });
    scene.add(Item::Line {
        points: vec![
            Point::new(plot.left(), plot.centre().y),
            Point::new(plot.right(), plot.centre().y),
        ],
        stroke,
        dash: Dash::Solid,
    });
    scene.add(Item::Rect {
        rect: plot,
        radius: 0.0,
        fill: None,
        stroke: Some(Stroke::new(options.theme.grid, parts::LINE)),
    });
}

/// The four axis titles, one at each end of each axis.
fn draw_axes(scene: &mut Scene, chart: &Chart, plot: Rect, options: &Options) {
    let style = parts::text_style(options, 0.85, false, options.theme.dim);
    let measure = options.style(0.85, false);
    let below = plot.bottom() + 8.0;
    for (words, at, anchor) in [
        (&chart.x_axis.0, Point::new(plot.left() + 4.0, below), Anchor::Start),
        (&chart.x_axis.1, Point::new(plot.right() - 4.0, below), Anchor::End),
    ] {
        if words.trim().is_empty() {
            continue;
        }
        let width = text::width_of(words, &measure, options.metrics);
        parts::one_line(scene, words, at, &style, anchor, width);
    }
    // The vertical axis's two titles are written along the left edge rather than turned on their
    // side, because the scene has no way to rotate text and turned words in a diagram are harder to
    // read than words that are simply beside the axis.
    for (words, y, anchor) in [
        (&chart.y_axis.1, plot.top() + 4.0, Anchor::Start),
        (&chart.y_axis.0, plot.bottom() - measure.size - 4.0, Anchor::Start),
    ] {
        if words.trim().is_empty() {
            continue;
        }
        let label = text::measure(words, &measure, options.metrics, GUTTER * 2.0);
        parts::label_at(scene, &label, Point::new(2.0, y), &style, anchor);
    }
}

/// Each point, and its name beside it.
fn draw_spots(scene: &mut Scene, chart: &Chart, plot: Rect, options: &Options) {
    let style = parts::text_style(options, 0.85, false, options.theme.text);
    let measure = options.style(0.85, false);
    for (index, spot) in chart.spots.iter().enumerate() {
        // One is at the **top**, so the vertical coordinate is flipped.
        let at = Point::new(
            plot.left() + plot.width * spot.x,
            plot.bottom() - plot.height * spot.y,
        );
        scene.add(Item::Circle {
            centre: at,
            radius: spot.radius,
            fill: Some(Paint::solid(options.theme.series(index))),
            stroke: Some(Stroke::new(options.theme.node_fill.color, parts::LINE)),
        });
        if spot.label.trim().is_empty() {
            continue;
        }
        let width = text::width_of(&spot.label, &measure, options.metrics);
        // Written to the left when the point is near the right edge, so a name never runs off.
        let room = plot.right() - at.x - spot.radius - 6.0;
        let (anchor, x) = if width > room {
            (Anchor::End, at.x - spot.radius - 6.0)
        } else {
            (Anchor::Start, at.x + spot.radius + 6.0)
        };
        parts::one_line(
            scene,
            &spot.label,
            Point::new(x, at.y - measure.size * 0.6),
            &style,
            anchor,
            width,
        );
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
    fn the_axes_and_the_four_quadrants_are_read() {
        let text = "quadrantChart\n title Reach and engagement\n\
            x-axis Low Reach --> High Reach\n y-axis Low Engagement --> High Engagement\n\
            quadrant-1 We should expand\n quadrant-2 Needs promotion\n \
            quadrant-3 Re-evaluate\n quadrant-4 May be improved\n";
        let chart = chart(text);
        assert_eq!(chart.title.as_deref(), Some("Reach and engagement"));
        assert_eq!(chart.x_axis, ("Low Reach".to_owned(), "High Reach".to_owned()));
        assert_eq!(chart.y_axis, ("Low Engagement".to_owned(), "High Engagement".to_owned()));
        assert_eq!(chart.quadrants[0], "We should expand");
        assert_eq!(chart.quadrants[3], "May be improved");
    }

    #[test]
    fn a_point_is_read_with_its_two_numbers() {
        let chart = chart("quadrantChart\n Campaign A: [0.3, 0.6]\n");
        assert_eq!(chart.spots[0].label, "Campaign A");
        assert_eq!((chart.spots[0].x, chart.spots[0].y), (0.3, 0.6));
    }

    #[test]
    fn the_newer_class_and_radius_forms_are_read() {
        let chart = chart("quadrantChart\n Point A:::class1: [0.9, 0.1] radius: 12\n");
        assert_eq!(chart.spots[0].label, "Point A", "the class is not part of the name");
        assert_eq!(chart.spots[0].radius, 12.0);
    }

    #[test]
    fn a_point_outside_the_square_is_brought_back_onto_it() {
        let chart = chart("quadrantChart\n Far: [1.8, -0.4]\n");
        assert_eq!((chart.spots[0].x, chart.spots[0].y), (1.0, 0.0));
    }

    #[test]
    fn quadrant_one_is_the_top_right_which_is_not_what_anybody_guesses() {
        let text = "quadrantChart\n quadrant-1 TopRight\n quadrant-2 TopLeft\n \
                    quadrant-3 BottomLeft\n quadrant-4 BottomRight\n";
        let scene = check::drawn(text, &options(), &["TopRight", "BottomLeft"]);
        let where_is = |words: &str| {
            scene
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Text { at, text, .. } if text == words => Some(*at),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{words} was not drawn"))
        };
        let (tr, tl, bl, br) = (
            where_is("TopRight"),
            where_is("TopLeft"),
            where_is("BottomLeft"),
            where_is("BottomRight"),
        );
        assert!(tr.x > tl.x && tr.y < br.y, "one is top right");
        assert!(bl.x < br.x && bl.y > tl.y, "three is bottom left");
    }

    #[test]
    fn a_higher_number_is_plotted_higher_up() {
        let scene = check::drawn("quadrantChart\n Low: [0.5, 0.1]\n High: [0.5, 0.9]\n", &options(), &["Low", "High"]);
        let dots: Vec<Point> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Circle { centre, .. } => Some(*centre),
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 2);
        assert!(dots[1].y < dots[0].y, "0.9 is above 0.1");
    }

    #[test]
    fn a_point_with_no_brackets_says_which_line() {
        let problem = check::refused("quadrantChart\n Campaign A: 0.3, 0.6\n", &options());
        assert_eq!(problem.line, Some(2));
    }

    #[test]
    fn a_quadrant_chart_is_drawn_and_keeps_every_property() {
        let text = "quadrantChart\n title Reach and engagement\n\
            x-axis Low Reach --> High Reach\n y-axis Low Engagement --> High Engagement\n\
            quadrant-1 We should expand\n quadrant-2 Needs promotion\n\
            quadrant-3 Re-evaluate\n quadrant-4 May be improved\n\
            Campaign A: [0.3, 0.6]\n Campaign B: [0.45, 0.23]\n Campaign C: [0.57, 0.69]\n";
        check::drawn(
            text,
            &options(),
            &["Reach and engagement", "High Reach", "We should expand", "Campaign A", "Campaign C"],
        );
    }
}
