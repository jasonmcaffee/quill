//! Timelines: `timeline`.
//!
//! A line across the page with a period hanging under it and that period's events stacked below,
//! coloured by the section they are in. The first period written is at the left and the last at the
//! right, which is Mermaid's own rule and is the only ordering a timeline can sensibly have.
//!
//! **A continuation line belongs to the period above it.** `2004 : Facebook : Google` and
//! ```text
//! 2004 : Facebook
//!      : Google
//! ```
//! are the same timeline, so a line beginning with a colon adds to whatever period was last.

use super::parts;
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How far apart two periods are, at the least.
const PERIOD_GAP: f32 = 26.0;
/// How wide a period's column is allowed to get before its words wrap.
const COLUMN: f32 = 150.0;
/// How much room the section band takes.
const SECTION_HEIGHT: f32 = 30.0;
/// The gap between one event card and the next.
const CARD_GAP: f32 = 8.0;

/// One period, with the events under it.
#[derive(Debug, Clone, PartialEq)]
struct Period {
    name: String,
    events: Vec<String>,
    /// The section it is in, if any.
    section: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    sections: Vec<String>,
    periods: Vec<Period>,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    let mut section: Option<usize> = None;
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            diagram.title = Some(source::label(rest));
            continue;
        }
        if let Some(rest) = line.after_word("section") {
            diagram.sections.push(source::label(rest));
            section = Some(diagram.sections.len() - 1);
            continue;
        }
        let text = line.text.trim();
        // A line that begins with a colon carries on the period above it.
        if let Some(rest) = text.strip_prefix(':') {
            let Some(period) = diagram.periods.last_mut() else {
                return Err(Problem::at(line, "this event has no period above it to belong to"));
            };
            period.events.extend(events_of(rest));
            continue;
        }
        let (name, events) = match text.split_once(':') {
            Some((name, rest)) => (name.trim(), events_of(rest)),
            None => (text, Vec::new()),
        };
        if name.is_empty() {
            continue;
        }
        diagram.periods.push(Period { name: source::label(name), events, section });
    }
    Ok(diagram)
}

/// The events on one line, which are separated by further colons.
fn events_of(rest: &str) -> Vec<String> {
    rest.split(':')
        .map(source::label)
        .filter(|words| !words.is_empty())
        .collect()
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&diagram.title);
    }
    if diagram.periods.is_empty() {
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let name_style = options.style(1.0, true);
    let event_style = options.style(0.85, false);
    let names: Vec<Label> = diagram
        .periods
        .iter()
        .map(|period| text::measure(&period.name, &name_style, options.metrics, COLUMN))
        .collect();
    let events: Vec<Vec<Label>> = diagram
        .periods
        .iter()
        .map(|period| {
            period
                .events
                .iter()
                .map(|words| text::measure(words, &event_style, options.metrics, COLUMN))
                .collect()
        })
        .collect();

    let widths: Vec<f32> = (0..diagram.periods.len())
        .map(|index| {
            names[index]
                .width
                .max(events[index].iter().map(|label| label.width).fold(0.0_f32, f32::max))
                + parts::PADDING_X * 2.0
        })
        .collect();
    let width: f32 =
        widths.iter().sum::<f32>() + PERIOD_GAP * (widths.len() - 1) as f32 + parts::MARGIN * 2.0;

    let top = parts::title(&mut scene, &titled, options, width);
    let has_sections = !diagram.sections.is_empty();
    let band = top + parts::MARGIN;
    let axis = band + if has_sections { SECTION_HEIGHT + 10.0 } else { 0.0 };

    let mut lefts = Vec::with_capacity(widths.len());
    let mut at = parts::MARGIN;
    for width in &widths {
        lefts.push(at);
        at += width + PERIOD_GAP;
    }
    if has_sections {
        draw_sections(&mut scene, diagram, &lefts, &widths, band, options);
    }
    draw_axis(&mut scene, axis, parts::MARGIN, width - parts::MARGIN, options);
    let bottom = draw_periods(&mut scene, diagram, &names, &events, &lefts, &widths, axis, options);
    scene.claim(Rect::new(0.0, 0.0, width, bottom));
    parts::finish(&mut scene);
    scene
}

/// Draw the coloured band over each section's run of periods.
fn draw_sections(
    scene: &mut Scene,
    diagram: &Diagram,
    lefts: &[f32],
    widths: &[f32],
    band: f32,
    options: &Options,
) {
    for (index, name) in diagram.sections.iter().enumerate() {
        let members: Vec<usize> = (0..diagram.periods.len())
            .filter(|&at| diagram.periods[at].section == Some(index))
            .collect();
        let (Some(&first), Some(&last)) = (members.first(), members.last()) else {
            continue;
        };
        let rect = Rect::new(
            lefts[first],
            band,
            lefts[last] + widths[last] - lefts[first],
            SECTION_HEIGHT,
        );
        scene.add(Item::Rect {
            rect,
            radius: parts::CORNER,
            fill: Some(options.theme.wash(index, 60)),
            stroke: None,
        });
        let width = text::width_of(name, &options.style(0.95, true), options.metrics);
        parts::one_line(
            scene,
            name,
            Point::new(rect.centre().x, rect.centre().y - options.base.size * 0.6),
            &parts::text_style(options, 0.95, true, options.theme.text),
            Anchor::Middle,
            width,
        );
    }
}

