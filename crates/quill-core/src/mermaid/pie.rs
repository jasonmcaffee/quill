//! Pie charts: `pie`.
//!
//! Slices clockwise from twelve o'clock in the order they were written, a legend down the right, and
//! the share on each slice — the percentage, or the value itself when the header says `showData`.
//!
//! The arc is flattened into a polygon here rather than left as an arc for the painter, which is what
//! keeps [`super::scene::Item`] down to five kinds. One segment every four degrees is under half a
//! point of error at this radius, which is well below what anybody can see.

use super::parts;
use super::scene::{Anchor, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How big the pie itself is.
const RADIUS: f32 = 130.0;
/// How far out from the centre the share is written, as a fraction of the radius.
const LABEL_AT: f32 = 0.68;

/// One slice.
#[derive(Debug, Clone, PartialEq)]
struct Slice {
    label: String,
    value: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    slices: Vec<Slice>,
    /// Set by `showData` in the header: the value is written on the slice instead of the share.
    show_values: bool,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let chart = read(source)?;
    Ok(draw(&chart, source, options))
}

fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart::default();
    // `pie showData title Pets` — both of the header's words are optional and either order is seen.
    let mut header = source.header.trim();
    if let Some(rest) = strip_word(header, "showData") {
        chart.show_values = true;
        header = rest;
    }
    if let Some(rest) = strip_word(header, "title") {
        chart.title = Some(source::label(rest));
    }
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            chart.title = Some(source::label(rest));
            continue;
        }
        if line.starts_with_word("showData") {
            chart.show_values = true;
            continue;
        }
        let Some((label, value)) = line.text.rsplit_once(':') else {
            return Err(Problem::at(
                line,
                "a slice looks like `\"Dogs\" : 386` — a name, a colon, and a number.",
            ));
        };
        let Ok(value) = value.trim().parse::<f32>() else {
            return Err(Problem::at(
                line,
                format!("`{}` is not a number, and a slice needs one.", value.trim()),
            ));
        };
        // Mermaid refuses a value of zero or less, and so does this: a slice of nothing has no angle
        // to be drawn at and would leave a legend entry pointing at an invisible wedge.
        if value <= 0.0 {
            return Err(Problem::at(line, "a slice's value has to be more than zero"));
        }
        chart.slices.push(Slice { label: source::label(label), value });
    }
    Ok(chart)
}

/// What follows `word` when `text` begins with it as a whole word, ignoring case.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() || !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let rest = &text[word.len()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&chart.title);
    }
    let total: f32 = chart.slices.iter().map(|slice| slice.value).sum();
    if chart.slices.is_empty() || total <= 0.0 {
        parts::finish(&mut scene);
        return scene;
    }
    // The legend's width has to be known before the pie is placed, so the whole thing is centred
    // rather than the pie being centred and the legend hanging off the right.
    let legend_style = options.style(0.9, false);
    let widest = chart
        .slices
        .iter()
        .map(|slice| text::width_of(&slice.label, &legend_style, options.metrics))
        .fold(0.0_f32, f32::max);
    let legend_width = legend_style.size * 0.9 + 8.0 + widest;
    let width = parts::MARGIN + RADIUS * 2.0 + 32.0 + legend_width;

    let top = parts::title(&mut scene, &titled, options, width);
    let centre = Point::new(parts::MARGIN + RADIUS, top + parts::MARGIN + RADIUS);

    draw_slices(&mut scene, chart, total, centre, options);
    let entries: Vec<(String, super::super::style::Color)> = chart
        .slices
        .iter()
        .enumerate()
        .map(|(index, slice)| (slice.label.clone(), options.theme.series(index)))
        .collect();
    parts::legend(
        &mut scene,
        &entries,
        Point::new(centre.x + RADIUS + 32.0, centre.y - RADIUS + 10.0),
        options,
    );
    scene.claim(Rect::new(0.0, 0.0, width, centre.y + RADIUS));
    parts::finish(&mut scene);
    scene
}

/// Draw every wedge, and the share on it.
fn draw_slices(scene: &mut Scene, chart: &Chart, total: f32, centre: Point, options: &Options) {
    let mut angle = 0.0_f32;
    for (index, slice) in chart.slices.iter().enumerate() {
        let sweep = slice.value / total * std::f32::consts::TAU;
        let mut points = vec![centre];
        points.extend(parts::arc(centre, RADIUS, angle, angle + sweep, parts::arc_steps(sweep)));
        scene.add(Item::Polygon {
            points,
            fill: Some(Paint::solid(options.theme.series(index))),
            stroke: Some(Stroke::new(options.theme.node_fill.color, parts::LINE)),
        });
        draw_share(scene, chart, slice, total, centre, angle + sweep / 2.0, sweep, options);
        angle += sweep;
    }
}

