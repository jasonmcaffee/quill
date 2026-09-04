//! Flowcharts: `flowchart` and `graph`.
//!
//! The most written of Mermaid's diagrams by a long way, so it gets the most care. Fourteen node
//! shapes, six link styles, labels on links written either way round, `&` for several nodes at once,
//! nested subgraphs, and all four directions.
//!
//! ## Finding the links, which is the whole of the parsing
//!
//! A statement is an alternating run of node specifications and links: `A[Start] --> B{Ok?} -->|yes|
//! C(Done)`. So the parser finds the links and everything between two of them is a node. Finding
//! them is the only fiddly part, because a `-` is also an ordinary character.
//!
//! A **core** is a run of two or more of `-`, `=`, `.` and `~`, found outside any bracket or quote —
//! so the `-` in `A[a - b]` and in `A["-->"]` is never mistaken for a link. After a core comes an
//! optional **head**: `>`, `o` or `x`.
//!
//! Then one rule settles the ambiguity that Mermaid's own grammar settles lexically, and it is worth
//! writing down because it is not obvious:
//!
//! - A core **with** a head is a whole link. `-->`, `==>`, `-.->`, `--o`, `--x`.
//! - A core with no head that is **three or more** characters is a whole link. `---`, `===`, `-.-`,
//!   `~~~`.
//! - A core with no head that is **exactly two** — `--`, `==`, `-.` — is the *opening* of a link
//!   whose label follows and which the next core closes. That is Mermaid's `A-- text -->B`.
//!
//! Which is what tells `A --- B --- C`, a chain of three nodes, apart from `A -- text --> B`, one
//! link with a label on it. The two are otherwise identical in shape, and reading the second as a
//! chain would silently turn a label into a node.

use std::collections::HashMap;

use super::layered::{self, Direction, EdgeSpec, GroupSpec};
use super::parts::{self, Ending, Outline};
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Size, Stroke};
use super::shapes::Shape;
use super::source::{self, Line, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How a link is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkStyle {
    #[default]
    Solid,
    Dotted,
    Thick,
    /// `~~~`, which reserves the layout but draws nothing.
    Invisible,
}

/// One link, once it has been read.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub style: LinkStyle,
    /// What is drawn at the end it points at.
    pub head: Ending,
    /// What is drawn at the end it comes from, for `<-->` and `o--o`.
    pub tail: Ending,
    pub label: String,
    /// How many ranks it must span, from the number of dashes.
    pub span: usize,
}

/// One node, once it has been read.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    id: String,
    label: String,
    shape: Shape,
    group: Option<usize>,
}

