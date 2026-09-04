//! The layered graph layout, shared by five diagram types.
//!
//! A flowchart, a class diagram, a state diagram, an ER diagram and a requirement diagram are the
//! same problem underneath: boxes joined by labelled arrows, drawn in a direction. So the layout is
//! written once, here, and each of those five is left with only its own grammar and its own shapes
//! to worry about.
//!
//! The method is Sugiyama's, which is what `dagre` does and so what Mermaid's own pictures look
//! like:
//!
//! 1. **Break the cycles.** A depth-first walk; an edge pointing back at a node already on the stack
//!    is reversed for the purpose of layering and remembered, so it is still *drawn* the way it was
//!    written.
//! 2. **Rank.** Longest path down the resulting acyclic graph. A node sits below the lowest of its
//!    parents.
//! 3. **Fill in the gaps.** An edge spanning more than one rank gets a chain of dummy nodes, one a
//!    rank. Those are what it is later routed through, which is what stops a long edge cutting
//!    across a box that happens to be in the way.
//! 4. **Order within a rank**, by repeatedly moving each node to the median position of its
//!    neighbours and keeping the result only when it crosses fewer edges.
//! 5. **Place**, then **route**.
//!
//! ## Two rules this keeps, and why they matter more here than usual
//!
//! **No randomness, and a fixed number of passes.** Every sweep count in this file is a constant. A
//! layout that improved itself until it stopped improving would give a different picture for the
//! same source depending on how the floating point rounded, and every screenshot test of a diagram
//! would be noise. It also makes the cost O(n) passes whatever the input, which is what lets a
//! preview lay a diagram out on a keystroke.
//!
//! **A subgraph is laid out on its own and placed as one box.** Its contents cannot then overlap
//! anything outside it, which is the failure a single flat layout with a frame drawn round some of
//! the nodes always ends in. Edges that cross the frame are routed at the end, between the real
//! nodes' final positions, so they still point at the box they name rather than at the frame.

use std::collections::HashMap;

use super::scene::{Point, Rect, Size};

/// How far apart two nodes in the same rank are.
const NODE_GAP: f32 = 34.0;
/// How far apart one rank is from the next, before any edge label is allowed for.
const RANK_GAP: f32 = 54.0;
/// How wide a dummy node is: the lane an edge passing a rank takes for itself.
const LANE: f32 = 14.0;
/// Space between a subgraph's frame and what is inside it.
const GROUP_PADDING: f32 = 20.0;
/// How many times the ordering is swept up and down. Four is where the improvement stops being
/// visible on the diagrams in `sample/mermaid`, measured by counting crossings.
const ORDER_SWEEPS: usize = 4;
/// How many times the positions are relaxed towards the median of each node's neighbours.
const POSITION_SWEEPS: usize = 6;

/// Which way the diagram flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    /// Read Mermaid's own spelling: `TB`, `TD`, `BT`, `LR`, `RL`.
    pub fn parse(text: &str) -> Option<Direction> {
        match text.trim().to_ascii_uppercase().as_str() {
            "TB" | "TD" | "V" => Some(Direction::Down),
            "BT" => Some(Direction::Up),
            "LR" => Some(Direction::Right),
            "RL" => Some(Direction::Left),
            _ => None,
        }
    }

    /// True when ranks run across the page rather than down it.
    fn is_horizontal(self) -> bool {
        matches!(self, Direction::Right | Direction::Left)
    }
}

/// A node to place. Its identity is its position in [`Graph::nodes`].
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    pub size: Size,
    /// The group it is directly inside, if any.
    pub group: Option<usize>,
}

/// An edge to route.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSpec {
    pub from: usize,
    pub to: usize,
    /// How much room its label needs, so the rank gap can be widened to hold it.
    pub label: Size,
    /// The fewest ranks it must span. Mermaid's extra dashes ask for more than one.
    pub span: usize,
}

impl EdgeSpec {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to, label: Size::default(), span: 1 }
    }
}

/// A subgraph: a frame drawn round some nodes, with a title.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSpec {
    /// How much room the title takes, which is added to the top of the frame.
    pub title: Size,
    /// The group this one is directly inside, if any.
    pub parent: Option<usize>,
}

/// Everything to be laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Graph {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    pub groups: Vec<GroupSpec>,
    pub direction: Direction,
}

impl Graph {
    pub fn add_node(&mut self, size: Size, group: Option<usize>) -> usize {
        self.nodes.push(NodeSpec { size, group });
        self.nodes.len() - 1
    }
}

/// Where everything went.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placed {
    /// One rectangle per node, in the order they were given.
    pub nodes: Vec<Rect>,
    /// One frame per group, in the order they were given.
    pub groups: Vec<Rect>,
    /// One polyline per edge, from the centre of its source to the centre of its target, through
    /// whatever bends the layout gave it. Trimming each end back to the shape's own border is the
    /// caller's, because only the caller knows what shape it drew.
    pub edges: Vec<Vec<Point>>,
    /// Where each edge's label goes: the middle of its middle segment.
    pub labels: Vec<Point>,
    pub size: Size,
}

