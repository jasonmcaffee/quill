//! User journeys: `journey`.
//!
//! A band across the top for each section, a task under it, and the score drawn as a mark on a scale
//! of one to five with the actors listed underneath.
//!
//! **The score is what the diagram is for**, so it is drawn rather than written: a filled circle
//! climbing a five-step ladder, so a run of tasks reads as a line going up or down without anybody
//! having to compare five numbers. The number is written beside it as well, because a mark on a
//! ladder is a comparison and the number is the fact.

use super::parts;
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// The best score a task can have. Mermaid's scale is one to five.
const BEST: f32 = 5.0;
/// How tall the ladder a score is marked on is.
const LADDER: f32 = 84.0;
/// How wide one task's column is.
const COLUMN: f32 = 128.0;
/// The gap between two task columns.
const COLUMN_GAP: f32 = 12.0;
/// How tall the band naming a section is.
const BAND: f32 = 28.0;

/// One task.
#[derive(Debug, Clone, PartialEq)]
struct Task {
    name: String,
    score: f32,
    actors: Vec<String>,
    section: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    sections: Vec<String>,
    tasks: Vec<Task>,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    let mut section: Option<usize> = None;
    if !source.header.trim().is_empty() {
        diagram.title = Some(source::label(&source.header));
    }
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
        // `Make tea: 5: Me, You` — the name, the score, then whoever was involved.
        let parts: Vec<&str> = line.text.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(Problem::at(
                line,
                "a task looks like `Make tea: 5: Me, You` — a name, a score from one to five, and who was there.",
            ));
        }
        let Ok(score) = parts[1].trim().parse::<f32>() else {
            return Err(Problem::at(
                line,
                format!("`{}` is not a score. It should be a number from one to five.", parts[1].trim()),
            ));
        };
        let actors = parts
            .get(2)
            .map(|rest| {
                source::split_outside_quotes(rest, ',')
                    .into_iter()
                    .filter(|name| !name.trim().is_empty())
                    .map(|name| source::label(&name))
                    .collect()
            })
            .unwrap_or_default();
        diagram.tasks.push(Task {
            name: source::label(parts[0]),
            score: score.clamp(0.0, BEST),
            actors,
            section,
        });
    }
    Ok(diagram)
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&diagram.title);
    }
    if diagram.tasks.is_empty() {
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let name_style = options.style(0.9, false);
    let actor_style = options.style(0.8, false);
    let names: Vec<Label> = diagram
        .tasks
        .iter()
        .map(|task| text::measure(&task.name, &name_style, options.metrics, COLUMN - 12.0))
        .collect();
    let actors: Vec<Label> = diagram
        .tasks
        .iter()
        .map(|task| {
            text::measure(&task.actors.join(", "), &actor_style, options.metrics, COLUMN - 12.0)
        })
        .collect();

    let width = parts::MARGIN * 2.0
        + COLUMN * diagram.tasks.len() as f32
        + COLUMN_GAP * (diagram.tasks.len() - 1) as f32;
    let top = parts::title(&mut scene, &titled, options, width);
    let band = top + parts::MARGIN;
    let ladder_top = band + BAND + 12.0;

    let lefts: Vec<f32> = (0..diagram.tasks.len())
        .map(|index| parts::MARGIN + (COLUMN + COLUMN_GAP) * index as f32)
        .collect();
    draw_bands(&mut scene, diagram, &lefts, band, options);
    draw_ladder(&mut scene, ladder_top, parts::MARGIN, width - parts::MARGIN, options);
    let bottom = draw_tasks(&mut scene, diagram, &names, &actors, &lefts, ladder_top, options);
    scene.claim(Rect::new(0.0, 0.0, width, bottom));
    parts::finish(&mut scene);
    scene
}

/// The coloured band naming each section, over the tasks it holds.
fn draw_bands(
    scene: &mut Scene,
    diagram: &Diagram,
    lefts: &[f32],
    band: f32,
    options: &Options,
) {
    for (index, name) in diagram.sections.iter().enumerate() {
        let members: Vec<usize> = (0..diagram.tasks.len())
            .filter(|&at| diagram.tasks[at].section == Some(index))
            .collect();
        let (Some(&first), Some(&last)) = (members.first(), members.last()) else {
            continue;
        };
        let rect = Rect::new(lefts[first], band, lefts[last] + COLUMN - lefts[first], BAND);
        scene.add(Item::Rect {
            rect,
            radius: parts::CORNER,
            fill: Some(options.theme.wash(index, 70)),
            stroke: None,
        });
        let width = text::width_of(name, &options.style(0.9, true), options.metrics);
        parts::one_line(
            scene,
            name,
            Point::new(rect.centre().x, rect.top() + 5.0),
            &parts::text_style(options, 0.9, true, options.theme.text),
            Anchor::Middle,
            width,
        );
    }
}