/// A whole flowchart, read but not yet placed.
#[derive(Debug, Clone, PartialEq, Default)]
struct Chart {
    nodes: Vec<Node>,
    by_id: HashMap<String, usize>,
    links: Vec<(usize, usize, Link)>,
    groups: Vec<Group>,
    direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
struct Group {
    title: String,
    parent: Option<usize>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let chart = read(source)?;
    Ok(draw(&chart, source, options))
}

/// Read the whole diagram.
fn read(source: &Source) -> Result<Chart, Problem> {
    let mut chart = Chart {
        direction: Direction::parse(&source.header).unwrap_or(Direction::Down),
        ..Chart::default()
    };
    // Which subgraph the lines being read are inside. A stack, because they nest.
    let mut open: Vec<usize> = Vec::new();
    for line in source.statements() {
        if line.starts_with_word("end") {
            open.pop();
            continue;
        }
        if let Some(rest) = line.after_word("subgraph") {
            let parent = open.last().copied();
            chart.groups.push(Group { title: subgraph_title(rest), parent });
            open.push(chart.groups.len() - 1);
            continue;
        }
        if let Some(rest) = line.after_word("direction") {
            // Mermaid lets a subgraph set its own direction. Unluminate lays a subgraph out with the
            // diagram's, because a box whose contents run the other way needs the layout to place a
            // frame whose inside disagrees with its outside, and the picture is worse for it.
            if open.is_empty() {
                if let Some(direction) = Direction::parse(rest) {
                    chart.direction = direction;
                }
            }
            continue;
        }
        if is_decoration(line) {
            continue;
        }
        read_statement(&mut chart, line, open.last().copied())?;
    }
    Ok(chart)
}

/// A subgraph's own title: `subgraph one[The First]`, or just `subgraph The First`.
fn subgraph_title(rest: &str) -> String {
    if let Some(open) = rest.find('[') {
        if let Some(close) = rest.rfind(']') {
            if close > open {
                return source::label(&rest[open + 1..close]);
            }
        }
    }
    source::label(rest)
}

/// True for a line that says how something looks or what it does when clicked.
///
/// Every one of these is read and thrown away, which §13 of the design document records: a document
/// does not get to choose the window's colours, and nothing in a diagram is going to run.
fn is_decoration(line: &Line) -> bool {
    ["style", "classDef", "class", "click", "linkStyle", "linkstyle", "callback", "href"]
        .iter()
        .any(|word| line.starts_with_word(word))
}

/// Read one statement: a run of nodes with links between them.
fn read_statement(chart: &mut Chart, line: &Line, group: Option<usize>) -> Result<(), Problem> {
    let pieces = split_into_links(&line.text);
    if pieces.links.is_empty() {
        // No link on the line, so it is a node being introduced or relabelled on its own.
        for part in source::split_outside_quotes(&pieces.parts[0], '&') {
            if !part.trim().is_empty() {
                node_of(chart, part.trim(), group, line)?;
            }
        }
        return Ok(());
    }
    let mut previous: Vec<usize> = Vec::new();
    for (index, part) in pieces.parts.iter().enumerate() {
        let here: Vec<usize> = source::split_outside_quotes(part, '&')
            .iter()
            .filter(|piece| !piece.trim().is_empty())
            .map(|piece| node_of(chart, piece.trim(), group, line))
            .collect::<Result<_, _>>()?;
        if here.is_empty() {
            return Err(Problem::at(line, "there is a link here with nothing on one side of it"));
        }
        if index > 0 {
            let link = pieces.links[index - 1].clone();
            for &from in &previous {
                for &to in &here {
                    chart.links.push((from, to, link.clone()));
                }
            }
        }
        previous = here;
    }
    Ok(())
}

/// A statement split into the node specifications and the links between them.
struct Pieces {
    parts: Vec<String>,
    links: Vec<Link>,
}

/// Find every link in a statement, and hand back what is between them.
fn split_into_links(text: &str) -> Pieces {
    let mut parts = Vec::new();
    let mut links = Vec::new();
    let mut at = 0;
    while let Some(found) = next_link(text, at) {
        parts.push(text[at..found.start].to_owned());
        links.push(found.link);
        at = found.end;
    }
    parts.push(text[at..].to_owned());
    Pieces { parts, links }
}

/// One link found in a statement.
struct Found {
    start: usize,
    end: usize,
    link: Link,
}

/// The next link at or after `from`, or nothing when the rest is all node.
fn next_link(text: &str, from: usize) -> Option<Found> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < bytes.len() {
        if let Some(after) = skip_bracketed(text, at) {
            at = after;
            continue;
        }
        let Some(core_end) = core_at(text, at) else {
            at += 1;
            continue;
        };
        let core = &text[at..core_end];
        let (head, head_end) = head_at(text, core_end);
        let tail = tail_before(text, at);
        let tail_start = if tail == Ending::None { at } else { at - 1 };
        // A two character core with no head opens a labelled link that the next core closes.
        if head == Ending::None && opens_a_label(core) {
            if let Some(closing) = closing_core(text, head_end) {
                return Some(Found {
                    start: tail_start,
                    end: closing.end,
                    link: Link {
                        style: style_of(core),
                        head: closing.head,
                        tail,
                        label: source::label(&text[head_end..closing.start]),
                        span: span_of(core),
                    },
                });
            }
        }
        // Otherwise it is a whole link, and its label may follow it in bars.
        let (label, end) = bar_label(text, head_end);
        return Some(Found {
            start: tail_start,
            end,
            link: Link {
                style: style_of(core),
                head,
                tail,
                label,
                span: span_of(core),
            },
        });
    }
    None
}

