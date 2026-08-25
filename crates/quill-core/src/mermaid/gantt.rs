//! Gantt charts: `gantt`.
//!
//! Sections down the left, a date axis across the top, and a bar for each task. `done`, `active` and
//! `crit` are shaded differently and a milestone is a diamond rather than a bar.
//!
//! ## Dates, without a calendar library
//!
//! A gantt chart is the one Mermaid diagram that needs real date arithmetic: `after`, `until` and
//! `5d` all have to resolve to a position on an axis. That is done here with two small functions
//! that convert a civil date to a day number and back, which is enough because nothing here needs
//! time zones, leap seconds or locales — only "how many days apart are these two dates".
//!
//! `dateFormat` is read and only `YYYY-MM-DD` is honoured, which is its default and the overwhelming
//! majority of what is written. A chart whose dates will not parse is **still drawn**: the tasks are
//! laid out one after another by their durations alone, with no axis. Losing the calendar is much
//! better than losing the chart.
//!
//! ## Resolving is a pass of its own
//!
//! `after` and `until` can name a task written later in the file, so nothing can be placed while the
//! file is being read. The tasks are collected first and resolved afterwards, repeatedly, until a
//! pass changes nothing — bounded by the number of tasks, so a chart whose tasks depend on each
//! other in a circle stops rather than looping.

use std::collections::HashMap;

use super::parts;
use super::scene::{Anchor, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How tall one task's row is.
const ROW: f32 = 28.0;
/// The gap between two rows.
const ROW_GAP: f32 = 6.0;
/// How wide the column of task names down the left is allowed to get.
const NAME_COLUMN: f32 = 190.0;
/// How wide the plotting area is.
const PLOT: f32 = 520.0;
/// How much room the axis takes above the first row.
const AXIS_HEIGHT: f32 = 26.0;

/// How a task is shaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shade {
    #[default]
    Ordinary,
    Done,
    Active,
    Critical,
}

/// What a task said about when it starts.
#[derive(Debug, Clone, PartialEq)]
enum When {
    On(f64),
    /// After every one of these tasks has finished.
    After(Vec<String>),
    /// Nothing was said, so it follows whatever came before it.
    Unsaid,
}

/// One task, as it was written.
#[derive(Debug, Clone, PartialEq)]
struct Task {
    name: String,
    id: String,
    shade: Shade,
    milestone: bool,
    start: When,
    /// An explicit finish date.
    finish: Option<f64>,
    /// How long it lasts, in days.
    length: Option<f64>,
    /// Until every one of these tasks starts.
    until: Vec<String>,
    section: Option<usize>,
    /// Filled in by the resolving pass.
    placed: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    sections: Vec<String>,
    tasks: Vec<Task>,
    title: Option<String>,
    /// True when every date in the chart parsed, so an axis of real dates can be drawn.
    dated: bool,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let mut chart = read(source)?;
    resolve(&mut chart);
    Ok(draw(&chart, source, options))
}

fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart { dated: true, ..Chart::default() };
    let mut section: Option<usize> = None;
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            chart.title = Some(source::label(rest));
            continue;
        }
        if let Some(rest) = line.after_word("section") {
            chart.sections.push(source::label(rest));
            section = Some(chart.sections.len() - 1);
            continue;
        }
        // Everything else about the axis is about wording rather than about position.
        if ["dateFormat", "axisFormat", "excludes", "includes", "todayMarker", "tickInterval",
            "weekday", "inclusiveEndDates", "topAxis", "displayMode"]
            .iter()
            .any(|word| line.starts_with_word(word))
        {
            continue;
        }
        let Some((name, rest)) = line.text.split_once(':') else {
            return Err(Problem::at(
                line,
                "a task looks like `Design : des1, 2024-01-01, 5d` — a name, a colon, then when it happens.",
            ));
        };
        chart.tasks.push(read_task(source::label(name), rest, section, &mut chart.dated));
    }
    Ok(chart)
}