/// The five faint rules a score is read against, with the best at the top.
fn draw_ladder(scene: &mut Scene, top: f32, from: f32, to: f32, options: &Options) {
    for step in 0..=BEST as usize {
        let y = top + LADDER * (1.0 - step as f32 / BEST);
        let strong = step == 0;
        scene.add(Item::Line {
            points: vec![Point::new(from, y), Point::new(to, y)],
            stroke: Stroke::new(
                if strong { options.theme.grid } else { options.theme.grid },
                if strong { parts::LINE } else { 1.0 },
            ),
            dash: if strong { Dash::Solid } else { parts::DASH },
        });
    }
}

/// Draw each task: its mark on the ladder, its number, its name and its actors.
#[allow(clippy::too_many_arguments)]
fn draw_tasks(
    scene: &mut Scene,
    diagram: &Diagram,
    names: &[Label],
    actors: &[Label],
    lefts: &[f32],
    ladder_top: f32,
    options: &Options,
) -> f32 {
    let mut lowest = ladder_top + LADDER;
    let mut previous: Option<Point> = None;
    for (index, task) in diagram.tasks.iter().enumerate() {
        let centre = lefts[index] + COLUMN / 2.0;
        let y = ladder_top + LADDER * (1.0 - task.score / BEST);
        let at = Point::new(centre, y);
        let colour = options.theme.series(task.section.unwrap_or(0));
        // The line joining one score to the next is what turns five marks into a journey.
        if let Some(before) = previous {
            scene.add(Item::Line {
                points: vec![before, at],
                stroke: Stroke::new(colour, parts::LINE),
                dash: Dash::Solid,
            });
        }
        previous = Some(at);
        scene.add(Item::Circle {
            centre: at,
            radius: 8.0,
            fill: Some(Paint::solid(colour)),
            stroke: Some(Stroke::new(options.theme.node_fill.color, parts::LINE)),
        });
        let score = super::pie::format_number(task.score);
        let width = text::width_of(&score, &options.style(0.8, true), options.metrics);
        parts::one_line(
            scene,
            &score,
            Point::new(centre + 12.0, y - options.base.size * 0.5),
            &parts::text_style(options, 0.8, true, options.theme.dim),
            Anchor::Start,
            width,
        );
        let mut below = ladder_top + LADDER + 12.0;
        parts::label_at(
            scene,
            &names[index],
            Point::new(centre, below),
            &parts::text_style(options, 0.9, false, options.theme.text),
            Anchor::Middle,
        );
        below += names[index].height + 4.0;
        if !task.actors.is_empty() {
            parts::label_at(
                scene,
                &actors[index],
                Point::new(centre, below),
                &parts::text_style(options, 0.8, false, options.theme.dim),
                Anchor::Middle,
            );
            below += actors[index].height;
        }
        lowest = lowest.max(below);
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
    fn a_task_is_read_as_a_name_a_score_and_its_actors() {
        let diagram = diagram("journey\n title My day\n section Morning\n Make tea: 5: Me, You\n");
        assert_eq!(diagram.title.as_deref(), Some("My day"));
        assert_eq!(diagram.sections, vec!["Morning"]);
        assert_eq!(diagram.tasks[0].name, "Make tea");
        assert_eq!(diagram.tasks[0].score, 5.0);
        assert_eq!(diagram.tasks[0].actors, vec!["Me", "You"]);
    }

    #[test]
    fn a_task_with_no_actors_is_still_a_task() {
        let diagram = diagram("journey\n Sit down: 3\n");
        assert_eq!(diagram.tasks[0].score, 3.0);
        assert!(diagram.tasks[0].actors.is_empty());
    }

    #[test]
    fn the_title_may_be_on_the_first_line_after_the_keyword() {
        let diagram = diagram("journey My day\n Make tea: 5: Me\n");
        assert_eq!(diagram.title.as_deref(), Some("My day"));
    }

    #[test]
    fn a_score_that_is_not_a_number_says_so_with_its_line() {
        let problem = check::refused("journey\n Make tea: good: Me\n", &options());
        assert_eq!(problem.line, Some(2));
        assert!(problem.reason.contains("good"));
    }

    #[test]
    fn a_journey_is_drawn_and_keeps_every_property() {
        let text = "journey\n title Going to work\n\
            section Home\n Wake up: 3: Me\n Make tea: 5: Me, Cat\n\
            section Office\n Sit down: 1: Me\n Do the work: 4: Me, Team\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Going to work", "Home", "Office", "Wake up", "Make tea", "Cat", "Team"],
        );
        assert!(scene.size.width > 400.0);
    }

    #[test]
    fn a_better_score_is_drawn_higher_up_than_a_worse_one() {
        // The whole point of the diagram is that a run of scores reads as a line, so which way up it
        // goes is worth a test of its own.
        let scene = check::drawn("journey\n Bad: 1: Me\n Good: 5: Me\n", &options(), &["Bad", "Good"]);
        let marks: Vec<Point> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Circle { centre, .. } => Some(*centre),
                _ => None,
            })
            .collect();
        assert_eq!(marks.len(), 2);
        assert!(marks[1].y < marks[0].y, "five is drawn above one");
    }
}
