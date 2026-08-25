//! XY charts: `xychart` and `xychart-beta`.
//!
//! Bars and lines over a shared pair of axes, vertical by default and horizontal when the header
//! says so.
//!
//! **The two series kinds share one scale**, which is the point of having them on one chart. So the
//! range is worked out across every series before anything is drawn, and a `y-axis` that names its
//! own range wins over the one the data would have chosen. A range given the wrong way round is
//! turned round rather than refused, because a chart drawn upside down is worse than one whose
//! author typed the numbers in the order that came to mind.

use super::parts;
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How big the plotting area is.
const PLOT_ACROSS: f32 = 480.0;
const PLOT_DOWN: f32 = 300.0;
/// How much room the labels down the left take.
const GUTTER: f32 = 56.0;
/// How much room the labels along the bottom take.
const FOOTER: f32 = 30.0;
/// How many rules are drawn across the plot.
const RULES: usize = 5;

/// One run of numbers.
#[derive(Debug, Clone, PartialEq)]
struct Series {
    name: String,
    values: Vec<f32>,
    bars: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    title: Option<String>,
    /// The names along the horizontal axis, when it is a list of them.
    categories: Vec<String>,
    x_title: String,
    y_title: String,
    /// A range the author named for the vertical axis.
    y_range: Option<(f32, f32)>,
    series: Vec<Series>,
    /// True when the bars run across the page rather than up it.
    horizontal: bool,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let chart = read(source)?;
    Ok(draw(&chart, source, options))
}

fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart {
        horizontal: source.header.to_ascii_lowercase().contains("horizontal"),
        ..Chart::default()
    };
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            chart.title = Some(source::label(rest));
            continue;
        }
        if let Some(rest) = line.after_word("x-axis") {
            read_x_axis(&mut chart, rest);
            continue;
        }
        if let Some(rest) = line.after_word("y-axis") {
            read_y_axis(&mut chart, rest);
            continue;
        }
        for (word, bars) in [("bar", true), ("line", false)] {
            if let Some(rest) = line.after_word(word) {
                chart.series.push(read_series(rest, bars, line)?);
                break;
            }
        }
    }
    Ok(chart)
}

/// `x-axis "Month" [jan, feb, mar]`, or `x-axis "Score" 0 --> 100`.
fn read_x_axis(chart: &mut Chart, rest: &str) {
    let rest = rest.trim();
    if let (Some(open), Some(close)) = (rest.find('['), rest.rfind(']')) {
        chart.x_title = source::label(&rest[..open]);
        chart.categories = source::split_outside_quotes(&rest[open + 1..close], ',')
            .into_iter()
            .map(|name| source::label(&name))
            .filter(|name| !name.is_empty())
            .collect();
        return;
    }
    // A numeric range across the bottom: the title is whatever comes before the numbers.
    match rest.split_once("-->") {
        Some((left, _)) => {
            let words: Vec<&str> = left.split_whitespace().collect();
            chart.x_title = source::label(&words[..words.len().saturating_sub(1)].join(" "));
        }
        None => chart.x_title = source::label(rest),
    }
}

/// `y-axis "Revenue" 0 --> 100`, or just `y-axis "Revenue"`.
fn read_y_axis(chart: &mut Chart, rest: &str) {
    let rest = rest.trim();
    let Some((left, right)) = rest.split_once("-->") else {
        chart.y_title = source::label(rest);
        return;
    };
    let words: Vec<&str> = left.split_whitespace().collect();
    let low = words.last().and_then(|word| word.parse::<f32>().ok());
    let high = right.split_whitespace().next().and_then(|word| word.parse::<f32>().ok());
    chart.y_title = source::label(&words[..words.len().saturating_sub(1)].join(" "));
    if let (Some(low), Some(high)) = (low, high) {
        // Turned round rather than refused: a chart drawn upside down is worse than a typo.
        chart.y_range = Some(if low <= high { (low, high) } else { (high, low) });
    }
}