/// Lay `graph` out.
pub fn layout(graph: &Graph) -> Placed {
    if graph.nodes.is_empty() {
        return Placed::default();
    }
    // Everything is worked out as though the diagram ran downwards, and turned at the end. For a
    // left-to-right diagram the node sizes go in turned as well, because in that layout a node's
    // height is what takes room along a rank; turning the result back restores both.
    let turned = graph.direction.is_horizontal();
    let mut placed = place_container(graph, None, turned);
    if turned {
        transpose(&mut placed);
    }
    match graph.direction {
        Direction::Up => flip_vertically(&mut placed),
        Direction::Left => flip_horizontally(&mut placed),
        _ => {}
    }
    attach_to_real_nodes(graph, &mut placed);
    placed
}

/// Move every edge's two ends onto the nodes it actually names.
///
/// A container lays an edge that reaches into a subgraph out as an edge to the *subgraph*, because
/// that is the thing it placed. Once everything has an absolute position the real node does too, so
/// the end is moved onto it: the arrow points at the box it names rather than at the frame round it,
/// which is what Mermaid draws and what a reader expects. For an edge whose ends were both ordinary
/// nodes this changes nothing, so it is one pass over all of them rather than a special case.
fn attach_to_real_nodes(graph: &Graph, placed: &mut Placed) {
    for (index, edge) in graph.edges.iter().enumerate() {
        let path = &mut placed.edges[index];
        if path.len() < 2 {
            continue;
        }
        let last = path.len() - 1;
        path[0] = placed.nodes[edge.from].centre();
        path[last] = placed.nodes[edge.to].centre();
        placed.labels[index] = midpoint(path);
    }
}

/// One box of the layout: the top level, or the inside of one subgraph.
///
/// Positions are relative to the box's own top left corner. A child group is laid out by calling
/// this again and is then placed inside as a single node, which is what keeps its contents from ever
/// overlapping anything outside it.
fn place_container(graph: &Graph, group: Option<usize>, turned: bool) -> Placed {
    let nodes: Vec<usize> = (0..graph.nodes.len())
        .filter(|&index| graph.nodes[index].group == group)
        .collect();
    let children: Vec<usize> = (0..graph.groups.len())
        .filter(|&index| graph.groups[index].parent == group)
        .collect();

    // Each child group becomes one box, laid out first so that its size is known.
    let inner: Vec<Placed> =
        children.iter().map(|&child| place_container(graph, Some(child), turned)).collect();

    // The things this container places: its own nodes, then its child groups.
    let mut entity_size: Vec<Size> = nodes
        .iter()
        .map(|&index| turn(graph.nodes[index].size, turned))
        .collect();
    for (position, &child) in children.iter().enumerate() {
        let title = turn(graph.groups[child].title, turned);
        entity_size.push(frame_size(inner[position].size, title));
    }

    let owner = ownership(graph, &nodes, &children);
    let lifted = lift_edges(graph, &owner, turned);
    let arranged = arrange(&entity_size, &lifted);

    let mut placed = Placed {
        nodes: vec![Rect::default(); graph.nodes.len()],
        groups: vec![Rect::default(); graph.groups.len()],
        edges: vec![Vec::new(); graph.edges.len()],
        labels: vec![Point::default(); graph.edges.len()],
        size: arranged.size,
    };
    for (position, &index) in nodes.iter().enumerate() {
        placed.nodes[index] = arranged.entities[position];
    }
    for (position, &child) in children.iter().enumerate() {
        let frame = arranged.entities[nodes.len() + position];
        placed.groups[child] = frame;
        let title = turn(graph.groups[child].title, turned);
        let (dx, dy) = (frame.x + GROUP_PADDING, frame.y + GROUP_PADDING + title.height);
        merge(&mut placed, &inner[position], dx, dy);
    }
    for (edge, path) in arranged.paths {
        placed.edges[edge] = path.points;
        placed.labels[edge] = path.label;
    }
    placed
}

/// A size with its two numbers swapped when the diagram has been turned on its side.
fn turn(size: Size, turned: bool) -> Size {
    if turned {
        Size::new(size.height, size.width)
    } else {
        size
    }
}

/// How big a subgraph's frame is round contents of `inner`, with a title of `title`.
fn frame_size(inner: Size, title: Size) -> Size {
    Size::new(
        (inner.width + GROUP_PADDING * 2.0).max(title.width + GROUP_PADDING * 2.0),
        inner.height + GROUP_PADDING * 2.0 + title.height,
    )
}

/// Copy everything `child` placed into `into`, moved by `dx` and `dy`.
fn merge(into: &mut Placed, child: &Placed, dx: f32, dy: f32) {
    for (index, rect) in child.nodes.iter().enumerate() {
        if rect.width > 0.0 || rect.height > 0.0 {
            into.nodes[index] = rect.moved(dx, dy);
        }
    }
    for (index, rect) in child.groups.iter().enumerate() {
        if rect.width > 0.0 || rect.height > 0.0 {
            into.groups[index] = rect.moved(dx, dy);
        }
    }
    for (index, path) in child.edges.iter().enumerate() {
        if !path.is_empty() {
            into.edges[index] =
                path.iter().map(|point| Point::new(point.x + dx, point.y + dy)).collect();
            into.labels[index] = Point::new(child.labels[index].x + dx, child.labels[index].y + dy);
        }
    }
}