/// The end of the bracketed or quoted run starting at `at`, if one starts there.
///
/// This is what keeps the `-` in `A[a - b]` and the whole of `A["-->"]` out of the link scanner.
fn skip_bracketed(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let open = *bytes.get(at)?;
    let close = match open {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        b'"' => b'"',
        _ => return None,
    };
    let mut depth = 0;
    let mut index = at;
    let mut quoted = false;
    while index < bytes.len() {
        let here = bytes[index];
        if open == b'"' {
            if index > at && here == b'"' {
                return Some(index + 1);
            }
        } else {
            if here == b'"' {
                quoted = !quoted;
            } else if !quoted && here == open {
                depth += 1;
            } else if !quoted && here == close {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
        }
        index += 1;
    }
    Some(bytes.len())
}

/// The end of a link core starting at `at`, if one does.
///
/// A core is two or more of `-`, `=`, `.` and `~`. Two is the fewest, so the hyphen in `my-node` is
/// an ordinary character and never a link.
fn core_at(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = at;
    while end < bytes.len() && matches!(bytes[end], b'-' | b'=' | b'.' | b'~') {
        end += 1;
    }
    (end - at >= 2).then_some(end)
}

/// The ending at `at`, and where it finishes.
///
/// `o` and `x` are heads only when what follows could not be part of a name, so `A --o B` is a
/// circle ending and `A --order` is an open link to a node called `order`.
fn head_at(text: &str, at: usize) -> (Ending, usize) {
    let bytes = text.as_bytes();
    match bytes.get(at) {
        Some(b'>') => (Ending::Arrow, at + 1),
        Some(b'o') if !continues_a_name(bytes.get(at + 1)) => (Ending::Circle, at + 1),
        Some(b'x') if !continues_a_name(bytes.get(at + 1)) => (Ending::Cross, at + 1),
        _ => (Ending::None, at),
    }
}

/// The ending just before `at`, for `<--`, `o--o` and `x--x`.
fn tail_before(text: &str, at: usize) -> Ending {
    if at == 0 {
        return Ending::None;
    }
    let bytes = text.as_bytes();
    match bytes[at - 1] {
        b'<' => Ending::Arrow,
        b'o' if at < 2 || !continues_a_name(Some(&bytes[at - 2])) => Ending::Circle,
        b'x' if at < 2 || !continues_a_name(Some(&bytes[at - 2])) => Ending::Cross,
        _ => Ending::None,
    }
}

/// True when this byte could be part of a node's name.
fn continues_a_name(byte: Option<&u8>) -> bool {
    matches!(byte, Some(b) if b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
}

/// True for a core that opens a labelled link rather than being one on its own.
fn opens_a_label(core: &str) -> bool {
    matches!(core, "--" | "==" | "-.")
}

/// A closing core after a labelled link's text.
struct Closing {
    start: usize,
    end: usize,
    head: Ending,
}

/// Find the core that closes a labelled link, skipping the label itself.
fn closing_core(text: &str, from: usize) -> Option<Closing> {
    let mut at = from;
    while at < text.len() {
        if let Some(after) = skip_bracketed(text, at) {
            at = after;
            continue;
        }
        if let Some(core_end) = core_at(text, at) {
            let (head, end) = head_at(text, core_end);
            return Some(Closing { start: at, end, head });
        }
        at += 1;
    }
    None
}

/// A label written after the link in bars: `-->|yes|`.
fn bar_label(text: &str, from: usize) -> (String, usize) {
    let rest = &text[from..];
    let trimmed = rest.trim_start();
    let skipped = rest.len() - trimmed.len();
    let Some(inner) = trimmed.strip_prefix('|') else {
        return (String::new(), from);
    };
    let Some(close) = inner.find('|') else {
        return (String::new(), from);
    };
    (source::label(&inner[..close]), from + skipped + 1 + close + 1)
}

fn style_of(core: &str) -> LinkStyle {
    if core.contains('~') {
        LinkStyle::Invisible
    } else if core.contains('.') {
        LinkStyle::Dotted
    } else if core.contains('=') {
        LinkStyle::Thick
    } else {
        LinkStyle::Solid
    }
}

/// How many ranks a link asks to span, from how many dashes were written.
///
/// `-->` is two dashes and spans one rank; every dash past two adds another, which is Mermaid's own
/// rule for `---->` and is how an author pushes a node further down the page.
fn span_of(core: &str) -> usize {
    let marks = core.chars().filter(|c| matches!(c, '-' | '=')).count();
    marks.saturating_sub(2).max(1)
}

/// Find or make the node a piece of text names, and update its label and shape if it carries them.
fn node_of(
    chart: &mut Chart,
    piece: &str,
    group: Option<usize>,
    line: &Line,
) -> Result<usize, Problem> {
    let (id, label, shape) = read_node(piece, line)?;
    if let Some(&known) = chart.by_id.get(&id) {
        // A node named again with a shape or a label takes them: `A --> B` then `B[Done]` is how a
        // great many flowcharts are written.
        if let Some(label) = label {
            chart.nodes[known].label = label;
            chart.nodes[known].shape = shape;
        }
        return Ok(known);
    }
    let label = label.unwrap_or_else(|| id.clone());
    chart.nodes.push(Node { id: id.clone(), label, shape, group });
    chart.by_id.insert(id, chart.nodes.len() - 1);
    Ok(chart.nodes.len() - 1)
}

/// Read one node specification: its name, the words in it, and its shape.
fn read_node(piece: &str, line: &Line) -> Result<(String, Option<String>, Shape), Problem> {
    let piece = piece.trim();
    if piece.is_empty() {
        return Err(Problem::at(line, "a node with no name"));
    }
    // `:::class` picks a colour scheme out of the document, which Unluminate reads and ignores.
    let piece = match piece.find(":::") {
        Some(at) => piece[..at].trim(),
        None => piece,
    };
    // The newer `id@{ shape: rect, label: "..." }` form.
    if let Some(at) = piece.find("@{") {
        let (id, rest) = piece.split_at(at);
        return Ok(read_curly_node(id.trim(), rest));
    }
    let Some(open) = piece.find(['[', '(', '{', '>']) else {
        return Ok((piece.to_owned(), None, Shape::Rect));
    };
    let id = piece[..open].trim().to_owned();
    if id.is_empty() {
        return Err(Problem::at(line, "a shape with no name in front of it"));
    }
    let body = &piece[open..];
    let Some((shape, inner)) = read_shape(body) else {
        return Err(Problem::at(line, format!("`{body}` is not a node shape Unluminate knows")));
    };
    Ok((id, Some(source::label(inner)), shape))
}

/// Read a node specification for a caller that has no line to blame for a mistake.
///
/// The block diagram uses the flowchart's own fourteen node shapes, so it reads them through here
/// rather than keeping a second copy of the table. It is deliberately **permissive** where
/// [`read_node`] is strict: brackets that do not match any shape leave a plain box named by the
/// whole piece, because a block diagram has no arrows to lose and refusing it would take the whole
/// picture away over one square bracket.
pub fn read_block_shape(piece: &str) -> (String, Option<String>, Shape) {
    let piece = piece.trim();
    let piece = match piece.find(":::") {
        Some(at) => piece[..at].trim(),
        None => piece,
    };
    if let Some(at) = piece.find("@{") {
        let (id, rest) = piece.split_at(at);
        return read_curly_node(id.trim(), rest);
    }
    let Some(open) = piece.find(['[', '(', '{', '>']) else {
        return (piece.to_owned(), None, Shape::Rect);
    };
    let id = piece[..open].trim();
    match read_shape(&piece[open..]) {
        Some((shape, inner)) if !id.is_empty() => {
            (id.to_owned(), Some(source::label(inner)), shape)
        }
        _ => (piece.to_owned(), None, Shape::Rect),
    }
}

/// Read Mermaid's newer `@{ shape: ..., label: ... }` form.
///
/// Only the two keys that change the picture are read. The rest — icons, images, a constraint — fall
/// back to a plain box, which is what the design document says happens to anything that would need a
/// file fetched or a font of icons.
fn read_curly_node(id: &str, rest: &str) -> (String, Option<String>, Shape) {
    let inner = rest.trim_start_matches("@{").trim_end_matches('}');
    let mut label = None;
    let mut shape = Shape::Rect;
    for part in source::split_outside_quotes(inner, ',') {
        let Some((key, value)) = part.split_once(':') else {
            continue;
        };
        match key.trim() {
            "label" => label = Some(source::label(value)),
            "shape" => shape = named_shape(&source::unquote(value)),
            _ => {}
        }
    }
    (id.to_owned(), label, shape)
}

/// Mermaid's shape names, as the `@{ shape: ... }` form uses them.
fn named_shape(name: &str) -> Shape {
    match name.trim() {
        "rounded" | "rounded-rect" | "event" => Shape::Round,
        "stadium" | "pill" | "terminal" => Shape::Stadium,
        "subprocess" | "subroutine" | "framed-rectangle" => Shape::Subroutine,
        "cylinder" | "database" | "db" | "cyl" => Shape::Cylinder,
        "circle" | "circ" => Shape::Circle,
        "double-circle" | "dbl-circ" => Shape::DoubleCircle,
        "diamond" | "decision" | "diam" | "question" => Shape::Diamond,
        "hexagon" | "hex" | "prepare" => Shape::Hexagon,
        "lean-r" | "lean-right" | "in-out" => Shape::Parallelogram,
        "lean-l" | "lean-left" | "out-in" => Shape::ParallelogramAlt,
        "trap-b" | "trapezoid-bottom" | "priority" => Shape::Trapezoid,
        "trap-t" | "trapezoid-top" | "manual" => Shape::TrapezoidAlt,
        "odd" | "flag" => Shape::Asymmetric,
        _ => Shape::Rect,
    }
}

/// Match a node's brackets against the fourteen shapes, longest opener first.
///
/// The order matters: `[[` has to be tried before `[`, and `[/x\]` before `[/x/]`, or a subroutine
/// reads as a rectangle whose label begins with a bracket.
fn read_shape(body: &str) -> Option<(Shape, &str)> {
    const FORMS: &[(&str, &str, Shape)] = &[
        ("(((", ")))", Shape::DoubleCircle),
        ("((", "))", Shape::Circle),
        ("([", "])", Shape::Stadium),
        ("[[", "]]", Shape::Subroutine),
        ("[(", ")]", Shape::Cylinder),
        ("[/", "\\]", Shape::Trapezoid),
        ("[\\", "/]", Shape::TrapezoidAlt),
        ("[/", "/]", Shape::Parallelogram),
        ("[\\", "\\]", Shape::ParallelogramAlt),
        ("{{", "}}", Shape::Hexagon),
        (">", "]", Shape::Asymmetric),
        ("[", "]", Shape::Rect),
        ("(", ")", Shape::Round),
        ("{", "}", Shape::Diamond),
    ];
    for (open, close, shape) in FORMS {
        let Some(rest) = body.strip_prefix(open) else {
            continue;
        };
        let Some(inner) = rest.strip_suffix(close) else {
            continue;
        };
        return Some((*shape, inner));
    }
    None
}

/// Lay the chart out and draw it.
fn draw(chart: &Chart, source: &Source, options: &Options) -> Scene {
    let style = options.style(1.0, false);
    let labels: Vec<Label> = chart
        .nodes
        .iter()
        .map(|node| text::measure(&node.label, &style, options.metrics, text::WRAP))
        .collect();
    let link_style = options.style(0.85, false);
    let link_labels: Vec<Label> = chart
        .links
        .iter()
        .map(|(_, _, link)| text::measure(&link.label, &link_style, options.metrics, text::EDGE_WRAP))
        .collect();

    let graph = build_graph(chart, &labels, &link_labels, options);
    let placed = layered::layout(&graph);

    let mut scene = Scene::new();
    let top = parts::title(&mut scene, source, options, placed.size.width);
    let origin = Point::new(parts::MARGIN, top + parts::MARGIN);

    draw_groups(&mut scene, chart, &placed, origin, options);
    draw_links(&mut scene, chart, &placed, origin, &link_labels, options);
    draw_nodes(&mut scene, chart, &placed, origin, &labels, options);
    parts::finish(&mut scene);
    scene
}

/// Turn the chart into the graph the layered layout takes.
fn build_graph(
    chart: &Chart,
    labels: &[Label],
    link_labels: &[Label],
    options: &Options,
) -> layered::Graph {
    let title_style = options.style(0.95, true);
    let mut graph = layered::Graph { direction: chart.direction, ..layered::Graph::default() };
    for group in &chart.groups {
        let title = text::measure_unwrapped(&group.title, &title_style, options.metrics);
        graph.groups.push(GroupSpec {
            title: Size::new(title.width, title.height + 6.0),
            parent: group.parent,
        });
    }
    for (index, node) in chart.nodes.iter().enumerate() {
        graph.add_node(node.shape.size_for(labels[index].size()), node.group);
    }
    for (index, (from, to, link)) in chart.links.iter().enumerate() {
        graph.edges.push(EdgeSpec {
            from: *from,
            to: *to,
            label: link_labels[index].size(),
            span: link.span,
        });
    }
    graph
}

/// Draw the subgraph frames, behind everything else.
fn draw_groups(
    scene: &mut Scene,
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    options: &Options,
) {
    let theme = &options.theme;
    for (index, group) in chart.groups.iter().enumerate() {
        let frame = placed.groups[index].moved(origin.x, origin.y);
        if frame.width <= 0.0 {
            continue;
        }
        scene.add(Item::Rect {
            rect: frame,
            radius: parts::CORNER,
            fill: Some(theme.group_fill),
            stroke: Some(Stroke::new(theme.group_stroke, parts::LINE)),
        });
        if group.title.trim().is_empty() {
            continue;
        }
        let style = parts::text_style(options, 0.95, true, theme.text);
        let width =
            text::width_of(&group.title, &options.style(0.95, true), options.metrics);
        parts::one_line(
            scene,
            &group.title,
            Point::new(frame.left() + 12.0, frame.top() + 6.0),
            &style,
            Anchor::Start,
            width,
        );
    }
}

/// Draw every node: its shape, then its words.
fn draw_nodes(
    scene: &mut Scene,
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    labels: &[Label],
    options: &Options,
) {
    let theme = &options.theme;
    let style = parts::text_style(options, 1.0, false, theme.text);
    for (index, node) in chart.nodes.iter().enumerate() {
        let rect = placed.nodes[index].moved(origin.x, origin.y);
        node.shape.draw(
            scene,
            rect,
            theme.node_fill,
            Stroke::new(theme.node_stroke, parts::LINE),
        );
        parts::centred_label(scene, &labels[index], rect, &style);
    }
}

/// Draw every link: its line, its endings, and its label.
fn draw_links(
    scene: &mut Scene,
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    labels: &[Label],
    options: &Options,
) {
    let theme = &options.theme;
    for (index, (from, to, link)) in chart.links.iter().enumerate() {
        if link.style == LinkStyle::Invisible {
            continue;
        }
        let path: Vec<Point> = placed.edges[index]
            .iter()
            .map(|point| Point::new(point.x + origin.x, point.y + origin.y))
            .collect();
        let path = if path.len() >= 2 {
            clip_to_shapes(chart, placed, origin, *from, *to, path)
        } else {
            self_loop(chart, placed, origin, *from)
        };
        if path.len() < 2 {
            continue;
        }
        let width = if link.style == LinkStyle::Thick { parts::THICK } else { parts::LINE };
        let stroke = Stroke::new(theme.line, width);
        let dash = if link.style == LinkStyle::Dotted { parts::DASH } else { Dash::Solid };
        let drawn = parts::trimmed(
            &path,
            parts::ending_inset(link.tail),
            parts::ending_inset(link.head),
        );
        scene.add(Item::Line { points: drawn, stroke, dash });
        parts::ending(
            scene,
            link.head,
            path[path.len() - 1],
            parts::heading(&path),
            theme.line,
            theme.node_fill,
        );
        parts::ending(scene, link.tail, path[0], parts::tail_heading(&path), theme.line, theme.node_fill);
        draw_link_label(scene, &labels[index], &path, options);
    }
}

/// Cut a link's two ends back to the borders of the shapes it joins.
fn clip_to_shapes(
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    from: usize,
    to: usize,
    mut path: Vec<Point>,
) -> Vec<Point> {
    let last = path.len() - 1;
    let start = outline_of(chart, placed, origin, from);
    let finish = outline_of(chart, placed, origin, to);
    path[0] = start.border_towards(path[1]);
    path[last] = finish.border_towards(path[last - 1]);
    path
}

fn outline_of(
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    index: usize,
) -> Outline {
    chart.nodes[index].shape.outline(placed.nodes[index].moved(origin.x, origin.y))
}

/// A link from a node to itself: a loop out of its right side and back again.
///
/// The layered layout cannot rank one of these, so it hands back nothing and this draws it, which is
/// what Mermaid does with them too.
fn self_loop(
    chart: &Chart,
    placed: &layered::Placed,
    origin: Point,
    node: usize,
) -> Vec<Point> {
    let rect = placed.nodes[node].moved(origin.x, origin.y);
    if rect.width <= 0.0 {
        return Vec::new();
    }
    let out = rect.width.min(rect.height).max(30.0) * 0.7;
    let _ = chart;
    vec![
        Point::new(rect.right(), rect.centre().y - rect.height * 0.2),
        Point::new(rect.right() + out, rect.centre().y - rect.height * 0.55),
        Point::new(rect.right() + out, rect.centre().y + rect.height * 0.55),
        Point::new(rect.right(), rect.centre().y + rect.height * 0.2),
    ]
}

/// Draw a link's label, on a small panel so the line behind it does not run through the words.
fn draw_link_label(scene: &mut Scene, label: &Label, path: &[Point], options: &Options) {
    if label.is_empty() {
        return;
    }
    let at = middle_of(path);
    let panel = Rect::around(at, Size::new(label.width + 8.0, label.height + 2.0));
    scene.add(Item::Rect {
        rect: panel,
        radius: 3.0,
        fill: Some(Paint::solid(options.theme.node_fill.color)),
        stroke: None,
    });
    parts::centred_label(
        scene,
        label,
        panel,
        &parts::text_style(options, 0.85, false, options.theme.dim),
    );
}

/// The middle of a polyline, measured along it.
fn middle_of(path: &[Point]) -> Point {
    let total: f32 = path.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    let mut walked = 0.0;
    for pair in path.windows(2) {
        let length = pair[0].distance(pair[1]);
        if walked + length >= total / 2.0 && length > 0.0 {
            return pair[0].towards(pair[1], (total / 2.0 - walked) / length);
        }
        walked += length;
    }
    path[path.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(text: &str) -> Chart {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    fn link_between(chart: &Chart, from: &str, to: &str) -> Link {
        let from = chart.by_id[from];
        let to = chart.by_id[to];
        chart
            .links
            .iter()
            .find(|(a, b, _)| *a == from && *b == to)
            .map(|(_, _, link)| link.clone())
            .unwrap_or_else(|| panic!("no link from {from} to {to}"))
    }

    #[test]
    fn a_chain_of_nodes_becomes_nodes_and_links() {
        let chart = chart("flowchart LR\n  A[Start] --> B{Ok?} --> C(Done)\n");
        assert_eq!(chart.direction, Direction::Right);
        assert_eq!(chart.nodes.len(), 3);
        assert_eq!(chart.nodes[0].label, "Start");
        assert_eq!(chart.nodes[0].shape, Shape::Rect);
        assert_eq!(chart.nodes[1].shape, Shape::Diamond);
        assert_eq!(chart.nodes[2].shape, Shape::Round);
        assert_eq!(chart.links.len(), 2);
    }

    #[test]
    fn all_fourteen_node_shapes_are_read() {
        let text = "flowchart TD\n\
            a[rect]\n b(round)\n c([stadium])\n d[[subroutine]]\n e[(cylinder)]\n \
            f((circle))\n g(((double)))\n h>asymmetric]\n i{diamond}\n j{{hexagon}}\n \
            k[/parallelogram/]\n l[\\parallelogram back\\]\n m[/trapezoid\\]\n n[\\trapezoid back/]\n";
        let chart = chart(text);
        let shapes: Vec<Shape> = chart.nodes.iter().map(|node| node.shape).collect();
        assert_eq!(
            shapes,
            vec![
                Shape::Rect, Shape::Round, Shape::Stadium, Shape::Subroutine, Shape::Cylinder,
                Shape::Circle, Shape::DoubleCircle, Shape::Asymmetric, Shape::Diamond,
                Shape::Hexagon, Shape::Parallelogram, Shape::ParallelogramAlt, Shape::Trapezoid,
                Shape::TrapezoidAlt,
            ]
        );
        assert_eq!(chart.nodes[6].label, "double", "the innermost brackets are the label");
    }

    #[test]
    fn every_link_style_and_ending_is_read() {
        let chart = chart(
            "flowchart LR\n A --> B\n A --- C\n A -.-> D\n A ==> E\n A --o F\n A --x G\n A <--> H\n",
        );
        assert_eq!(link_between(&chart, "A", "B").head, Ending::Arrow);
        assert_eq!(link_between(&chart, "A", "C").head, Ending::None);
        assert_eq!(link_between(&chart, "A", "D").style, LinkStyle::Dotted);
        assert_eq!(link_between(&chart, "A", "E").style, LinkStyle::Thick);
        assert_eq!(link_between(&chart, "A", "F").head, Ending::Circle);
        assert_eq!(link_between(&chart, "A", "G").head, Ending::Cross);
        let both = link_between(&chart, "A", "H");
        assert_eq!((both.tail, both.head), (Ending::Arrow, Ending::Arrow));
    }

    #[test]
    fn a_label_is_read_from_bars_and_from_between_the_dashes() {
        let chart = chart("flowchart LR\n A -->|yes| B\n C -- maybe --> D\n E -. later .-> F\n");
        assert_eq!(link_between(&chart, "A", "B").label, "yes");
        assert_eq!(link_between(&chart, "C", "D").label, "maybe");
        assert_eq!(link_between(&chart, "E", "F").label, "later");
    }

    #[test]
    fn three_dashes_is_a_chain_and_two_dashes_with_words_is_a_label() {
        // The one real ambiguity in the format. Reading the second as a chain would silently turn a
        // label into a node, and reading the first as a label would silently lose a node.
        let chain = chart("flowchart LR\n A --- B --- C\n");
        assert_eq!(chain.nodes.len(), 3, "three dashes joins three nodes");
        assert_eq!(chain.links.len(), 2);

        let labelled = chart("flowchart LR\n A -- text --> B\n");
        assert_eq!(labelled.nodes.len(), 2, "two dashes and words is one link with a label");
        assert_eq!(labelled.links.len(), 1);
        assert_eq!(labelled.links[0].2.label, "text");
    }

    #[test]
    fn a_dash_inside_a_label_or_a_name_is_not_a_link() {
        let chart = chart("flowchart LR\n my-node[\"a - b\"] --> other-node\n");
        assert_eq!(chart.nodes.len(), 2);
        assert_eq!(chart.nodes[0].id, "my-node");
        assert_eq!(chart.nodes[0].label, "a - b");
        assert_eq!(chart.nodes[1].id, "other-node");
        assert_eq!(chart.links.len(), 1);
    }

    #[test]
    fn an_arrow_written_inside_a_label_is_not_a_link() {
        let chart = chart("flowchart LR\n A[\"A --> B\"] --> B\n");
        assert_eq!(chart.nodes.len(), 2);
        assert_eq!(chart.nodes[0].label, "A --> B");
    }

    #[test]
    fn an_ampersand_joins_several_nodes_at_once() {
        let chart = chart("flowchart LR\n A & B --> C & D\n");
        assert_eq!(chart.nodes.len(), 4);
        assert_eq!(chart.links.len(), 4, "two sources times two targets");
    }

    #[test]
    fn extra_dashes_ask_for_a_longer_link() {
        let chart = chart("flowchart TD\n A --> B\n A ---> C\n A -----> D\n");
        assert_eq!(link_between(&chart, "A", "B").span, 1);
        assert_eq!(link_between(&chart, "A", "C").span, 1);
        assert_eq!(link_between(&chart, "A", "D").span, 3);
    }

    #[test]
    fn a_node_named_again_takes_its_later_label() {
        // Which is how a great many flowcharts are written: the links first, the words after.
        let chart = chart("flowchart LR\n A --> B\n B[Done]\n");
        assert_eq!(chart.nodes.len(), 2);
        assert_eq!(chart.nodes[1].label, "Done");
    }

    #[test]
    fn a_subgraph_holds_the_nodes_declared_inside_it() {
        let text = "flowchart TD\n\
            A --> B\n\
            subgraph inner[The Inside]\n  C --> D\nend\n\
            B --> C\n";
        let chart = chart(text);
        assert_eq!(chart.groups.len(), 1);
        assert_eq!(chart.groups[0].title, "The Inside");
        assert_eq!(chart.nodes[chart.by_id["C"]].group, Some(0));
        assert_eq!(chart.nodes[chart.by_id["D"]].group, Some(0));
        assert_eq!(chart.nodes[chart.by_id["A"]].group, None);
    }

    #[test]
    fn subgraphs_nest() {
        let text = "flowchart TD\nsubgraph a[Outer]\n  subgraph b[Inner]\n    X\n  end\n  Y\nend\n";
        let chart = chart(text);
        assert_eq!(chart.groups.len(), 2);
        assert_eq!(chart.groups[1].parent, Some(0));
        assert_eq!(chart.nodes[chart.by_id["X"]].group, Some(1));
        assert_eq!(chart.nodes[chart.by_id["Y"]].group, Some(0));
    }

    #[test]
    fn styling_and_clicking_are_read_and_ignored() {
        let text = "flowchart LR\n A --> B\n \
            style A fill:#f9f,stroke:#333\n classDef big font-size:40px\n class A big\n \
            click A \"https://example.com\" _blank\n linkStyle 0 stroke:#ff3\n";
        let chart = chart(text);
        assert_eq!(chart.nodes.len(), 2, "none of those lines makes a node");
        assert_eq!(chart.links.len(), 1);
    }

    #[test]
    fn the_newer_curly_form_is_read_for_its_shape_and_its_label() {
        let chart = chart("flowchart LR\n A@{ shape: database, label: \"The store\" } --> B\n");
        assert_eq!(chart.nodes[0].shape, Shape::Cylinder);
        assert_eq!(chart.nodes[0].label, "The store");
    }

    #[test]
    fn a_class_suffix_is_not_part_of_the_name() {
        let chart = chart("flowchart LR\n A:::important --> B\n");
        assert_eq!(chart.nodes[0].id, "A");
        assert_eq!(chart.nodes.len(), 2);
    }
}

#[cfg(test)]
mod drawing {
    use super::super::{check, Options};
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        // Leaked so the options can be returned; a test binary that ends is the only thing that
        // frees it, which is exactly what a `static` would do anyway.
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    #[test]
    fn a_flowchart_is_drawn_and_keeps_every_property() {
        let text = "flowchart TD\n\
            Start([Begin]) --> Check{Is it ready?}\n\
            Check -->|yes| Ship[Ship it]\n\
            Check -->|no| Fix[Fix it]\n\
            Fix --> Check\n\
            Ship --> Done(((Done)))\n";
        let scene = check::drawn(text, &options(), &["Begin", "Is it ready?", "Ship it", "Fix it", "yes", "no"]);
        assert!(scene.items.len() > 10, "a chart of five nodes draws more than ten things");
    }

    #[test]
    fn every_node_box_is_clear_of_every_other() {
        let text = "flowchart TD\n A --> B\n A --> C\n A --> D\n B --> E\n C --> E\n D --> E\n";
        let scene = check::drawn(text, &options(), &["A", "E"]);
        // Only the node shapes are rectangles in this chart, so no rectangle may touch another.
        check::no_two_rectangles_overlap(&scene.rects());
    }

    #[test]
    fn a_subgraph_frame_holds_its_members_and_misses_everything_else() {
        let text = "flowchart TD\n\
            Outside --> First\n\
            subgraph inner[Inside]\n  First --> Second\nend\n\
            Second --> After\n";
        let scene = check::drawn(text, &options(), &["Outside", "Inside", "First", "Second", "After"]);
        check::boxes_nest_or_miss(&scene.rects());
    }

    #[test]
    fn all_four_directions_draw_and_two_of_them_are_wider_than_tall() {
        let text = "flowchart DIR\n A[One] --> B[Two] --> C[Three]\n";
        let down = check::drawn(&text.replace("DIR", "TD"), &options(), &["One", "Three"]);
        let across = check::drawn(&text.replace("DIR", "LR"), &options(), &["One", "Three"]);
        assert!(down.size.height > down.size.width, "top to bottom is tall");
        assert!(across.size.width > across.size.height, "left to right is wide");
        check::drawn(&text.replace("DIR", "BT"), &options(), &["One", "Three"]);
        check::drawn(&text.replace("DIR", "RL"), &options(), &["One", "Three"]);
    }

    #[test]
    fn a_node_pointing_at_itself_is_drawn_as_a_loop_beside_it() {
        let text = "flowchart LR\n A[Retry] --> A\n";
        let scene = check::drawn(text, &options(), &["Retry"]);
        assert!(scene.items.iter().any(|item| matches!(item, crate::mermaid::Item::Line { .. })));
    }

    #[test]
    fn an_empty_flowchart_still_draws_something_rather_than_failing() {
        // A person who has typed the first line and nothing else should see a diagram waiting for
        // them, not an error.
        let scene = super::super::render("flowchart TD\n", &options()).expect("it should draw");
        assert!(scene.size.width >= 0.0);
    }

    #[test]
    fn a_shape_that_was_never_closed_says_which_line_it_was_on() {
        let problem = check::refused("flowchart LR\n A --> B\n C[never closed --> D\n", &options());
        assert_eq!(problem.line, Some(3), "the third line is the one at fault");
        assert!(!problem.unsupported, "a mistake in the source is the author's, not Unluminate's");
    }

    #[test]
    fn brackets_unluminate_has_no_shape_for_are_kept_as_the_label() {
        // Being permissive here is deliberate: Mermaid keeps adding shapes, and a file using one
        // Unluminate has not learnt yet should still draw with its words in it rather than refuse.
        let scene = check::drawn("flowchart LR\n A[|weird|] --> B\n", &options(), &["|weird|"]);
        assert!(!scene.is_empty());
    }
}
