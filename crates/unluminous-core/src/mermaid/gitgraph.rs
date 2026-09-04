//! Git graphs: `gitGraph`.
//!
//! A lane for each branch, a circle for each commit along it, and lines curving between lanes where
//! a branch is made or merged. Tags hang under the commits that carry them.
//!
//! ## Time runs one way and branches stack the other
//!
//! Every commit gets the next position along the time axis, whichever branch it is on. That is what
//! makes the picture readable: two commits on different branches are never on top of each other, and
//! a merge line always slopes forwards. Mermaid's `parallelCommits` puts commits on different
//! branches at the same depth; Unluminous does not, deliberately, because it makes the picture depend on
//! which branch was written first rather than on what happened.
//!
//! `LR` is the default and runs time across the page; `TB` and `BT` run it down and up. All three
//! are the same arithmetic with the axes swapped at the end, which is the same trick the layered
//! layout uses.

use std::collections::HashMap;

use super::parts;
use super::scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Stroke};
use super::source::{self, Source};
use super::text;
use super::{Options, Problem};

/// How far apart two commits are along the time axis.
const STEP: f32 = 62.0;
/// How far apart two branch lanes are.
const LANE: f32 = 54.0;
/// How big a commit's circle is.
const DOT: f32 = 9.0;

/// What kind of commit it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mark {
    #[default]
    Normal,
    /// `type: REVERSE` — a commit that undoes another, drawn with a cross through it.
    Reverse,
    /// `type: HIGHLIGHT` — drawn as a filled square rather than a circle.
    Highlight,
}