/// For every node, which entity of this container it belongs to, or nothing when it is elsewhere.
///
/// A node directly in this container is its own entity. A node inside one of this container's child
/// groups — however deeply — is that child group. That is what lets an edge reaching into a subgraph
/// be laid out here as an edge to the subgraph, and routed later to the real node.
fn ownership(graph: &Graph, nodes: &[usize], children: &[usize]) -> Vec<Option<usize>> {
    let mut owner = vec![None; graph.nodes.len()];
    for (position, &index) in nodes.iter().enumerate() {
        owner[index] = Some(position);
    }
    for (position, &child) in children.iter().enumerate() {
        for index in 0..graph.nodes.len() {
            if inside(graph, graph.nodes[index].group, child) {
                owner[index] = Some(nodes.len() + position);
            }
        }
    }
    owner
}

/// True when `start` is `wanted` or is nested inside it.
fn inside(graph: &Graph, start: Option<usize>, wanted: usize) -> bool {
    let mut at = start;
    // Bounded by the number of groups, so a manifest with a cycle in its nesting cannot loop here.
    for _ in 0..=graph.groups.len() {
        match at {
            Some(index) if index == wanted => return true,
            Some(index) => at = graph.groups[index].parent,
            None => return false,
        }
    }
    false
}

/// One edge as this container sees it: between two of its entities, remembering which edge it was.
#[derive(Debug, Clone, Copy)]
struct Lifted {
    edge: usize,
    from: usize,
    to: usize,
    label: Size,
    span: usize,
}

/// Every edge whose two ends are different entities of this container.
///
/// An edge inside one child group is left for that group's own layout; an edge to somewhere outside
/// this container is left for whichever container holds both ends.
fn lift_edges(graph: &Graph, owner: &[Option<usize>], turned: bool) -> Vec<Lifted> {
    graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(edge, spec)| {
            let from = owner.get(spec.from).copied().flatten()?;
            let to = owner.get(spec.to).copied().flatten()?;
            (from != to).then_some(Lifted {
                edge,
                from,
                to,
                // Turned with the node sizes, so the gap between two ranks holds the label the way
                // round it is really drawn. A left-to-right diagram's labels are laid out along the
                // gap between two columns, and it is their **width** that has to fit there; leaving
                // this untouched left every label on a left-to-right diagram sitting under the box
                // beside it, which is what the state diagram's picture showed.
                label: turn(spec.label, turned),
                span: spec.span.max(1),
            })
        })
        .collect()
}

/// An edge's route, in the container's own coordinates.
struct Path {
    points: Vec<Point>,
    label: Point,
}

/// What one container's own arrangement came to.
struct Arranged {
    entities: Vec<Rect>,
    paths: Vec<(usize, Path)>,
    size: Size,
}

/// Rank, order, place and route one container's entities. Everything below here is plain Sugiyama.
fn arrange(sizes: &[Size], edges: &[Lifted]) -> Arranged {
    if sizes.is_empty() {
        return Arranged { entities: Vec::new(), paths: Vec::new(), size: Size::default() };
    }
    let reversed = back_edges(sizes.len(), edges);
    let ranks = rank(sizes.len(), edges, &reversed);
    let (layers, chains) = insert_dummies(sizes, edges, &reversed, &ranks);
    let joins = build_joins(&chains, edges, &reversed);
    let order = order_layers(&layers, &joins);
    let placed = position(sizes, &order, &joins, edges);
    route(edges, &reversed, &chains, &placed)
}

/// Which edges point backwards, found by a depth-first walk.
///
/// An edge to a node already on the stack closes a cycle. It is reversed for ranking so the graph
/// becomes acyclic, and remembered so that it is still drawn pointing the way it was written.
fn back_edges(count: usize, edges: &[Lifted]) -> Vec<bool> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, edge) in edges.iter().enumerate() {
        out[edge.from].push(index);
    }
    let mut reversed = vec![false; edges.len()];
    // 0 not seen, 1 on the stack, 2 finished.
    let mut state = vec![0_u8; count];
    for start in 0..count {
        if state[start] != 0 {
            continue;
        }
        // An explicit stack rather than recursion: a chain of ten thousand nodes is a legal
        // flowchart and would otherwise be ten thousand stack frames.
        let mut stack = vec![(start, 0_usize)];
        state[start] = 1;
        while let Some((node, position)) = stack.pop() {
            if position < out[node].len() {
                stack.push((node, position + 1));
                let index = out[node][position];
                let next = edges[index].to;
                match state[next] {
                    0 => {
                        state[next] = 1;
                        stack.push((next, 0));
                    }
                    1 => reversed[index] = true,
                    _ => {}
                }
            } else {
                state[node] = 2;
            }
        }
    }
    reversed
}

/// The two ends of an edge, the way the layout should read them.
fn ends(edge: &Lifted, reversed: bool) -> (usize, usize) {
    if reversed {
        (edge.to, edge.from)
    } else {
        (edge.from, edge.to)
    }
}