/// `bar "Product A" [30, 45, 60]`.
fn read_series(rest: &str, bars: bool, line: &super::source::Line) -> Result<Series, Problem> {
    let rest = rest.trim();
    let Some(open) = rest.find('[') else {
        return Err(Problem::at(
            line,
            "a series looks like `bar [2, 4, 6]` — the numbers go in square brackets.",
        ));
    };
    let Some(close) = rest.rfind(']') else {
        return Err(Problem::at(line, "this series' brackets were never closed"));
    };
    let name = source::label(&rest[..open]);
    let mut values = Vec::new();
    for piece in rest[open + 1..close].split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        let Ok(value) = piece.parse::<f32>() else {
            return Err(Problem::at(line, format!("`{piece}` is not a number")));
        };
        values.push(value);
    }
    Ok(Series { name, values, bars })
}

fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&chart.title);
    }
    let width = parts::MARGIN * 2.0 + GUTTER + PLOT_ACROSS;
    let top = parts::title(&mut scene, &titled, options, width);
    let plot = Rect::new(parts::MARGIN + GUTTER, top + parts::MARGIN, PLOT_ACROSS, PLOT_DOWN);
    if chart.series.is_empty() {
        scene.claim(Rect::new(0.0, 0.0, width, plot.bottom() + FOOTER));
        parts::finish(&mut scene);
        return scene;
    }
    let (low, high) = range(chart);
    let steps = chart.series.iter().map(|series| series.values.len()).max().unwrap_or(1).max(1);

    draw_rules(&mut scene, plot, low, high, chart, options);
    draw_categories(&mut scene, chart, plot, steps, options);
    draw_series(&mut scene, chart, plot, low, high, steps, options);
    draw_axis_titles(&mut scene, chart, plot, options);
    scene.claim(Rect::new(0.0, 0.0, width, plot.bottom() + FOOTER));
    parts::finish(&mut scene);
    scene
}

/// The range every series is drawn against.
///
/// The author's own range wins. Otherwise it runs from zero — or from the lowest value when
/// something is negative — to a little above the highest, so the tallest bar is not flush with the
/// top of the chart.
fn range(chart: &Chart) -> (f32, f32) {
    if let Some(range) = chart.y_range {
        return if (range.1 - range.0).abs() < f32::EPSILON {
            (range.0, range.0 + 1.0)
        } else {
            range
        };
    }
    let values: Vec<f32> = chart.series.iter().flat_map(|series| series.values.iter().copied()).collect();
    if values.is_empty() {
        return (0.0, 1.0);
    }
    let highest = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let lowest = values.iter().copied().fold(f32::INFINITY, f32::min).min(0.0);
    if (highest - lowest).abs() < f32::EPSILON {
        return (lowest, lowest + 1.0);
    }
    (lowest, highest + (highest - lowest) * 0.08)
}

/// The faint rules across the plot, with their values down the left.
fn draw_rules(
    scene: &mut Scene,
    plot: Rect,
    low: f32,
    high: f32,
    chart: &Chart,
    options: &Options,
) {
    let style = parts::text_style(options, 0.75, false, options.theme.dim);
    let measure = options.style(0.75, false);
    for step in 0..=RULES {
        let share = step as f32 / RULES as f32;
        let value = low + (high - low) * share;
        let (from, to, at) = if chart.horizontal {
            let x = plot.left() + plot.width * share;
            (
                Point::new(x, plot.top()),
                Point::new(x, plot.bottom()),
                Point::new(x, plot.bottom() + 6.0),
            )
        } else {
            let y = plot.bottom() - plot.height * share;
            (
                Point::new(plot.left(), y),
                Point::new(plot.right(), y),
                Point::new(plot.left() - 8.0, y - measure.size * 0.6),
            )
        };
        scene.add(Item::Line {
            points: vec![from, to],
            stroke: Stroke::new(options.theme.grid, 1.0),
            dash: if step == 0 { Dash::Solid } else { parts::DASH },
        });
        let words = super::pie::format_number(value);
        let width = text::width_of(&words, &measure, options.metrics);
        let anchor = if chart.horizontal { Anchor::Middle } else { Anchor::End };
        parts::one_line(scene, &words, at, &style, anchor, width);
    }
}