/// One commit.
#[derive(Debug, Clone, PartialEq)]
struct Commit {
    id: String,
    tag: Option<String>,
    mark: Mark,
    branch: usize,
    /// Where it sits along the time axis, counting from zero.
    at: usize,
    /// The commit it follows on its own branch.
    parent: Option<usize>,
    /// The commit it merges in, for a merge commit.
    merged: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Graph {
    branches: Vec<String>,
    commits: Vec<Commit>,
    /// Which way time runs.
    across: bool,
    /// True when time runs up the page rather than down it.
    upwards: bool,
    title: Option<String>,
}

pub fn render(source: &Source, options: &Options) -> Result<Scene, Problem> {
    let graph = read(source)?;
    Ok(draw(&graph, source, options))
}

fn read(source: &Source) -> Result<Graph, Problem> {
    let heading = source.header.to_ascii_uppercase();
    let mut graph = Graph {
        across: !heading.contains("TB") && !heading.contains("BT"),
        upwards: heading.contains("BT"),
        ..Graph::default()
    };
    // `main` exists before anything is written, which is what a commit with no branch belongs to.
    graph.branches.push("main".to_owned());
    let mut current = 0;
    // The tip of each branch, which a new commit's parent is.
    let mut tips: HashMap<usize, usize> = HashMap::new();
    let mut counter = 0;
    let mut clock = 0;
    for line in source.statements() {
        if let Some(rest) = line.after_word("title") {
            graph.title = Some(source::label(rest));
            continue;
        }
        // `mainBranchName`, `showBranches` and the rest are about wording and about what is hidden;
        // Unluminous draws every branch with its name, so they are read and ignored.
        if ["accDescr", "mainBranchName", "mainBranchOrder", "showBranches", "showCommitLabel",
            "parallelCommits", "rotateCommitLabel", "options", "end"]
            .iter()
            .any(|word| line.starts_with_word(word))
        {
            continue;
        }
        if let Some(rest) = line.after_word("branch") {
            let name = branch_name(rest);
            let made_from = tips.get(&current).copied();
            current = branch_of(&mut graph, &name);
            // A new branch starts from wherever the branch it was made on had got to, so its first
            // commit's parent is that commit. Without this the new lane starts in mid air, with
            // nothing joining it to the history it came out of.
            if let Some(tip) = made_from {
                tips.entry(current).or_insert(tip);
            }
            continue;
        }
        if let Some(rest) = line.after_word("checkout").or_else(|| line.after_word("switch")) {
            current = branch_of(&mut graph, &branch_name(rest));
            continue;
        }
        if let Some(rest) = line.after_word("merge") {
            let fields = read_fields(rest);
            let from = branch_of(&mut graph, &branch_name(&fields.first));
            let parent = tips.get(&current).copied();
            let merged = tips.get(&from).copied();
            counter += 1;
            let index = graph.commits.len();
            graph.commits.push(Commit {
                id: fields.id.unwrap_or_else(|| format!("merge {}", fields.first.trim())),
                tag: fields.tag,
                mark: fields.mark,
                branch: current,
                at: clock,
                parent,
                merged,
            });
            clock += 1;
            tips.insert(current, index);
            continue;
        }
        if let Some(rest) = line.after_word("cherry-pick") {
            let fields = read_fields(rest);
            counter += 1;
            let index = graph.commits.len();
            graph.commits.push(Commit {
                id: fields.id.unwrap_or_else(|| format!("pick {counter}")),
                tag: fields.tag,
                mark: fields.mark,
                branch: current,
                at: clock,
                parent: tips.get(&current).copied(),
                merged: None,
            });
            clock += 1;
            tips.insert(current, index);
            continue;
        }
        if let Some(rest) = line.after_word("commit") {
            let fields = read_fields(rest);
            counter += 1;
            let index = graph.commits.len();
            graph.commits.push(Commit {
                id: fields.id.unwrap_or_else(|| format!("{counter}")),
                tag: fields.tag,
                mark: fields.mark,
                branch: current,
                at: clock,
                parent: tips.get(&current).copied(),
                merged: None,
            });
            clock += 1;
            tips.insert(current, index);
            continue;
        }
        return Err(Problem::at(
            line,
            "a git graph is made of `commit`, `branch`, `checkout`, `merge` and `cherry-pick`.",
        ));
    }
    Ok(graph)
}

/// A branch's name, without the quotes and without an `order:` after it.
fn branch_name(rest: &str) -> String {
    let rest = rest.trim();
    let name = match rest.find("order:") {
        Some(at) => &rest[..at],
        None => rest,
    };
    source::unquote(name.trim())
}

fn branch_of(graph: &mut Graph, name: &str) -> usize {
    if let Some(known) = graph.branches.iter().position(|known| known == name) {
        return known;
    }
    graph.branches.push(name.to_owned());
    graph.branches.len() - 1
}

/// The `id:`, `tag:` and `type:` fields a commit or a merge can carry.
#[derive(Debug, Default)]
struct Fields {
    /// Whatever came before the first field, which is a branch name on a `merge`.
    first: String,
    id: Option<String>,
    tag: Option<String>,
    mark: Mark,
}

/// Read `id: "abc" tag: "v1" type: HIGHLIGHT`, in any order, with or without quotes.
fn read_fields(rest: &str) -> Fields {
    let mut fields = Fields::default();
    let mut at = 0;
    let mut first_end = rest.len();
    while at < rest.len() {
        let Some(colon) = rest[at..].find(':').map(|found| at + found) else {
            break;
        };
        let key = rest[..colon].split_whitespace().last().unwrap_or_default().to_ascii_lowercase();
        if !matches!(key.as_str(), "id" | "tag" | "type") {
            at = colon + 1;
            continue;
        }
        first_end = first_end.min(colon - key.len());
        let (value, after) = read_value(&rest[colon + 1..]);
        match key.as_str() {
            "id" => fields.id = Some(value),
            "tag" => fields.tag = Some(value),
            "type" => {
                fields.mark = match value.to_ascii_uppercase().as_str() {
                    "REVERSE" => Mark::Reverse,
                    "HIGHLIGHT" => Mark::Highlight,
                    _ => Mark::Normal,
                }
            }
            _ => {}
        }
        at = colon + 1 + after;
    }
    fields.first = rest[..first_end.min(rest.len())].trim().to_owned();
    fields
}

/// One field's value, quoted or not, and how far along the text it ended.
fn read_value(rest: &str) -> (String, usize) {
    let trimmed = rest.trim_start();
    let skipped = rest.len() - trimmed.len();
    if let Some(inner) = trimmed.strip_prefix('"') {
        if let Some(close) = inner.find('"') {
            return (inner[..close].to_owned(), skipped + close + 2);
        }
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    (trimmed[..end].to_owned(), skipped + end)
}

fn draw(graph: &Graph, source: &Source, options: &Options) -> Scene {
    let mut scene = Scene::new();
    let mut titled = source.clone();
    if titled.title.is_none() {
        titled.title.clone_from(&graph.title);
    }
    if graph.commits.is_empty() {
        parts::title(&mut scene, &titled, options, 0.0);
        parts::finish(&mut scene);
        return scene;
    }
    let name_style = options.style(0.9, true);
    let widest = graph
        .branches
        .iter()
        .map(|name| text::width_of(name, &name_style, options.metrics))
        .fold(0.0_f32, f32::max);
    let steps = graph.commits.iter().map(|commit| commit.at).max().unwrap_or(0) + 1;
    // Far enough apart for the names under them. A fixed step ran two commit names together the
    // moment one of them was more than a word long, which `merge renderers` is.
    let id_style = options.style(0.75, false);
    let widest_id = graph
        .commits
        .iter()
        .map(|commit| text::width_of(&commit.id, &id_style, options.metrics))
        .fold(0.0_f32, f32::max);
    let step = if graph.across { STEP.max(widest_id + 16.0) } else { STEP };

    // The names go down the left when time runs across, and along the top when it runs down.
    let (gutter, size) = if graph.across {
        (widest + 24.0, (parts::MARGIN * 2.0 + widest + 24.0 + step * steps as f32, parts::MARGIN * 2.0 + LANE * graph.branches.len() as f32 + 40.0))
    } else {
        (28.0, (parts::MARGIN * 2.0 + LANE * graph.branches.len() as f32 + widest, parts::MARGIN * 2.0 + 28.0 + step * steps as f32 + 24.0))
    };
    let top = parts::title(&mut scene, &titled, options, size.0);
    let origin = Point::new(parts::MARGIN + if graph.across { gutter } else { 0.0 }, top + parts::MARGIN + if graph.across { 0.0 } else { gutter });

    let place = |commit: &Commit| -> Point {
        let along = step * commit.at as f32 + step / 2.0;
        let lane = LANE * commit.branch as f32 + LANE / 2.0;
        if graph.across {
            Point::new(origin.x + along, origin.y + lane)
        } else if graph.upwards {
            Point::new(origin.x + lane, origin.y + step * steps as f32 - along)
        } else {
            Point::new(origin.x + lane, origin.y + along)
        }
    };

    draw_lanes(&mut scene, graph, origin, steps, step, gutter, top, options);
    draw_links(&mut scene, graph, &place, options);
    draw_commits(&mut scene, graph, &place, options);
    scene.claim(Rect::new(0.0, 0.0, size.0, size.1 + top));
    parts::finish(&mut scene);
    scene
}

/// A faint line down each branch's lane, with its name at the start of it.
#[allow(clippy::too_many_arguments)]
fn draw_lanes(
    scene: &mut Scene,
    graph: &Graph,
    origin: Point,
    steps: usize,
    step: f32,
    gutter: f32,
    top: f32,
    options: &Options,
) {
    let style = parts::text_style(options, 0.9, true, options.theme.dim);
    let measure = options.style(0.9, true);
    for (index, name) in graph.branches.iter().enumerate() {
        let lane = LANE * index as f32 + LANE / 2.0;
        let colour = options.theme.series(index);
        let width = text::width_of(name, &measure, options.metrics);
        let (from, to, at, anchor) = if graph.across {
            (
                Point::new(origin.x, origin.y + lane),
                Point::new(origin.x + step * steps as f32, origin.y + lane),
                Point::new(origin.x - 12.0, origin.y + lane - measure.size * 0.6),
                Anchor::End,
            )
        } else {
            (
                Point::new(origin.x + lane, origin.y),
                Point::new(origin.x + lane, origin.y + step * steps as f32),
                Point::new(origin.x + lane, top + parts::MARGIN),
                Anchor::Middle,
            )
        };
        let _ = gutter;
        scene.add(Item::Line {
            points: vec![from, to],
            stroke: Stroke::new(colour, parts::LINE),
            dash: parts::DASH,
        });
        parts::one_line(scene, name, at, &style, anchor, width);
    }
}

/// The lines joining a commit to its parent and, for a merge, to what it merged.
fn draw_links(
    scene: &mut Scene,
    graph: &Graph,
    place: &impl Fn(&Commit) -> Point,
    options: &Options,
) {
    for commit in &graph.commits {
        let here = place(commit);
        for (other, merged) in
            [(commit.parent, false), (commit.merged, true)].into_iter().filter_map(|(index, m)| index.map(|i| (i, m)))
        {
            let there = place(&graph.commits[other]);
            let colour = options.theme.series(graph.commits[other].branch);
            // A link between two lanes is bent at the middle rather than drawn straight, so several
            // merges into one lane do not all lie on top of each other.
            let points = if (here.x - there.x).abs() > 0.5 && (here.y - there.y).abs() > 0.5 {
                let corner = if graph.across {
                    Point::new(there.x + (here.x - there.x) * 0.6, there.y)
                } else {
                    Point::new(there.x, there.y + (here.y - there.y) * 0.6)
                };
                vec![there, corner, here]
            } else {
                vec![there, here]
            };
            scene.add(Item::Line {
                points,
                stroke: Stroke::new(if merged { options.theme.accent } else { colour }, parts::THICK * 0.7),
                dash: Dash::Solid,
            });
        }
    }
}

/// Each commit's mark, its name, and its tag.
fn draw_commits(
    scene: &mut Scene,
    graph: &Graph,
    place: &impl Fn(&Commit) -> Point,
    options: &Options,
) {
    let id_style = parts::text_style(options, 0.75, false, options.theme.dim);
    let id_measure = options.style(0.75, false);
    let tag_style = parts::text_style(options, 0.75, true, options.theme.text);
    for commit in &graph.commits {
        let at = place(commit);
        let colour = options.theme.series(commit.branch);
        match commit.mark {
            Mark::Highlight => scene.add(Item::Rect {
                rect: Rect::around(at, super::scene::Size::new(DOT * 2.0, DOT * 2.0)),
                radius: 2.0,
                fill: Some(Paint::solid(options.theme.accent)),
                stroke: Some(Stroke::new(options.theme.text, parts::LINE)),
            }),
            _ => scene.add(Item::Circle {
                centre: at,
                radius: DOT,
                fill: Some(Paint::solid(colour)),
                stroke: Some(Stroke::new(options.theme.node_fill.color, parts::LINE)),
            }),
        }
        if commit.mark == Mark::Reverse {
            // A cross through it, which is how Mermaid marks a commit that undoes another.
            for (dx, dy) in [(1.0, 1.0), (1.0, -1.0)] {
                scene.add(Item::Line {
                    points: vec![
                        Point::new(at.x - DOT * 0.7 * dx, at.y - DOT * 0.7 * dy),
                        Point::new(at.x + DOT * 0.7 * dx, at.y + DOT * 0.7 * dy),
                    ],
                    stroke: Stroke::new(options.theme.node_fill.color, parts::LINE),
                    dash: Dash::Solid,
                });
            }
        }
        let width = text::width_of(&commit.id, &id_measure, options.metrics);
        parts::one_line(
            scene,
            &commit.id,
            Point::new(at.x, at.y + DOT + 5.0),
            &id_style,
            Anchor::Middle,
            width,
        );
        let Some(tag) = &commit.tag else {
            continue;
        };
        let tag_width = text::width_of(tag, &options.style(0.75, true), options.metrics);
        let panel = Rect::around(
            Point::new(at.x, at.y - DOT - 12.0),
            super::scene::Size::new(tag_width + 12.0, id_measure.size * 1.6),
        );
        scene.add(Item::Rect {
            rect: panel,
            radius: 3.0,
            fill: Some(Paint::solid(options.theme.node_fill.color)),
            stroke: Some(Stroke::new(options.theme.accent, parts::LINE)),
        });
        parts::one_line(
            scene,
            tag,
            Point::new(panel.centre().x, panel.top() + 2.0),
            &tag_style,
            Anchor::Middle,
            tag_width,
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

    fn read_graph(text: &str) -> Graph {
        read(&Source::read(text).expect("a diagram")).expect("it should read")
    }

    #[test]
    fn commits_go_on_the_branch_that_is_checked_out() {
        let text = "gitGraph\n commit\n branch develop\n commit\n checkout main\n commit\n";
        let graph = read_graph(text);
        assert_eq!(graph.branches, vec!["main", "develop"]);
        let branches: Vec<usize> = graph.commits.iter().map(|commit| commit.branch).collect();
        assert_eq!(branches, vec![0, 1, 0]);
    }

    #[test]
    fn every_commit_gets_its_own_place_along_the_time_axis() {
        // Two commits on different branches must never be on top of each other.
        let graph = read_graph("gitGraph\n commit\n branch a\n commit\n checkout main\n commit\n");
        let times: Vec<usize> = graph.commits.iter().map(|commit| commit.at).collect();
        assert_eq!(times, vec![0, 1, 2]);
    }

    #[test]
    fn a_commits_fields_are_read_in_any_order() {
        let graph = read_graph("gitGraph\n commit id: \"abc\" tag: \"v1.0\" type: HIGHLIGHT\n");
        assert_eq!(graph.commits[0].id, "abc");
        assert_eq!(graph.commits[0].tag.as_deref(), Some("v1.0"));
        assert_eq!(graph.commits[0].mark, Mark::Highlight);

        let other = read_graph("gitGraph\n commit type: REVERSE id: \"xyz\"\n");
        assert_eq!(other.commits[0].mark, Mark::Reverse);
        assert_eq!(other.commits[0].id, "xyz");
    }

    #[test]
    fn a_commit_with_no_id_is_numbered() {
        let graph = read_graph("gitGraph\n commit\n commit\n commit\n");
        let ids: Vec<&str> = graph.commits.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
    }

    #[test]
    fn a_merge_joins_the_tip_of_the_branch_it_names() {
        let text = "gitGraph\n commit\n branch develop\n commit\n checkout main\n merge develop\n";
        let graph = read_graph(text);
        let merge = graph.commits.last().expect("a merge");
        assert_eq!(merge.branch, 0, "the merge commit is on main");
        assert_eq!(merge.merged, Some(1), "it merges develop's tip");
        assert_eq!(merge.parent, Some(0), "and follows main's own tip");
    }

    #[test]
    fn a_branch_order_is_not_part_of_its_name() {
        let graph = read_graph("gitGraph\n branch develop order: 2\n commit\n");
        assert_eq!(graph.branches, vec!["main", "develop"]);
    }

    #[test]
    fn the_direction_is_read_from_the_header() {
        assert!(read_graph("gitGraph\n commit\n").across);
        assert!(read_graph("gitGraph LR:\n commit\n").across);
        assert!(!read_graph("gitGraph TB:\n commit\n").across);
        let up = read_graph("gitGraph BT:\n commit\n");
        assert!(!up.across && up.upwards);
    }

    #[test]
    fn a_word_that_is_not_a_git_operation_says_which_line() {
        let problem = check::refused("gitGraph\n commit\n rebase main\n", &options());
        assert_eq!(problem.line, Some(3));
    }

    #[test]
    fn a_git_graph_is_drawn_and_keeps_every_property() {
        let text = "gitGraph\n\
            commit id: \"start\"\n\
            branch develop\n commit id: \"work\"\n commit id: \"more\" tag: \"v0.9\"\n\
            checkout main\n commit id: \"hotfix\" type: HIGHLIGHT\n\
            merge develop tag: \"v1.0\"\n\
            commit id: \"undo\" type: REVERSE\n";
        let scene = check::drawn(
            text,
            &options(),
            &["main", "develop", "start", "work", "more", "v0.9", "hotfix", "v1.0", "undo"],
        );
        assert!(scene.size.width > scene.size.height, "the default runs across the page");
    }

    #[test]
    fn a_top_to_bottom_graph_is_taller_than_it_is_wide() {
        let text = "gitGraph TB:\n commit\n commit\n branch a\n commit\n commit\n";
        let scene = check::drawn(text, &options(), &["main", "a"]);
        assert!(scene.size.height > scene.size.width);
    }
}

#[cfg(test)]
mod branching {
    use super::*;

    #[test]
    fn a_new_branch_starts_from_where_it_was_made_rather_than_in_mid_air() {
        let text = "gitGraph\n commit\n commit\n branch develop\n commit\n";
        let graph = read(&Source::read(text).expect("a diagram")).expect("it should read");
        let first_on_develop = graph.commits.last().expect("a commit");
        assert_eq!(first_on_develop.branch, 1);
        assert_eq!(
            first_on_develop.parent,
            Some(1),
            "it follows main's second commit, which is where the branch was made"
        );
    }
}
