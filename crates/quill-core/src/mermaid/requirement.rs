//! Requirement diagrams: `requirementDiagram`.
//!
//! A requirement or an element is a box in two compartments: what it is and what it is called, then
//! its fields. They are joined by seven kinds of dashed, labelled arrow, and placed by the shared
//! layered layout.
//!
//! ## The relationship can be written either way round
//!
//! `A - satisfies -> B` and `B <- satisfies - A` mean the same thing, and both have to end up as one
//! edge pointing the same way. So the parser reads the direction off the arrows rather than off the
//! order the names were written in, and swaps the two ends when it sees the backwards form.

use std::collections::HashMap;

use super::layered::{self, Direction, EdgeSpec};
use super::parts::{self, Ending, Outline};
use super::scene::{Anchor, Item, Paint, Point, Rect, Scene, Size, Stroke};
use super::source::{self, Line, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// The six kinds of requirement, and `element`, as Mermaid spells them.
const KINDS: &[(&str, &str)] = &[
    ("functionalRequirement", "Functional Requirement"),
    ("interfaceRequirement", "Interface Requirement"),
    ("performanceRequirement", "Performance Requirement"),
    ("physicalRequirement", "Physical Requirement"),
    ("designConstraint", "Design Constraint"),
    ("requirement", "Requirement"),
    ("element", "Element"),
];

/// The seven kinds of relationship.
const RELATIONS: &[&str] =
    &["contains", "copies", "derives", "satisfies", "verifies", "refines", "traces"];

/// One requirement or element.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    /// What the box says it is: `<<Requirement>>`, `<<Element>>`.
    kind: String,
    name: String,
    /// `id`, `text`, `risk`, `verifymethod` for a requirement; `type` and `docref` for an element.
    fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
struct Relation {
    from: usize,
    to: usize,
    kind: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    nodes: Vec<Node>,
    by_name: HashMap<String, usize>,
    relations: Vec<Relation>,
    direction: Direction,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, source, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram {
        direction: Direction::parse(&source.header).unwrap_or(Direction::Down),
        ..Diagram::default()
    };
    let mut open: Option<usize> = None;
    for line in source.statements() {
        let text = line.text.trim();
        if text == "}" {
            open = None;
            continue;
        }
        if let Some(node) = open {
            if let Some((key, value)) = text.split_once(':') {
                diagram.nodes[node]
                    .fields
                    .push((key.trim().to_owned(), source::label(value)));
            }
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
        if let Some(node) = read_block(&mut diagram, line) {
            open = Some(node);
            continue;
        }
        if read_relation(&mut diagram, line)? {
            continue;
        }
    }
    Ok(diagram)
}

fn is_decoration(line: &Line) -> bool {
    ["style", "classDef", "class", "click"].iter().any(|word| line.starts_with_word(word))
}

/// Read `functionalRequirement name {`, giving back the node whose fields follow.
fn read_block(diagram: &mut Diagram, line: &Line) -> Option<usize> {
    for (word, shown) in KINDS {
        let Some(rest) = line.after_word(word) else {
            continue;
        };
        let name = rest.trim().trim_end_matches('{').trim();
        if name.is_empty() {
            continue;
        }
        let index = node_of(diagram, name);
        diagram.nodes[index].kind = (*shown).to_owned();
        return Some(index);
    }
    None
}

fn node_of(diagram: &mut Diagram, name: &str) -> usize {
    let name = source::unquote(name.trim()).trim().to_owned();
    if let Some(&known) = diagram.by_name.get(&name) {
        return known;
    }
    diagram.nodes.push(Node {
        kind: "Requirement".to_owned(),
        name: name.clone(),
        fields: Vec::new(),
    });
    diagram.by_name.insert(name, diagram.nodes.len() - 1);
    diagram.nodes.len() - 1
}

/// Read `A - satisfies -> B` and `B <- satisfies - A`, which mean the same thing.
fn read_relation(diagram: &mut Diagram, line: &Line) -> Result<bool, Problem> {
    let text = line.text.trim();
    let Some(kind) = RELATIONS.iter().find(|word| text.contains(**word)) else {
        return Ok(false);
    };
    let at = text.find(*kind).expect("it was just found");
    let before = text[..at].trim();
    let after = text[at + kind.len()..].trim();
    // Which way it points is in the arrows, not in the order the two names were written.
    let forwards = after.starts_with("->");
    let backwards = before.ends_with("<-");
    if !forwards && !backwards {
        return Ok(false);
    }
    let left = before.trim_end_matches(['-', '<']).trim();
    let right = after.trim_start_matches(['-', '>']).trim();
    if left.is_empty() || right.is_empty() {
        return Err(Problem::at(line, "a relationship needs something at each end of it"));
    }
    let left = node_of(diagram, left);
    let right = node_of(diagram, right);
    let (from, to) = if forwards { (left, right) } else { (right, left) };
    diagram.relations.push(Relation { from, to, kind: (*kind).to_owned() });
    Ok(true)
}

/// One box, measured.
struct Measured {
    kind: Label,
    name: Label,
    fields: Vec<Label>,
    size: Size,
    head_height: f32,
}

fn draw(diagram: &Diagram, source: &Source, options: &Options) -> Scene {
    let boxes: Vec<Measured> = diagram.nodes.iter().map(|node| measure(node, options)).collect();
    let label_style = options.style(0.85, false);
    let labels: Vec<Label> = diagram
        .relations
        .iter()
        .map(|relation| text::measure(&relation.kind, &label_style, options.metrics, text::EDGE_WRAP))
        .collect();

    let mut graph = layered::Graph { direction: diagram.direction, ..layered::Graph::default() };
    for measured in &boxes {
        graph.add_node(measured.size, None);
    }
    for (index, relation) in diagram.relations.iter().enumerate() {
        graph.edges.push(EdgeSpec {
            from: relation.from,
            to: relation.to,
            label: labels[index].size(),
            span: 1,
        });
    }
    let placed = layered::layout(&graph);

    let mut scene = Scene::new();
    let top = parts::title(&mut scene, source, options, placed.size.width);
    let origin = Point::new(parts::MARGIN, top + parts::MARGIN);
    draw_relations(&mut scene, diagram, &placed, origin, &labels, options);
    for (index, node) in diagram.nodes.iter().enumerate() {
        draw_node(
            &mut scene,
            node,
            &boxes[index],
            placed.nodes[index].moved(origin.x, origin.y),
            options,
        );
    }
    parts::finish(&mut scene);
    scene
}

fn measure(node: &Node, options: &Options) -> Measured {
    let kind_style = options.style(0.8, false);
    let name_style = options.style(1.0, true);
    let field_style = options.style(0.82, false);
    let kind = text::measure_unwrapped(&format!("«{}»", node.kind), &kind_style, options.metrics);
    let name = text::measure_unwrapped(&node.name, &name_style, options.metrics);
    let fields: Vec<Label> = node
        .fields
        .iter()
        .map(|(key, value)| {
            text::measure(&format!("{key}: {value}"), &field_style, options.metrics, 220.0)
        })
        .collect();
    let widest = [kind.width, name.width]
        .into_iter()
        .chain(fields.iter().map(|label| label.width))
        .fold(0.0_f32, f32::max);
    let head_height = kind.height + name.height + parts::PADDING_Y * 2.0;
    let body: f32 = fields.iter().map(|label| label.height).sum();
    Measured {
        size: Size::new(
            widest + parts::PADDING_X * 2.0,
            head_height + if fields.is_empty() { 0.0 } else { body + parts::PADDING_Y * 2.0 },
        ),
        kind,
        name,
        fields,
        head_height,
    }
}

fn draw_node(
    scene: &mut Scene,
    node: &Node,
    measured: &Measured,
    rect: Rect,
    options: &Options,
) {
    let theme = &options.theme;
    let stroke = Stroke::new(theme.node_stroke, parts::LINE);
    scene.add(Item::Rect {
        rect,
        radius: parts::CORNER,
        fill: Some(theme.node_fill),
        stroke: Some(stroke),
    });
    let mut y = rect.top() + parts::PADDING_Y;
    parts::one_line(
        scene,
        &format!("«{}»", node.kind),
        Point::new(rect.centre().x, y),
        &parts::text_style(options, 0.8, false, theme.dim),
        Anchor::Middle,
        measured.kind.width,
    );
    y += measured.kind.height;
    parts::one_line(
        scene,
        &node.name,
        Point::new(rect.centre().x, y),
        &parts::text_style(options, 1.0, true, theme.text),
        Anchor::Middle,
        measured.name.width,
    );
    if node.fields.is_empty() {
        return;
    }
    let divider = rect.top() + measured.head_height;
    scene.add(Item::Line {
        points: vec![Point::new(rect.left(), divider), Point::new(rect.right(), divider)],
        stroke,
        dash: super::scene::Dash::Solid,
    });
    let style = parts::text_style(options, 0.82, false, theme.text);
    let mut at = divider + parts::PADDING_Y;
    for (index, (key, value)) in node.fields.iter().enumerate() {
        let label = &measured.fields[index];
        parts::label_at(
            scene,
            &text::measure(
                &format!("{key}: {value}"),
                &options.style(0.82, false),
                options.metrics,
                220.0,
            ),
            Point::new(rect.left() + parts::PADDING_X, at),
            &style,
            Anchor::Start,
        );
        at += label.height;
    }
}

fn draw_relations(
    scene: &mut Scene,
    diagram: &Diagram,
    placed: &layered::Placed,
    origin: Point,
    labels: &[Label],
    options: &Options,
) {
    let theme = &options.theme;
    for (index, relation) in diagram.relations.iter().enumerate() {
        let mut path: Vec<Point> = placed.edges[index]
            .iter()
            .map(|point| Point::new(point.x + origin.x, point.y + origin.y))
            .collect();
        if path.len() < 2 {
            continue;
        }
        let last = path.len() - 1;
        path[0] = Outline::Rect(placed.nodes[relation.from].moved(origin.x, origin.y))
            .border_towards(path[1]);
        path[last] = Outline::Rect(placed.nodes[relation.to].moved(origin.x, origin.y))
            .border_towards(path[last - 1]);
        scene.add(Item::Line {
            points: parts::trimmed(&path, 0.0, parts::ending_inset(Ending::Arrow)),
            stroke: Stroke::new(theme.line, parts::LINE),
            dash: parts::DASH,
        });
        parts::ending(
            scene,
            Ending::Arrow,
            path[last],
            parts::heading(&path),
            theme.line,
            theme.node_fill,
        );
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
    fn every_requirement_kind_is_read_with_its_fields() {
        let text = "requirementDiagram\n\
            functionalRequirement test_req {\n\
            id: 1\n text: the system shall work\n risk: high\n verifymethod: test\n }\n\
            element test_entity {\n type: simulation\n docref: none\n }\n";
        let diagram = diagram(text);
        assert_eq!(diagram.nodes.len(), 2);
        assert_eq!(diagram.nodes[0].kind, "Functional Requirement");
        assert_eq!(diagram.nodes[0].fields.len(), 4);
        assert_eq!(diagram.nodes[0].fields[1], ("text".to_owned(), "the system shall work".to_owned()));
        assert_eq!(diagram.nodes[1].kind, "Element");
    }

    #[test]
    fn the_longer_kind_words_are_matched_before_the_shorter_one() {
        // `requirement` is a prefix of nothing but is a *suffix* of five of the others, and it is
        // also the name of a kind. Reading them in the wrong order gives every requirement the same
        // kind, which is the sort of fault nobody notices in a screenshot.
        let text = "requirementDiagram\n\
            performanceRequirement a {\n id: 1\n }\n\
            designConstraint b {\n id: 2\n }\n\
            requirement c {\n id: 3\n }\n";
        let diagram = diagram(text);
        let kinds: Vec<&str> = diagram.nodes.iter().map(|node| node.kind.as_str()).collect();
        assert_eq!(kinds, vec!["Performance Requirement", "Design Constraint", "Requirement"]);
    }

    #[test]
    fn a_relationship_written_backwards_points_the_same_way_as_one_written_forwards() {
        let forwards = diagram("requirementDiagram\n a - satisfies -> b\n");
        let backwards = diagram("requirementDiagram\n b <- satisfies - a\n");
        assert_eq!(forwards.relations[0].kind, "satisfies");
        // In both, the edge runs from `a` to `b`.
        assert_eq!(forwards.nodes[forwards.relations[0].from].name, "a");
        assert_eq!(forwards.nodes[forwards.relations[0].to].name, "b");
        assert_eq!(backwards.nodes[backwards.relations[0].from].name, "a");
        assert_eq!(backwards.nodes[backwards.relations[0].to].name, "b");
    }

    #[test]
    fn all_seven_relationship_words_are_read() {
        let text = "requirementDiagram\n\
            a - contains -> b\n c - copies -> d\n e - derives -> f\n g - satisfies -> h\n \
            i - verifies -> j\n k - refines -> l\n m - traces -> n\n";
        let diagram = diagram(text);
        let kinds: Vec<&str> = diagram.relations.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, RELATIONS.to_vec());
    }

    #[test]
    fn a_requirement_diagram_is_drawn_and_keeps_every_property() {
        let text = "requirementDiagram\n\
            requirement top {\n id: 1\n text: the top one\n risk: high\n verifymethod: test\n }\n\
            functionalRequirement child {\n id: 1.1\n text: a smaller one\n risk: low\n verifymethod: inspection\n }\n\
            element the_test {\n type: simulation\n }\n\
            top - contains -> child\n\
            the_test - verifies -> top\n";
        let scene = check::drawn(
            text,
            &options(),
            &["top", "child", "the_test", "contains", "verifies", "the top one"],
        );
        assert!(!scene.is_empty());
    }
}