/// Read everything after a task's colon.
///
/// The pieces are read by what they *are* rather than by where they are, because Mermaid allows them
/// in several orders and an author writing `crit, after des1, 5d` means the same as
/// `after des1, crit, 5d`.
fn read_task(name: String, rest: &str, section: Option<usize>, dated: &mut bool) -> Task {
    let mut task = Task {
        name,
        id: String::new(),
        shade: Shade::Ordinary,
        milestone: false,
        start: When::Unsaid,
        finish: None,
        length: None,
        until: Vec::new(),
        section,
        placed: None,
    };
    for piece in rest.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        match piece.to_ascii_lowercase().as_str() {
            "done" => {
                task.shade = Shade::Done;
                continue;
            }
            "active" => {
                task.shade = Shade::Active;
                continue;
            }
            "crit" => {
                task.shade = Shade::Critical;
                continue;
            }
            "milestone" => {
                task.milestone = true;
                continue;
            }
            _ => {}
        }
        if let Some(after) = strip_word(piece, "after") {
            task.start = When::After(after.split_whitespace().map(str::to_owned).collect());
            continue;
        }
        if let Some(until) = strip_word(piece, "until") {
            task.until = until.split_whitespace().map(str::to_owned).collect();
            continue;
        }
        if let Some(day) = parse_date(piece) {
            if matches!(task.start, When::Unsaid) {
                task.start = When::On(day);
            } else {
                task.finish = Some(day);
            }
            continue;
        }
        if let Some(days) = parse_length(piece) {
            task.length = Some(days);
            continue;
        }
        // Anything left over is the task's own name for other tasks to refer to. A piece that looks
        // like it was meant to be a date but is not takes the chart's axis away rather than being
        // silently treated as an identifier.
        if task.id.is_empty() && !looks_like_a_date(piece) {
            task.id = piece.to_owned();
        } else if looks_like_a_date(piece) {
            *dated = false;
        }
    }
    task
}

fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() || !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let rest = &text[word.len()..];
    (rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// True for something that was plainly meant to be a date, so a chart can say its axis is unusable.
fn looks_like_a_date(piece: &str) -> bool {
    piece.len() >= 8 && piece.chars().filter(|c| *c == '-').count() >= 2
}

/// `YYYY-MM-DD`, with anything after the day ignored, as a day number.
fn parse_date(text: &str) -> Option<f64> {
    let text = text.split(['T', ' ']).next()?;
    let mut parts = text.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64)
}

/// A duration — `5d`, `2w`, `12h`, `1.5d` — in days.
fn parse_length(text: &str) -> Option<f64> {
    let end = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    if end == 0 {
        return None;
    }
    let value: f64 = text[..end].parse().ok()?;
    let days = match &text[end..] {
        "ms" => value / 86_400_000.0,
        "s" => value / 86_400.0,
        "m" => value / 1_440.0,
        "h" => value / 24.0,
        "d" => value,
        "w" => value * 7.0,
        "M" => value * 30.0,
        "y" => value * 365.0,
        _ => return None,
    };
    Some(days)
}

/// Days since 1970-01-01, by Howard Hinnant's civil calendar algorithm.
///
/// Written out rather than taken from a library because this is the only date arithmetic in Quill,
/// and a calendar crate for two functions is a dependency for the whole editor.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date a day number names, which is the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = (shifted + if shifted < 10 { 3 } else { -9 }) as u32;
    (year + i64::from(month <= 2), month, day)
}

