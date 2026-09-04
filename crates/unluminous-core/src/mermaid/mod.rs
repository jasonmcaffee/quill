//! Mermaid diagrams, read and laid out.
//!
//! `task-1660` asks that a `.mmd` file open as a drawn diagram and that a ` ```mermaid ` block in a
//! Markdown file be drawn in the preview. `tasks/unluminous-mermaid-plugin-tdd.md` records why that is
//! done here, in Rust, rather than by running Mermaid's own JavaScript: the three alternatives each
//! bring a browser, a DOM or a second rendering engine into a text editor, and what a diagram
//! actually needs on top of what Unluminous already has is arithmetic.
//!
//! So this module is the arithmetic. It reads the source, works out where everything goes, and hands
//! back a [`Scene`] — rectangles, circles, polygons, lines and pieces of text at absolute positions.
//! It has no user interface dependency, it measures through [`FontMetrics`] like the editor's layout
//! does, and its tests run with no window and no fonts.
//!
//! ## What is drawn
//!
//! Twenty diagram types. Ten more are **named rather than drawn** — a panel saying which type it is,
//! above the source — because each is either a large grammar serving a narrow audience or is new
//! enough that its syntax is still moving. That distinction is deliberate and is tested: a
//! `c4Diagram` must be named, not quietly mis-parsed as something else and drawn wrongly.
//!
//! ## The bar these are held to
//!
//! Not "identical to `mermaid.js`" — the curves are polylines, the fonts are Unluminous's and the colours
//! are Unluminous's. **Correct and readable**: the right nodes, the right edges in the right direction,
//! the right labels, nothing overlapping, and nothing running off the edge. Those last two are
//! asserted for every diagram type by one shared function, so a type added later inherits the list.
//!
//! ## Nothing is fetched, and nothing is run
//!
//! There is no path through any of this that opens a socket or executes anything a document
//! contains. `click`, `href` and callbacks are read and ignored. That is a property of the design
//! rather than of a setting.

pub mod scene;
pub mod source;
pub mod text;
pub mod theme;

pub mod layered;
pub mod parts;
pub mod shapes;

#[cfg(test)]
pub mod check;

pub mod block;
pub mod class;
pub mod er;
pub mod flowchart;
pub mod gantt;
pub mod gitgraph;
pub mod journey;
pub mod kanban;
pub mod mindmap;
pub mod packet;
pub mod pie;
pub mod quadrant;
pub mod radar;
pub mod requirement;
pub mod sankey;
pub mod sequence;
pub mod state;
pub mod timeline;
pub mod treemap;
pub mod xychart;

use crate::metrics::FontMetrics;
use crate::style::CharStyle;

pub use scene::{Anchor, Dash, Item, Paint, Point, Rect, Scene, Size, Stroke, TextStyle};
pub use source::Source;
pub use theme::Theme;

/// A source with more lines than this is refused rather than laid out.
///
/// Not because the arithmetic would be wrong, but because a preview is worked out again on every
/// keystroke and a diagram nobody could read is not worth stopping the window for. Ten thousand
/// lines is far past any diagram a person writes and far short of anything that takes a noticeable
/// time.
const LINE_LIMIT: usize = 10_000;

/// Which diagram a source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Flowchart,
    Sequence,
    Class,
    State,
    Er,
    Requirement,
    Pie,
    Gantt,
    Journey,
    GitGraph,
    Mindmap,
    Timeline,
    Quadrant,
    XyChart,
    Sankey,
    Block,
    Packet,
    Kanban,
    Radar,
    Treemap,
    /// A Mermaid diagram type this version of Unluminous does not draw. It carries the name to say so
    /// with, which is the whole reason it is a variant rather than an error.
    NotDrawn(&'static str),
}

