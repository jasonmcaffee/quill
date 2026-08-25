//! Sankey diagrams: `sankey` and `sankey-beta`.
//!
//! Nodes in columns by how far along the flow they are, sized by how much passes through them, and
//! joined by ribbons whose width is the value.
//!
//! The source is comma separated values with exactly three columns — source, target, value — with
//! two rules of its own: a value holding a comma is wrapped in quotes, and a pair of quotes inside a
//! quoted value is one quote. **Blank lines are allowed**, which ordinary comma separated values do
//! not permit; Mermaid allows them for spacing and so does this.
//!
//! ## Which column a node goes in
//!
//! The furthest it can be from any source, which is the longest path rather than the shortest. That
//! is what makes every ribbon run forwards: a node that is both fed by the first column and by the
//! third has to sit after the third, or one of its two ribbons would run backwards and the picture
//! would read as though the flow reversed.

use std::collections::HashMap;

use super::parts;
use super::scene::{Anchor, Item, Paint, Point, Rect, Scene};
use super::source::Source;
use super::text;
use super::{Options, Problem};

/// How wide a column's bar is.
const BAR: f32 = 16.0;
/// How far apart two columns are.
const COLUMN_GAP: f32 = 180.0;
/// The gap between two nodes in the same column.
const NODE_GAP: f32 = 12.0;
/// How tall the whole diagram is.
const HEIGHT: f32 = 340.0;