/// The names along the axis the categories are on.
fn draw_categories(
    scene: &mut Scene,
    chart: &Chart,
    plot: Rect,
    steps: usize,
    options: &Options,
) {
    let style = parts::text_style(options, 0.8, false, options.theme.dim);
    let measure = options.style(0.8, false);
    for (index, name) in chart.categories.iter().enumerate().take(steps) {
        let share = (index as f32 + 0.5) / steps as f32;
        let width = text::width_of(name, &measure, options.metrics);
        let (at, anchor) = if chart.horizontal {
            (
                Point::new(plot.left() - 8.0, plot.top() + plot.height * share - measure.size * 0.6),
                Anchor::End,
            )
        } else {
            (Point::new(plot.left() + plot.width * share, plot.bottom() + 6.0), Anchor::Middle)
        };
        parts::one_line(scene, name, at, &style, anchor, width);
    }
}

/// Every series, bars behind lines so a line over a bar is still visible.
#[allow(clippy::too_many_arguments)]
fn draw_series(
    scene: &mut Scene,
    chart: &Chart,
    plot: Rect,
    low: f32,
    high: f32,
    steps: usize,
    options: &Options,
) {
    let span = (high - low).max(f32::EPSILON);
    let along = |index: usize| (index as f32 + 0.5) / steps as f32;
    let across = |value: f32| ((value - low) / span).clamp(0.0, 1.0);
    let bars = chart.series.iter().filter(|series| series.bars).count().max(1);
    let mut bar_number = 0;
    for (index, series) in chart.series.iter().enumerate().filter(|(_, s)| s.bars) {
        let width = plot.width / steps as f32 * 0.7 / bars as f32;
        for (at, value) in series.values.iter().enumerate() {
            let centre = along(at);
            let offset = (bar_number as f32 - (bars as f32 - 1.0) / 2.0) * width;
            let rect = if chart.horizontal {
                let y = plot.top() + plot.height * centre + offset - width / 2.0;
                Rect::new(plot.left(), y, plot.width * across(*value), width)
            } else {
                let x = plot.left() + plot.width * centre + offset - width / 2.0;
                let height = plot.height * across(*value);
                Rect::new(x, plot.bottom() - height, width, height)
            };
            scene.add(Item::Rect {
                rect,
                radius: 2.0,
                fill: Some(Paint::solid(options.theme.series(index))),
                stroke: None,
            });
        }
        bar_number += 1;
    }
    for (index, series) in chart.series.iter().enumerate().filter(|(_, s)| !s.bars) {
        let points: Vec<Point> = series
            .values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                if chart.horizontal {
                    Point::new(
                        plot.left() + plot.width * across(*value),
                        plot.top() + plot.height * along(at),
                    )
                } else {
                    Point::new(
                        plot.left() + plot.width * along(at),
                        plot.bottom() - plot.height * across(*value),
                    )
                }
            })
            .collect();
        if points.len() < 2 {
            continue;
        }
        scene.add(Item::Line {
            points: points.clone(),
            stroke: Stroke::new(options.theme.series(index), parts::THICK * 0.8),
            dash: Dash::Solid,
        });
        for point in points {
            scene.add(Item::Circle {
                centre: point,
                radius: 4.0,
                fill: Some(Paint::solid(options.theme.series(index))),
                stroke: None,
            });
        }
    }
    // A legend, but only when the series were named: an unnamed one has nothing to say.
    let entries: Vec<(String, crate::style::Color)> = chart
        .series
        .iter()
        .enumerate()
        .filter(|(_, series)| !series.name.trim().is_empty())
        .map(|(index, series)| (series.name.clone(), options.theme.series(index)))
        .collect();
    if !entries.is_empty() {
        parts::legend(scene, &entries, Point::new(plot.left() + 8.0, plot.top() + 6.0), options);
    }
}