impl Kind {
    /// What this kind is called when it has to be named to a reader.
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Flowchart => "flowchart",
            Kind::Sequence => "sequence",
            Kind::Class => "class",
            Kind::State => "state",
            Kind::Er => "entity relationship",
            Kind::Requirement => "requirement",
            Kind::Pie => "pie",
            Kind::Gantt => "gantt",
            Kind::Journey => "user journey",
            Kind::GitGraph => "git graph",
            Kind::Mindmap => "mindmap",
            Kind::Timeline => "timeline",
            Kind::Quadrant => "quadrant",
            Kind::XyChart => "xy chart",
            Kind::Sankey => "sankey",
            Kind::Block => "block",
            Kind::Packet => "packet",
            Kind::Kanban => "kanban",
            Kind::Radar => "radar",
            Kind::Treemap => "treemap",
            Kind::NotDrawn(name) => name,
        }
    }

    /// True when Unluminous can draw this one.
    pub fn is_drawn(&self) -> bool {
        !matches!(self, Kind::NotDrawn(_))
    }
}

/// The diagram a keyword names.
///
/// Matched without regard to case. Mermaid's own keywords are camel case, but somebody typing
/// `sequencediagram` means the same thing and there is nothing to be gained by refusing it.
pub fn kind_of(keyword: &str) -> Option<Kind> {
    let word = keyword.trim().to_ascii_lowercase();
    let kind = match word.as_str() {
        "flowchart" | "flowchart-v2" | "graph" => Kind::Flowchart,
        "sequencediagram" => Kind::Sequence,
        "classdiagram" | "classdiagram-v2" => Kind::Class,
        "statediagram" | "statediagram-v2" => Kind::State,
        "erdiagram" => Kind::Er,
        "requirementdiagram" | "requirement" => Kind::Requirement,
        "pie" | "piechart" => Kind::Pie,
        "gantt" => Kind::Gantt,
        "journey" | "userjourney" => Kind::Journey,
        "gitgraph" => Kind::GitGraph,
        "mindmap" => Kind::Mindmap,
        "timeline" => Kind::Timeline,
        "quadrantchart" | "quadrant" => Kind::Quadrant,
        "xychart" | "xychart-beta" => Kind::XyChart,
        "sankey" | "sankey-beta" => Kind::Sankey,
        "block" | "block-beta" => Kind::Block,
        "packet" | "packet-beta" => Kind::Packet,
        "kanban" => Kind::Kanban,
        "radar" | "radar-beta" => Kind::Radar,
        "treemap" | "treemap-beta" => Kind::Treemap,
        // Named rather than drawn. Each is either a grammar of its own serving a narrow audience or
        // is new enough that its syntax is still moving, and naming it is the honest answer.
        "c4diagram" | "c4context" | "c4container" | "c4component" | "c4dynamic"
        | "c4deployment" => Kind::NotDrawn("C4"),
        "zenuml" => Kind::NotDrawn("ZenUML"),
        "architecture" | "architecture-beta" => Kind::NotDrawn("architecture"),
        "swimlanes" | "swimlane" => Kind::NotDrawn("swimlanes"),
        "eventmodeling" => Kind::NotDrawn("event modelling"),
        "venn" | "venn-beta" => Kind::NotDrawn("Venn"),
        "ishikawa" | "fishbone" => Kind::NotDrawn("Ishikawa"),
        "wardley" | "wardley-beta" => Kind::NotDrawn("Wardley"),
        "cynefin" | "cynefin-beta" => Kind::NotDrawn("Cynefin"),
        "treeview" => Kind::NotDrawn("tree view"),
        _ => return None,
    };
    Some(kind)
}

/// Why a diagram could not be drawn.
///
/// Carries the line it went wrong on, and the line itself, because Mermaid's own error box says only
/// that there was a syntax error and knowing *which line* is what a person writing one wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The line in the source, counting from one. `None` when the trouble is with the whole thing.
    pub line: Option<usize>,
    /// The line itself, so the panel can show it without the caller going back to the source.
    pub text: String,
    pub reason: String,
    /// True when this is a diagram type Unluminous does not draw yet, rather than a fault in the source.
    /// The two are shown differently: one is the author's business and the other is Unluminous's.
    pub unsupported: bool,
}

impl Problem {
    /// A fault in the source, at a line.
    pub fn at(line: &source::Line, reason: impl Into<String>) -> Self {
        Self {
            line: Some(line.number),
            text: line.text.clone(),
            reason: reason.into(),
            unsupported: false,
        }
    }