/// How far down each entity sits, as a whole number of ranks.
///
/// Longest path: a node goes one rank below the lowest thing pointing at it. Walked in topological
/// order, which the cycle removal has made possible.
fn rank(count: usize, edges: &[Lifted], reversed: &[bool]) -> Vec<usize> {
    let mut incoming = vec![0_usize; count];
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    for (index, edge) in edges.iter().enumerate() {
        let (from, to) = ends(edge, reversed[index]);
        incoming[to] += 1;
        out[from].push((to, edge.span.max(1)));
    }
    let mut ranks = vec![0_usize; count];
    let mut ready: Vec<usize> = (0..count).filter(|&node| incoming[node] == 0).collect();
    let mut done = 0;
    while let Some(node) = ready.pop() {
        done += 1;
        for &(next, span) in &out[node] {
            ranks[next] = ranks[next].max(ranks[node] + span);
            incoming[next] -= 1;
            if incoming[next] == 0 {
                ready.push(next);
            }
        }
    }
    // Every node should have come off the queue. If one has not, the cycle removal missed something
    // and the ranks it has are still usable, so the diagram is drawn rather than refused.
    debug_assert!(done == count || count == 0, "the graph should be acyclic by now");
    ranks
}

/// A node in the layered graph: either one of the container's entities, or a dummy on an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Slot {
    Entity(usize),
    /// A point an edge passes through, and which edge it belongs to.
    Dummy(usize),
}

/// The chain of dummies belonging to one edge, from its source's rank downwards.
type Chains = Vec<Vec<usize>>;

/// Split every long edge into a chain of dummies, and hand back the nodes rank by rank.
///
/// The returned `layers[r]` holds every slot at rank `r`. `chains[e]` holds the dummy slot numbers
/// of edge `e`, in rank order, which is what it is routed through afterwards.
fn insert_dummies(
    sizes: &[Size],
    edges: &[Lifted],
    reversed: &[bool],
    ranks: &[usize],
) -> (Vec<Vec<Slot>>, Chains) {
    let depth = ranks.iter().copied().max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<Slot>> = vec![Vec::new(); depth];
    for entity in 0..sizes.len() {
        layers[ranks[entity]].push(Slot::Entity(entity));
    }
    let mut chains: Chains = vec![Vec::new(); edges.len()];
    for (index, edge) in edges.iter().enumerate() {
        let (from, to) = ends(edge, reversed[index]);
        let (top, bottom) = (ranks[from], ranks[to]);
        for level in (top + 1)..bottom {
            layers[level].push(Slot::Dummy(index));
            chains[index].push(level);
        }
    }
    (layers, chains)
}

/// The order of the slots within each rank, after the crossing reduction.
///
/// The starting order is the order things were declared in, which is deterministic and is usually
/// close to what the author meant. It is then swept down and up a fixed number of times, moving each
/// slot to the median position of what it is joined to in the neighbouring rank, and each sweep is
/// kept only if it crosses fewer edges than the best so far.
fn order_layers(layers: &[Vec<Slot>], joins: &Joins) -> Vec<Vec<Slot>> {
    let mut best = layers.to_vec();
    let mut best_crossings = crossings(&best, joins);
    let mut current = best.clone();
    for _ in 0..ORDER_SWEEPS {
        for pass in 0..2 {
            median_pass(&mut current, joins, pass == 0);
            transpose_pass(&mut current, joins);
            let count = crossings(&current, joins);
            if count < best_crossings {
                best_crossings = count;
                best = current.clone();
            }
        }
    }
    best
}

/// For each slot, which slots it is joined to in the rank above and in the rank below.
///
/// Keyed by the slot itself rather than by position, because positions move and this does not.
type Joins = HashMap<SlotKey, (Vec<SlotKey>, Vec<SlotKey>)>;

/// A slot, as something that can go in a map.
type SlotKey = (u8, usize, usize);

fn key(slot: Slot, rank: usize) -> SlotKey {
    match slot {
        Slot::Entity(index) => (0, index, 0),
        Slot::Dummy(edge) => (1, edge, rank),
    }
}

/// Work out what is joined to what, once, so the sweeps do not keep rediscovering it.
///
/// Every edge is a **run** of links: the entity it starts at, its dummies in rank order, then the
/// entity it ends at. Walking the run and joining each link to the next is the whole of it, and it
/// does not matter whether a link is an entity or a dummy. The run is in rank order rather than in
/// drawing order, which is why a reversed edge is read through [`ends`] first: the ordering and the
/// positioning both work down the page, and only the arrowhead cares which way it was written.
fn build_joins(chains: &Chains, edges: &[Lifted], reversed: &[bool]) -> Joins {
    let mut joins: Joins = HashMap::new();
    for (index, edge) in edges.iter().enumerate() {
        let (from, to) = ends(edge, reversed[index]);
        let mut run: Vec<SlotKey> = Vec::with_capacity(chains[index].len() + 2);
        run.push((0, from, 0));
        for &rank in &chains[index] {
            run.push((1, index, rank));
        }
        run.push((0, to, 0));
        for pair in run.windows(2) {
            joins.entry(pair[0]).or_default().1.push(pair[1]);
            joins.entry(pair[1]).or_default().0.push(pair[0]);
        }
    }
    joins
}

/// Move each slot to the median position of what it is joined to in the neighbouring rank.
fn median_pass(layers: &mut [Vec<Slot>], joins: &Joins, downwards: bool) {
    let count = layers.len();
    let order: Vec<usize> = if downwards { (1..count).collect() } else { (0..count - 1).rev().collect() };
    for rank in order {
        let neighbour = if downwards { rank - 1 } else { rank + 1 };
        let positions: HashMap<SlotKey, usize> = layers[neighbour]
            .iter()
            .enumerate()
            .map(|(at, slot)| (key(*slot, neighbour), at))
            .collect();
        let mut scored: Vec<(f32, usize, Slot)> = layers[rank]
            .iter()
            .enumerate()
            .map(|(at, slot)| {
                let median = median_of(key(*slot, rank), joins, &positions, downwards)
                    .unwrap_or(at as f32);
                (median, at, *slot)
            })
            .collect();
        // Ties keep the order they had, which is what makes this deterministic.
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
        layers[rank] = scored.into_iter().map(|(_, _, slot)| slot).collect();
    }
}