/// The two axis titles, under the plot and above it.
fn draw_axis_titles(scene: &mut Scene, chart: &Chart, plot: Rect, options: &Options) {
    let style = parts::text_style(options, 0.85, true, options.theme.dim);
    let measure = options.style(0.85, true);
    if !chart.x_title.trim().is_empty() {
        let width = text::width_of(&chart.x_title, &measure, options.metrics);
        parts::one_line(
            scene,
            &chart.x_title,
            Point::new(plot.centre().x, plot.bottom() + FOOTER - measure.size - 2.0),
            &style,
            Anchor::Middle,
            width,
        );
    }
    if !chart.y_title.trim().is_empty() {
        let width = text::width_of(&chart.y_title, &measure, options.metrics);
        parts::one_line(
            scene,
            &chart.y_title,
            Point::new(plot.left(), plot.top() - measure.size - 4.0),
            &style,
            Anchor::Start,
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
    fn categories_and_two_kinds_of_series_are_read() {
        let text = "xychart-beta\n title Sales\n x-axis [jan, feb, mar]\n \
                    y-axis \"Revenue\" 0 --> 100\n bar [30, 45, 60]\n line [20, 35, 50]\n";
        let chart = chart(text);
        assert_eq!(chart.title.as_deref(), Some("Sales"));
        assert_eq!(chart.categories, vec!["jan", "feb", "mar"]);
        assert_eq!(chart.y_title, "Revenue");
        assert_eq!(chart.y_range, Some((0.0, 100.0)));
        assert_eq!(chart.series.len(), 2);
        assert!(chart.series[0].bars);
        assert!(!chart.series[1].bars);
        assert_eq!(chart.series[0].values, vec![30.0, 45.0, 60.0]);
    }

    #[test]
    fn a_named_series_keeps_its_name_for_the_legend() {
        let chart = chart("xychart-beta\n bar \"Product A\" [1, 2]\n line \"Product B\" [3, 4]\n");
        assert_eq!(chart.series[0].name, "Product A");
        assert_eq!(chart.series[1].name, "Product B");
    }

    #[test]
    fn a_range_written_the_wrong_way_round_is_turned_round() {
        // A chart drawn upside down is worse than an author typing the numbers in the order that
        // came to mind.
        let chart = chart("xychart-beta\n y-axis \"Score\" 100 --> 0\n bar [1]\n");
        assert_eq!(chart.y_range, Some((0.0, 100.0)));
    }

    #[test]
    fn with_no_range_given_the_data_chooses_one_that_starts_at_zero() {
        let chart = chart("xychart-beta\n bar [10, 20, 30]\n");
        let (low, high) = range(&chart);
        assert_eq!(low, 0.0);
        assert!(high > 30.0, "there is room above the tallest bar");
    }

    #[test]
    fn a_negative_value_pulls_the_range_below_zero() {
        let chart = chart("xychart-beta\n bar [-5, 10]\n");
        let (low, _) = range(&chart);
        assert_eq!(low, -5.0);
    }

    #[test]
    fn a_series_that_is_not_numbers_says_which_line() {
        let problem = check::refused("xychart-beta\n bar [1, two, 3]\n", &options());
        assert_eq!(problem.line, Some(2));
        assert!(problem.reason.contains("two"));
    }

    #[test]
    fn a_series_with_no_brackets_says_what_one_looks_like() {
        let problem = check::refused("xychart-beta\n bar 1, 2, 3\n", &options());
        assert!(problem.reason.contains("square brackets"));
    }

    #[test]
    fn an_xy_chart_is_drawn_and_keeps_every_property() {
        let text = "xychart-beta\n title Sales performance\n\
            x-axis \"Month\" [jan, feb, mar, apr]\n y-axis \"Revenue\" 0 --> 100\n\
            bar \"Product A\" [30, 45, 60, 75]\n line \"Product B\" [20, 35, 50, 65]\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Sales performance", "Month", "Revenue", "jan", "apr", "Product A", "Product B"],
        );
        assert!(scene.size.width > 400.0);
    }

    #[test]
    fn a_taller_bar_is_drawn_taller() {
        let scene = check::drawn("xychart-beta\n bar [10, 90]\n", &options(), &[]);
        let bars: Vec<Rect> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 2);
        assert!(bars[1].height > bars[0].height * 3.0);
        assert!(bars[1].bottom() >= bars[0].bottom() - 0.5, "both stand on the same baseline");
    }

    #[test]
    fn a_horizontal_chart_draws_its_bars_across_rather_than_up() {
        let scene = check::drawn("xychart-beta horizontal\n bar [10, 90]\n", &options(), &[]);
        let bars: Vec<Rect> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 2);
        assert!(bars[1].width > bars[0].width * 3.0, "the longer bar reaches further right");
        assert!((bars[0].left() - bars[1].left()).abs() < 0.5, "both start at the same edge");
    }
}