    /// A fault with the whole source rather than with one line of it.
    pub fn whole(reason: impl Into<String>) -> Self {
        Self { line: None, text: String::new(), reason: reason.into(), unsupported: false }
    }

    /// A diagram type this version does not draw.
    pub fn not_drawn(name: &str) -> Self {
        Self {
            line: None,
            text: String::new(),
            reason: format!("Unluminous does not draw a {name} diagram yet."),
            unsupported: true,
        }
    }

    /// The whole message, with the line number in front of it when there is one.
    pub fn message(&self) -> String {
        match self.line {
            Some(number) => format!("Line {number}: {}", self.reason),
            None => self.reason.clone(),
        }
    }
}

/// What a renderer is given: a way to measure, a font to measure in, and the colours.
///
/// There is deliberately **no width**. Every diagram works out its own natural size and whoever
/// draws it fits that to the pane, which is what `services::picture` already does with a picture.
/// Passing a width in would mean a diagram that reflowed as a splitter moved and a screenshot test
/// that depended on the size of the window.
pub struct Options<'a> {
    pub metrics: &'a dyn FontMetrics,
    /// The family and the size ordinary label text is set in. Everything else is worked out from it,
    /// so a diagram follows the editor's font exactly as the Markdown preview does.
    pub base: CharStyle,
    pub theme: Theme,
}

impl<'a> Options<'a> {
    /// The usual options for `metrics`: Unluminous's palette, and a plain fourteen point face.
    pub fn new(metrics: &'a dyn FontMetrics) -> Self {
        Self {
            metrics,
            base: CharStyle { size: 14.0, ..CharStyle::default() },
            theme: Theme::default(),
        }
    }

    /// A style like the base one, at a size and a weight.
    pub fn style(&self, scale: f32, bold: bool) -> CharStyle {
        CharStyle { size: self.base.size * scale, bold, ..self.base.clone() }
    }
}

/// Read `text` and work out where everything in it goes.
///
/// The one way in. Everything else in this module is reached through here, so a caller never has to
/// know which of twenty parsers a source belongs to.
pub fn render(text: &str, options: &Options) -> Result<Scene, Problem> {
    let source = Source::read(text)
        .ok_or_else(|| Problem::whole("There is no diagram here: the first line should name one, such as `flowchart TD`."))?;
    if source.lines.len() > LINE_LIMIT {
        return Err(Problem::whole(format!(
            "This diagram has more than {LINE_LIMIT} lines, which is more than Unluminous draws."
        )));
    }
    let Some(kind) = kind_of(&source.keyword) else {
        return Err(Problem::whole(format!(
            "`{}` does not name a Mermaid diagram. The first line should be something like `flowchart TD` or `sequenceDiagram`.",
            source.keyword
        )));
    };
    match kind {
        Kind::Flowchart => flowchart::render(&source, options),
        Kind::Sequence => sequence::render(&source, options),
        Kind::Class => class::render(&source, options),
        Kind::State => state::render(&source, options),
        Kind::Er => er::render(&source, options),
        Kind::Requirement => requirement::render(&source, options),
        Kind::Pie => pie::render(&source, options),
        Kind::Gantt => gantt::render(&source, options),
        Kind::Journey => journey::render(&source, options),
        Kind::GitGraph => gitgraph::render(&source, options),
        Kind::Mindmap => mindmap::render(&source, options),
        Kind::Timeline => timeline::render(&source, options),
        Kind::Quadrant => quadrant::render(&source, options),
        Kind::XyChart => xychart::render(&source, options),
        Kind::Sankey => sankey::render(&source, options),
        Kind::Block => block::render(&source, options),
        Kind::Packet => packet::render(&source, options),
        Kind::Kanban => kanban::render(&source, options),
        Kind::Radar => radar::render(&source, options),
        Kind::Treemap => treemap::render(&source, options),
        Kind::NotDrawn(name) => Err(Problem::not_drawn(name)),
    }
}