/// The median position of everything one slot is joined to in the neighbouring rank.
fn median_of(
    slot: SlotKey,
    joins: &Joins,
    positions: &HashMap<SlotKey, usize>,
    downwards: bool,
) -> Option<f32> {
    let (above, below) = joins.get(&slot)?;
    let neighbours = if downwards { above } else { below };
    let mut found: Vec<usize> =
        neighbours.iter().filter_map(|other| positions.get(other).copied()).collect();
    if found.is_empty() {
        return None;
    }
    found.sort_unstable();
    let middle = found.len() / 2;
    if found.len() % 2 == 1 {
        Some(found[middle] as f32)
    } else {
        Some((found[middle - 1] + found[middle]) as f32 / 2.0)
    }
}

/// Swap neighbouring pairs where doing so crosses fewer edges. One pass, so it always ends.
fn transpose_pass(layers: &mut [Vec<Slot>], joins: &Joins) {
    for rank in 0..layers.len() {
        let mut at = 0;
        while at + 1 < layers[rank].len() {
            let before = crossings_at(layers, joins, rank);
            layers[rank].swap(at, at + 1);
            if crossings_at(layers, joins, rank) >= before {
                layers[rank].swap(at, at + 1);
            }
            at += 1;
        }
    }
}

/// How many edges cross between this rank and the one above it, and the one below it.
fn crossings_at(layers: &[Vec<Slot>], joins: &Joins, rank: usize) -> usize {
    let mut total = 0;
    if rank > 0 {
        total += crossings_between(layers, joins, rank - 1, rank);
    }
    if rank + 1 < layers.len() {
        total += crossings_between(layers, joins, rank, rank + 1);
    }
    total
}

/// How many edges cross in the whole drawing.
fn crossings(layers: &[Vec<Slot>], joins: &Joins) -> usize {
    (1..layers.len()).map(|rank| crossings_between(layers, joins, rank - 1, rank)).sum()
}

/// Count the crossings between two neighbouring ranks, by counting inversions.
fn crossings_between(layers: &[Vec<Slot>], joins: &Joins, upper: usize, lower: usize) -> usize {
    let lower_at: HashMap<SlotKey, usize> = layers[lower]
        .iter()
        .enumerate()
        .map(|(at, slot)| (key(*slot, lower), at))
        .collect();
    let mut ends: Vec<usize> = Vec::new();
    for slot in &layers[upper] {
        let Some((_, below)) = joins.get(&key(*slot, upper)) else {
            continue;
        };
        let mut here: Vec<usize> =
            below.iter().filter_map(|other| lower_at.get(other).copied()).collect();
        here.sort_unstable();
        ends.extend(here);
    }
    let mut total = 0;
    for first in 0..ends.len() {
        for second in first + 1..ends.len() {
            if ends[first] > ends[second] {
                total += 1;
            }
        }
    }
    total
}

/// Where every slot ended up.
struct Positions {
    /// The centre of each slot, by rank and position within the rank.
    centres: Vec<Vec<Point>>,
    layers: Vec<Vec<Slot>>,
    entities: Vec<Rect>,
    size: Size,
}

/// Give every slot a position: down the page by rank, across it by relaxation towards its
/// neighbours.
fn position(sizes: &[Size], order: &[Vec<Slot>], joins: &Joins, edges: &[Lifted]) -> Positions {
    let widths = slot_widths(sizes, order);
    let mut across = initial_across(&widths);
    for sweep in 0..POSITION_SWEEPS {
        relax(&mut across, &widths, joins, order, sweep % 2 == 0);
    }
    normalise(&mut across, &widths);

    let heights = rank_heights(sizes, order);
    let gap = rank_gap(edges);
    let mut centres: Vec<Vec<Point>> = Vec::with_capacity(order.len());
    let mut top = 0.0;
    for (rank, layer) in order.iter().enumerate() {
        let height = heights[rank];
        let row: Vec<Point> = layer
            .iter()
            .enumerate()
            .map(|(at, _)| Point::new(across[rank][at], top + height / 2.0))
            .collect();
        centres.push(row);
        top += height + gap;
    }
    let mut entities = vec![Rect::default(); sizes.len()];
    for (rank, layer) in order.iter().enumerate() {
        for (at, slot) in layer.iter().enumerate() {
            if let Slot::Entity(index) = slot {
                entities[*index] = Rect::around(centres[rank][at], sizes[*index]);
            }
        }
    }
    let size = extent(&entities, &centres);
    Positions { centres, layers: order.to_vec(), entities, size }
}

/// How wide every slot is, rank by rank.
fn slot_widths(sizes: &[Size], order: &[Vec<Slot>]) -> Vec<Vec<f32>> {
    order
        .iter()
        .map(|layer| {
            layer
                .iter()
                .map(|slot| match slot {
                    Slot::Entity(index) => sizes[*index].width,
                    Slot::Dummy(_) => LANE,
                })
                .collect()
        })
        .collect()
}

