//! Treemaps: `treemap` and `treemap-beta`.
//!
//! Nested rectangles sized by value. Indentation says the hierarchy, exactly as it does in a
//! mindmap; a node with a value after it is a leaf and one without is a section holding others.
//!
//! ## Squarified, not sliced
//!
//! The naive treemap slices each level alternately across and down, which gives long thin slivers
//! whose areas nobody can compare — the whole point of the picture is lost. This uses Bruls, Huizing
//! and van Wijk's **squarified** layout: take the next item, and keep adding items to the current row
//! while doing so improves the worst aspect ratio in it; when it stops improving, close the row and
//! start another. The result is rectangles near enough to square that their areas can be read.
//!
//! A section's own value is the total of what is inside it, so a section with no leaves under it has
//! nothing to draw and is left out rather than drawn as a sliver.

use super::parts;
use super::scene::{Anchor, Item, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How big the whole picture is.
const ACROSS: f32 = 520.0;
const DOWN: f32 = 340.0;
/// How much room a section's title takes at the top of its rectangle.
const TITLE: f32 = 20.0;
/// The gap between one rectangle and the next.
const GAP: f32 = 3.0;

/// One node of the tree.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    label: String,
    /// A leaf's own value. A section's is worked out from its children.
    value: Option<f32>,
    children: Vec<usize>,
    depth: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Tree {
    nodes: Vec<Node>,
    roots: Vec<usize>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let tree = read(source)?;
    Ok(draw(&tree, source, options))
}

fn read(source: &Source) -> Result<Tree, Problem> {
    let mut tree = Tree::default();
    let mut open: Vec<(usize, usize)> = Vec::new();
    for line in source.statements() {
        let text = line.text.trim();
        if text.starts_with("classDef") || text.starts_with(":::") {
            continue;
        }
        // `"Leaf": 12` — the value comes after the last colon that is outside the quotes.
        let (label, value) = match split_value(text) {
            Some((label, value)) => {
                let Ok(value) = value.trim().parse::<f32>() else {
                    return Err(Problem::at(
                        line,
                        format!("`{}` is not a number, and a leaf's value has to be one.", value.trim()),
                    ));
                };
                if value < 0.0 {
                    return Err(Problem::at(line, "a leaf's value cannot be less than zero"));
                }
                (label, Some(value))
            }
            None => (text, None),
        };
        let label = source::label(strip_class(label));
        if label.is_empty() {
            continue;
        }
        while open.last().is_some_and(|(indent, _)| *indent >= line.indent) {
            open.pop();
        }
        let parent = open.last().map(|(_, node)| *node);
        let index = tree.nodes.len();
        tree.nodes.push(Node { label, value, children: Vec::new(), depth: open.len() });
        match parent {
            Some(parent) => tree.nodes[parent].children.push(index),
            None => tree.roots.push(index),
        }
        open.push((line.indent, index));
    }
    Ok(tree)
}

/// Split a row into its label and its value, on the colon after the closing quote.
fn split_value(text: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (at, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some((&text[..at], &text[at + 1..])),
            _ => {}
        }
    }
    None
}

/// `"Leaf":::className` — the class is read and ignored.
fn strip_class(label: &str) -> &str {
    match label.find(":::") {
        Some(at) => label[..at].trim(),
        None => label.trim(),
    }
}