/// Which diagram `text` holds, without laying any of it out.
///
/// Cheap: it reads as far as the first real line and stops. The window asks this to decide whether a
/// fence is a diagram at all before it goes to the trouble of drawing one.
pub fn kind(text: &str) -> Option<Kind> {
    Source::read(text).and_then(|source| kind_of(&source.keyword))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FixedMetrics;

    #[test]
    fn every_keyword_mermaid_ships_is_recognised() {
        // The whole list from Mermaid's own navigation, so a type that is added to Unluminous later shows
        // up here as a change from NotDrawn rather than as a new line.
        let all = [
            "flowchart", "graph", "sequenceDiagram", "classDiagram", "stateDiagram",
            "stateDiagram-v2", "erDiagram", "journey", "gantt", "pie", "quadrantChart",
            "requirementDiagram", "gitGraph", "c4Diagram", "mindmap", "timeline", "zenuml",
            "sankey-beta", "xychart-beta", "block-beta", "packet-beta", "kanban", "architecture",
            "radar-beta", "eventModeling", "treemap", "venn", "ishikawa", "wardley", "cynefin",
            "treeView", "swimlanes",
        ];
        for keyword in all {
            assert!(kind_of(keyword).is_some(), "{keyword} should be recognised");
        }
    }

    #[test]
    fn the_ten_unluminous_does_not_draw_are_named_rather_than_mistaken_for_something_else() {
        for keyword in [
            "c4Diagram", "zenuml", "architecture", "swimlanes", "eventModeling", "venn",
            "ishikawa", "wardley", "cynefin", "treeView",
        ] {
            let kind = kind_of(keyword).expect("recognised");
            assert!(!kind.is_drawn(), "{keyword} is not drawn");
            assert!(!kind.name().is_empty(), "{keyword} still has a name to say");
        }
    }

    #[test]
    fn a_keyword_that_is_not_mermaid_at_all_is_nothing() {
        assert_eq!(kind_of("fn"), None);
        assert_eq!(kind_of("# heading"), None);
    }

    #[test]
    fn the_case_of_a_keyword_does_not_matter() {
        assert_eq!(kind_of("SequenceDiagram"), Some(Kind::Sequence));
        assert_eq!(kind_of("sequencediagram"), Some(Kind::Sequence));
        assert_eq!(kind_of("FlowChart"), Some(Kind::Flowchart));
    }

    #[test]
    fn a_source_with_no_diagram_in_it_says_so_rather_than_drawing_nothing() {
        let metrics = FixedMetrics::default();
        let problem = render("", &Options::new(&metrics)).expect_err("nothing to draw");
        assert!(problem.reason.contains("no diagram"));
        assert!(!problem.unsupported, "an empty source is not an unsupported type");
    }

    #[test]
    fn a_type_unluminous_does_not_draw_is_a_problem_that_names_it() {
        let metrics = FixedMetrics::default();
        let problem = render("wardley\nvalue chain\n", &Options::new(&metrics)).expect_err("not drawn");
        assert!(problem.unsupported, "this is Unluminous's limitation, not the author's fault");
        assert!(problem.reason.contains("Wardley"), "it says which type: {}", problem.reason);
    }

    #[test]
    fn an_unrecognised_first_word_is_told_what_one_looks_like() {
        let metrics = FixedMetrics::default();
        let problem = render("banana split\n", &Options::new(&metrics)).expect_err("not a diagram");
        assert!(problem.reason.contains("banana"), "it quotes what was written");
        assert!(problem.reason.contains("flowchart TD"), "it says what one looks like");
        assert!(!problem.unsupported);
    }

    #[test]
    fn a_problem_says_which_line_went_wrong() {
        let line = source::Line { number: 7, text: "A ~~> B".to_owned(), indent: 0 };
        let problem = Problem::at(&line, "that is not a link");
        assert_eq!(problem.message(), "Line 7: that is not a link");
        assert_eq!(problem.text, "A ~~> B");
    }
}