/// Pack every rank left to right, which is the starting point the relaxation improves on.
fn initial_across(widths: &[Vec<f32>]) -> Vec<Vec<f32>> {
    widths
        .iter()
        .map(|row| {
            let mut left = 0.0;
            row.iter()
                .map(|width| {
                    let centre = left + width / 2.0;
                    left += width + NODE_GAP;
                    centre
                })
                .collect()
        })
        .collect()
}

/// Move each slot towards the median of its neighbours, then push everything apart again.
///
/// The pushing apart is what makes this safe: whatever the medians asked for, no two slots in a rank
/// end up closer than [`NODE_GAP`], so the "no two nodes overlap" property every diagram type is
/// tested for holds by construction rather than by luck.
fn relax(
    across: &mut [Vec<f32>],
    widths: &[Vec<f32>],
    joins: &Joins,
    order: &[Vec<Slot>],
    downwards: bool,
) {
    let count = order.len();
    let ranks: Vec<usize> = if downwards { (0..count).collect() } else { (0..count).rev().collect() };
    for rank in ranks {
        let neighbour = if downwards { rank.checked_sub(1) } else { (rank + 1 < count).then_some(rank + 1) };
        let Some(neighbour) = neighbour else {
            continue;
        };
        let positions: HashMap<SlotKey, f32> = order[neighbour]
            .iter()
            .enumerate()
            .map(|(at, slot)| (key(*slot, neighbour), across[neighbour][at]))
            .collect();
        let wanted: Vec<f32> = order[rank]
            .iter()
            .enumerate()
            .map(|(at, slot)| {
                average_of(key(*slot, rank), joins, &positions, downwards)
                    .unwrap_or(across[rank][at])
            })
            .collect();
        across[rank] = separate(&wanted, &widths[rank]);
    }
}

/// The average position of everything a slot is joined to in the neighbouring rank.
///
/// The average rather than the median, because this one is about where a node should sit rather than
/// about what order it should be in, and a node with two parents belongs between them.
fn average_of(
    slot: SlotKey,
    joins: &Joins,
    positions: &HashMap<SlotKey, f32>,
    downwards: bool,
) -> Option<f32> {
    let (above, below) = joins.get(&slot)?;
    let neighbours = if downwards { above } else { below };
    let found: Vec<f32> =
        neighbours.iter().filter_map(|other| positions.get(other).copied()).collect();
    if found.is_empty() {
        return None;
    }
    Some(found.iter().sum::<f32>() / found.len() as f32)
}

/// Push a rank apart so nothing overlaps, staying as near to `wanted` as the widths allow.
///
/// Twice: once from the left, which guarantees the separation, and once from the right, which takes
/// up the slack the first pass left when the crowding was at the left hand end. Averaging the two
/// and separating once more keeps the result valid and centred.
fn separate(wanted: &[f32], widths: &[f32]) -> Vec<f32> {
    let left = pack(wanted, widths, true);
    let right = pack(wanted, widths, false);
    let middle: Vec<f32> =
        left.iter().zip(&right).map(|(first, second)| (first + second) / 2.0).collect();
    pack(&middle, widths, true)
}

/// One packing pass, from the left or from the right.
fn pack(wanted: &[f32], widths: &[f32], from_left: bool) -> Vec<f32> {
    let mut out = wanted.to_vec();
    if out.is_empty() {
        return out;
    }
    if from_left {
        for at in 1..out.len() {
            let lowest = out[at - 1] + widths[at - 1] / 2.0 + NODE_GAP + widths[at] / 2.0;
            out[at] = out[at].max(lowest);
        }
    } else {
        for at in (0..out.len() - 1).rev() {
            let highest = out[at + 1] - widths[at + 1] / 2.0 - NODE_GAP - widths[at] / 2.0;
            out[at] = out[at].min(highest);
        }
    }
    out
}

/// Slide everything so the leftmost edge is at zero.
///
/// The leftmost **edge**, not the leftmost centre. Sliding by the centre leaves the widest node in
/// the first rank hanging half its width off the left of the diagram, where it is clipped away — a
/// fault the shared "nothing is placed outside the scene" test caught on the first run.
fn normalise(across: &mut [Vec<f32>], widths: &[Vec<f32>]) {
    let smallest = across
        .iter()
        .zip(widths)
        .flat_map(|(row, sizes)| row.iter().zip(sizes).map(|(centre, width)| centre - width / 2.0))
        .fold(f32::INFINITY, f32::min);
    if !smallest.is_finite() {
        return;
    }
    for row in across.iter_mut() {
        for value in row.iter_mut() {
            *value -= smallest;
        }
    }
}

/// How tall each rank is: the tallest thing in it.
fn rank_heights(sizes: &[Size], order: &[Vec<Slot>]) -> Vec<f32> {
    order
        .iter()
        .map(|layer| {
            layer
                .iter()
                .map(|slot| match slot {
                    Slot::Entity(index) => sizes[*index].height,
                    Slot::Dummy(_) => 0.0,
                })
                .fold(0.0_f32, f32::max)
        })
        .collect()
}

/// The gap between two ranks: the usual one, widened to hold the largest edge label.
///
/// One gap for the whole diagram rather than one per rank. A diagram whose rows were different
/// distances apart because one of them happened to carry a two-line label reads as though the
/// spacing meant something, and it does not.
fn rank_gap(edges: &[Lifted]) -> f32 {
    RANK_GAP + edges.iter().map(|edge| edge.label.height).fold(0.0_f32, f32::max)
}