/// Give every task a start and a finish.
///
/// Repeated until nothing changes, because `after` may name a task written further down the file.
/// Bounded by the number of tasks, so a chart whose tasks wait on each other in a circle stops with
/// the ones it could place rather than running for ever.
fn resolve(chart: &mut Chart) {
    for _ in 0..=chart.tasks.len() {
        let mut changed = false;
        let known: HashMap<String, (f64, f64)> = chart
            .tasks
            .iter()
            .filter_map(|task| task.placed.map(|placed| (task.id.clone(), placed)))
            .filter(|(id, _)| !id.is_empty())
            .collect();
        for index in 0..chart.tasks.len() {
            if chart.tasks[index].placed.is_some() {
                continue;
            }
            let previous = index
                .checked_sub(1)
                .and_then(|before| chart.tasks[before].placed)
                .map(|(_, finish)| finish);
            if let Some(placed) = place(&chart.tasks[index], &known, previous) {
                chart.tasks[index].placed = Some(placed);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Anything still unplaced follows whatever came before it and lasts a day, which is better than
    // leaving it off the chart entirely.
    let mut at = chart.tasks.iter().filter_map(|task| task.placed).map(|(start, _)| start).fold(f64::INFINITY, f64::min);
    if !at.is_finite() {
        at = 0.0;
    }
    for index in 0..chart.tasks.len() {
        if chart.tasks[index].placed.is_some() {
            at = chart.tasks[index].placed.expect("just checked").1;
            continue;
        }
        let length = chart.tasks[index].length.unwrap_or(1.0);
        chart.tasks[index].placed = Some((at, at + length));
        at += length;
        chart.dated = false;
    }
}

/// Work out one task's start and finish, if everything it depends on is known.
fn place(
    task: &Task,
    known: &HashMap<String, (f64, f64)>,
    previous: Option<f64>,
) -> Option<(f64, f64)> {
    let start = match &task.start {
        When::On(day) => *day,
        When::After(names) => {
            let mut latest = f64::NEG_INFINITY;
            for name in names {
                latest = latest.max(known.get(name)?.1);
            }
            latest
        }
        When::Unsaid => previous?,
    };
    if task.milestone {
        return Some((start, start));
    }
    let finish = if let Some(finish) = task.finish {
        finish
    } else if !task.until.is_empty() {
        let mut earliest = f64::INFINITY;
        for name in &task.until {
            earliest = earliest.min(known.get(name)?.0);
        }
        earliest
    } else {
        start + task.length.unwrap_or(1.0)
    };
    Some((start, finish.max(start)))
}

fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&chart.title);
    }
    if chart.tasks.is_empty() {
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let style = options.style(0.9, false);
    let names: Vec<Label> = chart
        .tasks
        .iter()
        .map(|task| text::measure(&task.name, &style, options.metrics, NAME_COLUMN))
        .collect();
    let name_width = names.iter().map(|label| label.width).fold(0.0_f32, f32::max) + 16.0;
    let width = parts::MARGIN * 2.0 + name_width + PLOT;
    let top = parts::title(&mut scene, &titled, options, width);

    let (first, last) = span(chart);
    let plot_left = parts::MARGIN + name_width;
    let axis_top = top + parts::MARGIN;
    let rows_top = axis_top + AXIS_HEIGHT;
    let rows = rows_of(chart);

    draw_axis(&mut scene, chart, first, last, plot_left, axis_top, rows_top, rows.len(), options);
    draw_rows(&mut scene, chart, &rows, &names, first, last, plot_left, rows_top, options);
    let bottom = rows_top + (ROW + ROW_GAP) * rows.len() as f32;
    scene.claim(Rect::new(0.0, 0.0, width, bottom));
    parts::finish(&mut scene);
    scene
}

/// One row of the chart: a section's name, or a task.
///
/// A section takes a row of its own rather than being written beside its first task, because both
/// would be at the left of the same row and one would be drawn over the other.
enum Row {
    Section(usize),
    Task(usize),
}

/// The chart's rows, in order: each section's name, then the tasks in it.
///
/// Worked out once and used twice — for how tall the chart is and for where each thing goes — so the
/// two cannot come to different answers about how many rows there are.
fn rows_of(chart: &Chart) -> Vec<Row> {
    let mut rows = Vec::with_capacity(chart.tasks.len() + chart.sections.len());
    let mut named: Vec<Option<usize>> = Vec::new();
    for (index, task) in chart.tasks.iter().enumerate() {
        if !named.contains(&task.section) {
            named.push(task.section);
            if let Some(section) = task.section {
                if section < chart.sections.len() {
                    rows.push(Row::Section(section));
                }
            }
        }
        rows.push(Row::Task(index));
    }
    rows
}

/// The earliest start and the latest finish in the chart, widened when they are the same.
fn span(chart: &Chart) -> (f64, f64) {
    let mut first = f64::INFINITY;
    let mut last = f64::NEG_INFINITY;
    for (start, finish) in chart.tasks.iter().filter_map(|task| task.placed) {
        first = first.min(start);
        last = last.max(finish);
    }
    if !first.is_finite() || !last.is_finite() {
        return (0.0, 1.0);
    }
    if (last - first).abs() < 0.5 {
        return (first, first + 1.0);
    }
    (first, last)
}

/// Draw the axis across the top, and a faint rule down the chart at each tick.
#[allow(clippy::too_many_arguments)]
fn draw_axis(
    scene: &mut Scene,
    chart: &Chart,
    first: f64,
    last: f64,
    plot_left: f32,
    axis_top: f32,
    rows_top: f32,
    rows: usize,
    options: &Options,
) {
    let bottom = rows_top + (ROW + ROW_GAP) * rows as f32;
    let style = parts::text_style(options, 0.75, false, options.theme.dim);
    let measure = options.style(0.75, false);
    // Six ticks is enough to read a date off and few enough that the labels never collide.
    let ticks = 6;
    for tick in 0..=ticks {
        let share = tick as f32 / ticks as f32;
        let x = plot_left + PLOT * share;
        scene.add(Item::Line {
            points: vec![Point::new(x, rows_top - 4.0), Point::new(x, bottom)],
            stroke: Stroke::new(options.theme.grid, 1.0),
            dash: parts::DASH,
        });
        let day = first + (last - first) * share as f64;
        let words = if chart.dated { format_date(day) } else { format!("{:.0}", day - first) };
        let width = text::width_of(&words, &measure, options.metrics);
        parts::one_line(
            scene,
            &words,
            Point::new(x, axis_top + 4.0),
            &style,
            Anchor::Middle,
            width,
        );
    }
}

/// A day number as `YYYY-MM-DD`.
fn format_date(day: f64) -> String {
    let (year, month, date) = civil_from_days(day.round() as i64);
    format!("{year:04}-{month:02}-{date:02}")
}

/// Draw the section names, the task names and the bars.
#[allow(clippy::too_many_arguments)]
fn draw_rows(
    scene: &mut Scene,
    chart: &Chart,
    rows: &[Row],
    names: &[Label],
    first: f64,
    last: f64,
    plot_left: f32,
    rows_top: f32,
    options: &Options,
) {
    let across = (last - first).max(1.0);
    let at = |day: f64| plot_left + PLOT * ((day - first) / across) as f32;
    for (row, entry) in rows.iter().enumerate() {
        let y = rows_top + (ROW + ROW_GAP) * row as f32;
        let index = match entry {
            Row::Section(section) => {
                // The section's name, in the colour its tasks are drawn in, so a long chart can be
                // read down the left without counting rows.
                let style = options.style(0.95, true);
                let words = &chart.sections[*section];
                let width = text::width_of(words, &style, options.metrics);
                parts::one_line(
                    scene,
                    words,
                    Point::new(parts::MARGIN, y + (ROW - style.size) / 2.0),
                    &parts::text_style(options, 0.95, true, options.theme.series(*section)),
                    Anchor::Start,
                    width,
                );
                continue;
            }
            Row::Task(index) => *index,
        };
        let task = &chart.tasks[index];
        let colour = shade_colour(task, index, options);
        parts::label_at(
            scene,
            &names[index],
            Point::new(parts::MARGIN, y + (ROW - names[index].height) / 2.0),
            &parts::text_style(options, 0.9, false, options.theme.text),
            Anchor::Start,
        );
        let (start, finish) = task.placed.unwrap_or((first, first));
        if task.milestone {
            let centre = Point::new(at(start), y + ROW / 2.0);
            let arm = ROW * 0.36;
            scene.add(Item::Polygon {
                points: vec![
                    Point::new(centre.x, centre.y - arm),
                    Point::new(centre.x + arm, centre.y),
                    Point::new(centre.x, centre.y + arm),
                    Point::new(centre.x - arm, centre.y),
                ],
                fill: Some(Paint::solid(colour)),
                stroke: Some(Stroke::new(options.theme.node_stroke, parts::LINE)),
            });
            continue;
        }
        let left = at(start);
        let bar = Rect::new(left, y + 4.0, (at(finish) - left).max(3.0), ROW - 8.0);
        scene.add(Item::Rect {
            rect: bar,
            radius: 3.0,
            fill: Some(match task.shade {
                Shade::Done => Paint::faded(colour, 120),
                _ => Paint::solid(colour),
            }),
            stroke: Some(Stroke::new(
                if task.shade == Shade::Critical { options.theme.accent } else { options.theme.node_stroke },
                if task.shade == Shade::Critical { parts::THICK } else { parts::LINE },
            )),
        });
    }
}

/// The colour a task's bar is drawn in: by its section, unless a tag says otherwise.
fn shade_colour(task: &Task, index: usize, options: &Options) -> crate::style::Color {
    match task.shade {
        Shade::Critical => options.theme.accent,
        _ => options.theme.series(task.section.unwrap_or(index)),
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
        let mut chart = read(&Source::read(text).expect("a diagram")).expect("it should read");
        resolve(&mut chart);
        chart
    }

    fn placed(chart: &Chart, name: &str) -> (f64, f64) {
        chart
            .tasks
            .iter()
            .find(|task| task.name == name)
            .and_then(|task| task.placed)
            .unwrap_or_else(|| panic!("{name} was never placed"))
    }

    #[test]
    fn a_civil_date_and_a_day_number_are_inverses() {
        for (year, month, day) in [(1970, 1, 1), (2024, 2, 29), (1999, 12, 31), (2026, 8, 25)] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day), "{year}-{month}-{day}");
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn a_start_date_and_a_duration_give_a_finish() {
        let chart = chart("gantt\n dateFormat YYYY-MM-DD\n section One\n Design : des1, 2024-01-01, 5d\n");
        let (start, finish) = placed(&chart, "Design");
        assert_eq!(finish - start, 5.0);
        assert_eq!(format_date(start), "2024-01-01");
        assert_eq!(format_date(finish), "2024-01-06");
    }

    #[test]
    fn every_duration_unit_is_read() {
        assert_eq!(parse_length("5d"), Some(5.0));
        assert_eq!(parse_length("2w"), Some(14.0));
        assert_eq!(parse_length("12h"), Some(0.5));
        assert_eq!(parse_length("1.5d"), Some(1.5));
        assert_eq!(parse_length("3M"), Some(90.0));
        assert_eq!(parse_length("nonsense"), None);
    }

    #[test]
    fn a_start_and_an_end_date_are_told_apart_by_which_comes_first() {
        let chart = chart("gantt\n Build : b1, 2024-01-01, 2024-01-11\n");
        let (start, finish) = placed(&chart, "Build");
        assert_eq!(finish - start, 10.0);
    }

    #[test]
    fn after_waits_for_the_task_it_names_however_far_down_the_file_it_is() {
        // `after` may name a task declared later, which is why resolving is a pass of its own.
        let text = "gantt\n Second : s1, after f1, 3d\n First : f1, 2024-01-01, 4d\n";
        let chart = chart(text);
        let (first_start, first_finish) = placed(&chart, "First");
        let (second_start, _) = placed(&chart, "Second");
        assert_eq!(first_finish - first_start, 4.0);
        assert_eq!(second_start, first_finish, "the second starts when the first finishes");
    }

    #[test]
    fn until_stops_a_task_when_the_one_it_names_begins() {
        let text = "gantt\n Long : l1, 2024-01-01, until m1\n Later : m1, 2024-01-20, 1d\n";
        let chart = chart(text);
        let (start, finish) = placed(&chart, "Long");
        assert_eq!(finish - start, 19.0);
    }

    #[test]
    fn a_task_with_nothing_said_about_when_follows_the_one_before_it() {
        let chart = chart("gantt\n First : f1, 2024-01-01, 2d\n Second : 3d\n");
        assert_eq!(placed(&chart, "Second").0, placed(&chart, "First").1);
    }

    #[test]
    fn tasks_that_wait_on_each_other_in_a_circle_are_still_placed() {
        // A chart that cannot be resolved must not loop for ever; it is drawn with what can be
        // worked out and the rest laid end to end.
        let chart = chart("gantt\n A : a1, after b1, 2d\n B : b1, after a1, 2d\n");
        assert!(chart.tasks.iter().all(|task| task.placed.is_some()));
    }

    #[test]
    fn every_tag_is_read_and_a_milestone_has_no_length() {
        let text = "gantt\n Done one : d1, done, 2024-01-01, 2d\n \
                    Active one : a1, active, 2024-01-03, 2d\n \
                    Critical one : c1, crit, 2024-01-05, 2d\n \
                    A moment : m1, milestone, 2024-01-07, 0d\n";
        let chart = chart(text);
        let shades: Vec<Shade> = chart.tasks.iter().map(|task| task.shade).collect();
        assert_eq!(shades, vec![Shade::Done, Shade::Active, Shade::Critical, Shade::Ordinary]);
        assert!(chart.tasks[3].milestone);
        let (start, finish) = placed(&chart, "A moment");
        assert_eq!(start, finish, "a milestone is an instant");
    }

    #[test]
    fn the_tags_may_come_in_any_order() {
        let one = chart("gantt\n A : a1, crit, after b1, 5d\n B : b1, 2024-01-01, 1d\n");
        let two = chart("gantt\n A : a1, after b1, crit, 5d\n B : b1, 2024-01-01, 1d\n");
        assert_eq!(one.tasks[0].shade, Shade::Critical);
        assert_eq!(two.tasks[0].shade, Shade::Critical);
        assert_eq!(placed(&one, "A"), placed(&two, "A"));
    }

    #[test]
    fn a_line_that_is_not_a_task_says_which_line_it_was() {
        let problem = check::refused("gantt\n title Plan\n what is this\n", &options());
        assert_eq!(problem.line, Some(3));
    }

    #[test]
    fn a_gantt_chart_is_drawn_and_keeps_every_property() {
        let text = "gantt\n title A plan\n dateFormat YYYY-MM-DD\n excludes weekends\n\
            section Design\n Sketch it : des1, 2024-01-01, 5d\n Review it : des2, after des1, 3d\n\
            section Build\n Write it : done, 2024-01-09, 10d\n Test it : crit, active, after des2, 6d\n\
            Ship it : milestone, 2024-01-25, 0d\n";
        let scene = check::drawn(
            text,
            &options(),
            &["A plan", "Sketch it", "Review it", "Write it", "Test it", "Ship it", "2024-01-01"],
        );
        assert!(scene.size.width > 500.0);
    }

    #[test]
    fn a_chart_whose_dates_will_not_parse_is_still_drawn() {
        // Losing the calendar is much better than losing the chart.
        let text = "gantt\n dateFormat DD-MM-YYYY\n A : a1, 01-01-2024, 5d\n B : b1, after a1, 3d\n";
        let scene = check::drawn(text, &options(), &["A", "B"]);
        assert!(!scene.is_empty());
    }
}

#[cfg(test)]
mod rows {
    use super::super::{check, Options};
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    #[test]
    fn a_section_takes_a_row_of_its_own_above_the_tasks_in_it() {
        // Written beside its first task, a section's name and the task's name would both be at the
        // left of the same row, and one would be drawn over the other.
        let text = "gantt\n section Design\n A : 2024-01-01, 1d\n B : 1d\n section Build\n C : 1d\n";
        let scene = check::drawn(text, &options(), &["Design", "Build", "A", "B", "C"]);
        let texts = scene.texts();
        let at = |words: &str| texts.iter().position(|drawn| *drawn == words);
        assert!(at("Design") < at("A"), "the section comes before its first task");
        assert!(at("A") < at("Build"), "and the next section after the last of them");
        assert!(at("Build") < at("C"));
    }

    #[test]
    fn a_chart_with_no_sections_has_a_row_for_each_task_and_no_more() {
        let text = "gantt\n A : 2024-01-01, 1d\n B : 1d\n";
        let chart = {
            let mut chart = read(&Source::read(text).expect("a diagram")).expect("read");
            resolve(&mut chart);
            chart
        };
        assert_eq!(rows_of(&chart).len(), 2);
    }
}
