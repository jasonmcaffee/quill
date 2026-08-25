//! Mindmaps: `mindmap`.
//!
//! A tree growing to the right from its root, in the six node shapes Mermaid gives it.
//!
//! ## Indentation says the shape of the tree, but not by how much
//!
//! Mermaid's own rule is that only the **comparison** with the previous line matters, never the
//! absolute amount: a file indented by two spaces and the same file indented by four are the same
//! mindmap. So the parser keeps a stack of the indents it has seen. More indented than the last line
//! means a child; less means walk back up the stack until an indent that is smaller is found. That
//! also makes a file with a stray extra space still read correctly, which one written by hand very
//! often has.
//!
//! ## A subtree takes exactly the room its leaves need
//!
//! Each node's height is the total height of its children, or one row when it has none, and it is
//! centred against them. That is what stops two branches of a wide mindmap overlapping, and it is
//! one pass up the tree followed by one pass down.

use super::parts;
use super::scene::{Dash, Item, Point, Rect, Scene, Size, Stroke};
use super::shapes::Shape;
use super::source::{self, Source};
use super::text::{self, Label};
use super::{Options, Problem};

/// How far apart one level of the tree is from the next.
const LEVEL_GAP: f32 = 46.0;
/// The least room one leaf takes down the page.
const ROW: f32 = 40.0;
/// How wide a node's words may get before they wrap.
const WRAP: f32 = 150.0;

/// One node of the tree.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    label: String,
    shape: Shape,
    children: Vec<usize>,
    depth: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Map {
    nodes: Vec<Node>,
    /// The nodes with no parent. There is normally one, and a file may have several.
    roots: Vec<usize>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let map = read(source)?;
    Ok(draw(&map, source, options))
}

fn read(source: &Source) -> Result<Map, Problem> {
    let mut map = Map::default();
    // The indent of each open ancestor, and which node it is.
    let mut open: Vec<(usize, usize)> = Vec::new();
    for line in source.statements() {
        let text = line.text.trim();
        // An icon or a class is about how a node looks, which a document does not decide here.
        if text.starts_with("::icon") || text.starts_with(":::") {
            continue;
        }
        let (label, shape) = read_node(text);
        while open.last().is_some_and(|(indent, _)| *indent >= line.indent) {
            open.pop();
        }
        let parent = open.last().map(|(_, node)| *node);
        let index = map.nodes.len();
        map.nodes.push(Node {
            label,
            shape,
            children: Vec::new(),
            depth: open.len(),
        });
        match parent {
            Some(parent) => map.nodes[parent].children.push(index),
            None => map.roots.push(index),
        }
        open.push((line.indent, index));
    }
    Ok(map)
}

/// Read one node: its words and, from the brackets round them, its shape.
///
/// Longest opener first, so `))text((` is never read as `)text(` with stray brackets. A node with no
/// brackets at all is the ordinary case and is drawn as a rounded box.
fn read_node(text: &str) -> (String, Shape) {
    const FORMS: &[(&str, &str, Shape)] = &[
        ("))", "((", Shape::Bang),
        ("((", "))", Shape::Circle),
        ("{{", "}}", Shape::Hexagon),
        ("[", "]", Shape::Rect),
        (")", "(", Shape::Cloud),
        ("(", ")", Shape::Round),
    ];
    // The brackets come after the identifier: `id[the words]`.
    for (open, close, shape) in FORMS {
        let Some(at) = text.find(open) else {
            continue;
        };
        if !text.trim_end().ends_with(close) {
            continue;
        }
        let inner = &text[at + open.len()..text.trim_end().len() - close.len()];
        return (source::label(inner), *shape);
    }
    (source::label(text), Shape::Round)
}

/// One node, measured and given the room its subtree needs.
struct Placed {
    label: Label,
    size: Size,
    /// How much room this node and everything under it takes down the page.
    span: f32,
    rect: Rect,
}