/// One sample of every diagram type Unluminous draws, and the words each one has to end up saying.
///
/// This is the list the cross-cutting tests below walk, and it is deliberately a constant in the
/// module rather than a local in one test: adding a diagram type means adding a line here, and every
/// property is then asserted for it without anybody having to remember to do so.
#[cfg(test)]
pub const SAMPLES: &[(&str, &str, &[&str])] = &[
    (
        "flowchart",
        "flowchart TD\n Start([Begin]) --> Check{Ready?}\n Check -->|yes| Ship[Ship it]\n Check -->|no| Fix[Fix it]\n Fix --> Check\n",
        &["Begin", "Ready?", "Ship it", "yes"],
    ),
    (
        "sequence",
        "sequenceDiagram\n actor Alice\n participant Bob\n Alice ->>+ Bob: Ask\n Bob -->>- Alice: Answer\n Note over Alice,Bob: done\n",
        &["Alice", "Bob", "Ask", "Answer"],
    ),
    (
        "class",
        "classDiagram\n class Animal {\n <<abstract>>\n +String name\n +move()\n }\n Animal <|-- Dog\n Animal <|-- Cat\n",
        &["Animal", "Dog", "Cat", "+String name"],
    ),
    (
        "state",
        "stateDiagram-v2\n [*] --> Idle\n Idle --> Running : start\n Running --> [*]\n",
        &["Idle", "Running", "start"],
    ),
    (
        "er",
        "erDiagram\n CUSTOMER ||--o{ ORDER : places\n ORDER {\n int number PK\n date placed\n }\n",
        &["CUSTOMER", "ORDER", "places", "number"],
    ),
    (
        "requirement",
        "requirementDiagram\n requirement top {\n id: 1\n text: it works\n }\n element test {\n type: simulation\n }\n test - verifies -> top\n",
        &["top", "test", "verifies"],
    ),
    ("pie", "pie title Pets\n \"Dogs\" : 386\n \"Cats\" : 85\n", &["Pets", "Dogs", "Cats"]),
    (
        "gantt",
        "gantt\n title Plan\n dateFormat YYYY-MM-DD\n section Design\n Sketch : d1, 2024-01-01, 5d\n Review : after d1, 3d\n",
        &["Plan", "Sketch", "Review"],
    ),
    (
        "journey",
        "journey\n title My day\n section Morning\n Wake up: 3: Me\n Make tea: 5: Me, Cat\n",
        &["My day", "Wake up", "Make tea", "Cat"],
    ),
    (
        "gitGraph",
        "gitGraph\n commit id: \"one\"\n branch develop\n commit id: \"two\"\n checkout main\n merge develop tag: \"v1\"\n",
        &["main", "develop", "one", "two", "v1"],
    ),
    (
        "mindmap",
        "mindmap\nroot((Unluminous))\n  Editing\n    Undo\n  Panes\n    Terminal\n",
        &["Unluminous", "Editing", "Undo", "Panes", "Terminal"],
    ),
    (
        "timeline",
        "timeline\n title History\n section Early\n 2002 : LinkedIn\n 2004 : Facebook : Google\n",
        &["History", "Early", "2002", "LinkedIn", "Google"],
    ),
    (
        "quadrantChart",
        "quadrantChart\n title Reach\n x-axis Low --> High\n y-axis Few --> Many\n quadrant-1 Expand\n A: [0.3, 0.6]\n",
        &["Reach", "Expand", "A"],
    ),
    (
        "xychart",
        "xychart-beta\n title Sales\n x-axis [jan, feb, mar]\n y-axis \"Revenue\" 0 --> 100\n bar [30, 45, 60]\n line [20, 35, 50]\n",
        &["Sales", "Revenue", "jan", "mar"],
    ),
    (
        "sankey",
        "sankey-beta\n Coal,Electricity,25\n Gas,Electricity,15\n Electricity,Homes,30\n",
        &["Coal", "Gas", "Electricity", "Homes"],
    ),
    (
        "block",
        "block-beta\n columns 3\n a[\"Front\"]:3\n block:services\n  api[\"API\"]\n end\n space\n db[(\"Store\")]\n a --> db\n",
        &["Front", "API", "Store"],
    ),
    (
        "packet",
        "packet-beta\n title TCP\n 0-15: \"Source Port\"\n 16-31: \"Destination Port\"\n 32-63: \"Sequence Number\"\n",
        &["TCP", "Source Port", "Sequence Number"],
    ),
    (
        "kanban",
        "kanban\n todo[Todo]\n  a[Write it]@{ assigned: \"Jason\", priority: \"High\" }\n doing[Doing]\n  b[Test it]\n",
        &["Todo", "Doing", "Write it", "Jason", "Test it"],
    ),
    (
        "radar",
        "radar-beta\n title Scores\n axis Speed, Accuracy, Quality\n curve A[\"Team A\"]{4, 3, 5}\n max 5\n",
        &["Scores", "Speed", "Quality", "Team A"],
    ),
    (
        "treemap",
        "treemap-beta\n\"Editing\"\n  \"Undo\": 30\n  \"Search\": 12\n\"Panes\": 22\n",
        &["Editing", "Undo", "Panes"],
    ),
];