/// The number written on one slice, when there is room for it.
#[allow(clippy::too_many_arguments)]
fn draw_share(
    scene: &mut Scene,
    chart: &Chart,
    slice: &Slice,
    total: f32,
    centre: Point,
    middle: f32,
    sweep: f32,
    options: &Options,
) {
    // A slice thinner than about seven degrees has nowhere to put a number that would not run over
    // its neighbours, so it is left to the legend.
    if sweep < 0.12 {
        return;
    }
    let words = if chart.show_values {
        format_number(slice.value)
    } else {
        format!("{:.0}%", slice.value / total * 100.0)
    };
    let at = Point::new(
        centre.x + RADIUS * LABEL_AT * middle.sin(),
        centre.y - RADIUS * LABEL_AT * middle.cos(),
    );
    let style = options.style(0.9, true);
    let width = text::width_of(&words, &style, options.metrics);
    parts::one_line(
        scene,
        &words,
        Point::new(at.x, at.y - style.size / 2.0),
        &parts::text_style(options, 0.9, true, options.theme.text),
        Anchor::Middle,
        width,
    );
}

/// A value as a person would write it: no decimal point when it does not need one.
pub fn format_number(value: f32) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
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
    fn slices_are_read_in_the_order_they_were_written() {
        let chart = chart("pie\n \"Dogs\" : 386\n \"Cats\" : 85\n \"Rats\" : 15\n");
        assert_eq!(chart.slices.len(), 3);
        assert_eq!(chart.slices[0].label, "Dogs");
        assert_eq!(chart.slices[0].value, 386.0);
        assert_eq!(chart.slices[2].label, "Rats");
    }

    #[test]
    fn the_header_carries_both_show_data_and_a_title() {
        let chart = chart("pie showData title Pets adopted\n \"Dogs\" : 1\n");
        assert!(chart.show_values);
        assert_eq!(chart.title.as_deref(), Some("Pets adopted"));
    }

    #[test]
    fn a_title_on_its_own_line_is_read_too() {
        let chart = chart("pie\n title Pets\n \"Dogs\" : 1\n");
        assert_eq!(chart.title.as_deref(), Some("Pets"));
        assert_eq!(chart.slices.len(), 1, "the title is not a slice");
    }

    #[test]
    fn a_value_that_is_not_a_number_says_so_with_its_line() {
        let problem = check::refused("pie\n \"Dogs\" : lots\n", &options());
        assert_eq!(problem.line, Some(2));
        assert!(problem.reason.contains("lots"));
    }

    #[test]
    fn a_value_of_zero_or_less_is_refused_the_way_mermaid_refuses_it() {
        assert!(check::refused("pie\n \"Nothing\" : 0\n", &options()).line.is_some());
        assert!(check::refused("pie\n \"Less\" : -4\n", &options()).line.is_some());
    }

    #[test]
    fn a_pie_chart_is_drawn_and_keeps_every_property() {
        let text = "pie title Pets adopted\n \"Dogs\" : 386\n \"Cats\" : 85\n \"Rats\" : 15\n";
        let scene = check::drawn(text, &options(), &["Pets adopted", "Dogs", "Cats", "Rats"]);
        // Three wedges, drawn as polygons.
        let wedges = scene
            .items
            .iter()
            .filter(|item| matches!(item, Item::Polygon { .. }))
            .count();
        assert_eq!(wedges, 3);
    }

    #[test]
    fn the_shares_add_up_to_a_hundred_per_cent() {
        let text = "pie\n \"A\" : 1\n \"B\" : 1\n \"C\" : 2\n";
        let scene = check::drawn(text, &options(), &["A", "B", "C"]);
        let shares: Vec<&str> =
            scene.texts().into_iter().filter(|words| words.ends_with('%')).collect();
        assert_eq!(shares, vec!["25%", "25%", "50%"]);
    }

    #[test]
    fn show_data_writes_the_value_rather_than_the_share() {
        let scene = check::drawn("pie showData\n \"A\" : 40\n \"B\" : 60\n", &options(), &["A"]);
        let texts = scene.texts();
        assert!(texts.contains(&"40"), "it says the value: {texts:?}");
        assert!(!texts.iter().any(|words| words.ends_with('%')));
    }

    #[test]
    fn a_number_is_written_the_way_a_person_would_write_it() {
        assert_eq!(format_number(386.0), "386");
        assert_eq!(format_number(0.5), "0.50");
    }

    #[test]
    fn a_pie_with_no_slices_draws_nothing_rather_than_dividing_by_zero() {
        let scene = super::super::render("pie title Empty\n", &options()).expect("it should draw");
        assert!(scene.size.width.is_finite());
    }
}