fn draw(tree: &Tree, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    if tree.nodes.is_empty() {
        parts::title(&mut scene, source, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let width = parts::MARGIN * 2.0 + ACROSS;
    let top = parts::title(&mut scene, source, options, width);
    let whole = Rect::new(parts::MARGIN, top + parts::MARGIN, ACROSS, DOWN);
    let totals: Vec<f32> = (0..tree.nodes.len()).map(|index| total(tree, index)).collect();
    lay_out(&mut scene, tree, &totals, &tree.roots, whole, options);
    scene.claim(Rect::new(0.0, 0.0, width, whole.bottom()));
    parts::finish(&mut scene);
    scene
}

/// A node's value: its own, or the total of everything under it.
fn total(tree: &Tree, index: usize) -> f32 {
    if let Some(value) = tree.nodes[index].value {
        return value;
    }
    tree.nodes[index].children.iter().map(|&child| total(tree, child)).sum()
}

/// Place a list of nodes inside `area`, and then everything inside each of them.
fn lay_out(
    scene: &mut Scene,
    tree: &Tree,
    totals: &[f32],
    nodes: &[usize],
    area: Rect,
    options: &Options,
) {
    // A node worth nothing has no area to be drawn in, so it is left out rather than drawn as a
    // sliver nobody can read.
    let mut worth: Vec<usize> = nodes.iter().copied().filter(|&index| totals[index] > 0.0).collect();
    worth.sort_by(|a, b| totals[*b].total_cmp(&totals[*a]));
    if worth.is_empty() || area.width <= 1.0 || area.height <= 1.0 {
        return;
    }
    for (index, rect) in squarify(&worth, totals, area) {
        let node = &tree.nodes[index];
        let colour = options.theme.series(node.depth);
        scene.add(Item::Rect {
            rect,
            radius: 3.0,
            fill: Some(options.theme.wash(node.depth, if node.children.is_empty() { 110 } else { 44 })),
            stroke: Some(Stroke::new(colour, parts::LINE)),
        });
        if node.children.is_empty() {
            draw_leaf(scene, node, totals[index], rect, options);
            continue;
        }
        draw_section_title(scene, &node.label, rect, options);
        let inside = Rect::new(
            rect.x + GAP,
            rect.y + TITLE,
            (rect.width - GAP * 2.0).max(0.0),
            (rect.height - TITLE - GAP).max(0.0),
        );
        lay_out(scene, tree, totals, &node.children, inside, options);
    }
}

/// A leaf's name and its value, when there is room for them.
fn draw_leaf(scene: &mut Scene, node: &Node, value: f32, rect: Rect, options: &Options) {
    let style = options.style(0.8, false);
    if rect.height < style.size * 2.0 || rect.width < 30.0 {
        return;
    }
    let label = text::measure(&node.label, &style, options.metrics, rect.width - 8.0);
    parts::label_at(
        scene,
        &label,
        Point::new(rect.left() + 5.0, rect.top() + 4.0),
        &parts::text_style(options, 0.8, false, options.theme.text),
        Anchor::Start,
    );
    // Measured against what the label actually took, not a guess: a label that wrapped to three
    // lines leaves no room for a value under it, and drawing one anyway ran it off the bottom.
    if rect.height < label.height + style.size * 1.6 + 8.0 {
        return;
    }
    let words = super::pie::format_number(value);
    let width = text::width_of(&words, &style, options.metrics);
    parts::one_line(
        scene,
        &words,
        Point::new(rect.left() + 5.0, rect.top() + 4.0 + label.height),
        &parts::text_style(options, 0.8, false, options.theme.dim),
        Anchor::Start,
        width,
    );
}

/// A section's name across the top of its rectangle.
fn draw_section_title(scene: &mut Scene, label: &str, rect: Rect, options: &Options) {
    if rect.width < 40.0 || rect.height < TITLE + 8.0 {
        return;
    }
    let style = options.style(0.82, true);
    let width = text::width_of(label, &style, options.metrics);
    parts::one_line(
        scene,
        label,
        Point::new(rect.left() + 5.0, rect.top() + 3.0),
        &parts::text_style(options, 0.82, true, options.theme.text),
        Anchor::Start,
        width,
    );
}

/// The squarified treemap: rows of rectangles whose aspect ratios are kept as near to one as the
/// values allow.
fn squarify(nodes: &[usize], totals: &[f32], area: Rect) -> Vec<(usize, Rect)> {
    let mut out = Vec::with_capacity(nodes.len());
    let sum: f32 = nodes.iter().map(|&index| totals[index]).sum();
    if sum <= 0.0 {
        return out;
    }
    // Values are turned into areas once, so the arithmetic below is all in points.
    let scale = (area.width * area.height) / sum;
    let mut remaining: Vec<usize> = nodes.to_vec();
    let mut free = area;
    while !remaining.is_empty() {
        let short = free.width.min(free.height);
        let mut row: Vec<usize> = Vec::new();
        let mut row_area = 0.0_f32;
        // Keep adding while the worst rectangle in the row gets better, which is the whole of the
        // method: the moment it stops improving, this row is as square as it is going to get.
        while let Some(&next) = remaining.first() {
            let next_area = totals[next] * scale;
            let with = worst(&row, totals, scale, row_area + next_area, short, Some(next_area));
            let without = worst(&row, totals, scale, row_area, short, None);
            if !row.is_empty() && with > without {
                break;
            }
            row.push(next);
            row_area += next_area;
            remaining.remove(0);
        }
        free = place_row(&mut out, &row, totals, scale, row_area, free);
    }
    out
}

/// The worst aspect ratio in a row, which is what the method is minimising.
fn worst(
    row: &[usize],
    totals: &[f32],
    scale: f32,
    row_area: f32,
    short: f32,
    extra: Option<f32>,
) -> f32 {
    if row_area <= 0.0 || short <= 0.0 {
        return f32::INFINITY;
    }
    let mut smallest = f32::INFINITY;
    let mut largest = 0.0_f32;
    for area in row.iter().map(|&index| totals[index] * scale).chain(extra) {
        smallest = smallest.min(area);
        largest = largest.max(area);
    }
    if smallest <= 0.0 {
        return f32::INFINITY;
    }
    // Written the way the paper writes it, which is far easier to check than the equivalent algebra
    // in terms of the row's thickness:
    //   max(short² × largest / rowArea², rowArea² / (short² × smallest))
    let first = (short * short * largest) / (row_area * row_area);
    let second = (row_area * row_area) / (short * short * smallest);
    first.max(second)
}

/// Put one row along the short side of the free space, and hand back what is left.
fn place_row(
    out: &mut Vec<(usize, Rect)>,
    row: &[usize],
    totals: &[f32],
    scale: f32,
    row_area: f32,
    free: Rect,
) -> Rect {
    if row.is_empty() || row_area <= 0.0 {
        return Rect::new(free.x, free.y, 0.0, 0.0);
    }
    let along_the_top = free.width <= free.height;
    let thickness = if along_the_top {
        (row_area / free.width).min(free.height)
    } else {
        (row_area / free.height).min(free.width)
    };
    let mut at = 0.0;
    for &index in row {
        let share = totals[index] * scale / row_area;
        let rect = if along_the_top {
            Rect::new(free.x + at, free.y, free.width * share, thickness)
        } else {
            Rect::new(free.x, free.y + at, thickness, free.height * share)
        };
        at += if along_the_top { rect.width } else { rect.height };
        // The gap is taken off the inside, so two neighbours never touch and the tree reads.
        out.push((
            index,
            Rect::new(
                rect.x,
                rect.y,
                (rect.width - GAP).max(1.0),
                (rect.height - GAP).max(1.0),
            ),
        ));
    }
    if along_the_top {
        Rect::new(free.x, free.y + thickness, free.width, (free.height - thickness).max(0.0))
    } else {
        Rect::new(free.x + thickness, free.y, (free.width - thickness).max(0.0), free.height)
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

    fn tree(text: &str) -> Tree {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn indentation_builds_the_tree_and_a_value_makes_a_leaf() {
        let text = "treemap-beta\n\"Section 1\"\n    \"Leaf 1.1\": 12\n    \"Section 1.2\"\n      \"Leaf\": 8\n\"Section 2\"\n    \"Leaf 2.1\": 20\n";
        let tree = tree(text);
        assert_eq!(tree.roots.len(), 2);
        assert_eq!(tree.nodes[0].label, "Section 1");
        assert_eq!(tree.nodes[0].value, None, "a section has no value of its own");
        assert_eq!(tree.nodes[1].label, "Leaf 1.1");
        assert_eq!(tree.nodes[1].value, Some(12.0));
        assert_eq!(tree.nodes[2].children.len(), 1);
    }

    #[test]
    fn a_sections_value_is_the_total_of_what_is_inside_it() {
        let tree = tree("treemap-beta\n\"S\"\n  \"a\": 3\n  \"b\": 7\n");
        assert_eq!(total(&tree, 0), 10.0);
    }

    #[test]
    fn a_value_that_is_not_a_number_says_which_line() {
        let problem = check::refused("treemap-beta\n\"S\"\n  \"a\": lots\n", &options());
        assert_eq!(problem.line, Some(3));
    }

    #[test]
    fn a_colon_inside_a_label_is_not_the_value_separator() {
        let tree = tree("treemap-beta\n\"Time: the whole of it\": 5\n");
        assert_eq!(tree.nodes[0].label, "Time: the whole of it");
        assert_eq!(tree.nodes[0].value, Some(5.0));
    }

    #[test]
    fn a_treemap_is_drawn_and_keeps_every_property() {
        let text = "treemap-beta\n\
            \"Editing\"\n  \"Undo\": 30\n  \"Formatting\": 18\n  \"Search\": 12\n\
            \"Panes\"\n  \"Explorer\": 22\n  \"Terminal\": 26\n  \"Git\": 16\n\
            \"Plugins\": 14\n";
        check::drawn(
            text,
            &options(),
            &["Editing", "Undo", "Panes", "Terminal", "Plugins"],
        );
    }

    #[test]
    fn a_bigger_value_gets_a_bigger_rectangle() {
        let scene = check::drawn("treemap-beta\n\"a\": 90\n\"b\": 10\n", &options(), &["a", "b"]);
        let rects = scene.rects();
        assert_eq!(rects.len(), 2);
        let area = |rect: &Rect| rect.width * rect.height;
        assert!(area(&rects[0]) > area(&rects[1]) * 4.0, "ninety takes far more room than ten");
    }

    #[test]
    fn the_rectangles_are_squarish_rather_than_slivers() {
        // The whole point of squarifying. Four equal values in a square area should each come out
        // near enough to square that their areas can be compared by eye.
        let text = "treemap-beta\n\"a\": 25\n\"b\": 25\n\"c\": 25\n\"d\": 25\n";
        let scene = check::drawn(text, &options(), &["a", "d"]);
        for rect in scene.rects() {
            let ratio = (rect.width / rect.height).max(rect.height / rect.width);
            assert!(ratio < 3.0, "{rect:?} is a sliver, ratio {ratio}");
        }
    }

    #[test]
    fn a_leaf_worth_nothing_is_left_out_rather_than_drawn_as_a_sliver() {
        let scene = check::drawn("treemap-beta\n\"a\": 10\n\"nothing\": 0\n", &options(), &["a"]);
        assert_eq!(scene.rects().len(), 1);
    }

    #[test]
    fn nested_rectangles_nest_rather_than_half_overlapping() {
        let text = "treemap-beta\n\"S\"\n  \"a\": 10\n  \"b\": 10\n\"T\": 20\n";
        let scene = check::drawn(text, &options(), &["S", "a", "b", "T"]);
        check::boxes_nest_or_miss(&scene.rects());
    }
}