#[cfg(test)]
mod every_type {
    use super::*;
    use crate::metrics::FixedMetrics;

    fn options() -> Options<'static> {
        Options::new(Box::leak(Box::new(FixedMetrics::default())))
    }

    #[test]
    fn there_is_a_sample_for_every_type_unluminous_draws() {
        // The guard on the whole of the rest of this module: a diagram type added to `kind_of`
        // without a sample here would otherwise be tested by nothing at all.
        let drawn: Vec<&'static str> = [
            "flowchart", "sequenceDiagram", "classDiagram", "stateDiagram-v2", "erDiagram",
            "requirementDiagram", "pie", "gantt", "journey", "gitGraph", "mindmap", "timeline",
            "quadrantChart", "xychart-beta", "sankey-beta", "block-beta", "packet-beta", "kanban",
            "radar-beta", "treemap-beta",
        ]
        .into_iter()
        .filter(|keyword| kind_of(keyword).is_some_and(|kind| kind.is_drawn()))
        .collect();
        assert_eq!(drawn.len(), 20, "twenty types are drawn");
        assert_eq!(SAMPLES.len(), 20, "and every one of them has a sample");
    }

    #[test]
    fn every_diagram_type_keeps_every_property() {
        for (name, source, wanted) in SAMPLES {
            let scene = match render(source, &options()) {
                Ok(scene) => scene,
                Err(problem) => panic!("{name} should draw: {}", problem.message()),
            };
            check::properties(&scene, wanted);
            assert!(
                scene.size.width > 0.0 && scene.size.height > 0.0,
                "{name} produced a scene with no size"
            );
            assert!(!scene.items.is_empty(), "{name} drew nothing");
        }
    }

    #[test]
    fn every_diagram_type_lays_out_the_same_way_twice() {
        // Everything the screenshot tests do rests on this one.
        for (name, source, _) in SAMPLES {
            let first = render(source, &options());
            let second = render(source, &options());
            assert!(first == second, "{name} gave two different pictures for one source");
        }
    }

    #[test]
    fn every_diagram_type_survives_being_cut_off_part_way_through() {
        // Which is what a preview sees on nearly every keystroke while one is being typed. Any of
        // these may refuse to draw; none of them may panic, and none may hand back a scene with a
        // number in it that is not a number.
        for (name, source, _) in SAMPLES {
            for length in 0..source.len() {
                if !source.is_char_boundary(length) {
                    continue;
                }
                if let Ok(scene) = render(&source[..length], &options()) {
                    assert!(
                        scene.size.width.is_finite() && scene.size.height.is_finite(),
                        "{name} cut off at {length} gave a size that is not a number"
                    );
                }
            }
            // And a line at a time, which is what a paste looks like.
            let lines: Vec<&str> = source.lines().collect();
            for count in 0..lines.len() {
                let _ = render(&lines[..count].join("\n"), &options());
            }
        }
    }

    #[test]
    fn a_type_unluminous_does_not_draw_is_named_rather_than_mis_parsed() {
        for keyword in [
            "c4Diagram", "zenuml", "architecture", "swimlanes", "eventModeling", "venn",
            "ishikawa", "wardley", "cynefin", "treeView",
        ] {
            let source = format!("{keyword}\n  something\n  something else\n");
            let problem = render(&source, &options()).expect_err("it should not be drawn");
            assert!(problem.unsupported, "{keyword} is Unluminous's limitation, not the author's fault");
            assert!(
                problem.reason.contains("does not draw"),
                "{keyword} should say so plainly: {}",
                problem.reason
            );
        }
    }
}