/// How much room everything placed takes up.
fn extent(entities: &[Rect], centres: &[Vec<Point>]) -> Size {
    let mut size = Size::default();
    for rect in entities {
        size.width = size.width.max(rect.right());
        size.height = size.height.max(rect.bottom());
    }
    for row in centres {
        for point in row {
            size.width = size.width.max(point.x);
            size.height = size.height.max(point.y);
        }
    }
    size
}

/// Turn the placed slots into one polyline an edge, and say where its label goes.
fn route(edges: &[Lifted], reversed: &[bool], chains: &Chains, placed: &Positions) -> Arranged {
    let mut where_is: HashMap<SlotKey, Point> = HashMap::new();
    for (rank, layer) in placed.layers.iter().enumerate() {
        for (at, slot) in layer.iter().enumerate() {
            where_is.insert(key(*slot, rank), placed.centres[rank][at]);
        }
    }
    let mut paths = Vec::with_capacity(edges.len());
    for (index, edge) in edges.iter().enumerate() {
        let mut points = Vec::with_capacity(chains[index].len() + 2);
        points.push(placed.entities[edge.from].centre());
        let mut middle: Vec<Point> = chains[index]
            .iter()
            .filter_map(|&rank| where_is.get(&(1, index, rank)).copied())
            .collect();
        // The chain was built from the source's rank downwards, which for a reversed edge is from
        // the target's. Drawing it the way it was written means walking it the other way.
        if reversed[index] {
            middle.reverse();
        }
        points.extend(middle);
        points.push(placed.entities[edge.to].centre());
        let label = midpoint(&points);
        paths.push((edge.edge, Path { points, label }));
    }
    Arranged { entities: placed.entities.clone(), paths, size: placed.size }
}

/// The middle of a polyline, measured along it rather than between its ends.
fn midpoint(points: &[Point]) -> Point {
    if points.len() < 2 {
        return points.first().copied().unwrap_or_default();
    }
    let total: f32 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if walked + length >= total / 2.0 && length > 0.0 {
            return pair[0].towards(pair[1], (total / 2.0 - walked) / length);
        }
        walked += length;
    }
    points[points.len() / 2]
}

/// Reflect the whole layout across the diagonal, turning a downward diagram into a rightward one.
fn transpose(placed: &mut Placed) {
    for rect in placed.nodes.iter_mut().chain(placed.groups.iter_mut()) {
        *rect = Rect::new(rect.y, rect.x, rect.height, rect.width);
    }
    for path in &mut placed.edges {
        for point in path.iter_mut() {
            *point = Point::new(point.y, point.x);
        }
    }
    for label in &mut placed.labels {
        *label = Point::new(label.y, label.x);
    }
    placed.size = Size::new(placed.size.height, placed.size.width);
}

fn flip_vertically(placed: &mut Placed) {
    let height = placed.size.height;
    for rect in placed.nodes.iter_mut().chain(placed.groups.iter_mut()) {
        rect.y = height - rect.bottom();
    }
    for path in &mut placed.edges {
        for point in path.iter_mut() {
            point.y = height - point.y;
        }
    }
    for label in &mut placed.labels {
        label.y = height - label.y;
    }
}

