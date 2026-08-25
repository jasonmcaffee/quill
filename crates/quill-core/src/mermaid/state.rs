//! State diagrams: `stateDiagram` and `stateDiagram-v2`.
//!
//! Rounded boxes joined by labelled arrows, through the shared layered layout, with four things that
//! are not ordinary states: the start marker, the end marker, a choice diamond and a fork or join
//! bar.
//!
//! ## `[*]` is not one state
//!
//! Mermaid writes both the start and the end of a machine as `[*]`, and which one it means depends
//! on which side of the arrow it is: `[*] --> Still` is a start and `Still --> [*]` is an end. So
//! each occurrence becomes a **new** marker node rather than being looked up by name. A diagram with
//! three ways to finish gets three end markers, which is what Mermaid draws and what reads correctly
//! — one shared end node would drag every finishing state into the same rank and pull the picture
//! out of shape.

use std::collections::HashMap;

use super::layered::{self, Direction, EdgeSpec, GroupSpec};
use super::parts::{self, Ending};
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Size, Stroke};
use super::shapes::Shape;
use super::source::{self, Line, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How big a start or end marker is.
const MARKER: f32 = 20.0;
/// How thick a fork or join bar is, and how long.
const BAR_THICKNESS: f32 = 8.0;
const BAR_LENGTH: f32 = 90.0;

/// What a state is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An ordinary state: a rounded box with its description in it.
    State,
    /// `[*]` at the start of an arrow.
    Start,
    /// `[*]` at the end of an arrow.
    End,
    /// `<<choice>>`
    Choice,
    /// `<<fork>>` and `<<join>>`, which are the same bar.
    Bar,
}