/// The line across the page that the periods hang from.
fn draw_axis(scene: &mut Scene, y: f32, from: f32, to: f32, options: &Options) {
    scene.add(Item::Line {
        points: vec![Point::new(from, y), Point::new(to, y)],
        stroke: Stroke::new(options.theme.grid, parts::THICK),
        dash: Dash::Solid,
    });
}

/// Draw each period's marker, its name, and the cards under it. Returns how far down it reached.
#[allow(clippy::too_many_arguments)]
fn draw_periods(
    scene: &mut Scene,
    diagram: &Diagram,
    names: &[Label],
    events: &[Vec<Label>],
    lefts: &[f32],
    widths: &[f32],
    axis: f32,
    options: &Options,
) -> f32 {
    let mut lowest = axis;
    for (index, period) in diagram.periods.iter().enumerate() {
        let colour = options.theme.series(period.section.unwrap_or(index));
        let centre = lefts[index] + widths[index] / 2.0;
        scene.add(Item::Circle {
            centre: Point::new(centre, axis),
            radius: 6.0,
            fill: Some(Paint::solid(colour)),
            stroke: Some(Stroke::new(options.theme.node_fill.color, parts::LINE)),
        });
        let mut y = axis + 16.0;
        parts::label_at(
            scene,
            &names[index],
            Point::new(centre, y),
            &parts::text_style(options, 1.0, true, options.theme.text),
            Anchor::Middle,
        );
        y += names[index].height + CARD_GAP;
        for (at, label) in events[index].iter().enumerate() {
            let rect = Rect::new(
                lefts[index],
                y,
                widths[index],
                label.height + parts::PADDING_Y * 2.0,
            );
            scene.add(Item::Rect {
                rect,
                radius: parts::CORNER,
                fill: Some(Paint::faded(colour, 46)),
                stroke: Some(Stroke::new(colour, parts::LINE)),
            });
            parts::centred_label(
                scene,
                label,
                rect,
                &parts::text_style(options, 0.85, false, options.theme.text),
            );
            let _ = at;
            y = rect.bottom() + CARD_GAP;
        }
        lowest = lowest.max(y);
    }
    lowest
}

#[cfg(test)]
mod tests {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    fn diagram(text: &str) -> Diagram {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn several_events_on_one_line_all_belong_to_that_period() {
        let diagram = diagram("timeline\n 2004 : Facebook : Google\n");
        assert_eq!(diagram.periods.len(), 1);
        assert_eq!(diagram.periods[0].name, "2004");
        assert_eq!(diagram.periods[0].events, vec!["Facebook", "Google"]);
    }

    #[test]
    fn a_continuation_line_adds_to_the_period_above_it() {
        let text = "timeline\n 2004 : Facebook\n      : Google\n      : Flickr\n";
        let diagram = diagram(text);
        assert_eq!(diagram.periods.len(), 1, "the colons carry on the same period");
        assert_eq!(diagram.periods[0].events, vec!["Facebook", "Google", "Flickr"]);
    }

    #[test]
    fn sections_group_the_periods_that_follow_them() {
        let text = "timeline\n title Social media\n section Early\n 2002 : LinkedIn\n \
                    2004 : Facebook\n section Later\n 2006 : Twitter\n";
        let diagram = diagram(text);
        assert_eq!(diagram.title.as_deref(), Some("Social media"));
        assert_eq!(diagram.sections, vec!["Early", "Later"]);
        assert_eq!(diagram.periods[0].section, Some(0));
        assert_eq!(diagram.periods[1].section, Some(0));
        assert_eq!(diagram.periods[2].section, Some(1));
    }

    #[test]
    fn an_event_with_no_period_above_it_says_which_line() {
        let problem = check::refused("timeline\n : an orphan\n", &options());
        assert_eq!(problem.line, Some(2));
    }

    #[test]
    fn a_timeline_is_drawn_and_keeps_every_property() {
        let text = "timeline\n title History\n section Early\n \
                    2002 : LinkedIn\n 2004 : Facebook : Google\n \
                    section Later\n 2005 : YouTube\n";
        let scene = check::drawn(
            text,
            &options(),
            &["History", "Early", "Later", "2002", "LinkedIn", "Facebook", "Google", "YouTube"],
        );
        assert!(scene.size.width > scene.size.height, "a timeline runs across the page");
    }

    #[test]
    fn the_first_period_written_is_the_leftmost() {
        let scene = check::drawn("timeline\n One : a\n Two : b\n Three : c\n", &options(), &["One"]);
        let circles: Vec<f32> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Circle { centre, .. } => Some(centre.x),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 3);
        assert!(circles[0] < circles[1] && circles[1] < circles[2]);
    }

    #[test]
    fn a_timeline_with_a_title_and_nothing_else_still_draws() {
        let scene = super::super::render("timeline\n title Nothing yet\n", &options()).expect("draws");
        assert!(scene.texts().iter().any(|words| words.contains("Nothing")));
    }
}