fn flip_horizontally(placed: &mut Placed) {
    let width = placed.size.width;
    for rect in placed.nodes.iter_mut().chain(placed.groups.iter_mut()) {
        rect.x = width - rect.right();
    }
    for path in &mut placed.edges {
        for point in path.iter_mut() {
            point.x = width - point.x;
        }
    }
    for label in &mut placed.labels {
        label.x = width - label.x;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(count: usize) -> Graph {
        let mut graph = Graph::default();
        for _ in 0..count {
            graph.add_node(Size::new(80.0, 40.0), None);
        }
        for index in 1..count {
            graph.edges.push(EdgeSpec::new(index - 1, index));
        }
        graph
    }

    #[test]
    fn a_chain_runs_down_the_page_in_order() {
        let placed = layout(&chain(4));
        for index in 1..4 {
            assert!(
                placed.nodes[index].top() > placed.nodes[index - 1].top(),
                "node {index} should be below node {}",
                index - 1
            );
        }
        assert_eq!(placed.nodes.len(), 4);
        assert_eq!(placed.edges.len(), 3);
    }

    #[test]
    fn left_to_right_runs_across_the_page_and_keeps_the_node_sizes() {
        let mut graph = chain(3);
        graph.direction = Direction::Right;
        let placed = layout(&graph);
        for index in 1..3 {
            assert!(
                placed.nodes[index].left() > placed.nodes[index - 1].left(),
                "node {index} should be to the right of node {}",
                index - 1
            );
        }
        // Turning the layout must not turn the boxes: they are still eighty by forty.
        for rect in &placed.nodes {
            assert_eq!(rect.size(), Size::new(80.0, 40.0), "the node keeps its own shape");
        }
    }

    #[test]
    fn bottom_to_top_is_the_same_layout_upside_down() {
        let mut graph = chain(3);
        graph.direction = Direction::Up;
        let placed = layout(&graph);
        for index in 1..3 {
            assert!(placed.nodes[index].top() < placed.nodes[index - 1].top());
        }
        assert!(placed.nodes.iter().all(|rect| rect.top() >= -0.01), "nothing above the top edge");
    }

    #[test]
    fn two_nodes_in_the_same_rank_never_overlap() {
        // One node pointing at six, which puts all six in one rank.
        let mut graph = Graph::default();
        let root = graph.add_node(Size::new(90.0, 40.0), None);
        for _ in 0..6 {
            let leaf = graph.add_node(Size::new(90.0, 40.0), None);
            graph.edges.push(EdgeSpec::new(root, leaf));
        }
        let placed = layout(&graph);
        for first in 0..placed.nodes.len() {
            for second in first + 1..placed.nodes.len() {
                assert!(
                    !placed.nodes[first].overlaps(&placed.nodes[second]),
                    "{first} and {second} overlap: {:?} {:?}",
                    placed.nodes[first],
                    placed.nodes[second]
                );
            }
        }
    }

    #[test]
    fn a_cycle_is_laid_out_rather_than_looping_for_ever() {
        let mut graph = chain(3);
        graph.edges.push(EdgeSpec::new(2, 0));
        let placed = layout(&graph);
        assert_eq!(placed.nodes.len(), 3);
        // The edge that closes the cycle is still drawn, and still from 2 to 0.
        let back = &placed.edges[2];
        assert!(back.len() >= 2);
        assert_eq!(back[0], placed.nodes[2].centre());
        assert_eq!(back[back.len() - 1], placed.nodes[0].centre());
    }

    #[test]
    fn a_node_pointing_at_itself_is_left_for_the_caller() {
        // A self loop cannot be ranked, so it is not laid out here. The caller draws it as a loop
        // beside the node, which is what Mermaid does too.
        let mut graph = Graph::default();
        let only = graph.add_node(Size::new(60.0, 30.0), None);
        graph.edges.push(EdgeSpec::new(only, only));
        let placed = layout(&graph);
        assert_eq!(placed.nodes.len(), 1);
        assert!(placed.edges[0].is_empty(), "a self loop gets no route");
    }

    #[test]
    fn an_edge_spanning_several_ranks_bends_through_them() {
        // Zero to three directly, past two ranks: the route should have points in between.
        let mut graph = chain(4);
        graph.edges.push(EdgeSpec::new(0, 3));
        let placed = layout(&graph);
        assert!(
            placed.edges[3].len() > 2,
            "the long edge should bend through the ranks it passes, not cut across them"
        );
    }

    #[test]
    fn a_subgraph_is_placed_as_one_box_with_its_members_inside_it() {
        let mut graph = Graph::default();
        graph.groups.push(GroupSpec { title: Size::new(60.0, 18.0), parent: None });
        let outside = graph.add_node(Size::new(80.0, 40.0), None);
        let first = graph.add_node(Size::new(80.0, 40.0), Some(0));
        let second = graph.add_node(Size::new(80.0, 40.0), Some(0));
        graph.edges.push(EdgeSpec::new(first, second));
        graph.edges.push(EdgeSpec::new(outside, first));
        let placed = layout(&graph);
        let frame = placed.groups[0];
        for member in [first, second] {
            let rect = placed.nodes[member];
            assert!(
                rect.left() >= frame.left() - 0.01 && rect.right() <= frame.right() + 0.01,
                "member {member} should be inside the frame across"
            );
            assert!(
                rect.top() >= frame.top() - 0.01 && rect.bottom() <= frame.bottom() + 0.01,
                "member {member} should be inside the frame down"
            );
        }
        assert!(
            !placed.nodes[outside].overlaps(&frame),
            "a node outside the subgraph must not sit on top of it"
        );
    }

    #[test]
    fn an_edge_into_a_subgraph_still_ends_on_the_real_node() {
        let mut graph = Graph::default();
        graph.groups.push(GroupSpec { title: Size::new(40.0, 18.0), parent: None });
        let outside = graph.add_node(Size::new(80.0, 40.0), None);
        let inside = graph.add_node(Size::new(80.0, 40.0), Some(0));
        graph.edges.push(EdgeSpec::new(outside, inside));
        let placed = layout(&graph);
        let path = &placed.edges[0];
        assert_eq!(path[0], placed.nodes[outside].centre());
        assert_eq!(
            path[path.len() - 1],
            placed.nodes[inside].centre(),
            "it points at the node, not at the frame round it"
        );
    }

    #[test]
    fn laying_the_same_graph_out_twice_gives_exactly_the_same_answer() {
        // The whole of the screenshot testing rests on this.
        let mut graph = chain(6);
        graph.edges.push(EdgeSpec::new(0, 4));
        graph.edges.push(EdgeSpec::new(5, 1));
        assert_eq!(layout(&graph), layout(&graph));
    }

    #[test]
    fn nothing_is_placed_above_or_left_of_the_origin() {
        let mut graph = chain(5);
        graph.edges.push(EdgeSpec::new(0, 4));
        graph.edges.push(EdgeSpec::new(3, 1));
        let placed = layout(&graph);
        for rect in &placed.nodes {
            assert!(rect.left() >= -0.01 && rect.top() >= -0.01, "{rect:?} is outside the scene");
        }
    }

    #[test]
    fn an_empty_graph_lays_out_to_nothing_rather_than_panicking() {
        assert_eq!(layout(&Graph::default()), Placed::default());
    }
}