/// One flow.
#[derive(Debug, Clone, PartialEq)]
struct Flow {
    from: usize,
    to: usize,
    value: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Diagram {
    names: Vec<String>,
    by_name: HashMap<String, usize>,
    flows: Vec<Flow>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let diagram = read(source)?;
    Ok(draw(&diagram, options))
}

fn read(source: &Source) -> Result<Diagram, Problem> {
    let mut diagram = Diagram::default();
    for line in source.statements() {
        let fields = split_row(&line.text);
        // A blank row is allowed for spacing, which is what `statements` has already dropped; a row
        // with the wrong number of fields is a real mistake and is worth saying so about.
        if fields.len() != 3 {
            return Err(Problem::at(
                line,
                format!(
                    "a sankey row has exactly three fields — a source, a target and a value — and this one has {}.",
                    fields.len()
                ),
            ));
        }
        let Ok(value) = fields[2].trim().parse::<f32>() else {
            return Err(Problem::at(
                line,
                format!("`{}` is not a number, and a flow needs one.", fields[2].trim()),
            ));
        };
        if value <= 0.0 {
            return Err(Problem::at(line, "a flow's value has to be more than zero"));
        }
        let from = node_of(&mut diagram, &fields[0]);
        let to = node_of(&mut diagram, &fields[1]);
        if from == to {
            return Err(Problem::at(line, "a flow from something to itself has nowhere to go"));
        }
        diagram.flows.push(Flow { from, to, value });
    }
    Ok(diagram)
}

fn node_of(diagram: &mut Diagram, name: &str) -> usize {
    let name = name.trim().to_owned();
    if let Some(&known) = diagram.by_name.get(&name) {
        return known;
    }
    diagram.names.push(name.clone());
    diagram.by_name.insert(name, diagram.names.len() - 1);
    diagram.names.len() - 1
}

/// Split one row, honouring the two quoting rules.
///
/// A comma inside a quoted field is part of the field, and two quotes inside a quoted field are one
/// quote — which is the ordinary comma separated values convention and is what Mermaid follows.
fn split_row(text: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                current.push('"');
                characters.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    fields.push(current);
    fields.into_iter().map(|field| field.trim().to_owned()).collect()
}

/// A node, placed.
struct Placed {
    column: usize,
    /// How much passes through it, which is what its height is worked out from.
    through: f32,
    rect: Rect,
}

fn draw(diagram: &Diagram, options: &Options) -> Scene {
    let mut scene = Scene::new();
    if diagram.flows.is_empty() {
        parts::finish(&mut scene);
        return scene;
    }
    let columns = columns_of(diagram);
    let depth = columns.iter().copied().max().unwrap_or(0) + 1;
    let mut placed: Vec<Placed> = (0..diagram.names.len())
        .map(|index| Placed {
            column: columns[index],
            through: through(diagram, index),
            rect: Rect::default(),
        })
        .collect();

    // The tallest column decides the scale, so no column ever runs off the bottom.
    let tallest = (0..depth)
        .map(|column| {
            let members: Vec<usize> =
                (0..placed.len()).filter(|&index| placed[index].column == column).collect();
            let total: f32 = members.iter().map(|&index| placed[index].through).sum();
            let gaps = NODE_GAP * members.len().saturating_sub(1) as f32;
            (total, gaps)
        })
        .fold((0.0_f32, 0.0_f32), |best, (total, gaps)| {
            if total > best.0 {
                (total, gaps)
            } else {
                best
            }
        });
    let scale = if tallest.0 > 0.0 { (HEIGHT - tallest.1).max(20.0) / tallest.0 } else { 1.0 };

    let name_style = options.style(0.85, false);
    let widest = diagram
        .names
        .iter()
        .map(|name| text::width_of(name, &name_style, options.metrics))
        .fold(0.0_f32, f32::max);
    let width = parts::MARGIN * 2.0 + COLUMN_GAP * (depth - 1) as f32 + BAR + widest + 12.0;

    for column in 0..depth {
        let members: Vec<usize> =
            (0..placed.len()).filter(|&index| placed[index].column == column).collect();
        let mut y = parts::MARGIN;
        for index in members {
            let height = (placed[index].through * scale).max(4.0);
            placed[index].rect = Rect::new(
                parts::MARGIN + COLUMN_GAP * column as f32,
                y,
                BAR,
                height,
            );
            y += height + NODE_GAP;
        }
    }

    draw_ribbons(&mut scene, diagram, &placed, options);
    draw_nodes(&mut scene, diagram, &placed, depth, options);
    scene.claim(Rect::new(0.0, 0.0, width, parts::MARGIN + HEIGHT));
    parts::finish(&mut scene);
    scene
}

/// Which column each node sits in: the furthest it can be from any source.
///
/// Walked until nothing moves, bounded by the number of nodes. A cycle would otherwise push its
/// nodes one column further apart on every pass for ever, so a column is **clamped** to the number
/// of nodes: with a cycle in it the diagram is drawn with one ribbon running backwards, which is
/// what a cyclic flow actually is and is more use than refusing the whole thing.
fn columns_of(diagram: &Diagram) -> Vec<usize> {
    let count = diagram.names.len();
    let furthest = count.saturating_sub(1);
    let mut columns = vec![0_usize; count];
    for _ in 0..count {
        let mut changed = false;
        for flow in &diagram.flows {
            let wanted = (columns[flow.from] + 1).min(furthest);
            if columns[flow.to] < wanted {
                columns[flow.to] = wanted;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    columns
}

/// How much passes through a node: the larger of what goes in and what comes out.
fn through(diagram: &Diagram, index: usize) -> f32 {
    let out: f32 = diagram.flows.iter().filter(|f| f.from == index).map(|f| f.value).sum();
    let into: f32 = diagram.flows.iter().filter(|f| f.to == index).map(|f| f.value).sum();
    out.max(into).max(f32::EPSILON)
}

/// Every ribbon, drawn behind the bars.
fn draw_ribbons(scene: &mut Scene, diagram: &Diagram, placed: &[Placed], options: &Options) {
    // How far down each node's own bar the next ribbon starts, kept separately for each side.
    let mut leaving = vec![0.0_f32; placed.len()];
    let mut arriving = vec![0.0_f32; placed.len()];
    for (index, flow) in diagram.flows.iter().enumerate() {
        let from = &placed[flow.from];
        let to = &placed[flow.to];
        let thickness = (flow.value / through(diagram, flow.from) * from.rect.height).max(2.0);
        let landing = (flow.value / through(diagram, flow.to) * to.rect.height).max(2.0);
        let start = from.rect.top() + leaving[flow.from];
        let finish = to.rect.top() + arriving[flow.to];
        leaving[flow.from] += thickness;
        arriving[flow.to] += landing;
        // The ribbon is a four-sided shape: along the source's bar, across, down the target's bar,
        // and back. Drawn straight rather than curved, which is what keeps the scene to five kinds
        // of item and is perfectly readable at this width.
        scene.add(Item::Polygon {
            points: vec![
                Point::new(from.rect.right(), start),
                Point::new(to.rect.left(), finish),
                Point::new(to.rect.left(), finish + landing),
                Point::new(from.rect.right(), start + thickness),
            ],
            fill: Some(options.theme.wash(index, 90)),
            stroke: None,
        });
    }
}

/// Every bar, and its name beside it.
fn draw_nodes(
    scene: &mut Scene,
    diagram: &Diagram,
    placed: &[Placed],
    depth: usize,
    options: &Options,
) {
    let style = parts::text_style(options, 0.85, false, options.theme.text);
    let measure = options.style(0.85, false);
    for (index, name) in diagram.names.iter().enumerate() {
        let rect = placed[index].rect;
        scene.add(Item::Rect {
            rect,
            radius: 2.0,
            fill: Some(Paint::solid(options.theme.series(index))),
            stroke: None,
        });
        let width = text::width_of(name, &measure, options.metrics);
        // The name goes to the right of the bar, except in the last column where there is no room.
        let last_column = placed[index].column + 1 == depth;
        let (at, anchor) = if last_column {
            (Point::new(rect.left() - 8.0, rect.centre().y - measure.size * 0.6), Anchor::End)
        } else {
            (Point::new(rect.right() + 8.0, rect.centre().y - measure.size * 0.6), Anchor::Start)
        };
        // On a panel. A ribbon is a wash of colour behind the name, and words over one are hard to
        // read whatever colour they are drawn in.
        let left = if last_column { at.x - width - 4.0 } else { at.x - 4.0 };
        scene.add(Item::Rect {
            rect: Rect::new(left, at.y - 2.0, width + 8.0, measure.size * 1.5),
            radius: 3.0,
            fill: Some(Paint::solid(options.theme.node_fill.color)),
            stroke: None,
        });
        parts::one_line(scene, name, at, &style, anchor, width);
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
    fn three_columns_become_a_flow() {
        let diagram = diagram("sankey-beta\n Coal,Electricity,25\n Gas,Electricity,15\n");
        assert_eq!(diagram.names, vec!["Coal", "Electricity", "Gas"]);
        assert_eq!(diagram.flows.len(), 2);
        assert_eq!(diagram.flows[0].value, 25.0);
    }

    #[test]
    fn a_comma_inside_quotes_is_part_of_the_name() {
        let diagram = diagram("sankey-beta\n \"Coal, imported\",Electricity,25\n");
        assert_eq!(diagram.names[0], "Coal, imported");
        assert_eq!(diagram.flows[0].value, 25.0);
    }

    #[test]
    fn two_quotes_inside_a_quoted_field_are_one_quote() {
        let diagram = diagram("sankey-beta\n \"The \"\"good\"\" stuff\",Out,1\n");
        assert_eq!(diagram.names[0], "The \"good\" stuff");
    }

    #[test]
    fn blank_lines_are_allowed_for_spacing() {
        let diagram = diagram("sankey-beta\n A,B,1\n\n\n C,D,2\n");
        assert_eq!(diagram.flows.len(), 2);
    }

    #[test]
    fn a_row_with_the_wrong_number_of_fields_says_how_many_it_had() {
        let problem = check::refused("sankey-beta\n A,B,1,extra\n", &options());
        assert_eq!(problem.line, Some(2));
        assert!(problem.reason.contains('4'), "{}", problem.reason);
    }

    #[test]
    fn a_flow_to_itself_is_refused_rather_than_drawn_as_nothing() {
        assert!(check::refused("sankey-beta\n A,A,5\n", &options()).line.is_some());
    }

    #[test]
    fn a_node_sits_as_far_along_as_anything_feeding_it() {
        // The longest path, not the shortest: otherwise one of this node's two ribbons would run
        // backwards and the picture would read as though the flow reversed.
        let diagram = diagram("sankey-beta\n A,B,1\n B,C,1\n A,C,1\n");
        let columns = columns_of(&diagram);
        assert_eq!(columns[diagram.by_name["A"]], 0);
        assert_eq!(columns[diagram.by_name["B"]], 1);
        assert_eq!(columns[diagram.by_name["C"]], 2, "C is after B, not beside it");
    }

    #[test]
    fn a_source_with_a_cycle_in_it_still_stops() {
        let diagram = diagram("sankey-beta\n A,B,1\n B,A,1\n");
        let columns = columns_of(&diagram);
        assert!(
            columns.iter().all(|column| *column < diagram.names.len()),
            "a cycle must not push the columns apart for ever: {columns:?}"
        );
    }

    #[test]
    fn a_sankey_diagram_is_drawn_and_keeps_every_property() {
        let text = "sankey-beta\n\
            Coal,Electricity,25\n Gas,Electricity,18\n Wind,Electricity,12\n\
            Electricity,Homes,30\n Electricity,Industry,25\n";
        let scene = check::drawn(
            text,
            &options(),
            &["Coal", "Gas", "Wind", "Electricity", "Homes", "Industry"],
        );
        assert!(scene.size.width > 300.0);
    }

    #[test]
    fn a_bigger_flow_makes_a_taller_bar() {
        let scene = check::drawn("sankey-beta\n Small,Out,1\n Large,Out,9\n", &options(), &["Small", "Large"]);
        let bars: Vec<Rect> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        // Small, Out, Large — in the order the names were first seen.
        assert!(bars[2].height > bars[0].height * 4.0, "nine is much taller than one");
    }
}