#[derive(Debug, Clone, PartialEq)]
struct State {
    label: String,
    kind: Kind,
    group: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct Transition {
    from: usize,
    to: usize,
    label: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Note {
    /// The state it belongs to.
    about: usize,
    text: String,
    /// True when it goes on the left rather than on the right.
    left: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    states: Vec<State>,
    by_id: HashMap<String, usize>,
    transitions: Vec<Transition>,
    composites: Vec<String>,
    notes: Vec<Note>,
    direction: Direction,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    let mut open: Vec<usize> = Vec::new();
    // A `note ... end note` block collects its lines until the terminator.
    let mut note: Option<(usize, bool, Vec<String>)> = None;
    for line in source.statements() {
        let text = line.text.trim();
        if let Some((about, left, words)) = note.as_mut() {
            if text.eq_ignore_ascii_case("end note") {
                diagram.notes.push(Note {
                    about: *about,
                    text: words.join("\n"),
                    left: *left,
                });
                note = None;
            } else {
                words.push(text.to_owned());
            }
            continue;
        }
        if text == "}" {
            open.pop();
            continue;
        }
        if text == "--" {
            // The divider between two concurrent regions of a composite state. Quill draws the
            // regions one after another rather than side by side, so it is read and not drawn.
            continue;
        }
        if let Some(rest) = line.after_word("direction") {
            if let Some(direction) = Direction::parse(rest) {
                diagram.direction = direction;
            }
            continue;
        }
        if is_decoration(line) {
            continue;
        }
        if let Some(rest) = line.after_word("note") {
            read_note(&mut diagram, rest, open.last().copied(), &mut note);
            continue;
        }
        if let Some(rest) = line.after_word("state") {
            read_state(&mut diagram, rest, &mut open)?;
            continue;
        }
        if read_transition(&mut diagram, line, open.last().copied())? {
            continue;
        }
        // `Still : it is not moving` — a description for a state declared elsewhere.
        if let Some((name, description)) = text.split_once(':') {
            if !name.trim().is_empty() {
                let state = state_of(&mut diagram, name.trim(), open.last().copied());
                diagram.states[state].label = source::label(description);
                continue;
            }
        }
        if !text.is_empty() {
            state_of(&mut diagram, text, open.last().copied());
        }
    }
    Ok(diagram)
}

fn is_decoration(line: &Line) -> bool {
    ["classDef", "class", "style", "click"].iter().any(|word| line.starts_with_word(word))
}

/// Read `note left of A : text`, `note right of A : text`, and the block form.
fn read_note(
    diagram: &mut Diagram,
    rest: &str,
    group: Option<usize>,
    open: &mut Option<(usize, bool, Vec<String>)>,
) {
    let lower = rest.to_ascii_lowercase();
    let (left, after) = if let Some(after) = lower.strip_prefix("left of") {
        (true, &rest[rest.len() - after.len()..])
    } else if let Some(after) = lower.strip_prefix("right of") {
        (false, &rest[rest.len() - after.len()..])
    } else {
        return;
    };
    let (name, words) = match after.split_once(':') {
        Some((name, words)) => (name.trim(), Some(words)),
        None => (after.trim(), None),
    };
    if name.is_empty() {
        return;
    }
    let about = state_of(diagram, name, group);
    match words {
        Some(words) => diagram.notes.push(Note { about, text: source::label(words), left }),
        None => *open = Some((about, left, Vec::new())),
    }
}

/// Read every form the `state` word introduces.
fn read_state(
    diagram: &mut Diagram,
    rest: &str,
    open: &mut Vec<usize>,
) -> Result<(), Problem> {
    let rest = rest.trim();
    // `state Name { ... }` — a composite state, which becomes a group.
    if let Some(name) = rest.strip_suffix('{') {
        let name = name.trim();
        let title = match split_alias(name) {
            Some((_, shown)) => source::label(shown),
            None => source::label(name),
        };
        diagram.composites.push(title);
        open.push(diagram.composites.len() - 1);
        return Ok(());
    }
    // `state "the description" as id`
    if let Some((description, id)) = split_alias(rest) {
        let index = state_of(diagram, id.trim(), open.last().copied());
        diagram.states[index].label = source::label(description);
        return Ok(());
    }
    // `state id <<choice>>`, `state id <<fork>>`, `state id <<join>>`
    if let (Some(mark), Some(close)) = (rest.find("<<"), rest.rfind(">>")) {
        let name = rest[..mark].trim();
        let word = rest[mark + 2..close].trim().to_ascii_lowercase();
        let index = state_of(diagram, name, open.last().copied());
        diagram.states[index].kind = match word.as_str() {
            "choice" => Kind::Choice,
            "fork" | "join" => Kind::Bar,
            _ => Kind::State,
        };
        // A choice or a bar shows no words; its shape is what it says.
        diagram.states[index].label = String::new();
        return Ok(());
    }
    // `state Name : a description`
    if let Some((name, description)) = rest.split_once(':') {
        let index = state_of(diagram, name.trim(), open.last().copied());
        diagram.states[index].label = source::label(description);
        return Ok(());
    }
    state_of(diagram, rest, open.last().copied());
    Ok(())
}

/// Split `"a description" as id` into its two halves.
fn split_alias(text: &str) -> Option<(&str, &str)> {
    let at = text.find(" as ")?;
    Some((text[..at].trim(), text[at + 4..].trim()))
}

/// Read `a --> b : label`. Returns false when there is no arrow on the line.
fn read_transition(
    diagram: &mut Diagram,
    line: &Line,
    group: Option<usize>,
) -> Result<bool, Problem> {
    let (head, label) = match line.text.split_once(':') {
        Some((head, label)) => (head, source::label(label)),
        None => (line.text.as_str(), String::new()),
    };
    let Some(at) = head.find("-->") else {
        return Ok(false);
    };
    let from = head[..at].trim();
    let to = head[at + 3..].trim();
    if from.is_empty() || to.is_empty() {
        return Err(Problem::at(line, "a transition needs a state at each end of it"));
    }
    let from = marker_or_state(diagram, from, group, Kind::Start);
    let to = marker_or_state(diagram, to, group, Kind::End);
    diagram.transitions.push(Transition { from, to, label });
    Ok(true)
}

/// `[*]` becomes a fresh marker; anything else is a state looked up by name.
///
/// Fresh, not shared: see this module's own comment. Three ways to finish should be three end
/// markers, not one node every finishing state is dragged towards.
fn marker_or_state(
    diagram: &mut Diagram,
    name: &str,
    group: Option<usize>,
    which: Kind,
) -> usize {
    if name.trim() == "[*]" {
        diagram.states.push(State { label: String::new(), kind: which, group });
        return diagram.states.len() - 1;
    }
    state_of(diagram, name, group)
}

fn state_of(diagram: &mut Diagram, name: &str, group: Option<usize>) -> usize {
    let id = source::unquote(name.trim()).trim().to_owned();
    if let Some(&known) = diagram.by_id.get(&id) {
        return known;
    }
    diagram.states.push(State { label: id.clone(), kind: Kind::State, group });
    diagram.by_id.insert(id, diagram.states.len() - 1);
    diagram.states.len() - 1
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let style = options.style(1.0, false);
    let labels: Vec<Label> = diagram
        .states
        .iter()
        .map(|state| text::measure(&state.label, &style, options.metrics, text::WRAP))
        .collect();
    let transition_style = options.style(0.85, false);
    let transitions: Vec<Label> = diagram
        .transitions
        .iter()
        .map(|t| text::measure(&t.label, &transition_style, options.metrics, text::EDGE_WRAP))
        .collect();

    let mut graph = layered::Graph { direction: diagram.direction, ..layered::Graph::default() };
    for title in &diagram.composites {
        let measured = text::measure_unwrapped(title, &options.style(0.95, true), options.metrics);
        graph.groups.push(GroupSpec {
            title: Size::new(measured.width, measured.height + 6.0),
            parent: None,
        });
    }
    for (index, state) in diagram.states.iter().enumerate() {
        graph.add_node(size_of(state.kind, labels[index].size()), state.group);
    }
    for (index, transition) in diagram.transitions.iter().enumerate() {
        graph.edges.push(EdgeSpec {
            from: transition.from,
            to: transition.to,
            label: transitions[index].size(),
            span: 1,
        });
    }
    let placed = layered::layout(&graph);

    let mut scene = Scene::new();
    let top = parts::title(&mut scene, source, options, placed.size.width);
    let origin = Point::new(parts::MARGIN, top + parts::MARGIN);
    draw_composites(&mut scene, diagram, &placed, origin, options);
    draw_transitions(&mut scene, diagram, &placed, origin, &transitions, options);
    for (index, state) in diagram.states.iter().enumerate() {
        draw_state(&mut scene, state, &labels[index], placed.nodes[index].moved(origin.x, origin.y), options);
    }
    draw_notes(&mut scene, diagram, &placed, origin, placed_width(&placed, origin), options);
    parts::finish(&mut scene);
    scene
}

/// How big a state of this kind is.
fn size_of(kind: Kind, label: Size) -> Size {
    match kind {
        Kind::Start | Kind::End => Size::new(MARKER, MARKER),
        Kind::Choice => Shape::Diamond.size_for(Size::new(24.0, 24.0)),
        Kind::Bar => Size::new(BAR_LENGTH, BAR_THICKNESS),
        Kind::State => Size::new(
            label.width + parts::PADDING_X * 2.0,
            label.height + parts::PADDING_Y * 2.0,
        ),
    }
}

/// Draw one state, by what kind it is.
fn draw_state(
    scene: &mut Scene,
    state: &State,
    label: &Label,
    rect: Rect,
    options: &Options,
) {
    let theme = &options.theme;
    let stroke = Stroke::new(theme.node_stroke, parts::LINE);
    match state.kind {
        Kind::Start => scene.add(Item::Circle {
            centre: rect.centre(),
            radius: MARKER / 2.0,
            fill: Some(Paint::solid(theme.text)),
            stroke: None,
        }),
        Kind::End => {
            scene.add(Item::Circle {
                centre: rect.centre(),
                radius: MARKER / 2.0,
                fill: None,
                stroke: Some(Stroke::new(theme.text, parts::LINE)),
            });
            scene.add(Item::Circle {
                centre: rect.centre(),
                radius: MARKER / 2.0 - 4.0,
                fill: Some(Paint::solid(theme.text)),
                stroke: None,
            });
        }
        Kind::Bar => scene.add(Item::Rect {
            rect,
            radius: 2.0,
            fill: Some(Paint::solid(theme.text)),
            stroke: None,
        }),
        Kind::Choice => {
            Shape::Diamond.draw(scene, rect, theme.node_fill, stroke);
        }
        Kind::State => {
            scene.add(Item::Rect {
                rect,
                radius: parts::CORNER * 2.0,
                fill: Some(theme.node_fill),
                stroke: Some(stroke),
            });
            parts::centred_label(
                scene,
                label,
                rect,
                &parts::text_style(options, 1.0, false, theme.text),
            );
        }
    }
}

/// Draw the frames round the composite states.
fn draw_composites(
    scene: &mut Scene,
    diagram: &Diagram,
    placed: &layered::Placed,
    origin: Point,
    options: &Options,
) {
    for (index, title) in diagram.composites.iter().enumerate() {
        let frame = placed.groups[index].moved(origin.x, origin.y);
        if frame.width <= 0.0 {
            continue;
        }
        scene.add(Item::Rect {
            rect: frame,
            radius: parts::CORNER * 2.0,
            fill: Some(options.theme.group_fill),
            stroke: Some(Stroke::new(options.theme.group_stroke, parts::LINE)),
        });
        let width = text::width_of(title, &options.style(0.95, true), options.metrics);
        parts::one_line(
            scene,
            title,
            Point::new(frame.left() + 12.0, frame.top() + 6.0),
            &parts::text_style(options, 0.95, true, options.theme.text),
            Anchor::Start,
            width,
        );
    }
}

/// Draw every transition: an arrow with its label beside the middle of it.
fn draw_transitions(
    scene: &mut Scene,
    diagram: &Diagram,
    placed: &layered::Placed,
    origin: Point,
    labels: &[Label],
    options: &Options,
) {
    let theme = &options.theme;
    for (index, transition) in diagram.transitions.iter().enumerate() {
        let mut path: Vec<Point> = placed.edges[index]
            .iter()
            .map(|point| Point::new(point.x + origin.x, point.y + origin.y))
            .collect();
        if path.len() < 2 {
            continue;
        }
        let last = path.len() - 1;
        path[0] = outline(diagram, placed, origin, transition.from).border_towards(path[1]);
        path[last] = outline(diagram, placed, origin, transition.to).border_towards(path[last - 1]);
        let stroke = Stroke::new(theme.line, parts::LINE);
        scene.add(Item::Line {
            points: parts::trimmed(&path, 0.0, parts::ending_inset(Ending::Arrow)),
            stroke,
            dash: Dash::Solid,
        });
        parts::ending(scene, Ending::Arrow, path[last], parts::heading(&path), theme.line, theme.node_fill);
        if labels[index].is_empty() {
            continue;
        }
        let at = placed.labels[index];
        let panel = Rect::around(
            Point::new(at.x + origin.x, at.y + origin.y),
            Size::new(labels[index].width + 8.0, labels[index].height + 2.0),
        );
        scene.add(Item::Rect {
            rect: panel,
            radius: 3.0,
            fill: Some(Paint::solid(theme.node_fill.color)),
            stroke: None,
        });
        parts::centred_label(
            scene,
            &labels[index],
            panel,
            &parts::text_style(options, 0.85, false, theme.dim),
        );
    }
}

fn outline(
    diagram: &Diagram,
    placed: &layered::Placed,
    origin: Point,
    index: usize,
) -> parts::Outline {
    let rect = placed.nodes[index].moved(origin.x, origin.y);
    match diagram.states[index].kind {
        Kind::Start | Kind::End => parts::Outline::Circle(rect.centre(), MARKER / 2.0),
        Kind::Choice => Shape::Diamond.outline(rect),
        _ => parts::Outline::Rect(rect),
    }
}

/// The right hand edge of everything that has been placed, which is where the notes go.
fn placed_width(placed: &layered::Placed, origin: Point) -> f32 {
    placed
        .nodes
        .iter()
        .map(|rect| rect.right() + origin.x)
        .fold(origin.x, f32::max)
}

/// Draw the notes, in a column down the right of the whole diagram.
///
/// Beside the diagram rather than beside the state, with a faint leader from one to the other. A
/// note put directly next to its state lands on top of whatever the layout put there — which is
/// exactly what the state diagram's first picture showed — and the layout has no way to know a note
/// is coming, because a note is not a node and must not be ranked as one.
fn draw_notes(
    scene: &mut Scene,
    diagram: &Diagram,
    placed: &layered::Placed,
    origin: Point,
    right: f32,
    options: &Options,
) {
    let style = options.style(0.85, false);
    for note in &diagram.notes {
        let rect = placed.nodes[note.about].moved(origin.x, origin.y);
        if rect.width <= 0.0 {
            continue;
        }
        let label = text::measure(&note.text, &style, options.metrics, 180.0);
        let width = label.width + parts::PADDING_X * 2.0;
        let height = label.height + 12.0;
        let panel = Rect::new(right + 34.0, rect.centre().y - height / 2.0, width, height);
        // The leader, so it is plain which state the note is about.
        scene.add(Item::Line {
            points: vec![
                Point::new(rect.right(), rect.centre().y),
                Point::new(panel.left(), panel.centre().y),
            ],
            stroke: Stroke::new(options.theme.group_stroke, parts::LINE),
            dash: parts::DASH,
        });
        scene.add(Item::Rect {
            rect: panel,
            radius: parts::CORNER,
            fill: Some(options.theme.group_fill),
            stroke: Some(Stroke::new(options.theme.group_stroke, parts::LINE)),
        });
        parts::centred_label(
            scene,
            &label,
            panel,
            &parts::text_style(options, 0.85, false, options.theme.dim),
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

    fn diagram(text: &str) -> Diagram {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn a_start_and_an_end_are_told_apart_by_which_side_of_the_arrow_they_are_on() {
        let diagram = diagram("stateDiagram-v2\n [*] --> Still\n Still --> [*]\n");
        let kinds: Vec<Kind> = diagram.states.iter().map(|state| state.kind).collect();
        assert!(kinds.contains(&Kind::Start), "the first is a start");
        assert!(kinds.contains(&Kind::End), "the second is an end");
        assert_eq!(kinds.iter().filter(|k| **k == Kind::State).count(), 1);
    }

    #[test]
    fn two_ways_of_finishing_are_two_end_markers_and_not_one_shared_node() {
        let diagram = diagram("stateDiagram-v2\n A --> [*]\n B --> [*]\n");
        assert_eq!(
            diagram.states.iter().filter(|s| s.kind == Kind::End).count(),
            2,
            "one shared end node would drag both states into the same rank"
        );
    }

    #[test]
    fn a_description_can_be_given_either_way_round() {
        let diagram = diagram(
            "stateDiagram-v2\n state \"It is not moving\" as Still\n Moving : It is moving\n",
        );
        assert_eq!(diagram.states[diagram.by_id["Still"]].label, "It is not moving");
        assert_eq!(diagram.states[diagram.by_id["Moving"]].label, "It is moving");
    }

    #[test]
    fn choice_fork_and_join_are_read_as_their_own_shapes() {
        let text = "stateDiagram-v2\n state pick <<choice>>\n state split <<fork>>\n state rejoin <<join>>\n";
        let diagram = diagram(text);
        assert_eq!(diagram.states[diagram.by_id["pick"]].kind, Kind::Choice);
        assert_eq!(diagram.states[diagram.by_id["split"]].kind, Kind::Bar);
        assert_eq!(diagram.states[diagram.by_id["rejoin"]].kind, Kind::Bar);
        assert!(diagram.states[diagram.by_id["pick"]].label.is_empty(), "its shape says what it is");
    }

    #[test]
    fn a_composite_state_groups_what_is_inside_it() {
        let text = "stateDiagram-v2\n\
            [*] --> First\n\
            state First {\n  [*] --> second\n  second --> [*]\n }\n";
        let diagram = diagram(text);
        assert_eq!(diagram.composites, vec!["First"]);
        assert_eq!(diagram.states[diagram.by_id["second"]].group, Some(0));
    }

    #[test]
    fn a_note_is_read_in_both_of_its_forms() {
        let text = "stateDiagram-v2\n\
            A --> B\n\
            note left of A : the first one\n\
            note right of B\n  spread over\n  two lines\n end note\n";
        let diagram = diagram(text);
        assert_eq!(diagram.notes.len(), 2);
        assert!(diagram.notes[0].left);
        assert_eq!(diagram.notes[0].text, "the first one");
        assert!(!diagram.notes[1].left);
        assert_eq!(diagram.notes[1].text, "spread over\ntwo lines");
    }

    #[test]
    fn a_state_diagram_is_drawn_and_keeps_every_property() {
        let text = "stateDiagram-v2\n\
            direction LR\n\
            [*] --> Idle\n\
            Idle --> Running : start\n\
            Running --> Idle : stop\n\
            Running --> Failed : it broke\n\
            Failed --> [*]\n\
            note right of Failed : somebody should look\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Idle", "Running", "Failed", "start", "stop", "it broke", "somebody should look"],
        );
        assert!(scene.size.width > scene.size.height, "left to right is wide");
    }

    #[test]
    fn the_concurrency_divider_is_read_without_becoming_a_state() {
        let text = "stateDiagram-v2\n state Both {\n  A --> B\n  --\n  C --> D\n }\n";
        let diagram = diagram(text);
        assert!(!diagram.by_id.contains_key("--"), "the divider is not a state");
        assert_eq!(diagram.states.len(), 4);
    }

    #[test]
    fn a_transition_with_nothing_on_one_end_says_which_line() {
        let problem = check::refused("stateDiagram-v2\n A --> B\n --> C\n", &options());
        assert_eq!(problem.line, Some(3));
    }
}