fn draw(map: &Map, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    if map.nodes.is_empty() {
        parts::title(&mut scene, source, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let mut placed: Vec<Placed> = map
        .nodes
        .iter()
        .map(|node| {
            let style = options.style(size_scale(node.depth), node.depth == 0);
            let label = text::measure(&node.label, &style, options.metrics, WRAP);
            let size = node.shape.size_for(label.size());
            Placed { label, size, span: 0.0, rect: Rect::default() }
        })
        .collect();

    // Up the tree: how much room each subtree needs.
    for &root in &map.roots {
        measure_span(map, &mut placed, root);
    }
    // How wide each level is, so the columns line up rather than being ragged.
    let depth = map.nodes.iter().map(|node| node.depth).max().unwrap_or(0) + 1;
    let mut widths = vec![0.0_f32; depth];
    for (index, node) in map.nodes.iter().enumerate() {
        widths[node.depth] = widths[node.depth].max(placed[index].size.width);
    }
    let mut lefts = Vec::with_capacity(depth);
    let mut at = parts::MARGIN;
    for width in &widths {
        lefts.push(at);
        at += width + LEVEL_GAP;
    }
    let width = at - LEVEL_GAP + parts::MARGIN;

    let top = parts::title(&mut scene, source, options, width);
    // Down the tree: where each node actually goes.
    let mut y = top + parts::MARGIN;
    for &root in &map.roots {
        place(map, &mut placed, root, &lefts, &widths, y);
        y += placed[root].span;
    }

    draw_links(&mut scene, map, &placed, options);
    for (index, node) in map.nodes.iter().enumerate() {
        let theme = &options.theme;
        node.shape.draw(
            &mut scene,
            placed[index].rect,
            theme.wash(node.depth, 70),
            Stroke::new(theme.series(node.depth), parts::LINE),
        );
        parts::centred_label(
            &mut scene,
            &placed[index].label,
            placed[index].rect,
            &parts::text_style(options, size_scale(node.depth), node.depth == 0, theme.text),
        );
    }
    scene.claim(Rect::new(0.0, 0.0, width, y));
    parts::finish(&mut scene);
    scene
}

/// How much bigger a node at this depth is than an ordinary one. The root is the biggest.
fn size_scale(depth: usize) -> f32 {
    match depth {
        0 => 1.3,
        1 => 1.05,
        _ => 0.92,
    }
}

/// Work out how much room a subtree needs, from the leaves upwards.
fn measure_span(map: &Map, placed: &mut [Placed], index: usize) -> f32 {
    let children = map.nodes[index].children.clone();
    let span = if children.is_empty() {
        placed[index].size.height.max(ROW)
    } else {
        children.iter().map(|&child| measure_span(map, placed, child)).sum::<f32>()
    };
    placed[index].span = span.max(placed[index].size.height);
    placed[index].span
}

/// Give every node in a subtree its rectangle, from the root down.
fn place(
    map: &Map,
    placed: &mut [Placed],
    index: usize,
    lefts: &[f32],
    widths: &[f32],
    top: f32,
) {
    let depth = map.nodes[index].depth;
    let size = placed[index].size;
    // The node is centred against the room its whole subtree takes, which is what keeps a parent
    // opposite the middle of its children rather than opposite the first of them.
    placed[index].rect = Rect::new(
        lefts[depth] + (widths[depth] - size.width) / 2.0,
        top + (placed[index].span - size.height) / 2.0,
        size.width,
        size.height,
    );
    let mut y = top;
    for child in map.nodes[index].children.clone() {
        place(map, placed, child, lefts, widths, y);
        y += placed[child].span;
    }
}

/// The lines joining a parent to each of its children.
fn draw_links(scene: &mut Scene, map: &Map, placed: &[Placed], options: &Options) {
    for (index, node) in map.nodes.iter().enumerate() {
        let from = placed[index].rect;
        for &child in &node.children {
            let to = placed[child].rect;
            let start = Point::new(from.right(), from.centre().y);
            let finish = Point::new(to.left(), to.centre().y);
            // Bent at the halfway point rather than drawn straight, so the branches of a wide
            // mindmap read as a tree rather than as a fan of diagonals.
            let middle = (start.x + finish.x) / 2.0;
            scene.add(Item::Line {
                points: vec![
                    start,
                    Point::new(middle, start.y),
                    Point::new(middle, finish.y),
                    finish,
                ],
                stroke: Stroke::new(options.theme.series(node.depth), parts::LINE),
                dash: Dash::Solid,
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

    fn map(text: &str) -> Map {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn indentation_builds_the_tree() {
        let text = "mindmap\nRoot\n  A\n    B\n    C\n  D\n";
        let map = map(text);
        assert_eq!(map.roots, vec![0]);
        assert_eq!(map.nodes[0].label, "Root");
        assert_eq!(map.nodes[0].children, vec![1, 4]);
        assert_eq!(map.nodes[1].children, vec![2, 3], "B and C hang off A");
        assert_eq!(map.nodes[4].label, "D");
    }

    #[test]
    fn only_the_comparison_with_the_line_above_matters() {
        // The same mindmap indented two ways has to come out the same, which is Mermaid's own rule.
        let two = map("mindmap\nRoot\n  A\n    B\n");
        let four = map("mindmap\nRoot\n    A\n        B\n");
        let odd = map("mindmap\nRoot\n   A\n     B\n");
        for read in [&two, &four, &odd] {
            assert_eq!(read.nodes[0].children, vec![1]);
            assert_eq!(read.nodes[1].children, vec![2]);
            assert_eq!(read.nodes[2].depth, 2);
        }
    }

    #[test]
    fn coming_back_out_two_levels_finds_the_right_parent() {
        let text = "mindmap\nRoot\n  A\n    B\n      C\n  D\n";
        let map = map(text);
        assert_eq!(map.nodes[0].children, vec![1, 4], "D goes back to the root");
        assert_eq!(map.nodes[4].depth, 1);
    }

    #[test]
    fn all_six_node_shapes_are_read() {
        let text = "mindmap\nroot\n  a[square]\n  b(rounded)\n  c((circle))\n  d))bang((\n  e)cloud(\n  f{{hexagon}}\n";
        let map = map(text);
        let shapes: Vec<Shape> = map.nodes[1..].iter().map(|node| node.shape).collect();
        assert_eq!(
            shapes,
            vec![Shape::Rect, Shape::Round, Shape::Circle, Shape::Bang, Shape::Cloud, Shape::Hexagon]
        );
        assert_eq!(map.nodes[1].label, "square");
        assert_eq!(map.nodes[4].label, "bang");
    }

    #[test]
    fn an_icon_or_a_class_is_not_a_node() {
        let map = map("mindmap\nRoot\n  A\n  ::icon(fa fa-book)\n  :::urgent\n  B\n");
        assert_eq!(map.nodes.len(), 3, "only Root, A and B");
    }

    #[test]
    fn a_mindmap_is_drawn_and_keeps_every_property() {
        let text = "mindmap\n\
            root((Quill))\n\
              Editing\n    Undo\n    Formatting\n\
              Panes\n    Explorer\n    Terminal\n    Git\n\
              Plugins\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Quill", "Editing", "Undo", "Formatting", "Panes", "Explorer", "Terminal", "Git", "Plugins"],
        );
        assert!(scene.size.width > 300.0);
    }

    #[test]
    fn two_branches_of_a_wide_mindmap_never_sit_on_top_of_each_other() {
        // Every node takes exactly the room its leaves need, so this holds by construction rather
        // than by the numbers happening to work out.
        let text = "mindmap\nroot\n  A\n    a1\n    a2\n    a3\n  B\n    b1\n    b2\n  C\n    c1\n";
        let scene = check::drawn(text, &options(), &["A", "a1", "b2", "c1"]);
        check::no_two_rectangles_overlap(&scene.rects());
    }

    #[test]
    fn a_parent_sits_opposite_the_middle_of_its_children() {
        let text = "mindmap\nroot\n  A\n    one\n    two\n    three\n";
        let scene = check::drawn(text, &options(), &["A", "one", "three"]);
        let boxes = scene.rects();
        // root, A, one, two, three — in the order they were declared.
        let a = boxes[1];
        let (first, last) = (boxes[2], boxes[4]);
        let middle = (first.centre().y + last.centre().y) / 2.0;
        assert!((a.centre().y - middle).abs() < 1.0, "A is opposite the middle of its three children");
    }
}
